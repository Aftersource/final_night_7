//! The Earth simulator inside a match: casts feed it, it steps every beat, and what it does comes back through the
//! rules (damage shaped by gear, crowds, settlements, roads, shrines, encounters, the DM's memory, public events).

use fn7_model::dm::{Shrine, ShrinePower, Trail};
use fn7_model::earth::EarthState;
use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::*;

use super::grid::grid;
use super::rules;
use super::sim::*;
use super::sim::Cell;
use crate::driver::BeatReport;
use crate::testkit::Failing;
use crate::world::{load, settings};
use crate::FinalNight7Plugin;

fn humans(n: u8) -> Vec<SeatInfo> {
    (0..n).map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix", "Cyra", "Dune"][i as usize % 4].into(), is_bot: false }).collect()
}

fn instant(seat: Seat, seq: u64, fields: Value) -> Action {
    Action { instant: true, seq, received_at_ms: 1_000 + seq as i64, fields, ..Action::new(seat, "") }
}

/// Megas near faults and on rivals, a few minis: what four players overloading on Entropy send.
fn storm_of_casts(round: u32) -> Vec<Action> {
    let megas = ["volcano", "earthquake", "wildfire", "flood", "meteor", "storm_clouds", "sink_land", "raise_land", "swarm", "lightning", "grow_forest"];
    let spots = [[138.7, 35.4], [143.5, 38.0], [-72.0, -33.0], [-122.0, 37.5], [100.0, 0.0], [28.0, 39.0], [-60.0, -5.0], [22.0, 0.0]];
    let mut out = vec![];
    for seat in 0..4u8 {
        let n = round * 4 + seat as u32;
        // One cast in seven is aimed at a rival's demigod; the rest at the faults.
        let target = if (n % 7) == 0 { json!(format!("dg:{}", ((seat as u32 + 1) % 4) * 2)) } else { json!(spots[n as usize % spots.len()]) };
        out.push(instant(seat, (round * 10 + seat as u32) as u64, json!({"cast": {"power": megas[(n * 7) as usize % megas.len()], "target": target, "tier": "mega"}})));
        if round % 2 == 0 {
            out.push(instant(seat, (round * 10 + 5 + seat as u32) as u64, json!({"cast": {"power": megas[n as usize % megas.len()], "target": spots[(n + 3) as usize % spots.len()], "tier": "mini"}})));
        }
    }
    out
}

struct Run {
    outs: Vec<Value>,
    states: Vec<Value>,
}

/// A match of four humans who cast (no LLM). `calm`: nobody casts.
async fn play(seed: u64, beats: u32, extra: Value, calm: bool) -> Run {
    let p = FinalNight7Plugin::new();
    // (Mild Entropy per cast and no debts, so the four last the test's beats while they cast without stop.)
    let mut s = json!({ "seed": seed, "beats_total": 60, "llm": false, "clusters": 60, "players": 4, "bolt_start": 40, "spark_start": 40,
                        "entropy_per_mega": 1.0, "entropy_per_mini": 0.2, "debt_scale": 0.0, "accident_scale": 0.0, "earth_keyframe_beats": 8 });
    if let (Some(o), Some(x)) = (s.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            o.insert(k.clone(), v.clone());
        }
    }
    let script = |_: Seat, _: u32| -> Option<Action> { None };
    let extras = |round: u32| -> (Vec<Action>, RoundContext) { (if calm { vec![] } else { storm_of_casts(round) }, RoundContext::default()) };
    let mut run = Run { outs: vec![], states: vec![] };
    crate::driver::run_match_with(&p, &humans(4), &s, &Failing, Some(beats), &script, &extras, |r: &BeatReport, st: &Value| {
        run.outs.push(r.outcome.public.clone());
        run.states.push(st.clone());
        true
    })
    .await
    .unwrap();
    run
}

