//! Schema 12 beside the typing AI: the logged commands in a real duel (class, clip, setups, leaps), variety in the
//! panels, one duel per player, and the duel that ends at its deadline.

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::duel_ai_tests::{duels, Typist};
use crate::duel_tests::*;
use crate::moves::tests::LOGGED;
use crate::*;

fn tick(id: &str) -> Value {
    json!({ "duel_tick": { "duel_id": id } })
}

/// The designer's logged lines, typed in a duel: each blow's class is the words' (as far as the range, the bolts and
/// the army allow), its clip is one of the words' clips, and the panel plays it.
#[test]
fn the_logged_commands_play_their_clips_in_a_duel() {
    // The schema-12/14 mapper (schema 21 reads movement commands as positioning: duel_words_tests).
    let mut t = table(71, json!({ "duel_deadline_ms": 120000, "duel_damage_scale": 0.05 }));
    let mut w0 = world::load(&t.state).unwrap();
    w0.meta.schema = 20;
    t.state = world::save(&w0);
    let lock = submit(&t, &instant(0, challenge(&t, "people"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let mut seen = std::collections::BTreeSet::new();
    for (k, (text, class, clips, setup)) in LOGGED.iter().enumerate() {
        let at = 1_500 + 4_000 * k as i64;
        let b = submit(&t, &instant(0, line(&id, text), 2 + k as u64, at)).unwrap();
        let got: choreo::MoveClass = serde_json::from_value(b["choreo"]["a"]["move_class"].clone()).unwrap();
        let range: choreo::Range = serde_json::from_value(b["choreo"]["range_before"].clone()).unwrap();
        let w = world::load(&t.state).unwrap();
        let me = &w.demigods[t.me.ix()];
        let army = beat::own_army(&w, t.me).is_some();
        let want = moves::effective(*class, range, me.bolts > 0, army);
        let clip = b["move"]["clip"].as_str().unwrap();
        // (Invokes spend the purse's bolts as the duel goes on: accept either reading of the bolt gate.)
        assert!(got == want || (*class == choreo::MoveClass::Invoke && got == choreo::MoveClass::Taunt) || (*class == choreo::MoveClass::Command && got == choreo::MoveClass::Taunt),
                "{text:?}: {got:?}, expected {want:?} at {range:?}");
        if got == *class || matches!(class, choreo::MoveClass::Invoke) {
            assert!(clips.contains(&clip), "{text:?} plays {clip}, one of {clips:?}");
            assert_eq!(b["move"]["setup"], json!(setup), "{text:?}");
        } else {
            assert!(moves::defaults(got).contains(&clip), "{text:?} became {got:?} and plays {clip}");
        }
        assert!(panel::CLIPS.contains(&clip) && b["move"]["word"].is_string());
        assert!(b["counter"]["move"]["clip"].is_string(), "the counter has its clip too");
        seen.insert(clip.to_string());
        // The next panel stages it with that clip.
        let pr = t.p.public_view(&t.state)["present"]["duels"][0].clone();
        let close = pr["panels"].as_array().unwrap().last().unwrap()["input_closes_ms"].as_i64().unwrap() - t0();
        let p = submit(&t, &instant(0, tick(&id), 100 + k as u64, close.max(at) + 20)).unwrap();
        let mut ps = vec![p.clone()];
        ps.extend(p["also"].as_array().cloned().unwrap_or_default());
        let staged = ps.iter().find(|p| p["blows"].as_array().is_some_and(|x| x.contains(&b["n"]))).expect("staged");
        let segs = staged["a"]["segs"].as_array().unwrap();
        assert!(segs.iter().any(|s| s["clip"] == clip), "{text:?}: {clip} in {segs:?}");
        for s in setup.iter() {
            let (i, j) = (segs.iter().position(|x| x["clip"] == *s).expect("the setup"), segs.iter().position(|x| x["clip"] == clip).unwrap());
            assert!(i < j, "the setup comes first");
        }
        if b["move"]["path"] == "leap" {
            assert!(segs.iter().any(|s| s["clip"] == clip && s["path"] == "leap" && s["to"].is_array()), "{text:?} leaves the ground: {segs:?}");
        }
    }
    assert!(seen.len() >= 8, "{seen:?}");
    // Every panel is well formed (clips of the vocabulary, tracks that fill the panel, moving segs with a hex).
    let pr = t.p.public_view(&t.state)["present"]["duels"][0].clone();
    for p in pr["panels"].as_array().unwrap() {
        for s in ["a", "b"] {
            let segs = p[s]["segs"].as_array().unwrap();
            assert_eq!(segs.iter().map(|x| x["ms"].as_i64().unwrap()).sum::<i64>(), p["play_ms"].as_i64().unwrap(), "{segs:?}");
            assert!(segs.iter().all(|x| panel::CLIPS.contains(&x["clip"].as_str().unwrap()) && x["ms"].as_i64().unwrap() > 0 && (x["path"].is_null() || x["to"].is_array())));
        }
    }
    // Every new clip has a fallback among the 35 old ones.
    assert_eq!(panel::CLIP_FALLBACKS.len(), 29, "15 in use and 14 reserved for weapon classes");
    assert!(panel::CLIP_FALLBACKS.iter().all(|(new, old)| panel::CLIPS[35..].contains(new) && panel::CLIPS[..35].contains(old)));
    // A schema-11 state reads the same lines as 0.8.1 did: no `move`, the old classes.
    let mut w = world::load(&t.state).unwrap();
    w.meta.schema = 11;
    w.dm.duels.clear();
    let t2 = table(71, json!({}));
    let mut w2 = world::load(&t2.state).unwrap();
    w2.meta.schema = 11;
    let old = Table { p: t2.p.clone(), state: world::save(&w2), me: t2.me, foe: t2.foe };
    let lock = submit(&old, &instant(0, challenge(&old, "people"), 1, 1_000)).unwrap();
    let b = submit(&old, &instant(0, line(lock["duel_id"].as_str().unwrap(), "block!"), 2, 1_500)).unwrap();
    assert!(b.get("move").is_none() && b["choreo"]["a"]["move_class"] == "guard");
    let b = submit(&old, &instant(0, line(lock["duel_id"].as_str().unwrap(), "atack!!"), 3, 2_500)).unwrap();
    assert!(b.get("move").is_none() && b["choreo"]["a"]["move_class"] == "strike");
}

/// Variety: a fighter never plays the same move clip twice in a row when its list has a choice, idle time is not
/// only a held guard, and hard hits knock down and get up.
#[test]
fn panels_have_variety() {
    let t = table(72, json!({ "duel_ai_lines_per_window": 1.0 }));
    let who = Typist { name: "commands", lines: &["kick!", "punch!", "block!", "kick!", "attack!", "kick!", "smash!", "dodge!"], cps: 6.0, think_ms: 1500 };
    let bouts = duels(&t, &who, true, 40, json!({ "duel_damage_scale": 0.4 }), STATE_SCHEMA);
    let mut clips: std::collections::BTreeMap<String, u32> = Default::default();
    for b in &bouts {
        for side in 0..2 {
            let who = json!(Ref::Demigod(b.duel.f[side].id));
            let mine: Vec<&Value> = b.blows.iter().filter(|e| e["by"] == who).collect();
            for pair in mine.windows(2) {
                let (x, y) = (&pair[0]["move"], &pair[1]["move"]);
                let same_word = x["word"] == y["word"] && pair[0]["choreo"][["a", "b"][side]]["move_class"] == pair[1]["choreo"][["a", "b"][side]]["move_class"];
                let one_choice = ["flying_kick", "jump_attack", "leg_sweep", "lunge", "command", "taunt", "evade_spin", "grapple_throw_down", "uppercut", "jab", "roundhouse_kick"]
                    .contains(&y["clip"].as_str().unwrap());
                assert!(x["clip"] != y["clip"] || !same_word || one_choice, "the same clip twice in a row: {} then {} ({})", x, y, pair[1]["text"]);
            }
        }
        for p in &b.panels {
            for s in ["a", "b"] {
                for seg in p[s]["segs"].as_array().unwrap() {
                    *clips.entry(seg["clip"].as_str().unwrap().to_string()).or_default() += 1;
                }
            }
        }
    }
    let total: u32 = clips.values().sum();
    let idle = clips.get("idle_guard").copied().unwrap_or(0);
    assert!(idle * 100 < total * 40, "a held guard is under 40% of the segments: {idle} of {total}");
    for c in ["walk", "evade_stand", "block", "kick", "roundhouse_kick", "high_kick", "jab", "block_cross", "airborne_hit", "downed", "stagger_back", "stagger_waist"] {
        assert!(clips.contains_key(c), "{c} is used: {clips:?}");
    }
    assert!(clips.contains_key("rise") || clips.contains_key("spin_rise"), "{clips:?}");
    assert!(clips.len() >= 30, "{} different clips: {clips:?}", clips.len());
}

/// One duel per player: while one of a player's demigods is in an open duel, a challenge by or to any of its
/// demigods is refused `busy:player`, and the AI's chooser skips that player (and itself fights one at a time).
#[test]
fn a_player_fights_one_duel_at_a_time() {
    let t = table(73, json!({ "duel_deadline_ms": 30000 }));
    let mut w = world::load(&t.state).unwrap();
    let sib = w.siblings(t.me).into_iter().find(|x| *x != t.me).unwrap();
    let foe_sib = w.siblings(t.foe).into_iter().find(|x| *x != t.foe).unwrap();
    let third = w.bodies_of_seat(2).next().unwrap().id;
    // Everyone within reach of everyone.
    let at = w.demigods[t.me.ix()].pos;
    let fourth = w.bodies_of_seat(3).next().unwrap().id;
    for (k, d) in [sib, foe_sib, third, fourth].into_iter().enumerate() {
        w.demigods[d.ix()].pos = fn7_society::geo::destination(at, 180.0, 1.0 + k as f64);
    }
    let t = Table { p: t.p.clone(), state: world::save(&w), me: t.me, foe: t.foe };
    let c = |a: DemigodId, b: DemigodId| json!({ "challenge": { "demigod": Ref::Demigod(a), "target": Ref::Demigod(b), "level": "regions" } });
    let v = |t: &Table, f: Value, seat: u8| t.p.validate_action(&t.state, seat, "", &f, SubmitKind::Instant);
    assert_eq!(v(&t, c(sib, third), 0), Ok(()), "free before the duel");
    submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).expect("her Right Hand's duel");
    assert_eq!(v(&t, c(t.me, third), 0), Err("busy".into()), "the duellist itself: busy, as before");
    assert_eq!(v(&t, c(sib, third), 0), Err("busy:player".into()), "her other demigod cannot start a second duel");
    // The resolve refuses the same (a challenge that raced it), and the AI's chooser skips both players.
    let acts = [instant(0, challenge(&t, "regions"), 1, 1_000), instant(0, c(sib, third), 2, 2_000)];
    let o = resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts);
    let mut w = world::load(&o.state).unwrap();
    assert_eq!(w.dm.duels.len(), 1, "one duel under way");
    let mut s = w.meta.settings.clone();
    s["duel_ai_chance"] = json!(1.0);
    w.meta.settings = s;
    let st = world::settings(&w);
    let bouts = duel::open_bouts(&w);
    let her = |d: DemigodId| w.same_player(d, t.me) || w.same_player(d, t.foe);
    for beat in 3..9 {
        let cs = duel::ai_challenges(&w, beat, &st);
        assert!(!cs.is_empty(), "the AI challenges (chance 1)");
        assert!(cs.iter().all(|x| !her(x.a) && !her(x.b)), "nobody challenges a player who is duelling: {cs:?}");
        for (i, x) in cs.iter().enumerate() {
            assert!(cs.iter().skip(i + 1).all(|y| ![y.a, y.b].iter().any(|d| w.same_player(*d, x.a) || w.same_player(*d, x.b))), "an AI player fights one at a time: {cs:?}");
        }
    }
    let foe_seat = w.demigods[t.foe.ix()].seat;
    assert_eq!(duel::check_challenge(&w, 2, &actions::parse("", &c(third, foe_sib)).unwrap().challenge.unwrap(), 3, &st, &bouts, &[]),
               Err("busy:player".into()), "seat {foe_seat}'s other demigod is busy too");
    // The setting off, or an older state: only the two duellists are busy.
    let mut off = w.clone();
    off.meta.settings["duel_one_per_player"] = json!(false);
    let order = actions::parse("", &c(sib, third)).unwrap().challenge.unwrap();
    assert_eq!(duel::check_challenge(&off, 0, &order, 3, &world::settings(&off), &bouts, &[]), Ok(()));
    let mut old = w.clone();
    old.meta.schema = 11;
    assert_eq!(duel::check_challenge(&old, 0, &order, 3, &world::settings(&old), &bouts, &[]), Ok(()));
    assert!(duel::ai_challenges(&old, 3, &world::settings(&old)).iter().any(|x| her(x.a) || her(x.b)), "0.9.0: the AI may still pick her other demigod");
}

/// The duel ends AT its deadline: the first duel action stamped past it closes it (the panels left, the outro and a
/// provisional `challenge.end` are that action's event); the resolve applies the same result.
#[test]
fn the_first_action_past_the_deadline_closes_the_duel() {
    for typing in [0.0, 1.0] {
        let t = table(74, json!({ "duel_deadline_ms": 9000, "duel_damage_scale": 0.3, "duel_ai_lines_per_window": typing }));
        let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
        let id = lock["duel_id"].as_str().unwrap().to_string();
        let a1 = instant(0, line(&id, "kick!"), 2, 2_000);
        submit(&t, &a1).unwrap();
        // 10 ms before the deadline: nothing closes. 12 ms past it: a line that is no blow, but closes the duel.
        assert!(submit(&t, &instant(0, tick(&id), 3, 9_990)).map(|v| v["event"] != "challenge.end" && v["also"].as_array().is_none_or(|a| a.iter().all(|e| e["event"] != "challenge.end"))).unwrap_or(true));
        let late = if typing == 0.0 { instant(0, line(&id, "one more kick"), 4, 10_012) } else { instant(0, tick(&id), 4, 10_012) };
        let ev = submit(&t, &late).expect("the closing action has an event");
        let mut all = vec![ev.clone()];
        all.extend(ev["also"].as_array().cloned().unwrap_or_default());
        assert!(all.iter().all(|e| e["event"] != "duel.blow" || e["ai"] == true), "the late line is no blow: {all:?}");
        let end = all.last().unwrap();
        assert_eq!((end["event"].as_str(), end["provisional"].as_bool(), end["reason"].as_str(), end["duel_id"].as_str()),
                   (Some("challenge.end"), Some(true), Some("deadline"), Some(id.as_str())), "{end}");
        assert!(end["winner"].is_string() || end["winner"].is_null());
        let outro = &all[all.len() - 2];
        assert_eq!((outro["event"].as_str(), outro["kind"].as_str()), (Some("duel.panel"), Some("outro")));
        // It is over for everyone now: the present shows the outro; another tick shows nothing.
        let pr = t.p.public_view(&t.state)["present"]["duels"][0].clone();
        assert_eq!(pr["panels"].as_array().unwrap().last().unwrap()["kind"], "outro");
        assert_eq!(t.p.validate_action(&t.state, 0, "", &tick(&id), SubmitKind::Instant), Err("no_duel".into()));
        // The resolve (a replay too) ends it with the same winner and healths, and the deed.
        let acts = [instant(0, challenge(&t, "regions"), 1, 1_000), a1.clone(), instant(0, tick(&id), 3, 9_990), late.clone()];
        let o = resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts);
        let fin = &o.public["duels"][0];
        assert_eq!((fin["event"].as_str(), fin["reason"].as_str(), &fin["winner"], &fin["hp"], fin.get("provisional")),
                   (Some("challenge.end"), Some("deadline"), &end["winner"], &end["hp"], None));
        assert!(fin["deed"].as_str().is_some_and(|d| !d.is_empty()));
        assert_eq!(fin["panels_total"], end["panels_total"]);
        assert_eq!(resolve_at(&t.p, &t, 2, &acts).state, o.state);
    }
}

