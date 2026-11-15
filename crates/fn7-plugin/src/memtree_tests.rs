//! The tree of consequence (`game.memory_tree`): the tree is the memory exactly (every count re-derived from the
//! state), recall paths follow one fact through the real prompt builder, hidden facts are counted and never shown,
//! the answer is deterministic and bounded, and a past beat is the memory as it stood.

use std::sync::OnceLock;

use fn7_model::dm::{DmEntry, Scope};
use fn7_model::hexgrid::{cell_at, people_cell, HexCell, HexLevel};
use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::*;

/// One played planet state (20 beats), built once for every test here.
fn state() -> &'static Value {
    static S: OnceLock<Value> = OnceLock::new();
    S.get_or_init(|| {
        std::thread::spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(testkit::played_state(9, 20, 100))
        })
        .join()
        .unwrap()
    })
}

fn q(st: &Value, seat: Option<Seat>, args: Value) -> Value {
    FinalNight7Plugin::new().query(st, seat, "memory_tree", &args).unwrap_or_else(|e| panic!("memory_tree {args}: {e}"))
}

/// Every node with its parent's id, depth first.
fn nodes<'a>(n: &'a Value, parent: Option<&'a Value>, out: &mut Vec<(&'a Value, Option<&'a Value>)>) {
    out.push((n, parent));
    for c in n["children"].as_array().unwrap() {
        nodes(c, Some(n), out);
    }
}

fn all_nodes(v: &Value) -> Vec<(&Value, Option<&Value>)> {
    let mut out = vec![];
    nodes(&v["tree"], None, &mut out);
    out
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("not a count: {v}"))
}

fn region_of(e: &DmEntry) -> Option<&str> {
    if e.place == "world" {
        None
    } else if e.up.len() <= 1 {
        Some(&e.place)
    } else {
        Some(&e.up[e.up.len() - 2])
    }
}

fn slot_of(e: &DmEntry) -> Option<String> {
    let r = region_of(e)?;
    if e.place == r {
        return None;
    }
    Some(match e.scope {
        Scope::Cell if e.up.len() >= 3 => e.up[0].clone(),
        Scope::Cell => format!("open:{r}"),
        _ => e.place.clone(),
    })
}

fn hex_of(e: &DmEntry, l: HexLevel) -> Option<String> {
    let c = HexCell::parse(&e.place)?;
    (c.level <= l).then(|| if c.level == l { e.place.clone() } else { cell_at(c.center(), l).key() })
}

/// How many entries a DM node holds (re-derived from the whole memory, not the query's window).
fn dm_count(w: &World, n: &Value, slot: Option<&str>) -> usize {
    let id = n["id"].as_str().unwrap();
    let m = &w.dm.memory;
    match n["kind"].as_str().unwrap() {
        "world" => m.len(),
        "region" => m.iter().filter(|e| region_of(e) == Some(id)).count(),
        "settlement" | "open" => m.iter().filter(|e| slot_of(e).as_deref() == Some(id)).count(),
        "hex" => {
            let l = HexLevel::parse(n["hex_level"].as_str().unwrap()).unwrap();
            m.iter().filter(|e| e.scope == Scope::Cell && slot_of(e).as_deref() == slot && hex_of(e, l).as_deref() == Some(id)).count()
        }
        k => panic!("not a DM node: {k}"),
    }
}

