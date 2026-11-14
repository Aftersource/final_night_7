//! Recorded matches in the Theatre core's JSONL format (fn7-sim's recorder, temp dir, offline): every DATA_MODEL §3
//! event kind appears, LLM steps are annotated, the core's snapshot labels and ruleset are there, and replay
//! verification (the engine's `ReplayLlm`) reproduces every snapshot byte for byte.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fn7_plugin::driver::run_match;
use fn7_plugin::testkit::{Failing, Scripted};
use fn7_plugin::FinalNight7Plugin;
use fn7_server::record::{recorder, traced, Recorded};
use serde_json::{json, Value};
use theatre_plugin_api::*;

/// Every event kind of DATA_MODEL §3.
const KINDS: &[&str] = &[
    "utterance",
    "intent",
    "mention_resolved",
    "memorized",
    "agent_spawned",
    "agent_step",
    "agent_ended",
    "ruling",
    "proposal",
    "resolution",
    "ledger_row",
    "spread",
    "debt_due",
    "debt_delayed",
    "debt_collected",
    "rewind",
    "memory_write",
    "memory_forget",
    "memory_fold",
    "hand_said",
    "zoom",
    "beat",
    "elimination",
];

/// Every LLM step of DATA_MODEL §3 the plugin makes (`route` has no mind: hands route themselves).
const STEPS: &[&str] = &[
    "hand",
    "subagent",
    "possession",
    "god",
    "paladin",
    "resolver",
    "materialize",
    "fold",
    "narrator",
];

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("fn7-{name}-{}", uuid::Uuid::new_v4()))
}

fn seats() -> Vec<SeatInfo> {
    ["Ash", "Brix", "Cyra", "Dune"]
        .iter()
        .enumerate()
        .map(|(i, n)| SeatInfo {
            seat: i as u8,
            display_name: n.to_string(),
            is_bot: true,
        })
        .collect()
}

/// Record one match into `dir` at full level; returns the recording's path.
async fn record(dir: &Path, settings: Value, llm: Arc<dyn LlmProvider>) -> PathBuf {
    let rec = recorder(dir, theatre_trace::Level::Full).unwrap();
    let run = uuid::Uuid::new_v4().to_string();
    let p = Recorded::new(FinalNight7Plugin::new(), rec.run(run.clone()));
    let llm = traced(llm);
    let none = |_: Seat, _: u32| None;
    run_match(&p, &seats(), &settings, llm.as_ref(), None, &none, |_, _| true)
        .await
        .unwrap();
    rec.flush();
    dir.join(format!("{run}.jsonl"))
}

fn lines(path: &Path) -> Vec<Value> {
    fn7_server::replay::read_records(path).unwrap()
}

fn of<'a>(ls: &'a [Value], ty: &str) -> impl Iterator<Item = &'a Value> + 'a {
    let ty = ty.to_string();
    ls.iter().filter(move |l| l["type"] == ty.as_str())
}

