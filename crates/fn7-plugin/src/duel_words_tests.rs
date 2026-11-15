//! Duel words (state schema 21; docs/DUEL_MOVES.md §5): the instant `stage` on every `duel.blow`, lines that use only
//! what the fighter holds and what is on the board (a whiff otherwise), staged steps on the panels, throws of the
//! board and the ground, `usable` and `look` per fighter, and schema 20 untouched.

use fn7_model::manifest::{ItemKind, Tier};
use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::duel_tests::*;
use crate::*;

fn at(seat: u8, fields: Value, seq: u64, ms: i64) -> Action {
    Action { fields, instant: true, seq, received_at_ms: ms, ..Action::new(seat, "") }
}

/// A duel table at `schema` where seat 0's demigod holds `items`.
fn armed_at(seed: u64, schema: u32, items: &[(ItemKind, Tier, &str)], extra: Value) -> Table {
    let mut t = table(seed, extra);
    let mut w = world::load(&t.state).unwrap();
    w.meta.schema = schema;
    let pantheon = fn7_pantheon::load();
    let st = world::settings(&w);
    for (k, tier, name) in items {
        crate::wills::forge_now(&mut w, 2, t.me, *k, *tier, name, None, "a test", &pantheon, &st).unwrap();
    }
    t.state = world::save(&w);
    t
}

fn panels_of(ev: &Value) -> Vec<Value> {
    let mut out = vec![];
    if ev["event"] == "duel.panel" {
        out.push(ev.clone());
    }
    out.extend(ev["also"].as_array().into_iter().flatten().filter(|x| x["event"] == "duel.panel").cloned());
    out
}

fn challenge(t: &Table) -> Value {
    json!({ "challenge": { "demigod": Ref::Demigod(t.me), "target": Ref::Demigod(t.foe), "level": "regions" } })
}

