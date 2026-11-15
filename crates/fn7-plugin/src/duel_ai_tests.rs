//! AI duellists type (state schema 12, `duel_ai`): a script at the lock and at every window, a line and a blow in
//! every window, no double strike, the script inside its window, live = resolve = replay, the present, older states
//! untouched, two AIs with a clock, and the balance against scripted human typists.

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::duel_tests::*;
use crate::*;

fn tick(id: &str) -> Value {
    json!({ "duel_tick": { "duel_id": id } })
}

fn typing_table(seed: u64, extra: Value) -> Table {
    let mut s = json!({ "duel_ai_lines_per_window": 1.0, "duel_deadline_ms": 30000, "duel_damage_scale": 0.1 });
    if let (Some(o), Some(x)) = (s.as_object_mut(), extra.as_object()) {
        o.extend(x.clone());
    }
    table(seed, s)
}

/// The main event of a submit and its `also[]`, flattened in order.
fn events(v: &Value) -> Vec<Value> {
    let mut main = v.clone();
    let also = main.as_object_mut().and_then(|o| o.remove("also")).and_then(|a| a.as_array().cloned()).unwrap_or_default();
    let mut out = vec![main];
    out.extend(also);
    out
}

fn of<'a>(evs: &'a [Value], kind: &str) -> Vec<&'a Value> {
    evs.iter().filter(|e| e["event"] == kind).collect()
}

/// A script is well formed and lies inside the window of the panel that opens it.
fn check_script(s: &Value, panel: &Value, by: &str, demigod: DemigodId) {
    assert_eq!((s["event"].as_str(), s["by"].as_str(), s["demigod"].clone(), s["window"].clone(), s["duel_id"].clone()),
               (Some("duel.typing_script"), Some(by), json!(Ref::Demigod(demigod)), panel["n"].clone(), panel["duel_id"].clone()), "{s}");
    let text = s["text"].as_str().unwrap();
    let n = text.chars().count();
    assert!((duel_ai::MIN_CHARS..=duel_ai::MAX_CHARS).contains(&n) && s["len"].as_u64() == Some(n as u64), "a sentence of 20 to 70 characters: {text:?}");
    assert!(text.split(' ').count() >= 3, "a sentence: {text:?}");
    let (cps, key) = (s["cps"].as_f64().unwrap(), s["key_ms"].as_i64().unwrap());
    assert!((6.0..=9.0).contains(&cps) && key == (1000.0 / cps).round() as i64, "6 to 9 characters a second: {cps} ({key} ms)");
    let (starts, ends, closes) = (s["starts_at_ms"].as_i64().unwrap(), s["ends_at_ms"].as_i64().unwrap(), s["input_closes_ms"].as_i64().unwrap());
    let (opens, p_closes) = (panel["input_opens_ms"].as_i64().unwrap(), panel["input_closes_ms"].as_i64().unwrap());
    assert_eq!(closes, p_closes, "it names its window's close");
    assert!(starts >= opens + 100, "it starts typing after the window opens: {starts} vs {opens}");
    assert!(ends + duel_ai::GUARD_MS <= closes, "Enter at least 400 ms before the input closes: {ends} vs {closes}");
    let pauses = s["pauses"].as_array().unwrap();
    assert!(pauses.len() <= 2 && pauses.iter().all(|p| (80..=180).contains(&p["ms"].as_i64().unwrap()) && (p["after"].as_u64().unwrap() as usize) < n));
    // The client's rule: character k (1-based) appears at starts + k * key_ms + the pauses with after < k.
    let at = |k: usize| starts + k as i64 * key + pauses.iter().filter(|p| (p["after"].as_u64().unwrap() as usize) < k).map(|p| p["ms"].as_i64().unwrap()).sum::<i64>();
    assert!(at(1) > starts && at(n) < ends && ends - at(n) <= 240, "the last letter, then Enter: {} .. {ends}", at(n));
    assert!((1..n).all(|k| at(k + 1) - at(k) >= key), "letters come in order");
    assert_eq!(panel[by]["typing_script"], *s, "the same object rides on the panel that opens the window");
    // The panel plays as long as the line needs.
    assert_eq!(p_closes, panel["starts_at_ms"].as_i64().unwrap() + panel["play_ms"].as_i64().unwrap() - panel::LEAD_MS);
}

