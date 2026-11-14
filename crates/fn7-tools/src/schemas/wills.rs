//! Rules 0.9.0 ("wills that matter") payload definitions: gear modifiers, curses, mystic quests, a will's answer.
//! Hand-written to match `fn7-plugin/src/wills/{mod,arms,curses,quests}.rs`; the strict validation tests keep them
//! honest.

use serde_json::{json, Map, Value};

use super::{array, boolean, int, nullable, num, object, r, string};

fn e(vals: &[&str], desc: &str) -> Value {
    json!({ "type": "string", "enum": vals, "description": desc })
}

const TIERS: [&str; 8] = ["trinket", "artifact", "relic", "wonder", "legend", "myth", "titan", "apocalypse"];
const CURSES: [&str; 5] = ["seal", "wither", "silence", "bind", "hex"];
const PHASES: [&str; 4] = ["dawn", "day", "dusk", "night"];

/// The gear fields an `Item` gains (all optional: only weapons, armour, relics and artifacts that are held have them).
pub fn item_fields() -> Vec<(&'static str, Value)> {
    vec![
        ("slot", e(&["weapon", "armor", "charm"], "Rules 0.9.0: the slot this gear fills (a demigod fights with its best item per slot).")),
        ("material", string("Rules 0.9.0: what it is made of (the damage type its armour resists; some add to journeys).")),
        ("mods", r("ItemMods")),
        ("power", nullable(r("ItemPower"))),
        ("blessed_by", nullable(string("Rules 0.9.0: the god whose blessing it carries (a tier up, and that god's signature)."))),
        ("does", string("Rules 0.9.0: one line saying what it does (\"+2 to strikes, +0.5 damage, +1 in duels; Smite: ...\").")),
    ]
}

/// The gear fields of a story peg (`story.items[]`), public.
pub fn peg_fields() -> Vec<(&'static str, Value)> {
    vec![
        ("name", string("Rules 0.9.0: the gear's name.")),
        ("slot", e(&["weapon", "armor", "charm"], "")),
        ("tier", e(&TIERS, "")),
        ("mods", r("ItemMods")),
        ("power", json!({ "type": ["string", "null"], "enum": ["smite", "last_stand", "second_wind", null], "description": "Its once-per-match power." })),
        ("does", string("What it does, in one line.")),
    ]
}

