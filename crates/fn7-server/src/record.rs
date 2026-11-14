//! In-process recording in the Theatre core's format (no server): [`Recorded`] wraps a game and records what the core
//! records for a match (`run_start`, `ruleset`, async `hook`s with input/output, `llm_call`s through
//! [`theatre_server::recording::TracedLlm`], plugin `event`s, `snapshot`s with the core's labels, `run_end`), as JSON
//! lines in `<dir>/<run>.jsonl` through the engine's own `JsonlSink` and `RunRec`. One toolchain (replay,
//! `fn7-project`, the admin API's readers) then reads recordings from `fn7-sim` and from the real server alike.
//!
//! Hook inputs, snapshot labels and rounds follow `theatre-server`'s room (1741d8f; `resolve_round`'s input carries its
//! `RoundContext` as `ctx`): `on_match_start` (round 0),
//! `on_round_start` (full), `resolve_round` (full; also after a fallback), `commit_round`. Like the core, the sync hooks
//! (`validate_action`, `on_action_begin`, `on_instant`, `query`, `fallback_outcome`) run inside the run's trace context
//! (round, a fresh hook id, `actor = "seat:N"` where a seat acted), so the plugin's events there land in the run;
//! `on_action_begin` / `on_instant` / `query` are recorded at `full`. A fallback is recorded at every level as a
//! `fallback_outcome` hook (input: round, actions, ctx, resolve_error) plus a `fallback_outcome` snapshot.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use theatre_plugin_api::*;
use theatre_server::recording::{Recorder, RunRec, StateOut, TracedLlm};
use theatre_trace::{JsonlSink, Level};

/// A recorder writing `<dir>/<run>.jsonl` at `level`.
pub fn recorder(dir: impl AsRef<Path>, level: Level) -> anyhow::Result<Recorder> {
    let dir = dir.as_ref().to_path_buf();
    Ok(Recorder {
        sink: JsonlSink::new(&dir)?,
        level,
        dir,
    })
}

/// The LLM as the core hands it to a recorded match: every call becomes an `llm_call` record of its hook.
pub fn traced(llm: Arc<dyn LlmProvider>) -> Arc<dyn LlmProvider> {
    Arc::new(TracedLlm { inner: llm })
}

/// A game recorded like the core records a match (see the module doc). One instance records one match.
pub struct Recorded<P> {
    inner: P,
    rec: RunRec,
    /// The round the core would be in (`m.round`): 0 until the first `on_round_start`.
    round: std::sync::atomic::AtomicU32,
    /// The last `resolve_round` error (`{e:#}`), the `resolve_error` of a fallback that follows it.
    resolve_error: std::sync::Mutex<Option<String>>,
}

impl<P: GamePlugin> Recorded<P> {
    pub fn new(inner: P, rec: RunRec) -> Self {
        Self {
            inner,
            rec,
            round: Default::default(),
            resolve_error: Default::default(),
        }
    }

    /// The run id (the file is `<dir>/<run>.jsonl`).
    pub fn run(&self) -> &str {
        self.rec.run()
    }