/// The designer's duel: a human challenges an AI and only ticks. The lock carries the AI's first script; every
/// window has an AI line that becomes a blow inside it and is staged in the next panel as a typed line; the human,
/// typing nothing, only gets hit and loses.
#[test]
fn the_ai_types_a_line_and_strikes_in_every_window() {
    let t = typing_table(61, json!({}));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let first = events(&lock);
    assert_eq!(first.iter().map(|e| e["event"].as_str().unwrap()).collect::<Vec<_>>(),
               ["challenge.lock", "duel.scene", "duel.panel", "duel.typing_script"], "the lock, the scene, panel 0, the AI's first script");
    check_script(&first[3], &first[2], "b", t.foe);
    assert!(first[2]["a"]["typing_script"].is_null(), "the human's side has no script");
    assert!(first[2]["play_ms"].as_i64().unwrap() > 2600, "the intro plays as long as the first line needs");
    let mut acts = vec![instant(0, challenge(&t, "regions"), 1, 1_000)];
    let mut panels = vec![first[2].clone()];
    let mut scripts = vec![first[3].clone()];
    let mut blows: Vec<Value> = vec![];
    let mut seq = 2;
    // The human's client ticks at every input close.
    while let Some(close) = panels.last().unwrap()["input_closes_ms"].as_i64() {
        let a = instant(0, tick(&id), seq, close - t0() + 30);
        seq += 1;
        let evs = events(&submit(&t, &a).expect("the tick closes the window"));
        acts.push(a);
        assert_eq!(evs[0]["event"], "duel.panel", "a tick's event is the panel it made due");
        // (The live event carries the action's stamps; the panel itself does not.)
        let mut p = evs[0].clone();
        assert!(p["seq"].is_u64() && p["received_at_ms"].is_i64());
        for k in ["seq", "received_at_ms"] {
            p.as_object_mut().unwrap().remove(k);
        }
        let n = p["n"].as_u64().unwrap();
        // The AI's line of the window that just closed: a blow, stamped at its Enter, staged in this panel.
        let entered = of(&evs, "duel.blow");
        let script = panels.last().unwrap()["b"]["typing_script"].clone();
        if !script.is_object() {
            // A window too close to the deadline to hold a line: the AI typed nothing in it.
            assert!(entered.is_empty() && p["blows"] == json!([]) && n >= 4, "{p}");
            panels.push(p);
            continue;
        }
        assert_eq!(entered.len(), 1, "one AI blow per window: {evs:?}");
        let blow = entered[0];
        assert_eq!((blow["ai"].as_bool(), blow["auto"].as_bool(), blow["text"].clone(), blow["at_ms"].clone(), blow["by"].clone(), blow["window"].as_u64()),
                   (Some(true), Some(false), script["text"].clone(), script["ends_at_ms"].clone(), json!(Ref::Demigod(t.foe)), Some(n - 1)));
        assert!(blow.get("counter").is_none(), "an idle human is not countered for");
        assert!(blow["roll"].as_u64().is_some_and(|r| (1..=20).contains(&r)) && blow["choreo"]["b"]["move_class"].is_string(), "the d20 and the matrix: {blow}");
        assert_eq!(blow["choreo"]["b"]["move_class"], json!(choreo::effective(choreo::classify(script["text"].as_str().unwrap()),
                   serde_json::from_value(blow["choreo"]["range_before"].clone()).unwrap(), true, true)), "the class is the line's");
        // Its wit is capped; its handicap shows, until the human has been silent for 12 s.
        let silent = blow["at_ms"].as_i64().unwrap() - (t0() + 1_000) > duel_ai::IDLE_MS;
        assert!(blow["wit"].as_i64().unwrap() <= 2);
        assert_eq!(blow["modifiers"].get("handicap").cloned(), (!silent).then(|| json!(-(Settings::default().duel_ai_handicap as i64))), "{}", blow["modifiers"]);
        assert_eq!(p["blows"], json!([blow["n"]]), "the panel stages it like any blow");
        assert_eq!((p["b"]["line"].clone(), p["b"]["lines"].as_u64(), p["b"]["auto"].as_bool(), p["a"]["lines"].as_u64()),
                   (script["text"].clone(), Some(1), Some(false), Some(0)));
        assert!(matches!(p["kind"].as_str(), Some("exchange" | "ko")));
        blows.push(blow.clone());
        let next = of(&evs, "duel.typing_script");
        if p["input_closes_ms"].is_i64() && n < 4 {
            assert_eq!(next.len(), 1, "the next window's script comes with the panel that opens it");
        }
        if let Some(s) = next.first() {
            check_script(s, &p, "b", t.foe);
            scripts.push((*s).clone());
        } else if p["b"].get("typing_script").is_some() {
            assert!(p["b"]["typing_script"].is_null());
        }
        panels.push(p);
        if panels.last().unwrap()["input_closes_ms"].is_null() {
            break;
        }
        assert!(panels.len() < 20);
    }
    assert!(blows.len() >= 4, "a 30 s duel has at least four windows it types in: {}", blows.len());
    assert_eq!(blows.iter().map(|b| b["n"].as_u64().unwrap()).collect::<Vec<_>>(), (1..=blows.len() as u64).collect::<Vec<_>>());
    let texts: std::collections::BTreeSet<&str> = scripts.iter().map(|s| s["text"].as_str().unwrap()).collect();
    assert!(texts.len() >= 3, "the lines differ: {texts:?}");
    let plays: Vec<i64> = panels.iter().map(|p| p["play_ms"].as_i64().unwrap()).collect();
    assert!(plays.iter().all(|p| (2400..=6000).contains(p)), "{plays:?}");
    // The present has every panel (with its script) and the blows; the resolve ends it: the idle human lost.
    let pr = t.p.public_view(&t.state)["present"]["duels"][0].clone();
    let shown = pr["panels"].as_array().unwrap();
    assert_eq!(shown.len(), panels.len());
    for (x, y) in shown.iter().zip(&panels) {
        assert_eq!(x, y, "panel {}", y["n"]);
    }
    assert_eq!(pr["blows"].as_array().unwrap(), &blows);
    let o = resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts);
    let w = world::load(&o.state).unwrap();
    let b: duel::Bout = serde_json::from_value(w.dm.duels[0].clone()).unwrap();
    assert_eq!((b.typed, b.ai.as_ref().unwrap().struck as usize, b.hp[1]), (0, blows.len(), 10.0));
    assert!(b.hp[0] < 10.0 && b.result() == Some((1, 0)), "whoever types nothing loses: {:?}", b.hp);
    assert_eq!(o.public["duels_open"][0]["log"].as_array().unwrap(), &blows, "the outcome's log has the AI's blows");
    assert_eq!(resolve_at(&t.p, &t, 2, &acts).state, o.state, "live plugin and fresh plugin resolve alike");
}

