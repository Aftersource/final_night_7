//! What the earth did, thing by thing (schema 20): crowds flee floods to shelters, forests burn down cell by cell, a
//! storm on a village sends people to shelters that it then destroys, caps, determinism and time travel.

use fn7_model::earth::{EarthInput, EarthState, Phenomenon};
use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::GamePlugin;

use super::effects::{self, *};
use super::grid::grid;
use super::rules::{self, EarthBeat};
use super::sim::*;
use super::sim::Cell;
use crate::world::settings;
use crate::FinalNight7Plugin;

fn pantheon() -> std::sync::Arc<Pantheon> {
    FinalNight7Plugin::new().pantheon().clone()
}

/// A crowd (not an army) living in a planet city's quarter, and its home.
fn a_city_crowd(w: &World) -> (ClusterId, LonLat) {
    let c = w
        .clusters
        .iter()
        .find(|c| c.army.is_none() && c.size >= 200 && fn7_society::geo::city_of(w, c.place).is_some())
        .expect("a city crowd");
    (c.id, w.places[c.place.ix()].center)
}

fn fx_of(eb: &EarthBeat) -> Vec<Value> {
    eb.effects.as_ref().map(|f| f.rows.iter().map(Fx::json).collect()).unwrap_or_default()
}

/// A world whose field is set by hand at beat 3 (one phenomenon over it, cast by seat 0 at magnitude 8).
fn flooded_world(at: LonLat, cell: Cell, kind: u8) -> World {
    let mut w = crate::testkit::world(4, 9);
    let st = settings(&w);
    let g = grid();
    let i = g.index_of(at);
    let mut sim = Sim::new(w.meta.seed, rules::cfg(&st));
    sim.beat = 3;
    for (j, _) in g.disc(i, 1) {
        *sim.m(j) = cell;
    }
    let (lon_e3, lat_e3) = ((at[0] * 1000.0).round() as i32, (at[1] * 1000.0).round() as i32);
    sim.phen.push(Phenomenon { id: 1, kind, cell: i, lon_e3, lat_e3, born: 3, power: 128, radius: 1, dir: NO_DIR, dur: 4, owner: 0,
                               cause: CAUSE_CAST, parent: 0, a: 0, b: 0, tier: TIER_MEGA });
    sim.next_id = 2;
    sim.normalize();
    let mut e = EarthState { first_beat: 1, entropy: vec![0; 3], ..Default::default() };
    e.inputs.push(EarthInput { beat: 3, kind, cell: i, lon_e3, lat_e3, energy: 8, radius: 1, owner: 0, cause: CAUSE_CAST, tier: TIER_MEGA });
    sim.to_state(&mut e);
    w.dm.earth = Some(Box::new(e));
    w.meta.beat = 4;
    w
}