/// Lock a duel at `now - back` ms: (lock event, duel id).
fn lock(t: &Table, now: i64, back: i64) -> (Value, String) {
    let lock = submit(t, &at(0, challenge(t), 1, now - back)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    (lock, id)
}

/// Each Enter plays at once: the `duel.blow` carries `stage` from the keyword fallback (no model, no panel wait).
#[test]
fn every_enter_plays_at_once() {
    let t = armed_at(91, fn7_model::STATE_SCHEMA, &[], json!({ "duel_damage_scale": 0.05 }));
    let now = theatre_trace::now_ms();
    let (_, id) = lock(&t, now, 30_000);
    let cases: &[(&str, &str, Option<&str>)] = &[
        ("roll left", "evade_roll", Some("left")),
        ("roll right", "evade_roll", Some("right")),
        ("jump", "jump_attack", Some("leap")),
        ("run in circles", "run", Some("circle")),
        ("duck", "evade_stand", None),
        ("block", "block", None),
        ("parry", "parry_high", None),
        ("charge", "run", Some("in")),
    ];
    for (k, (text, clip, path)) in cases.iter().enumerate() {
        let b = submit(&t, &at(0, line(&id, text), 2 + k as u64, now - 29_000 + 1_000 * k as i64)).unwrap();
        assert_eq!(b["event"], "duel.blow");
        let s0 = &b["stage"][0];
        assert_eq!((s0["clip"].as_str(), s0["path"].as_str()), (Some(*clip), *path), "{text}: {}", b["stage"]);
        assert!(b["verdict"].is_string() && b["roll"].as_u64().is_some(), "{text}: a d20 for it");
        if b["ko"] == true {
            break;
        }
    }
    // A plain blow is one step: its clip.
    let b = submit(&t, &at(0, line(&id, "kick"), 50, now - 20_000)).unwrap();
    assert_eq!(b["stage"][0]["clip"], b["move"]["clip"]);
    // "roll left" is positioning: an evade.
    let b = submit(&t, &at(0, line(&id, "duck roll to the left"), 51, now - 19_000)).unwrap();
    assert_eq!((b["choreo"]["a"]["move_class"].as_str(), b["stage"][0]["clip"].as_str(), b["stage"][0]["path"].as_str()),
               (Some("evade"), Some("evade_roll"), Some("left")));
}

/// Only what it holds: "draw my sword" draws THAT held sword (its name, its colour); naming one it does not hold is a
/// whiff (a fail, "you have no pistol", −2 for the line); no hammer-space.
#[test]
fn only_held_items_come_out() {
    let t = armed_at(92, fn7_model::STATE_SCHEMA, &[(ItemKind::Weapon, Tier::Relic, "Obsidian Blade")], json!({ "duel_damage_scale": 0.05 }));
    let now = theatre_trace::now_ms();
    let (lock_ev, id) = lock(&t, now, 30_000);
    // The lock says what it can use and how it looks.
    let a = &lock_ev["a"];
    assert_eq!(a["usable"]["items"][0]["name"], "Obsidian Blade", "{}", a["usable"]);
    assert!(a["look"]["aura"].as_str().is_some_and(|x| x.starts_with('#')) && a["look"]["held"][0]["colour"] == "black", "{}", a["look"]);
    // It wields the blade from the lock: a draw of it is no draw; slashing with it carries its prop.
    let b = submit(&t, &at(0, line(&id, "I slash his throat with my obsidian blade"), 2, now - 29_000)).unwrap();
    let mv = &b["move"];
    assert_eq!((mv["prop"]["name"].as_str(), mv["prop"]["colour"].as_str(), mv["prop"]["material"].as_str(), mv["uses"]["kind"].as_str()),
               (Some("Obsidian Blade"), Some("black"), Some("obsidian"), Some("item")), "{mv}");
    assert_eq!((mv["target"].as_str(), b["stage"][0]["clip"].as_str()), (Some("neck"), Some("strike_slash")), "{}", b["stage"]);
    assert!(b["tags"].as_array().unwrap().contains(&json!("uses")) && !b["tags"].as_array().unwrap().contains(&json!("item")), "{}", b["tags"]);
    // A pistol it does not hold: a whiff.
    let b = submit(&t, &at(0, line(&id, "I draw a pistol and shoot him"), 3, now - 28_000)).unwrap();
    assert!(matches!(b["verdict"].as_str(), Some("fail" | "crit_fail")), "{b}");
    assert_eq!((b["damage"].as_f64(), b["move"]["whiff"]["named"].as_str()), (Some(0.0), Some("pistol")), "{b}");
    assert!(b["flavor"].as_str().unwrap().contains("you have no pistol"), "{}", b["flavor"]);
    assert!(b["tags"].as_array().unwrap().contains(&json!("unavailable")) && b["move"]["draw"].is_null() && b["move"]["prop"]["name"] == "Obsidian Blade");
    assert_eq!(b["stage"][0]["whiff"], true);
}

/// A held weapon that is not in hand comes out (free, `drawn`), with its name and material: the draw step first.
#[test]
fn a_held_sword_is_drawn() {
    let t = armed_at(93, fn7_model::STATE_SCHEMA, &[(ItemKind::Weapon, Tier::Artifact, "a yew longbow"), (ItemKind::Weapon, Tier::Trinket, "Golden Sword")], json!({}));
    let now = theatre_trace::now_ms();
    let (lock_ev, id) = lock(&t, now, 30_000);
    assert_eq!(panels_of(&lock_ev)[0]["a"]["weapon_class"], "bow");
    let b = submit(&t, &at(0, line(&id, "I pull out a sword"), 2, now - 29_000)).unwrap();
    let mv = &b["move"];
    assert_eq!((mv["draw"]["verdict"].as_str(), mv["prop"]["name"].as_str(), mv["prop"]["colour"].as_str(), mv["prop"]["hammer"].as_bool()),
               (Some("drawn"), Some("Golden Sword"), Some("gold"), Some(false)), "{mv}");
    assert_eq!((b["stage"][0]["clip"].as_str(), b["stage"][0]["prop"]["name"].as_str()), (Some("draw_from_nowhere"), Some("Golden Sword")), "{}", b["stage"]);
}

/// The board and the ground: "hide behind that tree then pop up and slit his throat" plays four steps on the panel
/// (to the tree, crouch, rise, the slash at the neck); a tree that is not there is a whiff; dirt thrown in the eyes
/// flies as a projectile; the duel replays identically.
#[test]
fn the_board_and_the_ground() {
    let t = armed_at(94, fn7_model::STATE_SCHEMA, &[], json!({ "duel_damage_scale": 0.05 }));
    let now = theatre_trace::now_ms();
    let (lock_ev, id) = lock(&t, now, 30_000);
    let scene = lock_ev["also"].as_array().unwrap().iter().find(|x| x["event"] == "duel.scene").unwrap().clone();
    let kinds: Vec<&str> = scene["objects"].as_array().unwrap().iter().filter_map(|o| o["kind"].as_str()).collect();
    assert!(scene["fighters"]["a"]["usable"]["board"].as_array().is_some_and(|b| !b.is_empty()), "{}", scene["fighters"]["a"]["usable"]);
    assert!(scene["fighters"]["a"]["look"]["loadout"].is_array() && scene["fighters"]["b"]["look"]["aura"].is_string());
    let ground = scene["fighters"]["a"]["usable"]["ground"][0].as_str().unwrap().to_string();
    // A kind on the board, and one that is not.
    let have = ["tree", "rock", "boulder", "crate", "pillar", "wall", "table", "log", "jar"].into_iter().find(|k| kinds.contains(k)).expect("an object");
    let lack = ["cart", "barrel"].into_iter().find(|k| !kinds.contains(k)).unwrap();
    let mut acts = vec![at(0, challenge(&t), 1, now - 30_000)];
    let mut send = |text: &str, seq: u64, ms: i64| {
        let a = at(0, line(&id, text), seq, ms);
        let b = submit(&t, &a).unwrap();
        acts.push(a);
        b
    };
    let b = send(&format!("hide behind that {have} then pop up and slit his throat"), 2, now - 29_500);
    let st: Vec<(&str, Option<&str>)> = b["stage"].as_array().unwrap().iter().map(|s| (s["clip"].as_str().unwrap(), s["path"].as_str())).collect();
    assert_eq!(st, vec![("run", Some("cover")), ("kneel", None), ("rise", None), ("strike_slash", None)], "{}", b["stage"]);
    assert!(b["stage"][0]["object_id"].as_str().is_some_and(|o| o.starts_with(if have == "rock" { "" } else { have })), "{}", b["stage"]);
    assert_eq!((b["move"]["steps"].as_array().map(Vec::len), b["move"]["target"].as_str(), b["move"]["uses"]["kind"].as_str()), (Some(4), Some("neck"), Some("board")));
    let w = send(&format!("duck behind the {lack}"), 3, now - 28_000);
    assert!(w["move"]["whiff"]["where"] == "board" && w["flavor"].as_str().unwrap().contains(&format!("no {lack} here")) && (w["verdict"] == "fail" || w["verdict"] == "crit_fail"), "{w}");
    let d = send(&format!("throw {ground} in his eyes"), 4, now - 26_500);
    assert_eq!((d["move"]["throw"]["ground"].as_str(), d["move"]["target"].as_str(), d["move"]["uses"]["kind"].as_str()), (Some(ground.as_str()), Some("head"), Some("ground")), "{}", d["move"]);
    assert_eq!(d["choreo"]["a"]["move_class"], "strike", "a throw is a strike");
    let tick = at(0, json!({ "duel_tick": { "duel_id": id } }), 9, now);
    let _ = submit(&t, &tick);
    acts.push(tick);
    // The panels: the steps staged, the contact at the neck when the slash lands, the dirt flying.
    let o = resolve_at(&t.p, &t, 2, &acts);
    let w2 = world::load(&o.state).unwrap();
    let bout = crate::duel::open_bouts(&w2).into_iter().find(|x| x.duel.id == id).or_else(|| None);
    let panels: Vec<Value> = match &bout {
        Some(b) => b.panels.clone(),
        None => o.public["duels"][0]["panels"].as_array().cloned().unwrap_or_default(),
    };
    let all: Vec<Value> = panels.iter().flat_map(|p| p["a"]["segs"].as_array().cloned().unwrap_or_default()).collect();
    let stepped: Vec<&Value> = all.iter().filter(|s| s["step"].is_u64()).collect();
    assert!(stepped.iter().any(|s| s["behind"].is_string() && s["path"] == "dash") && stepped.iter().any(|s| s["clip"] == "strike_slash"), "{all:?}");
    for p in &panels {
        for s in ["a", "b"] {
            let sum: i64 = p[s]["segs"].as_array().into_iter().flatten().map(|x| x["ms"].as_i64().unwrap_or(0)).sum();
            assert_eq!(sum, p["play_ms"].as_i64().unwrap(), "the steps fit the panel: {p}");
        }
        for c in p["contacts"].as_array().into_iter().flatten() {
            if c["by"] == "a" && c["miss"] == false && c["at"] == "neck" {
                assert_eq!(c["on"], "b");
            }
        }
    }
    let dirt = panels.iter().flat_map(|p| p["projectiles"].as_array().cloned().unwrap_or_default()).find(|x| x["thing"]["ground"].is_string());
    assert!(dirt.as_ref().is_some_and(|x| x["kind"] == "dirt"), "{panels:?}");
    assert_eq!(resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts).state, o.state, "a replay is unchanged");
}

