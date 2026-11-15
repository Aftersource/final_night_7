//! The duel picker (state schema 14; docs/DUEL_MOVES.md §4.1) against a fake gateway: the request's enums come from
//! the list, a pick that comes back before the panel's input close stages the line, a timeout leaves the mapper's
//! pick, a cached pick is known at the line's arrival and sets the class, and a replay fed the recorded `duel_picks`
//! reproduces the state without a model.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use theatre_plugin_api::*;
use theatre_trace::{Ctx, Record, Sink};

use crate::duel_tests::*;
use crate::*;

/// A gateway that answers every request with `answer` after `delay`, reports 900 / 40 tokens and keeps the requests.
struct Fake {
    answer: Value,
    delay: Duration,
    seen: Mutex<Vec<LlmRequest>>,
}

#[async_trait]
impl LlmProvider for Fake {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        self.seen.lock().unwrap().push(req);
        tokio::time::sleep(self.delay).await;
        theatre_trace::http(theatre_trace::HttpRecord { input_tokens: Some(900), output_tokens: Some(40), ..Default::default() });
        Ok(LlmResponse { choices: vec![self.answer.to_string()], cached: false })
    }
}

fn fake(answer: Value, delay_ms: u64) -> Arc<Fake> {
    Arc::new(Fake { answer, delay: Duration::from_millis(delay_ms), seen: Mutex::new(vec![]) })
}

/// A duel table whose plugin picks with `llm` (the room's LLM on, the AI typing nothing).
fn picking(seed: u64, llm: Arc<Fake>, extra: Value) -> Table {
    let mut s = json!({ "llm": true, "duel_damage_scale": 0.1 });
    if let (Some(o), Some(x)) = (s.as_object_mut(), extra.as_object()) {
        o.extend(x.clone());
    }
    let mut t = table(seed, s);
    t.p = Arc::new(FinalNight7Plugin::new().with_picker(llm, None));
    t
}

/// The picks the journal holds for `w`'s match.
fn picks(t: &Table) -> Vec<picker::PickRec> {
    let w = world::load(&t.state).unwrap();
    t.p.journal().picks(&w, Some((i64::MAX, u64::MAX)))
}

