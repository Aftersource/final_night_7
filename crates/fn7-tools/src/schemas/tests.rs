use std::path::PathBuf;

use serde_json::{json, Value};

use super::*;

fn docs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

fn load(p: PathBuf) -> Value {
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn schema(stem: &str) -> Value {
    load(docs().join(format!("schemas/{stem}.schema.json")))
}

fn example(t: &str) -> Value {
    load(docs().join(format!("examples/{t}.json")))
}

/// docs/schemas matches what `fn7-schemas` generates from the code (run it after changing a payload).
#[test]
fn schemas_on_disk_are_current() {
    for (stem, s) in all() {
        let p = docs().join(format!("schemas/{stem}.schema.json"));
        let on_disk = std::fs::read_to_string(&p).unwrap_or_default();
        assert_eq!(
            on_disk,
            pretty(&s),
            "{} is stale: run `cargo run -p fn7-tools --bin fn7-schemas`",
            p.display()
        );
    }
}

/// Every real example frame validates against its schema, strictly (no key the docs don't describe).
#[test]
fn examples_validate() {
    for (stem, t, ptr) in TARGETS {
        let ex = example(t);
        assert_eq!(ex["t"], json!(t), "examples/{t}.json is a {t} frame");
        let part = ex
            .pointer(ptr)
            .unwrap_or_else(|| panic!("examples/{t}.json has no {ptr}"));
        let errs = validate(&schema(stem), part, true);
        assert!(
            errs.is_empty(),
            "examples/{t}.json {ptr} vs {stem}:\n{}",
            errs.join("\n")
        );
    }
}

/// Every example file is a well-formed envelope: `t` names the file and `d` is an object.
#[test]
fn examples_are_envelopes() {
    let dir = docs().join("examples");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).expect("docs/examples") {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let v = load(p.clone());
        let t = p.file_stem().unwrap().to_str().unwrap();
        assert_eq!(v["t"], json!(t), "{}", p.display());
        assert!(v["d"].is_object(), "{}: d must be an object", p.display());
        n += 1;
    }
    assert!(n >= TARGETS.len());
}

/// The validator catches what it claims to: a missing required key, a wrong type, a bad ref, an out-of-range setting,
/// an unknown zoom level, an undocumented key, a wrong `oneOf` branch.
#[test]
fn validator_rejects_bad_payloads() {
    let rp = schema("round_private");
    let good = example("round.private")["d"]["private"].clone();
    assert!(validate(&rp, &good, true).is_empty());

    let mut v = good.clone();
    v.as_object_mut().unwrap().remove("you");
    assert!(validate(&rp, &v, true).iter().any(|e| e.contains("`you`")));

    let mut v = good.clone();
    v["you"]["health"] = json!("full");
    assert!(!validate(&rp, &v, true).is_empty());

    let mut v = good.clone();
    v["focus"] = json!("ch:12"); // a bare ref: focus is an Entity object everywhere
    assert!(!validate(&rp, &v, true).is_empty());
    v["focus"] = json!({ "ref": "xx:1", "name": "Ada", "kind": "character" });
    assert!(!validate(&rp, &v, true).is_empty());
    v["focus"] = json!({ "ref": "ch:12", "name": "Ada", "kind": "character", "alive": true });
    assert!(validate(&rp, &v, true).is_empty(), "{:?}", validate(&rp, &v, true));
    v["focus"] = Value::Null;
    assert!(validate(&rp, &v, true).is_empty());

    let mut v = good.clone();
    v["you"]["favor"] = json!([[0, 1.0, 2.0]]);
    assert!(!validate(&rp, &v, true).is_empty());

    let mut v = good;
    v["surprise"] = json!(1);
    assert!(validate(&rp, &v, true)
        .iter()
        .any(|e| e.contains("undocumented")));
    assert!(validate(&rp, &v, false).is_empty());

    let st = schema("settings");
    assert!(validate(
        &st,
        &json!({"players": 4, "beat_ms": 3000, "llm": false}),
        true
    )
    .is_empty());
    assert!(!validate(&st, &json!({"players": 9}), true).is_empty());
    assert!(!validate(&st, &json!({"beat_ms": 100}), true).is_empty());
    assert!(!validate(&st, &json!({"compose_seconds": 0}), true).is_empty());
    assert!(!validate(&st, &json!({"llm": "yes"}), true).is_empty());

    let af = schema("action_fields");
    for ok in [
        json!({"left": "go to the docks", "right": "slow time"}),
        json!({"left": "x", "zoom": "world"}),
        json!({"zoom": {"level": "character", "focus": "ch:3"}}),
        json!({"left": null, "right": "r", "zoom": null}),
        json!({"zoom": ""}),
    ] {
        assert!(validate(&af, &ok, true).is_empty(), "{ok}");
    }
    for bad in [
        json!({"left": 3}),
        json!({"zoom": "galaxy"}),
        json!({"zoom": {"focus": "ch:3"}}),
        json!({"left": "a".repeat(2001)}),
    ] {
        assert!(!validate(&af, &bad, true).is_empty(), "{bad}");
    }

    let pv = schema("public_view");
    let mut v = example("round.committed")["d"]["public_state"].clone();
    let i = v["demigods"].as_array().unwrap().iter().position(|d| d["alive"] == json!(true)).unwrap();
    v["demigods"][i]["alive"] = json!(false); // alive=false without eliminated_beat/cause
    assert!(validate(&pv, &v, true).iter().any(|e| e.contains("oneOf")));
}

