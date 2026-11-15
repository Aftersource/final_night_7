//! Duel weapons (state schema 14; docs/DUEL_MOVES.md §1, §4.4-4.6): the list as data, weapon classes on items and in
//! the duel, held draws and hammer-space, projectiles on panels, clip trims on segs, and the duel sim on the live
//! stream only.

use fn7_model::manifest::{ItemKind, Tier};
use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::duel_tests::*;
use crate::*;

/// An instant action stamped `ms` (absolute: these tests never touch the shared `t0`).
fn at(seat: u8, fields: Value, seq: u64, ms: i64) -> Action {
    Action { fields, instant: true, seq, received_at_ms: ms, ..Action::new(seat, "") }
}

/// A duel table where seat 0's demigod holds `items`.
fn armed(seed: u64, items: &[(ItemKind, Tier, &str)], extra: Value) -> Table {
    let mut t = table(seed, extra);
    let mut w = world::load(&t.state).unwrap();
    let pantheon = fn7_pantheon::load();
    let st = world::settings(&w);
    for (k, tier, name) in items {
        crate::wills::forge_now(&mut w, 2, t.me, *k, *tier, name, None, "a test", &pantheon, &st).unwrap();
    }
    t.state = world::save(&w);
    t
}

fn panel_of(ev: &Value) -> Option<Value> {
    if ev["event"] == "duel.panel" {
        return Some(ev.clone());
    }
    ev["also"].as_array()?.iter().find(|x| x["event"] == "duel.panel").cloned()
}

#[test]
fn the_list_file_loads_validates_and_replays() {
    let file = moves::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/duel_moves.json"))).unwrap();
    assert_eq!(file.version, moves::compiled().version, "data/duel_moves.json is the compiled list");
    assert_eq!(moves::compiled().version, moves::list()["version"].as_str().unwrap());
    assert!(file.is_compiled() && file.ruleset_json()["list"].is_null(), "a compiled list is recorded by version only");
    // Edits a designer can make: a word, a description, a trim. The version changes and the list records itself.
    let mut v = moves::list();
    v["words"][0]["words"].as_array_mut().unwrap().push(json!("bicycle kick"));
    v["clips"][0]["about"] = "knocked flying".into();
    let edited = moves::MoveList::from_value(v.clone(), "edited").unwrap();
    assert_ne!(edited.version, file.version);
    assert!(edited.ruleset_json()["list"].is_object());
    // What it refuses, and says where.
    let bad = |f: &dyn Fn(&mut Value)| {
        let mut x = moves::list();
        f(&mut x);
        moves::MoveList::from_value(x, "bad.json").unwrap_err()
    };
    assert!(bad(&|x| x["clips"][40]["fallback"] = "flying_kick".into()).contains("not one of the 35 original clips"));
    assert!(bad(&|x| x["words"][0]["clips"] = json!(["moonwalk"])).contains("moonwalk"));
    assert!(bad(&|x| x["words"][0]["class"] = "dance".into()).contains("unknown class"));
    assert!(bad(&|x| x["clips"][0]["clip"] = "moonwalk".into()).contains("not one the rig has"));
    assert!(bad(&|x| x["weapons"][0]["weapon_class"] = "laser".into()).contains("weapon class"));
    let jab = moves::list()["clips"].as_array().unwrap().iter().position(|c| c["clip"] == "jab").unwrap();
    assert!(bad(&|x| x["clips"][jab]["use_ms"] = 99_000.into()).contains("does not fit"));
    // The recorded list round-trips (a replay installs it; not here: the list is process-wide and tests run in
    // parallel), and a ruleset without one means the compiled list.
    let back = moves::MoveList::from_value(edited.ruleset_json()["list"].clone(), "recorded").unwrap();
    assert_eq!(back.version, edited.version);
    assert!(back.rows[0].words.contains(&"bicycle kick"));
    assert_eq!(moves::install_recorded(None).unwrap().version, file.version);
    assert_eq!(moves::current().version, file.version);
    // Trims: a long clip plays its used part at a speed that fits the seg; the contact falls inside it.
    let mut seg = json!({ "clip": "twohand_chop", "ms": 1200 });
    let hit = panel::trim_seg(&mut seg).unwrap();
    assert!(seg["speed"].as_f64().unwrap() <= 2.5 && seg["at"].as_i64().unwrap() >= 2684 && hit > 0 && hit < 1200, "{seg} {hit}");
    let mut short = json!({ "clip": "jab", "ms": 1730 });
    panel::trim_seg(&mut short);
    assert_eq!((short["at"].as_i64(), short["speed"].as_f64()), (Some(0), Some(1.0)));
    let mut looping = json!({ "clip": "run", "ms": 300, "loop": true });
    assert!(panel::trim_seg(&mut looping).is_none() && looping.get("speed").is_none());
}