#[test]
fn the_tree_is_the_memory_exactly() {
    let st = state();
    let w = world::load(st).unwrap();
    for args in [json!({ "per_node": 12 }), json!({ "focus": "dg:1", "per_node": 12 }), json!({ "focus": people_cell(w.demigods[2].pos), "per_node": 12 })] {
        let v = q(st, None, args.clone());
        assert_eq!(v["bounds"]["dm_window"]["complete"], true, "beat 20 fits the window");
        assert_eq!(u(&v["totals"]["dm_memory"]["entries"]), w.dm.memory.len() as u64);
        assert_eq!(u(&v["totals"]["ledger_rows"]), w.ledger.rows.len() as u64);
        assert_eq!(u(&v["totals"]["persons_known"]), w.characters.len() as u64);
        let ns = all_nodes(&v);
        assert!(ns.iter().any(|(n, _)| n["kind"] == "hex" && n["hex_level"] == "people"), "{args}: the tree reaches a people hex");
        for (n, parent) in &ns {
            let total = u(&n["facts_total"]);
            assert!(u(&n["facts_here"]) <= total && u(&n["private_facts"]) <= total, "{args} {}", n["id"]);
            let kinds: u64 = n["kinds"].as_object().unwrap().values().map(u).sum();
            let fams: u64 = n["families"].as_object().unwrap().values().map(u).sum();
            assert_eq!((kinds, fams), (total, total), "{args} {}: kinds and families add up", n["id"]);
            if let Some(p) = parent {
                assert!(u(&n["level"]) > u(&p["level"]), "{args}: {} below {}", n["id"], p["id"]);
            }
            if n["store"] != "dm" || n["kind"] == "demigod" {
                continue;
            }
            // A hex's settlement slot: the settlement (or open country) on the path; hexes only hang under it.
            let slot = ns.iter().find(|(m, _)| m["level"] == 2 && m["on_path"] == true).map(|(m, _)| m["id"].as_str().unwrap());
            assert_eq!(dm_count(&w, n, slot) as u64, total, "{args} {} {}", n["kind"], n["id"]);
            if n["on_path"] != true {
                continue;
            }
            // Facts here plus every child's are the node's (when no child was cut off the list).
            let rolled = n["rolled_up_from"].as_array().unwrap();
            if rolled.len() < 12 {
                let sum: u64 = rolled.iter().map(|r| u(&r["facts"])).sum();
                assert_eq!(u(&n["facts_here"]) + sum, total, "{args} {}: here + rolled up", n["id"]);
            }
            // Its latest roll-up cites exactly what it counted.
            if let Some(r) = n["rollup"].as_object() {
                assert_eq!(r["exact"], true, "{args} {}", n["id"]);
                assert_eq!(r["cites_total"], r["facts"]);
                let cites = r["cites"].as_array().unwrap();
                if cites.len() as u64 == u(&r["cites_total"]) {
                    let mut by: std::collections::BTreeMap<String, u64> = Default::default();
                    for c in cites {
                        *by.entry(w.dm.memory[u(c) as usize].kind.clone()).or_default() += 1;
                    }
                    assert_eq!(json!(by), Value::Object(r["counts"].as_object().unwrap().clone()), "{args} {}", n["id"]);
                }
                let e = &w.dm.memory[u(&r["id"]) as usize];
                assert_eq!(e.kind, "memory.rollup");
                assert_eq!(e.place, n["key"].as_str().or(n["id"].as_str()).unwrap());
            }
            for s in n["sample_facts"].as_array().unwrap() {
                let e = &w.dm.memory[u(&s["id"]) as usize];
                assert!(e.public && e.beat == u(&s["beat"]) as u32 && e.kind == s["kind"].as_str().unwrap());
            }
        }
    }
}

#[test]
fn recall_paths_follow_one_fact_to_the_prompt() {
    let st = state();
    let w = world::load(st).unwrap();
    let v = q(st, None, json!({}));
    let paths = v["recall_paths"].as_array().unwrap();
    assert!((3..=5).contains(&paths.len()), "{} paths", paths.len());
    let d20 = paths.iter().find(|p| p["fact"] == "d20").expect("a d20 path");
    let steps = d20["steps"].as_array().unwrap();
    let key = steps[0]["key"].as_str().unwrap();
    assert_eq!(steps[0]["reproduces"], true, "the seeded die gives the stored face again");
    let e = w.dm.decided(key).expect("the key is decided");
    assert_eq!(steps[1]["roll"], e.data["roll"]);
    assert_eq!(steps[1]["target_dc"], e.data["target_dc"]);
    assert_eq!(steps[2]["entry"]["id"], json!(e.id));
    // Asked again, the DM gives the same die (whatever the modifier).
    assert_eq!(json!(crate::dm::roll_d20(&w, key, 7, 3).roll), e.data["roll"]);
    for r in steps[3]["items"].as_array().unwrap() {
        assert_eq!(r["cites_this"], true);
        let rid = u(&r["id"]) as usize;
        assert_eq!(w.dm.memory[rid].query.as_deref(), r["query_key"].as_str());
    }
    assert!(!steps[3]["items"].as_array().unwrap().is_empty(), "a public roll rolls up");
    let prompts = steps[4]["items"].as_array().unwrap();
    let dm = prompts.iter().find(|p| p["prompt"] == "dm_memory").unwrap();
    assert_eq!(dm["included"], false, "rolls are left out of the DM prompt");
    // The lines are the real prompt builder's on the whole memory.
    let region: PlaceId = match dm["place"].as_str().unwrap().parse::<Ref>().unwrap() {
        Ref::Place(p) => p,
        r => panic!("{r}"),
    };
    let st_ = world::settings(&w);
    let real = crate::dm::prompt_lines(&w, region, w.meta.beat, st_.dm_memory_lines as usize);
    assert_eq!(dm["lines"], json!(real));
    if let Some(s) = paths.iter().find(|p| p["fact"] == "settled") {
        let p = s["steps"].as_array().unwrap().iter().find(|x| x["step"] == "prompts").unwrap();
        let dm = p["items"].as_array().unwrap().iter().find(|x| x["prompt"] == "dm_memory").unwrap();
        assert_eq!(dm["included"], true);
        assert!(dm["lines"].as_array().unwrap().contains(&dm["line"]));
    }
    if let Some(p) = paths.iter().find(|p| p["fact"] == "person_memory") {
        let m = p["steps"].as_array().unwrap().iter().find(|x| x["step"] == "mind_memory").unwrap();
        let item = &w.memory.items[u(&m["memory"]) as usize];
        assert_eq!(json!(item.text), m["text"]);
        assert_eq!(json!(item.owner), m["owner"]);
        let row = p["steps"].as_array().unwrap().iter().find(|x| x["step"] == "ledger_row").unwrap();
        assert_eq!(row["public"], true, "a public answer shows only memories of public events");
    }
}

