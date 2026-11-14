//! JSON Schemas for every game-defined payload a Final Night 7 client receives or sends (see `docs/CLIENT_API.md`), a
//! light validator for them, and the table that ties each schema to the message (and JSON pointer) it describes.
//!
//! The model types that payloads carry verbatim (`SeatDelta`, `YouLine`, `NewsLine`, `HandLine`, `SeatHand`,
//! `SubagentLine`, `Omen`, `FavorLine`, `Zoom`, `DebtStatus`, `Stage`, `Ref`) come from `schemars` derives in `fn7-model`
//! (feature `schema`).
//! Payloads the plugin builds with `serde_json::json!` (settings, brief, round.started, round.outcome, the public and
//! seat views, the match summary) are written here by hand to match `crates/fn7-plugin/src/{settings,actions,phases,
//! beat,world}.rs` exactly. `fn7-schemas` writes them to `docs/schemas/`; the tests check the files are current and
//! that every example in `docs/examples/` validates.

use fn7_model::*;
use schemars::generate::SchemaSettings;
use serde_json::{json, Map, Value};

/// The `Ref` pattern (also in fn7-model's `JsonSchema` impl for `Ref`).
pub const REF_PATTERN: &str = r"^(dg|ch|cl|gr|gd|pl|it|ag|lg|ce|mb):\d+$";

/// Where each schema applies: (schema stem, message type `t`, JSON pointer into the envelope). The example for a message
/// is `docs/examples/<t>.json`.
pub const TARGETS: &[(&str, &str, &str)] = &[
    ("settings", "match.started", "/d/rules"),
    ("action_fields", "action.submit", "/d/fields"),
    ("round_started_public", "round.started", "/d/public"),
    ("round_started_private", "round.started", "/d/private"),
    ("brief_public", "round.brief", "/d/public"),
    ("brief_private", "round.brief", "/d/private"),
    ("outcome_public", "round.outcome", "/d/public"),
    ("round_private", "round.private", "/d/private"),
    ("public_view", "round.committed", "/d/public_state"),
    ("seat_view", "round.private_state", "/d/state"),
    ("match_summary", "match.ended", "/d/summary"),
    ("phase_event", "phase.event", "/d/public"),
];

const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

fn r(name: &str) -> Value {
    json!({ "$ref": format!("#/$defs/{name}") })
}
fn int(desc: &str) -> Value {
    json!({ "type": "integer", "minimum": 0, "description": desc })
}
fn num(desc: &str) -> Value {
    json!({ "type": "number", "description": desc })
}
fn string(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}
fn boolean(desc: &str) -> Value {
    json!({ "type": "boolean", "description": desc })
}
fn nullable(mut v: Value) -> Value {
    if let Some(t) = v.get("type").and_then(Value::as_str).map(str::to_string) {
        v["type"] = json!([t, "null"]);
        v
    } else {
        json!({ "anyOf": [v, { "type": "null" }] })
    }
}
fn array(items: Value, desc: &str) -> Value {
    json!({ "type": "array", "items": items, "description": desc })
}
/// An object with every listed property required (in this order) plus optional ones.
fn object(desc: &str, req: &[(&str, Value)], opt: &[(&str, Value)]) -> Value {
    let mut props = Map::new();
    for (k, v) in req.iter().chain(opt.iter()) {
        props.insert((*k).into(), v.clone());
    }
    json!({
        "type": "object",
        "description": desc,
        "properties": props,
        "required": req.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
    })
}

/// Shared hand-written definitions (payload pieces built with `json!`).
fn shared_defs() -> Map<String, Value> {
    let mut d = Map::new();
    d.insert(
        "LonLat".into(),
        json!({ "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2,
                "description": "[longitude, latitude] in degrees (WGS84). Note the order: longitude first." }),
    );
    d.insert(
        "Arena".into(),
        object(
            "The arena: the whole planet (default) or one real city. Time's clock shrinks the safe radius around `center` \
             linearly from start_radius_km (beat 0) to end_radius_km (the Final Night).",
            &[
                ("kind", json!({ "type": "string", "enum": ["planet", "city"], "description": "planet: regions → cities; city: districts → sites." })),
                ("name", string("\"Earth\" on the planet, else the city's name.")),
                ("city", string("The city at `center`: on the planet, where the Final Night converges; else the arena city.")),
                ("center", r("LonLat")),
                ("radius_km", num("Current safe radius around `center`. Bodies outside take 1-5 damage at each beat's Start.")),
                ("start_radius_km", num("Safe radius at match start.")),
                ("end_radius_km", num("Safe radius on the Final Night.")),
            ],
            &[],
        ),
    );
    d.insert(
        "Entity".into(),
        object(
            "One thing a seat can see (`world::entity_json`): a ref, its display name and kind, plus a few kind-specific \
             fields. Rivals' health is never included.",
            &[
                ("ref", r("Ref")),
                ("name", string("Display name.")),
                ("kind", json!({ "type": "string",
                    "enum": ["demigod", "character", "cluster", "group", "god", "place", "item", "agent", "ledger", "cell", "mob"],
                    "description": "Matches the ref prefix (dg, ch, cl, gr, gd, pl, it, ag, lg, ce, mb)." })),
            ],
            &[
                ("pos", json!({ "$ref": "#/$defs/LonLat", "description": "Where it is, when it has a position (absent for groups, gods, agents)." })),
                ("size", int("Clusters only: approximate head count.")),
                ("mood", json!({ "type": "string", "enum": ["calm", "angry", "afraid", "hopeful", "devoted"],
                                 "description": "Clusters only: the dominant mood." })),
                ("alive", boolean("Characters only.")),
                ("army_of", json!({ "$ref": "#/$defs/Ref", "description": "Clusters that are armies only: the demigod (`dg:<n>`) it marches for." })),
                ("role", json!({ "$ref": "#/$defs/Role", "description": "Characters with a society sheet only." })),
                ("rank", json!({ "$ref": "#/$defs/Rank", "description": "Characters with a society sheet only." })),
                ("action", json!({ "type": "string", "description": "Mobs only: what it is doing (gathering, rioting, looting, attacking, marching, dispersed)." })),
                ("ringleader", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "Mobs only." })),
                ("cell_kind", json!({ "type": "string", "description": "Cells only: gang, squad, congregation, crew or militia." })),
                ("leader", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "Cells only." })),
            ],
        ),
    );
    let geo = |desc: &str| json!({ "anyOf": [{ "$ref": "#/$defs/LonLat" }, { "type": "null" }], "description": desc });
    d.insert(
        "Entropy".into(),
        object(
            "The world's Entropy meter. Every bolt spent anywhere raises it; thresholds make the world wackier (20 \
             weather, 40 physics, 60 riots, 80 godfall); at `max` heat death makes that beat the Final Night.",
            &[
                ("value", num("0..max.")),
                ("max", num("100: heat death.")),
                ("level", int("0..4.")),
                ("key", json!({ "type": "string", "enum": ["calm", "weather", "physics", "riots", "godfall"], "description": "The level's key." })),
                ("text", string("What this level does to the world, one line.")),
                ("next_at", num("The value of the next threshold (max at the top level).")),
                ("heat_death", json!({ "type": ["integer", "null"], "minimum": 0, "description": "The beat heat death struck, or null." })),
            ],
            &[("delta", num("Change during this beat (outcome only)."))],
        ),
    );
    d.insert(
        "SlowState".into(),
        object(
            "The open slow-time window as of the last resolve (public: who opened it, never who else is typing). The \
             live window is the Theatre's: `window.opened` / `window.closed` and `match.snapshot.window`.",
            &[
                ("by", string("The initiator's name.")),
                ("by_ref", r("Ref")),
                ("window_id", int("The Theatre's window id (`window.opened.window_id`); submit orders with it.")),
                ("opened_ms", int("When it opened (epoch ms).")),
                ("deadline_ms", int("When it closes at the latest (epoch ms).")),
                ("opened_beat", int("The first beat that resolved while it was open.")),
            ],
            &[],
        ),
    );
    d.insert(
        "SlowEvent".into(),
        object(
            "One slow-time announcement this beat: a window that closed since the last beat (with Time's bill), or \
             orders sent without a window (an instant Enter, billed as a zero-second slow). A window still open is \
             announced by the core (`window.opened`, `match.snapshot.window`) and the `slowed` phase.event.",
            &[
                ("event", json!({ "type": "string", "enum": ["closed", "instant"], "description": "closed (settled now), or orders without a window." })),
                ("by", string("The initiator's name.")),
                ("by_ref", r("Ref")),
                ("text", string("\"<name> slowed time\", the closing line, or the instant-Enter line.")),
            ],
            &[
                ("window_id", int("closed: the Theatre's window id.")),
                ("how", json!({ "type": "string", "enum": ["enter", "timeout"], "description": "closed only: the initiator's submit (Enter), or the deadline (the opener is billed the full slow even if nobody submitted)." })),
                ("after_ms", int("closed / instant: how long time was slowed (real ms: from the window's opening to the Enter's arrival, or the deadline; 0 for instant).")),
                ("debt", num("closed / instant: the debt Time wrote (0 if the initiator had fallen).")),
                ("paid_bolt", boolean("closed / instant: Time got its bolt (if not, the debt doubled).")),
            ],
        ),
    );
    d.insert(
        "Slowed".into(),
        object(
            "`phase.event.public` right after `window.opened`: who slowed time (from `action.begin`).",
            &[
                ("event", json!({ "const": "slowed" })),
                ("by", string("The initiator: the player's name (rules 0.7.0; before, the demigod's).")),
                ("by_ref", r("Ref")),
                ("by_seat", int("The initiator's seat (= window.opened.by_seat).")),
                ("slow_ms", int("How long the window stays open at most.")),
                ("text", string("\"<name> slowed time\".")),
            ],
            &[
                ("opened_at_ms", int("Rules 0.7.0, the live Time bill: when the window opened (server epoch ms).")),
                ("deadline_ms", int("When it closes at the latest (epoch ms).")),
                ("bill_now", object("The bill if Enter came now (at opening): one bolt and `debt_base`.", &[("bolts", int("")), ("debt", num(""))], &[])),
                ("debt_base", num("debt(t) = min(debt_cap, debt_base + debt_per_second × t), t = seconds since opened_at_ms.")),
                ("debt_per_second", num("")),
                ("debt_per_10s", num("= debt_base.")),
                ("debt_cap", num("10.")),
                ("repeat_factor", num("1 + 0.5 × the opener's slows in the last 20 beats (already in debt_base).")),
                ("recent_slows", int("")),
                ("bolt_paid", boolean("The purse has the bolt (else the debt is doubled, already in debt_base).")),
            ],
        ),
    );
    let streak_cast = object(
        "One cast of the streak, as aimed (nothing is paid until the next resolve; at Entropy ≥ 40 it may still land off target).",
        &[
            ("power", string("Palette slug or \"custom\".")),
            ("power_name", string("Display name.")),
            ("icon", string("Icon key.")),
            ("cost", int("Bolts it will cost.")),
            ("radius_km", num("Area it affects.")),
        ],
        &[
            ("text", string("custom only: what it is.")),
            ("tier", json!({ "type": "string", "enum": ["mega", "mini"], "description": "Rules 0.7.0." })),
            ("at", r("LonLat")),
            ("region", string("Region (planet) or district (city) aimed at.")),
            ("region_ref", r("Ref")),
            ("place", string("The most specific place aimed at.")),
            ("target", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "What it is aimed at directly, or null (a point)." })),
            ("target_name", json!({ "type": ["string", "null"] })),
            ("fizzled", string("Its target doesn't exist (it will fizzle).")),
            ("fault_km", num("Rules 0.8.0, megas only: great-circle distance (km) from the aim to the nearest fault line.")),
            ("fault_factor", num("Rules 0.8.0, megas only: the damage multiplier the fault rule gives (1.6 near a fault .. 0.6 far).")),
        ],
    );
    d.insert(
        "Streak".into(),
        object(
            "`phase.event.public` the moment an instant cast is accepted: its streak, for everyone. The first cast's fields \
             are repeated at the top level.",
            &[
                ("event", json!({ "const": "streak" })),
                ("by", string("Caster's name.")),
                ("by_ref", r("Ref")),
                ("by_seat", int("Caster's seat.")),
                ("casts", array(streak_cast, "Every cast in the action, in order.")),
                ("seq", int("The action's room-wide arrival order (casts are paid in this order).")),
                ("received_at_ms", int("When the server received it (epoch ms).")),
                ("power", string("First cast: slug.")),
                ("power_name", string("First cast: display name.")),
                ("icon", string("First cast: icon key.")),
                ("cost", int("First cast: bolts.")),
            ],
            &[
                ("at", r("LonLat")),
                ("region", string("First cast: region aimed at.")),
                ("region_ref", r("Ref")),
                ("place", string("First cast: place aimed at.")),
                ("target", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }] })),
                ("target_name", json!({ "type": ["string", "null"] })),
                ("fault_km", num("Rules 0.8.0: first cast, a mega: distance (km) to the nearest fault line.")),
                ("fault_factor", num("Rules 0.8.0: first cast, a mega: the fault rule's damage multiplier.")),
            ],
        ),
    );
    d.insert(
        "CastLine".into(),
        object(
            "One cast this beat (public). A fizzled cast has `fizzled` and no row.",
            &[
                ("by", string("Caster's name.")),
                ("by_ref", r("Ref")),
                ("power", string("The power's slug (`powers[].slug`, or \"custom\").")),
                ("icon", string("The power's icon key (= its slug for palette powers).")),
                ("power_name", string("Display name.")),
                ("cost", int("Bolts it cost.")),
                ("seq", int("Its action's room-wide arrival order (0 for the built-in AI, which casts first).")),
                ("instant", boolean("Sent as an instant action.")),
            ],
            &[
                ("tier", json!({ "type": "string", "enum": ["mega", "mini"], "description": "Rules 0.7.0: mega (cost a bolt) or mini (cost a spark)." })),
                ("radius_km", num("Rules 0.7.0: the variant's radius.")),
                ("at", r("LonLat")),
                ("region", string("Region (planet) or district (city) it struck.")),
                ("region_ref", r("Ref")),
                ("place", string("The most specific place it struck (a city, or the region).")),
                ("target", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "What it hit directly, or null (a point)." })),
                ("target_name", json!({ "type": ["string", "null"] })),
                ("off_target", boolean("Entropy ≥ 40: physics threw it off its aim.")),
                ("row", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "Its ledger row." })),
                ("fizzled", string("Why it fizzled (not enough bolts, no such target); nothing was spent.")),
                ("fault_km", num("Rules 0.8.0, megas only: distance (km) from where it struck to the nearest fault line.")),
                ("fault_factor", num("Rules 0.8.0, megas only: the fault rule's damage multiplier (radius and magnitude scale with it).")),
            ],
        ),
    );
    d.insert(
        "ImageObject".into(),
        object(
            "An image the media server attached (only when the server has images on; see docs/MEDIA.md). Fetch `url` \
             once `status` is `ready` (watch GET /media/index for `hash`); `off` = none will come, show a placeholder.",
            &[
                ("hash", string("Content key (sha256 hex): cache by it forever.")),
                ("kind", json!({ "type": "string", "enum": ["portrait", "item", "event", "statue", "place", "army", "diorama"] })),
                ("status", json!({ "type": "string", "enum": ["pending", "ready", "off"] })),
                ("url", json!({ "type": ["string", "null"], "description": "`/media/<hash>.<ext>` once ready, relative to the server's HTTP origin." })),
                ("prompt", string("The full prompt sent to the image model (house style; no engine ids).")),
            ],
            &[],
        ),
    );
    d.insert(
        "Visual".into(),
        object(
            "A character's visual sheet (docs/SHARED_MEDIA_SPEC.md §3.1): picked from the match seed, the character's id \
             and name; the same at every beat and in every replay. Every image of the character carries these traits.",
            &[
                ("sheet_hash", string("sha256 (hex) of the sheet version, the character's ref and its traits: key art or caches on it.")),
                ("traits", object(
                    "",
                    &[
                        ("build", string("Body type.")),
                        ("face", string("Face and hair.")),
                        ("outfit", string("What they wear.")),
                        ("colors", json!({ "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 2,
                                           "description": "Two signature colors (color words)." })),
                        ("prop", string("The one thing they carry.")),
                        ("emblem", string("Their emblem (also on their armies' banners).")),
                    ],
                    &[],
                )),
            ],
            &[],
        ),
    );
    d.insert(
        "StoryboardShot".into(),
        object(
            "One shot of the final-cut animatic (docs/SHARED_MEDIA_SPEC.md §3.3): fly the globe to `at`, show `image`, \
             caption it, hold `duration_ms`.",
            &[
                ("shot", int("Index, from 0.")),
                ("kind", json!({ "type": "string", "enum": ["title", "moment", "champion"],
                                 "description": "title: the final cut's title over the arena; moment: one of the match's biggest public moments; champion: the champion's close-up with the final cut's closing lines." })),
                ("beat", int("The beat it shows (0 for the title).")),
                ("at", geo("Where, [lon, lat].")),
                ("region", string("Region (planet) or district (city), or the arena for the title.")),
                ("image_prompt", string("The subject the image is drawn from (a moment's is exactly its moment card's, so the same picture).")),
                ("image_kind", json!({ "type": "string", "enum": ["event", "portrait"] })),
                ("image", json!({ "anyOf": [{ "$ref": "#/$defs/ImageObject" }, { "type": "null" }],
                                  "description": "The event or portrait image object (null when the server has images off)." })),
                ("caption", string("What to show under it.")),
                ("duration_ms", int("How long to hold it.")),
                ("camera", json!({ "type": "string", "enum": ["globe", "close"], "description": "globe: fly the globe to `at`; close: a close-up (eliminations, the champion)." })),
                ("row", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "moment: its ledger row." })),
                ("refs", array(r("Ref"), "Who is in it.")),
            ],
            &[],
        ),
    );
    d.insert(
        "WorldEvent".into(),
        object(
            "A public, geotagged world event (one ledger row): casts and their ramifications, armies raised, marching and \
             fighting, freak weather, world mods, wounded gods, eliminations.",
            &[
                ("row", r("Ref")),
                ("kind", string("The row's verb: cast, happen, march, raise_army, shrink, damage, mood_shift, mod, unmod, wound, eliminate...")),
                ("text", string("What happened, in prose.")),
                ("at", geo("Where, [lon, lat].")),
                ("region", json!({ "type": ["string", "null"], "description": "Region (planet) or district (city)." })),
                ("refs", array(r("Ref"), "The actor, then the targets.")),
                ("magnitude", int("0..10.")),
            ],
            &[
                ("image_prompt", string("Every public event: its image subject (a candidate moment card; it names who is in it with their visual sheets). Older recordings: only the big ones.")),
                ("image_kind", json!({ "type": "string", "enum": ["event"], "description": "With image_prompt." })),
                ("image", json!({ "$ref": "#/$defs/ImageObject", "description": "A moment card: only on events the media style's event_selection picks (and only with images on)." })),
            ],
        ),
    );
    d.insert(
        "Army".into(),
        object(
            "A live army (a cluster raised from a crowd).",
            &[
                ("ref", r("Ref")),
                ("name", string("")),
                ("demigod", string("The demigod it marches for.")),
                ("demigod_ref", r("Ref")),
                ("size", int("Head count.")),
                ("pos", r("LonLat")),
                ("region", string("Where it is.")),
                ("dest", geo("Where it is headed, or null.")),
                ("target", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "What it marches on (a place, or a demigod it hunts)." })),
            ],
            &[
                ("orders", string("Seat view only (your armies): its standing orders.")),
                ("rogue", boolean("Its general mutinied: it answers to no demigod.")),
                ("stance", r("Stance")),
                ("general", json!({ "anyOf": [object("", &[("ref", r("Ref")), ("name", string(""))], &[]), { "type": "null" }], "description": "Its live general." })),
                ("chain", json!({ "type": "object", "additionalProperties": true, "description": "Its chain of command (general, captains, units, morale, supply, the order as understood; the order's text only for its owner)." })),
            ],
        ),
    );
    d.insert(
        "WorldMod".into(),
        object(
            "A persistent world mod (left by a cast's ramifications), applied every beat inside its radius.",
            &[
                ("id", int("")),
                ("name", string("")),
                ("description", string("")),
                ("at", r("LonLat")),
                ("radius_km", num("")),
                ("region", string("")),
                ("region_ref", r("Ref")),
                ("tags", array(json!({ "type": "string" }), "")),
                ("stacks", int("1..5: the same mod again in the same region stacks.")),
                ("since_beat", int("")),
                ("expires_beat", json!({ "type": ["integer", "null"], "minimum": 0, "description": "null = lasting." })),
                ("power", json!({ "type": ["string", "null"], "description": "The power that left it." })),
                ("by", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "The caster." })),
            ],
            &[],
        ),
    );
    d.insert(
        "Influence".into(),
        object(
            "One demigod's share of one region (district in a city): crowds there weighted by their allegiance, its \
             Following, its armies, its presence. A region's shares sum to at most 1; shares under 0.005 are omitted.",
            &[
                ("region", string("")),
                ("region_ref", r("Ref")),
                ("demigod", string("")),
                ("demigod_ref", r("Ref")),
                ("share", num("0..1.")),
            ],
            &[],
        ),
    );
    d.insert(
        "DebtLine".into(),
        object(
            "One open debt you owe.",
            &[
                ("row", json!({ "$ref": "#/$defs/Ref", "description": "The ledger row that opened the debt (`lg:<n>`)." })),
                ("owed_to", string("The god's display name (usually Consequence; a god's price is owed to that god).")),
                ("owed_to_ref", json!({ "$ref": "#/$defs/Ref", "description": "That god's ref (`gd:<n>`)." })),
                ("amount", num("Damage points (0-10 scale) collected as an accident when it falls due.")),
                ("due_beat", int("Beat it falls due (everything falls due on the Final Night).")),
                ("status", r("DebtStatus")),
            ],
            &[("owed_by", json!({ "$ref": "#/$defs/Ref", "description": "Rules 0.7.0: which of your two demigods owes it (the acting body)." }))],
        ),
    );
    d
}