#[test]
fn rules_for_ranged_weapons() {
    use choreo::{MoveClass::*, Range::*};
    assert_eq!(weapons::effective(CloseIn, Strike, Far, "bow"), Strike, "a bow shoots from far");
    assert_eq!(weapons::effective(CloseIn, Strike, Far, "one_handed"), CloseIn, "a sword steps in");
    assert_eq!(weapons::damage("rifle", Strike, Mid), (1.0, 0.5));
    assert_eq!(weapons::damage("rifle", Strike, choreo::Range::Grapple), (0.5, 0.0), "weak in a grapple");
    assert_eq!(weapons::damage("two_handed", Heavy, Close), (1.0, 0.5));
    assert_eq!(weapons::damage("unarmed", Strike, Mid), (1.0, 0.0));
    assert_eq!((weapons::kind_for("bow", "strike"), weapons::kind_for("staff", "invoke"), weapons::kind_for("rifle", "guard")), (Some("arrow"), Some("orb"), None));
}

/// A rocket launcher: the class on the item and in the loadout, the prop in the hand from panel 0, its clips, a rocket
/// with a blast on the panel, the contact when it lands, trims on the segs, the duel sim on the live panels only, and
/// a replay that is unchanged.
#[test]
fn a_launcher_fires_a_rocket_with_a_blast() {
    let t = armed(81, &[(ItemKind::Weapon, Tier::Artifact, "Twin Rocket Launchers")], json!({ "duel_damage_scale": 0.1 }));
    let look = t.p.public_view(&t.state)["demigods"].as_array().unwrap().iter().find(|x| x["ref"] == json!(Ref::Demigod(t.me))).unwrap()["look"].clone();
    assert_eq!(look["loadout"][0]["weapon_class"], "launcher", "{look}");
    let now = theatre_trace::now_ms();
    let c = json!({ "challenge": { "demigod": Ref::Demigod(t.me), "target": Ref::Demigod(t.foe), "level": "regions" } });
    let lock = submit(&t, &at(0, c.clone(), 1, now - 1_500)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let scene = &lock["also"][0];
    assert_eq!((scene["clip_fallbacks"]["launcher_fire"].as_str(), scene["list_version"].as_str()), (Some("heavy_charged"), Some(moves::list_version().as_str())));
    let p0 = panel_of(&lock).unwrap();
    assert_eq!((p0["a"]["weapon_class"].as_str(), p0["a"]["segs"][0]["prop"]["weapon_class"].as_str(), p0["b"]["weapon_class"].as_str()),
               (Some("launcher"), Some("launcher"), Some("unarmed")));
    assert!(p0["sim"].is_object(), "a live panel carries the duel sim");
    let b = submit(&t, &at(0, line(&id, "shoot him"), 2, now - 1_000)).unwrap();
    assert_eq!((b["move"]["weapon_class"].as_str(), b["move"]["clip"].as_str()), (Some("launcher"), Some("launcher_fire")));
    let close = p0["input_closes_ms"].as_i64().unwrap();
    let tick = submit(&t, &at(0, json!({ "duel_tick": { "duel_id": id } }), 3, close + 20)).unwrap();
    let p1 = panel_of(&tick).unwrap();
    assert!(p1["sim"].is_object());
    let rocket = p1["projectiles"].as_array().unwrap().iter().find(|x| x["kind"] == "rocket").unwrap_or_else(|| panic!("{}", p1["projectiles"])).clone();
    assert_eq!((rocket["by"].as_str(), rocket["on_hit"]["blow"].as_u64()), (Some("a"), b["n"].as_u64()));
    assert_eq!(rocket["on_hit"]["aoe_hexes"].as_array().map(Vec::len), Some(7), "the landing hex and its six neighbours");
    assert!(rocket["on_hit"]["objects"].is_array() && rocket["t_hit_ms"].as_i64() > rocket["t_fire_ms"].as_i64());
    assert_eq!(rocket["miss"], json!(b["damage"].as_f64() == Some(0.0)));
    let contact = p1["contacts"].as_array().unwrap().iter().find(|x| x["by"] == "a").unwrap().clone();
    assert_eq!((contact["ranged"].clone(), contact["t_ms"].clone()), (json!(true), rocket["t_hit_ms"].clone()), "the contact lands with the rocket");
    let fire = p1["a"]["segs"].as_array().unwrap().iter().find(|s| s["clip"] == "launcher_fire").unwrap().clone();
    assert!(fire["at"].is_i64() && fire["speed"].is_f64() && fire["prop"]["name"] == "Twin Rocket Launchers", "{fire}");
    // present.duels[] carries the sim checkpoint (a viewer joining mid-duel); its panels stay bare.
    let pd = t.p.public_view(&t.state)["present"]["duels"][0].clone();
    assert!(pd["sim"]["sim"].is_object() && pd["panels"].as_array().unwrap().iter().all(|p| p.get("sim").is_none()), "{}", pd["sim"]);
    // The resolve: the recorded outcome carries the panels bare (no sim); the state is what a fresh plugin computes.
    let acts = [at(0, c, 1, now - 1_500), at(0, line(&id, "shoot him"), 2, now - 1_000), at(0, json!({ "duel_tick": { "duel_id": id } }), 3, close + 20)];
    let o = resolve_at(&t.p, &t, 2, &acts);
    let open = &o.public["duels_open"][0];
    assert!(open["panels"].as_array().is_some_and(|ps| !ps.is_empty() && ps.iter().all(|p| p.get("sim").is_none())), "{open}");
    assert!(!o.state.to_string().contains("\"sim\""), "no sim in the state");
    assert_eq!(resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts).state, o.state, "a replay is unchanged");
}

