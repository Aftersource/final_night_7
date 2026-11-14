//! Exact resume (state schema 8; docs/TIME_TRAVEL.md): the present moment is in every public view, a duel's whole
//! timeline is in the state, the journal survives the process, and the side inputs replay.

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::duel_tests::*;
use crate::testkit::Failing;
use crate::*;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("fn7-{name}-{n}-{}", std::process::id()))
}

fn present(p: &FinalNight7Plugin, state: &Value) -> Value {
    p.public_view(state)["present"].clone()
}

/// What a reconnecting client is handed for a duel, minus the clock (it reads the wall).
fn duel_of(present: &Value) -> Value {
    let mut d = present["duels"][0].clone();
    d.as_object_mut().unwrap().remove("clock");
    d
}

/// (a) A client that drops and resumes mid-duel gets the duel as it stands now in `public_state.present`: both
/// fighters, hp, range/initiative/stances, every blow so far (choreography and stamps), the deadline and the server's
/// clock; plus the public events since the last resolve (here also a summoning). Across a resolve the earlier blows
/// come from the state, the later ones from the journal, and the timeline is one.
#[test]
fn a_reconnect_mid_duel_gets_the_exact_present() {
    let t = table(34, json!({ "duel_deadline_ms": 9000, "duel_damage_scale": 0.1 }));
    assert_eq!(present(&t.p, &t.state)["duels"], json!([]));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let b1 = submit(&t, &instant(0, line(&id, "I kick your knee into the mud"), 2, 2_000)).unwrap();
    let before = theatre_trace::now_ms();
    let p = present(&t.p, &t.state);
    assert!(p["server_time_ms"].as_i64().unwrap() >= before, "the server's clock: {p}");
    assert_eq!(p["beat"], 2);
    let ev: Vec<&str> = p["events"].as_array().unwrap().iter().map(|e| e["event"].as_str().unwrap()).collect();
    assert_eq!(ev, ["challenge.lock", "duel.blow"], "the mid-beat events, in order");
    assert_eq!((p["events"][0]["seq"].as_u64(), p["events"][1]["received_at_ms"].as_i64()), (Some(1), Some(t0() + 2_000)));
    let d = &p["duels"][0];
    assert_eq!(d["duel_id"], id.as_str());
    assert_eq!((d["a"]["demigod"].clone(), d["b"]["demigod"].clone()), (json!(Ref::Demigod(t.me)), json!(Ref::Demigod(t.foe))));
    assert_eq!(d["lock"]["event"], "challenge.lock");
    for k in ["hp", "range", "initiative", "stances", "blows", "deadline_ms", "started_ms", "ends_beat", "clock", "n", "typed"] {
        assert!(!d[k].is_null(), "{k} in {d}");
    }
    assert_eq!(d["blows"].as_array().unwrap().len(), 1);
    for k in ["n", "roll", "total", "verdict", "damage", "hp", "choreo", "at_ms", "seq", "text"] {
        assert_eq!(d["blows"][0][k], b1[k], "{k}: the blow as it was shown live");
    }
    assert_eq!(d["hp"], b1.get("hp_after").unwrap_or(&b1["hp"]).clone());
    assert_eq!((d["started_ms"].as_i64(), d["deadline_ms"].as_i64()), (Some(t0() + 1_000), Some(t0() + 10_000)));
    assert_eq!(d["clock"]["elapsed_ms"].as_i64().unwrap() + d["clock"]["remaining_ms"].as_i64().unwrap(), 9_000);
    // Beat 2 resolves with what it was handed; the duel stays open, its timeline now in the state.
    let acts2 = [instant(0, challenge(&t, "regions"), 1, 1_000), instant(0, line(&id, "I kick your knee into the mud"), 2, 2_000)];
    let o2 = resolve_at(&t.p, &t, 2, &acts2);
    let w2 = world::load(&o2.state).unwrap();
    assert_eq!(w2.dm.mark.map(|m| (m.at_ms, m.seq, m.resolved_ms)), Some((t0() + 2_000, 2, t0() + 2_000)), "the delivery mark");
    assert_eq!(w2.dm.duels[0]["blows"].as_array().map(Vec::len), Some(1), "every blow so far is in the state");
    let after = present(&t.p, &o2.state);
    assert_eq!(after["events"], json!([]), "nothing is pending after the resolve");
    assert_eq!(duel_of(&after)["blows"], duel_of(&p)["blows"], "the same timeline, now from the state");
    // A fresh plugin (another process, a fork, the admin's state view) reads the same duel from the state alone.
    assert_eq!(duel_of(&present(&FinalNight7Plugin::new(), &o2.state)), duel_of(&after));
    // A line that arrives while the beat resolves, then one in beat 3: the present is state + journal.
    let mid = submit(&t, &instant(0, line(&id, "I sweep your ankle by the river"), 3, 3_000)).unwrap();
    let t3 = next_beat(&t, &o2, 2);
    let b3 = submit(&t3, &instant(0, line(&id, "then I smash your jaw with my elbow"), 10, 5_000)).unwrap();
    let p3 = present(&t3.p, &t3.state);
    let blows = p3["duels"][0]["blows"].as_array().unwrap();
    assert_eq!(blows.iter().map(|b| b["n"].as_u64().unwrap()).collect::<Vec<_>>(),
               [b1["n"].as_u64().unwrap(), mid["n"].as_u64().unwrap(), b3["n"].as_u64().unwrap()], "one timeline across the resolve");
    assert_eq!((blows[1]["choreo"].clone(), blows[2]["choreo"].clone()), (mid["choreo"].clone(), b3["choreo"].clone()));
    assert_eq!(p3["events"].as_array().unwrap().len(), 2, "only what the state has not been handed");
    assert_eq!(p3["duels"][0]["hp"], b3.get("hp_after").unwrap_or(&b3["hp"]).clone());
    // The query is the same object (a viewer's follow-up over HTTP or the socket).
    let q = t3.p.query(&t3.state, None, "present", &json!({})).unwrap();
    assert_eq!(duel_of(&q), duel_of(&p3));
    assert!(t3.p.queries().iter().any(|s| s.name == "present" && s.public));
}