#[test]
fn hidden_facts_are_counted_never_shown() {
    let mut w = world::load(state()).unwrap();
    let beat = w.meta.beat;
    let d0 = world::seat_state(&w, 0).unwrap().demigod;
    let at = w.demigods[d0.ix()].pos;
    let cell = people_cell(at);
    let wh = crate::dm::locate(&w, at, HexLevel::People);
    let (_, up) = wh.key(Scope::Cell);
    for (text, seats) in [("SECRET-DM-0 seat 0's own", json!([0])), ("SECRET-DM-X nobody's", json!([]))] {
        let id = w.dm.memory.len() as u32;
        w.dm.memory.push(DmEntry { id, beat, scope: Scope::Cell, place: cell.clone(), up: up.clone(), kind: "quest.given".into(), query: None,
                                   public: false, text: text.into(), data: json!({ "seats": seats }) });
    }
    // A private row only seat 0's hand knows, and a person's memory of it.
    let hand = w.demigods[d0.ix()].hands[0];
    let row = w.ledger.next_id();
    w.ledger.rows.push(LedgerRow {
        id: row, beat, source: Source::Agent { agent: hand }, actor: Ref::Agent(hand), verb: "whisper".into(), act: "SECRET-ROW-0".into(),
        targets: vec![], place: None, magnitude: 2, valence: 0, witnessed_by: vec![], known_by: Default::default(), known_at: vec![],
        told_to: vec![], erased_from: vec![], debt: None, cause: None, intent: None, at: Some(at),
    });
    let me0 = world::selves(&w, d0);
    let x = w
        .characters
        .iter()
        .find(|c| c.alive && matches!(c.origin, Origin::Materialized { .. }) && !me0.contains(&Ref::Char(c.id)) && c.memorized_by.is_none_or(|m| !me0.contains(&m)))
        .unwrap()
        .id;
    let id = w.memory.next_id();
    w.memory.items.push(MemoryItem { id, owner: Ref::Char(x), beat, kind: MemoryKind::Witnessed, text: "SECRET-MEM-0 heard it".into(), about: vec![],
                                     ledger: Some(row), from: None, salience: 9, confidence: 1.0, forgotten: false, folded_into: None, folded_beat: None });
    let st = world::save(&w);
    let before = q(state(), None, json!({ "focus": cell, "per_node": 12 }));
    for seat in [None, Some(1)] {
        for args in [json!({ "focus": cell, "per_node": 12 }), json!({ "focus": Ref::Char(x).to_string(), "per_node": 12 }), json!({ "per_node": 12 })] {
            let v = q(&st, seat, args.clone());
            assert!(!v.to_string().contains("SECRET"), "{args} seat {seat:?} leaks a secret");
            assert!(!v.to_string().contains("\"health\""), "{args}: no health in a public answer");
        }
        // Counted where they are.
        let v = q(&st, seat, json!({ "focus": cell, "per_node": 12 }));
        let hex = |v: &Value| all_nodes(v).into_iter().find(|(n, _)| n["id"] == cell.as_str()).map(|(n, _)| n.clone()).unwrap();
        assert_eq!(u(&hex(&v)["private_facts"]), u(&hex(&before)["private_facts"]) + 2);
        assert_eq!(u(&v["totals"]["dm_memory"]["private"]), u(&before["totals"]["dm_memory"]["private"]) + 2);
    }
    // Seat 0 sees its own private entry and the memory of a row it knows, never the other one.
    let v = q(&st, Some(0), json!({ "focus": cell, "per_node": 12 })).to_string();
    assert!(v.contains("SECRET-DM-0") && !v.contains("SECRET-DM-X"), "{}", &v[..400.min(v.len())]);
    let v = q(&st, Some(0), json!({ "focus": Ref::Char(x).to_string(), "per_node": 12 })).to_string();
    assert!(v.contains("SECRET-MEM-0"), "seat 0 knows the row the memory is about");
}