#[test]
fn a_flood_on_a_crowd_sends_it_to_a_shelter_with_counts() {
    let w0 = crate::testkit::world(4, 9);
    assert!(w0.earth_effects_rules(), "new matches are schema 20");
    let (crowd, home) = a_city_crowd(&w0);
    let mut w = flooded_world(home, Cell { water: 220, ..Default::default() }, FLOOD);
    let st = settings(&w);
    let size = w.clusters[crowd.ix()].size;
    let eb = rules::step(&mut w, 4, &pantheon(), &st).unwrap();
    let fx = fx_of(&eb);
    let me = format!("cl:{}", crowd.0);
    let fled = fx.iter().find(|f| f["what"] == "fled" && f["thing"]["ref"] == me.as_str()).unwrap_or_else(|| panic!("{me} fled: {fx:#?}"));
    assert_eq!(fled["thing"]["kind"], "crowd");
    assert_eq!(fled["level"], "villages");
    assert_eq!(fled["cell"], fn7_model::hexgrid::cell_at(home, fn7_model::hexgrid::HexLevel::Villages).key());
    let to = fled["to_cell"].as_str().expect("to a shelter");
    assert!(to.contains(":houses:"), "a building: {to}");
    let n = fled["count"].as_u64().unwrap() as u32;
    assert!(n > 0 && n <= size / 2 && n <= SHELTER_PER_LEVEL * 7, "{n} of {size}");
    // It came from the flood that was cast, with the cast's power and seat.
    assert_eq!((fled["cause"].as_u64(), fled["phenomenon"].as_str(), fled["power"].as_u64(), fled["seat"].as_u64()),
               (Some(1), Some("flood"), Some(8), Some(0)));
    let sh = fx.iter().find(|f| f["what"] == "sheltered" && f["cell"] == to && f["thing"]["ref"] == me.as_str()).expect("sheltered where they ran");
    assert_eq!(sh["count"].as_u64(), Some(n as u64));
    // The shelter is out of the flood: its houses-level hex is dry.
    let s = w.dm.earth.as_deref().unwrap();
    let r = s.refuges.iter().find(|r| r.crowd == crowd.0).expect("the state keeps them");
    assert_eq!((r.at.as_str(), r.count, r.since, r.until, r.cause), (to, n, 4, 0, 1));
    let b = fn7_model::hexgrid::HexCell::parse(to).unwrap();
    let i = grid().index_of(b.center());
    assert_eq!(super::view::fine(&eb.sim, &b, i).water, 0, "they sheltered somewhere dry");
    // The beat's public `earth` carries them, and `present` does too.
    let pub_ = super::view::beat_json(&eb);
    assert_eq!(pub_["effects"].as_array().unwrap().len(), fx.len());
    assert_eq!(pub_["effects_total"].as_u64(), Some(eb.effects.as_ref().unwrap().total as u64));
    assert!(pub_["effects_counts"]["fled"].as_u64().unwrap() >= 1);
    let sum = super::view::summary_of(&w, &st).unwrap();
    assert_eq!(sum["effects"], pub_["effects"]);
    // A second beat: those who ran are not counted twice; more of the rest run.
    let eb2 = rules::step(&mut w, 5, &pantheon(), &st).unwrap();
    let away: u32 = w.dm.earth.as_deref().unwrap().refuges.iter().filter(|r| r.crowd == crowd.0 && r.until == 0).map(|r| r.count).sum();
    assert!(away <= w.clusters[crowd.ix()].size);
    let _ = eb2;
}

#[test]
fn a_wildfire_burns_a_forest_down_cell_by_cell() {
    let mut w = crate::testkit::world(4, 9);
    let st = settings(&w);
    let p = pantheon();
    let amazon = [-62.0, -4.0];
    let g = grid();
    let mut burned: Vec<String> = vec![];
    let mut last = None;
    for beat in 1..=24u32 {
        if beat == 2 {
            rules::note(&mut w, beat, &st, WILDFIRE, amazon, 9, 800.0, TIER_MEGA, 1, CAUSE_CAST);
        }
        w.meta.beat = beat;
        let eb = rules::step(&mut w, beat, &p, &st).unwrap();
        for f in fx_of(&eb).iter().filter(|f| f["thing"]["kind"] == "forest") {
            assert_eq!(f["what"], "burned", "{f}");
            assert_eq!(f["level"], "regions");
            assert!(f["cell"].as_str().unwrap().contains(":regions:"));
            assert_eq!(f["seat"].as_u64(), Some(1), "the caster's: beat {beat} {f}");
            let c = f["cell"].as_str().unwrap().to_string();
            assert!(!burned.contains(&c), "each forest cell burns once: {c}");
            burned.push(c);
        }
        last = Some(eb);
    }
    let eb = last.unwrap();
    assert!(burned.len() >= 5, "the fire front crossed {} forest cells", burned.len());
    // Every forest cell it crossed is burnt: nothing that counts as forest is left, most of it bare.
    let bare = burned
        .iter()
        .filter(|k| {
            let i = g.index(&fn7_model::hexgrid::HexCell::parse(k).unwrap());
            assert!(eb.sim.fuel(i) < FOREST_FUEL, "{k} still a forest");
            assert_ne!(eb.sim.top(i), T_FOREST);
            eb.sim.fuel(i) < 24
        })
        .count();
    assert!(bare * 2 >= burned.len(), "{bare} of {} burned to nothing", burned.len());
    // What lasts: the burnt ground is in the scars, regrowing from what is left.
    let s = w.dm.earth.as_deref().unwrap();
    let scars = scars_json(&eb.sim, s, None);
    let burnt: Vec<&str> = scars["burnt"].as_array().unwrap().iter().map(|x| x["cell"].as_str().unwrap()).collect();
    assert!(burned.iter().filter(|k| burnt.contains(&k.as_str())).count() * 2 >= burned.len());
}

