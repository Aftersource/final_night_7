//! Guardrail (docs/ENGINE_SYNC.md): the wrappers around the game (`MediaPlugin`, which the server runs, and fn7-sim's
//! core-format recorder `Recorded`, alone and around `MediaPlugin`) forward EVERY `GamePlugin` hook to the inner plugin, including hooks with a default impl (which a
//! wrapper could silently shadow) and the v2.2 real-time hooks. A stub records each call; every hook is called once
//! through each wrapper and the stub must have seen exactly the full list. A new engine hook goes into `ALL_HOOKS`
//! (required hooks already fail to compile in a wrapper that forgets them).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fn7_server::media::MediaPlugin;
use serde_json::{json, Value};
use theatre_plugin_api::*;
use fn7_server::record::{recorder, Recorded};

const ALL_HOOKS: &[&str] = &[
    "info",
    "validate_settings",
    "queries",
    "query",
    "on_match_start",
    "on_round_start",
    "brief",
    "validate_action",
    "default_action",
    "on_action_begin",
    "on_instant",
    "on_seat_takeover",
    "bot_action",
    "resolve_round",
    "fallback_outcome",
    "default_choice",
    "commit_round",
    "is_match_over",
    "public_view",
    "seat_view",
    "summarize",
];

#[derive(Clone, Default)]
struct Stub {
    saw: Arc<Mutex<Vec<&'static str>>>,
}

impl Stub {
    fn hit(&self, h: &'static str) {
        self.saw.lock().unwrap().push(h);
    }
}

#[async_trait]
impl GamePlugin for Stub {
    fn info(&self) -> PluginInfo {
        self.hit("info");
        PluginInfo { id: "stub", version: "0.0.1", rules_version: "r1", min_players: 1, max_players: 2, min_active: 1, default_settings: json!({}) }
    }
    fn validate_settings(&self, s: &Value) -> Result<Value, String> {
        self.hit("validate_settings");
        Ok(s.clone())
    }
    fn queries(&self) -> Vec<QuerySpec> {
        self.hit("queries");
        vec![QuerySpec { name: "map".into(), description: "d".into(), args_schema: json!({}), public: true }]
    }
    fn query(&self, _: &Value, seat: Option<Seat>, name: &str, _: &Value) -> Result<Value, String> {
        self.hit("query");
        Ok(json!({ "q": name, "seat": seat }))
    }
    async fn on_match_start(&self, _: &[SeatInfo], _: &Value, _: &dyn LlmProvider) -> anyhow::Result<Value> {
        self.hit("on_match_start");
        Ok(json!({ "n": 0 }))
    }
    async fn on_round_start(&self, state: &Value, _: u32, _: &dyn LlmProvider) -> anyhow::Result<RoundStart> {
        self.hit("on_round_start");
        Ok(RoundStart { state: state.clone(), revived: vec![(1, "back".into())], ..Default::default() })
    }
    async fn brief(&self, _: &Value, _: u32, _: &dyn LlmProvider) -> anyhow::Result<Brief> {
        self.hit("brief");
        Ok(Brief::default())
    }
    fn validate_action(&self, _: &Value, _: Seat, _: &str, _: &Value, kind: SubmitKind) -> Result<(), String> {
        self.hit("validate_action");
        match kind {
            SubmitKind::Window { window_id: 9 } => Ok(()),
            k => Err(format!("{k:?}")),
        }
    }
    fn default_action(&self, _: &Value, seat: Seat) -> Action {
        self.hit("default_action");
        Action::defaulted(seat, "wait")
    }
    fn on_action_begin(&self, _: &Value, _: u32, seat: Seat, _: Option<&WindowInfo>, next: u64) -> PhaseControl {
        self.hit("on_action_begin");
        assert_eq!(next, 7, "next_window_id forwarded");
        PhaseControl {
            window: Some(WindowControl {
                deadline_in_ms: 5,
                close_on_submit_of: Some(seat),
                missing: MissingActions::Drop,
            }),
            public: Some(json!({ "slowed_by": seat })),
            ..Default::default()
        }
    }
    fn on_instant(&self, _: &Value, seat: Seat, _: &Action) -> Option<Value> {
        self.hit("on_instant");
        Some(json!({ "streak": seat }))
    }
    fn on_seat_takeover(&self, _: &Value, t: &SeatTakeover) -> Option<Value> {
        self.hit("on_seat_takeover");
        Some(json!({ "taken_by": t.display_name, "seat": t.seat, "new_seat": t.new_seat }))
    }
    async fn bot_action(&self, _: &Value, seat: Seat, _: &dyn LlmProvider) -> anyhow::Result<Action> {
        self.hit("bot_action");
        Ok(Action::new(seat, "go"))
    }
    async fn resolve_round(
        &self,
        state: &Value,
        _: u32,
        _: &[Action],
        ctx: &RoundContext,
        _: &dyn LlmProvider,
        _: &dyn Progress,
    ) -> anyhow::Result<RoundOutcome> {
        self.hit("resolve_round");
        assert_eq!(ctx.windows_closed.len(), 1, "RoundContext forwarded");
        Ok(RoundOutcome { state: state.clone(), ..Default::default() })
    }
    fn fallback_outcome(&self, state: &Value, _: u32, _: &[Action], ctx: &RoundContext) -> RoundOutcome {
        self.hit("fallback_outcome");
        assert_eq!(ctx.windows_closed.len(), 1, "RoundContext forwarded to fallback_outcome");
        RoundOutcome { state: state.clone(), ..Default::default() }
    }
    fn default_choice(&self, _: &Value, _: Seat, _: &[Choice]) -> String {
        self.hit("default_choice");
        "x".into()
    }
    async fn commit_round(&self, state: &Value, _: u32, _: &[Picked], _: &dyn LlmProvider) -> anyhow::Result<Committed> {
        self.hit("commit_round");
        Ok(Committed { state: state.clone(), revived: vec![(1, "Death relents".into())], ..Default::default() })
    }
    fn is_match_over(&self, _: &Value, _: u32) -> bool {
        self.hit("is_match_over");
        true
    }
    fn public_view(&self, _: &Value) -> Value {
        self.hit("public_view");
        json!({ "public": true })
    }
    fn seat_view(&self, _: &Value, seat: Seat) -> Value {
        self.hit("seat_view");
        json!({ "seat": seat })
    }
    async fn summarize(&self, _: &Value, _: &dyn LlmProvider) -> anyhow::Result<Summary> {
        self.hit("summarize");
        Ok(Summary::default())
    }
}