/// Schema 20 untouched: the same lines in a schema-20 duel carry no `stage`, no `look`, no `usable`, no whiff, and
/// hammer-space still works there.
#[test]
fn schema_20_is_untouched() {
    let t = armed_at(95, 20, &[], json!({ "duel_damage_scale": 0.05 }));
    let now = theatre_trace::now_ms();
    let (lock_ev, id) = lock(&t, now, 30_000);
    assert!(lock_ev["a"].get("look").is_none() && lock_ev["a"].get("usable").is_none());
    let scene = lock_ev["also"].as_array().unwrap().iter().find(|x| x["event"] == "duel.scene").unwrap();
    assert!(scene.get("fighters").is_none());
    let b = submit(&t, &at(0, line(&id, "roll left"), 2, now - 29_000)).unwrap();
    assert!(b.get("stage").is_none() && b["move"].get("steps").is_none());
    let b = submit(&t, &at(0, line(&id, "I pull out a rocket launcher"), 3, now - 27_000)).unwrap();
    assert!(b["move"]["whiff"].is_null() && b["move"]["draw"]["held"] == false, "hammer-space as before: {}", b["move"]);
    let w = world::load(&t.state).unwrap();
    assert!(!w.duel_words_rules() && fn7_model::STATE_SCHEMA == 21);
}

