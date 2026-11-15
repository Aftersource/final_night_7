//! State schema 13 (the Earth simulator) payload definitions: the per-beat `earth` object (delta + phenomena), the
//! field summary in `present` and the PublicView, `game.earth` / `game.earth_at` replies and the room settings.
//! Hand-written to match `fn7-plugin/src/earth/view.rs`; the strict validation tests keep them honest.

use serde_json::{json, Map, Value};

use super::{array, boolean, int, nullable, num, object, r, string};

fn e(vals: &[&str], desc: &str) -> Value {
    json!({ "type": "string", "enum": vals, "description": desc })
}

const KINDS: [&str; 14] = ["storm", "lightning", "flood", "wildfire", "volcano", "earthquake", "meteor", "raise_land", "sink_land",
                           "forest", "swarm", "blizzard", "tidal_wave", "plate_drift"];
const TOPS: [&str; 13] = ["water", "rock", "soil", "sand", "ice", "forest", "lava", "fire", "ash", "snow", "flood", "rift", "swarm"];

fn b64_or_arr(desc: &str) -> Value {
    json!({ "type": ["string", "array"], "description": desc })
}

const WHATS: [&str; 10] = ["burned", "flooded", "destroyed", "damaged", "fled", "died", "sheltered", "buried", "swept", "returned"];
const THINGS: [&str; 9] = ["building", "forest", "crowd", "road", "shrine", "demigod", "person", "army", "settlement"];

/// Schema 20's optional fields on a beat's `earth` (and the summary, `game.earth_at`): the effects, thing by thing.
fn fx_fields() -> Vec<(&'static str, Value)> {
    vec![("effects", array(r("EarthEffect"), "State schema 20: what the earth did this beat, thing by thing, worst first, at most `earth_effects_max` (docs/EARTH_SIM.md §13).")),
         ("effects_total", int("State schema 20: effects before the cap.")),
         ("effects_counts", json!({ "type": "object", "additionalProperties": { "type": "integer" }, "description": "State schema 20: effects per `what`, before the cap." }))]
}