/// Pulling out a held item is free; something not held comes from hammer-space: odds, a d20, sparks; success sets the
/// weapon class for the rest of the duel, a fumble flips the initiative; at most `duel_draws`.
#[test]
fn draws_from_the_hand_and_from_hammer_space() {
    let t = armed(82, &[(ItemKind::Weapon, Tier::Artifact, "a yew longbow"), (ItemKind::Weapon, Tier::Trinket, "Obsidian Dagger")], json!({}));
    let now = theatre_trace::now_ms();
    let c = json!({ "challenge": { "demigod": Ref::Demigod(t.me), "target": Ref::Demigod(t.foe), "level": "regions" } });
    let lock = submit(&t, &at(0, c, 1, now - 3_000)).unwrap();
    assert_eq!(panel_of(&lock).unwrap()["a"]["weapon_class"], "bow", "it wields its best");
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let b = submit(&t, &at(0, line(&id, "I draw my dagger and stab"), 2, now - 2_000)).unwrap();
    let d = &b["move"]["draw"];
    assert_eq!((d["verdict"].as_str(), d["held"].as_bool(), d["weapon_class"].as_str()), (Some("drawn"), Some(true), Some("one_handed")));
    assert_eq!((b["move"]["setup"][0].as_str(), b["move"]["prop"]["name"].as_str(), b["move"]["weapon_class"].as_str()),
               (Some("draw_from_nowhere"), Some("Obsidian Dagger"), Some("one_handed")));

    // Hammer-space, with sparks in the purse: six tries in one duel (duel_draws 3, then the line reads without it).
    // (Hammer-space draws of things not held are schema 14..20: schema 21 uses only what is held.)
    let mut t = table(83, json!({ "duel_draws": 5, "duel_damage_scale": 0.05 }));
    let mut w = world::load(&t.state).unwrap();
    w.meta.schema = 20;
    w.set_sparks(t.me, 3);
    t.state = world::save(&w);
    let now = theatre_trace::now_ms();
    let c = json!({ "challenge": { "demigod": Ref::Demigod(t.me), "target": Ref::Demigod(t.foe), "level": "regions" } });
    let lock = submit(&t, &at(0, c.clone(), 1, now - 20_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let mut acts = vec![at(0, c, 1, now - 20_000)];
    let (mut wins, mut fumbles) = (0, 0);
    for k in 0..6u64 {
        let a = at(0, line(&id, "i pull out a rocket launcher and fire"), 2 + k, now - 19_000 + 1_500 * k as i64);
        let Ok(b) = submit(&t, &a) else { break };
        acts.push(a);
        let d = b["move"]["draw"].clone();
        match d["verdict"].as_str() {
            Some("success") => {
                wins += 1;
                assert_eq!((b["move"]["weapon_class"].as_str(), b["move"]["prop"]["hammer"].as_bool()), (Some("launcher"), Some(true)));
                assert!(d["roll"].as_i64().unwrap() >= d["need"].as_i64().unwrap() || d["roll"] == 20);
            }
            Some("fumble") => {
                fumbles += 1;
                assert_eq!(b["choreo"]["initiative_before"], "b", "the fumble flipped the initiative before the blow");
            }
            Some("spent") => assert!(k >= 5, "five draws a duel"),
            other => panic!("{other:?} {d}"),
        }
        if k < 3 {
            assert_eq!((d["sparks"].as_u64(), d["tier"].as_str()), (Some(1), Some("artifact")), "paid while the purse lasts");
        } else if k < 5 {
            assert_eq!((d["sparks"].as_u64(), d["tier"].as_str()), (Some(0), Some("trinket")), "then unpaid: a trinket");
        }
        if b["ko"] == true {
            break;
        }
    }
    assert!(wins > 0 && fumbles > 0, "wins {wins}, fumbles {fumbles}");
    acts.push(at(3, json!({ "duel_tick": { "duel_id": "duel:s999999:0:1" } }), 9_999, now + 600_000));
    let o = resolve_at(&t.p, &t, 2, &acts);
    let w2 = world::load(&o.state).unwrap();
    assert_eq!(w2.demigods[t.me.ix()].sparks, 0, "three paid draws cost three sparks");
    assert_eq!(resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts).state, o.state);
}

/// A bow at mid range: arrows on the panel, a hit flies to the foe, a miss lands on an object or a hex.
#[test]
fn a_bow_shoots_arrows() {
    let t = armed(84, &[(ItemKind::Weapon, Tier::Artifact, "a yew longbow")], json!({ "duel_damage_scale": 0.05 }));
    let now = theatre_trace::now_ms();
    let c = json!({ "challenge": { "demigod": Ref::Demigod(t.me), "target": Ref::Demigod(t.foe), "level": "regions" } });
    let lock = submit(&t, &at(0, c, 1, now - 8_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let mut arrows = vec![];
    for k in 0..3u64 {
        let b = submit(&t, &at(0, line(&id, "I shoot an arrow at his heart"), 2 + k, now - 7_500 + 2_600 * k as i64)).unwrap();
        assert_eq!(b["move"]["weapon_class"], "bow");
        if let Some(p) = panel_of(&b) {
            arrows.extend(p["projectiles"].as_array().cloned().unwrap_or_default());
        }
    }
    let p = panel_of(&submit(&t, &at(0, json!({ "duel_tick": { "duel_id": id } }), 9, theatre_trace::now_ms())).unwrap());
    arrows.extend(p.and_then(|p| p["projectiles"].as_array().cloned()).unwrap_or_default());
    assert!(!arrows.is_empty());
    for a in &arrows {
        assert_eq!(a["kind"], "arrow");
        if a["miss"] == false {
            assert_eq!(a["to"], "b");
        } else {
            assert!(a["to"].is_string() || a["to"].is_array(), "a miss lands on an object or a hex: {a}");
        }
        assert!(a["arc_h"].as_f64().unwrap() > 0.0 && a.to_string().len() < 400, "a few dozen bytes: {a}");
    }
}