/// The other instant actions of a beat are in the present too (a cast's streak and a summoning), with their stamps.
#[test]
fn casts_and_summonings_since_the_last_resolve_are_in_the_present() {
    let t = table(35, json!({}));
    let w = world::load(&t.state).unwrap();
    let to = fn7_society::geo::destination(w.demigods[t.me.ix()].pos, 10.0, 30.0);
    let summon = instant(0, json!({ "summon": { "demigod": Ref::Demigod(t.me), "target": to } }), 4, 500);
    let ev = submit(&t, &summon).expect("a summoning is shown at once");
    let p = present(&t.p, &t.state);
    let got = &p["events"][0];
    assert_eq!((got["event"].as_str(), got["seq"].as_u64(), got["received_at_ms"].as_i64(), got["beat"].as_u64()),
               (Some("summoning"), Some(4), Some(t0() + 500), Some(2)));
    assert_eq!(got["to"], ev["to"]);
    // Delivered: gone from the pending events.
    let o = resolve_at(&t.p, &t, 2, &[summon]);
    assert_eq!(present(&t.p, &o.state)["events"], json!([]));
}

/// (b) The server dies (the plugin and the core's queue with it) after a challenge and two lines the last commit never
/// saw. The restarted plugin reads its journal: the present shows the same duel with the same blows, the first resolve
/// is handed the lost actions (`journal_recovered`) and gives the state the dead process would have reached, and a
/// replay fed that recorded side input reproduces it.
#[test]
fn a_restart_recovers_the_duel_from_the_journal() {
    let dir = temp_dir("journal");
    let mut t = table(36, json!({ "duel_deadline_ms": 9000, "duel_damage_scale": 0.1 }));
    let live = std::sync::Arc::new(FinalNight7Plugin::new().with_journal(&dir));
    t.p = live.clone();
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 7, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let lost = [instant(0, challenge(&t, "regions"), 7, 1_000), instant(0, line(&id, "I kick your knee into the mud"), 8, 2_000),
                instant(0, line(&id, "then I smash your jaw with my elbow"), 9, 3_200)];
    submit(&t, &lost[1]).unwrap();
    submit(&t, &lost[2]).unwrap();
    let before = duel_of(&present(&t.p, &t.state));
    assert_eq!(before["blows"].as_array().map(Vec::len), Some(2));
    // What the dead process would have resolved, had its core delivered them.
    let would = resolve_at(&FinalNight7Plugin::new(), &t, 2, &lost);
    drop(live);
    // The restart: a new plugin on the same directory, the state as of the last commit, an empty queue.
    let p2 = FinalNight7Plugin::new().with_journal(&dir);
    assert_eq!(duel_of(&present(&p2, &t.state)), before, "the same duel, blows, hp and deadline after the restart");
    // The core's `seq` restarts at 1: a fresh line continues the same duel after the recovered ones.
    t.p = std::sync::Arc::new(p2);
    let next = submit(&t, &instant(0, line(&id, "I sweep your ankle by the river"), 1, 4_500)).unwrap();
    assert_eq!(next["n"].as_u64(), Some(before["n"].as_u64().unwrap() + 1));
    let fresh = [instant(0, line(&id, "I sweep your ankle by the river"), 1, 4_500)];
    let o = resolve_at(&t.p, &t, 2, &fresh);
    let all: Vec<Action> = lost.iter().chain(&fresh).cloned().collect();
    assert_eq!(o.state, resolve_at(&FinalNight7Plugin::new(), &t, 2, &all).state, "nothing accepted was lost");
    assert_ne!(o.state, would.state);
    let w = world::load(&o.state).unwrap();
    assert_eq!(w.dm.duels[0]["blows"].as_array().map(Vec::len), Some(3));
    // The next resolve does not hand them over again.
    let t3 = next_beat(&t, &o, 2);
    let o3 = resolve_at(&t3.p, &t3, 3, &[]);
    assert_eq!(o3.state, resolve_at(&FinalNight7Plugin::new(), &t3, 3, &[]).state);
    // Replay: no journal, the recorded side input instead.
    let mut feed = present::Feed::default();
    feed.recovered.insert(2, lost.to_vec());
    let replay = FinalNight7Plugin::new().with_feed(feed);
    assert_eq!(resolve_at(&replay, &t, 2, &fresh).state, o.state, "the replay reproduces the recovery");
    let _ = std::fs::remove_dir_all(dir);
}

