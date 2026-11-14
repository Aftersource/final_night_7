//! In-process recording in the Theatre core's format (no server): [`Recorded`] wraps a game and records what the core
//! records for a match (`run_start`, `ruleset`, async `hook`s with input/output, `llm_call`s through
//! [`theatre_server::recording::TracedLlm`], plugin `event`s, `snapshot`s with the core's labels, `run_end`), as JSON
//! lines in `<dir>/<run>.jsonl` through the engine's own `JsonlSink` and `RunRec`. One toolchain (replay,
//! `fn7-project`, the admin API's readers) then reads recordings from `fn7-sim` and from the real server alike.
//!
//! Hook inputs, snapshot labels and rounds follow `theatre-server`'s room (faa7220; `resolve_round`'s input carries its
//! `RoundContext` as `ctx`): `on_match_start` (round 0),
//! `on_round_start` (full), `resolve_round` (full; also after a fallback), `commit_round`. The sync hooks are recorded
//! at `full` like the core does.

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
}

impl<P: GamePlugin> Recorded<P> {
    pub fn new(inner: P, rec: RunRec) -> Self {
        Self {
            inner,
            rec,
            round: Default::default(),
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
        let res = self.inner.query(state, seat, name, args);
        let out = match &res {
            Ok(v) => v.clone(),
            Err(e) => json!({ "error": e }),
        };
        self.rec
            .sync_hook(Some(self.round()), "query", seat, json!({ "name": name, "args": args }), out);
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
        self.inner.validate_action(state, seat, text, fields, kind)
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
        let c = self.inner.on_action_begin(state, round, seat, window, next_window_id);
        self.rec.sync_hook(
            Some(round),
            "on_action_begin",
            Some(seat),
            json!({ "window": window }),
            serde_json::to_value(&c).unwrap_or(Value::Null),
        );
        c
    }
    fn on_instant(&self, state: &Value, seat: Seat, action: &Action) -> Option<Value> {
        let v = self.inner.on_instant(state, seat, action);
        self.rec
            .sync_hook(None, "on_instant", Some(seat), json!({ "action": action }), json!(v));
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
        let o = self
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
            .await?;
        if self.rec.full() {
            self.rec.snapshot(Some(round), "resolve_round", &o.state);
        }
        Ok(o)
    }
    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action]) -> RoundOutcome {
        // The core doesn't record fallback_outcome as a hook; its state is snapshotted as the round's resolve.
        let o = self.inner.fallback_outcome(state, round, actions);
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
