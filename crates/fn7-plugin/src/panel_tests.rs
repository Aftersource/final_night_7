//! Duel panels (rules 0.8.2, state schema 9): the scene and panel 0 at the lock, the paced cycle on stamps and ticks,
//! live = resolve = replay, and the phase-1 guarantees (present, restart, pause) extended to panels.

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::duel_tests::*;
use crate::*;

fn tick(id: &str) -> Value {
    json!({ "duel_tick": { "duel_id": id } })
}

fn panels_of(p: &FinalNight7Plugin, state: &Value) -> Vec<Value> {
    p.public_view(state)["present"]["duels"][0]["panels"].as_array().cloned().unwrap_or_default()
}

/// Every panel is well formed: the vocabulary, the timing chain, tracks that fill the panel.
fn check(panels: &[Value], scene: &Value) {
    let ids: Vec<&str> = scene["objects"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap()).collect();
    for (k, p) in panels.iter().enumerate() {
        assert_eq!((p["event"].as_str(), p["n"].as_u64()), (Some("duel.panel"), Some(k as u64)), "{p}");
        let (starts, play) = (p["starts_at_ms"].as_i64().unwrap(), p["play_ms"].as_i64().unwrap());
        assert!((2400..=3600).contains(&play), "play_ms {play}");
        if let Some(c) = p["input_closes_ms"].as_i64() {
            assert!(c < starts + play && c == starts + play - panel::LEAD_MS, "the input closes a render lead before the panel ends: {p}");
        }
        if k > 0 {
            let prev = &panels[k - 1];
            assert_eq!(starts, prev["starts_at_ms"].as_i64().unwrap() + prev["play_ms"].as_i64().unwrap(), "panel {k} starts when {} ends", k - 1);
        }
        assert!(panel::SHOTS.contains(&p["shot"].as_str().unwrap()), "{}", p["shot"]);
        assert!(!p["caption"].as_str().unwrap().is_empty());
        for s in ["a", "b"] {
            let f = &p[s];
            let segs = f["segs"].as_array().unwrap();
            assert!(!segs.is_empty());
            assert_eq!(segs.iter().map(|x| x["ms"].as_i64().unwrap()).sum::<i64>(), play, "{s}'s track fills panel {k}: {segs:?}");
            for seg in segs {
                assert!(panel::CLIPS.contains(&seg["clip"].as_str().unwrap()), "clip {}", seg["clip"]);
                if let Some(path) = seg["path"].as_str() {
                    assert!(panel::PATHS.contains(&path), "path {path}");
                    for key in ["over", "under", "on"] {
                        if let Some(o) = seg[key].as_str() {
                            assert!(ids.contains(&o), "{path} names a scene object: {o}");
                        }
                    }
                    assert!(seg["to"].is_array(), "{seg}");
                }
            }
            assert_eq!(f["line_masked_len"].as_u64().unwrap() as usize, f["line"].as_str().map(|l| l.chars().count()).unwrap_or(0));
        }
        for c in p["contacts"].as_array().unwrap() {
            let t = c["t_ms"].as_i64().unwrap();
            assert!(t >= 0 && t < play && matches!(c["by"].as_str(), Some("a" | "b")), "{c}");
            let on = c["on"].as_str().unwrap();
            assert!(on == "a" || on == "b" || ids.contains(&on), "a contact lands on a fighter or a scene object: {c}");
            assert!(c["limb"].is_string() && c["impulse"].is_number());
        }
        for k in ["hp", "range", "initiative", "flip", "blows"] {
            assert!(!p[k].is_null(), "{k}");
        }
    }
}