/// A pause (the room stood still: nobody connected, or the server was down) stops an open duel's clock: its deadline
/// moves by the pause, at the next Start, from the recorded side input.
#[test]
fn a_pause_stops_the_duels_clock() {
    let t = table(37, json!({ "duel_deadline_ms": 9000, "duel_damage_scale": 0.1 }));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let acts = [instant(0, challenge(&t, "regions"), 1, 1_000), instant(0, line(&id, "I kick your knee into the mud"), 2, 2_000)];
    let o2 = resolve_at(&t.p, &t, 2, &acts);
    let s2 = block(t.p.commit_round(&o2.state, 2, &[], &Failing)).unwrap().state;
    // The room paused 2.5 s after the lock and came back a minute later.
    let (from, to) = (t0() + 3_500, t0() + 63_500);
    let mut feed = present::Feed::default();
    feed.clocks.insert(3, present::ClockIn { now_ms: to, pause: Some([from, to]) });
    let p = FinalNight7Plugin::new().with_feed(feed);
    let rs = block(p.on_round_start(&s2, 3, &Failing)).unwrap();
    assert_eq!(rs.public["duel_clock"]["pause"]["ms"], 60_000);
    let w = world::load(&rs.state).unwrap();
    let b: duel::Bout = serde_json::from_value(w.dm.duels[0].clone()).unwrap();
    assert_eq!((b.duel.deadline_ms, b.pauses.clone()), (t0() + 10_000 + 60_000, vec![[from, to]]));
    assert_eq!(b.clock(to + 500), Some((3_000, 6_000)), "2.5 s before the pause + 0.5 s after it; 6 s left");
    assert_eq!(w.dm.mark.map(|m| m.round_ms), Some(to));
    // A line 4 s after the return is inside the moved deadline (it would have been 55 s late).
    let t3 = Table { p: std::sync::Arc::new(FinalNight7Plugin::new()), state: rs.state.clone(), me: t.me, foe: t.foe };
    let o3 = resolve_at(&t3.p, &t3, 3, &[instant(0, line(&id, "I sweep your ankle by the river"), 5, 67_500)]);
    assert!(o3.public.get("duels").is_none(), "still under way: {}", o3.public["duels"]);
    assert_eq!(o3.public["duels_open"][0]["typed"], 2);
    // Without the side input (a simulation) nothing moves.
    let plain = block(FinalNight7Plugin::new().on_round_start(&s2, 3, &Failing)).unwrap();
    assert_eq!(world::load(&plain.state).unwrap().dm.duels[0]["duel"]["deadline_ms"], t0() + 10_000);
}

/// A schema-7 state (rules 0.8.1 as recorded) keeps its shapes: no mark, no kept blows, no `present`.
#[test]
fn older_states_are_untouched() {
    let t = table(38, json!({ "duel_deadline_ms": 9000 }));
    let mut w = world::load(&t.state).unwrap();
    w.meta.schema = 7;
    w.dm.mark = None; // (beat 1 resolved under schema 8)
    let old = Table { p: t.p.clone(), state: world::save(&w), me: t.me, foe: t.foe };
    let lock = submit(&old, &instant(0, challenge(&old, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let acts = [instant(0, challenge(&old, "regions"), 1, 1_000), instant(0, line(&id, "I kick your knee into the mud"), 2, 2_000)];
    submit(&old, &acts[1]).unwrap();
    assert!(old.p.public_view(&old.state).get("present").is_none());
    let o = resolve_at(&old.p, &old, 2, &acts);
    let w2 = world::load(&o.state).unwrap();
    assert!(w2.dm.mark.is_none());
    let b = w2.dm.duels[0].as_object().unwrap();
    assert!(!b.contains_key("blows") && !b.contains_key("full") && !b.contains_key("pauses"), "{:?}", b.keys().collect::<Vec<_>>());
}
