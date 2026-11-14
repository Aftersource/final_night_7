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