/// The narrator runs one beat late: `previous_caption` / `previous_callouts` / `caption_beat` are required (null
/// `caption_beat` on the first beat); omens are `{god, god_ref, speech}` objects.
#[test]
fn caption_and_omen_fields_are_first_class() {
    let o = example("round.outcome")["d"]["public"].clone();
    let s = schema("outcome_public");
    assert!(validate(&s, &o, true).is_empty());
    let mut x = o.clone();
    x["caption_beat"] = Value::Null;
    assert!(validate(&s, &x, true).is_empty());
    for k in ["previous_caption", "previous_callouts", "caption_beat"] {
        let mut x = o.clone();
        x.as_object_mut().unwrap().remove(k);
        assert!(!validate(&s, &x, true).is_empty(), "{k} is required");
    }
    let mut x = o.clone();
    x["omens"] = json!(["Time: tick."]);
    assert!(!validate(&s, &x, true).is_empty());
    x["omens"] = json!([{ "god": "Time", "god_ref": "gd:0", "speech": "\"Tick.\"", "at": [2.35, 48.86], "region": "Western Europe" }]);
    assert!(validate(&s, &x, true).is_empty());
    let mut sum = example("match.ended")["d"]["summary"].clone();
    sum["last_caption"] = json!({ "beat": 5, "caption": "Beat 5: the end." });
    assert!(validate(&schema("match_summary"), &sum, true).is_empty());
}