/// Call every hook once through `p` and check what came back is the stub's (nothing dropped on the way).
async fn call_all(p: &dyn GamePlugin) {
    let llm = theatre_llm::FakeLlm::failing();
    let seats = vec![SeatInfo { seat: 0, display_name: "a".into(), is_bot: false }];
    assert_eq!(p.info().id, "stub");
    p.validate_settings(&json!({})).unwrap();
    assert_eq!(p.queries()[0].name, "map");
    let st = p.on_match_start(&seats, &json!({}), &llm).await.unwrap();
    assert_eq!(p.query(&st, Some(0), "map", &json!({})), Ok(json!({ "q": "map", "seat": 0 })));
    let rs = p.on_round_start(&st, 1, &llm).await.unwrap();
    assert_eq!(rs.revived, vec![(1, "back".to_string())]);
    p.brief(&st, 1, &llm).await.unwrap();
    p.validate_action(&st, 0, "t", &json!({}), SubmitKind::Window { window_id: 9 }).unwrap();
    assert_eq!(p.validate_action(&st, 0, "t", &json!({}), SubmitKind::Instant), Err("Instant".into()));
    assert!(p.default_action(&st, 0).defaulted);
    let info = WindowInfo { window_id: 1, by_seat: 0, opened_ms: 1, deadline_ms: 6, engaged: vec![0] };
    let c = p.on_action_begin(&st, 1, 0, Some(&info), 7);
    assert_eq!(c.window.as_ref().map(|w| w.close_on_submit_of), Some(Some(0)));
    assert_eq!(c.public, Some(json!({ "slowed_by": 0 })));
    let a = Action { instant: true, seq: 3, ..Action::new(0, "bolt") };
    assert_eq!(p.on_instant(&st, 0, &a), Some(json!({ "streak": 0 })));
    let t = SeatTakeover { seat: 1, display_name: "Zed".into(), round: 2, new_seat: true, previous_name: None };
    assert_eq!(p.on_seat_takeover(&st, &t), Some(json!({ "taken_by": "Zed", "seat": 1, "new_seat": true })));
    p.bot_action(&st, 0, &llm).await.unwrap();
    let ctx = RoundContext {
        windows_closed: vec![ClosedWindow {
            info: info.clone(),
            closed_ms: 9,
            reason: "deadline".into(),
            by_seat: None,
            submitted: vec![],
            dropped: vec![],
        }],
    };
    p.resolve_round(&st, 1, std::slice::from_ref(&a), &ctx, &llm, &NoProgress).await.unwrap();
    p.fallback_outcome(&st, 1, &[a], &ctx);
    assert_eq!(p.default_choice(&st, 0, &[Choice::new("x", "X")]), "x");
    let c = p.commit_round(&st, 1, &[], &llm).await.unwrap();
    assert_eq!(c.revived, vec![(1, "Death relents".to_string())]);
    assert!(p.is_match_over(&st, 1));
    assert_eq!(p.public_view(&st), json!({ "public": true }));
    assert_eq!(p.seat_view(&st, 0), json!({ "seat": 0 }));
    p.summarize(&st, &llm).await.unwrap();
}

fn assert_saw_all(stub: &Stub, which: &str) {
    let saw = stub.saw.lock().unwrap();
    for h in ALL_HOOKS {
        assert!(saw.contains(h), "{which} did not forward `{h}` (saw {saw:?})");
    }
}