pub fn defs() -> Map<String, Value> {
    let mut d = Map::new();
    let levels = ["regions", "villages", "houses", "people"];
    d.insert("EarthEffect".into(), object(
        "State schema 20: one thing the earth did (docs/EARTH_SIM.md §13). `cell` is at the thing's level: houses for buildings, people for demigods, persons and shrines, villages for settlements and crowds, regions for forests, roads and armies.",
        &[("cell", string("Hex key `face:level:q:r`.")), ("level", e(&levels, "")),
          ("thing", object("", &[("kind", e(&THINGS, ""))], &[("ref", string("cl:N (crowd, army), dg:N, ch:N (person), pl:N (settlement; a building's settlement), shrine:ID, pl:A~pl:B (road)."))])),
          ("what", e(&WHATS, "")), ("cause", int("The phenomenon's id (0: none named)."))],
        &[("to_cell", string("Where they went (fled: the shelter; returned: home).")), ("count", int("People.")),
          ("amount", num("Damage (demigod, person); share of vegetation gone as the front crossed (forest); ruin share (building); levels lost / 3 (settlement).")),
          ("phenomenon", e(&KINDS, "The cause's kind.")), ("power", int("The cast's magnitude, 1..10.")), ("seat", int("The caster's seat."))],
    ));
    let list = |item: Value, desc: &str| array(item, desc);
    d.insert("EarthScars".into(), object(
        "State schema 20: what the earth left, per cell, at a beat (docs/EARTH_SIM.md §13): draw lasting damage at any zoom.",
        &[("beat", int("")),
          ("burnt", list(object("", &[("cell", string("")), ("fuel", int("Vegetation now.")), ("base", int("Vegetation at rest."))], &[]), "Land burned off, regrowing.")),
          ("mud", list(object("", &[("cell", string("")), ("water", int("1..23 left."))], &[]), "A flood drying.")),
          ("ruins", list(object("", &[("cell", string("")), ("ruin", int(""))], &[]), "Ruin ≥ 96.")),
          ("wrecks", list(object("", &[("cell", string("Houses-level hex.")), ("settlement", string("")), ("beat", int("")), ("how", e(&["destroyed", "burned", "buried", "swept"], "")),
              ("cause", int(""))], &[]), "Buildings destroyed.")),
          ("sheltering", list(object("", &[("crowd", string("")), ("from", string("Villages-level home.")), ("at", string("Houses-level shelter.")), ("settlement", string("")),
              ("count", int("")), ("since", int("")), ("cause", int(""))], &[]), "People in shelters then.")),
          ("fallen", list(object("", &[("cell", string("")), ("level", e(&levels, "")), ("count", int("")), ("beat", int("The latest beat."))], &[]), "The dead of the last 12 beats, per cell."))],
        &[],
    ));
    d.insert("EarthCells".into(), object(
        "State schema 13: cell columns (docs/EARTH_SIM.md §7). `enc: pack` (the default): `data` is one base64 block, per cell in index order: varint index delta, height byte (0..15), top byte (an index into `tops`), varint layer mask, then one byte per set layer (CLIENT_LAYERS order; `heat` signed). `enc: columns`: `cells` (u16 LE), `h`, `top` and `layers{name}`, one base64 array each. `enc: json`: the same columns as number arrays.",
        &[("n", int("Cells.")), ("enc", e(&["pack", "columns", "json"], ""))],
        &[("data", string("enc pack: the base64 block.")), ("cells", b64_or_arr("Regions-grid cell indices.")), ("h", b64_or_arr("Heights 0..15 (8 = sea level).")),
          ("top", b64_or_arr("Top material indices (into `tops`).")),
          ("layers", json!({ "type": "object", "additionalProperties": { "type": ["string", "array"] }, "description": "A column per non-zero layer." }))],
    ));
    let to = object("Where it will be a beat from now (glide toward it).", &[("dir", int("Hex direction 0..5.")), ("to", r("LonLat")), ("to_cell", string("")),
        ("cells_per_beat", num(""))], &[]);
    d.insert("EarthPhenomenon".into(), object(
        "State schema 13: one live phenomenon of the Earth simulator.",
        &[("id", int("")), ("kind", e(&KINDS, "")), ("at", r("LonLat")), ("cell", string("Regions-level hex key.")), ("cell_index", int("")),
          ("origin", r("LonLat")), ("radius_cells", int("")), ("radius_km", int("")), ("power", json!({ "type": "integer" })), ("intensity", num("power / 16 (a mega of magnitude 8 starts near 8).")),
          ("age", int("Beats since it began.")), ("born", int("")), ("beats_left", int("")), ("owner_seat", nullable(int("The seat that cast it; null = the world's own."))),
          ("cause", e(&["cast", "god", "ramification", "entropy", "interaction"], "")), ("tier", e(&["mini", "mega", "god", "world"], ""))],
        &[("parent", int("The phenomenon that set it off.")), ("velocity", to), ("front_cells", json!({ "type": "integer" })), ("front_km", json!({ "type": "integer" })),
          ("front_speed_cells_per_beat", json!({ "type": "integer" })), ("moves_left", json!({ "type": "integer" })), ("next_move_in", json!({ "type": "integer" })),
          ("state", e(&["erupting", "dormant"], "A volcano's.")), ("ruptured_cells", json!({ "type": "integer" }))],
    ));
    d.insert("EarthBeat".into(), object(
        "State schema 13: `round.outcome.public.earth`: what changed on the field this beat (`delta`, only cells a client must redraw) and every live phenomenon.",
        &[("beat", int("")), ("hash", string("The field's hash (16 hex digits); compare with present.earth.hash.")), ("dirty", int("Cells that differ from base.")),
          ("substeps", int("")), ("delta", r("EarthCells")), ("phenomena", array(r("EarthPhenomenon"), "The 20 biggest (power × reach), each with its stable id.")),
          ("phenomena_total", int("Live phenomena in all.")),
          ("ended", array(object("", &[("id", int("")), ("kind", e(&KINDS, ""))], &[]), "Phenomena that ended this beat.")),
          ("strikes", array(json!({ "type": "string" }), "Lightning strikes this beat (cell keys).")),
          ("did", object("This beat's counts.", &[("spawned", array(json!({ "type": "integer" }), "Ids born this beat.")), ("spontaneous", int("")), ("ignitions", int("")), ("quenched", int("")), ("steamed", int("")),
              ("rock_made", int("")), ("melted", int("")), ("land_rose", array(json!({ "type": "string" }), "")), ("drowned", array(json!({ "type": "string" }), "")),
              ("woke", array(json!({ "type": "integer" }), "Volcano ids a quake woke.")), ("evicted", int("")), ("refused", int(""))], &[]))],
        &fx_fields(),
    ));
    d.insert("EarthSummary".into(), object(
        "State schema 13: the field in short (present.earth and the PublicView's `earth`); fetch the cells with game.earth when `hash` differs from yours.",
        &[("beat", int("")), ("hash", string("")), ("dirty", int("")),
          ("tops", json!({ "type": "object", "additionalProperties": { "type": "integer" }, "description": "Dirty cells by top material." })),
          ("phenomena", array(r("EarthPhenomenon"), "")), ("keyframes", array(json!({ "type": "integer" }), "Beats with a keyframe.")),
          ("first_beat", int("")), ("grid", object("", &[("level", string("")), ("cells", int(""))], &[])),
          ("phenomena_total", int("Live phenomena (`phenomena` has the 20 biggest)."))],
        &fx_fields(),
    ));
    d
}