#[tokio::test(flavor = "multi_thread")]
async fn every_event_kind_and_step_is_recorded() {
    let dir = temp_dir("events");
    let path = record(
        &dir,
        json!({"seed": 4, "beats_total": 30, "clusters": 80, "memory_per_owner": 12}),
        Arc::new(Scripted::default()),
    )
    .await;
    let ls = lines(&path);
    let events: Vec<&Value> = of(&ls, "event").collect();
    let kind = |k: &str| events.iter().filter(|e| e["kind"] == k).count();
    let missing: Vec<&str> = KINDS.iter().copied().filter(|k| kind(k) == 0).collect();
    assert!(missing.is_empty(), "event kinds never emitted: {missing:?}");
    let steps_missing: Vec<&str> = STEPS
        .iter()
        .copied()
        .filter(|s| !of(&ls, "llm_call").any(|c| c["step"] == *s))
        .collect();
    assert!(steps_missing.is_empty(), "LLM steps never annotated: {steps_missing:?}");
    // Payloads the projection reads.
    let has = |k: &str, f: &dyn Fn(&Value) -> bool| events.iter().any(|e| e["kind"] == k && f(&e["data"]));
    assert!(has("ledger_row", &|d| !d["effect"]["fx"].is_null() && !d["row"]["id"].is_null()));
    assert!(has("memorized", &|d| !d["character"]["groups"].is_null()));
    assert!(has("elimination", &|d| !d["chain"].is_null() && !d["demigod"].is_null()));
    assert!(
        has("beat", &|d| d["record"]["timing"]["ms"].as_array().map(Vec::len) == Some(7)),
        "stage timings in the beat event"
    );
    assert!(events
        .iter()
        .any(|e| e["kind"].as_str().unwrap_or("").starts_with("debt_") && !e["data"]["debt"].is_null()));
    // Round = beat.
    assert!(events
        .iter()
        .filter(|e| e["kind"] == "beat")
        .all(|e| e["ctx"]["round"] == e["data"]["beat"]));
    // The core's ruleset (version, rules_version as label) plus the plugin's rich one; every core snapshot label.
    assert!(of(&ls, "ruleset").any(|r| r["label"] == fn7_plugin::RULES_VERSION && r["version"] == "0.1.0"));
    assert!(of(&ls, "ruleset").any(|r| r["body"]["mechanics"].is_array()));
    for l in ["on_match_start", "on_round_start", "resolve_round", "commit_round"] {
        assert!(of(&ls, "snapshot").any(|s| s["label"] == l), "{l}");
    }
    assert_eq!(of(&ls, "run_start").count(), 1);
    assert_eq!(of(&ls, "run_end").count(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_reproduces_every_snapshot() {
    for (name, llm) in [
        ("scripted", Arc::new(Scripted::default()) as Arc<dyn LlmProvider>),
        ("failing", Arc::new(Failing)),
    ] {
        let dir = temp_dir(&format!("replay-{name}"));
        let path = record(&dir, json!({"seed": 12, "beats_total": 15, "clusters": 60}), llm).await;
        let rep = fn7_server::replay::verify(&path, |l| eprintln!("{l}")).await.unwrap();
        eprintln!(
            "{name}: {} snapshots, {} identical, hits {} misses {}",
            rep.snapshots, rep.identical, rep.llm_hits, rep.llm_misses
        );
        assert!(rep.snapshots > 10);
        assert!(rep.differences.is_empty(), "{:?}", rep.differences);
        assert_eq!(rep.identical, rep.snapshots);
        assert_eq!(rep.summary_identical, Some(true));
        if name == "scripted" {
            assert!(rep.llm_hits > 0);
            assert_eq!(rep.llm_misses, 0, "every request was answered from the recording");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The game with `resolve_round` failing on some beats (to drive the fallback path in-process).
struct FailResolve<P> {
    inner: P,
    fail: Vec<u32>,
}

#[async_trait::async_trait]
impl<P: GamePlugin> GamePlugin for FailResolve<P> {
    fn info(&self) -> PluginInfo {
        self.inner.info()
    }
    fn validate_settings(&self, s: &Value) -> Result<Value, String> {
        self.inner.validate_settings(s)
    }
    fn queries(&self) -> Vec<QuerySpec> {
        self.inner.queries()
    }
    fn query(&self, state: &Value, seat: Option<Seat>, name: &str, args: &Value) -> Result<Value, String> {
        self.inner.query(state, seat, name, args)
    }
    async fn on_match_start(&self, seats: &[SeatInfo], s: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Value> {
        self.inner.on_match_start(seats, s, llm).await
    }
    async fn on_round_start(&self, state: &Value, round: u32, llm: &dyn LlmProvider) -> anyhow::Result<RoundStart> {
        self.inner.on_round_start(state, round, llm).await
    }
    async fn brief(&self, state: &Value, round: u32, llm: &dyn LlmProvider) -> anyhow::Result<Brief> {
        self.inner.brief(state, round, llm).await
    }
    fn validate_action(&self, state: &Value, seat: Seat, text: &str, f: &Value, k: SubmitKind) -> Result<(), String> {
        self.inner.validate_action(state, seat, text, f, k)
    }
    fn default_action(&self, state: &Value, seat: Seat) -> Action {
        self.inner.default_action(state, seat)
    }
    fn on_action_begin(&self, state: &Value, round: u32, seat: Seat, w: Option<&WindowInfo>, next: u64) -> PhaseControl {
        self.inner.on_action_begin(state, round, seat, w, next)
    }
    fn on_instant(&self, state: &Value, seat: Seat, a: &Action) -> Option<Value> {
        self.inner.on_instant(state, seat, a)
    }
    async fn bot_action(&self, state: &Value, seat: Seat, llm: &dyn LlmProvider) -> anyhow::Result<Action> {
        self.inner.bot_action(state, seat, llm).await
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
        if self.fail.contains(&round) {
            anyhow::bail!("forced resolve failure (beat {round})");
        }
        self.inner.resolve_round(state, round, actions, ctx, llm, progress).await
    }
    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action], ctx: &RoundContext) -> RoundOutcome {
        self.inner.fallback_outcome(state, round, actions, ctx)
    }
    fn default_choice(&self, state: &Value, seat: Seat, offered: &[Choice]) -> String {
        self.inner.default_choice(state, seat, offered)
    }
    async fn commit_round(&self, state: &Value, round: u32, p: &[Picked], llm: &dyn LlmProvider) -> anyhow::Result<Committed> {
        self.inner.commit_round(state, round, p, llm).await
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
        self.inner.summarize(state, llm).await
    }
}

fn hook_id_of(l: &Value) -> Option<i64> {
    l["hook_id"].as_i64().or(l["ctx"]["hook_id"].as_i64())
}

fn actor_of(l: &Value) -> Option<&str> {
    l["ctx"]["actor"].as_str()
}

/// A fallback in fn7-sim is recorded as the core records it (theatre 1741d8f): a `fallback_outcome` hook (input: round,
/// actions, ctx, resolve_error) whose plugin events land under it, a `fallback_outcome` snapshot (plus `resolve_round`
/// at full); replay calls `fallback_outcome` with that input and reproduces every snapshot.
#[tokio::test(flavor = "multi_thread")]
async fn fallback_is_recorded_and_replayed() {
    for level in [theatre_trace::Level::Full, theatre_trace::Level::Events] {
        let dir = temp_dir("fallback");
        let rec = recorder(&dir, level).unwrap();
        let run = uuid::Uuid::new_v4().to_string();
        let p = Recorded::new(
            FailResolve { inner: FinalNight7Plugin::new(), fail: vec![2, 4] },
            rec.run(run.clone()),
        );
        let llm = traced(Arc::new(Scripted::default()));
        let none = |_: Seat, _: u32| None;
        // Beat 2's resolve is handed a window seat 0 opened that timed out empty: the fallback must still bill it.
        let extras = |round: u32| {
            let ctx = if round == 2 {
                RoundContext {
                    windows_closed: vec![ClosedWindow {
                        info: WindowInfo { window_id: 1, by_seat: 0, opened_ms: 1_000, deadline_ms: 6_000, engaged: vec![0] },
                        closed_ms: 6_000,
                        reason: "deadline".into(),
                        by_seat: None,
                        submitted: vec![],
                        dropped: vec![],
                    }],
                }
            } else {
                RoundContext::default()
            };
            (vec![], ctx)
        };
        fn7_plugin::driver::run_match_with(
            &p,
            &seats(),
            &json!({"seed": 21, "beats_total": 6, "clusters": 60}),
            llm.as_ref(),
            None,
            &none,
            &extras,
            |_, _| true,
        )
        .await
        .unwrap();
        rec.flush();
        let path = dir.join(format!("{run}.jsonl"));
        let ls = lines(&path);
        let fb: Vec<&Value> = of(&ls, "hook").filter(|h| h["hook"] == "fallback_outcome").collect();
        assert_eq!(fb.iter().map(|h| h["round"].as_u64().unwrap()).collect::<Vec<_>>(), vec![2, 4]);
        let f2 = fb[0];
        assert_eq!(f2["input"]["round"], 2);
        assert_eq!(f2["input"]["ctx"]["windows_closed"][0]["reason"], "deadline");
        assert!(f2["input"]["actions"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(f2["input"]["resolve_error"].as_str().unwrap().contains("forced resolve failure (beat 2)"));
        assert_eq!(f2["output"]["state"], "<see snapshot>");
        assert!(f2["error"].is_null());
        assert_eq!(of(&ls, "snapshot").filter(|s| s["label"] == "fallback_outcome").count(), 2);
        // The fallback's plugin events (Time's bill for the window from ctx among them) land under its hook.
        let under: Vec<&str> = of(&ls, "event")
            .filter(|e| hook_id_of(e) == f2["id"].as_i64())
            .filter_map(|e| e["kind"].as_str())
            .collect();
        assert!(under.contains(&"slow_closed"), "events under the fallback hook: {under:?}");
        let rep = fn7_server::replay::verify(&path, |l| eprintln!("{l}")).await.unwrap();
        assert!(rep.differences.is_empty(), "{:?}", rep.differences);
        assert_eq!(rep.fallbacks, 2);
        assert_eq!(rep.identical, rep.snapshots);
        assert_eq!(rep.summary_identical, Some(true));
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Events the plugin emits in the sync hooks land in the run under the hook's record (round, `actor = "seat:N"`),
/// with no plugin-side re-entry (theatre 1741d8f runs sync hooks in the match's context; `Recorded` does the same).
#[tokio::test(flavor = "multi_thread")]
async fn sync_hook_events_land_in_the_run() {
    let dir = temp_dir("sync");
    let rec = recorder(&dir, theatre_trace::Level::Full).unwrap();
    let run = uuid::Uuid::new_v4().to_string();
    let p = Recorded::new(FinalNight7Plugin::new(), rec.run(run.clone()));
    let settings = p.validate_settings(&json!({"seed": 5, "clusters": 60})).unwrap();
    let st = p.on_match_start(&seats(), &settings, &Failing).await.unwrap();
    let rs = p.on_round_start(&st, 1, &Failing).await.unwrap();
    let c = p.on_action_begin(&rs.state, 1, 0, None, 1);
    assert!(c.window.is_some());
    let cast = Action {
        instant: true,
        seq: 4,
        received_at_ms: 1_004,
        fields: json!({"bolt": "Ash"}),
        ..Action::new(1, "")
    };
    p.validate_action(&rs.state, 1, &cast.text, &cast.fields, SubmitKind::Instant).unwrap();
    assert!(p.on_instant(&rs.state, 1, &cast).is_some());
    rec.flush();
    let ls = lines(&dir.join(format!("{run}.jsonl")));
    for (hook, kind, seat) in [("on_action_begin", "slow_begin", 0), ("on_instant", "cast_streak", 1)] {
        let h = of(&ls, "hook").find(|h| h["hook"] == hook).unwrap_or_else(|| panic!("no {hook} record"));
        assert_eq!((h["round"].as_u64(), h["seat"].as_u64()), (Some(1), Some(seat)));
        let e = of(&ls, "event").find(|e| e["kind"] == kind).unwrap_or_else(|| panic!("no {kind} event"));
        assert_eq!(hook_id_of(e), h["id"].as_i64(), "{kind} is under its {hook} record");
        assert_eq!(e["ctx"]["run"].as_str().or(e["run"].as_str()), Some(run.as_str()));
        assert_eq!(e["ctx"]["round"].as_u64().or(e["round"].as_u64()), Some(1));
        assert_eq!(actor_of(e), Some(format!("seat:{seat}").as_str()), "{e}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