fn wait_for(t: &Table, n: usize) -> Vec<picker::PickRec> {
    for _ in 0..300 {
        let p = picks(t);
        if p.len() >= n {
            return p;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no pick came back");
}

/// The trace events a hook wrote (a recording's `event` records).
#[derive(Default)]
struct Capture(Mutex<Vec<Value>>);

impl Sink for Capture {
    fn next_id(&self) -> i64 {
        1
    }
    fn write(&self, r: Record) {
        if let Record::Event { kind, data, ctx, .. } = r {
            self.0.lock().unwrap().push(json!({ "type": "event", "kind": kind, "data": data, "round": ctx.round }));
        }
    }
}

/// An instant action stamped `ms` (absolute, the real clock: these tests never touch the shared `t0`).
fn at(seat: u8, fields: Value, seq: u64, ms: i64) -> Action {
    Action { fields, instant: true, seq, received_at_ms: ms, ..Action::new(seat, "") }
}

fn kick_pick() -> Value {
    json!({ "move_class": "strike", "clips": ["roundhouse_kick"], "path": null, "object_id": null, "target_part": "head" })
}

#[test]
fn a_pick_from_the_list_stages_the_line_and_replays_exactly() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _g = rt.enter();
    let llm = fake(kick_pick(), 0);
    let t = picking(71, llm.clone(), json!({}));
    let now = theatre_trace::now_ms();
    let lock = submit(&t, &at(0, challenge(&t, "regions"), 1, now - 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let close = lock["also"].as_array().unwrap().iter().find(|x| x["event"] == "duel.panel").unwrap()["input_closes_ms"].as_i64().unwrap();
    let b = submit(&t, &at(0, line(&id, "I hit him"), 2, now - 500)).unwrap();
    assert_eq!((b["move"]["picker"].clone(), b["choreo"]["a"]["move_class"].as_str()), (Value::Null, Some("strike")), "resolved at once by the mapper");
    let got = wait_for(&t, 1);
    assert!(got[0].at_ms <= close, "the pick came back before the input close");
    assert_eq!((got[0].pick.as_ref().map(|p| p.clips.clone()), got[0].tokens_in, got[0].tokens_out, got[0].pre), (Some(vec!["roundhouse_kick".to_string()]), 900, 40, false));
    assert!(got[0].cost_usd > 0.0 && got[0].latency_ms >= 0 && got[0].error.is_none());
    // The request: the model, no thinking budget to speak of, every choice an enum built from the list and the scene.
    let req = llm.seen.lock().unwrap()[0].clone();
    let list = moves::current().body.clone();
    let schema = req.json_schema.clone().unwrap();
    assert_eq!(req.model.as_deref(), Some(fn7_agents::duel_pick::MODEL));
    assert_eq!(schema["properties"]["move_class"]["enum"], json!(fn7_agents::duel_pick::classes(&list)));
    assert_eq!(schema["properties"]["clips"]["items"]["enum"], json!(fn7_agents::duel_pick::clips(&list)));
    let objects: Vec<Value> = lock["also"][0]["objects"].as_array().unwrap().iter().map(|o| o["id"].clone()).chain([Value::Null]).collect();
    assert_eq!(schema["properties"]["object_id"]["enum"], json!(objects), "the scene's object ids");
    assert!(req.messages[0].1.contains("<<<BEGIN UNTRUSTED DUEL LINE") && req.messages[0].1.contains("I hit him"));
    // The tick at the close: panel 1 stages the line with the pick (its clip, the head), and the blow says so.
    let tick = submit(&t, &at(0, json!({ "duel_tick": { "duel_id": id } }), 3, close + 30)).unwrap();
    let panel = if tick["event"] == "duel.panel" { tick.clone() } else { tick["also"].as_array().unwrap().iter().find(|x| x["event"] == "duel.panel").unwrap().clone() };
    let segs: Vec<&str> = panel["a"]["segs"].as_array().unwrap().iter().filter_map(|s| s["clip"].as_str()).collect();
    assert!(segs.contains(&"roundhouse_kick"), "{segs:?}");
    assert!(panel["contacts"].as_array().unwrap().iter().any(|c| c["by"] == "a" && c["at"] == "head"), "{}", panel["contacts"]);
    // The resolve that ends the duel: handed the pick (`duel_picks`), its rows say it was used.
    let acts = [at(0, challenge(&t, "regions"), 1, now - 1_000), at(0, line(&id, "I hit him"), 2, now - 500),
                at(0, json!({ "duel_tick": { "duel_id": id } }), 3, close + 30), at(3, json!({ "duel_tick": { "duel_id": "duel:s999999:0:1" } }), 9_999, now + 600_000)];
    let cap = Arc::new(Capture::default());
    let mut c = Ctx::new(cap.clone(), "");
    c.round = Some(2);
    let o = theatre_trace::within_sync(c, || resolve_at(&t.p, &t, 2, &acts));
    let events = cap.0.lock().unwrap().clone();
    let handed = events.iter().find(|e| e["kind"] == "duel_picks").expect("the picks are recorded");
    assert_eq!(handed["data"]["picks"].as_array().map(Vec::len), Some(1));
    let row = events.iter().find(|e| e["kind"] == "duel_line" && e["data"]["line"] == "I hit him").expect("the line's record");
    assert_eq!((row["data"]["picker"]["used"].clone(), row["data"]["staged"]["clip"].clone()), (json!(true), json!("roundhouse_kick")));
    assert_eq!(row["data"]["picker"]["tokens_in"], 900);
    // Replay: no model, the recorded picks; the very same state. Without them it differs (the pick mattered).
    let replay = FinalNight7Plugin::new().with_feed(present::Feed::from_records(&events));
    assert_eq!(resolve_at(&replay, &t, 2, &acts).state, o.state, "the replay reproduces the picks");
    assert_ne!(resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts).state, o.state);
    // A client can never send one.
    assert!(t.p.validate_action(&t.state, 0, "", &json!({ "duel_pick": got[0] }), SubmitKind::Instant).is_err());
}

#[test]
fn a_timeout_leaves_the_mappers_pick_and_a_late_answer_fills_the_cache() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _g = rt.enter();
    let llm = fake(json!({ "move_class": "throw", "clips": ["throw_shoulder"], "path": null, "object_id": null, "target_part": "belly" }), 300);
    let t = picking(72, llm.clone(), json!({ "duel_picker_timeout_ms": 100 }));
    let now = theatre_trace::now_ms();
    let lock = submit(&t, &at(0, challenge(&t, "people"), 1, now - 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let b = submit(&t, &at(0, line(&id, "grab him"), 2, now - 600)).unwrap();
    assert_eq!(b["choreo"]["a"]["move_class"], "grapple", "the mapper's class");
    let got = wait_for(&t, 1);
    assert_eq!((got[0].pick.clone(), got[0].error.as_deref()), (None, Some("timeout")));
    assert!(got[0].latency_ms >= 100);
    // The answer arrives after the timeout: it is cached.
    std::thread::sleep(Duration::from_millis(400));
    let pk = t.p.picker().unwrap();
    assert_eq!(pk.cached(&got[0].key).map(|p| p.move_class), Some("throw".to_string()), "the late answer warms the cache");
    // A line whose (words, range, list) is cached is picked at its arrival: the pick is journaled before it and
    // chooses its class (here a parry, which no range refuses).
    let w = world::load(&t.state).unwrap();
    let range = t.p.journal().live(&w, &world::settings(&w)).bouts[0].flow.range;
    let range = serde_json::to_value(range).unwrap().as_str().unwrap().to_string();
    let parry: fn7_agents::duel_pick::Pick = serde_json::from_value(json!({ "move_class": "parry", "clips": ["parry_high"], "path": null,
                                                                           "object_id": null, "target_part": "arms" })).unwrap();
    pk.prime(&picker::key("Grab him!", &range, "unarmed", &moves::list_version()), &parry);
    let b2 = submit(&t, &at(0, line(&id, "Grab him!"), 3, theatre_trace::now_ms())).unwrap();
    let all = picks(&t);
    let pre = all.iter().find(|p| p.pre).expect("a cached pick, journaled at the line's arrival");
    assert!(pre.cached && pre.line_seq == 3 && pre.at_ms == pre.line_at_ms);
    assert_eq!(b2["choreo"]["a"]["move_class"], "parry", "a cached pick chooses the class");
    assert_eq!((b2["move"]["picker"].as_str(), b2["move"]["clip"].as_str()), (Some("pre"), Some("parry_high")));
    assert_eq!(llm.seen.lock().unwrap().len(), 1, "no second call");
}

#[test]
fn off_or_old_or_without_the_llm_nothing_is_asked() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _g = rt.enter();
    for extra in [json!({ "duel_picker": "off" }), json!({ "llm": false })] {
        let llm = fake(kick_pick(), 0);
        let t = picking(73, llm.clone(), extra.clone());
        let now = theatre_trace::now_ms();
        let lock = submit(&t, &at(0, challenge(&t, "regions"), 1, now - 1_000)).unwrap();
        submit(&t, &at(0, line(lock["duel_id"].as_str().unwrap(), "I hit him"), 2, now - 500)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(llm.seen.lock().unwrap().is_empty() && picks(&t).is_empty(), "{extra}");
    }
    assert!(settings::Settings::validate(&json!({ "duel_picker": "maybe" })).is_err());
}
