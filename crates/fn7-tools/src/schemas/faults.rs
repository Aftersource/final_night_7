//! State schema 16 (fault junctions and tiers) payload definitions: the `junction` and `fault_tier` fields on a mega's
//! streak and cast line, the `game.fault` / `game.faults_overview` replies and the room settings. Hand-written to match
//! `fn7-plugin/src/faults.rs`; the strict validation tests keep them honest.

use serde_json::{json, Map, Value};

use super::{array, boolean, num, object, r, string};

pub const TIERS: [&str; 5] = ["junction", "fault", "near", "mild", "tiny"];

fn kind() -> Value {
    json!({ "type": "string", "enum": ["triple", "knot"], "description": "triple: boundaries of 3+ plates meet; knot: a dense, twisting stretch of boundary far from any triple junction." })
}
fn plates() -> Value {
    array(json!({ "type": "string" }), "PB2002 plate codes meeting there (sorted); empty for a knot.")
}
fn strength() -> Value {
    json!({ "type": "number", "minimum": 0, "maximum": 1, "description": "0..1: the strongest triple junction is 1 (by the size of the plates meeting and the boundary around it)." })
}

pub(super) fn defs() -> Map<String, Value> {
    let mut d = Map::new();
    d.insert("FaultTier".into(), json!({ "type": "string", "enum": TIERS,
        "description": "State schema 16: a mega's tier: junction (within fault_junction_km of a junction), fault (≤ fault_near_km from a fault line), near (the first half of the curve), mild (the second half), tiny (≥ fault_far_km: a tiny volcano; its lasting mods stay local)." }));
    d.insert("FaultJunction".into(), object(
        "State schema 16: the plate-boundary junction pulling on a mega (the strongest pull within fault_junction_km).",
        &[("kind", kind()), ("at", r("LonLat")), ("strength", strength()), ("plates", plates()), ("km", num("Great-circle distance (km) from the aim to it."))],
        &[("name", string("Its name when it is a famous one (\"Afar triple junction\"): narration says \"at the Afar triple junction\"."))],
    ));
    d
}

/// Optional fields of a mega's streak cast, streak and cast line.
pub(super) fn cast_fields() -> Vec<(&'static str, Value)> {
    vec![
        ("fault_tier", r("FaultTier")),
        ("junction", json!({ "anyOf": [r("FaultJunction"), { "type": "null" }], "description": "State schema 16, megas only: the junction it falls near (its multipliers are in fault_factor, radius_km and the magnitude), or null." })),
    ]
}

pub(super) fn settings(p: &mut impl FnMut(&str, Value)) {
    let f = |min: f64, max: f64, def: f64, desc: &str| json!({ "type": "number", "minimum": min, "maximum": max, "default": def, "description": desc });
    p("fault_junctions_enabled", json!({ "type": "boolean", "default": true, "description": "State schema 16, fault junctions and tiers (the feature gate): a mega near a plate-boundary junction gets the junction multipliers on top of the distance curve, every mega lands in a tier (junction .. tiny) that scales the world's answer, and a tiny one's lasting mods stay local. Settings recorded before it lack it and read back false (old recordings replay unchanged)." }));
    p("fault_junction_km", f(0.0, 3000.0, 300.0, "State schema 16: a mega this close (km) to a junction gets its multipliers, scaled by the junction's strength and falling off linearly to nothing at this distance."));
    p("fault_junction_magnitude", f(-10.0, 10.0, 3.0, "State schema 16: magnitude added at a junction of strength 1 (× strength × falloff)."));
    p("fault_junction_radius", f(0.1, 10.0, 1.5, "State schema 16: radius multiplier at a junction of strength 1 (1 + (this − 1) × strength × falloff)."));
    p("fault_junction_damage", f(0.0, 10.0, 1.8, "State schema 16: damage multiplier at a junction of strength 1 (1 + (this − 1) × strength × falloff)."));
    p("fault_tiny_mod_km", f(20.0, 1500.0, 150.0, "State schema 16: a tiny mega's lasting world mods reach at most this far (km): local, never regional."));
}

pub(super) fn replies() -> Vec<(&'static str, &'static str, Value)> {
    let factor = object(
        "The multipliers a mega gets there under this match's rules (the distance curve, times the junction's).",
        &[("magnitude", num("Added to the mega's magnitude (rounded when it lands, 1..10).")), ("radius", num("Radius multiplier.")),
          ("damage", num("Damage multiplier (the cast's fault_factor)."))],
        &[],
    );
    let threshold = object(
        "One tier and where it ends.",
        &[("tier", r("FaultTier"))],
        &[("max_km", num("It holds up to this distance (km) from the nearest fault line.")), ("min_km", num("tiny: from this distance on.")),
          ("within_km_of_junction", num("junction: within this distance of a junction."))],
    );
    let curve = |desc: &str| object(desc, &[("km", num("")), ("magnitude", num("")), ("radius", num("")), ("damage", num(""))], &[]);
    let junction = object(
        "One junction.",
        &[("lon", num("")), ("lat", num("")), ("kind", kind()), ("strength", strength()), ("plates", plates())],
        &[("name", string("A famous one's name."))],
    );
    vec![
        ("query_fault", "fault", object(
            "`game.fault` (state schema 16): what a mega aimed here would get (the hover preview while a mega is armed). Read-only and deterministic.",
            &[("at", r("LonLat")), ("km", num("Great-circle distance (km) to the nearest fault line.")), ("tier", r("FaultTier")),
              ("junction", json!({ "anyOf": [r("FaultJunction"), { "type": "null" }], "description": "The junction pulling on it, or null (also null before state schema 16 or with fault_junctions_enabled off)." })),
              ("factor", factor), ("junctions", boolean("The match's fault rule includes junctions and tiers (state schema 16 and fault_junctions_enabled)."))],
            &[],
        )),
        ("query_faults_overview", "faults_overview", object(
            "`game.faults_overview` (state schema 16): the globe overlay while a mega is armed: every junction (glow them by strength) and the tier thresholds. The same all match: ask once.",
            &[("junctions_rule", boolean("The match's fault rule includes junctions and tiers.")), ("junction_km", num("fault_junction_km.")),
              ("tiers", array(threshold, "junction, fault, near, mild, tiny, in that order.")),
              ("factors", object("The curve's settings.", &[("near", curve("At or inside fault_near_km.")), ("far", curve("At or beyond fault_far_km.")),
                  ("junction", curve("At a junction of strength 1 (km = fault_junction_km)."))], &[])),
              ("junctions", array(junction, "Strongest first.")), ("credit", string("The plate data's required credit (ODC-BY 1.0)."))],
            &[],
        )),
    ]
}