#[test]
fn deterministic_and_bounded() {
    let st = state();
    for args in [json!({}), json!({ "focus": "dg:0" }), json!({ "focus": "pl:1", "depth": 3 })] {
        let a = q(st, None, args.clone());
        assert_eq!(a, q(st, None, args.clone()), "{args}: twice the same");
        assert_eq!(a, q(&world::save(&world::load(st).unwrap()), None, args.clone()), "{args}: a reloaded state");
        assert!(u(&a["bounds"]["ledger_rows_read"]) <= crate::queries::SCAN_CAP as u64);
    }
    // A memory longer than the scan cap: only the newest SCAN_CAP entries are read, and the reply says so.
    let mut w = world::load(st).unwrap();
    let beat = w.meta.beat;
    let tail: Vec<DmEntry> = w.dm.memory.iter().filter(|e| e.beat == beat).cloned().collect();
    while w.dm.memory.len() < crate::queries::SCAN_CAP + 500 {
        for e in &tail {
            let mut e = e.clone();
            e.id = w.dm.memory.len() as u32;
            e.query = None;
            w.dm.memory.push(e);
        }
    }
    let v = q(&world::save(&w), None, json!({}));
    assert_eq!(v["bounds"]["dm_window"]["complete"], false);
    assert_eq!(u(&v["bounds"]["dm_window"]["entries"]), crate::queries::SCAN_CAP as u64);
    assert_eq!(u(&v["tree"]["facts_total"]), crate::queries::SCAN_CAP as u64);
    assert!(u(&v["bounds"]["minds_window"]["items"]) <= crate::queries::SCAN_CAP as u64);
    // The size estimate is close to the real thing.
    let real = serde_json::to_vec(st).unwrap().len() as f64;
    let est = u(&q(st, None, json!({}))["totals"]["state_bytes"]["total"]) as f64;
    assert!((est / real - 1.0).abs() < 0.25, "estimate {est} vs {real}");
}

#[test]
fn a_past_beat_is_the_memory_as_it_stood() {
    let st = state();
    let w = world::load(st).unwrap();
    let b = 8;
    let v = q(st, None, json!({ "beat": b, "per_node": 12 }));
    assert_eq!(v["as_of"], json!({ "beat": b, "memory": "exact", "society": "now", "earth": "omitted" }));
    assert_eq!(u(&v["tree"]["facts_total"]), w.dm.memory.iter().filter(|e| e.beat <= b).count() as u64);
    for (n, _) in all_nodes(&v) {
        assert!(n["last_beat"].as_u64().is_none_or(|l| l <= b as u64));
        for s in n["sample_facts"].as_array().unwrap() {
            assert!(u(&s["beat"]) <= b as u64);
        }
        assert!(n.get("earth_line").is_none_or(Value::is_null));
    }
    // The same DM tree as a state whose memory ends at that beat.
    let mut cut = w.clone();
    cut.dm.memory.retain(|e| e.beat <= b);
    let c = q(&world::save(&cut), None, json!({ "beat": b, "per_node": 12 }));
    assert_eq!(v["tree"]["kinds"], c["tree"]["kinds"]);
    assert_eq!(v["tree"]["children"][0]["id"], c["tree"]["children"][0]["id"]);
    assert_eq!(v["recall_paths"].as_array().unwrap().iter().find(|p| p["fact"] == "d20"),
               c["recall_paths"].as_array().unwrap().iter().find(|p| p["fact"] == "d20"));
}

#[test]
fn bad_args_are_refused() {
    let st = state();
    let p = FinalNight7Plugin::new();
    for args in [json!({ "beat": 999 }), json!({ "depth": 0 }), json!({ "depth": 7 }), json!({ "per_node": 13 }), json!({ "focus": "xx" }),
                 json!({ "focus": "it:0" }), json!({ "focus": "pl:99999" }), json!({ "focus": 3 }), json!({ "depth": "deep" })] {
        assert!(p.query(st, None, "memory_tree", &args).is_err(), "{args} accepted");
    }
    // HTTP-style string args work.
    let v = q(st, None, json!({ "depth": "2", "per_node": "3", "beat": "4" }));
    assert_eq!((u(&v["depth"]), u(&v["per_node"]), u(&v["as_of"]["beat"])), (2, 3, 4));
    assert!(all_nodes(&v).iter().all(|(n, _)| u(&n["level"]) <= 2));
}