#[tokio::test]
async fn every_wrapper_forwards_every_hook() {
    // MediaPlugin alone (images off).
    let stub = Stub::default();
    call_all(&MediaPlugin::new(stub.clone(), fn7_media::Media::off())).await;
    assert_saw_all(&stub, "MediaPlugin");

    // The accounts wrapper (no account holds a name here: every hook goes straight through), alone and as the server
    // stacks it, inside MediaPlugin.
    let accounts = fn7_server::accounts::Accounts::new(
        Arc::new(fn7_profile::ProfileStore::open_in_memory().unwrap()),
        None,
        None,
        Default::default(),
    );
    let stub = Stub::default();
    call_all(&fn7_server::accounts::plugin::AccountsPlugin::new(stub.clone(), accounts.clone())).await;
    assert_saw_all(&stub, "AccountsPlugin");
    let stub = Stub::default();
    call_all(&MediaPlugin::new(
        fn7_server::accounts::plugin::AccountsPlugin::new(stub.clone(), accounts),
        fn7_media::Media::off(),
    ))
    .await;
    assert_saw_all(&stub, "MediaPlugin(AccountsPlugin)");

    // fn7-sim's recorder alone, and around MediaPlugin, recording at full (sync hooks included).
    let dir = std::env::temp_dir().join(format!("fn7-wrappers-{}", uuid::Uuid::new_v4()));
    let rec = recorder(&dir, theatre_trace::Level::Full).unwrap();
    let stub = Stub::default();
    call_all(&Recorded::new(stub.clone(), rec.run("w1"))).await;
    assert_saw_all(&stub, "Recorded");
    let stub = Stub::default();
    call_all(&Recorded::new(MediaPlugin::new(stub.clone(), fn7_media::Media::off()), rec.run("w2"))).await;
    assert_saw_all(&stub, "Recorded(MediaPlugin)");
    rec.flush();

    // The recording captured the real-time hooks too (hook name + input/output), in the core's format.
    for run in ["w1", "w2"] {
        let lines: Vec<Value> = std::fs::read_to_string(dir.join(format!("{run}.jsonl")))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let hook = |name: &str| lines.iter().filter(|l| l["type"] == "hook" && l["hook"] == name).count();
        assert_eq!(hook("on_action_begin"), 1);
        assert!(lines.iter().any(|l| l["hook"] == "on_action_begin" && l["input"]["window"]["window_id"] == 1
            && l["output"]["window"]["close_on_submit_of"] == 0));
        assert!(lines.iter().any(|l| l["hook"] == "resolve_round" && l["input"]["ctx"]["windows_closed"][0]["reason"] == "deadline"));
        assert!(lines.iter().any(|l| l["hook"] == "on_instant" && l["output"]["streak"] == 0 && l["input"]["action"]["seq"] == 3));
        assert_eq!(hook("query"), 1);
        // A mid-match join, as the core records it: a hook record (input {takeover}, output {state_changed}) and an
        // `on_seat_takeover` snapshot of the new state, at the round the joiner plays from.
        assert!(lines.iter().any(|l| l["hook"] == "on_seat_takeover" && l["input"]["takeover"]["display_name"] == "Zed"
            && l["output"]["state_changed"] == true && l["round"] == 2 && l["seat"] == 1));
        assert!(lines.iter().any(|l| l["type"] == "snapshot" && l["label"] == "on_seat_takeover"
            && l["state"]["taken_by"] == "Zed"));
        // The fallback is a hook record with its input (round, actions, ctx, resolve_error) and a snapshot.
        assert!(lines.iter().any(|l| l["hook"] == "fallback_outcome" && l["input"]["round"] == 1
            && l["input"]["ctx"]["windows_closed"][0]["reason"] == "deadline" && l["input"]["actions"][0]["seq"] == 3
            && l["input"].get("resolve_error").is_some() && l["output"]["state"] == "<see snapshot>"));
        assert!(lines.iter().any(|l| l["type"] == "snapshot" && l["label"] == "fallback_outcome"));
        assert!(lines.iter().any(|l| l["type"] == "ruleset" && l["version"] == "0.0.1" && l["label"] == "r1"));
        assert!(lines.iter().any(|l| l["type"] == "snapshot" && l["label"] == "commit_round"));
        assert!(lines.iter().any(|l| l["type"] == "run_end"));
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn final_night_7_declares_its_queries_through_the_media_wrapper() {
    let p = MediaPlugin::new(fn7_plugin::FinalNight7Plugin::new(), fn7_media::Media::off());
    let names: Vec<String> = p.queries().into_iter().map(|q| q.name).collect();
    assert_eq!(
        names,
        ["globe", "region", "node", "person", "army", "events", "me", "powers", "gods", "standings", "present", "media", "items", "item", "memory",
         "encounters", "earth", "earth_at", "fault", "faults_overview", "piles_final"]
    );
    assert!(p.query(&json!({}), None, "powers", &json!({})).unwrap()["powers"].is_array());
    assert_eq!(p.query(&json!({}), Some(0), "globe", &json!({})), Err("no match state".to_string()));
    let info = p.info();
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.rules_version, fn7_plugin::RULES_VERSION);
}