/// A mega storm on a city's quarter, stepped for `beats` beats: the effects of each beat and the world after.
fn storm_on_a_village(beats: u32) -> (World, Vec<Vec<Value>>, Vec<Value>) {
    let mut w = crate::testkit::world(4, 9);
    let st = settings(&w);
    let p = pantheon();
    let (_, home) = a_city_crowd(&w);
    let mut out = vec![];
    let mut pubs = vec![];
    for beat in 1..=beats {
        if beat == 2 || beat == 3 {
            rules::note(&mut w, beat, &st, STORM, home, 10, 800.0, TIER_MEGA, 2, CAUSE_CAST);
        }
        w.meta.beat = beat;
        let eb = rules::step(&mut w, beat, &p, &st).unwrap();
        out.push(fx_of(&eb));
        pubs.push(super::view::beat_json(&eb));
    }
    (w, out, pubs)
}

#[test]
fn a_storm_on_a_village_sends_people_to_shelters_it_then_destroys() {
    let (w, beats, _) = storm_on_a_village(10);
    let all: Vec<&Value> = beats.iter().flatten().collect();
    let sheltered: Vec<&&Value> = all.iter().filter(|f| f["what"] == "sheltered" && f["phenomenon"] == "storm").collect();
    assert!(!sheltered.is_empty(), "people take cover from the storm");
    assert!(sheltered.iter().all(|f| f["level"] == "houses" && f["count"].as_u64().unwrap() > 0));
    let destroyed: Vec<&&Value> = all.iter().filter(|f| f["thing"]["kind"] == "building" && f["what"] == "destroyed").collect();
    assert!(!destroyed.is_empty(), "the storm breaks shelters: {all:#?}");
    assert!(destroyed.iter().all(|f| f["thing"]["ref"].as_str().unwrap().starts_with("pl:") && f["seat"] == 2));
    // A shelter that fell with people in it: some died there, the survivors ran on.
    let wrecked: Vec<&str> = destroyed.iter().map(|f| f["cell"].as_str().unwrap()).collect();
    let died = all.iter().filter(|f| f["what"] == "died" && wrecked.contains(&f["cell"].as_str().unwrap())).count();
    let ran = all.iter().filter(|f| f["what"] == "fled" && wrecked.contains(&f["cell"].as_str().unwrap())).count();
    assert!(died > 0 && ran > 0, "died {died}, ran on {ran}");
    // The state keeps the wrecks and the shelters (closed when the shelter fell).
    let s = w.dm.earth.as_deref().unwrap();
    assert!(s.wrecks.iter().any(|x| x.how == "destroyed" && wrecked.contains(&x.at.as_str())));
    assert!(s.refuges.iter().any(|r| r.until > 0 && wrecked.contains(&r.at.as_str())));
    // Deaths went through the rules: the ledger has them.
    assert!(w.ledger.rows.iter().any(|r| r.act.contains("die as their shelter falls")));
    // No shelter holds more than it can: 250 × (1 + its city's level), and that is at most level 6.
    let mut occ: std::collections::BTreeMap<&str, u32> = Default::default();
    for r in s.refuges.iter().filter(|r| r.until == 0) {
        *occ.entry(r.at.as_str()).or_default() += r.count;
    }
    assert!(occ.values().all(|n| *n <= SHELTER_PER_LEVEL * 7), "{occ:?}");
    // Wrecks stay standing as wrecks: no building is destroyed twice.
    let mut seen = std::collections::BTreeSet::new();
    assert!(s.wrecks.iter().all(|x| seen.insert(x.at.clone())));
}

