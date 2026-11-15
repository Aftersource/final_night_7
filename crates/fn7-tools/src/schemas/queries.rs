//! Reply schemas for the `game.*` queries (`fn7_plugin::queries`), one `query_<name>.schema.json` each. The args
//! schema the plugin publishes in `server.hello.api.queries` is copied in as `x-args` (with `x-public`), so the file
//! is the whole contract of one query. Written by hand to match `crates/fn7-plugin/src/queries.rs` and the society
//! builders it reuses (`society.rs`: node, crowd, cell, mob, who); a test validates real replies strictly.

use serde_json::{json, Map, Value};

use super::*;

fn e(values: &[&str], desc: &str) -> Value {
    json!({ "type": "string", "enum": values, "description": desc })
}
fn opt_ref(desc: &str) -> Value {
    json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": desc })
}
fn opt_geo(desc: &str) -> Value {
    json!({ "anyOf": [{ "$ref": "#/$defs/LonLat" }, { "type": "null" }], "description": desc })
}
fn opt_int(desc: &str) -> Value {
    json!({ "type": ["integer", "null"], "minimum": 0, "description": desc })
}
fn cursor() -> Value {
    opt_int("Pass as `cursor` for the next page; null on the last page.")
}
/// `{items, next_cursor, total}`.
fn page(item: Value, desc: &str) -> Value {
    object(
        desc,
        &[
            ("items", array(item, "This page.")),
            ("next_cursor", cursor()),
            ("total", int("Entries in the whole list.")),
        ],
        &[],
    )
}

const LEVELS: &[&str] = &["planet", "region", "city", "district", "crowd", "cell", "person"];
const STANCES: &[&str] = &[
    "calm", "restless", "rioting", "fleeing", "worshipping", "fighting", "marching", "holding", "besieging", "raiding",
    "retreating", "rogue",
];
const ROLES: &[&str] = &[
    "commoner", "notable", "ringleader", "general", "captain", "soldier", "hero", "coward", "witness", "prophet",
    "paladin", "deserter", "avenger",
];