/// A hex of `game.earth` (regions level: true values; below: derived from the coarse cell).
fn hex() -> Value {
    let layers = ["dh", "water", "lava", "ash", "fire", "snow", "cloud", "charge", "heat", "stress", "crack", "ruin", "dfuel", "swarm", "shake", "plume"];
    let mut opt: Vec<(&str, Value)> = layers.iter().map(|k| (*k, json!({ "type": "integer" }))).collect();
    opt.extend([("index", int("")), ("base_h", int("")), ("fault_hops", int("")), ("coast_hops", int("")), ("fuel", int("")),
                ("coarse", string("The regions cell it is derived from.")), ("coarse_index", int("")), ("forest", boolean(""))]);
    json!({ "anyOf": [object("", &[("cell", string("")), ("level", string("")), ("derived", boolean("")), ("h", int("")), ("h16", int("Height in 256ths.")),
        ("top", e(&TOPS, "")), ("material", e(&TOPS, "")), ("weather", string(""))], &opt), { "type": "null" }] })
}

fn head() -> Vec<(&'static str, Value)> {
    vec![("beat", int("The beat shown.")), ("now", int("The last beat stepped.")), ("hash", string("")), ("dirty", int("")),
         ("phenomena", array(r("EarthPhenomenon"), "")), ("rebuilt", boolean("Re-simulated from a keyframe (a past beat).")),
         ("from_keyframe", nullable(int(""))), ("resimulated_beats", int("")),
         ("tops", array(json!({ "type": "string" }), "Top material names by index."))]
}

fn layers() -> (&'static str, Value) {
    ("layers", array(json!({ "type": "string" }), "CLIENT_LAYERS: layer names by bit (absent in a board or tiles answer)."))
}