/// A human who types: the AI's line of the window is entered first when its Enter came first, the human's line draws
/// no automatic counter (the AI typed in this window), and each side strikes once.
#[test]
fn a_typing_ai_does_not_also_counter() {
    let t = typing_table(62, json!({}));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let first = events(&lock);
    let (p0, s0) = (&first[2], &first[3]);
    let (ends, closes) = (s0["ends_at_ms"].as_i64().unwrap() - t0(), p0["input_closes_ms"].as_i64().unwrap() - t0());
    // Before the AI's Enter: the human's line is blow 1, alone (no counter rides on it).
    let b1 = submit(&t, &instant(0, line(&id, "I kick your knee into the mud"), 2, ends - 600)).unwrap();
    assert_eq!((b1["n"].as_u64(), b1.get("counter"), b1.get("also")), (Some(1), None, None), "{b1}");
    // After the AI's Enter and before the close: the AI's line is entered first (blow 2), then the human's (blow 3).
    let b3 = submit(&t, &instant(0, line(&id, "then I smash your jaw with my elbow"), 3, ends + 100)).unwrap();
    let evs = events(&b3);
    assert_eq!((b3["n"].as_u64(), b3.get("counter"), b3["ai"].as_bool()), (Some(3), None, None));
    assert_eq!(evs.len(), 2, "{evs:?}");
    assert_eq!((evs[1]["event"].as_str(), evs[1]["ai"].as_bool(), evs[1]["n"].as_u64(), evs[1]["at_ms"].as_i64(), evs[1]["text"].clone()),
               (Some("duel.blow"), Some(true), Some(2), Some(t0() + ends), s0["text"].clone()));
    // Against a human the AI's handicap comes off its modifier and shows.
    let m = &evs[1]["modifiers"];
    assert_eq!(m["handicap"], -(Settings::default().duel_ai_handicap as i64));
    assert_eq!(evs[1]["modifier"].as_i64().unwrap(), ["wit", "health", "gear", "follow_up", "home", "situation", "handicap"].iter().map(|k| m[*k].as_i64().unwrap()).sum::<i64>());
    assert!(evs[1]["wit"].as_i64().unwrap() <= 2 && b3["modifiers"].get("handicap").is_none());
    // The close: nothing more is entered for window 0 (one AI strike per window); the panel stages all three.
    let p1 = submit(&t, &instant(0, tick(&id), 4, closes + 20)).unwrap();
    let evs = events(&p1);
    assert_eq!((p1["event"].as_str(), p1["blows"].clone()), (Some("duel.panel"), json!([1, 2, 3])));
    assert!(of(&evs, "duel.blow").is_empty(), "no second AI strike in the window: {evs:?}");
    assert_eq!((p1["a"]["lines"].as_u64(), p1["b"]["lines"].as_u64(), p1["a"]["auto"].as_bool(), p1["b"]["auto"].as_bool()), (Some(2), Some(1), Some(false), Some(false)));
    let ai: Vec<Value> = t.p.public_view(&t.state)["present"]["duels"][0]["blows"].as_array().unwrap().iter().filter(|b| b["ai"] == true).cloned().collect();
    assert_eq!(ai.len(), 1);
    // With the AI's typing off (0.8.2), the same line is countered automatically.
    let old = table(62, json!({ "duel_deadline_ms": 30000, "duel_damage_scale": 0.1 }));
    let l = submit(&old, &instant(0, challenge(&old, "regions"), 1, 1_000)).unwrap();
    let c = submit(&old, &instant(0, line(l["duel_id"].as_str().unwrap(), "I kick your knee into the mud"), 2, 1_500)).unwrap();
    assert!(c["counter"].is_object() && c["counter"]["auto"] == true && l["also"].as_array().unwrap().len() == 2);
}

