//! Every `game.*` query on a planet-scale state at beat 50 runs well under 5 ms (they run synchronously on the room
//! actor); the tree of consequence (`memory_tree`) under ~5 ms. Release only: `cargo test --release -p fn7-plugin --test query_timing -- --nocapture`.

use std::time::{Duration, Instant};

use fn7_model::*;
use fn7_plugin::{testkit, world, FinalNight7Plugin};
use serde_json::{json, Value};
use theatre_plugin_api::{GamePlugin, Seat};

#[tokio::test(flavor = "multi_thread")]
#[cfg_attr(debug_assertions, ignore = "timing is meaningful in release only")]
async fn every_query_is_cheap_at_beat_50() {
    let state = testkit::played_state(9, 50, 100).await;
    let w = world::load(&state).unwrap();
    let bytes = serde_json::to_vec(&state).unwrap().len();
    eprintln!(
        "state at beat {}: {} KB, {} ledger rows, {} memories, {} characters, {} crowds/armies, {} places",
        w.meta.beat, bytes / 1000, w.ledger.rows.len(), w.memory.items.len(), w.characters.len(), w.clusters.len(), w.places.len()
    );
    assert_eq!(w.meta.beat, 50);
    assert!(w.is_planet());

    // The heaviest arguments for each query.
    let region = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .max_by_key(|p| w.clusters.iter().filter(|c| fn7_society::geo::district_of(&w, c.place) == p.id).count())
        .unwrap()
        .id;
    let city = w.places.iter().find(|p| p.kind == PlaceKind::Site).unwrap().id;
    let crowd = w.clusters.iter().filter(|c| c.army.is_none()).max_by_key(|c| c.size).unwrap().id;
    let person = w
        .characters
        .iter()
        .max_by_key(|c| w.memory.items.iter().filter(|m| m.owner == Ref::Char(c.id)).count())
        .unwrap()
        .id;
    let (army, owner) = fn7_society::armies(&w).map(|(c, a)| (c.id, a.demigod)).next().expect("an army");
    let owner_seat = w.seats.iter().find(|s| s.demigod == owner).map(|s| s.seat);
    // The city with the most people (the people-and-houses boards centre on it).
    use fn7_model::hexgrid;
    let busy = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::Site)
        .max_by_key(|p| w.characters.iter().filter(|c| fn7_society::geo::distance_km(c.pos, p.center) < 30.0).count())
        .unwrap()
        .center;
    let mut over = state.clone();
    over["meta"]["beats_total"] = json!(w.meta.beat);

    // The worst case for `game.media` portraits: a person seat 0 may see but who is not public (every check runs).
    let p = FinalNight7Plugin::new();
    let private = w
        .characters
        .iter()
        .map(|c| c.id)
        .find(|c| p.query(&state, None, "media", &json!({ "kind": "portrait", "ref": Ref::Char(*c).to_string() })).is_err()
            && p.query(&state, Some(0), "media", &json!({ "kind": "portrait", "ref": Ref::Char(*c).to_string() })).is_ok())
        .unwrap_or(person);
    let public_row = w.ledger.rows.iter().rev().find(|r| fn7_plugin::payload::is_public_event(&w, r)).expect("a public row").id;
    let cases: Vec<(&str, Option<Seat>, Value, &Value)> = vec![
        ("globe", None, json!({}), &state),
        ("globe", None, json!({ "since": 0, "limit": 200 }), &state),
        ("region", None, json!({ "ref": Ref::Place(region).to_string(), "limit": 50 }), &state),
        ("node", None, json!({ "ref": Ref::Place(w.meta.arena).to_string(), "limit": 100 }), &state),
        ("node", None, json!({ "ref": Ref::Place(city).to_string(), "limit": 100 }), &state),
        ("node", None, json!({ "ref": Ref::Cluster(crowd).to_string(), "limit": 100 }), &state),
        ("person", None, json!({ "ref": Ref::Char(person).to_string() }), &state),
        ("person", Some(0), json!({ "ref": Ref::Char(person).to_string(), "limit": 100 }), &state),
        ("army", None, json!({ "ref": Ref::Cluster(army).to_string() }), &state),
        ("army", owner_seat, json!({ "ref": Ref::Cluster(army).to_string() }), &state),
        ("events", None, json!({ "limit": 200 }), &state),
        ("events", None, json!({ "since": 0, "limit": 200 }), &state),
        ("events", None, json!({ "since": 0, "limit": 200, "region": Ref::Place(region).to_string() }), &state),
        ("me", Some(0), json!({}), &state),
        ("me", Some(3), json!({}), &state),
        ("powers", None, json!({}), &state),
        ("gods", None, json!({}), &state),
        ("standings", None, json!({}), &state),
        ("piles_final", None, json!({}), &over),
        ("memory", None, json!({ "place": Ref::Place(region).to_string(), "limit": 200 }), &state),
        ("memory", None, json!({ "place": fn7_model::hexgrid::people_cell(w.demigods[0].pos) }), &state),
        ("node", None, json!({ "cell": fn7_model::hexgrid::people_cell(w.demigods[0].pos) }), &state),
        ("encounters", None, json!({}), &state),
        ("media", None, json!({ "kind": "portrait", "ref": "dg:0" }), &state),
        ("media", Some(0), json!({ "kind": "portrait", "ref": Ref::Char(person).to_string() }), &state),
        ("media", Some(0), json!({ "kind": "portrait", "ref": Ref::Char(private).to_string() }), &state),
        ("media", None, json!({ "kind": "place", "ref": Ref::Place(region).to_string() }), &state),
        ("media", None, json!({ "kind": "army", "ref": Ref::Cluster(army).to_string() }), &state),
        ("media", Some(0), json!({ "kind": "event", "ref": Ref::Ledger(public_row).to_string() }), &state),
        // People and houses: the biggest boards around the busiest city, at each level, and a hex zoomed into.
        ("people", None, json!({ "cell": hexgrid::cell_at(busy, hexgrid::HexLevel::Regions).key(), "limit": 300 }), &state),
        ("people", None, json!({ "cell": hexgrid::cell_at(busy, hexgrid::HexLevel::Villages).key(), "limit": 300 }), &state),
        ("people", None, json!({ "cell": hexgrid::cell_at(busy, hexgrid::HexLevel::Houses).key(), "limit": 300 }), &state),
        ("people", Some(0), json!({ "cell": hexgrid::people_cell(busy) }), &state),
        ("buildings", None, json!({ "cell": hexgrid::cell_at(busy, hexgrid::HexLevel::Regions).key() }), &state),
        ("buildings", None, json!({ "cell": hexgrid::cell_at(busy, hexgrid::HexLevel::Houses).key() }), &state),
        ("node", None, json!({ "cell": hexgrid::cell_at(busy, hexgrid::HexLevel::Houses).key(), "limit": 100 }), &state),
    ];
    let mut worst = Duration::ZERO;
    for (name, seat, args, st) in &cases {
        let mut times: Vec<Duration> = (0..40)
            .map(|_| {
                let t = Instant::now();
                let v = p.query(st, *seat, name, args).unwrap_or_else(|e| panic!("{name} {args}: {e}"));
                let d = t.elapsed();
                std::hint::black_box(v);
                d
            })
            .collect();
        times.sort();
        let (med, p90) = (times[times.len() / 2], times[times.len() * 9 / 10]);
        let size = serde_json::to_vec(&p.query(st, *seat, name, args).unwrap()).unwrap().len();
        eprintln!("{name:>12} {:>8} {:<64} median {med:>9.3?}  p90 {p90:>9.3?}  {size:>7} B", format!("{seat:?}"), args.to_string());
        assert!(med < Duration::from_micros(2500), "{name} {args}: median {med:?}");
        assert!(p90 < Duration::from_millis(5), "{name} {args}: p90 {p90:?}");
        worst = worst.max(med);
    }
    // The tree of consequence reads two stores (the newest 2,000 DM entries and minds' memories), the society and the
    // earth line: the heaviest query, held to the brief's ~5 ms (median under 3.5 ms).
    let tree_cases: Vec<(Option<Seat>, Value)> = vec![
        (None, json!({})),
        (None, json!({ "per_node": 12 })),
        (Some(0), json!({ "focus": "dg:0", "per_node": 12 })),
        (None, json!({ "focus": Ref::Char(person).to_string() })),
        (None, json!({ "focus": Ref::Place(region).to_string(), "per_node": 12 })),
        (None, json!({ "focus": fn7_model::hexgrid::people_cell(w.demigods[0].pos) })),
        (None, json!({ "beat": 25 })),
    ];
    for (seat, args) in &tree_cases {
        let mut times: Vec<Duration> = (0..40)
            .map(|_| {
                let t = Instant::now();
                let v = p.query(&state, *seat, "memory_tree", args).unwrap_or_else(|e| panic!("memory_tree {args}: {e}"));
                let d = t.elapsed();
                std::hint::black_box(v);
                d
            })
            .collect();
        times.sort();
        let (med, p90) = (times[times.len() / 2], times[times.len() * 9 / 10]);
        let size = serde_json::to_vec(&p.query(&state, *seat, "memory_tree", args).unwrap()).unwrap().len();
        eprintln!("{:>12} {:>8} {:<64} median {med:>9.3?}  p90 {p90:>9.3?}  {size:>7} B", "memory_tree", format!("{seat:?}"), args.to_string());
        assert!(med < Duration::from_micros(3500), "memory_tree {args}: median {med:?}");
        assert!(p90 < Duration::from_millis(5), "memory_tree {args}: p90 {p90:?}");
    }
    eprintln!("worst median {worst:?}; a full world::load alone: {:?}", {
        let t = Instant::now();
        std::hint::black_box(world::load(&state).unwrap());
        t.elapsed()
    });
}