/// Generated definitions for the model types payloads carry, with descriptions added where the Rust has none.
fn model_defs() -> Map<String, Value> {
    let mut g = SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator();
    g.subschema_for::<SeatDelta>();
    g.subschema_for::<SeatHand>();
    g.subschema_for::<Hand>();
    g.subschema_for::<DebtStatus>();
    g.subschema_for::<Stage>();
    g.subschema_for::<Ref>();
    let mut d = g.take_definitions(true);
    let mut describe = |ty: &str, field: &str, text: &str| {
        if let Some(p) = d
            .get_mut(ty)
            .and_then(|t| t.pointer_mut(&format!("/properties/{field}")))
        {
            if let Some(o) = p.as_object_mut() {
                o.insert("description".into(), text.into());
            }
        }
    };
    describe(
        "SeatDelta",
        "beat",
        "The beat that just resolved (= the Theatre round).",
    );
    describe("SeatDelta", "seat", "Your seat.");
    describe(
        "SeatDelta",
        "zoom",
        "Your zoom level after this beat's view changes.",
    );
    describe(
        "SeatDelta",
        "hands",
        "Your two hands (index 0 = left, 1 = right): what each said back this beat and its live subagents.",
    );
    describe(
        "SeatDelta",
        "omens",
        "This beat's rulings you may see: the public ones plus your own invocations, even small ones.",
    );
    describe(
        "SeatDelta",
        "news",
        "Ledger rows new this beat that you know about, newest first, at most one line per row.",
    );
    describe("Omen", "god", "The god's display name (\"Time\", \"Nature\", \"Consequence\", \"Death\", \"Chance\", \"Memory\", \"Entropy\").");
    describe("Omen", "god_ref", "The god's ref (`gd:<n>`; the table is in round.brief.public.gods).");
    describe("Omen", "speech", "What the god said. Never starts with the god's name (render `god` yourself).");
    describe("FavorLine", "god", "The god's display name.");
    describe("FavorLine", "god_ref", "The god's ref (`gd:<n>`).");
    describe("FavorLine", "favor", "That god's favor toward you (about -10..10; it shifts its rulings).");
    describe("SubagentLine", "agent", "The agent's ref (`ag:<n>`).");
    describe("SubagentLine", "name", "Display name; a possession reads \"<person> (steered by <you>)\".");
    describe("SubagentLine", "goal", "Its task.");
    describe("SubagentLine", "line", "What it said this beat (round.private), else its goal.");
    describe("SubagentLine", "beats_left", "Beats until it expires; null = no budget (possessions last until recalled).");
    describe("SubagentLine", "kind", "\"subagent\", \"possession\" (steers `body`) or \"paladin\" (a god's champion in `body`).");
    describe("SubagentLine", "parent", "The agent that spawned it (`ag:<n>`, your hand for subagents and possessions), or null.");
    describe("SubagentLine", "body", "The person it steers (`ch:<n>`) for possessions and paladins, else null.");
    describe("SeatHand", "agent", "The hand's agent ref (`ag:<n>`).");
    describe("SeatHand", "name", "\"<you>'s Left Hand\" / \"<you>'s Right Hand\".");
    describe("SeatHand", "live", "False once your demigod is eliminated.");
    describe("SeatHand", "subagents", "Live subagents and possessions under this hand.");
    describe(
        "YouLine",
        "health",
        "Current health (0 to max_health, Final Night's 0-10 scale).",
    );
    describe("YouLine", "pos", "[longitude, latitude].");
    describe(
        "YouLine",
        "debt_open",
        "Sum of your open debts (damage points).",
    );
    describe(
        "YouLine",
        "favor",
        "Every god's favor toward you, in god order (gd:0 .. gd:6).",
    );
    describe(
        "YouLine",
        "followers",
        "Head count of your Following group.",
    );
    describe(
        "YouLine",
        "echo_charges",
        "Charges left for the echo (duplicate an item) base power.",
    );
    describe("YouLine", "bolts", "Lightning bolts you hold (the god-energy unit: casts and god invocations cost them).");
    describe("NewsLine", "at", "Where it happened, [lon, lat] (null only for bookkeeping with no place).");
    describe("NewsLine", "region", "The region (planet) or district (city) it happened in.");
    describe("Omen", "at", "Where it was invoked, [lon, lat].");
    describe("Omen", "region", "The region (planet) or district (city).");
    describe("NewsLine", "row", "The ledger row's ref (`lg:<n>`). Never repeated within one payload.");
    describe("NewsLine", "text", "The act, in prose.");
    describe(
        "NewsLine",
        "refs",
        "The actor, then the targets (refs to link in the UI), each at most once.",
    );
    describe(
        "NewsLine",
        "how",
        "How you know: \"you did it\", \"it happened to you\", \"you saw it\", \"you were told\", \"your followers heard\" or \"rumor\".",
    );
    describe("HandLine", "agent", "The hand's agent ref (`ag:<n>`).");
    describe("HandLine", "name", "\"<you>'s Left Hand\" / \"<you>'s Right Hand\".");
    describe(
        "HandLine",
        "said",
        "The hand's one spoken line back to you this beat (empty when you said nothing to it).",
    );
    describe(
        "HandLine",
        "subagents",
        "Live subagents and possessions under this hand (same objects as seat_view.hands[].subagents).",
    );
    if let Some(p) = d.get_mut("SeatDelta").and_then(|t| t.pointer_mut("/properties/focus")) {
        *p = json!({ "anyOf": [{ "$ref": "#/$defs/Entity" }, { "type": "null" }],
                     "description": "What you are focused on, as an Entity object (same shape as seat_view.focus), or null." });
    }
    if let Some(n) = d
        .get_mut("NewsLine")
        .and_then(|t| t.pointer_mut("/properties/how"))
    {
        n["type"] = "string".into();
    }
    d
}