/// Deterministic, and independent of when things are computed: the live view, a resolve by a fresh plugin (a
/// replay), a resolve without the ticks, and the same match again all give the same scripts, blows and panels.
#[test]
fn scripts_and_blows_are_the_same_live_at_the_resolve_and_on_replay() {
    let run = |seed: u64| {
        let t = typing_table(seed, json!({}));
        let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
        let id = lock["duel_id"].as_str().unwrap().to_string();
        let mut acts = vec![instant(0, challenge(&t, "regions"), 1, 1_000)];
        let says = ["I kick your knee into the mud", "I hook your chin then jab your nose", "I sweep your ankle by the river"];
        let mut seq = 2;
        for k in 0..3 {
            let close = t.p.public_view(&t.state)["present"]["duels"][0]["panels"].as_array().unwrap().last().unwrap()["input_closes_ms"].as_i64().unwrap() - t0();
            for a in [instant(0, line(&id, says[k]), seq, close - 900), instant(0, tick(&id), seq + 1, close + 25)] {
                submit(&t, &a).unwrap();
                acts.push(a);
            }
            seq += 2;
        }
        (t, id, acts)
    };
    let (t, _, acts) = run(63);
    let live = t.p.public_view(&t.state)["present"]["duels"][0].clone();
    assert_eq!(live["panels"].as_array().unwrap().len(), 4);
    assert_eq!(live["blows"].as_array().unwrap().len(), 6, "three human lines and three AI lines");
    // Mid-line: the script being typed is in the present, and on the newest panel.
    let typing = live["typing_scripts"].as_array().unwrap();
    assert_eq!(typing.len(), 1);
    assert_eq!(typing[0], live["panels"][3]["b"]["typing_script"]);
    check_script(&typing[0], &live["panels"][3], "b", t.foe);
    // The resolve (a fresh plugin: a replay) builds the same duel; so does the live plugin; and it is in the state.
    let o = resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts);
    let w = world::load(&o.state).unwrap();
    assert_eq!((&w.dm.duels[0]["panels"], &w.dm.duels[0]["blows"]), (&live["panels"], &live["blows"]));
    assert_eq!(w.dm.duels[0]["ai"]["pending"].as_array().unwrap().len(), 1, "the script being typed is in the state");
    assert_eq!(resolve_at(&t.p, &t, 2, &acts).state, o.state);
    assert_eq!(resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts).state, o.state);
    // Without the ticks (no client ticked): the same blows and panels, computed when the next line or the resolve came.
    let no_ticks: Vec<Action> = acts.iter().filter(|a| a.fields.get("duel_tick").is_none()).cloned().collect();
    let w2 = world::load(&resolve_at(&FinalNight7Plugin::new(), &t, 2, &no_ticks).state).unwrap();
    let (some, all) = (w2.dm.duels[0]["blows"].as_array().unwrap(), w.dm.duels[0]["blows"].as_array().unwrap());
    assert!(some.len() >= 5 && some[..] == all[..some.len()], "{} of {}", some.len(), all.len());
    assert_eq!(w2.dm.duels[0]["panels"].as_array().unwrap()[..3], w.dm.duels[0]["panels"].as_array().unwrap()[..3]);
    // The same match again: the same scripts. Another seed: other lines.
    let (t2, _, _) = run(63);
    assert_eq!(t2.p.public_view(&t2.state)["present"]["duels"][0]["panels"], live["panels"]);
    let (t3, _, _) = run(64);
    assert_ne!(t3.p.public_view(&t3.state)["present"]["duels"][0]["panels"][0]["b"]["typing_script"]["text"], live["panels"][0]["b"]["typing_script"]["text"]);
    // A reconnect after the resolve (the next beat): the state's duel shows the line being typed.
    let t4 = next_beat(&t, &o, 2);
    let pr = t4.p.public_view(&t4.state)["present"].clone();
    assert_eq!(pr["duels"][0]["typing_scripts"], live["typing_scripts"]);
    assert!(pr["server_time_ms"].is_i64());
    // Nobody ticks and nobody types: the next resolve's clock enters the pending line and closes the windows.
    let later = typing[0]["input_closes_ms"].as_i64().unwrap() - t0() + 9_000;
    let o5 = resolve_at(&FinalNight7Plugin::new(), &t4, 3, &[instant(0, tick("duel:s99:0:1"), 50, later)]);
    let open = &o5.public["duels_open"][0];
    let log = open["log"].as_array().unwrap();
    assert!(log.len() >= 2 && log.iter().all(|b| b["ai"] == true), "the AI's lines of the windows that closed: {log:?}");
    assert_eq!(log[0]["text"], typing[0]["text"]);
    assert!(open["panels"].as_array().unwrap().len() >= 2);
}

