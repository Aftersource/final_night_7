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
                ("by", string("The initiator's demigod name.")),
                ("by_ref", r("Ref")),
                ("by_seat", int("The initiator's seat (= window.opened.by_seat).")),
                ("slow_ms", int("How long the window stays open at most.")),
                ("text", string("\"<name> slowed time\".")),
            ],
            &[],
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
            ("at", r("LonLat")),
            ("region", string("Region (planet) or district (city) aimed at.")),
            ("region_ref", r("Ref")),
            ("place", string("The most specific place aimed at.")),
            ("target", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "What it is aimed at directly, or null (a point)." })),
            ("target_name", json!({ "type": ["string", "null"] })),
            ("fizzled", string("Its target doesn't exist (it will fizzle).")),
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
                ("at", r("LonLat")),
                ("region", string("Region (planet) or district (city) it struck.")),
                ("region_ref", r("Ref")),
                ("place", string("The most specific place it struck (a city, or the region).")),
                ("target", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "What it hit directly, or null (a point)." })),
                ("target_name", json!({ "type": ["string", "null"] })),
                ("off_target", boolean("Entropy ≥ 40: physics threw it off its aim.")),
                ("row", json!({ "anyOf": [{ "$ref": "#/$defs/Ref" }, { "type": "null" }], "description": "Its ledger row." })),
                ("fizzled", string("Why it fizzled (not enough bolts, no such target); nothing was spent.")),
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
            &[],
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
    p("start_bolts", i(0, 99, json!(3), "Lightning bolts each demigod starts with."));
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
            "magnitude": { "type": ["number", "null"], "minimum": 0, "maximum": 10, "description": "custom only: 0..10 (default 5); prices it (1 + magnitude/3 bolts, at most 4)." }
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
        &[],
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
        &[],
    )
}

fn brief_public() -> Value {
    object(
        "`round.brief.public`.",
        &[
            ("title", string("\"Beat <n> of <total>\".")),
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
                &[],
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
        &[],
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
        &[("hierarchy", json!({ "type": ["object", "null"], "additionalProperties": true, "description": "The society hierarchy at your zoom (schema 4: node, path, children, crowds, cells, people, armies with chains of command, mobs, battles). Its shape is not frozen yet (the society build is in progress); read defensively." }))],
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
            ("events", array(r("WorldEvent"), "Public geotagged world events this beat, in order (<= 40).")),
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
        &[("society", json!({ "type": ["object", "null"], "additionalProperties": true, "description": "The society's public summary (schema 4). Its shape is not frozen yet (the society build is in progress); read defensively." }))],
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
            "diorama_image": { "$ref": "#/$defs/ImageObject", "description": "Its diorama, the table minifig (images on only; kind diorama)." }
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
            ("demigods", array(demigod, "Every demigod in the match, seats and built-in AI, in id order (dg:0, dg:1, ...).")),
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
        &[("society", json!({ "type": ["object", "null"], "additionalProperties": true, "description": "The society's public summary (schema 4). Its shape is not frozen yet (the society build is in progress); read defensively." }))],
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
        &[],
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
        &[],
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
        &[],
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
pub fn all() -> Vec<(&'static str, Value)> {
    let mut pool = model_defs();
    pool.extend(shared_defs());
    pool.extend(queries::defs());
    let mut round_private = pool["SeatDelta"].clone();
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
                    "oneOf": [r("Slowed"), r("Streak")] }), &pool)),
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
