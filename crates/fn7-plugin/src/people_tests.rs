//! People and houses: role and look assignment, house kinds, determinism, visibility and the query shapes.

use std::sync::OnceLock;

use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::GamePlugin;

use super::*;
use crate::{testkit, FinalNight7Plugin};

fn life(role: Role, title: &str) -> Box<Life> {
    Box::new(Life {
        role,
        rank: Rank { level: Level::Person, of: None, title: title.into() },
        competence: 0.5,
        ambition: 0.5,
        cruelty: 0.0,
        courage: 0.5,
        grudges: vec![],
        need: Default::default(),
        goal: Default::default(),
        emerged_as: role,
        emerged: 1,
        deeds: 0,
        kills: 0,
        cell: None,
        army: None,
        mob: None,
        died: None,
    })
}

/// A person materialized from the first crowd, then given these words.
fn person(w: &mut World, aliases: &[&str], l: Option<Box<Life>>) -> CharId {
    let k = w.clusters.iter().find(|c| c.army.is_none() && c.size > 10).unwrap().id;
    let m = Mention { text: String::new(), kind: "person".into(), near: None };
    let c = fn7_society::materialize(w, k, &m, 1, None);
    let x = &mut w.characters[c.ix()];
    x.aliases = aliases.iter().map(|s| s.to_string()).collect();
    x.traits.clear();
    x.life = l;
    c
}

fn played() -> &'static Value {
    static S: OnceLock<Value> = OnceLock::new();
    S.get_or_init(|| {
        std::thread::spawn(|| {
            tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap().block_on(testkit::played_state(9, 12, 100))
        })
        .join()
        .unwrap()
    })
}

#[test]
fn named_people_get_the_right_sprite_role() {
    let mut w = testkit::world(4, 9);
    let fisher = person(&mut w, &["a fisherman mending his nets"], None);
    let witch = person(&mut w, &["the ugly witch of the marsh"], None);
    let general = person(&mut w, &[], Some(life(Role::General, "General of Ash's Host of Cairo")));
    let maiden = person(&mut w, &["a beautiful maiden by the well"], None);
    let paladin = person(&mut w, &[], Some(life(Role::Paladin, "")));
    let soldier = person(&mut w, &[], Some(life(Role::Soldier, "")));
    let odd = person(&mut w, &["a witch in a soldier's coat"], Some(life(Role::Soldier, "")));
    let role = |c: CharId| role_of(&w, &w.characters[c.ix()]);
    assert_eq!(role(fisher), "fisherman");
    assert_eq!(role(witch), "witch");
    assert_eq!(role(general), "warrior");
    assert_eq!(role(maiden), "beauty");
    assert_eq!(role(paladin), "priest");
    assert_eq!(role(soldier), "soldier");
    // Their own words beat their society role: a soldier the story calls a witch is drawn as a witch.
    assert_eq!(role(odd), "witch");
    // Sex words narrow the look: a fisherman is one of the two men, a maiden one of the two women.
    let look = |c: CharId| look_of(&w.characters[c.ix()], role(c));
    assert!([1, 3].contains(&look(fisher)), "a man's look: {}", look(fisher));
    assert!([1, 2].contains(&look(maiden)), "a woman's look: {}", look(maiden));
    // Every role is reachable and every key has a subject.
    for r in ROLES {
        for l in 1..=LOOKS {
            let (kind, s) = subject(&sprite_key(r, l)).unwrap();
            assert_eq!(kind, "sprite");
            assert!(s.starts_with(&format!("{r} (look {l} of 3): ")), "{s}");
        }
    }
    assert_eq!(library_keys().len(), 72);
    assert!(subject("sprite/warrior/4").is_none() && subject("building/castle/rich").is_none() && subject("nope").is_none());
}