/// Real `game.*` replies (public and seat-scoped, on a played planet state) validate strictly against their
/// `query_<name>` schemas, and each schema carries the plugin's args schema.
#[test]
fn query_replies_validate() {
    use fn7_plugin::{testkit, FinalNight7Plugin};
    use theatre_plugin_api::GamePlugin;
    let state = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(testkit::played_state(9, 16, 100));
    let w = fn7_plugin::world::load(&state).unwrap();
    let p = FinalNight7Plugin::new();
    let region = w.places.iter().find(|x| x.kind == PlaceKind::District).unwrap().id;
    let city = w.places.iter().find(|x| x.kind == PlaceKind::Site).unwrap().id;
    let crowd = w.clusters.iter().find(|c| c.army.is_none() && c.size > 0).unwrap().id;
    let person = w.characters.iter().find(|c| c.life.is_some()).unwrap().id;
    let army = w.clusters.iter().find(|c| c.army.is_some()).map(|c| c.id).expect("an army");
    let owner = w.clusters[army.ix()].army.as_ref().unwrap().demigod.0 as u8;
    let mut over = state.clone();
    over["meta"]["beats_total"] = over["meta"]["beat"].clone();
    let s = |r: Ref| r.to_string();
    let mut cases: Vec<(&str, Option<u8>, Value, &Value)> = vec![
        ("globe", None, json!({}), &state),
        ("globe", None, json!({ "since": 0, "limit": 200 }), &state),
        ("region", None, json!({ "ref": s(Ref::Place(region)) }), &state),
        ("node", None, json!({ "ref": s(Ref::Place(w.meta.arena)) }), &state),
        ("node", None, json!({ "ref": s(Ref::Place(city)) }), &state),
        ("node", None, json!({ "ref": s(Ref::Cluster(crowd)) }), &state),
        ("person", None, json!({ "ref": s(Ref::Char(person)) }), &state),
        ("person", Some(0), json!({ "ref": s(Ref::Char(person)) }), &state),
        ("army", None, json!({ "ref": s(Ref::Cluster(army)) }), &state),
        ("army", Some(owner), json!({ "ref": s(Ref::Cluster(army)) }), &state),
        ("events", None, json!({ "since": 0, "limit": 200 }), &state),
        ("me", Some(0), json!({}), &state),
        ("me", Some(2), json!({}), &state),
        ("powers", None, json!({}), &state),
        ("gods", None, json!({}), &state),
        ("standings", None, json!({}), &state),
        ("present", None, json!({}), &state),
        ("piles_final", None, json!({}), &over),
        ("media", None, json!({ "kind": "portrait", "ref": "dg:0" }), &state),
        ("media", Some(1), json!({ "kind": "place", "ref": s(Ref::Place(region)) }), &state),
        ("media", None, json!({ "kind": "army", "ref": s(Ref::Cluster(army)) }), &state),
    ];
    if let Some(c) = w.cells.first() {
        cases.push(("node", None, json!({ "ref": s(Ref::Cell(c.id)) }), &state));
    }
    cases.push(("items", None, json!({}), &state));
    cases.push(("items", Some(0), json!({ "limit": 100 }), &state));
    let mine: Vec<Ref> = w.items.iter().filter(|i| i.holders.last() == Some(&Ref::Demigod(DemigodId(0)))).map(|i| Ref::Item(i.id)).collect();
    let item_args: Vec<Value> = mine.iter().take(1).map(|r| json!({ "ref": s(*r) })).collect();
    for a in &item_args {
        cases.push(("item", Some(0), a.clone(), &state));
    }
    if let Some(m) = w.mobs.first() {
        cases.push(("node", None, json!({ "ref": s(Ref::Mob(m.id)) }), &state));
    }
    // State schema 13: the Earth simulator's views (the field, a board at two levels, the globe's tiles, a past beat).
    let key = fn7_model::hexgrid::cell_at(w.places[region.ix()].center, fn7_model::hexgrid::HexLevel::Regions).key();
    let since = w.meta.beat.saturating_sub(3);
    for a in [json!({}), json!({ "cell": key }), json!({ "cell": key, "level": "villages", "rings": 4, "coarse": true }), json!({ "cell": key, "since": since }),
              json!({ "tiles": 4 }), json!({ "tiles": 4, "since": since }), json!({ "cell": key, "level": "houses", "rings": 2, "hexes": true }),
              json!({ "base": true, "grid": true, "bbox": "0,0,20,20" })] {
        cases.push(("earth", None, a, &state));
    }
    cases.push(("earth_at", None, json!({ "beat": since }), &state));
    let specs = p.queries();
    for (name, seat, args, st) in cases {
        let schema = schema(&format!("query_{name}"));
        let spec = specs.iter().find(|x| x.name == name).unwrap();
        assert_eq!(schema["x-args"], spec.args_schema, "query_{name} x-args");
        assert_eq!(schema["x-public"], json!(spec.public));
        let v = p.query(st, seat, name, &args).unwrap_or_else(|e| panic!("{name}: {e}"));
        let errs = validate(&schema, &v, true);
        assert!(errs.is_empty(), "game.{name} {args} (seat {seat:?}) vs query_{name}:\n{}", errs.join("\n"));
    }
}