#[test]
fn a_lock_builds_the_scene_and_panel_zero_and_the_cycle_runs_on_stamps() {
    let t = table(41, json!({ "duel_deadline_ms": 30000, "duel_damage_scale": 0.1 }));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    // The lock carries the scene and panel 0.
    let also = lock["also"].as_array().expect("also[]");
    let (scene, p0) = (&also[0], &also[1]);
    assert_eq!((scene["event"].as_str(), scene["duel_id"].as_str(), scene["source"].as_str()), (Some("duel.scene"), Some(id.as_str()), Some("rules")));
    let objects = scene["objects"].as_array().unwrap();
    assert!((6..=10).contains(&objects.len()), "{} objects", objects.len());
    for o in objects {
        assert!(o["id"].is_string() && o["kind"].is_string() && o["at"].as_array().map(Vec::len) == Some(2) && o["affords"].is_array(), "{o}");
        for part in o["parts"].as_array().unwrap() {
            assert!(part["shape"].is_string() && part["size"].is_array() && part["offset"].is_array() && part["material"].is_string(), "{part}");
        }
        assert!(o["at"] != scene["a_start"] && o["at"] != scene["b_start"]);
    }
    assert!(objects.iter().any(|o| !o["affords"].as_array().unwrap().is_empty()), "something to vault, slide under or kick off");
    assert_eq!((p0["n"].as_u64(), p0["kind"].as_str(), p0["starts_at_ms"].as_i64(), p0["play_ms"].as_i64(), p0["input_closes_ms"].as_i64()),
               (Some(0), Some("intro"), Some(t0() + 1_000), Some(2600), Some(t0() + 1_000 + 2600 - 700)));
    // The same duel, the same scene (seeded by the match and the hex).
    let w = world::load(&t.state).unwrap();
    let again = duel::Duel::with_id(&w, id.clone(), 2, t.me, t.foe, "regions", Some(0), t0() + 1_000, &[]);
    assert_eq!(&panel::scene(&w, &again), scene);
    // A line inside window 0: a blow as in 0.8.1, at once, nothing due yet.
    let b1 = submit(&t, &instant(0, line(&id, "I kick your knee into the mud"), 2, 1_500)).unwrap();
    assert_eq!((b1["event"].as_str(), b1.get("also")), (Some("duel.blow"), None));
    // Only a duellist ticks; a tick long before the close shows nothing; one inside the grace closes the window.
    assert_eq!(t.p.validate_action(&t.state, 2, "", &tick(&id), SubmitKind::Instant), Err("not_yours".into()));
    assert_eq!(t.p.validate_action(&t.state, 0, "", &tick("duel:s99:0:1"), SubmitKind::Instant), Err("no_duel".into()));
    assert!(t.p.validate_action(&t.state, 0, "", &tick(&id), SubmitKind::Compose).is_err(), "a tick is an instant action");
    assert!(submit(&t, &instant(0, tick(&id), 3, 2_000)).is_err(), "nothing due: no event");
    let p1 = submit(&t, &instant(0, tick(&id), 4, 2_800)).unwrap();
    assert_eq!((p1["event"].as_str(), p1["n"].as_u64(), p1["kind"].as_str()), (Some("duel.panel"), Some(1), Some("exchange")));
    assert_eq!(p1["starts_at_ms"].as_i64(), Some(t0() + 3_600), "it starts when panel 0 ends: known 800 ms ahead");
    assert_eq!(p1["blows"], json!([b1["n"]]));
    assert_eq!((p1["a"]["line"].as_str(), p1["a"]["line_masked_len"].as_u64()), (Some("I kick your knee into the mud"), Some(29)));
    assert_eq!(p1["hp"], b1.get("hp_after").unwrap_or(&b1["hp"]).clone(), "the panel shows what the blows decided");
    assert_eq!(p1["caption"], b1["flavor"]);
    // The opponent's second tick for the same close changes nothing.
    assert!(submit(&t, &instant(1, tick(&id), 5, 2_950)).is_err());
    // Nobody ticks for a while: the next line closes every window its stamp has passed (idle panels: circle and guard).
    let b2 = submit(&t, &instant(0, line(&id, "then I smash your jaw with my elbow"), 6, 12_000)).unwrap();
    let due = b2["also"].as_array().expect("the panels that fell due");
    assert!(due.len() >= 2 && due.iter().all(|p| p["event"] == "duel.panel" && p["kind"] == "idle"), "{due:?}");
    assert!(due.iter().any(|p| p["a"]["default"] == "circle" || p["b"]["default"] == "circle") && due.iter().all(|p| p["a"]["default"].is_string() && p["b"]["default"].is_string()));
    // The present: scene + every panel, well formed.
    let pr = t.p.public_view(&t.state)["present"].clone();
    let live = pr["duels"][0]["panels"].as_array().unwrap().clone();
    assert_eq!(&pr["duels"][0]["scene"], scene);
    assert_eq!(live.len(), 2 + due.len());
    check(&live, scene);
    // The resolve builds exactly these panels from the same actions (a fresh plugin: a replay), and they are in the state.
    let acts = [instant(0, challenge(&t, "regions"), 1, 1_000), instant(0, line(&id, "I kick your knee into the mud"), 2, 1_500),
                instant(0, tick(&id), 3, 2_000), instant(0, tick(&id), 4, 2_800), instant(1, tick(&id), 5, 2_950),
                instant(0, line(&id, "then I smash your jaw with my elbow"), 6, 12_000)];
    let o = resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts);
    let w2 = world::load(&o.state).unwrap();
    assert_eq!(w2.dm.duels[0]["panels"].as_array().unwrap(), &live, "live and the resolve build the same panels");
    assert_eq!(&w2.dm.duels[0]["scene"], scene);
    assert_eq!(o.public["duels_open"][0]["panels"].as_array().unwrap(), &live, "the outcome carries the panels this resolve made due");
    assert_eq!(resolve_at(&t.p, &t, 2, &acts).state, o.state);
    // Without the ticks the content is the same; only the early close moves (panel 1 would be computed at 2900).
    let no_ticks: Vec<Action> = acts.iter().filter(|a| a.fields.get("duel_tick").is_none()).cloned().collect();
    let o2 = resolve_at(&FinalNight7Plugin::new(), &t, 2, &no_ticks);
    assert_eq!(world::load(&o2.state).unwrap().dm.duels[0]["panels"], w2.dm.duels[0]["panels"], "ticks only decide WHEN a panel is computed");
}

