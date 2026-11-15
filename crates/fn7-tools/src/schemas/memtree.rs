//! The `game.memory_tree` reply (the tree of consequence): hand-written to match `fn7-plugin/src/memtree.rs`; the strict
//! validation test keeps it honest.

use serde_json::{json, Map, Value};

use super::{array, boolean, int, nullable, num, object, r, string};

fn e(values: &[&str], desc: &str) -> Value {
    json!({ "type": "string", "enum": values, "description": desc })
}
fn counts(desc: &str) -> Value {
    json!({ "type": "object", "additionalProperties": { "type": "integer", "minimum": 0 }, "description": desc })
}
fn opt_int(desc: &str) -> Value {
    json!({ "type": ["integer", "null"], "minimum": 0, "description": desc })
}
fn signed(desc: &str) -> Value {
    json!({ "type": ["integer", "null"], "description": desc })
}
fn opt_str(desc: &str) -> Value {
    json!({ "type": ["string", "null"], "description": desc })
}
fn opt_num(desc: &str) -> Value {
    json!({ "type": ["number", "null"], "description": desc })
}
fn opt_geo(desc: &str) -> Value {
    json!({ "anyOf": [r("LonLat"), { "type": "null" }], "description": desc })
}

pub const NODE_KINDS: [&str; 10] = ["world", "region", "settlement", "open", "hex", "district", "crowd", "cell", "person", "demigod"];
pub const PATH_FACTS: [&str; 5] = ["d20", "encounter", "earth", "settled", "person_memory"];
pub const STEPS: [&str; 9] = ["query_key", "stored_roll", "memory_entry", "rollups", "prompts", "chain", "field", "ledger_row", "mind_memory"];
pub const PROMPTS: [&str; 6] = ["dm_memory", "memory_summary", "earth", "recall", "game.person", "fold"];