/// Older states and the switch: a schema-11 state (and `duel_ai_lines_per_window: 0`) duels exactly as 0.8.2 did.
#[test]
fn states_before_schema_12_have_no_typing_ai() {
    let t = typing_table(65, json!({}));
    let mut w = world::load(&t.state).unwrap();
    assert!(w.ai_duel_rules());
    w.meta.schema = 11;
    assert!(!w.ai_duel_rules());
    let old = Table { p: t.p.clone(), state: world::save(&w), me: t.me, foe: t.foe };
    let off = table(65, json!({ "duel_deadline_ms": 30000, "duel_damage_scale": 0.1 }));
    for x in [&old, &off] {
        let lock = submit(x, &instant(0, challenge(x, "regions"), 1, 1_000)).unwrap();
        let id = lock["duel_id"].as_str().unwrap().to_string();
        let also = lock["also"].as_array().unwrap();
        assert_eq!((also.len(), also[1]["play_ms"].as_i64(), also[1]["b"].get("typing_script")), (2, Some(2600), None));
        let b = submit(x, &instant(0, line(&id, "I kick your knee into the mud"), 2, 1_500)).unwrap();
        assert!(b["counter"].is_object(), "the automatic counter, as before");
        let p1 = submit(x, &instant(0, tick(&id), 3, 2_900)).unwrap();
        assert_eq!((p1["play_ms"].as_i64(), p1["b"]["auto"].as_bool(), p1.get("also")), (Some(2600), Some(true), None));
        let pr = x.p.public_view(&x.state)["present"]["duels"][0].clone();
        assert!(pr.get("typing_scripts").is_none());
        let acts = [instant(0, challenge(x, "regions"), 1, 1_000), instant(0, line(&id, "I kick your knee into the mud"), 2, 1_500), instant(0, tick(&id), 3, 2_900)];
        let o = resolve_at(&FinalNight7Plugin::new(), x, 2, &acts);
        let d = world::load(&o.state).unwrap().dm.duels[0].clone();
        assert!(d.get("ai").is_none() && !serde_json::to_string(&o.state).unwrap().contains("typing_script"), "nothing of it in the state");
    }
    // The two old-style duels are the same duel (the schema number aside): the setting off is the 0.8.2 duel.
    assert!(Settings::from_value(&json!({ "beats_total": 30 })).duel_ai_lines_per_window == 0.0, "settings recorded before it: off");
    assert!(Settings::validate(&json!({ "duel_ai_lines_per_window": 1.5 })).is_err() && Settings::validate(&json!({ "duel_ai_aggression": -0.1 })).is_err()
            && Settings::validate(&json!({ "duel_ai_handicap": 11 })).is_err() && Settings::validate(&json!({ "duel_ai_handicap": -1 })).is_err());
    let d = Settings::validate(&Value::Null).unwrap();
    assert_eq!((d["duel_ai_lines_per_window"].to_string(), d["duel_ai_aggression"].to_string(), d["duel_ai_handicap"].to_string()),
               ("1.0".to_string(), Settings::default().duel_ai_aggression.to_string(), Settings::default().duel_ai_handicap.to_string()));
}