#[test]
fn effects_are_deterministic_capped_worst_first_and_kept_for_time_travel() {
    let (w1, a, pubs) = storm_on_a_village(8);
    let (w2, b, _) = storm_on_a_village(8);
    assert_eq!(a, b, "the same beats name the same effects");
    assert_eq!(serde_json::to_value(&w1.dm.earth).unwrap(), serde_json::to_value(&w2.dm.earth).unwrap());
    // Worst first: deaths before destruction before flight.
    let rank = |f: &Value| match f["what"].as_str().unwrap() { "died" => 0, "destroyed" | "buried" | "swept" => 1, _ => 2 };
    for list in &a {
        let r: Vec<u8> = list.iter().filter(|f| f["thing"]["kind"] != "settlement").map(rank).collect();
        assert!(r.windows(2).all(|x| x[0] <= x[1]), "{list:#?}");
    }
    // game.earth_at at a past beat answers that beat's effects, exactly as the beat carried them.
    let state = serde_json::to_value(&w1).unwrap();
    let p = FinalNight7Plugin::new();
    let busiest = (0..a.len()).max_by_key(|k| a[*k].len()).unwrap();
    assert!(!a[busiest].is_empty());
    let beat = busiest as u32 + 1;
    let past = p.query(&state, None, "earth_at", &json!({ "beat": beat })).unwrap();
    assert_eq!(past["beat"].as_u64(), Some(beat as u64));
    assert_eq!(past["effects"], pubs[busiest]["effects"]);
    assert_eq!(past["effects_total"], pubs[busiest]["effects_total"]);
    assert_eq!(past["scars"]["beat"].as_u64(), Some(beat as u64));
    // ... and what lasted then: the shelters open at that beat.
    let open_then = w1.dm.earth.as_deref().unwrap().refuges.iter().filter(|r| r.since <= beat && (r.until == 0 || r.until > beat)).count();
    assert_eq!(past["scars"]["sheltering"].as_array().unwrap().len(), open_then.min(SCARS_MAX));
    // The scars over the whole planet, now.
    let now = p.query(&state, None, "earth", &json!({ "scars": true })).unwrap();
    assert!(now["scars"]["wrecks"].as_array().unwrap().len() == w1.dm.earth.as_deref().unwrap().wrecks.len());
    // A board asks for what lasts within its reach; without `scars` it is the board it always was.
    let wreck = w1.dm.earth.as_deref().unwrap().wrecks[0].at.clone();
    let board = p.query(&state, None, "earth", &json!({ "cell": wreck, "level": "houses", "scars": true })).unwrap();
    assert!(board["scars"]["wrecks"].as_array().unwrap().iter().any(|x| x["cell"] == wreck.as_str()));
    assert!(p.query(&state, None, "earth", &json!({ "cell": wreck, "level": "houses" })).unwrap().get("scars").is_none());
    // The cap: the worst N, with the total before it.
    let mut w = crate::testkit::world(4, 9);
    let (_, home) = a_city_crowd(&w);
    let mut st = settings(&w);
    st.earth_effects_max = 3;
    let pan = pantheon();
    let mut capped = false;
    for beat in 1..=8u32 {
        if beat == 2 || beat == 3 {
            rules::note(&mut w, beat, &st, STORM, home, 10, 800.0, TIER_MEGA, 2, CAUSE_CAST);
        }
        let eb = rules::step(&mut w, beat, &pan, &st).unwrap();
        let fx = eb.effects.as_ref().unwrap();
        assert!(fx.rows.len() <= 3);
        assert_eq!(fx.counts.iter().sum::<u32>(), fx.total);
        if fx.total > 3 {
            capped = true;
            assert_eq!(fx.rows.len(), 3);
            let kept: Vec<Value> = fx.rows.iter().map(Fx::json).collect();
            assert_eq!(kept[..], a[beat as usize - 1][..3], "the cap keeps the worst");
        }
    }
    assert!(capped, "a beat with more than 3 effects");
}