pub fn defs() -> Map<String, Value> {
    let mut d = Map::new();
    d.insert("ItemMods".into(), object(
        "Rules 0.9.0: what a piece of gear adds (zeroes are left out). Totals are capped per demigod: +3 on a d20, +1.5 damage, 1.5 guard, 0.4 regen, +3 in duels.",
        &[],
        &[("strike", json!({ "type": "integer", "description": "d20 modifier on its bearer's strikes (attacks and threats)." })),
          ("journey", json!({ "type": "integer", "description": "d20 modifier on its journeys." })),
          ("deed", json!({ "type": "integer", "description": "d20 modifier on its other deeds (parleys, makings, its own wills)." })),
          ("damage", num("Damage added to each strike it lands on another body.")),
          ("guard", num("Damage turned aside from each hit it takes (never more than half the hit).")),
          ("regen", num("Health regained each beat, below its maximum.")),
          ("duel", json!({ "type": "integer", "description": "Added to its duel blows' d20 (challenge mode's gear term)." })),
          ("resist", string("A damage type halved: fire, storm, curse, lightning, cold, water, poison, blade or fall.")),
          ("thorns", num("Damage a demigod takes for striking its bearer."))],
    ));
    d.insert("ItemPower".into(), object(
        "Rules 0.9.0: a once-per-match power (relic tier and up, or Death's blessing).",
        &[("id", e(&["smite", "last_stand", "second_wind"], "")), ("name", string("")), ("text", string("What it does and when.")), ("used", boolean("Spent."))],
        &[],
    ));
    d.insert("QuestStep".into(), json!({
        "type": "object", "additionalProperties": true,
        "description": "Rules 0.9.0: one checkable requirement of a mystic quest. `kind` says which, with its own fields: at {place: shrine {of} | settlement {level} | fault | coast | point {at, name}, phase?, offer?}, hold {word}, encounter, shrine, duel, sparks {n}, army {at, name, general}, vigil {phase, times, seen}.",
        "properties": {
            "step": { "type": "integer", "minimum": 1, "description": "Its number in the chain (steps are done in order)." },
            "kind": { "type": "string", "enum": ["at", "hold", "encounter", "shrine", "duel", "sparks", "army", "vigil"] },
            "text": { "type": "string", "description": "The requirement in plain words." },
            "done": { "type": "boolean" },
            "done_beat": { "type": ["integer", "null"], "minimum": 0 },
            "progress": { "type": "string", "description": "Why it is not done yet, as of the last check (absent when done or never checked)." },
            "place": { "type": "string", "enum": ["shrine", "settlement", "fault", "coast", "point"] },
            "phase": { "type": "string", "enum": PHASES },
            "offer": { "type": "string", "description": "at: an item whose name carries this word must be in hand there; it is burned." },
            "word": { "type": "string", "description": "hold: the word the item's name must carry." }
        },
        "required": ["step", "kind", "text", "done", "done_beat"]
    }));
    d.insert("Quest".into(), object(
        "Rules 0.9.0: a mystic quest: a short chain of checkable steps; when all are done its effect applies.",
        &[
            ("ref", string("`q:<n>`.")),
            ("for", r("Ref")), ("for_name", string("The demigod it is for.")),
            ("seat", int("Whose quest it is (their standing demigods do the steps).")),
            ("helpers", array(json!({ "type": "integer", "minimum": 0 }), "Other seats who took it up (a plea for someone else's fallen demigod).")),
            ("goal", json!({ "type": "object", "additionalProperties": true,
                "description": "What finishing it does: {kind: heal, amount} | {kind: boon, bonus, beats} | {kind: temper, item, intent, tier} | {kind: arm, slot, tier, name, god} | {kind: break_curse, curse, curse_kind} | {kind: raise, years}.",
                "properties": { "kind": { "type": "string", "enum": ["heal", "boon", "temper", "arm", "break_curse", "raise"] } }, "required": ["kind"] })),
            ("god", string("The god whose ruling it is.")),
            ("text", string("The DM's telling, Rube Goldberg style.")),
            ("steps", array(r("QuestStep"), "1-3 steps, in order.")),
            ("steps_done", int("")),
            ("status", e(&["open", "done", "failed"], "")),
            ("given_beat", int("")),
            ("expires_beat", json!({ "type": ["integer", "null"], "minimum": 0, "description": "The last beat it may be finished on (null: it stays)." })),
            ("done_beat", json!({ "type": ["integer", "null"], "minimum": 0 })),
            ("failed", json!({ "type": ["string", "null"], "enum": ["expired", "moot", null] })),
            ("failed_beat", json!({ "type": ["integer", "null"], "minimum": 0 })),
            ("public", boolean("Everyone sees it (a raising, a seal's counter); else only its seat and the curse's owner.")),
            ("waiting", json!({ "type": ["string", "null"], "description": "Every step is done but something holds the effect back (a seal on the one to be raised)." })),
        ],
        &[],
    ));
    d.insert("Curse".into(), object(
        "Rules 0.9.0: a seal or curse. The public face is the mark on its target; `owner`, `owner_name`, `yours`, `on_you`, `paid_sparks` and `quest` are only for the bearer's and the owner's seats.",
        &[
            ("ref", string("`cu:<n>`.")),
            ("kind", e(&CURSES, "seal: no resurrection; wither: no healing; silence: wills cost double; bind: can't be summoned; hex: minus its strength on d20s.")),
            ("title", string("What the table calls it.")), ("does", string("What it does, in plain words.")),
            ("target", r("Ref")), ("target_name", string("")),
            ("strength", json!({ "type": "integer", "minimum": 1, "maximum": 3, "description": "1 weak .. 3 god-tier: its counter-quest's length, a hex's bite." })),
            ("cast_beat", int("")),
            ("until_beat", json!({ "type": ["integer", "null"], "minimum": 0, "description": "The last beat it holds; null = until countered." })),
            ("god_tier", boolean("")), ("god", nullable(string("The god whose god-tier variant it is."))),
            ("counter", string("Always \"a mystic quest\": every curse has one.")),
            ("broken_beat", json!({ "type": ["integer", "null"], "minimum": 0 })),
            ("broken_how", e(&["", "countered", "faded"], "")),
        ],
        &[("owner", r("Ref")), ("owner_name", string("")), ("yours", boolean("You cast it.")), ("on_you", boolean("It lies on one of your demigods.")),
          ("paid_sparks", int("")), ("quest", nullable(r("Quest")))],
    ));
    d.insert("WillRoll".into(), object(
        "Rules 0.9.0: a will part's d20, with every modifier named.",
        &[("die", json!({ "const": "d20" })), ("roll", json!({ "type": "integer", "minimum": 1, "maximum": 20 })),
          ("modifier", json!({ "type": "integer" })), ("total", json!({ "type": "integer" })), ("target_dc", json!({ "type": "integer" })),
          ("verdict", e(&["crit_fail", "fail", "success", "crit_success"], "")),
          ("mods", array(object("", &[("name", string("wardrobe, charm, boon, hexed, <God>'s favor, paid in full, on credit, part paid, the Final Wish, the caster, <God>'s hand.")),
                                      ("value", json!({ "type": "integer" }))], &[]), "The modifiers that made `modifier` (zeroes left out)."))],
        &[],
    ));
    d.insert("WillPaid".into(), object(
        "Rules 0.9.0: what a will part's price came to.",
        &[("price_sparks", int("The price, in sparks.")), ("sparks", int("Sparks paid.")),
          ("bolts_broken", int("Bolts broken into sparks for it (the change stays in the purse).")),
          ("short_sparks", int("Sparks of the price the purse did not pay.")), ("debt", num("Debt the ruling god lent for the shortfall (0 = none)."))],
        &[],
    ));
    d.insert("WillPart".into(), json!({
        "type": "object", "additionalProperties": true,
        "description": "Rules 0.9.0: one part of a will for one target, with its own ruling. Always `part`, `outcome` and `result`; a roll, a price, the reason and the way forward when they apply. Extra fields by part: heal {healed, boon?}, arm {asked_tier, blessed, item}, raise {price {sparks, years, fallen_beat, raised_before}, curse?}, curse {curse, curse_ref, counter}, ask {quests[], curses[]}.",
        "properties": {
            "part": { "type": "string", "enum": ["heal", "bless", "arm", "raise", "curse", "ask", "offer", "gather", "refused"] },
            "target": { "$ref": "#/$defs/Ref" }, "target_name": { "type": "string" },
            "god": { "type": "string", "description": "The god that ruled this part." },
            "roll": { "$ref": "#/$defs/WillRoll" },
            "paid": { "$ref": "#/$defs/WillPaid" },
            "outcome": { "type": "string", "enum": ["crit", "success", "partial", "fizzle", "quest", "blocked", "raised", "sealed", "refused", "unpaid", "cast", "backfired", "told", "offered", "gathered"],
                         "description": "crit / success: as willed; partial: a token now and a quest for the rest (or gear a tier low); fizzle: a natural 1, a quest; quest: nothing could be paid, a quest; blocked: a curse forbids it; sealed: a seal forbids the raising; refused / unpaid: not done, with why; raised; cast / backfired: a curse landed (on its caster); told; offered; gathered." },
            "result": { "type": "string", "description": "What happened, in plain words (\"healed Chris's Left Hand 2.5\", \"forged a Gold Rocket Launcher (trinket): +1 to strikes\")." },
            "why": { "type": "string", "description": "When it fell short: why." },
            "next": { "type": "string", "description": "When it fell short: what would work (the quest's steps, or the price)." },
            "quest": { "$ref": "#/$defs/Quest" },
            "item": { "anyOf": [{ "$ref": "#/$defs/Item" }, { "type": "null" }] },
            "healed": { "type": "number" },
            "asked_tier": { "type": "string", "enum": TIERS },
            "blessed": { "type": "boolean" },
            "curse": {}, "curse_ref": { "type": "string" }, "counter": { "type": "string" }
        },
        "required": ["part", "outcome", "result"]
    }));
    d.insert("Will".into(), object(
        "Rules 0.9.0: what became of one will (one text box of yours this beat, or a fallen player's plea): split into its parts, each resolved on its own. Rules 0.8.1 states send only {text, routed_to, verdict, speech, reason}.",
        &[("text", string("Your words.")),
          ("routed_to", nullable(string("The first god that ruled (Nature for heals, Death for raisings, the gear's patron...)."))),
          ("verdict", e(&["granted", "partial", "refused", "grant", "bargain", "twist", "refuse"], "0.9.0: granted (every part as willed), partial (something fell short: see the parts) or refused. 0.8.1: the god's verdict.")),
          ("speech", nullable(string("The gods' words (rules-written)."))),
          ("reason", string("Every part's result, joined."))],
        &[("by", r("Ref")), ("by_name", string("The demigod whose hand heard it.")),
          ("gods", array(json!({ "type": "string" }), "Every god that ruled, in order.")),
          ("parts", array(r("WillPart"), "Each part x each target."))],
    ));
    d.insert("WillLine".into(), json!({
        "type": "object", "additionalProperties": true,
        "description": "Rules 0.9.0: a public one-liner of a will whose act is public: will.raised {by, by_name, target, target_name, text} or will.curse {target, target_name, curse, text} (never the caster).",
        "properties": { "event": { "type": "string", "enum": ["will.raised", "will.curse"] }, "text": { "type": "string" },
                        "target": { "$ref": "#/$defs/Ref" }, "target_name": { "type": "string" } },
        "required": ["event", "text", "target", "target_name"]
    }));
    d.insert("Boon".into(), object(
        "Rules 0.9.0: a boon on one of your demigods.",
        &[("demigod", r("Ref")), ("bonus", json!({ "type": "integer" })), ("until_beat", int("The last beat it holds.")), ("from", string(""))],
        &[],
    ));
    d.insert("Raises".into(), object("Rules 0.9.0: raisings Death granted you this match, of the cap.", &[("granted", int("")), ("cap", int(""))], &[]));
    d
}

/// The 0.9.0 fields of the seat view and `round.private`: quests, curses, boons, raises.
pub fn seat_fields() -> Vec<(&'static str, Value)> {
    vec![
        ("quests", array(r("Quest"), "Rules 0.9.0: your mystic quests, the public ones and the counters of curses you cast (open, or closed in the last 8 beats), steps with done flags.")),
        ("curses", array(r("Curse"), "Rules 0.9.0: curses holding now (or broken in the last 8 beats): the ones on your demigods and the ones you cast in full, the others' public faces.")),
        ("boons", array(r("Boon"), "Rules 0.9.0: boons on your demigods.")),
        ("raises", r("Raises")),
    ]
}