/// Two AIs with a clock (the server: an AI's challenge is stamped with the beat's Start): both type in every window,
/// the duel runs to a knockout or its deadline instead of ending at once, and with no clock it is fought at one
/// resolve as before.
#[test]
fn two_ais_with_a_clock_type_their_duel_out() {
    let t = typing_table(66, json!({ "duel_damage_scale": 1.0 }));
    let mut w = world::load(&t.state).unwrap();
    let st = world::settings(&w);
    let pantheon = fn7_pantheon::load();
    let (a, b) = (w.bodies_of_seat(1).next().unwrap().id, w.bodies_of_seat(2).next().unwrap().id);
    let at = w.demigods[a.ix()].pos;
    w.demigods[b.ix()].pos = fn7_society::geo::destination(at, 90.0, 1.0);
    let start = 1_800_000_000_000i64;
    let bout = duel::Bout::new_in(&w, duel::Duel::new(&w, 2, a, b, "regions", None, start, &[]));
    let p0 = &bout.panels[0];
    assert!(p0["a"]["typing_script"].is_object() && p0["b"]["typing_script"].is_object(), "both type in window 0");
    assert_eq!(bout.ai.as_ref().unwrap().pending.len(), 2);
    let (ea, eb, close) = (p0["a"]["typing_script"]["ends_at_ms"].as_i64().unwrap(), p0["b"]["typing_script"]["ends_at_ms"].as_i64().unwrap(),
                           p0["input_closes_ms"].as_i64().unwrap());
    assert!(ea.max(eb) + duel_ai::GUARD_MS <= close);
    // A resolve whose clock is 12 s in: not over (no "auto" at once); both struck in every window closed so far.
    w.dm.duels = vec![serde_json::to_value(&bout).unwrap()];
    let mut w1 = w.clone();
    let acts = duel::Acts { now_ms: start + 14_000, ..Default::default() };
    let r = duel::process(&mut w1, 2, &acts, &pantheon, &st);
    assert!(r.ended.is_empty() || r.ended[0].reason == "ko", "two typing AIs do not end at once");
    if let Some(open) = r.open.first() {
        let log = open["log"].as_array().unwrap();
        let by = |d: DemigodId| log.iter().filter(|e| e["by"] == json!(Ref::Demigod(d)) && e["ai"] == true).count();
        assert!(by(a) >= 2 && by(b) >= 2 && log.iter().all(|e| e.get("counter").is_none()), "{} and {} blows", by(a), by(b));
        assert_eq!(by(a) + by(b), log.len());
        // The same resolve again (a replay): the same state.
        let mut w1b = w.clone();
        duel::process(&mut w1b, 2, &acts, &pantheon, &st);
        assert_eq!(world::save(&w1b), world::save(&w1));
    }
    // Nobody ticks and the batch has no clock: at its last beat the duel is typed out to the deadline or a knockout.
    let mut w2 = w.clone();
    let ends_beat = bout.duel.ends_beat;
    assert!(duel::process(&mut w2.clone(), ends_beat, &duel::Acts::default(), &pantheon, &st).ended.is_empty(), "not before its deadline");
    let r = duel::process(&mut w2, ends_beat + 1, &duel::Acts::default(), &pantheon, &st);
    let end = &r.ended[0];
    assert!(matches!(end.reason, "ko" | "deadline"), "{}", end.reason);
    assert!(end.bout.n >= 4 && end.bout.ai.as_ref().unwrap().struck == end.bout.n && end.bout.typed == 0);
    assert_eq!(end.bout.panels.last().unwrap()["kind"], "outro");
    assert!(end.bout.blows.iter().all(|e| e["ai"] == true && e["auto"] == false));
    // No clock (a simulation): no scripts, fought automatically at once.
    let plain = duel::Bout::new_in(&w, duel::Duel::new(&w, 2, a, b, "regions", None, 0, &[]));
    assert!(plain.ai.is_none() && plain.cycle.is_none());
    let mut w3 = w.clone();
    w3.dm.duels = vec![serde_json::to_value(&plain).unwrap()];
    let r = duel::process(&mut w3, 2, &duel::Acts::default(), &pantheon, &st);
    assert!(matches!(r.ended[0].reason, "auto" | "ko") && r.ended[0].bout.blows.iter().all(|e| e["auto"] == true));
}

// ---------------------------------------------------------------------------------------------------------------------
// Balance: scripted human typists against the AI
// ---------------------------------------------------------------------------------------------------------------------

/// A human-like typist: what it types, how fast, and how long it thinks between lines.
#[derive(Clone, Copy)]
pub(crate) struct Typist {
    pub name: &'static str,
    pub lines: &'static [&'static str],
    pub cps: f64,
    pub think_ms: i64,
}

