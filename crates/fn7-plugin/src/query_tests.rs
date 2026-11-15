//! The `game.*` queries: shapes, pagination, hidden information, and agreement with the views they mirror.

use std::sync::OnceLock;

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::*;

/// One played planet state (20 beats, everyone kept alive), built once for every test here.
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

fn plugin() -> &'static FinalNight7Plugin {
    static P: OnceLock<FinalNight7Plugin> = OnceLock::new();
    P.get_or_init(FinalNight7Plugin::new)
}

fn q(state: &Value, seat: Option<Seat>, name: &str, args: Value) -> Value {
    plugin()
        .query(state, seat, name, &args)
        .unwrap_or_else(|e| panic!("{name} {args}: {e}"))
}

fn qerr(state: &Value, seat: Option<Seat>, name: &str, args: Value) -> String {
    plugin().query(state, seat, name, &args).expect_err(name)
}

fn keys(v: &Value, want: &[&str]) {
    for k in want {
        assert!(v.get(*k).is_some(), "missing `{k}` in {}", trunc(v));
    }
}

fn trunc(v: &Value) -> String {
    v.to_string().chars().take(600).collect()
}

fn region_refs(w: &World) -> Vec<String> {
    w.places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .map(|p| Ref::Place(p.id).to_string())
        .collect()
}

fn some_army(w: &World) -> Option<ClusterId> {
    fn7_society::armies(w).map(|(c, _)| c.id).next()
}

#[test]
fn manifest_lists_every_query_with_an_args_schema() {
    let specs = plugin().queries();
    let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["globe", "region", "node", "person", "army", "events", "me", "powers", "gods", "standings", "present", "media", "items", "item", "memory",
         "encounters", "fault", "faults_overview", "piles_final"]
    );
    for s in &specs {
        assert_eq!(s.args_schema["type"], "object", "{}", s.name);
        assert!(!s.description.is_empty());
        assert_eq!(s.public, s.name != "me", "{} public", s.name);
    }
}