#[test]
fn coastal_settlements_get_a_dock_and_houses_follow_wealth() {
    let w = testkit::world(4, 9);
    assert!(w.is_planet());
    let sites: Vec<&Place> = w.places.iter().filter(|p| p.kind == PlaceKind::Site).collect();
    let coast = sites.iter().find(|p| crate::wills::quests::is_coast(&w, p.center)).expect("a coastal city");
    let inland = sites.iter().find(|p| !crate::wills::quests::is_coast(&w, p.center)).expect("an inland city");
    let hc = houses(&w, coast);
    assert_eq!(hc[0].kind, "dock", "{} is on the coast: {hc:?}", coast.name);
    assert!(houses(&w, inland).iter().all(|b| b.kind != "dock"), "{} is inland", inland.name);
    for p in &sites {
        let hs = houses(&w, p);
        assert_eq!(hs.len(), crate::earth::effects::site(&w, p).buildings.len());
        assert!(hs.iter().all(|b| HOUSE_KINDS.contains(&b.kind) && HOUSE_LOOKS.contains(&b.look)));
        assert!(hs.iter().all(|b| b.kind != "manor" || b.look != "poor"));
    }
    // Wealth: the same city, poor then rich.
    let mut w2 = w.clone();
    let p = coast.id;
    let set = |w: &mut World, x: f32| {
        if let Some(s) = w.dm.settlements.iter_mut().find(|s| s.place == p) {
            s.prosperity = x;
        }
    };
    if w2.dm.settlement(p).is_some() {
        set(&mut w2, 0.1);
        let poor = houses(&w2, &w2.places[p.ix()].clone());
        set(&mut w2, 0.95);
        let rich = houses(&w2, &w2.places[p.ix()].clone());
        assert_eq!(poor[0].kind, rich[0].kind);
        assert!(HOUSE_LOOKS.iter().position(|l| *l == poor[0].look) < HOUSE_LOOKS.iter().position(|l| *l == rich[0].look));
    }
}

#[test]
fn people_and_buildings_are_deterministic_bounded_and_public() {
    let state = played();
    let w = crate::world::load(state).unwrap();
    let p = FinalNight7Plugin::new();
    let x = w.characters.iter().find(|c| c.alive).unwrap();
    for level in ["regions", "villages", "houses", "people"] {
        let cell = fn7_model::hexgrid::cell_at(x.pos, fn7_model::hexgrid::HexLevel::parse(level).unwrap()).key();
        let a = p.query(state, None, "people", &json!({ "cell": cell })).unwrap();
        let b = p.query(state, None, "people", &json!({ "cell": cell })).unwrap();
        assert_eq!(a, b, "the same state, the same answer");
        assert_eq!(a["level"], level);
        let people = a["people"].as_array().unwrap();
        assert!(!people.is_empty(), "{level}: the person at the centre is listed");
        assert!(people.len() <= 100 && a["total"].as_u64().unwrap() as usize >= people.len());
        for v in people {
            for k in ["ref", "name", "role", "look", "sprite", "cell", "doing", "activity", "moving", "home_cell", "work_cell", "group"] {
                assert!(v.get(k).is_some(), "{level}: missing {k} in {v}");
            }
            // Public fields only: nothing seat-scoped.
            for k in ["allegiance", "memories", "yours", "follower"] {
                assert!(v.get(k).is_none(), "{k} must not leak: {v}");
            }
            assert!(ROLES.contains(&v["role"].as_str().unwrap()));
            assert_eq!(v["sprite"]["key"], format!("sprite/{}/{}", v["role"].as_str().unwrap(), v["look"]));
            assert_eq!(v["sprite"]["status"], "off", "the bare plugin draws nothing");
            assert!(!v["doing"].as_str().unwrap().is_empty());
            match v["ref"].as_str() {
                Some(r) => {
                    let c = match r.parse::<Ref>().unwrap() {
                        Ref::Char(c) => c,
                        other => panic!("{other}"),
                    };
                    assert!(w.characters[c.ix()].alive, "only the living");
                    assert_eq!(v["resident"], false);
                }
                None => {
                    assert_eq!(v["resident"], true, "{v}");
                    assert!(v["home_cell"].is_string(), "a resident lives in a house: {v}");
                }
            }
            // An act is only ever a public one.
            if v["activity"] == "acting" {
                let rows = crate::queries::rows(state);
                assert!(rows.iter().any(|r| r["actor"] == v["ref"] && crate::queries::is_public(&w, r)), "{v}");
            }
        }
        let all = p.query(state, None, "people", &json!({ "cell": cell, "limit": 300 })).unwrap();
        assert!(all["people"].as_array().unwrap().len() <= 300);
        assert!(p.query(state, None, "people", &json!({ "cell": cell, "limit": 301 })).is_err());
    }
    assert!(p.query(state, None, "people", &json!({})).is_err());
    assert!(p.query(state, None, "people", &json!({ "cell": "nope" })).is_err());
    // Buildings: a city's own hexes come back with their kinds; deterministic.
    let city = w.places.iter().find(|p| p.kind == PlaceKind::Site).unwrap();
    let key = fn7_model::hexgrid::cell_at(city.center, fn7_model::hexgrid::HexLevel::Houses).key();
    let a = p.query(state, None, "buildings", &json!({ "cell": key })).unwrap();
    assert_eq!(a, p.query(state, None, "buildings", &json!({ "cell": key })).unwrap());
    let bs = a["buildings"].as_array().unwrap();
    let mine: Vec<&Value> = bs.iter().filter(|b| b["settlement"] == Ref::Place(city.id).to_string()).collect();
    assert_eq!(mine.len(), houses(&w, city).len(), "{a}");
    for b in bs {
        assert_eq!(b["sprite"]["key"], format!("building/{}/{}", b["kind"].as_str().unwrap(), b["look"].as_str().unwrap()));
    }
}