/// Query-only definitions (added to the pool).
pub(super) fn defs() -> Map<String, Value> {
    let mut d = Map::new();
    d.insert("Mood".into(), object("Mood, each 0..1.", &[("anger", num("")), ("fear", num("")), ("hope", num("")), ("devotion", num(""))], &[]));
    d.insert("Need".into(), object("Unmet needs, each 0 (met) .. 1 (desperate).", &[("food", num("")), ("safety", num("")), ("faith", num(""))], &[]));
    d.insert("Stance".into(), e(STANCES, "What it is doing as a whole."));
    d.insert("Level".into(), e(LEVELS, "A level of the hierarchy."));
    d.insert("Role".into(), e(ROLES, "A person's part in the society."));
    d.insert(
        "Rank".into(),
        object(
            "Where a person stands.",
            &[("level", r("Level")), ("title", string(""))],
            &[("of", json!({ "$ref": "#/$defs/Ref", "description": "The node or unit they hold rank in." }))],
        ),
    );
    d.insert(
        "Allegiance".into(),
        object("Loyalty to one demigod, -1..1.", &[("demigod", string("")), ("demigod_ref", r("Ref")), ("value", num(""))], &[]),
    );
    d.insert(
        "Who".into(),
        object(
            "A person in short: who leads, commands or belongs somewhere.",
            &[
                ("ref", r("Ref")),
                ("name", string("")),
                ("alive", boolean("")),
                ("role", nullable(r("Role"))),
                ("title", json!({ "type": ["string", "null"], "description": "\"General of Ash's Host of Cairo\"." })),
            ],
            &[("pos", r("LonLat")), ("cell", string("Rules 0.7.0: their people-level hex cell (`<face>:people:<q>:<r>`)."))],
        ),
    );
    d.insert(
        "Node".into(),
        object(
            "The rolled-up state of a place (planet, region, city, city district).",
            &[
                ("ref", r("Ref")),
                ("name", string("")),
                ("level", r("Level")),
                ("population", int("People living here now (crowds, not armies passing through).")),
                ("base", int("Population at setup.")),
                ("mood", r("Mood")),
                ("need", r("Need")),
                ("allegiance", array(r("Allegiance"), "Size-weighted allegiance per demigod (sparse).")),
                ("worship", array(object("", &[("god", string("")), ("god_ref", r("Ref")), ("value", num("0..1"))], &[]), "Worship share per god (sparse).")),
                ("supply", num("Food and goods stock: 1 = normal.")),
                ("stance", r("Stance")),
                ("leader", nullable(r("Who"))),
                ("dead", int("Deaths here so far.")),
                ("at", r("LonLat")),
                ("parent", opt_ref("The place above (null for the planet).")),
            ],
            &[
                ("settlement", r("Settlement")),
                ("observed_beat", int("Rules 0.8.0: simulated every beat: the beat now.")),
                ("extrapolated", boolean("Rules 0.8.0: false (a simulated place).")),
            ],
        ),
    );
    d.insert(
        "Cell".into(),
        object(
            "A cell: 10-500 people out of a crowd (a gang, squad, congregation, crew or militia).",
            &[
                ("ref", r("Ref")),
                ("name", string("")),
                ("kind", e(&["gang", "squad", "congregation", "crew", "militia"], "")),
                ("size", int("")),
                ("crowd", r("Ref")),
                ("leader", nullable(r("Who"))),
                ("stance", r("Stance")),
                ("mob", opt_ref("The mob it merged into.")),
                ("at", opt_geo("")),
            ],
            &[],
        ),
    );
    d.insert(
        "Crowd".into(),
        object(
            "A crowd (a cluster of unaddressed people) and its live cells.",
            &[
                ("ref", r("Ref")),
                ("name", string("")),
                ("size", int("Head count.")),
                ("place", r("Ref")),
                ("at", opt_geo("")),
                ("mood", r("Mood")),
                ("need", r("Need")),
                ("stance", r("Stance")),
                ("allegiance", array(r("Allegiance"), "Per demigod (sparse).")),
                ("leader", nullable(r("Who"))),
                ("dead", int("")),
                ("cells", array(r("Cell"), "Live cells.")),
            ],
            &[
                ("pos", json!({ "$ref": "#/$defs/LonLat", "description": "Rules 0.7.0: where its piece stands (a fixed seeded spot around its place, so a city's crowds don't stack)." })),
                ("cell", string("Rules 0.7.0: `pos`'s people-level hex cell.")),
                ("village", string("Rules 0.7.0: `pos`'s village-level hex cell.")),
            ],
        ),
    );
    d.insert(
        "Mob".into(),
        object(
            "A mob: cells merged under a ringleader.",
            &[
                ("ref", r("Ref")),
                ("name", string("")),
                ("size", int("")),
                ("action", e(&["gathering", "rioting", "looting", "attacking", "marching", "dispersed"], "")),
                ("target", opt_ref("What it is angry at.")),
                ("ringleader", nullable(r("Who"))),
                ("cause", string("Why it formed.")),
                ("at", r("LonLat")),
                ("city", r("Ref")),
                ("place", r("Ref")),
                ("region", string("")),
                ("side", opt_ref("The demigod it rallies behind.")),
                ("anger", num("")),
                ("fear", num("")),
                ("cells", array(r("Ref"), "Its cells (`ce:` refs).")),
                ("formed_beat", int("")),
                ("ended_beat", opt_int("")),
                ("casualties", int("")),
                ("kills", int("")),
            ],
            &[
                ("pos", json!({ "$ref": "#/$defs/LonLat", "description": "Rules 0.7.0: = at." })),
                ("cell", string("Rules 0.7.0: its people-level hex cell.")),
                ("ringleader_ref", opt_ref("Rules 0.7.0: the ringleader (`ch:`).")),
            ],
        ),
    );
    d.insert(
        "ArmyMarker".into(),
        object(
            "An army as everyone sees it (no orders, no chain of command).",
            &[
                ("ref", r("Ref")),
                ("name", string("")),
                ("demigod", json!({ "type": ["string", "null"], "description": "The demigod it marches for." })),
                ("demigod_ref", r("Ref")),
                ("rogue", boolean("Its general mutinied: it answers to no demigod.")),
                ("size", int("Head count.")),
                ("pos", r("LonLat")),
                ("region", string("Where it is.")),
                ("dest", opt_geo("Where it is headed.")),
                ("target", opt_ref("What it marches on.")),
                ("stance", r("Stance")),
                ("morale", num("0..1.")),
            ],
            &[],
        ),
    );
    d.insert(
        "QueryEvent".into(),
        json!({
            "type": "object",
            "description": "A public geotagged event (a world event plus its beat and region ref).",
            "allOf": [
                { "$ref": "#/$defs/WorldEvent" },
                { "type": "object", "properties": {
                    "beat": { "type": "integer", "minimum": 0, "description": "The beat it happened." },
                    "region_ref": { "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "The region it happened in." } },
                  "required": ["beat", "region_ref"] }
            ]
        }),
    );
    d.insert(
        "EventPage".into(),
        object(
            "A page of public events in ledger order.",
            &[
                ("items", array(r("QueryEvent"), "Oldest first.")),
                ("since", opt_int("The `since` asked (null: the latest events).")),
                ("next", int("The ledger row to ask from next time (`since`); poll with it.")),
                ("more", boolean("More rows to scan right now: ask again with `since: next`.")),
                ("head", int("Ledger rows so far.")),
            ],
            &[],
        ),
    );
    d
}

fn person() -> Value {
    let memory = object(
        "One memory you can see.",
        &[
            ("memory", int("Its index.")),
            ("beat", int("")),
            ("kind", e(&["witnessed", "did", "told", "rumor", "inherited", "planted", "summary", "report"], "")),
            ("text", string("")),
            ("about", array(r("Ref"), "Who and what it is about.")),
            ("row", opt_ref("The ledger row it remembers.")),
            ("confidence", num("0..1.")),
        ],
        &[],
    );
    object(
        "`game.person`: one person. The seat-only fields are absent for a public asker (HTTP).",
        &[
            ("ref", r("Ref")),
            ("name", string("")),
            ("alive", boolean("")),
            ("role", nullable(r("Role"))),
            ("rank", nullable(r("Rank"))),
            ("place", r("Ref")),
            ("place_name", string("")),
            ("region", string("")),
            ("at", r("LonLat")),
            ("died_beat", opt_int("")),
            ("visual", r("Visual")),
        ],
        &[
            ("yours", boolean("Seat only: a body your agents steer, or someone your side brought into the story.")),
            ("follower", boolean("Seat only: in your Following.")),
            ("allegiance", array(r("Allegiance"), "Seat only: what you know of their loyalty: toward you, or all of it when they are yours.")),
            ("memories", object(
                "Seat only: their memories you can see (all when yours, else those about events you know or told by your side), newest first.",
                &[("items", array(memory, "")), ("next_cursor", cursor())],
                &[],
            )),
        ],
    )
}

fn army() -> Value {
    let order = object(
        "The order the army acts on, as its general understood it.",
        &[
            ("text", json!({ "type": ["string", "null"], "description": "The demigod's words: owner only (null for others)." })),
            ("by", r("Ref")),
            ("beat", int("")),
            ("reading", e(&["obey", "misread", "stall", "refuse", "mutiny", "defect"], "How the general read it.")),
            ("target", opt_ref("")),
            ("stance", r("Stance")),
            ("speech", string("The general's line.")),
            ("why", string("order, engage, strain or initiative.")),
            ("llm", boolean("The general mind decided it.")),
        ],
        &[],
    );
    json!({
        "type": "object",
        "description": "`game.army`: one army. `orders` and `chain` only for its demigod's seat.",
        "allOf": [
            { "$ref": "#/$defs/ArmyMarker" },
            object("", &[
                ("live", boolean("Still in the field (not wiped out, and its demigod stands or it is rogue).")),
                ("yours", boolean("You are its demigod's seat.")),
                ("supply", num("")),
                ("raised_beat", int("")),
                ("battles", int("")),
                ("losses", int("")),
                ("general", nullable(r("Who"))),
                ("captains", array(r("Who"), "")),
                ("units", int("Live units (squads).")),
                ("order", nullable(order)),
            ], &[
                ("orders", string("Owner only: its standing orders.")),
                ("chain", json!({ "type": "object", "additionalProperties": true,
                                  "description": "Owner only: the full chain of command (general and captains with their sheets, units, morale, supply, strain, the order with its text)." })),
            ]),
        ]
    })
}

/// (file stem, query name, reply body).
pub(super) fn replies() -> Vec<(&'static str, &'static str, Value)> {
    let pv = public_view();
    let demigod = pv["properties"]["demigods"]["items"].clone();
    let sum = match_summary();
    let standing = sum["properties"]["standings"]["items"].clone();
    let pile = sum["properties"]["piles"]["items"].clone();
    let power = brief_public()["properties"]["powers"]["items"].clone();
    vec![
        ("query_globe", "globe", object(
            "`game.globe`: the globe poll.",
            &[
                ("beat", int("Last beat started.")),
                ("beats_total", int("The Final Night's beat.")),
                ("final_night", boolean("")),
                ("arena", r("Arena")),
                ("alive", int("Demigods standing.")),
                ("entropy", r("Entropy")),
                ("demigods", array(demigod.clone(), "Every demigod, as in the PublicView.")),
                ("influence", array(r("Influence"), "Each demigod's share of each region.")),
                ("mods", array(r("WorldMod"), "Live world mods.")),
                ("armies", array(r("ArmyMarker"), "Live armies.")),
                ("events", r("EventPage")),
            ],
            &[
                ("players", array(r("Player"), "Rules 0.7.0: the seat list.")),
                ("roads", array(r("Road"), "Rules 0.8.0: every road.")),
                ("shrines", array(r("Shrine"), "Rules 0.8.0: every shrine.")),
                ("time_of_day", r("TimeOfDay")),
                ("time_of_day_phase", r("DayPhase")),
                ("regions", array(object("A region and its hotspot.", &[("ref", r("Ref")), ("name", string("")), ("center", r("LonLat")),
                    ("hotspot", json!({ "anyOf": [{ "$ref": "#/$defs/Hotspot" }, { "type": "null" }] }))], &[]),
                    "Rules 0.7.0: every region with its hotspot (null where nothing happens). Zoom level 2 opens on it.")),
            ],
        )),
        ("query_region", "region", object(
            "`game.region`: one region (a district in a city arena).",
            &[
                ("ref", r("Ref")),
                ("name", string("")),
                ("center", r("LonLat")),
                ("radius_km", num("")),
                ("node", r("Node")),
                ("cities", array(r("Node"), "Its cities (in a city arena: its sites).")),
                ("crowds", page(r("Crowd"), "Its crowds, biggest first.")),
                ("mobs", array(r("Mob"), "Live mobs in it.")),
                ("armies", array(r("ArmyMarker"), "Live armies standing in it.")),
                ("demigods", array(demigod, "Live demigods standing in it.")),
                ("mods", array(r("WorldMod"), "Live mods centered in it.")),
                ("influence", array(r("Influence"), "Shares of this region.")),
                ("events", r("EventPage")),
            ],
            &[("hotspot", json!({ "anyOf": [{ "$ref": "#/$defs/Hotspot" }, { "type": "null" }], "description": "Rules 0.7.0: its most dynamic area." })),
            ("encounters", array(r("Encounter"), "Rules 0.8.0: encounter hexes here (hidden ones only as marked).")),
                ("loot", array(r("LootToken"), "Rules 0.8.0: item tokens lying here.")),
                ("shrines", array(r("Shrine"), "Rules 0.8.0.")),
                ("roads", array(r("Road"), "Rules 0.8.0: roads with an end here.")),
                ("memory_summary", nullable(r("MemoryEntry"))),
                ("observed_beat", opt_int("Rules 0.8.0.")),
                ("extrapolated", boolean("Rules 0.8.0.")),
                ("time_of_day", r("TimeOfDay")),
                ("time_of_day_phase", r("DayPhase")),
            ],
        )),
        ("query_node", "node", object(
            "`game.node`: one hierarchy node.",
            &[
                ("kind", e(&["place", "crowd", "cell", "mob", "hex"], "Which builder `node` is (hex: rules 0.8.0, asked with `cell`).")),
                ("node", json!({ "description": "place: Node; crowd: Crowd; cell: Cell; mob: Mob; hex: HexState.",
                                 "anyOf": [r("Node"), r("Crowd"), r("Cell"), r("Mob"), r("HexState")] })),
            ],
            &[
                ("ref", r("Ref")),
                ("cell", string("hex only: the hex key asked.")),
                ("beat", int("hex only.")),
            ("encounters", array(r("Encounter"), "Rules 0.8.0: encounter hexes here (hidden ones only as marked).")),
                ("loot", array(r("LootToken"), "Rules 0.8.0: item tokens lying here.")),
                ("shrines", array(r("Shrine"), "Rules 0.8.0.")),
                ("roads", array(r("Road"), "Rules 0.8.0: roads with an end here.")),
                ("memory_summary", nullable(r("MemoryEntry"))),
                ("observed_beat", opt_int("Rules 0.8.0.")),
                ("extrapolated", boolean("Rules 0.8.0.")),
                ("time_of_day", r("TimeOfDay")),
                ("time_of_day_phase", r("DayPhase")),
                ("children", array(r("Node"), "place only: the places below.")),
                ("crowds", page(r("Crowd"), "place only: crowds living under it, biggest first.")),
                ("people", page(r("Who"), "crowd, cell, mob: people known there (came from the crowd, members, in the mob).")),
            ],
        )),
        ("query_person", "person", person()),
        ("query_army", "army", army()),
        ("query_events", "events", json!({
            "type": "object",
            "description": "`game.events`: a page of public events.",
            "allOf": [ r("EventPage"), object("", &[("beat", int("Last beat started."))], &[]) ]
        })),
        ("query_me", "me", seat_view_body("`game.me`: your seat view now (the SeatView, as in round.private_state).")),
        ("query_powers", "powers", object("`game.powers`.", &[("powers", array(power, "The palette, then the custom entry."))], &[])),
        ("query_gods", "gods", object(
            "`game.gods`.",
            &[("gods", array(object(
                "One god.",
                &[
                    ("ref", r("Ref")),
                    ("name", string("")),
                    ("slug", string("")),
                    ("domain", string("")),
                    ("health", json!({ "type": ["number", "null"], "description": "0..10 (Entropy wounds gods at 80+)." })),
                    ("unmade", boolean("Health 0: its invocations go to Entropy.")),
                ],
                &[],
            ), "In id order (gd:0 Time .. gd:6 Entropy)."))],
            &[],
        )),
        ("query_standings", "standings", object(
            "`game.standings`: everyone ranked now (alive first, then who fell last, then score; never by health).",
            &[
                ("beat", int("")),
                ("beats_total", int("")),
                ("alive", int("")),
                ("over", boolean("The match is over.")),
                ("standings", array(json!({ "type": "object", "allOf": [standing, object("", &[
                    ("bolts", int("Bolts held (public).")),
                    ("controller", e(&["human", "ai"], "As in the PublicView's demigods.")),
                    ("connected", json!({ "type": ["boolean", "null"], "description": "As in the PublicView's demigods: true for AI, null for a human (see room.updated seats)." })),
                ], &[])] }), "Best first.")),
            ],
            &[
                ("players_alive", int("Rules 0.7.0.")),
                ("players", array(object("Rules 0.7.0: one player ranked (never health).", &[("rank", int("")),
                    ("seat", json!({ "type": ["integer", "null"], "minimum": 0 })), ("player", string("")), ("aura", string("")),
                    ("alive", boolean("")), ("bolts", int("")),
                    ("demigods", array(object("", &[("ref", r("Ref")), ("side", r("Hand")), ("name", string("")), ("alive", boolean("")),
                        ("eliminated_beat", opt_int("")), ("cause", json!({ "type": ["string", "null"] }))], &[]), ""))], &[]), "Best first.")),
            ],
        )),
        ("query_present", "present", json!({ "anyOf": [r("Present"), { "type": "null" }],
            "description": "`game.present` (state schema 8): the present moment, the same object as match.snapshot.public_state.present; null in older states." })),
        ("query_items", "items", object(
            "`game.items` (rules 0.7.0): a page of items, most recent first: public ones for everyone, your own in full.",
            &[("items", array(r("Item"), "")), ("next_cursor", opt_int("")), ("total", int("")), ("beat", int(""))],
            &[],
        )),
        ("query_item", "item", json!({ "type": "object", "description": "`game.item` (rules 0.7.0): one item with its manifestation and custody history.",
            "allOf": [r("Item"), object("", &[], &[("manifestation", json!({ "anyOf": [{ "$ref": "#/$defs/Manifestation" }, { "type": "string" }, { "type": "null" }] }))])] })),
        ("query_media", "media", object(
            "`game.media`: an image of something in the game, by reference. Through the server: `image` at once \
             (pending, then ready via GET /media/index). The bare plugin (no media server) answers `image_prompt` and \
             `image_kind` instead of `image`.",
            &[
                ("kind", e(&["portrait", "diorama", "place", "event", "army"], "As asked.")),
                ("ref", r("Ref")),
                ("name", string("The demigod's, person's, place's or army's name; an event's text.")),
            ],
            &[
                ("image", r("ImageObject")),
                ("reason", string("Why `image.status` is `off` (images off, a budget reached).")),
                ("visual", r("Visual")),
                ("image_prompt", string("Bare plugin only: the subject.")),
                ("image_kind", e(&["portrait", "diorama", "place", "event", "army"], "Bare plugin only.")),
            ],
        )),
        ("query_memory", "memory", object(
            "`game.memory` (rules 0.8.0): the DM's public decisions about a place, or anything above or below it, at or before a beat, newest first.",
            &[("place", string("As asked.")), ("ancestors", array(json!({ "type": "string" }), "The place's ancestors, nearest first.")),
              ("until_beat", int("")), ("beat", int("Last beat started.")), ("items", array(r("MemoryEntry"), "")),
              ("observed_beat", opt_int("A hex: when the DM last observed it (null: never); a place: until_beat.")),
              ("extrapolated", boolean("A hex: its state is extrapolated from its last observation."))],
            &[("state", r("HexState"))],
        )),
        ("query_encounters", "encounters", object(
            "`game.encounters` (rules 0.8.0): the encounter hexes on the board.",
            &[("beat", int("")), ("region", opt_ref("As asked (null: the whole board).")), ("hidden", int("")), ("revealed", int("")),
              ("encounters", array(r("Encounter"), "Hidden ones only as {id, cell, marked}."))],
            &[],
        )),
        // (the Earth simulator's replies are in earth.rs)
        ("query_piles_final", "piles_final", object(
            "`game.piles_final`: the end-of-match piles (an error until the match is over).",
            &[("beat", int("The last beat.")), ("piles", array(pile, "One per demigod, in id order (as in match.ended.summary.piles)."))],
            &[],
        )),
    ]
}