/// Every `$ref` target reachable from `v`, resolved against `pool`.
fn collect_refs(v: &Value, pool: &Map<String, Value>, out: &mut Map<String, Value>) {
    match v {
        Value::Object(o) => {
            if let Some(name) = o
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|s| s.strip_prefix("#/$defs/"))
            {
                if !out.contains_key(name) {
                    if let Some(def) = pool.get(name) {
                        out.insert(name.into(), def.clone());
                        collect_refs(def, pool, out);
                    }
                }
            }
            for x in o.values() {
                collect_refs(x, pool, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_refs(x, pool, out)),
        _ => {}
    }
}

fn finish(
    stem: &str,
    title: &str,
    applies: &str,
    mut body: Value,
    pool: &Map<String, Value>,
) -> Value {
    let mut defs = Map::new();
    collect_refs(&body, pool, &mut defs);
    let defs: Map<String, Value> = {
        let mut keys: Vec<&String> = defs.keys().collect();
        keys.sort();
        keys.into_iter()
            .map(|k| (k.clone(), defs[k].clone()))
            .collect()
    };
    let mut out = Map::new();
    out.insert("$schema".into(), DRAFT.into());
    out.insert("$id".into(), format!("{stem}.schema.json").into());
    out.insert("title".into(), title.into());
    out.insert("x-applies-to".into(), applies.into());
    if let Value::Object(b) = &mut body {
        for (k, v) in std::mem::take(b) {
            out.insert(k, v);
        }
    }
    if !defs.is_empty() {
        out.insert("$defs".into(), Value::Object(defs));
    }
    Value::Object(out)
}

fn settings_schema() -> Value {
    let i = |min: i64, max: i64, def: Value, desc: &str| json!({ "type": "integer", "minimum": min, "maximum": max, "default": def, "description": desc });
    let timer = |max: f64, desc: &str| json!({ "type": "number", "exclusiveMinimum": 0, "maximum": max, "description": desc });
    let mut props = Map::new();
    let mut p = |k: &str, v: Value| {
        props.insert(k.into(), v);
    };
    p(
        "seed",
        json!({ "type": "integer", "minimum": 0, "description": "Match seed (u64). Absent = picked at match start (not echoed back in `rules`)." }),
    );
    p(
        "city",
        json!({ "type": "string", "maxLength": 80, "description": "Arena city by name (fuzzy match). Absent = a seeded pick among cities >= 1M on land." }),
    );
    p("players", i(1, 8, json!(4), "Demigods in the match (a minimum: every seat gets one; the built-in AI plays the rest)."));
    p("arena", json!({ "type": "string", "enum": ["planet", "city"], "default": "planet",
        "description": "planet: the whole Earth (regions → cities; `city` = where the Final Night converges). city: one real city (districts → sites)." }));
    p("flow_ms", i(500, 120_000, json!(3000), "Flow beat length in ms: the world's action runs in beats this long whether or not anyone types. compose_seconds defaults to flow_ms / 1000."));
    p("slow_ms", i(1000, 600_000, json!(30000), "How long a slow-time window may stay open (ceil(slow_ms / flow_ms) flow beats)."));
    p("start_bolts", i(0, 99, json!(fn7_plugin::settings::DEFAULT_BOLT_START), "Alias of bolt_start (rules 0.7.0: lightning bolts each player starts with)."));
    p("entropy_scale", json!({ "type": "number", "minimum": 0, "maximum": 10, "default": 1.0,
        "description": "Multiplier on every Entropy rise (a playtest dial; 2 = heat death twice as fast). Cooling is not scaled." }));
    p("max_intents_per_window", i(1, 24, json!(12), "Acts one hand makes from orders that closed a slow window open at least one beat."));
    p(
        "beats_total",
        i(
            1,
            1000,
            json!(fn7_plugin::settings::DEFAULT_BEATS_TOTAL),
            "The Final Night falls on this beat (or earlier: heat death, or nobody standing). One Theatre round = one beat.",
        ),
    );
    p("rounds", i(1, 1000, json!(fn7_plugin::settings::DEFAULT_BEATS_TOTAL), "Input-only alias of beats_total (used when beats_total is absent; dropped from `rules`)."));
    p(
        "beat_ms",
        i(
            500,
            120_000,
            json!(3000),
            "Input alias of flow_ms (used when flow_ms is absent); echoed = flow_ms.",
        ),
    );
    p(
        "clusters",
        i(8, 600, json!(200), "Population clusters created at setup."),
    );
    p(
        "max_subagents_per_hand",
        i(0, 10, json!(3), "Live subagents + possessions per hand."),
    );
    p(
        "mind_timeout_ms",
        i(
            50,
            120_000,
            json!(6000),
            "Per-call timeout for every LLM mind; a late call takes its rules fallback.",
        ),
    );
    p(
        "llm",
        json!({ "type": "boolean", "default": true, "description": "false = never call the LLM (every mind uses its rules fallback)." }),
    );
    p(
        "debt_scale",
        json!({ "type": "number", "minimum": 0, "maximum": 10, "default": 0.04,
        "description": "Consequence's bill multiplier (echoed back at f32 precision: 0.04, not 0.03999999910593033)." }),
    );
    p(
        "debt_due_beats",
        i(1, 100, json!(fn7_plugin::settings::DEFAULT_DEBT_DUE_BEATS), "Beats until a bill falls due (Consequence's per-act bill and Time's bill for orders)."),
    );
    let scale = |def: f64, desc: &str| json!({ "type": "number", "minimum": 0, "maximum": 10, "default": def, "description": desc });
    p("instant_order_debt", scale(0.5, "Time's flat debt for orders sent without a slow window (bots, the built-in AI, Compose submits that skip action.begin): no repeat or seconds factor; doubled when the bolt can't be paid. Real windows keep 0.5 × (1 + seconds/10) × (1 + 0.5 × recent slows)."));
    p(
        "arena_close_beats",
        i(1, 1000, json!(fn7_plugin::settings::DEFAULT_ARENA_CLOSE_BEATS),
          "Time's circle reaches its end radius (1,500 km on the planet) on this beat, and its damage ramps on the same schedule: its own clock, independent of beats_total (a circle scheduled past the Final Night is still closing when it falls)."),
    );
    // The f32 default as its shortest decimal (0.82, not 0.8199999928474426).
    let k: f64 = format!("{}", fn7_plugin::settings::DEFAULT_KILLER_SCALE).parse().unwrap_or(1.0);
    p("accident_scale", scale(k, "Multiplier on the rules fallback's Final Destination accident chance (1 in 20 per non-attack act at 1)."));
    p("army_strike_scale", scale(k, "Multiplier on the damage an army does when it storms a demigod."));
    p("army_first_host_scale", scale(k, "Multiplier on the extra chance a crowd near an unarmed demigod raises their first volunteer host."));
    p("demigods_per_player", i(1, 2, json!(2), "Rules 0.7.0: 2 = each player's Left and Right hands are two demigods, each with its own health (the player is out when both fall); 1 = one shared body (as before 0.7.0)."));
    p("manifest_scale", scale(1.0, "Rules 0.7.0: multiplier on the item engine's bolt price per tier."));
    let muster: f64 = format!("{}", fn7_plugin::settings::DEFAULT_MUSTER_SCALE).parse().unwrap_or(0.25);
    p("manifest_muster_scale", json!({ "type": "number", "minimum": 0, "maximum": 1, "default": muster,
        "description": "Rules 0.7.0: a manifested host's army footprint in the simulation, as a share of muster_size(count) (0 = hosts never take the field)." }));
    p("manifest_calls_per_beat", i(0, 16, json!(2), "Rules 0.7.0: most manifest-mind (LLM) calls per beat (names for wonders and up, and twists)."));
    p("final_wish_magnitude", i(0, 5, json!(2), "Rules 0.7.0: magnitude added to every act of the Final Wish (beat 1)."));
    p("bolt_start", i(0, 99, json!(fn7_plugin::settings::DEFAULT_BOLT_START), "Rules 0.7.0: lightning bolts (the mega currency; also Time's toll and gods' prices) each player starts with. `start_bolts` is an alias."));
    p("spark_start", i(0, 99, json!(5), "Rules 0.7.0: sparks (the mini currency) each player starts with."));
    p("bolt_gift", i(0, 10, json!(1), "Rules 0.7.0: bolts every OTHER player gains when someone casts a mega power."));
    p("spark_gift", i(0, 10, json!(0), "Rules 0.7.0: sparks every OTHER player gains when someone casts a mega power."));
    p("entropy_per_mega", json!({ "type": "number", "minimum": 0, "maximum": 100, "default": 10.0, "description": "Rules 0.7.0: Entropy a mega cast adds (x entropy_scale)." }));
    p("entropy_per_mini", json!({ "type": "number", "minimum": 0, "maximum": 20, "default": 1.0, "description": "Rules 0.7.0: Entropy a mini cast adds (x entropy_scale)." }));
    // Rules 0.8.0: the DM's rulebook (docs/DM_RULEBOOK.md, appendix).
    let f = |min: f64, max: f64, def: f64, desc: &str| json!({ "type": "number", "minimum": min, "maximum": max, "default": def, "description": desc });
    p("beats_per_day", i(2, 200, json!(8), "Rules 0.8.0: beats in one game day (dawn, day, dusk, night)."));
    p("dm_reach_km", f(0.1, 2000.0, 30.0, "Rules 0.8.0: the step radius (km): a demigod steps on a hex when its move passes this close (planet; a city arena uses a hundredth)."));
    p("dm_encounters_per_beat", i(0, 8, json!(1), "Rules 0.8.0: encounter hexes the DM may mark per beat."));
    p("dm_encounters_max", i(0, 64, json!(10), "Rules 0.8.0: most hidden encounter hexes at once."));
    p("dm_encounter_fade_beats", i(1, 1000, json!(20), "Rules 0.8.0: beats an unfound encounter stays marked."));
    p("dm_encounter_damage_scale", f(0.0, 10.0, 1.0, "Rules 0.8.0: multiplier on encounter damage."));
    p("dm_settlement_step_beats", i(1, 100, json!(6), "Rules 0.8.0: beats a settlement holds its level before another step."));
    p("dm_road_wear", f(0.5, 1000.0, 4.0, "Rules 0.8.0: traffic wear at which a path becomes a road."));
    p("dm_road_decay", f(0.0, 10.0, 0.05, "Rules 0.8.0: wear an unused path loses per beat."));
    p("dm_shrines_per_demigod", i(0, 20, json!(3), "Rules 0.8.0: most shrines per demigod."));
    p("dm_shrine_spark_every", i(1, 100, json!(6), "Rules 0.8.0: a spark shrine's interval in beats (one fewer per level above 1, at least 2)."));
    p("dm_shrine_heal", f(0.0, 5.0, 0.15, "Rules 0.8.0: a heal shrine's healing per level per beat."));
    p("dm_shrine_ward", f(0.0, 5.0, 0.1, "Rules 0.8.0: a ward shrine's damage per level per beat to rivals within reach."));
    p("dm_memory_lines", i(0, 30, json!(6), "Rules 0.8.0: most memory lines in one LLM DM prompt."));
    p("fault_near_km", f(0.0, 5000.0, 150.0, "Rules 0.8.0: a mega this close to a fault line gets the full near bonus."));
    p("fault_far_km", f(1.0, 20000.0, 1500.0, "Rules 0.8.0: a mega this far from any fault line gets the full far penalty."));
    p("fault_near_magnitude", f(-10.0, 10.0, 3.0, "Rules 0.8.0: magnitude added near a fault line."));
    p("fault_far_magnitude", f(-10.0, 10.0, -2.0, "Rules 0.8.0: magnitude added far from any fault line."));
    p("fault_near_radius", f(0.1, 10.0, 1.5, "Rules 0.8.0: radius multiplier near a fault line."));
    p("fault_far_radius", f(0.1, 10.0, 1.0, "Rules 0.8.0: radius multiplier far from any fault line."));
    p("fault_near_damage", f(0.0, 10.0, 1.6, "Rules 0.8.0: damage multiplier near a fault line."));
    p("fault_far_damage", f(0.0, 10.0, 0.6, "Rules 0.8.0: damage multiplier far from any fault line."));
    p("summon_enabled", json!({ "type": "boolean", "default": true, "description": "Rules 0.8.0: players may summon their demigods (fields.summon)." }));
    p("summon_cooldown_beats", i(1, 100, json!(1), "Rules 0.8.0: beats between two summonings of one demigod."));
    p("summon_loss_scale", f(0.0, 10.0, 1.0, "Rules 0.8.0: multiplier on a reckless summoning's health cost."));
    p("god_power_cost", i(0, 20, json!(3), "Rules 0.8.0: bolts a god-tier power costs."));
    p("god_power_gift", i(0, 10, json!(1), "Rules 0.8.0: bolts every other player gains per god-tier power."));
    p("challenge_enabled", json!({ "type": "boolean", "default": true, "description": "Rules 0.8.1: challenge mode (fields.challenge / fields.duel, and the built-in AI's challenges). Settings recorded before 0.8.1 read it as false." }));
    p("rules_0_8_1", json!({ "type": "boolean", "default": true, "description": "Rules 0.8.1 marker (always true when validated by a 0.8.1+ server; absent in older recordings)." }));
    p("challenge_min_km", f(0.0, 20000.0, 120.0, "Rules 0.8.1: a challenge always reaches this far (km), at any zoom."));
    p("challenge_contact_km_regions", f(0.0, 20000.0, 600.0, "Rules 0.8.1: at the regions level a challenge also reaches as far as two touching board cards (km); the reach is max(challenge_min_km, this)."));
    p("challenge_contact_km_villages", f(0.0, 20000.0, 15.0, "Rules 0.8.1: the contact reach at the villages level (km)."));
    p("challenge_contact_km_houses", f(0.0, 20000.0, 0.15, "Rules 0.8.1: the contact reach at the houses level (km)."));
    p("challenge_contact_km_people", f(0.0, 20000.0, 0.015, "Rules 0.8.1: the contact reach at the people level (km)."));
    p("duel_cooldown_beats", i(1, 100, json!(1), "Rules 0.8.1: beats between two challenges by one demigod."));
    p("duel_damage_scale", f(0.0, 10.0, 1.0, "Rules 0.8.1: multiplier on duel blow damage."));
    p("duel_ai_chance", f(0.0, 1.0, 0.02, "Rules 0.8.1: chance per beat that a built-in AI demigod challenges its nearest rival."));
    p("duel_ai_reach_km", f(0.0, 20000.0, 2000.0, "Rules 0.8.1: a built-in AI demigod's rival beyond regions reach but within this summons it next to it first."));
    p("duel_auto_rounds", i(0, 10, json!(2), "Rules 0.8.1: automatic exchanges in a duel nobody typed in."));
    p("duel_counter_ms", i(0, 60000, json!(3000), "Rules 0.8.1: a human idle this long is countered automatically."));
    p("duel_min_gap_ms", i(0, 5000, json!(500), "Rules 0.8.1: a blow sooner than this after the same side's last is a penalty."));
    p("duel_followup_ms", i(0, 10000, json!(1500), "Rules 0.8.1: a blow within this of the same side's last is a quick follow-up (+1)."));
    p("duel_deadline_ms", i(1000, 600000, json!(30000), "Rules 0.8.1: the longest a duel runs after its lock (the engine hold's deadline; without a hold the world keeps beating and the duel lasts at most ceil(duel_deadline_ms / flow_ms) resolves)."));
    p(
        "memory_per_owner",
        i(
            4,
            1000,
            json!(40),
            "Live memories per owner before old ones fold into a summary.",
        ),
    );
    p(
        "llm_folds_per_beat",
        i(
            0,
            32,
            json!(2),
            "Folds written by the fold mind per beat (the rest by rules).",
        ),
    );
    p(
        "max_intents_per_hand",
        i(
            1,
            12,
            json!(6),
            "Acts one hand turns its speech into per beat (orders sent in the beat they open a window).",
        ),
    );
    p(
        "max_memorize_per_beat",
        i(
            0,
            64,
            json!(6),
            "People memorized from player mentions per beat.",
        ),
    );
    p(
        "max_witnesses",
        i(1, 64, json!(4), "Witnesses recorded per act."),
    );
    p("compose_seconds", timer(600.0, "Compose window (the core's timer, fixed for the room). Default flow_ms / 1000; set it explicitly and it no longer follows flow_ms."));
    p(
        "choose_seconds",
        timer(
            600.0,
            "Unused by Final Night 7 (there is no Choose phase); copied through if set.",
        ),
    );
    p(
        "resolve_timeout_s",
        timer(1800.0, "The core's resolve timeout. Default 60."),
    );
    p(
        "allow_midmatch_join",
        json!({ "type": "boolean", "default": false,
                "description": "The core's room flag: humans may join a running match, taking over a bot's seat (its demigod) or a new seat (a live built-in AI demigod, the healthiest; none left: they spectate) from the next round. Copied through only if set." }),
    );
    json!({
        "type": "object",
        "description": "Room settings (`room.create.settings`, `room.settings.settings`; all optional) and, once validated, \
                        `match.started.rules` / `Room.settings`. Integers may be sent as whole floats (3.0). Unknown keys are \
                        dropped silently. Validated `rules` always contain every key except seed/city/allow_midmatch_join (when unset), rounds and \
                        choose_seconds (when unset).",
        "properties": props,
        "additionalProperties": true,
    })
}

fn action_fields_schema() -> Value {
    let level = json!({ "type": "string", "enum": ["world", "map", "planet", "globe", "district", "region", "city", "character", "person"],
                        "description": "Case-insensitive; map/planet/globe = world, region = district, person = character." });
    let target = json!({
        "description": "Where: a ref (\"dg:2\", any that exists), [lon, lat], \"lon,lat\", or a name (a demigod, region, city, crowd or person).",
        "anyOf": [{ "$ref": "#/$defs/LonLat" }, { "type": "string", "maxLength": 200 }] });
    let cast = json!({
        "type": "object",
        "description": "One god power: a slug from round.brief.public.powers, or \"custom\" with `text`.",
        "properties": {
            "power": { "type": "string", "maxLength": 40, "description": "Palette slug or \"custom\". Optional in `bolt`/`bolts` (lightning)." },
            "target": target.clone(),
            "text": { "type": ["string", "null"], "maxLength": 200, "description": "custom only: what the power is (required for custom)." },
            "magnitude": { "type": ["number", "null"], "minimum": 0, "maximum": 10, "description": "custom only: 0..10 (default 5); prices it (1 + magnitude/3 bolts, at most 4) before rules 0.7.0." },
            "tier": { "type": ["string", "null"], "enum": ["mega", "mini", "god", null], "description": "Rules 0.7.0: mega (a lightning bolt, map scale, every other god gains a bolt) or mini (a spark, the board). Default mini, so old clients never burn bolts. `scale` is accepted as an alias. Rules 0.8.0: god (a god-tier power `god_<slug>`: god_power_cost bolts, resolved as that god's ruling)." },
            "scale": { "type": ["string", "null"], "enum": ["mega", "mini", "god", null], "description": "Alias of tier." }
        },
        "required": ["target"]
    });
    let bolt = json!({ "description": "A lightning bolt: {target} or the bare target.", "anyOf": [cast.clone(), target.clone()] });
    let hand = |side: &str| {
        json!({ "type": ["string", "null"], "maxLength": 2000,
                "description": format!("What you say to your {side} hand this beat (<= 2000 chars). Empty = that hand does nothing (no LLM call). Newlines separate commands.") })
    };
    json!({
        "type": "object",
        "description": "`action.submit.fields`. Text entry (orders) goes into a slow window (`action.begin`, then \
                        `action.submit { window_id }`) or Compose; casts go as instant actions. If neither `left` nor \
                        `right` is present (or both are null), `text` is read instead: lines starting `l:`/`left:` go to \
                        the left hand, `r:`/`right:` to the right, other lines to the left. Remember the core caps `text` \
                        at 500 chars, so put long speech in the fields. The v0 keys `slow`, `sent_at`, `window` (and a \
                        `/slow` line) are refused.",
        "properties": {
            "left": hand("left"),
            "right": hand("right"),
            "cast": { "anyOf": [cast.clone(), { "type": "null" }], "description": "A god power to cast. Send casts as an instant action (`instant: true`, any phase) with no orders in the same action (casts never carry orders); `validate_action` refuses casts you can't afford at the last commit." },
            "casts": { "type": ["array", "null"], "items": cast, "description": "Several casts (at most 5 per action, with `cast`/`bolt`/`bolts`)." },
            "bolt": { "anyOf": [bolt.clone(), { "type": "null" }], "description": "Alias: cast lightning at a target." },
            "bolts": { "type": ["array", "null"], "items": bolt, "description": "Alias: several lightning bolts." },
            "summon": { "anyOf": [{ "type": "null" }, {
                "type": "object",
                "description": "Rules 0.8.0: summon one of your demigods there, this beat (an instant action, any phase).",
                "properties": {
                    "demigod": { "type": "string", "pattern": "^dg:\\d+$", "description": "One of your two demigods." },
                    "target": { "description": "[lon, lat] or a hex cell key.", "anyOf": [{ "$ref": "#/$defs/LonLat" }, { "type": "string" }] }
                },
                "required": ["demigod", "target"]
            }] },
            "challenge": { "anyOf": [{ "type": "null" }, {
                "type": "object",
                "description": "Rules 0.8.1: challenge a rival's demigod in reach to a duel (an instant action that travels alone; forced). Refusals: not_yours, fallen, bad_target, too_far:<distance km>:<allowed km>, busy, cooldown, challenge_disabled.",
                "properties": {
                    "demigod": { "type": "string", "pattern": "^dg:\\d+$", "description": "One of your standing demigods." },
                    "target": { "type": "string", "pattern": "^dg:\\d+$", "description": "A rival's standing demigod." },
                    "level": { "type": "string", "enum": ["regions", "villages", "houses", "people"], "description": "The board level it was issued at (missing or unknown = regions): the reach is max(challenge_min_km 120, the touching-cards distance at this level: regions 600, villages 15, houses 0.15, people 0.015 km); it also frames the fight (the lock's cell)." }
                },
                "required": ["demigod", "target"]
            }] },
            "duel_tick": { "anyOf": [{ "type": "null" }, { "type": "object", "required": ["duel_id"], "properties": { "duel_id": { "type": "string" } },
                "description": "Rules 0.8.2 (instant; travels alone): a duellist's client says the current panel's input window closed, so the next panel is computed now. Refusals: panels_off, no_duel, not_yours." }] },
            "duel": { "anyOf": [{ "type": "null" }, {
                "type": "object",
                "description": "Rules 0.8.1: one line typed in a duel, resolved at once (`duel.blow`). Refusals: no_duel, duel_over, not_yours, fallen, empty, too_long.",
                "properties": {
                    "duel_id": { "type": "string", "description": "From challenge.lock." },
                    "text": { "type": "string", "maxLength": 120 }
                },
                "required": ["duel_id", "text"]
            }] },
            "zoom": {
                "description": "Optional view change, applied at the start of Resolve: a level string, or {level, focus}. \
                                Null/empty = no change. A level without focus clears your focus.",
                "anyOf": [
                    { "type": "null" },
                    { "type": "string", "maxLength": 0 },
                    level.clone(),
                    {
                        "type": "object",
                        "properties": {
                            "level": level,
                            "focus": { "type": ["string", "null"], "maxLength": 200,
                                       "description": "A ref (\"ch:12\", any that exists) or the exact name of something visible at district zoom. Unresolvable = no focus (no error)." }
                        },
                        "required": ["level"]
                    }
                ]
            }
        },
        "additionalProperties": true,
    })
}

fn round_started_public() -> Value {
    object(
        "`round.started.public`: the beat's Start (Time's clock, debts coming due), before anyone acts.",
        &[
            ("beat", int("This beat (= round).")),
            ("beats_total", int("The Final Night's beat.")),
            ("final_night", boolean("True on the last beat: every debt is due and exactly one demigod survives.")),
            ("arena_radius_km", num("Safe radius after this beat's shrink.")),
            ("caught_outside", array(json!({ "type": "string" }), "Names of demigods/possessed bodies hurt for being outside the arena.")),
            ("debts_due", int("Number of debt collections queued for this beat (all demigods).")),
            ("eliminated", array(json!({ "type": "string" }), "Names of demigods that fell during Start.")),
            ("alive", int("Demigods still standing.")),
        ],
        &[("players_alive", int("Rules 0.7.0: players with a demigod standing.")),
          ("challenges", array(r("ChallengeLock"), "Rules 0.8.1: the built-in AI's challenges this beat (absent when none): fight back with fields.duel.")),
          ("duel_clock", r("DuelClock"))],
    )
}

fn round_started_private() -> Value {
    object(
        "`round.started.private`: your demigod after Start.",
        &[
            ("beat", int("This beat.")),
            ("health", num("Your health after Start.")),
            ("alive", boolean("False = you were eliminated (you are now a spectator).")),
            ("outside_arena", boolean("You are outside the safe radius: move inward or keep taking damage each beat.")),
            ("debt_due", num("Damage points Consequence collects from you this beat (resolved in this beat's merge; a Time delay can push it back).")),
        ],
        &[
            ("demigod", json!({ "$ref": "#/$defs/Ref", "description": "Rules 0.7.0: the lead demigod the top-level fields describe." })),
            ("demigods", array(object("", &[("ref", r("Ref")), ("side", r("Hand")), ("health", num("")), ("alive", boolean("")),
                ("outside_arena", boolean("")), ("debt_due", num(""))], &[]), "Rules 0.7.0: each of your two demigods.")),
        ],
    )
}

fn brief_public() -> Value {
    object(
        "`round.brief.public`.",
        &[
            ("title", string("\"Beat <n> of <total>\" (\"The Final Wish: beat 1 of <total>\" on the Final Wish).")),
            ("beat", int("This beat.")),
            ("beats_total", int("The Final Night's beat.")),
            ("final_night", boolean("True on the last beat.")),
            ("arena", r("Arena")),
            ("alive", int("Demigods still standing.")),
            (
                "rules",
                string("A one-paragraph reminder of the action format (for LLM players)."),
            ),
            ("gods", array(object(
                "One god of the pantheon.",
                &[
                    ("ref", json!({ "$ref": "#/$defs/Ref", "description": "`gd:<n>`; n is the index in the pantheon." })),
                    ("name", string("Display name (\"Time\").")),
                    ("slug", string("Stable id word (\"time\").")),
                    ("domain", string("What the god rules, in a phrase (for tooltips).")),
                ],
                &[],
            ), "The god table, in id order (gd:0 Time ... gd:6 Entropy).")),
            ("arena_kind", json!({ "type": "string", "enum": ["planet", "city"] })),
            ("powers", array(object(
                "One power of the palette (data/powers.json), or the custom entry.",
                &[
                    ("slug", string("Its id (send as `cast.power`).")),
                    ("name", string("Display name.")),
                    ("icon", string("Icon key (= slug).")),
                    ("cost", json!({ "type": ["integer", "null"], "minimum": 0, "description": "Bolts; null for custom (priced by magnitude)." })),
                    ("radius_km", num("Area it affects.")),
                    ("target", json!({ "type": "string", "enum": ["point", "region", "unit"], "description": "What it is aimed at." })),
                    ("magnitude", json!({ "type": ["integer", "null"], "minimum": 0, "maximum": 10 })),
                    ("damage", json!({ "type": ["number", "null"], "description": "Direct damage to a body it hits." })),
                    ("tags", array(json!({ "type": "string" }), "")),
                ],
                &[
                    ("mega", object("Rules 0.7.0: the MEGA variant (map scale; send `cast.tier: \"mega\"`).", &[("radius_km", num("")), ("magnitude", int("")), ("damage", num("Its target's damage; rival demigods within the radius take 40%.")), ("cost", int("1 bolt."))], &[])),
                    ("mini", object("Rules 0.7.0: the MINI variant (the board; `cast.tier: \"mini\"`, the default).", &[("radius_km", num("")), ("magnitude", int("")), ("damage", num("")), ("cost", int("1 spark."))], &[])),
                    ("tier", json!({ "const": "god", "description": "Rules 0.8.0: a god-tier power (cast with `tier: \"god\"`; `cost` in bolts)." })),
                    ("god", string("Rules 0.8.0: the god's slug.")),
                    ("god_ref", nullable(r("Ref"))),
                    ("blurb", string("Rules 0.8.0: what it does.")),
                ],
            ), "The power palette, then the custom entry.")),
            ("entropy", r("Entropy")),
            ("flow", object(
                "Flow and slow timing.",
                &[
                    ("flow_ms", int("Flow beat length (the room's Compose window).")),
                    ("slow_ms", int("Longest a slow window stays open.")),
                    ("slow", nullable(r("SlowState"))),
                ],
                &[],
            )),
            ("regions", array(object(
                "A region (planet) or district (city) and its cities (sites).",
                &[
                    ("ref", r("Ref")),
                    ("name", string("")),
                    ("center", r("LonLat")),
                    ("radius_km", num("")),
                    ("cities", array(object("", &[("ref", r("Ref")), ("name", string("")), ("center", r("LonLat"))], &[]), "")),
                ],
                &[],
            ), "Every region, in id order.")),
        ],
        &[("final_wish", boolean("Rules 0.7.0: this beat is THE FINAL WISH (beat 1): your orders are your final wish; both hands get the window act budget and +final_wish_magnitude.")),
          ("time_of_day", r("TimeOfDay")),
          ("time_of_day_phase", r("DayPhase"))],
    )
}

fn seat_view_body(desc: &str) -> Value {
    object(
        desc,
        &[
            ("seat", int("Your seat.")),
            ("demigod", json!({ "$ref": "#/$defs/Ref", "description": "Your demigod (`dg:<n>`)." })),
            ("name", string("Your demigod's name (your display name, de-duplicated).")),
            ("alive", boolean("False = eliminated; you keep receiving this view as a spectator.")),
            ("beat", int("The last beat started (in the brief: the coming beat).")),
            ("you", r("YouLine")),
            ("max_health", num("Your health cap (10, lowered by Time's Life prices).")),
            ("zoom", r("Zoom")),
            ("focus", nullable(json!({ "$ref": "#/$defs/Entity", "description": "What you are focused on (an Entity object, as in round.private.focus)." }))),
            ("visible", array(r("Entity"), "What you see at your zoom (<= 60): yourself, live rivals, your items, then the district (or world) contents, nearby districts, possessed bodies.")),
            ("hands", array(r("SeatHand"), "Your two hands: [left, right].")),
            ("items", array(r("Entity"), "Items your demigod holds.")),
            ("debts", array(r("DebtLine"), "Your open debts.")),
            ("news", array(r("NewsLine"), "Ledger rows you know from the last ~3 beats, newest first, one line per row (<= 20).")),
            ("armies", array(r("Army"), "Your live armies (with their orders).")),
            ("orders", array(object("A standing order (your hands keep carrying it out, by rules, while you're silent).",
                &[("hand", r("Hand")), ("text", string("Your words it came from.")), ("since", int("Beat given."))], &[]),
                "Your hands' standing orders.")),
            ("slow", json!({ "anyOf": [{ "type": "null" }, object("The open slow window, as it concerns you.",
                &[("yours", boolean("You opened it.")), ("window_id", int("The Theatre's window id."))], &[])],
                "description": "null when no window is open." })),
        ],
        &[
            ("player", string("Rules 0.7.0: your name (both of your demigods are yours).")),
            ("aura", string("Rules 0.7.0: your aura colour, `#RRGGBB`.")),
            ("time_bill", r("TimeBill")),
            ("charges", r("Charges")),
            ("demigods", array(json!({ "type": "object", "additionalProperties": true,
                "description": "Rules 0.7.0: one of your two demigods in full: the public fields plus health, max_health, debt_open, items (Item), hand (SeatHand), orders, manifests (Manifestation, with verdicts)." }),
                "Rules 0.7.0: both of your demigods, [left, right].")),
            ("my_items", array(r("Item"), "Rules 0.7.0: every item your demigods hold.")),
            ("hierarchy", json!({ "type": ["object", "null"], "additionalProperties": true, "description": "The society hierarchy at your zoom (schema 4: node, path, children, crowds, cells, people, armies with chains of command, mobs, battles). Its shape is not frozen yet (the society build is in progress); read defensively." }))],
    )
}

fn outcome_public() -> Value {
    object(
        "`round.outcome.public`: what everyone sees after the beat resolves. The narrator runs one beat late (off the \
         critical path), so the narration here describes beat `caption_beat` (the previous beat), not `beat`.",
        &[
            ("beat", int("The beat that resolved.")),
            ("beats_total", int("The Final Night's beat.")),
            ("previous_caption", string("The narrator's caption for beat `caption_beat` (\"\" on the first beat). Template text when the LLM is off or late.")),
            ("previous_callouts", array(json!({ "type": "string" }), "0-3 short asides for beat `caption_beat` (god quips, kill lines). Without the LLM these repeat the omen speeches.")),
            ("caption_beat", json!({ "type": ["integer", "null"], "minimum": 0,
                                    "description": "The beat `previous_caption` / `previous_callouts` describe (beat - 1); null on the first beat." })),
            ("caption", string("Same text as `previous_caption` (kept for older clients).")),
            ("callouts", array(json!({ "type": "string" }), "Same as `previous_callouts` (kept for older clients).")),
            ("eliminated", array(json!({ "type": "string" }), "Names of demigods that fell during this beat's resolve.")),
            ("alive", int("Demigods still standing.")),
            ("arena_radius_km", num("Current safe radius.")),
            ("omens", array(r("Omen"), "Public god rulings this beat (magnitude >= 6, or one that sent a paladin; Entropy's yes to every cast of magnitude >= 6).")),
            ("entropy", r("Entropy")),
            ("slow", array(r("SlowEvent"), "Slow-time announcements this beat (open / closed / instant).")),
            ("casts", array(r("CastLine"), "Every cast this beat, in the order settled.")),
            ("events", array(json!({ "anyOf": [r("WorldEvent"), r("DmEvent")] }), "Public geotagged world events this beat, in order (<= 40). Rules 0.8.0: a summoning adds its `summoned` event here too (`kind` and `event` \"summoned\", `at` its landing; see DmEvent).")),
            ("timing", object(
                "Server-side wall-clock timings of the resolve (diagnostics).",
                &[
                    ("total_ms", num("Whole resolve.")),
                    ("stages", array(object("", &[("stage", r("Stage")), ("ms", num(""))], &[]), "Per stage, in order.")),
                    ("llm_calls", int("LLM minds called this beat.")),
                    ("llm_fallbacks", int("Of those, how many used their rules fallback.")),
                ],
                &[
                    ("society_ms", num("The society step's wall time.")),
                    ("society_events", int("Society events this beat.")),
                    ("society_emerged", int("People who emerged from the simulation this beat.")),
                    ("society_dropped", int("Society events dropped from the payload.")),
                ],
            )),
        ],
        &[
            ("society", json!({ "type": ["object", "null"], "additionalProperties": true, "description": "The society's public summary (schema 4). Its shape is not frozen yet (the society build is in progress); read defensively." })),
            ("players_alive", int("Rules 0.7.0: players with a demigod standing.")),
            ("manifests", array(r("Manifestation"), "Rules 0.7.0: public manifestations this beat (made things of tier relic or up, or legend+ attempts).")),
            ("dice", array(r("DiceRolled"), "Rules 0.8.0: every d20 the DM rolled this beat (at most 60), in order.")),
            ("god_powers", array(r("GodPowerLine"), "Rules 0.8.0: god-tier powers settled this beat (each then ruled on by its god: see `omens`).")),
            ("duels", array(r("ChallengeEnd"), "Rules 0.8.1: the duels that ended this beat (absent when none).")),
            ("duels_open", array(r("ChallengeOpen"), "Rules 0.8.1: duels still under way (absent when none).")),
            ("summons", array(r("DmEvent"), "Rules 0.8.0: the `summoned` events of this beat (also in dm_events): {demigod, from {pos, cell}, to {pos, cell}, deed, health_lost}.")),
            ("dm_events", array(r("DmEvent"), "Rules 0.8.0: the DM's public decisions this beat (at most 40): settlements growing, encounters, loot, roads, shrines, scars, the time of day.")),
            ("time_of_day", r("TimeOfDay")),
            ("time_of_day_phase", r("DayPhase")),
        ],
    )
}

fn public_view() -> Value {
    let demigod = json!({
        "type": "object",
        "description": "A demigod as everyone sees it. Positions of live demigods are public; health is not.",
        "properties": {
            "ref": { "$ref": "#/$defs/Ref" },
            "name": { "type": "string" },
            "alive": { "type": "boolean" },
            "seat": { "type": ["integer", "null"], "minimum": 0, "description": "The Theatre seat; null for the built-in AI's demigods. A mid-match joiner's new seat takes over a built-in AI demigod, whose seat becomes that seat." },
            "controller": { "type": "string", "enum": ["human", "ai"], "description": "human: a Theatre seat whose player is human plays it (as the game knows: from match start and mid-match takeovers); ai: a Theatre bot seat or the built-in AI. Flips to human when a joiner takes it over. A human who leaves mid-match stays human here until someone takes the seat over: room.updated seats[].is_bot is the live truth." },
            "connected": { "type": ["boolean", "null"], "description": "true for an AI-controlled demigod (always there); null for a human one: the game never sees presence, so read room.updated seats[seat].connected." },
            "persona": { "type": "object", "additionalProperties": false, "description": "Accounts (docs/ACCOUNTS.md): the character the seat's signed-in player wrote for this demigod, already moderated. Player text: show it as written, never interpret it. Absent for every demigod whose player wrote nothing (guests, bots, the built-in AI).",
                "properties": {
                    "personality": { "type": "string", "maxLength": 600 },
                    "style": { "type": "string", "maxLength": 200, "description": "Speaking style." },
                    "creed": { "type": "string", "maxLength": 200 },
                    "pronouns": { "type": "string", "enum": ["she", "he", "they", "it"], "description": "The player's pronouns from their settings; absent when unspecified." } } },
            "pos": { "$ref": "#/$defs/LonLat", "description": "Alive only." },
            "place": { "type": "string", "description": "Alive only: the place name it is in." },
            "eliminated_beat": { "type": ["integer", "null"], "minimum": 0, "description": "Eliminated only." },
            "cause": { "type": ["string", "null"], "description": "Eliminated only: how it fell." },
            "bolts": { "type": "integer", "minimum": 0, "description": "Lightning bolts it holds (public)." },
            "region": { "type": "string", "description": "Alive only: the region (planet) or district (city) it is in." },
            "portrait_prompt": { "type": "string", "description": "An image prompt for its portrait (subject only, with its visual sheet; stable all match)." },
            "image_kind": { "type": "string", "enum": ["portrait"], "description": "The kind of portrait_prompt." },
            "diorama_prompt": { "type": "string", "description": "An image prompt for its diorama: the demigod as a tiny geometric miniature on a hex diorama tile with its signature prop (subject only: its name and visual sheet, identity only, the same in every match)." },
            "diorama_image_kind": { "type": "string", "enum": ["diorama"], "description": "The kind of diorama_prompt." },
            "visual": { "$ref": "#/$defs/Visual", "description": "Its visual sheet (stable all match). Always present since the media release; absent in older recordings." },
            "portrait_image": { "$ref": "#/$defs/ImageObject", "description": "Its portrait (images on only)." },
            "diorama_image": { "$ref": "#/$defs/ImageObject", "description": "Its diorama, the table minifig (images on only; kind diorama)." },
            "cell": { "type": "string", "description": "Alive only: its people-level hex cell (`<face>:people:<q>:<r>`, the client's own indexing; rules 0.7.0)." },
            "side": { "type": ["string", "null"], "enum": ["left", "right", null], "description": "Rules 0.7.0: which of its player's two hands it is." },
            "player": { "type": "string", "description": "Rules 0.7.0: its player's name." },
            "aura": { "type": "string", "description": "Rules 0.7.0: its player's aura colour (#RRGGBB)." },
            "sibling": { "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "Rules 0.7.0: its player's other demigod." },
            "items": { "type": "array", "items": { "$ref": "#/$defs/Item" }, "description": "Rules 0.7.0: its public item pieces (manifested, tier relic or up, or deployed)." },
            "sparks": { "type": "integer", "minimum": 0, "description": "Rules 0.7.0: its player's sparks (the mini-power currency; public, like bolts: hoarding is visible)." },
            "health": { "type": "number", "description": "Your own demigods only (seat_view.demigods[]); never a rival's." },
            "max_health": { "type": "number" },
            "debt_open": { "type": "number" },
            "hand": { "$ref": "#/$defs/SeatHand" },
            "orders": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
            "manifests": { "type": "array", "items": { "$ref": "#/$defs/Manifestation" } },
            "story": { "$ref": "#/$defs/Story", "description": "Rules 0.8.0: what its piece carries (followers, items, wardrobe, scars)." }
        },
        "required": ["ref", "name", "alive", "seat"],
        "oneOf": [
            { "properties": { "alive": { "const": true } }, "required": ["pos", "place"] },
            { "properties": { "alive": { "const": false } }, "required": ["eliminated_beat", "cause"] }
        ]
    });
    object(
        "`round.committed.public_state` and `match.snapshot.public_state`: the public view after each commit.",
        &[
            ("beat", int("Last beat started.")),
            ("beats_total", int("The Final Night's beat.")),
            ("arena", r("Arena")),
            ("alive", int("Demigods still standing.")),
            ("demigods", array(demigod, "Every demigod in the match, seats and built-in AI, in id order (dg:0, dg:1, ...). Rules 0.7.0: two per player, the Left hands first (dg:0..n), then the Right (dg:n..2n).")),
            ("omens", array(r("Omen"), "The last resolved beat's public rulings.")),
            ("caption", string("The latest caption written (narrated one beat late, so usually the beat before `beat`).")),
            ("caption_beat", json!({ "type": ["integer", "null"], "minimum": 0,
                                    "description": "The beat `caption` describes; null before the first caption." })),
            ("entropy", r("Entropy")),
            ("slow", nullable(r("SlowState"))),
            ("influence", array(r("Influence"), "Each demigod's share of each region.")),
            ("armies", array(r("Army"), "Every live army.")),
            ("mods", array(r("WorldMod"), "Every live world mod.")),
            ("gods", array(object("", &[("ref", r("Ref")), ("name", string("")), ("health", num("0..10; high Entropy wounds gods; at 0 Entropy answers its invocations."))], &[]), "The gods' health.")),
        ],
        &[("players_alive", int("Rules 0.7.0: players with a demigod standing.")),
          ("players", array(r("Player"), "Rules 0.7.0: the seat list, one entry per player.")),
          ("society", json!({ "type": ["object", "null"], "additionalProperties": true, "description": "The society's public summary (schema 4). Its shape is not frozen yet (the society build is in progress); read defensively." })),
          ("shrines", array(r("Shrine"), "Rules 0.8.0: every shrine on the board.")),
          ("time_of_day", r("TimeOfDay")),
          ("time_of_day_phase", r("DayPhase")),
          ("rules", object("The rules this match runs on.", &[("version", string("The match's rules version (e.g. \"0.8.1\").")),
              ("state_schema", int("The match's state schema.")), ("server", string("The server's current rules version (new matches start on it)."))], &[])),
          ("present", r("Present"))],
    )
}

fn match_summary() -> Value {
    let opt_seat = json!({ "type": ["integer", "null"], "minimum": 0, "description": "Theatre seat; null for the built-in AI's demigods." });
    let champion = object(
        "",
        &[
            ("demigod", r("Ref")),
            ("name", string("")),
            ("seat", opt_seat.clone()),
            ("health", num("Health at the end.")),
        ],
        &[("side", r("Hand")), ("player", string("Rules 0.7.0: the champion player's name."))],
    );
    let standing = object(
        "One demigod's final place.",
        &[
            ("rank", int("1 = champion.")),
            ("demigod", r("Ref")),
            ("name", string("")),
            ("seat", opt_seat.clone()),
            ("alive", boolean("")),
            ("eliminated_beat", nullable(int(""))),
            ("cause", nullable(string("How it fell."))),
            ("score", num("Same formula as round.committed scores: 1e6 if alive + followers + 100 x total favor.")),
        ],
        &[
            ("player", string("Rules 0.7.0: standings are per player; the player's name.")),
            ("aura", string("Rules 0.7.0.")),
            ("demigods", array(json!({ "type": "object", "additionalProperties": true, "description": "{ref, demigod, side, name, alive, health, eliminated_beat, cause, cell, visual}" }), "Rules 0.7.0: each of the player's demigods.")),
        ],
    );
    let item = object(
        "",
        &[
            ("item", r("Ref")),
            ("name", string("")),
            ("origin", json!({ "type": "string", "enum": ["starting", "summoned", "found", "echoed", "granted"],
                               "description": "How it came to be: starting kit, summoned, found, echoed (a copy of `origin_ref`) or granted by the god `origin_ref`." })),
            ("origin_ref", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }],
                                   "description": "The source item (`it:<n>`) for echoed, the god (`gd:<n>`) for granted, else null." })),
        ],
        &[("kind", json!({ "type": "string" })), ("icon", json!({ "type": "string" })), ("tier", json!({ "type": "string" })),
          ("count", num("")), ("unit", string("")), ("state", json!({ "type": "string" })), ("image_prompt", json!({ "type": ["string", "null"] })),
          ("image", r("ImageObject"))],
    );
    let summon = object("", &[("item", r("Ref")), ("name", string(""))], &[]);
    let pile = object(
        "Everything one demigod held and summoned, its stats and an image prompt for its statue (the end screen).",
        &[
            ("demigod", r("Ref")),
            ("name", string("")),
            ("seat", opt_seat),
            ("alive", boolean("")),
            ("items", array(item, "Items it or its agents/bodies ever held.")),
            ("summons", array(summon, "Items it created.")),
            ("stats", object(
                "",
                &[
                    ("words", int("Words spoken to its hands.")),
                    ("acts", int("Ledger rows it (or its agents/bodies) acted in.")),
                    ("debt_paid", num("Debt collected or settled.")),
                    ("rulings", int("God rulings it invoked.")),
                    ("memorized_people", int("People it memorized.")),
                    ("subagents", int("Subagents and possessions it spawned.")),
                ],
                &[],
            )),
            ("statue_prompt", string("An image-generation prompt for its amiibo-style statue (with its visual sheet).")),
            ("image_kind", json!({ "type": "string", "enum": ["statue"] })),
            ("bolts", int("Bolts it held at the end.")),
            ("slows", int("Times it slowed time.")),
        ],
        &[("statue_image", json!({ "$ref": "#/$defs/ImageObject", "description": "Its statue (images on only)." }))],
    );
    let stats_keys = [
        "beats",
        "beats_total",
        "ledger_rows",
        "characters",
        "memorized",
        "agents",
        "rulings",
        "intents",
        "memories",
        "llm_calls",
        "llm_fallbacks",
        "eliminations",
        "casts",
        "slows",
        "armies",
        "mods",
    ];
    let stats: Vec<(&str, Value)> = stats_keys.iter().map(|k| (*k, int(""))).collect();
    object(
        "`match.ended.summary`.",
        &[
            ("champions", array(champion, "The survivor (one entry; if nobody stands, the last to fall).")),
            ("champion_seat", json!({ "type": ["integer", "null"], "minimum": 0,
                                      "description": "The champion's Theatre seat; null when a built-in AI demigod won (then match.ended.winners is [])." })),
            ("standings", array(standing, "Every demigod, best first.")),
            ("piles", array(pile, "One per demigod, in id order.")),
            ("final_cut", object(
                "The match's story.",
                &[
                    ("title", string("")),
                    ("story", string("Narrator's final cut (<= 250 words), or the beat captions joined by newlines when fallback.")),
                    ("fallback", boolean("True = the captions, not an LLM story.")),
                ],
                &[],
            )),
            ("stats", object("Match totals.", &stats, &[])),
            ("entropy", r("Entropy")),
            ("heat_death", json!({ "type": ["integer", "null"], "minimum": 0, "description": "The beat heat death struck (the Final Night came early), or null." })),
        ],
        &[
            ("champion_player", json!({ "type": ["string", "null"], "description": "Rules 0.7.0: the surviving player." })),
            ("players", array(json!({ "type": "object", "additionalProperties": true,
                "description": "Rules 0.7.0 (for fn7-profile): {seat, display_name, demigod, rank, champion, alive, aura, controller, demigods: [{ref, side, name, alive, health, eliminated_beat, cause, cell, visual, rank, items, portrait_prompt, diorama_prompt}]}" }), "One per player, best first.")),
            ("manifests", array(r("Manifestation"), "Rules 0.7.0: every public manifestation of the match.")),
            ("storyboard", array(r("StoryboardShot"), "The final-cut animatic, in play order: the title, the biggest moments as they happened, the champion. Always present since the media release; absent in older recordings.")),
            (
            "last_caption",
            object(
                "The last beat's caption (narrated one beat late, so it is written at the end of the match, with the \
                 champion given to the narrator). Absent when the last beat saw nothing to narrate.",
                &[("beat", int("")), ("caption", string(""))],
                &[],
            ),
            ),
        ],
    )
}