#[test]
fn a_knockout_plays_its_panel_and_the_outro() {
    let t = table(42, json!({ "duel_deadline_ms": 30000, "duel_damage_scale": 3.0 }));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let says = ["I kick your knee into the mud then smash your jaw", "I grab your wrist and slam your ribs on the stone wall",
                "I sweep your ankle by the river and stomp your shin", "I hook your chin then jab your nose in the rain",
                "I headbutt your temple and whip your ear with wind", "I punch your gut then kick your hip into the dust"];
    let mut acts = vec![instant(0, challenge(&t, "regions"), 1, 1_000)];
    let mut over = false;
    for (k, s) in says.iter().cycle().take(40).enumerate() {
        let a = instant(0, line(&id, s), 2 + k as u64, 1_200 + 700 * k as i64);
        match submit(&t, &a) {
            Ok(ev) => {
                acts.push(a);
                if ev["ko"] == true {
                    over = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    assert!(over, "somebody is knocked out");
    // The duel is over for lines, but its duellists still tick the panels out.
    assert_eq!(t.p.validate_action(&t.state, 0, "", &line(&id, "one more"), SubmitKind::Instant), Err("duel_over".into()));
    let last = acts.last().unwrap().received_at_ms - t0();
    let ev = submit(&t, &instant(0, tick(&id), 90, last + 4_000)).expect("the knockout's panel falls due");
    let mut all = vec![ev.clone()];
    all.extend(ev["also"].as_array().cloned().unwrap_or_default());
    let kinds: Vec<&str> = all.iter().map(|p| p["kind"].as_str().unwrap()).collect();
    assert_eq!(&kinds[kinds.len() - 2..], ["ko", "outro"], "{kinds:?}");
    let (ko, outro) = (&all[all.len() - 2], &all[all.len() - 1]);
    assert!(ko["input_closes_ms"].is_null() && outro["input_closes_ms"].is_null());
    assert!(ko["hp"]["a"] == 0.0 || ko["hp"]["b"] == 0.0);
    let loser = if ko["hp"]["a"] == 0.0 { "a" } else { "b" };
    assert!(ko[loser]["segs"].as_array().unwrap().iter().any(|s| s["clip"] == "defeat" || s["clip"] == "downed"));
    assert_eq!((outro["ko"].as_str(), outro["winner"].as_str()), (Some(loser), Some(if loser == "a" { "b" } else { "a" })));
    assert_eq!(t.p.validate_action(&t.state, 0, "", &tick(&id), SubmitKind::Instant), Err("no_duel".into()), "the cycle is done");
    let pr = t.p.public_view(&t.state)["present"].clone();
    check(pr["duels"][0]["panels"].as_array().unwrap(), &pr["duels"][0]["scene"]);
    // The resolve ends it: challenge.end carries the panels through the outro even with no tick at all.
    let o = resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts);
    let end = &o.public["duels"][0];
    assert_eq!((end["event"].as_str(), end["reason"].as_str()), (Some("challenge.end"), Some("ko")));
    let ps = end["panels"].as_array().unwrap();
    assert_eq!((ps[0]["n"].as_u64(), ps.last().unwrap()["kind"].as_str(), end["panels_total"].as_u64()), (Some(0), Some("outro"), Some(ps.len() as u64)));
    check(ps, &pr["duels"][0]["scene"]);
    assert_eq!(&ps[..], &pr["duels"][0]["panels"].as_array().unwrap()[..], "the same panels live and at the resolve");
}

/// Phase 1 carries the panels: a restart reads them back from the journal, a pause moves the schedule, the deadline
/// ends in the outro, and a replay fed the side inputs reproduces every state.
#[test]
fn panels_survive_a_restart_and_a_pause() {
    let dir = std::env::temp_dir().join(format!("fn7-panels-{}-{}", std::process::id(), theatre_trace::now_ms()));
    let mut t = table(43, json!({ "duel_deadline_ms": 12000, "duel_damage_scale": 0.1 }));
    t.p = std::sync::Arc::new(FinalNight7Plugin::new().with_journal(&dir));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 7, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let lost = [instant(0, challenge(&t, "regions"), 7, 1_000), instant(0, line(&id, "I kick your knee into the mud"), 8, 1_500),
                instant(0, tick(&id), 9, 2_900)];
    submit(&t, &lost[1]).unwrap();
    submit(&t, &lost[2]).unwrap();
    let before = panels_of(&t.p, &t.state);
    assert_eq!(before.len(), 2);
    // kill, restart
    t.p = std::sync::Arc::new(FinalNight7Plugin::new().with_journal(&dir));
    assert_eq!(panels_of(&t.p, &t.state), before, "a reconnect after a restart gets every panel so far");
    let o = resolve_at(&t.p, &t, 2, &[]);
    let w = world::load(&o.state).unwrap();
    assert_eq!(w.dm.duels[0]["panels"].as_array().unwrap(), &before, "and the first resolve has them from the recovered actions");
    let mut feed = present::Feed::default();
    feed.recovered.insert(2, lost.to_vec());
    assert_eq!(resolve_at(&FinalNight7Plugin::new().with_feed(feed), &t, 2, &[]).state, o.state, "replay");
    // A pause of a minute before beat 3: the schedule moves with the deadline.
    let s2 = block(t.p.commit_round(&o.state, 2, &[], &crate::testkit::Failing)).unwrap().state;
    let (from, to) = (t0() + 3_000, t0() + 63_000);
    let mut feed = present::Feed::default();
    feed.clocks.insert(3, present::ClockIn { now_ms: to, pause: Some([from, to]) });
    let p = FinalNight7Plugin::new().with_feed(feed);
    let rs = block(p.on_round_start(&s2, 3, &crate::testkit::Failing)).unwrap();
    let b: duel::Bout = serde_json::from_value(world::load(&rs.state).unwrap().dm.duels[0].clone()).unwrap();
    let c = b.cycle.clone().unwrap();
    let p1 = &before[1];
    assert_eq!(c.next_ms, p1["starts_at_ms"].as_i64().unwrap() + p1["play_ms"].as_i64().unwrap() + 60_000, "panel 2 starts a pause later");
    // Past the (moved) deadline the resolve ends it and plays it out to the outro.
    let t3 = Table { p: std::sync::Arc::new(FinalNight7Plugin::new()), state: rs.state.clone(), me: t.me, foe: t.foe };
    let o3 = resolve_at(&t3.p, &t3, 3, &[instant(0, line(&id, "too late now"), 20, 63_000 + 20_000)]);
    let end = &o3.public["duels"][0];
    assert_eq!(end["reason"], "deadline");
    let ps = end["panels"].as_array().unwrap();
    assert_eq!(ps.last().unwrap()["kind"], "outro");
    assert!(ps.iter().all(|p| p["n"].as_u64().unwrap() >= 2), "only the panels this resolve added");
    let _ = std::fs::remove_dir_all(dir);
}

/// A schema-8 state (this branch before panels) and older: no scene, no panels, a tick is refused.
#[test]
fn states_before_0_8_2_have_no_panels() {
    let t = table(44, json!({}));
    let mut w = world::load(&t.state).unwrap();
    w.meta.schema = 8;
    let old = Table { p: t.p.clone(), state: world::save(&w), me: t.me, foe: t.foe };
    let lock = submit(&old, &instant(0, challenge(&old, "regions"), 1, 1_000)).unwrap();
    assert!(lock.get("also").is_none());
    let id = lock["duel_id"].as_str().unwrap().to_string();
    assert_eq!(old.p.validate_action(&old.state, 0, "", &tick(&id), SubmitKind::Instant), Err("panels_off".into()));
    let o = resolve_at(&old.p, &old, 2, &[instant(0, challenge(&old, "regions"), 1, 1_000)]);
    let b = world::load(&o.state).unwrap().dm.duels[0].clone();
    assert!(b.get("panels").is_none() && b.get("scene").is_none() && b.get("cycle").is_none());
    assert!(o.public["duels_open"][0].get("panels").is_none());
}

/// `cargo test -p fn7-plugin print_panel_examples -- --ignored --nocapture`: the shapes in docs/CLIENT_API.md.
#[test]
#[ignore]
fn print_panel_examples() {
    let t = table(41, json!({ "duel_damage_scale": 0.5 }));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let mut scene = lock["also"][0].clone();
    let n = scene["objects"].as_array().unwrap().len();
    scene["objects"].as_array_mut().unwrap().truncate(2);
    println!("SCENE ({n} objects, 2 shown)\n{}", serde_json::to_string(&scene).unwrap());
    println!("PANEL0\n{}", serde_json::to_string(&lock["also"][1]).unwrap());
    submit(&t, &instant(0, line(&id, "I kick your knee into the mud"), 2, 1_500)).unwrap();
    submit(&t, &instant(1, line(&id, "I duck under it and roll away"), 3, 2_200)).unwrap();
    let p1 = submit(&t, &instant(0, tick(&id), 4, 2_900)).unwrap();
    println!("PANEL1\n{}", serde_json::to_string(&p1).unwrap());
}