#[test]
fn every_query_has_its_shape() {
    let st = state();
    let w = world::load(st).unwrap();
    let g = q(st, None, "globe", json!({}));
    keys(&g, &["beat", "beats_total", "final_night", "arena", "alive", "entropy", "demigods", "influence", "mods", "armies", "events"]);
    keys(&g["arena"], &["center", "radius_km", "start_radius_km", "end_radius_km", "city"]);
    keys(&g["events"], &["items", "since", "next", "more", "head"]);
    assert_eq!(g["beat"], 20);
    assert_eq!(g["demigods"].as_array().unwrap().len(), 8, "two demigods per player (rules 0.7.0)");
    assert_eq!(g["players"].as_array().unwrap().len(), 4);
    for d in g["demigods"].as_array().unwrap().iter().filter(|d| d["alive"] == true) {
        keys(d, &["ref", "side", "player", "aura", "cell", "pos", "items", "visual"]);
        assert!(d.get("health").is_none(), "rival health stays hidden, per demigod");
    }
    for e in g["events"]["items"].as_array().unwrap() {
        keys(e, &["row", "beat", "kind", "text", "at", "region", "region_ref", "refs", "magnitude"]);
    }
    for a in g["armies"].as_array().unwrap() {
        keys(a, &["ref", "name", "demigod_ref", "size", "pos", "region", "dest", "target", "stance", "morale"]);
        assert!(a.get("orders").is_none());
    }

    let reg = &region_refs(&w)[0];
    let r = q(st, None, "region", json!({ "ref": reg }));
    keys(&r, &["ref", "name", "center", "radius_km", "node", "cities", "crowds", "mobs", "armies", "demigods", "mods", "influence", "events"]);
    keys(&r["node"], &["population", "mood", "stance", "level"]);
    keys(&r["crowds"], &["items", "next_cursor", "total"]);
    assert!(!r["cities"].as_array().unwrap().is_empty());

    let city = r["cities"][0]["ref"].as_str().unwrap().to_string();
    let n = q(st, None, "node", json!({ "ref": city }));
    assert_eq!(n["kind"], "place");
    keys(&n, &["node", "children", "crowds"]);
    let crowd = w.clusters.iter().find(|c| c.army.is_none() && c.size > 0).unwrap().id;
    let n = q(st, None, "node", json!({ "ref": Ref::Cluster(crowd).to_string() }));
    assert_eq!(n["kind"], "crowd");
    keys(&n["node"], &["size", "mood", "place", "cells"]);
    keys(&n["people"], &["items", "next_cursor", "total"]);
    if let Some(cell) = w.cells.first() {
        let n = q(st, None, "node", json!({ "ref": Ref::Cell(cell.id).to_string() }));
        assert_eq!(n["kind"], "cell");
    }
    if let Some(m) = w.mobs.first() {
        let n = q(st, None, "node", json!({ "ref": Ref::Mob(m.id).to_string() }));
        assert_eq!(n["kind"], "mob");
    }

    let ch = w.characters.iter().find(|c| c.life.is_some()).unwrap().id;
    let p = q(st, None, "person", json!({ "ref": Ref::Char(ch).to_string() }));
    keys(&p, &["ref", "name", "alive", "role", "rank", "place", "place_name", "region", "at", "died_beat"]);
    let p = q(st, Some(0), "person", json!({ "ref": Ref::Char(ch).to_string() }));
    keys(&p, &["yours", "follower", "allegiance", "memories"]);

    let k = some_army(&w).expect("an army at beat 20");
    let a = q(st, None, "army", json!({ "ref": Ref::Cluster(k).to_string() }));
    keys(&a, &["ref", "general", "captains", "size", "pos", "morale", "order", "live", "yours", "units"]);

    let e = q(st, None, "events", json!({}));
    keys(&e, &["items", "next", "more", "head", "beat"]);

    let me = q(st, Some(1), "me", json!({}));
    keys(&me, &["seat", "demigod", "you", "hands", "debts", "news", "armies", "orders"]);
    assert_eq!(me["seat"], 1);

    let pw = q(st, None, "powers", json!({}));
    assert!(pw["powers"].as_array().unwrap().iter().any(|p| p["slug"] == "lightning"));
    assert_eq!(pw["powers"].as_array().unwrap().last().unwrap()["slug"], "custom");
    let gd = q(st, None, "gods", json!({}));
    assert_eq!(gd["gods"].as_array().unwrap().len(), 7);
    keys(&gd["gods"][6], &["ref", "name", "slug", "domain", "health", "unmade"]);
    let sd = q(st, None, "standings", json!({}));
    keys(&sd, &["beat", "alive", "over", "standings"]);
    keys(&sd["standings"][0], &["rank", "demigod", "name", "seat", "alive", "score", "bolts"]);
    assert_eq!(sd["over"], false);
}

/// The globe and `me` say exactly what the views say (they are the same builders on a partial state).
#[test]
fn globe_and_me_agree_with_the_views() {
    let st = state();
    let w = world::load(st).unwrap();
    let pv = phases::public_view(&w);
    let g = q(st, None, "globe", json!({}));
    for k in ["demigods", "influence", "mods", "entropy", "arena", "alive", "beat"] {
        assert_eq!(g[k], pv[k], "globe.{k}");
    }
    let pv_armies: Vec<&Value> = pv["armies"].as_array().unwrap().iter().collect();
    assert_eq!(g["armies"].as_array().unwrap().len(), pv_armies.len());
    for s in 0..4 {
        assert_eq!(q(st, Some(s), "me", json!({})), phases::seat_view(&w, s), "me for seat {s}");
    }
}