fn board() -> Value {
    object("State schema 13: the client's board: 9 bytes per hex (`fields`) in ring order (ring 0, then each ring as the shared hex formula walks it), base64.",
        &[("level", string("")), ("center", string("The centre hex key.")), ("rings", int("")), ("n", int("Records in `data`.")),
          ("fields", array(json!({ "type": "string" }), "h16 (height in 256ths, 128 = sea level), top, water, lava, ash, fire, snow, heat (signed), crack.")),
          ("enc", json!({ "const": "b64" })), ("data", string(""))],
        &[("changed", string("With `since`: the ring positions (u16 LE) whose records are in `data`.")), ("since", int("")),
          ("coarse", string("With `coarse`: the regions cell index under each hex (u16 LE)."))])
}

fn tiles() -> Value {
    object("State schema 13: the globe in tiles: rows of latitude from the north down, west to east in each; 9 bytes per tile (`fields`), base64.",
        &[("deg", int("")), ("layout", e(&["rings", "grid"], "")), ("rows", int("")), ("row_cols", array(json!({ "type": "integer" }), "Tiles in each row.")),
          ("tiles", int("Tiles in all.")), ("n", int("Records in `data`.")), ("fields", array(json!({ "type": "string" }), "")),
          ("enc", json!({ "const": "b64" })), ("data", string(""))],
        &[("changed", string("With `since`: the tile indices (u16 LE) whose records are in `data`.")), ("since", int(""))])
}

pub fn replies() -> Vec<(&'static str, &'static str, Value)> {
    let mut earth = head();
    earth.push(("level", e(&["regions", "villages", "houses", "people"], "")));
    let col = |d: &str| json!({ "type": "string", "description": d });
    vec![
        ("query_earth", "earth", object("`game.earth` (state schema 13): the voxel field for a view (docs/EARTH_SIM.md).", &earth,
            &[layers(), ("center_cell", string("")), ("rings", int("")), ("derived", boolean("")), ("board", board()), ("tiles", tiles()), ("hexes", array(hex(), "Below regions: derived hexes.")),
              ("field", r("EarthCells")),
              ("globe", object("`base: true`: every cell's height, top and fuel now, in index order.", &[("n", int("")), ("enc", string("")),
                  ("h", b64_or_arr("")), ("top", b64_or_arr("")), ("fuel", b64_or_arr(""))], &[])),
              ("grid", object("`grid: true`: the regions grid (face, q, r of every cell, signed bytes, base64).", &[("level", string("")), ("hex_size_m", int("")),
                  ("cells", int("")), ("face", col("")), ("q", col("")), ("r", col(""))], &[])),
              ("scars", r("EarthScars"))].into_iter().chain(fx_fields()).collect::<Vec<_>>())),
        ("query_earth_at", "earth_at", object("`game.earth_at` (state schema 13): the whole field at the end of a past beat, rebuilt from a keyframe.",
            &head().into_iter().chain([layers(), ("field", r("EarthCells")), ("keyframes", array(json!({ "type": "integer" }), "")), ("inputs", int("Inputs up to that beat."))]).collect::<Vec<_>>(),
            &[("scars", r("EarthScars"))].into_iter().chain(fx_fields()).collect::<Vec<_>>())),
    ]
}