/// Prints one real example per case (docs): `cargo test -p fn7-plugin duel_words_examples -- --ignored --nocapture`.
#[test]
#[ignore]
fn duel_words_examples() {
    let t = armed_at(96, fn7_model::STATE_SCHEMA, &[(ItemKind::Weapon, Tier::Relic, "Obsidian Blade"), (ItemKind::Weapon, Tier::Trinket, "Golden Sword")], json!({ "duel_damage_scale": 0.05 }));
    let now = theatre_trace::now_ms();
    let (lock_ev, id) = lock(&t, now, 30_000);
    println!("LOCK.a.usable {}", lock_ev["a"]["usable"]);
    println!("LOCK.a.look {}", lock_ev["a"]["look"]);
    let scene = lock_ev["also"].as_array().unwrap().iter().find(|x| x["event"] == "duel.scene").unwrap().clone();
    let kinds: Vec<String> = scene["objects"].as_array().unwrap().iter().filter_map(|o| o["kind"].as_str().map(str::to_string)).collect();
    println!("KINDS {kinds:?}");
    let have = ["tree", "rock", "boulder", "crate", "pillar", "wall", "table", "log", "jar"].into_iter().find(|k| kinds.iter().any(|x| x == k)).unwrap();
    let ground = scene["fighters"]["a"]["usable"]["ground"][0].as_str().unwrap().to_string();
    let lines = vec!["roll left".to_string(), "jump".into(), "I pull out a sword".into(), "I slash his throat with my obsidian blade".into(), "I draw a pistol".into(),
                     format!("hide behind that {have} then pop up and slit his throat"), "duck behind the cart".into(), format!("throw {ground} in his eyes"), "run in circles".into()];
    let mut acts = vec![at(0, challenge(&t), 1, now - 30_000)];
    for (k, l) in lines.iter().enumerate() {
        let a = at(0, line(&id, l), 2 + k as u64, now - 29_000 + 1_500 * k as i64);
        let b = submit(&t, &a).unwrap();
        acts.push(a);
        println!("BLOW {l:?}\n  stage {}\n  move {}\n  verdict {} flavor {:?} tags {}", b["stage"], b["move"], b["verdict"], b["flavor"], b["tags"]);
    }
    let tick = at(0, json!({ "duel_tick": { "duel_id": id } }), 99, now);
    let _ = submit(&t, &tick);
    acts.push(tick);
    let o = resolve_at(&t.p, &t, 2, &acts);
    let w2 = world::load(&o.state).unwrap();
    if let Some(b) = crate::duel::open_bouts(&w2).into_iter().find(|x| x.duel.id == id) {
        for p in &b.panels {
            println!("PANEL {} segs {}\n  contacts {}\n  projectiles {}", p["n"], p["a"]["segs"], p["contacts"], p["projectiles"]);
        }
    }
}