const GOOD: &[&str] = &[
    "I kick your knee into the mud", "I smash your jaw with my elbow", "I hook your chin then jab your nose", "I sweep your ankle by the river",
    "I punch your gut then kick your shin", "I headbutt your temple in the rain", "I stab at your ribs from the shadow", "I slam your skull on the stone wall",
    "I whip your ear then stomp your toes", "I feint high and slash your thigh", "I parry and strike your wrist", "I lunge and pierce your shoulder in the dust",
];
const PLAIN: &[&str] = &["I hit you", "take that", "I attack you again", "die already", "ok I hit you hard", "have another one", "I swing at you", "and again"];

/// (5 characters a second is 60 words a minute.)
pub(crate) const TYPISTS: &[Typist] = &[
    Typist { name: "good lines, 7 cps (84 wpm)", lines: GOOD, cps: 7.0, think_ms: 700 },
    Typist { name: "good lines, 5 cps (60 wpm)", lines: GOOD, cps: 5.0, think_ms: 1000 },
    Typist { name: "good lines, 3.5 cps (42 wpm)", lines: GOOD, cps: 3.5, think_ms: 1000 },
    Typist { name: "plain short lines, 5 cps", lines: PLAIN, cps: 5.0, think_ms: 1000 },
    Typist { name: "types nothing", lines: &[], cps: 1.0, think_ms: 0 },
];

/// Fight `n` duels of the human (seat 0) against the AI (seat 1) on `t`'s world: the human types its lines one after
/// another (a seeded order, each taking its length at its speed plus the think time), its client ticks every window.
/// `challenger`: the human challenged (it opens with the initiative). Returns (human wins, AI wins, draws, knockouts,
/// mean AI blows, mean human blows).
pub(crate) fn fight(t: &Table, who: &Typist, challenger: bool, n: u32, settings: Value) -> (u32, u32, u32, u32, f64, f64) {
    let me = if challenger { 0 } else { 1 };
    let bouts = duels(t, who, challenger, n, settings, STATE_SCHEMA);
    let (mut win, mut lose, mut draw, mut kos, mut ai_blows, mut my_blows) = (0, 0, 0, 0, 0u32, 0u32);
    for bout in &bouts {
        ai_blows += bout.ai.as_ref().map(|p| p.struck).unwrap_or(0);
        my_blows += bout.typed;
        kos += bout.ko.is_some() as u32;
        match bout.result() {
            Some((x, _)) if x == me => win += 1,
            Some(_) => lose += 1,
            None => draw += 1,
        }
    }
    (win, lose, draw, kos, ai_blows as f64 / n as f64, my_blows as f64 / n as f64)
}

/// The `n` duels of [`fight`], fought to their end (`schema`: the state schema the world is in).
pub(crate) fn duels(t: &Table, who: &Typist, challenger: bool, n: u32, settings: Value, schema: u32) -> Vec<duel::Bout> {
    let mut w = world::load(&t.state).unwrap();
    w.meta.schema = schema;
    if let (Some(o), Some(x)) = (w.meta.settings.as_object_mut(), settings.as_object()) {
        o.extend(x.clone());
    }
    let st = world::settings(&w);
    let start = 1_800_000_000_000i64;
    let mut out = vec![];
    for k in 0..n {
        let (a, b) = if challenger { (t.me, t.foe) } else { (t.foe, t.me) };
        let me = if challenger { 0 } else { 1 };
        let id = format!("duel:s{}:{}:{}", 1000 + k, a.0, b.0);
        let d = duel::Duel::with_id(&w, id, 2, a, b, "regions", challenger.then_some(0), start, &[]);
        let mut bout = duel::Bout::new_in(&w, d);
        let mut r = fn7_model::seed::Rng::derive(k as u64, &[0xBA1A, challenger as u64]);
        let mut order: Vec<&str> = who.lines.to_vec();
        for i in (1..order.len()).rev() {
            order.swap(i, r.below(i as u64 + 1) as usize);
        }
        let type_ms = |l: &str, r: &mut fn7_model::seed::Rng| who.think_ms + r.below(600) as i64 + (l.chars().count() as f64 * 1000.0 / who.cps) as i64;
        let mut next = order.first().map(|l| start + 600 + type_ms(l, &mut r));
        let (mut said, mut seq, mut guard) = (0usize, 1u64, 0);
        while panel::running(&bout) && guard < 200 {
            guard += 1;
            let close = bout.cycle.as_ref().unwrap().closes_ms;
            while let Some(at) = next.filter(|at| *at <= close && bout.ko.is_none()) {
                panel::advance(&mut bout, &w, &st, at, false);
                seq += 1;
                bout.step(&w, &st, me, order[said % order.len()], at, seq);
                said += 1;
                next = Some(at + type_ms(order[said % order.len()], &mut r));
            }
            let now = if close > 0 { close + 30 } else { bout.cycle.as_ref().unwrap().next_ms };
            panel::advance(&mut bout, &w, &st, now, true);
        }
        out.push(bout);
    }
    out
}