/// `cargo test -p fn7-plugin print_moves_report -- --ignored --nocapture`: the line → class → clips table and the
/// clip usage over simulated duels, before (a schema-11 state) and after (schema 12).
#[test]
#[ignore]
fn print_moves_report() {
    println!("| line | word | class | clips | setup |");
    for (text, ..) in LOGGED {
        let r = moves::read(text).unwrap();
        println!("| `{text}` | {} | {:?}{} | {} | {} |", r.word, r.class, if r.leap { " + leap" } else { "" }, r.clips.join(", "), r.setup.join(", "));
    }
    let t = table(72, json!({ "duel_ai_lines_per_window": 1.0 }));
    let lines: Vec<&'static str> = LOGGED.iter().map(|x| x.0).collect();
    let who = Typist { name: "the logged commands", lines: Box::leak(lines.into_boxed_slice()), cps: 5.0, think_ms: 2500 };
    for (label, schema) in [("BEFORE (schema 11)", 11), ("AFTER (schema 12)", STATE_SCHEMA)] {
        let mut clips: std::collections::BTreeMap<String, u32> = Default::default();
        let mut paths: std::collections::BTreeMap<String, u32> = Default::default();
        let mut classes: std::collections::BTreeMap<String, u32> = Default::default();
        let mut n = 0;
        for challenger in [true, false] {
            for b in duels(&t, &who, challenger, 100, json!({}), schema) {
                n += 1;
                for e in &b.blows {
                    for x in [e, &e["counter"]] {
                        let by = if x["by"] == json!(Ref::Demigod(b.duel.f[0].id)) { "a" } else { "b" };
                        if let Some(c) = x["choreo"][by]["move_class"].as_str() {
                            *classes.entry(c.to_string()).or_default() += 1;
                        }
                    }
                }
                for p in &b.panels {
                    for s in ["a", "b"] {
                        for seg in p[s]["segs"].as_array().unwrap() {
                            *clips.entry(seg["clip"].as_str().unwrap().to_string()).or_default() += 1;
                            if let Some(x) = seg["path"].as_str() {
                                *paths.entry(x.to_string()).or_default() += 1;
                            }
                        }
                    }
                }
            }
        }
        let show = |m: &std::collections::BTreeMap<String, u32>| {
            let mut v: Vec<(&String, &u32)> = m.iter().collect();
            v.sort_by_key(|x| std::cmp::Reverse(*x.1));
            v.iter().map(|(k, c)| format!("{k} {c}")).collect::<Vec<_>>().join(", ")
        };
        let total: u32 = clips.values().sum();
        println!("\n{label}: {n} duels, {total} segments, {} different clips ({}% idle_guard, {}% walk)", clips.len(),
                 clips.get("idle_guard").copied().unwrap_or(0) * 100 / total, clips.get("walk").copied().unwrap_or(0) * 100 / total);
        println!("classes: {}", show(&classes));
        println!("clips: {}", show(&clips));
        println!("paths: {}", show(&paths));
        let unused: Vec<&str> = panel::CLIPS.iter().copied().filter(|c| !clips.contains_key(*c)).collect();
        println!("never used: {}", unused.join(", "));
    }
}
