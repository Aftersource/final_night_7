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

/// A recorded match with mid-match joins (a bot's seat taken over, two new seats binding the built-in AI demigods, and
/// a fourth with none left, who spectates) replays with zero differences, through `on_seat_takeover` from its hook
/// records; without those records the replay diverges.
#[tokio::test(flavor = "multi_thread")]
async fn replay_is_identical_through_a_midmatch_join() {
    let dir = temp_dir("join");
    let rec = recorder(&dir, theatre_trace::Level::Full).unwrap();
    let run = uuid::Uuid::new_v4().to_string();
    let p = Recorded::new(FinalNight7Plugin::new(), rec.run(run.clone()));
    let llm = traced(Arc::new(Scripted::default()));
    let seats: Vec<SeatInfo> = seats().into_iter().take(2).collect();
    let t = |seat: Seat, round: u32, new_seat: bool| SeatTakeover {
        seat,
        display_name: format!("Joiner {seat}"),
        round,
        new_seat,
        previous_name: (!new_seat).then(|| "Brix".to_string()),
    };
    let joins = move |round: u32| match round {
        3 => vec![t(1, 3, false)],
        4 => vec![t(2, 4, true), t(3, 4, true)],
        5 => vec![t(4, 5, true)],
        _ => vec![],
    };
    let script = |seat: Seat, round: u32| {
        (seat >= 1 && round >= 4 && round.is_multiple_of(2))
            .then(|| fn7_plugin::actions::action(seat, "raise an army from the nearest crowd", "pray to Chance for luck", None))
    };
    let settings = json!({ "seed": 21, "players": 4, "beats_total": 10, "clusters": 60 });
    fn7_plugin::driver::run_match_joins(&p, &seats, &settings, llm.as_ref(), None, &script,
        &|_| (vec![], RoundContext::default()), &joins, |_, _| true)
        .await
        .unwrap();
    rec.flush();
    let path = dir.join(format!("{run}.jsonl"));
    let ls = lines(&path);
    let hooks: Vec<&Value> = of(&ls, "hook").filter(|h| h["hook"] == "on_seat_takeover").collect();
    assert_eq!(hooks.len(), 4);
    assert_eq!(hooks.iter().filter(|h| h["output"]["state_changed"] == true).count(), 3, "the spectator changes nothing");
    assert_eq!(of(&ls, "snapshot").filter(|s| s["label"] == "on_seat_takeover").count(), 3);
    let ev = |k: &str| of(&ls, "event").filter(|e| e["kind"] == k).count();
    assert_eq!((ev("seat_takeover"), ev("seat_takeover_spectator")), (3, 1));

    let rep = fn7_server::replay::verify(&path, |l| eprintln!("{l}")).await.unwrap();
    assert!(rep.differences.is_empty(), "{:?}", rep.differences);
    assert_eq!(rep.takeovers, 4);
    assert!(rep.snapshots > 20);
    assert_eq!(rep.identical, rep.snapshots);
    assert_eq!(rep.summary_identical, Some(true));
    assert_eq!(rep.llm_misses, 0);

    // The takeovers matter: a copy without their hook records no longer replays.
    let cut = dir.join("cut.jsonl");
    let text: String = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter(|l| !l.contains("\"on_seat_takeover\""))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&cut, text).unwrap();
    let rep = fn7_server::replay::verify(&cut, |_| {}).await.unwrap();
    assert_eq!(rep.takeovers, 0);
    assert!(!rep.differences.is_empty(), "a replay that skips the joins must diverge");
    let _ = std::fs::remove_dir_all(dir);
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
    fn on_seat_takeover(&self, state: &Value, t: &SeatTakeover) -> Option<Value> {
        self.inner.on_seat_takeover(state, t)
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

/// Time travel (docs/TIME_TRAVEL.md, c): one recorded match with summonings, challenges, duels that last across beats
/// and end in knockouts, a mid-match join and a fallen player's will, driven hook by hook as the core drives it
/// (instant actions validated and shown on arrival, then delivered to the next resolve). The recording holds every
/// instant action with its arrival stamp (`on_instant` and the resolve's input), a duel's timeline can be rebuilt from
/// it to the millisecond, and replay reproduces every snapshot.
#[tokio::test(flavor = "multi_thread")]
async fn duels_summons_a_will_and_a_join_replay_with_zero_differences() {
    use fn7_model::{DemigodId, Ref};
    let dir = temp_dir("timetravel");
    let rec = recorder(&dir, theatre_trace::Level::Full).unwrap();
    let run = uuid::Uuid::new_v4().to_string();
    let p = Recorded::new(FinalNight7Plugin::new(), rec.run(run.clone()));
    let llm = traced(Arc::new(Failing));
    let llm = llm.as_ref();
    let mut seats: Vec<SeatInfo> = [("Ash", false), ("Brix", false), ("Cyra", true), ("Dune", true)]
        .iter().enumerate().map(|(i, (n, bot))| SeatInfo { seat: i as u8, display_name: n.to_string(), is_bot: *bot }).collect();
    let settings = p.validate_settings(&json!({ "seed": 77, "beats_total": 80, "llm": false, "duel_ai_chance": 0.0,
        "dm_encounters_per_beat": 0, "duel_damage_scale": 0.8, "duel_counter_ms": 60000, "summon_loss_scale": 0.0 })).unwrap();
    let mut state = p.on_match_start(&seats, &settings, llm).await.unwrap();
    let t0 = theatre_trace::now_ms();
    let says = ["I kick your knee into the mud then smash your jaw", "I grab your wrist and slam your ribs on the stone wall",
                 "I sweep your ankle by the river and stomp your shin", "I hook your chin then jab your nose in the rain",
                 "I headbutt your temple and whip your ear with wind", "I punch your gut then kick your hip into the dust",
                 "I smash your elbow on the rock and twist your arm", "I stab at your shoulder and slash your thigh in the fog"];
    let (mut seq, mut said, mut out, mut sent, mut will_round, mut locks) = (0u64, 0usize, Vec::<u8>::new(), Vec::<(u64, i64)>::new(), None::<u32>, 0u32);
    let mut poked: Vec<String> = vec![];
    let mut round = 0u32;
    while round < 60 {
        round += 1;
        if round == 4 {
            // The core completes a mid-match join at the boundary: a human takes over the bot Cyra's seat.
            let t = SeatTakeover { seat: 2, display_name: "Joan".into(), round, new_seat: false, previous_name: Some("Cyra".into()) };
            state = p.on_seat_takeover(&state, &t).expect("the bot's seat is taken over");
            seats[2] = SeatInfo { seat: 2, display_name: "Joan".into(), is_bot: false };
        }
        let start = p.on_round_start(&state, round, llm).await.unwrap();
        state = start.state.clone();
        out.extend(start.eliminated.iter().map(|e| e.0));
        if p.is_match_over(&state, round - 1) {
            break;
        }
        let mut actions = vec![];
        for s in seats.iter().filter(|s| !out.contains(&s.seat)) {
            actions.push(if s.is_bot { p.bot_action(&state, s.seat, llm).await.unwrap() } else { p.default_action(&state, s.seat) });
        }
        // An instant action as the core takes it: validate, stamp, show (`on_instant`), queue for the resolve.
        let mut k = 0i64;
        let mut instant = |state: &Value, seat: u8, fields: Value, seq: &mut u64, queue: &mut Vec<Action>, sent: &mut Vec<(u64, i64)>| -> Option<Value> {
            p.validate_action(state, seat, "", &fields, SubmitKind::Instant).ok()?;
            *seq += 1;
            k += 1;
            let a = Action { fields, instant: true, seq: *seq, received_at_ms: t0 + round as i64 * 3000 + k * 700, ..Action::new(seat, "") };
            let ev = p.on_instant(state, seat, &a);
            sent.push((a.seq, a.received_at_ms));
            queue.push(a);
            ev
        };
        let w = fn7_plugin::world::load(&state).unwrap();
        let mine: Option<DemigodId> = w.bodies_of_seat(0).find(|d| d.alive).map(|d| d.id);
        let foe: Option<DemigodId> = w.bodies_of_seat(1).find(|d| d.alive).map(|d| d.id);
        let mut ctx = RoundContext::default();
        let mut queue = vec![];
        match (mine, foe) {
            (Some(me), Some(foe)) => {
                let open = w.dm.duels.first().and_then(|d| d["duel"]["id"].as_str()).map(str::to_string);
                let id = match open {
                    Some(id) => Some(id),
                    None => {
                        let (a, b) = (w.demigods[me.ix()].pos, w.demigods[foe.ix()].pos);
                        if (a[0] - b[0]).abs() + (a[1] - b[1]).abs() > 0.5 {
                            let near = json!([b[0] + 0.004, b[1]]);
                            instant(&state, 0, json!({ "summon": { "demigod": Ref::Demigod(me), "target": near } }), &mut seq, &mut queue, &mut sent);
                        }
                        let lock = instant(&state, 0, json!({ "challenge": { "demigod": Ref::Demigod(me), "target": Ref::Demigod(foe), "level": "people" } }),
                                           &mut seq, &mut queue, &mut sent);
                        if lock.is_some() {
                            locks += 1;
                        }
                        lock.and_then(|l| l["duel_id"].as_str().map(str::to_string))
                    }
                };
                if let Some(id) = id {
                    // Brix answers once (so nobody counters for it), then Ash types.
                    if !poked.contains(&id) {
                        poked.push(id.clone());
                        instant(&state, 1, json!({ "duel": { "duel_id": id, "text": "hm" } }), &mut seq, &mut queue, &mut sent);
                    }
                    for _ in 0..2 {
                        said += 1;
                        instant(&state, 0, json!({ "duel": { "duel_id": id, "text": says[said % says.len()] } }), &mut seq, &mut queue, &mut sent);
                    }
                }
            }
            (_, None) if will_round.is_none() => {
                // Brix has fallen. The text it typed in a slow window opened while it still stood arrives now.
                seq += 1;
                let at = t0 + round as i64 * 3000 + 900;
                let mut a = fn7_plugin::actions::action(1, "my guys come back to life!", "", None);
                (a.windowed, a.window_id, a.seq, a.received_at_ms) = (true, Some(9), seq, at);
                queue.push(a);
                ctx.windows_closed.push(ClosedWindow {
                    info: WindowInfo { window_id: 9, by_seat: 1, opened_ms: at - 2500, deadline_ms: at + 20_000, engaged: vec![1] },
                    closed_ms: at, reason: "submitted".into(), by_seat: Some(1), submitted: vec![1], dropped: vec![],
                });
                will_round = Some(round);
            }
            _ => {}
        }
        let _ = &mut instant;
        actions.extend(queue);
        let outcome = p.resolve_round(&state, round, &actions, &ctx, llm, &NoProgress).await.unwrap();
        let committed = p.commit_round(&outcome.state, round, &[], llm).await.unwrap();
        state = committed.state.clone();
        out.extend(committed.eliminated.iter().map(|e| e.0));
        if will_round.is_some_and(|r| round > r) || mine.is_none() {
            break;
        }
    }
    p.summarize(&state, llm).await.unwrap();
    rec.flush();
    assert!(will_round.is_some(), "Brix fell within {round} beats ({locks} duels)");
    let path = dir.join(format!("{run}.jsonl"));
    let ls = lines(&path);
    let ev = |k: &str| of(&ls, "event").filter(|e| e["kind"] == k).count();
    assert!(locks >= 2 && ev("duel_lock") as u32 == locks, "two demigods, a duel each: {locks} locks, {} recorded", ev("duel_lock"));
    assert!(ev("summon_streak") >= 1 && ev("fallen_will") == 1 && ev("seat_takeover") == 1, "summons {}, wills {}, joins {}",
            ev("summon_streak"), ev("fallen_will"), ev("seat_takeover"));
    // Every instant action is in the recording twice with its arrival stamp: as it arrived, and as it was resolved.
    let arrived: Vec<(u64, i64)> = of(&ls, "hook").filter(|h| h["hook"] == "on_instant")
        .map(|h| (h["input"]["action"]["seq"].as_u64().unwrap(), h["input"]["action"]["received_at_ms"].as_i64().unwrap())).collect();
    assert_eq!(arrived, sent, "on_instant: every instant action with its arrival stamp, in order");
    let resolved: Vec<(u64, i64)> = of(&ls, "hook").filter(|h| h["hook"] == "resolve_round")
        .flat_map(|h| h["input"]["actions"].as_array().cloned().unwrap_or_default())
        .filter(|a| a["instant"] == true).map(|a| (a["seq"].as_u64().unwrap(), a["received_at_ms"].as_i64().unwrap())).collect();
    assert_eq!(resolved, sent, "resolve_round: the same actions, the same stamps");
    // A duel's timeline to the millisecond: the typed blows a state holds are exactly the recorded lines' stamps.
    let snap = of(&ls, "snapshot").filter(|s| s["label"] == "commit_round" && s["state"]["dm"]["duels"][0]["blows"].is_array()).last()
        .expect("a commit with an open duel");
    let bout = &snap["state"]["dm"]["duels"][0];
    let id = bout["duel"]["id"].as_str().unwrap();
    let typed: Vec<(i64, String)> = of(&ls, "hook").filter(|h| h["hook"] == "on_instant" && h["input"]["action"]["fields"]["duel"]["duel_id"] == id)
        .map(|h| (h["input"]["action"]["received_at_ms"].as_i64().unwrap(), h["input"]["action"]["fields"]["duel"]["text"].as_str().unwrap().to_string()))
        .filter(|(at, _)| *at <= snap["state"]["dm"]["mark"]["at_ms"].as_i64().unwrap()).collect();
    let kept: Vec<(i64, String)> = bout["blows"].as_array().unwrap().iter()
        .map(|b| (b["at_ms"].as_i64().unwrap(), b["text"].as_str().unwrap().to_string())).collect();
    assert!(!kept.is_empty());
    assert_eq!(kept, typed, "the duel's timeline rebuilt from the recording's instant actions");
    assert_eq!(bout["duel"]["started_ms"].as_i64(), of(&ls, "hook").find(|h| h["hook"] == "on_instant" && h["output"]["duel_id"] == id
        && h["output"]["event"] == "challenge.lock").and_then(|h| h["input"]["action"]["received_at_ms"].as_i64()), "its lock's stamp");
    let rep = fn7_server::replay::verify(&path, |l| eprintln!("{l}")).await.unwrap();
    eprintln!("time travel replay: {} snapshots over {} beats, {} identical, {} joins, {} duels, differences {:?}",
              rep.snapshots, rep.beats, rep.identical, rep.takeovers, locks, rep.differences);
    assert!(rep.differences.is_empty(), "{:?}", rep.differences);
    assert_eq!((rep.takeovers, rep.identical, rep.summary_identical), (1, rep.snapshots, Some(true)));
    let _ = std::fs::remove_dir_all(dir);
}