#[test]
fn packing_round_trips() {
    let (w, a, _) = storm_on_a_village(6);
    let s = w.dm.earth.as_deref().unwrap();
    assert!(!s.effects.is_empty());
    for e in &s.effects {
        let rows = effects::unpack(&e.rows);
        assert_eq!(effects::unpack(&effects::pack(&rows)), rows);
        let js: Vec<Value> = rows.iter().map(Fx::json).collect();
        assert_eq!(js, a[e.beat as usize - 1]);
    }
    let bytes: usize = s.effects.iter().map(|e| e.rows.len()).sum();
    let n: usize = s.effects.iter().map(|e| effects::unpack(&e.rows).len()).sum();
    eprintln!("{n} effects kept in {bytes} bytes of base64 ({} B each)", bytes / n.max(1));
}

#[test]
fn an_older_schema_names_nothing_and_moves_nobody() {
    let (crowd, home) = a_city_crowd(&crate::testkit::world(4, 9));
    let mut w = flooded_world(home, Cell { water: 220, ..Default::default() }, FLOOD);
    w.meta.schema = fn7_model::earth::EARTH_PROMPT_SCHEMA;
    let st = settings(&w);
    let size = w.clusters[crowd.ix()].size;
    let eb = rules::step(&mut w, 4, &pantheon(), &st).unwrap();
    assert!(eb.effects.is_none());
    let s = w.dm.earth.as_deref().unwrap();
    assert!(s.effects.is_empty() && s.refuges.is_empty() && s.wrecks.is_empty());
    let v = super::view::beat_json(&eb);
    assert!(v.get("effects").is_none());
    assert!(serde_json::to_value(s).unwrap().get("refuges").is_none(), "nothing new is serialized");
    assert!(w.clusters[crowd.ix()].size <= size);
}

/// Real effects per power, for the docs: `cargo test -p fn7-plugin examples_per_power -- --ignored --nocapture`.
#[test]
#[ignore]
fn examples_per_power() {
    let powers = [("flood", FLOOD), ("wildfire", WILDFIRE), ("storm_clouds", STORM), ("earthquake", EARTHQUAKE), ("volcano", VOLCANO),
                  ("meteor", METEOR), ("lightning", LIGHTNING), ("swarm", SWARM), ("sink_land", SINK_LAND), ("storm_clouds (blizzard)", BLIZZARD)];
    for (name, kind) in powers {
        let mut w = crate::testkit::world(4, 9);
        let st = settings(&w);
        let p = pantheon();
        let (_, home) = a_city_crowd(&w);
        let mut seen: std::collections::BTreeMap<(String, String), Value> = Default::default();
        let mut totals = 0u32;
        for beat in 1..=10u32 {
            if beat == 2 {
                rules::note(&mut w, beat, &st, kind, home, 9, 800.0, TIER_MEGA, 1, CAUSE_CAST);
            }
            let eb = rules::step(&mut w, beat, &p, &st).unwrap();
            totals += eb.effects.as_ref().map(|f| f.total).unwrap_or(0);
            for f in fx_of(&eb) {
                let k = (f["thing"]["kind"].as_str().unwrap().to_string(), f["what"].as_str().unwrap().to_string());
                seen.entry(k).or_insert_with(|| { let mut f = f.clone(); f["beat"] = beat.into(); f });
            }
        }
        eprintln!("== {name}: {totals} effects over 10 beats");
        for f in seen.values() {
            eprintln!("  {f}");
        }
    }
}