/// The balance the designer asked for, at the default settings: a human who types good lines beats the AI more often
/// than not, one who types nothing loses.
#[test]
fn a_good_typist_beats_the_ai_and_an_idle_one_loses() {
    let t = table(67, json!({ "duel_ai_lines_per_window": 1.0 }));
    for challenger in [true, false] {
        let good = fight(&t, &TYPISTS[0], challenger, 120, json!({}));
        assert!(good.0 * 100 >= 55 * 120, "a fast typist of good lines wins more often than not (challenger {challenger}): {good:?}");
        let avg = fight(&t, &TYPISTS[1], challenger, 120, json!({}));
        assert!(avg.0 > avg.1, "so does an average one (challenger {challenger}): {avg:?}");
        let idle = fight(&t, &TYPISTS[4], challenger, 120, json!({}));
        assert!(idle.0 == 0 && idle.1 * 100 >= 97 * 120, "typing nothing loses (challenger {challenger}): {idle:?}");
        assert!(good.4 >= 3.0 && idle.4 >= 5.0, "the AI strikes every window: {} and {} blows a duel", good.4, idle.4);
    }
}

/// `cargo test -p fn7-plugin print_ai_duel_balance -- --ignored --nocapture`: the win rates in the report and
/// docs/DM_RULEBOOK.md (400 duels a row: 200 as the challenger, 200 as the challenged).
#[test]
#[ignore]
fn print_ai_duel_balance() {
    let t = table(67, json!({ "duel_ai_lines_per_window": 1.0 }));
    let d = Settings::default();
    for (label, s) in [
        (format!("defaults (aggression {}, handicap {}, a line every window)", d.duel_ai_aggression, d.duel_ai_handicap), json!({})),
        ("handicap 0".into(), json!({ "duel_ai_handicap": 0 })),
        ("handicap 3".into(), json!({ "duel_ai_handicap": 3 })),
        ("handicap 4".into(), json!({ "duel_ai_handicap": 4 })),
        ("handicap 6".into(), json!({ "duel_ai_handicap": 6 })),
        ("handicap 7".into(), json!({ "duel_ai_handicap": 7 })),
        ("aggression 0.2".into(), json!({ "duel_ai_aggression": 0.2 })),
        ("aggression 1.0".into(), json!({ "duel_ai_aggression": 1.0 })),
        ("a line in half the windows".into(), json!({ "duel_ai_lines_per_window": 0.5 })),
    ] {
        println!("\n{label}");
        println!("| human typist | wins | AI wins | draws | knockouts | AI blows / duel | human blows / duel |");
        for who in TYPISTS {
            let (x, y) = (fight(&t, who, true, 200, s.clone()), fight(&t, who, false, 200, s.clone()));
            let pct = |v: u32| format!("{:.0}%", v as f64 / 4.0);
            println!("| {} | {} | {} | {} | {} | {:.1} | {:.1} |", who.name, pct(x.0 + y.0), pct(x.1 + y.1), pct(x.2 + y.2), pct(x.3 + y.3),
                     (x.4 + y.4) / 2.0, (x.5 + y.5) / 2.0);
        }
    }
}

/// `cargo test -p fn7-plugin print_ai_duel_examples -- --ignored --nocapture`: the shapes in docs/CLIENT_API.md.
#[test]
#[ignore]
fn print_ai_duel_examples() {
    let t = typing_table(61, json!({ "duel_damage_scale": 1.0 }));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let first = events(&lock);
    println!("SCRIPT\n{}", serde_json::to_string(&first[3]).unwrap());
    let close = first[2]["input_closes_ms"].as_i64().unwrap() - t0();
    let p1 = submit(&t, &instant(0, tick(&id), 2, close + 30)).unwrap();
    for e in events(&p1) {
        let mut e = e.clone();
        if e["event"] == "duel.panel" {
            for s in ["a", "b"] {
                e[s]["segs"] = json!("...");
            }
        }
        if e["event"] == "duel.blow" {
            e["choreo"] = json!("...");
            e["anim"] = json!("...");
        }
        println!("{}\n{}", e["event"].as_str().unwrap().to_uppercase(), serde_json::to_string(&e).unwrap());
    }
    println!("PRESENT.typing_scripts\n{}", serde_json::to_string(&t.p.public_view(&t.state)["present"]["duels"][0]["typing_scripts"]).unwrap());
}