#[test]
fn routines_follow_the_clock_and_groups_share_a_spot() {
    assert_eq!(gerund("strike"), "striking");
    assert_eq!(gerund("rob"), "robbing");
    assert_eq!(gerund("bless"), "blessing");
    assert_eq!(gerund("tie"), "tying");
    let mut v = vec![
        json!({ "activity": "fishing", "work_cell": "1:houses:1:1", "to_cell": null, "home_cell": "a", "cell": "x" }),
        json!({ "activity": "fishing", "work_cell": "1:houses:1:1", "to_cell": null, "home_cell": "b", "cell": "y" }),
        json!({ "activity": "praying", "work_cell": "1:houses:1:1", "to_cell": null, "home_cell": "c", "cell": "z" }),
        json!({ "activity": "sleeping", "work_cell": "w", "to_cell": "1:houses:9:9", "home_cell": "1:houses:9:9", "cell": "z" }),
        json!({ "activity": "sleeping", "work_cell": "w2", "to_cell": "1:houses:9:9", "home_cell": "1:houses:9:9", "cell": "q" }),
    ];
    assign_groups(&mut v);
    assert_eq!((v[0]["group"].clone(), v[1]["group"].clone()), (json!(1), json!(1)));
    assert_eq!(v[2]["group"], Value::Null, "alone");
    assert_eq!((v[3]["group"].clone(), v[4]["group"].clone()), (json!(2), json!(2)));
    // At night most people sleep at home; by day they work.
    let state = played();
    let mut w = crate::world::load(state).unwrap();
    let st = crate::world::settings(&w);
    let night = (1..=st.beats_per_day).find(|b| crate::dm::phase_of(*b, st.beats_per_day).1 == "night").unwrap();
    let day = (1..=st.beats_per_day).find(|b| crate::dm::phase_of(*b, st.beats_per_day).1 == "day").unwrap();
    let xs: Vec<CharId> = w.characters.iter().filter(|c| c.alive).map(|c| c.id).take(60).collect();
    let count = |w: &World, what: &str| {
        let cx = Ctx { acts: Default::default(), refuges: Default::default(), ..Ctx::new(w, &json!({})) };
        let refs: Vec<&Character> = xs.iter().map(|c| &w.characters[c.ix()]).collect();
        items(&cx, &refs).iter().filter(|v| v["activity"] == what).count()
    };
    w.meta.beat = night;
    let asleep = count(&w, "sleeping");
    w.meta.beat = day;
    let asleep_by_day = count(&w, "sleeping");
    assert!(asleep > xs.len() / 3, "{asleep} of {} asleep at night", xs.len());
    assert!(asleep_by_day < asleep, "{asleep_by_day} asleep by day");
}