pub(super) fn defs() -> Map<String, Value> {
    let mut d = Map::new();
    d.insert("TreeFact".into(), object(
        "One fact as a node shows it (newest first): a DM memory entry (`store: dm`) or a mind's memory (`store: minds`, kind `mind.<kind>`).",
        &[("beat", int("")), ("kind", string("`dice.rolled`, `encounter.marked`, `earth.quake`, `memory.rollup`, `place.observed`, `story.scar`, `mind.witnessed` ...")),
          ("text", string("The entry's line (an observation, which has none: what was seen).")), ("public", boolean("false only for a private fact shown to its own seat."))],
        &[("id", int("The entry's id (`dm.memory[id]`) or the memory's id.")), ("place", string("The entry's place key (a hex key, a `pl:` ref or \"world\").")),
          ("query_key", string("The question it answers, when it is one the DM may be asked again.")), ("owner", r("Ref")),
          ("row", r("Ref")), ("confidence", num("A memory's confidence (1 = true memory; rumors less)."))],
    ));
    d.insert("TreeEntry".into(), object(
        "A DM memory entry in full (as `game.memory` gives it, without `data`), or a hidden one as {id, beat, kind, public: false, hidden: true}.",
        &[("id", int("")), ("beat", int("")), ("kind", string("")), ("public", boolean(""))],
        &[("scope", e(&["cell", "settlement", "region", "world"], "")), ("place", string("")), ("up", array(json!({ "type": "string" }), "Its ancestors' keys, nearest first.")),
          ("query_key", opt_str("")), ("text", string("")), ("hidden", boolean("A private entry, counted only."))],
    ));
    d.insert("TreeRollup".into(), object(
        "The node's latest `memory.rollup` (dm::integrate rolls every beat's public facts up the hierarchy), and the exact entries it cites.",
        &[("rollups", int("Roll-ups at this place in the window.")), ("id", int("")), ("beat", int("")), ("query_key", opt_str("`rollup:<place>:<beat>`.")),
          ("text", string("")), ("counts", counts("Facts it counted, by kind.")), ("facts", opt_int("Facts it counted.")),
          ("cites", array(json!({ "type": "integer", "minimum": 0 }), "Entry ids it cites, newest first (at most 2 × per_node).")),
          ("cites_total", int("Entries it cites.")), ("exact", boolean("All of them found in the window (cites_total = facts)."))],
        &[],
    ));
    d.insert("TreeNode".into(), object(
        "One node of the tree of consequence. Draw levels as tiers, size by facts_total, glow by last_beat, edges as roll-ups.",
        &[("id", string("The node: \"world\", a `pl:` ref (region, settlement, city district), `open:pl:N` (a region's open country), a hex key, `cl:`, `ce:`, `ch:` or `dg:`.")),
          ("kind", e(&NODE_KINDS, "")), ("level", json!({ "type": "integer", "minimum": 0, "maximum": 6, "description": "The tier (see `levels`)." })),
          ("name", string("")), ("store", e(&["dm", "minds"], "dm: the DM's place × time memory; minds: the minds' memories (fn7-memory).")),
          ("children_total", int("Children it has (regions; settlements + open country; hexes the memory knows + a city's districts; crowds; cells; people; who stands on a people hex).")),
          ("facts_total", int("Facts at it or rolled up into it (public and private).")), ("facts_here", int("Facts at the node itself.")),
          ("private_facts", int("Facts that are not public: counted, never shown (except to their own seat).")),
          ("density", object("How dense the record is.",
              &[("per_beat", num("facts_total per beat of the window.")), ("per_million_km2", opt_num("facts_total per million km² (null: no area).")),
                ("log10_per_km2", opt_num("log10 of facts per km² (the planet ~ -5.5, a 3 m hex ~ +6): one scale for every tier.")),
                ("area_m2", opt_num("The node's area in m² (null: no area)."))], &[])),
          ("first_beat", opt_int("")), ("last_beat", opt_int("The newest fact's beat (glow = recency).")),
          ("kinds", counts("Facts by kind.")), ("families", counts("Facts by family (the kind before its dot: earth, encounter, settlement, dice, mind ...).")),
          ("sample_facts", array(r("TreeFact"), "The newest facts shown here or below (at most per_node; only on the path).")),
          ("rolled_up_from", array(object("A child whose facts rolled up here.", &[("from", string("The child's id.")), ("name", string("")), ("facts", int(""))], &[]), "Busiest first (only on the path).")),
          ("rollup", nullable(r("TreeRollup"))),
          ("on_path", boolean("On the path to the focus (expanded).")), ("collapsed", boolean("A sibling shown by its counts only.")),
          ("children", array(r("TreeNode"), "The path child and up to per_node busiest siblings, busiest first."))],
        &[("key", string("Its memory place key.")), ("center", r("LonLat")),
          ("hex_level", e(&["villages", "houses", "people"], "3 km, 30 m, 3 m.")), ("observed_beat", opt_int("A hex: when the DM last recorded it (`place.observed`), null: never.")),
          ("earth_line", opt_str("The earth line the prompts acting here carry now (null: calm, or a past beat).")),
          ("settlement_level", int("A settlement's level now.")), ("size", int("A crowd's or cell's people.")), ("cell_kind", string("gang, squad, congregation, crew, militia.")),
          ("role", nullable(r("Role"))), ("title", opt_str("")), ("alive", boolean("")),
          ("story", object("A demigod's story piece now.", &[("followers", int("")), ("wardrobe", int("")), ("scars", int(""))], &[]))],
    ));
    let prompt = object(
        "One prompt or reader and whether it carries the fact now.",
        &[("prompt", e(&PROMPTS, "")), ("reader", string("Who reads it.")), ("included", boolean(""))],
        &[("place", string("")), ("lines", array(json!({ "type": "string" }), "dm_memory: the exact lines (dm::prompt_lines), oldest first.")),
          ("line", opt_str("The fact's own line in it.")), ("why", string("Why it is not there.")), ("rollup_id", opt_int("")),
          ("at", r("LonLat")), ("rank", opt_int("recall: its rank among its owner's memories.")), ("of", int("")),
          ("window_complete", boolean("recall: the ranking saw the whole store."))],
    );
    let rollup_item = object(
        "A roll-up that cites it (DM), or a society node it counts in (minds).",
        &[("name", string(""))],
        &[("id", json!({ "type": ["integer", "string"] })), ("place", string("")), ("beat", int("")), ("query_key", string("")),
          ("text", string("")), ("counted_as", string("")), ("kind_count", opt_int("")), ("facts", opt_int("")), ("cites_this", boolean("")), ("how", string(""))],
    );
    d.insert("RecallStep".into(), object(
        "One step of a recall path. `step` says which; the fields are those of that step.",
        &[("step", e(&STEPS, ""))],
        &[("key", opt_str("query_key: the question's key.")), ("keys", array(json!({ "type": ["string", "null"] }), "query_key (encounter): every key of its chain.")),
          ("key_hash", string("query_key (d20): hash_str(key), the second seed of the die.")), ("derivation", string("")),
          ("reproduces", boolean("query_key (d20): the seeded die gives the stored face again.")), ("asked_again", string("")),
          ("roll", opt_int("")), ("modifier", signed("")), ("total", signed("")), ("target_dc", signed("")), ("verdict", opt_str("")), ("reason", opt_str("")),
          ("who", opt_str("")), ("who_name", opt_str("")),
          ("entry", r("TreeEntry")), ("entries", array(r("TreeEntry"), "chain: the entries under one encounter, oldest first.")),
          ("items", array(json!({ "anyOf": [prompt, rollup_item] }), "rollups: the roll-ups citing it; prompts: each prompt.")), ("why", string("")),
          ("cell", opt_str("")), ("at", opt_geo("")), ("phenomenon", opt_int("")), ("cells", opt_int("")), ("source", string("")),
          ("row", r("Ref")), ("beat", int("")), ("verb", string("")), ("text", string("")), ("public", boolean("")),
          ("memory", int("")), ("owner", r("Ref")), ("owner_name", string("")), ("kind", string("")), ("salience", int("")), ("confidence", num("")),
          ("hedge", string("")), ("about", array(r("Ref"), ""))],
    ));
    d.insert("RecallPath".into(), object(
        "How one granular fact is recalled: its key, its stored answer, its entry, the roll-ups citing it, the prompts carrying it now.",
        &[("fact", e(&PATH_FACTS, "")), ("title", string("")), ("highlight", array(json!({ "type": "string" }), "Tree node ids to light up.")),
          ("steps", array(r("RecallStep"), ""))],
        &[],
    ));
    d
}