    fn round(&self) -> u32 {
        self.round.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[async_trait]
impl<P: GamePlugin> GamePlugin for Recorded<P> {
    fn info(&self) -> PluginInfo {
        self.inner.info()
    }
    fn validate_settings(&self, settings: &Value) -> Result<Value, String> {
        self.inner.validate_settings(settings)
    }
    fn queries(&self) -> Vec<QuerySpec> {
        self.inner.queries()
    }
    fn query(&self, state: &Value, seat: Option<Seat>, name: &str, args: &Value) -> Result<Value, String> {
        let round = Some(self.round());
        let (res, id) = self.rec.within_sync(round, seat, || self.inner.query(state, seat, name, args));
        let out = match &res {
            Ok(v) => v.clone(),
            Err(e) => json!({ "error": e }),
        };
        self.rec.sync_hook(id, round, "query", seat, json!({ "name": name, "args": args }), out);
        res
    }
    async fn on_match_start(
        &self,
        seats: &[SeatInfo],
        settings: &Value,
        llm: &dyn LlmProvider,
    ) -> anyhow::Result<Value> {
        let info = self.inner.info();
        let seats_meta: Vec<Value> = seats
            .iter()
            .map(|s| json!({ "seat": s.seat, "player_id": Value::Null, "display_name": s.display_name, "is_bot": s.is_bot }))
            .collect();
        self.rec.run_start(
            info.id,
            settings,
            json!({ "match_id": self.rec.run(), "seats": seats_meta, "source": "fn7-sim",
                    "server_version": Value::Null }),
        );
        self.rec
            .ruleset(info.id, info.version, info.rules_version, settings);
        let input = json!({ "seats": seats, "settings": settings });
        let state = self
            .rec
            .clone()
            .hook(Some(0), "on_match_start", None, input, StateOut::Whole, self.inner.on_match_start(seats, settings, llm))
            .await?;
        self.rec.snapshot(Some(0), "on_match_start", &state);
        Ok(state)
    }
    async fn on_round_start(
        &self,
        state: &Value,
        round: u32,
        llm: &dyn LlmProvider,
    ) -> anyhow::Result<RoundStart> {
        self.round.store(round, std::sync::atomic::Ordering::Relaxed);
        let r = self
            .rec
            .clone()
            .hook(Some(round), "on_round_start", None, json!({ "round": round }), StateOut::Field, self.inner.on_round_start(state, round, llm))
            .await?;
        if self.rec.full() {
            self.rec.snapshot(Some(round), "on_round_start", &r.state);
        }
        Ok(r)
    }
    async fn brief(&self, state: &Value, round: u32, llm: &dyn LlmProvider) -> anyhow::Result<Brief> {
        self.rec
            .clone()
            .hook(Some(round), "brief", None, json!({ "round": round }), StateOut::None, self.inner.brief(state, round, llm))
            .await
    }
    fn validate_action(
        &self,
        state: &Value,
        seat: Seat,
        text: &str,
        fields: &Value,
        kind: SubmitKind,
    ) -> Result<(), String> {
        self.rec
            .within_sync(Some(self.round()), Some(seat), || self.inner.validate_action(state, seat, text, fields, kind))
            .0
    }
    fn on_seat_takeover(&self, state: &Value, takeover: &SeatTakeover) -> Option<Value> {
        // As the core's room (`complete_joins`): in the trace context (round, the seat), a hook record at every level
        // (input {takeover}, output {state_changed}), and an `on_seat_takeover` snapshot when the state changed.
        let round = Some(takeover.round);
        let (res, id) = self.rec.within_sync(round, Some(takeover.seat), || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.inner.on_seat_takeover(state, takeover)))
        });
        let new_state = res.unwrap_or(None);
        self.rec.hook_record(
            id,
            round,
            "on_seat_takeover",
            Some(takeover.seat),
            json!({ "takeover": takeover }),
            json!({ "state_changed": new_state.is_some() }),
            None,
        );
        if let Some(st) = &new_state {
            self.rec.snapshot(round, "on_seat_takeover", st);
        }
        new_state
    }
    fn default_action(&self, state: &Value, seat: Seat) -> Action {
        self.inner.default_action(state, seat)
    }
    fn on_action_begin(
        &self,
        state: &Value,
        round: u32,
        seat: Seat,
        window: Option<&WindowInfo>,
        next_window_id: u64,
    ) -> PhaseControl {
        let (c, id) = self.rec.within_sync(Some(round), Some(seat), || {
            self.inner.on_action_begin(state, round, seat, window, next_window_id)
        });
        self.rec.sync_hook(
            id,
            Some(round),
            "on_action_begin",
            Some(seat),
            json!({ "window": window }),
            serde_json::to_value(&c).unwrap_or(Value::Null),
        );
        c
    }
    fn on_instant(&self, state: &Value, seat: Seat, action: &Action) -> Option<Value> {
        let round = Some(self.round());
        let (v, id) = self.rec.within_sync(round, Some(seat), || self.inner.on_instant(state, seat, action));
        self.rec.sync_hook(id, round, "on_instant", Some(seat), json!({ "action": action }), json!(v));
        v
    }
    async fn bot_action(&self, state: &Value, seat: Seat, llm: &dyn LlmProvider) -> anyhow::Result<Action> {
        self.rec
            .clone()
            .hook(Some(self.round()), "bot_action", Some(seat), json!({ "seat": seat }), StateOut::None, self.inner.bot_action(state, seat, llm))
            .await
    }
    async fn resolve_round(
        &self,
        state: &Value,
        round: u32,
        actions: &[Action],
        ctx: &RoundContext,
        llm: &dyn LlmProvider,
        progress: &dyn Progress,
    ) -> anyhow::Result<RoundOutcome> {
        let input = json!({ "round": round, "actions": actions, "ctx": ctx });
        let res = self
            .rec
            .clone()
            .hook(
                Some(round),
                "resolve_round",
                None,
                input,
                StateOut::Field,
                self.inner.resolve_round(state, round, actions, ctx, llm, progress),
            )
            .await;
        if let Err(e) = &res {
            *self.resolve_error.lock().expect("resolve error") = Some(format!("{e:#}"));
        }
        let o = res?;
        if self.rec.full() {
            self.rec.snapshot(Some(round), "resolve_round", &o.state);
        }
        Ok(o)
    }
    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action], ctx: &RoundContext) -> RoundOutcome {
        // As the core's room (`on_resolved`): run in the trace context, record a `fallback_outcome` hook at every level
        // (state in its snapshot), then the round's `resolve_round` snapshot at full (`apply_outcome`).
        let (res, id) = self.rec.within_sync(Some(round), None, || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.inner.fallback_outcome(state, round, actions, ctx)
            }))
        });
        let output = match &res {
            Ok(o) => {
                let mut v = serde_json::to_value(o).unwrap_or(Value::Null);
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("state".into(), json!("<see snapshot>"));
                }
                v
            }
            Err(_) => Value::Null,
        };
        let resolve_error = self.resolve_error.lock().expect("resolve error").take().unwrap_or_default();
        self.rec.hook_record(
            id,
            Some(round),
            "fallback_outcome",
            None,
            json!({ "round": round, "actions": actions, "ctx": ctx, "resolve_error": resolve_error }),
            output,
            res.is_err().then(|| "fallback_outcome panicked".to_string()),
        );
        let o = res.unwrap_or_else(|p| std::panic::resume_unwind(p));
        self.rec.snapshot(Some(round), "fallback_outcome", &o.state);
        if self.rec.full() {
            self.rec.snapshot(Some(round), "resolve_round", &o.state);
        }
        o
    }
    fn default_choice(&self, state: &Value, seat: Seat, offered: &[Choice]) -> String {
        self.inner.default_choice(state, seat, offered)
    }
    async fn commit_round(
        &self,
        state: &Value,
        round: u32,
        picked: &[Picked],
        llm: &dyn LlmProvider,
    ) -> anyhow::Result<Committed> {
        let input = json!({ "round": round, "picked": picked });
        let c = self
            .rec
            .clone()
            .hook(Some(round), "commit_round", None, input, StateOut::Field, self.inner.commit_round(state, round, picked, llm))
            .await?;
        self.rec.snapshot(Some(round), "commit_round", &c.state);
        Ok(c)
    }
    fn is_match_over(&self, state: &Value, round: u32) -> bool {
        self.inner.is_match_over(state, round)
    }
    fn public_view(&self, state: &Value) -> Value {
        self.inner.public_view(state)
    }
    fn seat_view(&self, state: &Value, seat: Seat) -> Value {
        self.inner.seat_view(state, seat)
    }
    async fn summarize(&self, state: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Summary> {
        let s = self
            .rec
            .clone()
            .hook(Some(self.round()), "summarize", None, json!({}), StateOut::None, self.inner.summarize(state, llm))
            .await?;
        self.rec.run_end(json!({ "winners": s.winners, "summary": s.summary, "rounds": self.round() }));
        Ok(s)
    }
}