/// The room settings of the simulator.
pub fn settings(p: &mut dyn FnMut(&str, Value)) {
    p("earth_sim_enabled", json!({ "type": "boolean", "default": true, "description": "The Earth simulator (state schema 13, docs/EARTH_SIM.md): god powers act on a simulated voxel field of the planet whose fires, floods, quakes and lava reach demigods, crowds and settlements through the rules. Planet arenas only. Absent in settings recorded before it: off." }));
    p("earth_max_cells", json!({ "type": "integer", "minimum": 64, "maximum": 8000, "default": 1500, "description": "State schema 13: Most cells that may differ from the base terrain; past it the calmest settle back to base (remembered as `earth.settled`)." }));
    p("earth_max_phenomena", json!({ "type": "integer", "minimum": 4, "maximum": 200, "default": 48, "description": "State schema 13: Most live phenomena; past it a newcomer replaces the weakest, or is refused." }));
    p("earth_substeps", json!({ "type": "integer", "minimum": 1, "maximum": 4, "default": 1, "description": "State schema 13: Sub-steps of the field per beat at rest (Entropy 60, 80 and 100 each add one)." }));
    p("earth_keyframe_beats", json!({ "type": "integer", "minimum": 1, "maximum": 1000, "default": 16, "description": "State schema 13: A keyframe of the field is kept every this many beats (any beat is rebuilt by re-simulating at most this many)." }));
    p("earth_plate_beats", json!({ "type": "integer", "minimum": 1, "maximum": 50, "default": 3, "description": "State schema 13: A drifting patch of plate moves one cell every this many beats." }));
    p("earth_km_per_cell", json!({ "type": "number", "minimum": 50.0, "maximum": 5000.0, "default": 400.0, "description": "State schema 13: A mega's reach in cells is its radius in km over this (1 to 4 cells); a mini is one cell." }));
    p("earth_spread_scale", json!({ "type": "number", "minimum": 0.0, "maximum": 4.0, "default": 1.0, "description": "State schema 13: Multiplier on how fast fire, water and swarms spread." }));
    p("earth_spontaneous_scale", json!({ "type": "number", "minimum": 0.0, "maximum": 8.0, "default": 1.0, "description": "State schema 13: Multiplier on the odds of the world's own phenomena under Entropy (quakes, volcanoes and drifting plates on stressed faults, freak weather)." }));
    p("earth_energy_scale", json!({ "type": "number", "minimum": 0.1, "maximum": 4.0, "default": 1.0, "description": "State schema 13: Multiplier on every phenomenon's starting power." }));
    p("earth_mini_scale", json!({ "type": "number", "minimum": 0.0, "maximum": 2.0, "default": 0.5, "description": "State schema 13: A mini's starting power relative to a mega of the same magnitude." }));
    p("earth_damage_scale", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 1.0, "description": "State schema 13: Multiplier on the damage the field deals demigods standing in it." }));
    p("earth_damage_cap", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 1.0, "description": "State schema 13: Most damage the field deals one demigod in one beat (before `earth_damage_scale`)." }));
    p("earth_fire_damage", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 0.6, "description": "State schema 13: Damage per beat in a cell burning at full intensity." }));
    p("earth_lava_damage", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 1.2, "description": "State schema 13: Damage per beat in a cell deep in lava." }));
    p("earth_flood_damage", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 0.5, "description": "State schema 13: Damage per beat in a cell flooded to the brim, drowned, or hit by a tidal wave." }));
    p("earth_quake_damage", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 0.5, "description": "State schema 13: Damage in a cell shaking at full strength." }));
    p("earth_storm_damage", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 0.4, "description": "State schema 13: Damage from a lightning strike on the cell." }));
    p("earth_cold_damage", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 0.2, "description": "State schema 13: Damage per beat in a blizzard." }));
    p("earth_swarm_damage", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 0.2, "description": "State schema 13: Damage per beat in a cell thick with a swarm." }));
    p("earth_crowd_scale", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 1.0, "description": "State schema 13: Multiplier on the fear, awe and deaths the field brings crowds." }));
    p("earth_crowd_effects", json!({ "type": "integer", "minimum": 0, "maximum": 100, "default": 6, "description": "State schema 13: Most crowds the field moves in one beat (the worst hit first)." }));
    p("earth_ruin_scale", json!({ "type": "number", "minimum": 0.0, "maximum": 10.0, "default": 1.0, "description": "State schema 13: Multiplier on how much a cell's ruin lowers its settlements' level target." }));
    p("earth_events_per_beat", json!({ "type": "integer", "minimum": 0, "maximum": 50, "default": 6, "description": "State schema 13: Most Earth entries written to the DM's memory in one beat." }));
    p("earth_effects_max", json!({ "type": "integer", "minimum": 0, "maximum": 1000, "default": 200, "description": "State schema 20: Most effects (`earth.effects[]`: what the earth did, thing by thing) a beat names and keeps, the worst first." }));
}