#[test]
fn events_page_by_cursor_without_gaps_or_repeats() {
    let st = state();
    let w = world::load(st).unwrap();
    let expect: Vec<String> = w
        .ledger
        .rows
        .iter()
        .filter(|r| payload::is_public_event(&w, r))
        .map(|r| Ref::Ledger(r.id).to_string())
        .collect();
    assert!(expect.len() > 20, "{} public events", expect.len());
    let mut got = vec![];
    let mut since = 0u64;
    for _ in 0..1000 {
        // HTTP sends strings: numbers as numeric strings work too.
        let p = q(st, None, "events", json!({ "since": since.to_string(), "limit": "7" }));
        let items = p["items"].as_array().unwrap();
        assert!(items.len() <= 7);
        got.extend(items.iter().map(|e| e["row"].as_str().unwrap().to_string()));
        let next = p["next"].as_u64().unwrap();
        assert!(next > since || !p["more"].as_bool().unwrap());
        since = next;
        if !p["more"].as_bool().unwrap() {
            break;
        }
    }
    assert_eq!(got, expect);
    assert_eq!(since as usize, w.ledger.rows.len(), "the last page points at the head");
    // Polling from the head is empty until something happens.
    let p = q(st, None, "events", json!({ "since": since }));
    assert_eq!(p["items"], json!([]));
    assert_eq!(p["more"], false);
    // Without a cursor: the latest, oldest first, and `next` is the head.
    let p = q(st, None, "events", json!({ "limit": 5 }));
    let latest: Vec<&str> = p["items"].as_array().unwrap().iter().map(|e| e["row"].as_str().unwrap()).collect();
    assert_eq!(latest, expect[expect.len() - 5..].iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(p["next"].as_u64().unwrap() as usize, w.ledger.rows.len());
    // One region's events are all in it.
    let mut seen_regional = 0;
    for reg in region_refs(&w) {
        let p = q(st, None, "events", json!({ "region": reg, "since": 0, "limit": 200 }));
        for e in p["items"].as_array().unwrap() {
            assert_eq!(e["region_ref"], json!(reg));
            seen_regional += 1;
        }
    }
    assert!(seen_regional > 0);
    // The globe carries the same latest page.
    let g = q(st, None, "globe", json!({ "limit": 5 }));
    assert_eq!(g["events"]["items"], p_items(&q(st, None, "events", json!({ "limit": 5 }))));
}

fn p_items(v: &Value) -> Value {
    v["items"].clone()
}

#[test]
fn lists_page_with_cursors() {
    let st = state();
    let w = world::load(st).unwrap();
    // The region with the most crowds, two at a time.
    let (reg, n) = region_refs(&w)
        .into_iter()
        .map(|r| {
            let t = q(st, None, "region", json!({ "ref": r }))["crowds"]["total"].as_u64().unwrap();
            (r, t)
        })
        .max_by_key(|x| x.1)
        .unwrap();
    assert!(n >= 3, "{n} crowds");
    let mut got = vec![];
    let mut cursor: Option<u64> = None;
    loop {
        let mut args = json!({ "ref": reg, "limit": 2 });
        if let Some(c) = cursor {
            args["cursor"] = c.into();
        }
        let p = q(st, None, "region", args)["crowds"].clone();
        let items = p["items"].as_array().unwrap();
        assert!(items.len() <= 2);
        got.extend(items.iter().map(|c| (c["ref"].as_str().unwrap().to_string(), c["size"].as_u64().unwrap())));
        match p["next_cursor"].as_u64() {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    assert_eq!(got.len() as u64, n);
    let mut refs: Vec<&String> = got.iter().map(|x| &x.0).collect();
    refs.dedup();
    assert_eq!(refs.len() as u64, n, "no repeats");
    assert!(got.windows(2).all(|x| x[0].1 >= x[1].1), "biggest first");
}

#[test]
fn bad_args_are_refused() {
    let st = state();
    assert!(qerr(st, None, "region", json!({})).contains("required"));
    assert!(qerr(st, None, "region", json!({ "ref": "ch:1" })).contains("not a place"));
    assert!(qerr(st, None, "region", json!({ "ref": "pl:0" })).contains("not a region"));
    assert!(qerr(st, None, "region", json!({ "ref": "pl:1", "limit": 0 })).contains("limit"));
    assert!(qerr(st, None, "events", json!({ "since": "soon" })).contains("since"));
    assert!(qerr(st, None, "events", json!({ "limit": 1000 })).contains("limit"));
    assert!(qerr(st, None, "node", json!({ "ref": "ch:1" })).contains("not a node"));
    assert!(qerr(st, None, "node", json!({ "ref": "pl:99999" })).contains("no such node"));
    assert!(qerr(st, None, "person", json!({ "ref": "ch:99999" })).contains("no such person"));
    assert!(qerr(st, None, "army", json!({ "ref": "cl:0" })).contains("not an army"));
    assert!(qerr(st, None, "me", json!({})).contains("seat"));
    assert!(qerr(st, None, "nope", json!({})).contains("unknown"));
    assert!(qerr(st, None, "piles_final", json!({})).contains("not over"));
    // Unknown args (an ngrok `key`, say) are ignored; null args are an empty object.
    q(st, None, "globe", json!({ "key": "abc" }));
    q(st, None, "globe", Value::Null);
}

#[test]
fn piles_final_once_the_match_is_over() {
    let mut st = state().clone();
    let beat = st["meta"]["beat"].clone();
    st["meta"]["beats_total"] = beat;
    let w = world::load(&st).unwrap();
    let p = q(&st, None, "piles_final", json!({}));
    assert_eq!(p["piles"], phases::summary_base(&w).summary["piles"].clone().tap_tidy());
    assert_eq!(q(&st, None, "standings", json!({}))["over"], true);
}

trait TapTidy {
    fn tap_tidy(self) -> Self;
}
impl TapTidy for Value {
    fn tap_tidy(mut self) -> Self {
        payload::tidy(&mut self);
        self
    }
}

/// Plant one secret of each kind for seat 0 and check that no public answer (seat None, i.e. HTTP) and no other seat's
/// answer carries them, while seat 0 sees its own.
#[test]
fn hidden_information_stays_hidden() {
    let mut w = world::load(state()).unwrap();
    let d0 = world::seat_state(&w, 0).unwrap().demigod;
    let hand = w.demigods[d0.ix()].hands[0];
    w.demigods[d0.ix()].orders.push(StandingOrder {
        hand: Hand::Left,
        verb: "guard".into(),
        targets: vec![],
        place: None,
        magnitude: 3,
        span: "SECRET-ORDERS-0".into(),
        since: w.meta.beat,
    });
    // Seat 0's army, with secret standing orders and order text.
    let k = fn7_society::armies(&w).find(|(_, a)| a.demigod == d0 && !a.rogue).map(|(c, _)| c.id);
    let k = k.unwrap_or_else(|| {
        let k = some_army(&w).expect("an army");
        let a = w.clusters[k.ix()].army.as_mut().unwrap();
        a.demigod = d0;
        a.rogue = false;
        k
    });
    {
        let a = w.clusters[k.ix()].army.as_mut().unwrap();
        a.orders = "SECRET-ARMY-0".into();
        if let Some(o) = a.order.as_mut() {
            o.text = "SECRET-ARMY-0 order".into();
        }
    }
    // A private row only seat 0's hand knows, and memories of someone else's about it.
    let at = w.demigods[d0.ix()].pos;
    let row = w.ledger.next_id();
    w.ledger.rows.push(LedgerRow {
        id: row,
        beat: w.meta.beat,
        source: Source::Agent { agent: hand },
        actor: Ref::Agent(hand),
        verb: "whisper".into(),
        act: "SECRET-ROW-0".into(),
        targets: vec![],
        place: None,
        magnitude: 2,
        valence: 0,
        witnessed_by: vec![],
        known_by: Default::default(),
        known_at: vec![],
        told_to: vec![],
        erased_from: vec![],
        debt: None,
        cause: None,
        intent: None,
        at: Some(at),
    });
    let me0 = world::selves(&w, d0);
    let x = w
        .characters
        .iter()
        .find(|c| c.alive && !me0.contains(&Ref::Char(c.id)) && c.memorized_by.is_none_or(|m| !me0.contains(&m)))
        .unwrap()
        .id;
    let mut mem = |text: &str, ledger: Option<LedgerId>| {
        let id = w.memory.next_id();
        w.memory.items.push(MemoryItem {
            id,
            owner: Ref::Char(x),
            beat: w.meta.beat,
            kind: MemoryKind::Witnessed,
            text: text.into(),
            about: vec![],
            ledger,
            from: None,
            salience: 5,
            confidence: 1.0,
            forgotten: false,
            folded_into: None,
            folded_beat: None,
        });
    };
    for i in 0..5 {
        mem(&format!("SECRET-MEM-0 #{i}"), Some(row));
    }
    mem("SECRET-MEMX nobody may see", None);
    let st = world::save(&w);

    let xr = Ref::Char(x).to_string();
    let kr = Ref::Cluster(k).to_string();
    let mut public: Vec<(String, Value)> = vec![];
    for seat in [None, Some(1)] {
        let mut ask = |name: &str, args: Value| public.push((format!("{name} {args} seat {seat:?}"), q(&st, seat, name, args)));
        ask("globe", json!({ "limit": 200 }));
        ask("events", json!({ "limit": 200 }));
        ask("events", json!({ "since": 0, "limit": 200 }));
        ask("standings", json!({}));
        ask("gods", json!({}));
        ask("powers", json!({}));
        ask("person", json!({ "ref": xr }));
        ask("army", json!({ "ref": kr }));
        for reg in region_refs(&w) {
            ask("region", json!({ "ref": reg, "limit": 50 }));
            ask("node", json!({ "ref": reg, "limit": 100 }));
        }
        for c in w.clusters.iter().filter(|c| c.army.is_none()).take(40) {
            ask("node", json!({ "ref": Ref::Cluster(c.id).to_string(), "limit": 100 }));
        }
    }
    public.push(("me seat 1".into(), q(&st, Some(1), "me", json!({}))));
    for (what, v) in &public {
        let s = v.to_string();
        assert!(!s.contains("SECRET"), "{what} leaks a secret: {}", trunc(v));
        if !what.starts_with("me ") && !what.starts_with("gods ") {
            assert!(!has_key(v, "health"), "{what} carries a health value");
        }
    }
    // The public subset of a person and an army: no allegiance, memories, orders or chain.
    let p = q(&st, None, "person", json!({ "ref": xr }));
    for k in ["allegiance", "memories", "yours", "follower"] {
        assert!(p.get(k).is_none(), "public person has {k}");
    }
    let a = q(&st, None, "army", json!({ "ref": kr }));
    assert!(a.get("orders").is_none() && a.get("chain").is_none());
    assert_eq!(a["yours"], false);
    // A public query is the same for every asker.
    for name in ["globe", "standings", "gods"] {
        assert_eq!(q(&st, None, name, json!({})), q(&st, Some(2), name, json!({})), "{name}");
    }

    // Seat 0 sees its own.
    let me = q(&st, Some(0), "me", json!({})).to_string();
    assert!(me.contains("SECRET-ORDERS-0") && me.contains("SECRET-ROW-0") && me.contains("SECRET-ARMY-0"));
    let a = q(&st, Some(0), "army", json!({ "ref": kr }));
    assert_eq!(a["yours"], true);
    assert_eq!(a["orders"], "SECRET-ARMY-0");
    assert!(a["chain"].is_object());
    // The memories it can see (about a row it knows), never the one it can't; two per page.
    let mut texts = vec![];
    let mut cursor: Option<u64> = None;
    loop {
        let mut args = json!({ "ref": xr, "limit": 2 });
        if let Some(c) = cursor {
            args["cursor"] = c.into();
        }
        let p = q(&st, Some(0), "person", args);
        assert_eq!(p["yours"], false);
        let m = &p["memories"];
        assert!(m["items"].as_array().unwrap().len() <= 2);
        texts.extend(m["items"].as_array().unwrap().iter().map(|i| i["text"].as_str().unwrap().to_string()));
        match m["next_cursor"].as_u64() {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    let secret: Vec<&String> = texts.iter().filter(|t| t.contains("SECRET")).collect();
    assert_eq!(secret.len(), 5, "{texts:?}");
    assert!(secret.iter().all(|t| t.starts_with("SECRET-MEM-0")), "newest first, no MEMX: {texts:?}");
    assert!(secret[0].ends_with("#4"), "newest first: {texts:?}");
}

fn has_key(v: &Value, key: &str) -> bool {
    match v {
        Value::Object(o) => o.iter().any(|(k, x)| k == key || has_key(x, key)),
        Value::Array(a) => a.iter().any(|x| has_key(x, key)),
        _ => false,
    }
}