/// Every schema, as (file stem, schema).
/// Rules 0.7.0 payload pieces: items, manifestations, the seat list, Time's live bill, hotspots, a demigod in full.
/// Rules 0.8.0: the DM rulebook features (docs/CLIENT_API.md "DM rulebook features (0.8.0)").
fn v08_defs() -> Map<String, Value> {
    let mut d = Map::new();
    let e = |vals: &[&str], desc: &str| json!({ "type": "string", "enum": vals, "description": desc });
    let sint = |desc: &str| json!({ "type": "integer", "description": desc });
    let verdicts = ["crit_fail", "fail", "success", "crit_success"];
    let kinds = ["monster", "treasure", "trap", "stranger", "undead", "ghost", "thief"];
    d.insert("DayPhase".into(), e(&["dawn", "day", "dusk", "night"], "Rules 0.8.0: the phase of the game day."));
    d.insert("TimeOfDay".into(), object(
        "Rules 0.8.0: the time of day (`beats_per_day` beats a day).",
        &[("day", int("Game day, from 1.")), ("phase", r("DayPhase")), ("light", num("0 (midnight) .. 1 (noon); 0.5 at dawn and dusk."))],
        &[("beat_in_day", int("1..beats_per_day.")), ("beats_per_day", int(""))],
    ));
    d.insert("DiceRolled".into(), object(
        "Rules 0.8.0: one d20 the DM rolled (seeded, recorded: the same question always gives the same roll).",
        &[
            ("event", json!({ "const": "dice.rolled" })),
            ("who", r("Ref")),
            ("die", json!({ "const": "d20" })),
            ("roll", json!({ "type": "integer", "minimum": 1, "maximum": 20, "description": "The face." })),
            ("modifier", sint("")),
            ("total", sint("roll + modifier.")),
            ("target_dc", sint("")),
            ("verdict", e(&verdicts, "A natural 1 is crit_fail, a natural 20 crit_success, else total vs DC.")),
            ("reason", string("What it was for, in public words (\"a strike\", \"a monster encounter\", \"manifesting a relic\").")),
            ("at", r("LonLat")),
            ("cell", string("People-level hex.")),
            ("beat", int("")),
        ],
        &[],
    ));
    d.insert("DmEvent".into(), json!({
        "type": "object", "additionalProperties": true,
        "description": "Rules 0.8.0: one public decision of the DM this beat: `event` (settlement.grew, settlement.shrank, settlement.building, encounter.marked, encounter.revealed, encounter.faded, loot.dropped, loot.taken, road.formed, road.faded, shrine.built, shrine.level, story.scar, story.wardrobe, story.followers, time.phase), `text` (one line), `memory` (its memory entry id) and the decision's own fields (docs/CLIENT_API.md).",
        "properties": { "event": { "type": "string" }, "text": { "type": "string" }, "memory": { "type": "integer", "minimum": 0 } },
        "required": ["event", "text", "memory"]
    }));
    d.insert("GodPowerLine".into(), object(
        "Rules 0.8.0: a god-tier power this beat. A fizzled one has `fizzled` and nothing was spent.",
        &[("by", string("")), ("by_ref", r("Ref")), ("power", string("god_<slug>")), ("power_name", string("")),
          ("tier", json!({ "const": "god" })), ("god", string("")), ("god_ref", r("Ref")), ("cost", int("Bolts.")), ("seq", int(""))],
        &[("at", r("LonLat")), ("target", nullable(r("Ref"))), ("place", string("")), ("region_ref", r("Ref")), ("fizzled", string(""))],
    ));
    d.insert("Summoning".into(), object(
        "Rules 0.8.0: `phase.event.public` the moment a summoning or a god-tier power is accepted (it lands at the next resolve).",
        &[("event", e(&["summoning", "god_power"], "")), ("by", string("")), ("by_ref", r("Ref")), ("by_seat", int("")),
          ("summons", array(object("", &[("demigod", r("Ref")), ("to", r("LonLat")), ("cell", string(""))], &[("roll", int("Rules 0.8.1: the mishap d20, decided at the accept.")), ("modifier", json!({ "type": "integer" })), ("total", json!({ "type": "integer" })), ("target_dc", json!({ "type": "integer" })), ("verdict", string("")), ("health_lost", num("Rules 0.8.1: what the resolve will take."))]), "")),
          ("god_powers", array(object("", &[("power", string("")), ("tier", json!({ "const": "god" }))], &[]), "")),
          ("seq", int("")), ("received_at_ms", int(""))],
        &[("demigod", nullable(r("Ref"))), ("from", nullable(r("LonLat"))), ("to", nullable(r("LonLat"))),
          ("roll", int("Rules 0.8.1: the first summoning's mishap d20 (the damage check comes before the teleport).")),
          ("modifier", json!({ "type": "integer" })), ("total", json!({ "type": "integer" })), ("target_dc", json!({ "type": "integer" })),
          ("verdict", string("")), ("health_lost", num("Rules 0.8.1: health the resolve will take."))],
    ));
    let side = object(
        "Rules 0.8.1: one duellist at the lock.",
        &[("demigod", r("Ref")), ("name", string("")), ("player", string("")), ("seat", nullable(int("null for the built-in AI."))),
          ("controller", json!({ "type": "string", "enum": ["human", "ai"] })), ("health", num("Health at the lock (both are shown: the duel's health bars).")),
          ("max_health", num("")), ("portrait_prompt", string("")), ("diorama_prompt", string(""))],
        &[("portrait_image", json!({ "type": "object" })), ("diorama_image", json!({ "type": "object" }))],
    );
    d.insert("ChallengeLock".into(), object(
        "Rules 0.8.1: `phase.event.public` when a challenge is accepted (or `round.started.public.challenges[]` for the AI's): both portraits lock in.",
        &[("event", json!({ "const": "challenge.lock" })), ("duel_id", string("duel:s<seq>:<a>:<b> (a player's challenge: its action's seq) or duel:<beat>:<a>:<b> (the AI's)")), ("a", side.clone()), ("b", side),
          ("at", r("LonLat")), ("cell", string("The hex at `level`.")), ("level", json!({ "type": "string", "enum": ["regions", "villages", "houses", "people"] })),
          ("focus", object("Centre the camera and spin the globe here.", &[("at", r("LonLat")), ("lat", num("")), ("lon", num("")), ("cell", string("")), ("level", string(""))], &[])),
          ("ends", json!({ "const": "ko_or_deadline" })),
          ("ends_beat", int("The last beat whose resolve may still carry it (the world keeps beating meanwhile).")),
          ("deadline_ms", nullable(int("Epoch ms: lines after it are refused (null for the AI's challenges).")))
          ,("world", json!({ "type": "string", "enum": ["beating", "held"], "description": "beating until the engine offers a hold window." })),
          ("freeze", object("The world hold the duel requests (supported once the engine has holds).",
                            &[("requested", boolean("")), ("supported", boolean("")), ("participants", array(int(""), "Human seats.")),
                              ("deadline_ms", nullable(int("Epoch ms.")))], &[])),
          ("beat", int("")), ("place", string("")), ("time_of_day", string("")), ("distance_km", num("")),
          ("by_seat", nullable(int("The challenger's seat; null for the AI."))), ("ai", boolean("")),
          ("leap", nullable(object("The AI challenger summoned itself next to its rival first (applied at the resolve).",
                                   &[("demigod", r("Ref")), ("to", r("LonLat")), ("health_lost", num(""))], &[])))],
        &[("seq", int("")), ("received_at_ms", int("")),
          ("also", array(json!({ "anyOf": [r("DuelScene"), r("DuelPanel")] }), "Rules 0.8.2: the duel's scene and panel 0, made with the lock. Handle each entry as its own event."))],
    ));
    let moves = json!({ "type": "string", "enum": ["lunge", "slash", "parry", "dodge", "leap", "cast", "summon_army", "charge", "grapple", "taunt", "stagger", "fall", "block", "blast", "vanish", "rise"] });
    let anim = object(
        "Rules 0.8.1: animation cues for the figures (deterministic, from the rules).",
        &[("a", moves.clone()), ("b", moves), ("fx", nullable(json!({ "type": "string", "enum": ["fire", "ice", "lightning", "earth", "water", "shadow", "light", "poison"] }))),
          ("camera", json!({ "type": "string", "enum": ["wide", "close", "over_a", "over_b"] })), ("army", boolean("")),
          ("attacker", json!({ "type": "string", "enum": ["a", "b"] }))],
        &[],
    );
    let strike_req = vec![
        ("by", r("Ref")), ("by_name", string("")), ("text", string("")), ("roll", int("1-20 (0 on a penalty).")), ("modifier", json!({ "type": "integer" })),
        ("total", json!({ "type": "integer" })), ("target_dc", json!({ "type": "integer" })),
        ("verdict", json!({ "type": "string", "enum": ["crit_fail", "fail", "success", "crit_success", "penalty"] })),
        ("damage", num("0-3, a crit 4 (× duel_damage_scale).")), ("hp", object("Both healths after it.", &[("a", num("")), ("b", num(""))], &[])),
        ("flavor", string("The DM's rules-templated line.")), ("wit", json!({ "type": "integer", "minimum": -3, "maximum": 4 })),
        ("tags", array(string(""), "What the wit saw: verb, combo, body, item, terrain, story, repeat, echo, short, spam, empty, rate_limit.")),
        ("ko", boolean("")), ("auto", boolean("An automatic blow (an AI counter or a duel nobody typed in).")), ("anim", anim),
    ];
    let pose = json!({ "type": "string", "enum": ["idle_guard", "step_in", "lunge_extend", "slash_high", "slash_low", "overhead_windup", "overhead_strike", "parry_high",
        "parry_low", "block_cross", "dodge_left", "dodge_right", "duck", "leap_up", "land", "feint_shoulder", "grab_reach",
        "grapple_lock", "throw_hip", "stagger_back", "stumble", "fall_back", "kneel_hit", "rise", "cast_raise", "cast_release",
        "command_point", "taunt_beckon", "victory", "defeat"] });
    let class = json!({ "type": ["string", "null"], "enum": ["strike", "heavy", "guard", "parry", "feint", "evade", "close", "retreat", "grapple", "throw", "invoke", "command", "taunt", null] });
    let stance = json!({ "type": "string", "enum": ["neutral", "guarding", "committed", "off_balance", "airborne", "downed"] });
    let range = json!({ "type": "string", "enum": ["far", "mid", "close", "grapple"] });
    let figure = object("One figure in an exchange.",
        &[("move_class", class), ("pose_keys", array(object("", &[("pose", pose), ("t_ms", int(""))], &[]), "Timed poses.")),
          ("contact", nullable(object("", &[("limb", string("right_hand | left_hand | right_foot")), ("target_limb", string("head | torso | arms | legs")), ("t_ms", int(""))], &[]))),
          ("stance_after", stance)], &[]);
    let choreo = object("Rules 0.8.1: the formal model's choreography of one strike (DM_RULEBOOK 21.4).",
        &[("range_before", range.clone()), ("range_after", range), ("initiative_before", json!({ "type": "string", "enum": ["a", "b"] })),
          ("initiative_after", json!({ "type": "string", "enum": ["a", "b"] })), ("flip", boolean("")),
          ("result", json!({ "type": "string", "enum": ["landed", "absorbed", "parried", "evaded", "missed", "thrown", "grappled", "wasted", "moved", "set", "taunted", "penalty"] })),
          ("a", figure.clone()), ("b", figure), ("camera", json!({ "type": "string", "enum": ["wide", "close", "over_a", "over_b"] })),
          ("contacts", array(object("", &[("by", json!({ "type": "string", "enum": ["a", "b"] })), ("limb", string("right_hand | left_hand | right_foot")),
                                           ("on", json!({ "type": "string", "enum": ["a", "b"] })), ("target_limb", string("head | torso | arms | legs")), ("t_ms", int(""))], &[]),
                             "Every contact: who hits, with which limb, which body part of the other, when (empty when nothing lands).")),
          ("duration_ms", json!({ "type": "integer", "minimum": 600, "maximum": 1400 }))], &[]);
    let strike_opt = vec![
        ("choreo", choreo),
        ("modifiers", object("", &[("wit", json!({ "type": "integer" })), ("health", json!({ "type": "integer" })), ("gear", json!({ "type": "integer" })),
                                   ("follow_up", json!({ "type": "integer" })), ("home", json!({ "type": "integer" })), ("situation", json!({ "type": "integer" }))], &[])),
        ("self_damage", num("A penalty's cost to its author.")),
    ];
    d.insert("DuelStrike".into(), object("Rules 0.8.1: one strike (a counter's shape).", &strike_req, &strike_opt));
    let mut blow_req = vec![("event", json!({ "const": "duel.blow" })), ("duel_id", string("")), ("n", int("1-based blow number."))];
    blow_req.extend(strike_req.iter().cloned());
    let mut blow_opt = strike_opt.clone();
    blow_opt.extend([("counter", r("DuelStrike")), ("hp_after", object("Healths after the counter.", &[("a", num("")), ("b", num(""))], &[])),
                     ("at_ms", json!({ "type": "integer" })), ("seq", int("")), ("received_at_ms", json!({ "type": "integer" })),
                     ("also", array(r("DuelPanel"), "Rules 0.8.2 (live event only): the panels this line's stamp made due. Handle each as its own event."))]);
    d.insert("DuelBlow".into(), object("Rules 0.8.1: `phase.event.public` for each duel line, at once (also in the resolve's challenge.open / challenge.end log).", &blow_req, &blow_opt));
    let ab = |desc: &str, v: Value| object(desc, &[("a", v.clone()), ("b", v)], &[]);
    let stance = json!({ "type": "string", "enum": ["neutral", "guarding", "committed", "off_balance", "airborne", "downed"] });
    d.insert("DuelClock".into(), object(
        "State schema 8: `round.started.public.duel_clock`, when the room stood still before this beat (it was paused with nobody connected, the server was down, or it is a fork): every open duel's clock stood still for it, and its deadline moved.",
        &[("event", json!({ "const": "duel.clock" })), ("beat", int("")), ("now_ms", json!({ "type": "integer", "description": "The server's clock at this Start." })),
          ("pause", nullable(object("The span the room stood still.", &[("from_ms", json!({ "type": "integer" })), ("to_ms", json!({ "type": "integer" })), ("ms", int(""))], &[]))),
          ("duels", array(object("", &[("duel_id", string("")), ("deadline_ms", nullable(json!({ "type": "integer" }))),
              ("pauses", array(json!({ "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2 }), "Every [from_ms, to_ms] its clock stood still."))], &[]), "Each open duel's new deadline."))],
        &[],
    ));
    d.insert("PresentDuel".into(), object(
        "State schema 8: one open duel in full, as it stands now (`present.duels[]`): enough to redraw it and seek to the exact offset.",
        &[("duel_id", string("")), ("a", json!({ "type": "object", "additionalProperties": true, "description": "The challenger, as in challenge.lock.a." })),
          ("b", json!({ "type": "object", "additionalProperties": true, "description": "The target, as in challenge.lock.b." })),
          ("lock", r("ChallengeLock")), ("hp", ab("Duel healths now.", num(""))),
          ("range", json!({ "type": "string", "enum": ["far", "mid", "close", "grapple"] })), ("initiative", json!({ "type": "string", "enum": ["a", "b"] })),
          ("stances", ab("", stance)), ("set", ab("The defence each side has set and not used.", json!({ "type": ["string", "null"] }))),
          ("blows", array(r("DuelBlow"), "Every blow so far, in order, as it was shown: the state's, then the ones since the last resolve. Each has `at_ms` and `seq` (automatic blows of an unattended duel have none).")),
          ("n", int("Blows numbered so far.")), ("typed", int("")), ("ko", json!({ "type": ["string", "null"], "enum": ["a", "b", null] })),
          ("opened", boolean("A resolve has applied its lock.")),
          ("started_ms", nullable(json!({ "type": "integer", "description": "The lock's server stamp (null: an AI's challenge in a state without the beat's clock)." }))),
          ("deadline_ms", nullable(json!({ "type": "integer", "description": "Includes every pause." }))),
          ("ends_beat", int("")), ("pauses", array(json!({ "type": "array", "items": { "type": "integer" } }), "[from_ms, to_ms] spans its clock stood still.")),
          ("clock", nullable(object("Its clock when the view was built (frozen at `present.paused_since_ms` while the room stands still).",
              &[("at_ms", json!({ "type": "integer" })), ("elapsed_ms", int("Duel time elapsed: the offset to seek to.")), ("remaining_ms", int("")), ("running", boolean(""))], &[]))),
          ("over", boolean("Knocked out or past its deadline: the next resolve ends it."))],
        &[("scene", nullable(r("DuelScene"))), ("panels", array(r("DuelPanel"), "Rules 0.8.2: every panel so far, in order."))],
    ));
    let hex = json!({ "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "A stage hex [q, r]: x = 1.732 (q + r/2), z = 1.5 r metres." });
    d.insert("DuelScene".into(), object(
        "Rules 0.8.2: `duel.scene`, the duel's stage dressing in the duel lab's scene shape (in the lock's `also[]`, and in `present.duels[].scene`). `source: rules` is the deterministic fallback; an authored scene will use the same shape.",
        &[("event", json!({ "const": "duel.scene" })), ("duel_id", string("")), ("source", json!({ "type": "string", "enum": ["rules", "llm"] })),
          ("biome", string("harbour | forest | desert | hills | town | wild")), ("place", string("")), ("time_of_day", string("")), ("cell", string("")), ("level", string("")),
          ("settlement_level", int("")), ("backstory", string("")),
          ("hex", json!({ "type": "object", "additionalProperties": true, "description": "The hex-to-metres rule." })), ("radius", int("Stage radius in hexes.")),
          ("a_start", hex.clone()), ("b_start", hex.clone()),
          ("objects", array(object("One object.", &[("id", string("Unique in the scene; paths and contacts name it.")), ("kind", string("pillar | wall | crate | table | boulder | rock | tree | log | jar | brazier | altar | chest | bones | cairn")),
              ("at", hex.clone()), ("yaw", num("Degrees.")), ("fixed", boolean("Built into the ground.")), ("note", string("One line (a cairn's: what the DM remembers here).")),
              ("affords", array(json!({ "type": "string", "enum": ["vault", "slide", "wallrun", "wallkick", "mantle"] }), "The paths that may name it.")),
              ("parts", array(json!({ "type": "object", "required": ["shape", "size", "offset", "material"], "additionalProperties": true, "properties": {
                  "shape": { "type": "string", "enum": ["box", "cylinder", "cone", "sphere", "tetra", "octa", "ico", "dodeca"] },
                  "size": { "type": "array", "items": { "type": "number" }, "description": "box [w,h,d]; cylinder, cone [r,h]; the others [r]. Metres." },
                  "offset": { "type": "array", "items": { "type": "number" }, "description": "The part's centre from the object's origin on the floor." },
                  "rot": { "type": "array", "items": { "type": "number" } }, "material": { "type": "string" }, "fill": { "type": "number" }, "glow": { "type": "number" } } }), "Primitive parts."))], &[]), ""))],
        &[],
    ));
    let seg = json!({ "type": "object", "required": ["clip", "ms"], "additionalProperties": true, "properties": {
        "clip": { "type": "string", "enum": fn7_plugin::panel::CLIPS }, "ms": { "type": "integer", "minimum": 0 }, "loop": { "type": "boolean" }, "speed": { "type": "number" },
        "to": hex.clone(), "path": { "type": "string", "enum": fn7_plugin::panel::PATHS }, "h": { "type": "number", "description": "Apex in metres." },
        "over": { "type": "string", "description": "vault: the object id." }, "under": { "type": "string", "description": "slide: the object id." },
        "on": { "type": "string", "description": "wallrun, wallkick, mantle: the object id." }, "face": { "type": "string", "enum": ["foe", "path"] },
        "spin": { "type": "boolean" }, "steps": { "type": "integer" }, "rise": { "type": "number" } } });
    let fighter = object("One fighter's track.", &[
        ("line", json!({ "type": ["string", "null"], "description": "The last line it typed in the window this panel stages (null: none)." })),
        ("line_masked_len", int("That line's length in characters (0: none).")), ("lines", int("Lines it typed in the window.")),
        ("move_class", json!({ "type": ["string", "null"], "description": "Its last move's class (strike, heavy, guard, parry, feint, evade, close, retreat, grapple, throw, invoke, command, taunt)." })),
        ("default", json!({ "type": ["string", "null"], "enum": ["guard", "circle", null], "description": "It did nothing in the window: the rules default it plays." })),
        ("auto", boolean("An automatic counter of its is staged here.")),
        ("segs", array(seg, "Clip segments in order; their `ms` sum to `play_ms`.")), ("at", hex.clone())], &[]);
    d.insert("DuelPanel".into(), object(
        "Rules 0.8.2: `duel.panel`, one comic panel of a duel: the presentation of the blows of one input window (they are decided by the 0.8.1 rules; a panel decides nothing). Sent in `also[]` of the action that made it due, as the event of a `duel_tick`, in `challenge.open` / `challenge.end`, and all of them in `present.duels[].panels`.",
        &[("event", json!({ "const": "duel.panel" })), ("duel_id", string("")), ("n", int("0-based.")),
          ("kind", json!({ "type": "string", "enum": ["intro", "exchange", "idle", "ko", "outro"] })),
          ("starts_at_ms", json!({ "type": "integer", "description": "Server epoch ms: the previous panel's end." })), ("play_ms", int("2400..3600.")),
          ("input_opens_ms", json!({ "type": ["integer", "null"] })),
          ("input_closes_ms", json!({ "type": ["integer", "null"], "description": "starts_at_ms + play_ms - 700 (the render lead); null: no input (a knockout, the last slot, the outro). Lines stamped up to it feed panel n + 1." })),
          ("caption", string("")), ("shot", json!({ "type": "string", "enum": fn7_plugin::panel::SHOTS })), ("a", fighter.clone()), ("b", fighter),
          ("contacts", array(json!({ "type": "object", "required": ["t_ms", "by", "limb", "kind", "on", "impulse", "text", "miss"], "additionalProperties": true, "properties": {
              "t_ms": { "type": "integer", "minimum": 0, "description": "From the panel's start." }, "by": { "type": "string", "enum": ["a", "b"] },
              "limb": { "type": "string", "enum": ["hand_l", "hand_r", "foot_l", "foot_r"] }, "kind": { "type": "string", "enum": ["hand", "kick", "heavy"] },
              "on": { "type": "string", "description": "`a` / `b` (the fighter hit) or a scene object's id." },
              "at": { "type": ["string", "null"], "enum": ["head", "chest", "belly", "legs", null], "description": "On a fighter: where." },
              "impulse": { "type": "number", "description": "0..1." }, "text": { "type": "string", "description": "The burst word." },
              "miss": { "type": "boolean" }, "ranged": { "type": "boolean" } } }), "")),
          ("blows", array(json!({ "type": "integer" }), "The `n` of each duel.blow it stages.")), ("flip", boolean("The initiative changed hands.")),
          ("hp", object("After the panel.", &[("a", num("")), ("b", num(""))], &[])), ("range", json!({ "type": "string", "enum": ["far", "mid", "close", "grapple"] })),
          ("initiative", json!({ "type": "string", "enum": ["a", "b"] }))],
        &[("winner", json!({ "type": ["string", "null"], "description": "Outro only." })), ("ko", json!({ "type": ["string", "null"], "description": "Outro only: the side knocked out." })),
          ("also", array(r("DuelPanel"), "Live event only: further panels made due by the same action.")), ("seq", int("")), ("received_at_ms", json!({ "type": "integer" }))],
    ));
    d.insert("Present".into(), object(
        "State schema 8 (exact resume): the present moment, in every public view (so in every match.snapshot) and in `game.present`. See docs/TIME_TRAVEL.md.",
        &[("server_time_ms", json!({ "type": "integer", "description": "The server's clock when this view was built." })), ("beat", int("")),
          ("live", boolean("This is the match's live state (false: a past state, shown as it was, or a fork before its first Start).")),
          ("mark", nullable(object("The newest instant action the state itself contains, and its clocks.", &[("at_ms", json!({ "type": "integer" })), ("seq", int("")),
              ("resolved_ms", json!({ "type": "integer" })), ("round_ms", json!({ "type": "integer" }))], &[]))),
          ("paused_since_ms", nullable(json!({ "type": "integer", "description": "The room stands still since then (nobody connected, or restored and not started)." }))),
          ("events", array(json!({ "type": "object", "additionalProperties": true, "required": ["seq", "received_at_ms", "beat"],
              "description": "A phase.event.public payload (cast streak, summoning, god_power, challenge.lock, duel.blow) with its `seq`, `received_at_ms` and `beat`." }),
              "The public events of the instant actions since the last resolve, in arrival order.")),
          ("duels", array(r("PresentDuel"), "Every open duel in full."))],
        &[],
    ));
    d.insert("ChallengeOpen".into(), object(
        "Rules 0.8.1: a duel still under way after this resolve (`round.outcome.public.duels_open[]`; the damage so far is applied).",
        &[("event", json!({ "const": "challenge.open" })), ("duel_id", string("")), ("a", r("Ref")), ("b", r("Ref")),
          ("hp", object("Both duel healths now.", &[("a", num("")), ("b", num(""))], &[])), ("blows", int("")), ("typed", int("")),
          ("ends_beat", int("")), ("deadline_ms", nullable(int(""))), ("range", json!({ "type": "string", "enum": ["far", "mid", "close", "grapple"] })),
          ("initiative", json!({ "type": "string", "enum": ["a", "b"] })), ("log", array(r("DuelBlow"), "The blows this resolve was handed."))],
        &[("panels", array(r("DuelPanel"), "Rules 0.8.2: the panels this resolve made due (some may have come live already: key them by `n`).")),
          ("panels_total", int("Rules 0.8.2: panels so far."))],
    ));
    d.insert("ChallengeEnd".into(), object(
        "Rules 0.8.1: a duel over, applied at the resolve (`round.outcome.public.duels[]`).",
        &[("event", json!({ "const": "challenge.end" })), ("duel_id", string("")),
          ("reason", json!({ "type": "string", "enum": ["ko", "deadline", "fallen", "auto"] })), ("locked_beat", int("")), ("a", r("Ref")), ("b", r("Ref")), ("a_name", string("")), ("b_name", string("")),
          ("winner", nullable(r("Ref"))), ("loser", nullable(r("Ref"))), ("ko", boolean("")),
          ("damage", object("Damage each took.", &[("a", num("")), ("b", num(""))], &[])), ("hp", object("", &[("a", num("")), ("b", num(""))], &[])),
          ("blows", int("Blows numbered (typed and automatic).")), ("typed", int("Lines typed.")), ("deed", string("One line: the duel mind's, else the rules'.")),
          ("at", r("LonLat")), ("pos", json!({ "$ref": "#/$defs/LonLat", "description": "`at` at full precision (outcome numbers are rounded to 2 decimals except `pos`)." })),
          ("cell", string("")), ("level", string("")), ("place", string("")), ("log", array(r("DuelBlow"), "The blows this final resolve was handed (earlier ones came live and in challenge.open)."))],
        &[("panels", array(r("DuelPanel"), "Rules 0.8.2: the rest of its panels, through the outro (`starts_at_ms` may lie ahead: play them out).")),
          ("panels_total", int("Rules 0.8.2: panels in all."))],
    ));
    d.insert("Tale".into(), object("Rules 0.8.0: a deed the DM told.", &[("beat", int("")), ("kind", string("")), ("text", string(""))], &[]));
    d.insert("Scar".into(), object("Rules 0.8.0: a scar.", &[("beat", int("")), ("cause", string("")), ("place", string(""))], &[("cell", string(""))]));
    d.insert("Story".into(), object(
        "Rules 0.8.0: what a demigod's piece carries.",
        &[
            ("followers", int("Its Following's headcount.")),
            ("items", array(object("", &[("ref", r("Ref")), ("icon", string(""))], &[]), "Items it holds now (pegs).")),
            ("wardrobe", object("Its outfit.", &[
                ("stage", json!({ "type": "integer", "minimum": 0, "maximum": 4 })),
                ("name", e(&["plain", "marked", "adorned", "regalia", "mythic"], "")),
                ("condition", json!({ "type": ["string", "null"], "enum": ["scorched", "ragged", null] }))], &[])),
            ("scars", array(r("Scar"), "Newest last, at most 12.")),
        ],
        &[("deeds", num("The wardrobe's input.")), ("tales", array(r("Tale"), "Rules 0.8.0: the newest 8 tales (summonings), oldest first."))],
    ));
    d.insert("Settlement".into(), object(
        "Rules 0.8.0: a settlement's growth.",
        &[("level", int("1 hamlet .. 5 great city; 6-7 legendary.")), ("size", int("People.")), ("prosperity", num("0..1")),
          ("target", int("The level the rules aim at.")), ("since_beat", int("")), ("center_cell", string("People-level hex at its centre.")),
          ("settlement_cells", array(json!({ "type": "string" }), "Village-level hexes it covers."))],
        &[],
    ));
    let outcome = object("Revealed only.", &[
        ("beat", int("")), ("by", r("Ref")), ("by_name", string("")), ("roll", int("")), ("modifier", sint("")),
        ("total", sint("")), ("target_dc", sint("")), ("verdict", e(&verdicts, "")), ("text", string("")), ("damage", num("")),
        ("item", nullable(r("Ref"))), ("item_icon", json!({ "type": ["string", "null"] })), ("character", nullable(r("Ref"))),
        ("sparks", sint(""))], &[]);
    d.insert("Encounter".into(), object(
        "Rules 0.8.0: a marked hex. Hidden: only `id`, `cell`, `marked`. Revealed: its type and outcome, forever.",
        &[("id", int("")), ("cell", string("People- or house-level hex.")), ("marked", json!({ "const": true }))],
        &[("revealed", boolean("")), ("type", e(&kinds, "")), ("night", boolean("Placed at night.")), ("at", r("LonLat")),
          ("region_ref", r("Ref")), ("outcome", outcome), ("faded_beat", int(""))],
    ));
    d.insert("LootToken".into(), object(
        "Rules 0.8.0: an item token lying on a hex.",
        &[("ref", r("Ref")), ("icon", string("")), ("cell", string("")), ("since_beat", int("")), ("pile", int("")), ("at", r("LonLat"))],
        &[],
    ));
    d.insert("Road".into(), object(
        "Rules 0.8.0: a road worn in between two settlements.",
        &[("from_cell", string("")), ("to_cell", string("")), ("wear", num("")), ("since_beat", nullable(int(""))),
          ("from_ref", r("Ref")), ("to_ref", r("Ref")), ("from_name", string("")), ("to_name", string("")),
          ("path", array(r("LonLat"), "The two settlement centres."))],
        &[],
    ));
    d.insert("Shrine".into(), object(
        "Rules 0.8.0: a shrine to a demigod.",
        &[("id", int("")), ("cell", string("House-level hex.")), ("owner_seat", nullable(int(""))),
          ("side", json!({ "type": ["string", "null"], "enum": ["left", "right", null] })), ("worshippers", int("")),
          ("power", e(&["spark", "heal", "ward"], "")), ("level", int("1..5")), ("owner", r("Ref")), ("owner_name", string("")),
          ("player", string("")), ("name", string("")), ("at", r("LonLat")), ("region_ref", r("Ref")), ("built_beat", int("")),
          ("owner_alive", boolean(""))],
        &[],
    ));
    d.insert("MemoryEntry".into(), object(
        "Rules 0.8.0: one decision in the DM's memory.",
        &[("id", int("")), ("beat", int("")), ("scope", e(&["cell", "settlement", "region", "world"], "")),
          ("place", string("A hex key, a `pl:` ref or \"world\".")), ("up", array(json!({ "type": "string" }), "Its ancestors, nearest first.")),
          ("kind", string("")), ("query", json!({ "type": ["string", "null"] })), ("text", string("")),
          ("data", json!({ "description": "The decision's fields (by kind)." }))],
        &[],
    ));
    d.insert("HexState".into(), object(
        "Rules 0.8.0: a hex as the DM knows it now (observed, or extrapolated from its last observation).",
        &[("cell", string("")), ("level", e(&["people", "houses", "villages", "regions"], "")), ("center", r("LonLat")),
          ("settlement", nullable(r("Ref"))), ("region", r("Ref")), ("people", int("")), ("buildings", int("")),
          ("parent_population", int("")), ("settlement_level", int("")), ("terrain", json!({ "type": ["string", "null"] })),
          ("encounters", array(r("Encounter"), "")), ("loot_items", int("")), ("shrines", array(json!({ "type": "integer" }), "Shrine ids.")),
          ("on_road", boolean("")), ("weather", string("")),
          ("time_of_day", object("", &[("day", int("")), ("phase", r("DayPhase")), ("light", num(""))], &[])),
          ("changes_since", array(json!({ "type": "string" }), "Its parent's notable changes since it was last observed."))],
        &[],
    ));
    d
}

fn v07_defs() -> Map<String, Value> {
    let mut d = Map::new();
    let e = |vals: &[&str], desc: &str| json!({ "type": "string", "enum": vals, "description": desc });
    let tiers = ["trinket", "artifact", "relic", "wonder", "legend", "myth", "titan", "apocalypse"];
    let kinds = ["artifact", "weapon", "armor", "relic", "structure", "army", "fleet", "creature", "phenomenon", "other"];
    let states = ["held", "deployed", "destroyed", "transferred", "stolen"];
    let outcomes = ["success", "twisted", "partial", "failed", "backfired", "refused"];
    d.insert("Item".into(), object(
        "Rules 0.7.0: an item (a manifested thing, or one an older path made).",
        &[
            ("ref", r("Ref")), ("name", string("")), ("description", string("")),
            ("kind", e(&kinds, "What it is.")),
            ("icon", e(fn7_model::manifest::ICONS, "The client's piece or icon.")),
            ("tier", e(&tiers, "Its scale tier.")),
            ("count", num("Soldiers, vessels, creatures or copies (rounded; f64 range: 100,000,000 is fine).")),
            ("unit", string("\"soldiers\", \"vessels\", \"creatures\" or \"\" (copies).")),
            ("state", e(&states, "")),
            ("holder", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }] })),
            ("holder_name", json!({ "type": ["string", "null"] })),
            ("created_beat", int("")),
            ("manifestation", json!({ "type": ["string", "null"], "description": "`mf:<n>`, the attempt that made it." })),
            ("at", nullable(r("LonLat"))),
            ("banner", json!({ "anyOf": [{ "type": "null" }, object("A host's banner.", &[("device", string("")), ("colors", array(json!({"type": "string"}), ""))], &[("motto", string(""))])] })),
            ("image_prompt", string("The item image subject (`image_kind` item).")),
            ("image_kind", json!({ "const": "item" })),
        ],
        &[("image", r("ImageObject")), ("origin", string("game.item only.")), ("created_by", json!({})), ("holders", array(r("Ref"), "")),
          ("history", array(json!({ "type": "object", "additionalProperties": true }), "Custody: {beat, state, holder?, by?, row?, count?}.")),
          ("rows", array(r("Ref"), "game.item only: the attempt's ledger rows.")),
          ("manifestation_detail", json!({}))],
    ));
    d.insert("Manifestation".into(), object(
        "Rules 0.7.0: one manifestation attempt (made or not).",
        &[
            ("ref", string("`mf:<n>`.")), ("beat", int("")), ("by", r("Ref")), ("by_name", string("")),
            ("side", json!({ "anyOf": [{ "$ref": "#/$defs/Hand" }, { "type": "null" }] })),
            ("kind", e(&kinds, "")), ("icon", e(fn7_model::manifest::ICONS, "")), ("tier", e(&tiers, "")),
            ("count", num("")), ("unit", string("")), ("outcome", e(&outcomes, "")),
            ("item", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }] })),
            ("name", json!({ "type": ["string", "null"] })), ("at", nullable(r("LonLat"))),
            ("region", json!({ "type": ["string", "null"] })), ("public", boolean("")),
            ("image_prompt", json!({ "type": ["string", "null"] })), ("image_kind", json!({ "const": "item" })),
        ],
        &[("verdict", json!({ "type": "object", "additionalProperties": true, "description": "Your own only: outcome, tier, odds {success, twist, partial, backfire, terms}, roll, cost {required, bolts, extra, missing, debt}, god, favor, entropy, reasons, flavor." })),
          ("request", json!({ "type": "object", "additionalProperties": true, "description": "Your own only: kind, count, magnitude." })),
          ("image", r("ImageObject"))],
    ));
    d.insert("TimeBill".into(), object(
        "Rules 0.7.0: Time's bill for a window the player opened now: debt(t) = min(debt_cap, debt_base + debt_per_second × t) plus one bolt.",
        &[("bolts", int("")), ("can_pay_bolt", boolean("")), ("recent_slows", int("")), ("repeat_factor", num("")),
          ("debt_base", num("")), ("debt_per_second", num("")), ("debt_per_10s", num("")), ("debt_cap", num("")),
          ("instant_order_debt", num("An order without a window (doubled when no bolt)."))],
        &[],
    ));
    d.insert("Player".into(), object(
        "Rules 0.7.0: one player (a seat, or a built-in AI) and both of its demigods.",
        &[("index", int("Stable player index (aura order).")), ("seat", json!({ "type": ["integer", "null"], "minimum": 0 })),
          ("player", string("")), ("aura", string("`#RRGGBB`, shared by both demigods.")), ("alive", boolean("Either demigod stands.")),
          ("bolts", int("The player's purse of lightning bolts (the mega currency; both demigods share it).")),
          ("sparks", int("The player's sparks (the mini currency).")),
          ("charges", r("Charges")),
          ("demigods", array(r("Ref"), "[left, right].")),
          ("time_bill", r("TimeBill")), ("controller", e(&["human", "ai"], "")),
          ("connected", json!({ "type": ["boolean", "null"] }))],
        &[],
    ));
    d.insert("Charges".into(), object(
        "Rules 0.7.0, the 1:5 economy: the player's power charges (public). A MEGA power costs a bolt (and every other player gains bolt_gift bolts, spark_gift sparks); a MINI costs a spark (no gifts). Bolts also pay Time's toll and gods' prices.",
        &[("bolts", int("")), ("sparks", int(""))],
        &[],
    ));
    d.insert("Hotspot".into(), object(
        "Rules 0.7.0: a region's most dynamic area (events, battles, mobs, unrest, demigods of the last 5 beats).",
        &[("pos", r("LonLat")), ("radius_km", num("")), ("score", num("Weight inside it.")),
          ("why", array(e(&["events", "battles", "mobs", "unrest", "demigods"], ""), "Heaviest first."))],
        &[],
    ));
    d
}

pub fn all() -> Vec<(&'static str, Value)> {
    let mut pool = model_defs();
    pool.extend(shared_defs());
    pool.extend(v07_defs());
    pool.extend(v08_defs());
    pool.extend(queries::defs());
    let mut round_private = pool["SeatDelta"].clone();
    if let Some(props) = round_private["properties"].as_object_mut() {
        props.insert("demigods".into(), array(object("Rules 0.7.0: one of your two demigods this beat.",
            &[("ref", r("Ref")), ("side", r("Hand")), ("name", string("")), ("health", num("")), ("max_health", num("")),
              ("alive", boolean("")), ("pos", r("LonLat")), ("cell", json!({ "type": ["string", "null"], "description": "People-level hex key." }))], &[]),
            "Rules 0.7.0: both of your demigods (`you` is your lead one: the first standing)."));
        props.insert("manifests".into(), array(r("Manifestation"), "Rules 0.7.0: your demigods' manifestations this beat, with the verdict."));
        props.insert("wills".into(), array(object("Rules 0.8.1: what became of an order you gave while all your demigods were fallen (never silence).",
            &[("text", string("Your order.")), ("routed_to", nullable(string("The god it went to (Death for a plea to come back)."))),
              ("verdict", json!({ "type": "string", "enum": ["grant", "bargain", "twist", "refuse", "refused"], "description": "The god's verdict, or refused (never reached a god)." })),
              ("speech", nullable(string("The god's words."))), ("reason", string("Why: Death's price and your purse, or why it can't be done."))], &[]),
            "Rules 0.8.1: your fallen demigods' wills this beat (absent when none)."));
    }
    round_private["description"] =
        "`round.private.private` (the `SeatDelta`): what only your seat learns from the beat that just resolved. \
         Sent to every human seat, spectators included."
            .into();
    let mut out = vec![
        ("settings", finish("settings", "Final Night 7 room settings", "room.create.settings, room.settings.settings, match.started.rules, Room.settings", settings_schema(), &pool)),
        ("action_fields", finish("action_fields", "Final Night 7 action fields", "action.submit.fields", action_fields_schema(), &pool)),
        ("round_started_public", finish("round_started_public", "round.started public", "round.started.public, match.snapshot.started", round_started_public(), &pool)),
        ("round_started_private", finish("round_started_private", "round.started private", "round.started.private, match.snapshot.you.started", round_started_private(), &pool)),
        ("brief_public", finish("brief_public", "round.brief public", "round.brief.public, match.snapshot.brief", brief_public(), &pool)),
        ("brief_private", finish("brief_private", "round.brief private", "round.brief.private, match.snapshot.you.brief", seat_view_body("`round.brief.private`: your seat view (same shape as seat_view) at the start of the beat, `beat` = the coming beat."), &pool)),
        ("outcome_public", finish("outcome_public", "round.outcome public", "round.outcome.public, match.snapshot.outcome", outcome_public(), &pool)),
        ("round_private", finish("round_private", "round.private private (SeatDelta)", "round.private.private, match.snapshot.you.outcome", round_private, &pool)),
        ("public_view", finish("public_view", "Final Night 7 public view", "round.committed.public_state, match.snapshot.public_state", public_view(), &pool)),
        ("seat_view", finish("seat_view", "Final Night 7 seat view", "round.private_state.state, match.snapshot.you.private_state", seat_view_body("`round.private_state.state`: everything your seat may see, after each commit (players and spectators)."), &pool)),
        ("match_summary", finish("match_summary", "match.ended summary", "match.ended.summary", match_summary(), &pool)),
        ("phase_event", finish("phase_event", "phase.event public", "phase.event.public (pushed at once: who slowed time, a cast's streak)",
            json!({ "description": "`phase.event.public`: an immediate push from the game. `event` says which.",
                    "oneOf": [r("Slowed"), r("Streak"), r("Summoning"), r("ChallengeLock"), r("DuelBlow"), r("DuelPanel")] }), &pool)),
    ];
    out.extend(query_schemas(&pool));
    out
}

/// `query_<name>`: each `game.<name>` reply schema, with the args schema the plugin publishes (`x-args`) and whether
/// it is served over HTTP (`x-public`).
fn query_schemas(pool: &Map<String, Value>) -> Vec<(&'static str, Value)> {
    let specs = fn7_plugin::queries::specs();
    queries::replies()
        .into_iter()
        .map(|(stem, name, body)| {
            let spec = specs.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no query {name}"));
            let applies = format!(
                "game.{name} reply (WebSocket){}",
                if spec.public { format!(", GET /api/rooms/{{code}}/query/{name}") } else { String::new() }
            );
            let mut s = finish(stem, &format!("game.{name}"), &applies, body, pool);
            s["x-public"] = spec.public.into();
            s["x-description"] = spec.description.clone().into();
            s["x-args"] = spec.args_schema.clone();
            (stem, s)
        })
        .collect()
}

/// Pretty JSON with a trailing newline (the on-disk form).
pub fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("json");
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------------------------------------------------
// A light validator: type, enum, const, properties/required, additionalProperties, items/prefixItems, min/maxItems,
// minimum/maximum/exclusiveMinimum, maxLength, pattern (the Ref pattern only), $ref, anyOf/oneOf/allOf. `strict` also
// reports object keys a schema doesn't declare (unless it says `additionalProperties: true`), so the docs stay complete.
// ---------------------------------------------------------------------------------------------------------------------

pub fn validate(schema: &Value, v: &Value, strict: bool) -> Vec<String> {
    let mut errs = vec![];
    check(schema, schema, v, "", strict, &mut errs);
    errs
}

fn type_ok(t: &str, v: &Value) -> bool {
    match t {
        "null" => v.is_null(),
        "boolean" => v.is_boolean(),
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64() || v.as_f64().is_some_and(|f| f.fract() == 0.0),
        "object" => v.is_object(),
        "array" => v.is_array(),
        _ => true,
    }
}

fn declared_keys(root: &Value, s: &Value, out: &mut Vec<String>) -> bool {
    let s = resolve(root, s);
    let mut open = matches!(
        s.get("additionalProperties"),
        Some(Value::Bool(true)) | Some(Value::Object(_))
    );
    if let Some(p) = s.get("properties").and_then(Value::as_object) {
        out.extend(p.keys().cloned());
    }
    for k in ["allOf", "anyOf", "oneOf"] {
        for b in s.get(k).and_then(Value::as_array).into_iter().flatten() {
            open |= declared_keys(root, b, out);
        }
    }
    open
}

fn resolve<'a>(root: &'a Value, s: &'a Value) -> &'a Value {
    match s.get("$ref").and_then(Value::as_str) {
        Some(p) => p
            .strip_prefix('#')
            .and_then(|ptr| root.pointer(ptr))
            .unwrap_or(&Value::Bool(true)),
        None => s,
    }
}

fn check(root: &Value, s: &Value, v: &Value, path: &str, strict: bool, errs: &mut Vec<String>) {
    let at = if path.is_empty() { "/" } else { path };
    match s {
        Value::Bool(true) => return,
        Value::Bool(false) => {
            errs.push(format!("{at}: nothing is allowed here"));
            return;
        }
        _ => {}
    }
    if let Some(p) = s.get("$ref").and_then(Value::as_str) {
        match p.strip_prefix('#').and_then(|ptr| root.pointer(ptr)) {
            Some(t) => check(root, t, v, path, strict, errs),
            None => errs.push(format!("{at}: unresolved $ref {p}")),
        }
    }
    if let Some(t) = s.get("type") {
        let ok = match t {
            Value::String(t) => type_ok(t, v),
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| type_ok(t, v)),
            _ => true,
        };
        if !ok {
            errs.push(format!("{at}: expected type {t}, got {v}"));
            return;
        }
    }
    if let Some(e) = s.get("enum").and_then(Value::as_array) {
        if !e.contains(v) {
            errs.push(format!(
                "{at}: {v} is not one of {}",
                Value::Array(e.clone())
            ));
        }
    }
    if let Some(c) = s.get("const") {
        if c != v {
            errs.push(format!("{at}: expected {c}, got {v}"));
        }
    }
    if let Some(f) = v.as_f64() {
        if s.get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|m| f < m)
        {
            errs.push(format!("{at}: {f} is below the minimum"));
        }
        if s.get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|m| f > m)
        {
            errs.push(format!("{at}: {f} is above the maximum"));
        }
        if s.get("exclusiveMinimum")
            .and_then(Value::as_f64)
            .is_some_and(|m| f <= m)
        {
            errs.push(format!("{at}: {f} must be above the exclusive minimum"));
        }
    }
    if let Some(st) = v.as_str() {
        if s.get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| st.chars().count() as u64 > m)
        {
            errs.push(format!("{at}: string longer than maxLength"));
        }
        if s.get("pattern").and_then(Value::as_str) == Some(REF_PATTERN)
            && st
                .parse::<Ref>()
                .map(|r| r.to_string() != st)
                .unwrap_or(true)
        {
            errs.push(format!("{at}: {st:?} is not a ref"));
        }
    }
    if let Some(o) = v.as_object() {
        for k in s
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !o.contains_key(k) {
                errs.push(format!("{at}: missing required key `{k}`"));
            }
        }
        let props = s.get("properties").and_then(Value::as_object);
        for (k, x) in o {
            let sub = format!("{path}/{k}");
            match props.and_then(|p| p.get(k)) {
                Some(ps) => check(root, ps, x, &sub, strict, errs),
                None => match s.get("additionalProperties") {
                    Some(Value::Bool(false)) => errs.push(format!("{sub}: key not allowed")),
                    Some(a @ Value::Object(_)) => check(root, a, x, &sub, strict, errs),
                    _ => {}
                },
            }
        }
        // Strict: keys no branch of this schema declares.
        if strict
            && s.get("$ref").is_none()
            && (s.get("properties").is_some() || s.get("type") == Some(&json!("object")))
        {
            let mut keys = vec![];
            if !declared_keys(root, s, &mut keys) {
                for k in o.keys().filter(|k| !keys.contains(k)) {
                    errs.push(format!("{path}/{k}: undocumented key"));
                }
            }
        }
    }
    if let Some(a) = v.as_array() {
        if s.get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| (a.len() as u64) < m)
        {
            errs.push(format!("{at}: fewer items than minItems"));
        }
        if s.get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| a.len() as u64 > m)
        {
            errs.push(format!("{at}: more items than maxItems"));
        }
        let prefix = s.get("prefixItems").and_then(Value::as_array);
        for (i, x) in a.iter().enumerate() {
            let sub = format!("{path}/{i}");
            match prefix.and_then(|p| p.get(i)) {
                Some(ps) => check(root, ps, x, &sub, strict, errs),
                None => {
                    if let Some(is) = s.get("items") {
                        check(root, is, x, &sub, strict, errs);
                    }
                }
            }
        }
    }
    if let Some(bs) = s.get("allOf").and_then(Value::as_array) {
        for b in bs {
            check(root, b, v, path, false, errs);
        }
    }
    let passes = |b: &Value| {
        let mut e = vec![];
        check(root, b, v, path, false, &mut e);
        e.is_empty()
    };
    if let Some(bs) = s.get("anyOf").and_then(Value::as_array) {
        if !bs.iter().any(passes) {
            errs.push(format!("{at}: matches none of anyOf"));
        } else if strict {
            // Recurse strictly into the first matching branch (for nested undocumented keys).
            if let Some(b) = bs.iter().find(|b| passes(b)) {
                let mut e = vec![];
                check(root, b, v, path, true, &mut e);
                errs.extend(e.into_iter().filter(|x| x.ends_with("undocumented key")));
            }
        }
    }
    if let Some(bs) = s.get("oneOf").and_then(Value::as_array) {
        let n = bs.iter().filter(|b| passes(b)).count();
        if n != 1 {
            errs.push(format!("{at}: matches {n} of oneOf (expected exactly 1)"));
        }
    }
}

mod queries;

#[cfg(test)]
mod tests;