#[tokio::test(flavor = "multi_thread")]
async fn casts_feed_the_field_and_the_field_reaches_the_rules() {
    let r = play(41, 20, json!({}), false).await;
    let w = load(r.states.last().unwrap()).unwrap();
    assert_eq!(w.meta.schema, fn7_model::earth::EARTH_SCHEMA.max(fn7_model::STATE_SCHEMA));
    assert!(w.earth_rules() && settings(&w).earth_sim_enabled);
    let e = w.dm.earth.as_deref().expect("the state holds the Earth");
    assert_eq!((e.first_beat, e.beat), (1, w.meta.beat));
    assert_eq!(e.entropy.len() as u32, e.beat);
    // Every cast that landed is an input, with the beat it landed.
    let landed: usize = r.outs.iter().map(|o| o["casts"].as_array().unwrap().iter().filter(|c| c.get("fizzled").is_none()).count()).sum();
    let cast_inputs = e.inputs.iter().filter(|i| i.cause == CAUSE_CAST).count();
    assert!(landed >= 60, "{landed} casts landed");
    assert_eq!(cast_inputs, landed, "one input per landed cast");
    assert!(e.inputs.iter().any(|i| i.tier == TIER_MEGA && i.radius >= 1) && e.inputs.iter().any(|i| i.tier == TIER_MINI && i.radius == 0));
    assert!(e.inputs.iter().any(|i| i.cause == CAUSE_RAMIFICATION), "Entropy's physical ramifications feed it too");
    assert!(e.inputs.windows(2).all(|x| x[0].beat <= x[1].beat));
    assert_eq!(r.outs.len(), 20, "the match ran its 20 beats");
    assert_eq!(e.keyframes.iter().map(|k| k.beat).collect::<Vec<_>>(), vec![8, 16]);
    // A mega near a fault lands harder than one far from any (chapter 13 sets the energy).
    let g = grid();
    let near: Vec<u8> = e.inputs.iter().filter(|i| i.tier == TIER_MEGA && i.kind == VOLCANO && g.fault[i.cell as usize] == 0).map(|i| i.energy).collect();
    assert!(near.iter().all(|&x| x == 10), "volcanoes on a fault: {near:?}");
    // Every beat's outcome carries the Earth: the cells that changed and the live phenomena.
    for (k, o) in r.outs.iter().enumerate() {
        let ea = &o["earth"];
        assert_eq!(ea["beat"].as_u64(), Some(k as u64 + 1));
        assert!(ea["delta"]["n"].as_u64().is_some() && ea["delta"]["enc"] == "pack" && ea["phenomena"].is_array() && ea["hash"].as_str().unwrap().len() == 16);
    }
    let last = &r.outs.last().unwrap()["earth"];
    assert!(last["phenomena"].as_array().unwrap().len() >= 5);
    let p = &last["phenomena"][0];
    for k in ["id", "kind", "at", "cell", "radius_cells", "radius_km", "power", "intensity", "age", "beats_left", "cause", "tier"] {
        assert!(p.get(k).is_some(), "phenomenon.{k}: {p}");
    }
    assert!(last["phenomena"].as_array().unwrap().iter().any(|p| p["owner_seat"].is_u64()));
    // Consequences through the rules: demigods standing in it are hurt, by name, in the ledger like any damage.
    let hurt: Vec<&LedgerRow> = w.ledger.rows.iter().filter(|x| ["burned by the wildfire", "caught in the lava", "swept up in the flood",
        "thrown down by the quake", "struck by lightning", "frozen in the blizzard", "bitten by the swarm"].iter().any(|c| x.act.contains(c))).collect();
    assert!(hurt.len() >= 3, "{} hazard rows", hurt.len());
    assert!(hurt.iter().all(|x| matches!(x.targets.first(), Some(Ref::Demigod(_)))));
    // Crowds cower.
    assert!(w.ledger.rows.iter().any(|x| x.act.contains("cower before")), "crowds fear it");
    // The DM remembers, place × time, and the entries are the beat's public events.
    let kinds: std::collections::BTreeSet<&str> = w.dm.memory.iter().filter(|m| m.kind.starts_with("earth.")).map(|m| m.kind.as_str()).collect();
    assert!(kinds.contains("earth.quake") && kinds.contains("earth.eruption") && kinds.contains("earth.impact"), "{kinds:?}");
    assert!(kinds.len() >= 5, "{kinds:?}");
    let m = w.dm.memory.iter().find(|m| m.kind == "earth.quake").unwrap();
    assert!(m.public && m.place.starts_with("pl:") && m.up == vec!["world".to_string()] && m.data["cell"].is_string(), "{m:?}");
    let events: Vec<&Value> = r.outs.iter().flat_map(|o| o["dm_events"].as_array().unwrap()).filter(|e| e["event"].as_str().unwrap().starts_with("earth.")).collect();
    assert!(events.len() >= 8 && events.iter().all(|e| e["text"].is_string() && e["at"].is_array()));
    // Never more entries a beat than the setting allows (the cap's own record aside).
    for b in 1..=20u32 {
        let n = w.dm.memory.iter().filter(|m| m.beat == b && m.kind.starts_with("earth.") && m.kind != "earth.settled").count();
        assert!(n <= 6, "beat {b}: {n} entries");
    }
    // A region's memory answers "what happened here": the roll-up counts the Earth's facts.
    assert!(w.dm.memory.iter().any(|m| m.kind == "memory.rollup" && m.data["counts"].as_object().is_some_and(|c| c.keys().any(|k| k.starts_with("earth.")))));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_match_is_deterministic_and_off_means_absent() {
    let a = play(42, 12, json!({}), false).await;
    let b = play(42, 12, json!({}), false).await;
    assert_eq!(a.states.len(), 12);
    for (k, (x, y)) in a.states.iter().zip(&b.states).enumerate() {
        assert!(x == y, "state after beat {} differs", k + 1);
    }
    for (x, y) in a.outs.iter().zip(&b.outs) {
        assert_eq!(x["earth"], y["earth"]);
    }
    // Off: no Earth in the state, in the outcome, in the views; the queries say so.
    let off = play(42, 6, json!({ "earth_sim_enabled": false }), false).await;
    let st = off.states.last().unwrap();
    assert!(st["dm"].get("earth").is_none());
    assert!(off.outs.iter().all(|o| o.get("earth").is_none()));
    let p = FinalNight7Plugin::new();
    assert!(p.query(st, None, "earth", &json!({})).unwrap_err().contains("off"));
    assert!(p.public_view(st).get("earth").is_none());
    assert!(p.public_view(st)["present"]["earth"].is_null());
    // Settings recorded before the simulator read as off; a city arena never runs it.
    assert!(!crate::Settings::from_value(&json!({ "beats_total": 30, "flow_ms": 3000, "looks_enabled": true })).earth_sim_enabled);
    let city = play(42, 3, json!({ "arena": "city" }), false).await;
    assert!(city.states.last().unwrap()["dm"].get("earth").is_none());
    // A calm match: the field stays empty and costs nothing.
    let calm = play(42, 10, json!({}), true).await;
    let w = load(calm.states.last().unwrap()).unwrap();
    let e = w.dm.earth.as_deref().unwrap();
    assert!(e.inputs.is_empty() && e.cells.is_empty(), "{:?}", e.inputs);
    assert!(serde_json::to_string(e).unwrap().len() < 400);
    assert!(serde_json::to_string(&calm.outs[9]["earth"]).unwrap().len() < 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn queries_present_and_time_travel() {
    let r = play(43, 20, json!({}), false).await;
    let p = FinalNight7Plugin::new();
    let st = r.states.last().unwrap();
    // `game.earth {}`: every cell that differs from base, as columns.
    let now = p.query(st, None, "earth", &json!({})).unwrap();
    let hash_now = r.outs[19]["earth"]["hash"].as_str().unwrap();
    assert_eq!(now["hash"], hash_now);
    assert_eq!(now["field"]["n"], now["dirty"]);
    assert_eq!(now["field"]["enc"], "pack");
    let n = now["dirty"].as_u64().unwrap() as usize;
    assert!(n >= 50, "{n} dirty cells");
    let cells = super::view::unpack_cells(&super::codec::un64(now["field"]["data"].as_str().unwrap()));
    assert_eq!(cells.len(), n);
    // A base64 array per column, or JSON arrays, on request; the same numbers.
    let cols = p.query(st, None, "earth", &json!({ "format": "columns" })).unwrap();
    assert_eq!(super::codec::un64(cols["field"]["cells"].as_str().unwrap()).len(), 2 * n);
    let js = p.query(st, None, "earth", &json!({ "format": "json" })).unwrap();
    assert_eq!(js["field"]["cells"].as_array().unwrap().len(), n);
    assert_eq!(js["field"]["h"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect::<Vec<_>>(),
               cells.iter().map(|c| c.1).collect::<Vec<_>>());
    assert_eq!(super::codec::un64(cols["field"]["top"].as_str().unwrap()), cells.iter().map(|c| c.2).collect::<Vec<_>>());
    assert!(p.query(st, None, "earth", &json!({ "format": "xml" })).is_err());
    // The reconnect: `present` (and the PublicView) carry the summary; the hash says whether a client is current.
    let view = p.public_view(st);
    assert_eq!(view["present"]["earth"]["hash"], hash_now);
    assert_eq!(view["earth"]["hash"], hash_now);
    let ids = |v: &Value| v.as_array().unwrap().iter().map(|p| (p["id"].as_u64().unwrap(), p["cell"].as_str().unwrap().to_string(), p["power"].as_i64().unwrap())).collect::<Vec<_>>();
    assert_eq!(ids(&view["present"]["earth"]["phenomena"]), ids(&r.outs[19]["earth"]["phenomena"]));
    assert_eq!(p.query(st, None, "present", &json!({})).unwrap()["earth"]["dirty"], now["dirty"]);
    // Time travel: any past beat, from the live state alone, is the beat that was lived (its outcome's hash).
    for b in [1u32, 5, 8, 9, 15, 16, 19] {
        let past = p.query(st, None, "earth_at", &json!({ "beat": b })).unwrap();
        assert_eq!(past["hash"], r.outs[b as usize - 1]["earth"]["hash"], "beat {b}");
        // The outcome sends the 20 biggest; the rebuilt field has every one.
        let all = ids(&past["phenomena"]);
        assert!(ids(&r.outs[b as usize - 1]["earth"]["phenomena"]).iter().all(|x| all.contains(x)), "beat {b}");
        assert_eq!(all.len() as u64, r.outs[b as usize - 1]["earth"]["phenomena_total"].as_u64().unwrap(), "beat {b}");
        assert!(past["resimulated_beats"].as_u64().unwrap() <= 8, "at most K beats re-simulated");
        // And it is what the state of that beat itself says.
        let then = p.query(&r.states[b as usize - 1], None, "earth", &json!({})).unwrap();
        assert_eq!(then["field"], past["field"], "beat {b}");
    }
    assert_eq!(p.query(st, None, "earth_at", &json!({ "beat": "16" })).unwrap()["from_keyframe"], 16);
    assert!(p.query(st, None, "earth_at", &json!({ "beat": 21 })).is_err());
    assert!(p.query(st, None, "earth_at", &json!({})).is_err());
    // The client's board: 22 rings around a hex at any level, 1,519 records of 9 bytes in ring order, under 20 KB.
    let cell = now["phenomena"][0]["cell"].as_str().unwrap().to_string();
    let c0 = fn7_model::hexgrid::HexCell::parse(&cell).unwrap();
    for level in ["regions", "villages", "houses", "people"] {
        let b = p.query(st, None, "earth", &json!({ "cell": cell, "level": level })).unwrap();
        let data = super::codec::un64(b["board"]["data"].as_str().unwrap());
        assert_eq!((b["board"]["n"].as_u64(), data.len()), (Some(1519), 1519 * 9), "{level}");
        let bytes = b.to_string().len();
        assert!(bytes < 24 * 1024, "{level}: a board answer is {bytes} bytes");
        assert!(b["phenomena"].as_array().unwrap().len() <= super::view::PHENOMENA_SENT);
        assert_eq!(b, p.query(st, None, "earth", &json!({ "cell": cell, "level": level })).unwrap(), "pure");
        if level == "regions" {
            // The centre's record is the cell's own (height in 256ths: 128 = sea level).
            let sim = super::view::field_at(st, None).unwrap().0;
            let i = grid().index(&c0);
            assert_eq!(data[0] as u32, sim.height(i) as u32 * 16 + 8);
            assert_eq!(data[1], sim.top(i));
            eprintln!("board (22 rings, regions): {bytes} bytes");
        }
    }
    // A key's own level is the default; the board's centre is that hex.
    let b = p.query(st, None, "earth", &json!({ "cell": cell })).unwrap();
    assert_eq!((b["board"]["center"].as_str(), b["level"].as_str()), (Some(cell.as_str()), Some("regions")));
    // Only what changed since a beat: applying those records to the old board gives the new one.
    let old = p.query(st, None, "earth", &json!({ "cell": cell, "beat": 12 })).unwrap();
    let diff = p.query(st, None, "earth", &json!({ "cell": cell, "since": 12 })).unwrap();
    let mut board = super::codec::un64(old["board"]["data"].as_str().unwrap());
    let pos = super::codec::un64(diff["board"]["changed"].as_str().unwrap());
    let recs = super::codec::un64(diff["board"]["data"].as_str().unwrap());
    assert_eq!(pos.len() / 2 * 9, recs.len());
    assert!(!pos.is_empty() && pos.len() / 2 < 1519);
    for (k, ch) in pos.chunks(2).enumerate() {
        let at = u16::from_le_bytes([ch[0], ch[1]]) as usize;
        board[at * 9..at * 9 + 9].copy_from_slice(&recs[k * 9..k * 9 + 9]);
    }
    assert_eq!(board, super::codec::un64(b["board"]["data"].as_str().unwrap()));
    // The coarse cell under every board hex, to intersect a beat's delta with.
    let fine = p.query(st, None, "earth", &json!({ "cell": cell, "level": "villages", "rings": 3, "coarse": true })).unwrap();
    assert_eq!((fine["board"]["n"].as_u64(), super::codec::un64(fine["board"]["coarse"].as_str().unwrap()).len()), (Some(37), 74));
    // The verbose forms stay: JSON hexes below regions, columns at regions.
    let hexes = p.query(st, None, "earth", &json!({ "cell": cell, "level": "villages", "rings": 3, "hexes": true })).unwrap();
    assert_eq!((hexes["hexes"].as_array().unwrap().len(), hexes["derived"].as_bool()), (37, Some(true)));
    assert!(hexes["hexes"][0]["cell"].as_str().unwrap().contains(":villages:"));
    let region = p.query(st, None, "earth", &json!({ "region": "pl:1", "level": "houses", "hexes": true, "rings": 6 })).unwrap();
    assert_eq!(region["hexes"].as_array().unwrap().len(), 127);
    let cols2 = p.query(st, None, "earth", &json!({ "cell": cell, "rings": 2, "field": true })).unwrap();
    assert!(cols2["field"]["n"].as_u64().unwrap() >= 19);
    // The globe's raster: 4° tiles, 9 bytes each; only the changed tiles since a beat.
    let tiles = p.query(st, None, "earth", &json!({ "tiles": 4 })).unwrap();
    assert_eq!((tiles["tiles"]["rows"].as_u64(), tiles["tiles"]["n"].as_u64()), (Some(45), Some(2578)), "about equally large tiles: rings of latitude");
    let raster = p.query(st, None, "earth", &json!({ "tiles": 4, "layout": "grid" })).unwrap();
    assert_eq!(raster["tiles"]["n"].as_u64(), Some(4050));
    let tb = tiles.to_string().len();
    let tdiff = p.query(st, None, "earth", &json!({ "tiles": 4, "since": 12 })).unwrap();
    let tn = tdiff["tiles"]["n"].as_u64().unwrap();
    assert!(tn > 0 && tn < 2578);
    eprintln!("globe tiles (4 deg): {tb} bytes; changed since beat 12: {tn} tiles, {} bytes", tdiff.to_string().len());
    let bbox = p.query(st, None, "earth", &json!({ "bbox": "120,20,150,50" })).unwrap();
    assert!(bbox["field"]["n"].as_u64().unwrap() > 50);
    // The whole globe and the grid, once.
    let globe = p.query(st, None, "earth", &json!({ "base": true, "grid": true })).unwrap();
    assert_eq!(globe["globe"]["n"].as_u64().unwrap() as usize, grid().len());
    assert_eq!(super::codec::un64(globe["globe"]["h"].as_str().unwrap()).len(), grid().len());
    assert_eq!(globe["field"], now["field"]);
    assert_eq!(super::codec::un64(globe["grid"]["q"].as_str().unwrap()).len(), grid().len());
    eprintln!("game.earth: now {} bytes ({n} cells) packed, {} as columns, {} as json; with the globe and the grid {} bytes", now.to_string().len(),
              cols.to_string().len(), js.to_string().len(), globe.to_string().len());
    assert!(p.query(st, None, "earth", &json!({ "level": "villages" })).is_err());
    assert!(p.query(st, None, "earth", &json!({ "level": "moon" })).is_err());
    // A hex the DM observed carries the field's truth for it, and the weather is the field's.
    let w = load(st).unwrap();
    let obs = w.dm.memory.iter().rev().find(|m| m.kind == "place.observed" && m.data.get("earth").is_some()).expect("an observed hex with its earth");
    assert!(obs.data["earth"]["top"].is_string() && obs.data["weather"] == obs.data["earth"]["weather"]);
}

/// A world with the field set by hand at the beat before `w.meta.beat + 1`.
fn world_with(cells: &[(LonLat, Cell)]) -> (World, std::sync::Arc<Pantheon>) {
    let mut w = crate::testkit::world(4, 9);
    let st = settings(&w);
    let mut sim = Sim::new(w.meta.seed, rules::cfg(&st));
    sim.beat = 3;
    for (at, c) in cells {
        *sim.m(grid().index_of(*at)) = *c;
    }
    sim.normalize();
    let mut e = EarthState { first_beat: 1, entropy: vec![0; 3], ..Default::default() };
    sim.to_state(&mut e);
    w.dm.earth = Some(Box::new(e));
    w.meta.beat = 4;
    (w, FinalNight7Plugin::new().pantheon().clone())
}

#[test]
fn damage_is_bounded_named_and_follows_the_settings() {
    let at = [10.0, 50.0];
    let hell = Cell { fire: 255, lava: 200, water: 0, swarm: 255, ..Default::default() };
    let (mut w, pantheon) = world_with(&[(at, hell)]);
    let st = settings(&w);
    w.demigods[0].pos = at;
    w.demigods[1].pos = at;
    let h0 = w.demigods[0].health;
    let far = w.demigods[2].health;
    let eb = rules::step(&mut w, 4, &pantheon, &st).unwrap();
    assert_eq!(eb.damaged, 2);
    // Fire 0.6 + lava 1.2 + swarm 0.2 would be 2: the cap holds it to 1 a beat.
    let lost = h0 - w.demigods[0].health;
    assert!((lost - 1.0).abs() < 0.26, "lost {lost} (the cap is 1; lava that cooled a little)");
    assert!(lost <= st.earth_damage_cap + 1e-6);
    assert_eq!(w.demigods[2].health, far, "nobody else is touched");
    let row = w.ledger.rows.iter().rev().find(|r| r.targets.first() == Some(&Ref::Demigod(DemigodId(0)))).unwrap();
    assert!(row.act.contains("burned by the wildfire") || row.act.contains("caught in the lava"), "named for its worst part: {}", row.act);
    // The scale is a setting; zero turns the damage off and leaves the field running.
    let (mut w, pantheon) = world_with(&[(at, hell)]);
    w.demigods[0].pos = at;
    let mut st = settings(&w);
    st.earth_damage_scale = 0.0;
    let h0 = w.demigods[0].health;
    let eb = rules::step(&mut w, 4, &pantheon, &st).unwrap();
    assert_eq!((eb.damaged, w.demigods[0].health), (0, h0));
    assert!(eb.sim.c(grid().index_of(at)).fire > 0);
    // Each hazard alone, at full strength, is its own setting.
    for (cell, want, word) in [
        (Cell { fire: 255, ..Default::default() }, 0.6, "burned by the wildfire"),
        (Cell { lava: 255, ..Default::default() }, 1.2, "caught in the lava"),
        (Cell { water: 255, ..Default::default() }, 0.5, "swept up in the flood"),
        (Cell { shake: 255, ..Default::default() }, 0.5, "thrown down by the quake"),
        (Cell { snow: 200, heat: -100, ..Default::default() }, 0.2, "frozen in the blizzard"),
    ] {
        let (mut w, pantheon) = world_with(&[(at, cell)]);
        let st = settings(&w);
        w.demigods[0].pos = at;
        let h0 = w.demigods[0].health;
        rules::step(&mut w, 4, &pantheon, &st).unwrap();
        let lost = h0 - w.demigods[0].health;
        assert!(lost > want * 0.25 && lost <= want.min(1.0) + 0.01, "{word}: lost {lost}, at most {want}");
        assert!(w.ledger.rows.last().is_some_and(|r| r.act.contains(word)) || w.ledger.rows.iter().any(|r| r.act.contains(word)), "{word}");
    }
}

#[test]
fn gear_that_resists_the_element_halves_it() {
    use fn7_model::manifest::*;
    let at = [10.0, 50.0];
    let fire = Cell { fire: 255, ..Default::default() };
    let lost = |armoured: bool| {
        let (mut w, pantheon) = world_with(&[(at, fire)]);
        let st = settings(&w);
        w.demigods[0].pos = at;
        if armoured {
            // Dragonscale armour (an artifact): turns aside 0.5 of a hit (never more than half) and halves fire.
            let props = vec![Proposal {
                source: Source::Rules, intent: None,
                effect: Effect::CreateItem { name: "Dragonscale Mail".into(), description: "armour of dragonscale".into(), holder: Ref::Demigod(DemigodId(0)) },
                act: "armour".into(), actor: Ref::Demigod(DemigodId(0)), magnitude: 5, valence: 1, witnesses: vec![], cause: None,
            }];
            crate::merge::run(&mut w, props, &pantheon, 3, 8);
            let it = w.items.last_mut().expect("the armour");
            it.ext.kind = ItemKind::Armor;
            it.ext.tier = Tier::Artifact;
        }
        let kit = crate::wills::arms::kit(&w, DemigodId(0), &st);
        assert_eq!(kit.resists.iter().any(|r| r == "fire"), armoured, "{:?}", kit.resists);
        let h0 = w.demigods[0].health;
        rules::step(&mut w, 4, &pantheon, &st).unwrap();
        h0 - w.demigods[0].health
    };
    let (bare, clad) = (lost(false), lost(true));
    assert!(bare > 0.5, "bare: {bare}");
    assert!(clad <= bare * 0.26 + 0.01, "armour turns half aside and dragonscale halves the fire: {clad} against {bare}");
}

#[test]
fn roads_are_cut_shrines_toppled_settlements_ruined_and_encounters_uncovered() {
    let mut w0 = crate::testkit::world(4, 9);
    let sites: Vec<PlaceId> = w0.places.iter().filter(|p| p.kind == PlaceKind::Site).map(|p| p.id).collect();
    let (a, b) = (sites[0], *sites.iter().find(|s| grid().index_of(w0.places[s.ix()].center) != grid().index_of(w0.places[sites[0].ix()].center)).unwrap());
    let (ca, cb) = (w0.places[a.ix()].center, w0.places[b.ix()].center);
    w0.dm = Default::default();
    // A rift under one end of a road, a hard quake under a shrine.
    let (mut w, pantheon) = world_with(&[(ca, Cell { crack: 255, ..Default::default() }), (cb, Cell { shake: 255, ruin: 250, ..Default::default() })]);
    let st = settings(&w);
    w.dm.trails.push(Trail { a, b, wear: st.dm_road_wear * 2.0, road_since: Some(1), last_used: 3 });
    w.dm.shrines.push(Shrine { id: 0, cell: fn7_model::people_cell(cb), at: cb, region: fn7_society::geo::district_of(&w, b), place: b,
                               demigod: DemigodId(0), seat: 0, name: "the Shrine of Ash".into(), worshippers: 4000, level: 2,
                               power: ShrinePower::Ward, built_beat: 1, last_grant: 0 });
    let eb = rules::step(&mut w, 4, &pantheon, &st).unwrap();
    assert!(w.dm.trails[0].wear < st.dm_road_wear / 2.0, "worn below the road's own fading line");
    assert_eq!(w.dm.shrines[0].worshippers, 2000);
    let kinds: Vec<&str> = eb.entries.iter().map(|id| w.dm.memory[*id as usize].kind.as_str()).collect();
    assert!(kinds.contains(&"earth.road_cut") && kinds.contains(&"earth.shrine_toppled"), "{kinds:?}");
    let cut = w.dm.memory.iter().find(|m| m.kind == "earth.road_cut").unwrap();
    assert!(cut.text.starts_with("the road between ") && cut.text.ends_with(" is cut"));
    // Ruin pushes a settlement's target down by the rules' own count: one level per 96 points, three under the sea.
    let sim = rules::sim_of(&w, &st).unwrap();
    assert_eq!(rules::ruin_levels(&sim, &st, cb), 2);
    assert_eq!(rules::ruin_levels(&sim, &st, [0.0, 0.0]), 0);
    let mut half = st.clone();
    half.earth_ruin_scale = 0.5;
    assert_eq!(rules::ruin_levels(&sim, &half, cb), 1);
    // The road then fades by the DM's own step (it was not used this beat).
    let (alive, pos, health) = (vec![], w.demigods.iter().map(|d| d.pos).collect::<Vec<_>>(), w.demigods.iter().map(|d| d.health).collect::<Vec<_>>());
    let start = crate::dm::Start { alive: &alive, pos: &pos, health: &health, armies: &[], row_start: w.ledger.rows.len(),
                                   intent_start: w.intents.len(), manifest_start: w.manifests.len(), memory_start: w.dm.memory.len() };
    crate::dm::step(&mut w, 4, &start, &pantheon, &st);
    assert!(w.dm.memory.iter().any(|m| m.kind == "road.faded"), "the rules' own stepping fades the cut road");
    // An encounter uncovered by a crater: a marked hex like any other, told as uncovered.
    let land = super::tests::near([22.0, 0.0], true);
    let (mut w, pantheon) = world_with(&[]);
    let st = settings(&w);
    w.dm.earth.as_mut().unwrap().inputs.push(super::tests::input_at(4, METEOR, land, 10, TIER_MEGA));
    let eb = rules::step(&mut w, 4, &pantheon, &st).unwrap();
    assert_eq!(eb.report.impacts.len(), 1);
    let enc = w.dm.encounters.last().expect("an encounter in the crater");
    assert_eq!(grid().index_of(enc.at), grid().index_of(land));
    let m = w.dm.memory.iter().find(|m| m.kind == "encounter.marked").unwrap();
    assert!(m.text.starts_with("the crater uncovers") && m.data["uncovered_by"] == "the crater", "{}", m.text);
    assert!(w.dm.memory.iter().any(|m| m.kind == "earth.impact" && m.text.contains("left a crater")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ruined_city_shrinks_by_the_rules_own_steps() {
    // Everyone drops megas on one city for a dozen beats; the same match with the simulator off for comparison.
    let p = FinalNight7Plugin::new();
    let base = json!({ "seed": 44, "beats_total": 60, "llm": false, "clusters": 60, "players": 4, "bolt_start": 60, "entropy_per_mega": 0.5 });
    let first = p.on_match_start(&humans(4), &p.validate_settings(&base).unwrap(), &Failing).await.unwrap();
    let w0 = load(&first).unwrap();
    let city = w0.places.iter().filter(|x| x.kind == PlaceKind::Site).max_by_key(|x| w0.nodes[x.id.ix()].population).unwrap();
    let (city_id, at) = (city.id, city.center);
    let run = |on: bool| {
        let mut s = base.clone();
        s["earth_sim_enabled"] = on.into();
        async move {
            let p = FinalNight7Plugin::new();
            let script = |_: Seat, _: u32| -> Option<Action> { None };
            let extras = move |round: u32| -> (Vec<Action>, RoundContext) {
                let powers = ["earthquake", "meteor", "volcano", "wildfire"];
                ((0..4u8).filter(|_| round <= 12).map(|s| instant(s, (round * 10 + s as u32) as u64,
                    json!({"cast": {"power": powers[(round as usize + s as usize) % 4], "target": at, "tier": "mega"}}))).collect(), RoundContext::default())
            };
            let mut last = Value::Null;
            crate::driver::run_match_with(&p, &humans(4), &s, &Failing, Some(22), &script, &extras, |_: &BeatReport, st: &Value| {
                last = st.clone();
                true
            }).await.unwrap();
            load(&last).unwrap()
        }
    };
    let (on, off) = (run(true).await, run(false).await);
    let level = |w: &World| w.dm.settlements.iter().find(|s| s.place == city_id).map(|s| s.level).unwrap();
    let founded = w0.dm.settlements.iter().find(|s| s.place == city_id).map(|s| s.level);
    assert!(level(&on) < level(&off), "ruined: level {} against {} (founded {founded:?})", level(&on), level(&off));
    let drops: Vec<(u32, u64, u64)> = on.dm.memory.iter().filter(|m| m.kind == "settlement.shrank" && m.data["place"] == json!(Ref::Place(city_id)))
        .map(|m| (m.beat, m.data["from"].as_u64().unwrap(), m.data["to"].as_u64().unwrap())).collect();
    assert!(!drops.is_empty() && drops.iter().all(|d| d.1 == d.2 + 1), "one level at a time: {drops:?}");
    assert!(drops.windows(2).all(|x| x[1].0 - x[0].0 >= settings(&on).dm_settlement_step_beats), "{drops:?}");
}