pub(super) fn replies() -> Vec<(&'static str, &'static str, Value)> {
    let window = |n: &str| object("", &[("from_beat", opt_int("")), ("to_beat", int("")), (n, int("")), ("complete", boolean("Everything was read."))], &[]);
    vec![("query_memory_tree", "memory_tree", object(
        "`game.memory_tree`: the tree of consequence, the DM's place × time memory as the exact tree it is (docs/CLIENT_API.md \"Tree of consequence\"). Read-only, bounded, deterministic.",
        &[("beat", int("Now.")),
          ("as_of", object("What `beat` shows: the memory exactly as it stood; the society and the earth now (or omitted) for a past beat.",
              &[("beat", int("")), ("memory", e(&["exact"], "")), ("society", e(&["exact", "now"], "")), ("earth", e(&["exact", "omitted"], ""))], &[])),
          ("depth", int("")), ("per_node", int("")),
          ("visibility", object("", &[("seat", opt_int("The asking seat (null: public).")), ("rule", string(""))], &[])),
          ("bounds", object("What one call read.",
              &[("scan_cap", int("At most this many newest entries of each store.")), ("dm_window", window("entries")), ("minds_window", window("items")),
                ("ledger_rows_read", int("Ledger rows looked up by index."))], &[])),
          ("focus", object("The focus as asked (or the default: the busiest region now).",
              &[("arg", opt_str("")), ("default", boolean("")), ("kind", e(&["region", "hex", "place", "demigod", "person"], "")), ("id", opt_str("")), ("name", opt_str(""))],
              &[("why", string(""))])),
          ("levels", array(object("One tier, for the legend.", &[("level", int("")), ("kinds", array(json!({ "type": "string" }), "")), ("name", string("")),
              ("scale", string("")), ("store", string(""))], &[]), "")),
          ("tree", r("TreeNode")),
          ("recall_paths", array(r("RecallPath"), "3-5 examples near the focus: a d20, an encounter, an earth event, a settled fact in the DM's prompt, a person's memory.")),
          ("totals", object("The whole state's record.",
              &[("dm_memory", object("The DM's memory (`dm.memory`).",
                    &[("entries", int("All entries.")), ("read", int("Entries read (the window).")), ("public", int("")), ("private", int("")),
                      ("by_scope", counts("")), ("by_kind", counts("")), ("families", counts("")), ("first_beat", opt_int("")), ("last_beat", opt_int(""))], &[])),
                ("minds", object("The minds' memories (`memory.items`).",
                    &[("items", int("All items.")), ("read", int("Items read (the window).")), ("live", int("Neither folded nor forgotten, with text.")),
                      ("by_owner", counts("Read items by owner prefix (ch, gr, dg, cl, gd, ag)."))], &[])),
                ("ledger_rows", int("")), ("persons_known", int("")), ("society_cells", int("")), ("crowds", int("")),
                ("hexes_observed", int("Distinct hexes with a `place.observed` entry in the window.")), ("observations", int("")),
                ("places", object("", &[("regions", int("")), ("settlements", int("")), ("districts", int(""))], &[])),
                ("state_bytes", object("The state's JSON size by table, estimated cheaply (lists sampled).",
                    &[("total", int("")), ("tables", array(object("", &[("table", string("")), ("bytes", int("")), ("rows", opt_int(""))], &[]), "Biggest first."))],
                    &[("estimated", boolean(""))]))],
              &[]))],
        &[],
    ))]
}
