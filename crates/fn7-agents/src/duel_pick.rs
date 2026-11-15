//! The duel picker (state schema 14, docs/DUEL_MOVES.md §4.1): one small structured LLM call per typed human duel
//! line that CHOOSES, from the list (`data/duel_moves.json`), the move class and the presentation (clips, path, the
//! scene object, the body part aimed at). It never decides an outcome: the d20 and the interaction matrix do.
//!
//! The designer asked for a simple agentic flow: a list of options that is run through and selected from, finishing when done, with no need for thinking mode for the
//! most part, and with the list right. all of it should be right there, as long as you collect data properly". So:
//! model [`MODEL`] (no thinking, low effort, temperature 0), one request whose JSON schema's enums are built from the
//! list and the scene ([`schema`]): the reply can only name a class, clips, a path and an object that exist. The line
//! is fenced as untrusted text. [`parse`] checks the reply against the same enums again (a provider without
//! structured outputs could still answer junk) and drops whatever does not belong.
//!
//! Nothing here waits, stores or decides when a pick is used: the plugin's picker (`fn7_plugin::picker`) runs the call
//! with its timeout, journals the answer as a side input and falls back to the keyword mapper.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theatre_plugin_api::LlmRequest;

use crate::common::*;

pub const STEP: &str = "duel_pick";
/// The picker's model.
pub const MODEL: &str = "claude-haiku-5-5";
/// The body parts a pick may aim at.
pub const PARTS: &[&str] = &["head", "chest", "belly", "arms", "legs"];
/// Clips no line picks (they are the rules' reactions and poses).
const NOT_PICKED: &[&str] = &["reaction", "idle", "outro"];
/// Paths a line may ask for (the rest are the rules' or reserved).
pub const PICK_PATHS: &[&str] = &["dash", "leap", "vault", "slide", "wallkick"];

/// What the picker sees besides the list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PickCtx {
    /// The typed line (untrusted).
    pub line: String,
    /// `far` | `mid` | `close` | `grapple`.
    pub range: String,
    /// The typist holds the initiative.
    pub initiative: bool,
    pub my_stance: String,
    pub foe_stance: String,
    /// The defence the foe has set (`guard`, `parry`, `evade`), if any.
    pub foe_set: Option<String>,
    /// The typist's weapon class (`unarmed`, `bow`, ...) and what it holds (item names).
    pub weapon_class: String,
    pub held: Vec<String>,
    /// The scene's objects: `(id, kind, affords)`.
    pub objects: Vec<(String, String, Vec<String>)>,
    /// State schema 21 (duel words): the reply may stage the line as up to 4 `steps`, and aim at the `neck`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub steps: bool,
}

/// Paths a staged step may take (schema 21): to the far side of a scene object, a side step or roll, back, in to the
/// foe, a jump on the spot, a run in a ring.
pub const STEP_PATHS: &[&str] = &["cover", "left", "right", "back", "in", "leap", "circle"];
/// Steps one line may stage.
pub const MAX_STEPS: usize = 4;

/// One staged step of a pick (schema 21).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickStep {
    pub clip: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub object_id: Option<String>,
    #[serde(default)]
    pub target_part: Option<String>,
}

impl PickStep {
    /// As a `move.steps[]` entry: `{clip, path?, object_id?, target_part?}`.
    pub fn json(&self) -> Value {
        let mut v = json!({ "clip": self.clip });
        for (k, x) in [("path", &self.path), ("object_id", &self.object_id), ("target_part", &self.target_part)] {
            if let Some(x) = x {
                v[k] = x.clone().into();
            }
        }
        v
    }
}

/// The picker's choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pick {
    pub move_class: String,
    /// 1 to 3 clips, the first is the move's.
    pub clips: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub object_id: Option<String>,
    pub target_part: String,
    /// Schema 21: the line as staged steps (empty before, and when the line is one move).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<PickStep>,
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x[key].as_str().map(str::to_string)).collect()
}

/// The classes of the list.
pub fn classes(list: &Value) -> Vec<String> {
    strings(&list["classes"], "class")
}

/// The clips a line may pick (every clip of the list but the reactions, idles and outros).
pub fn clips(list: &Value) -> Vec<String> {
    list["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| !NOT_PICKED.iter().any(|n| c["for"].as_str().unwrap_or("").split(", ").any(|f| f == *n)))
        .filter_map(|c| c["clip"].as_str().map(str::to_string))
        .collect()
}

/// The clips a staged step may play (schema 21): the pickable clips and getting up (`rise`, `spin_rise`: popping up from
/// cover).
pub fn step_clips(list: &Value) -> Vec<String> {
    let mut c = clips(list);
    for x in ["rise", "spin_rise"] {
        if list["clips"].as_array().into_iter().flatten().any(|e| e["clip"] == x) && !c.iter().any(|y| y == x) {
            c.push(x.to_string());
        }
    }
    c
}

/// The reply's JSON schema: every choice an enum built from the list and the scene.
pub fn schema(list: &Value, ctx: &PickCtx) -> Value {
    let ids: Vec<Value> = ctx.objects.iter().map(|o| Value::String(o.0.clone())).chain([Value::Null]).collect();
    let paths: Vec<Value> = PICK_PATHS.iter().map(|p| json!(p)).chain([Value::Null]).collect();
    let mut s = json!({
        "type": "object",
        "properties": {
            "move_class": { "type": "string", "enum": classes(list) },
            "clips": { "type": "array", "items": { "type": "string", "enum": clips(list) }, "minItems": 1, "maxItems": 3 },
            "path": { "type": ["string", "null"], "enum": paths },
            "object_id": if ctx.objects.is_empty() { json!({ "type": "null" }) } else { json!({ "type": ["string", "null"], "enum": ids }) },
            "target_part": { "type": "string", "enum": PARTS },
        },
        "required": ["move_class", "clips", "path", "object_id", "target_part"],
        "additionalProperties": false,
    });
    if ctx.steps {
        let parts: Vec<&str> = parts(ctx);
        s["properties"]["target_part"]["enum"] = json!(parts);
        let step_paths: Vec<Value> = STEP_PATHS.iter().map(|p| json!(p)).chain([Value::Null]).collect();
        s["properties"]["steps"] = json!({ "type": "array", "maxItems": MAX_STEPS, "items": {
            "type": "object",
            "properties": {
                "clip": { "type": "string", "enum": step_clips(list) },
                "path": { "type": ["string", "null"], "enum": step_paths },
                "object_id": if ctx.objects.is_empty() { json!({ "type": "null" }) } else { json!({ "type": ["string", "null"], "enum": ids }) },
                "target_part": { "type": ["string", "null"], "enum": parts.iter().map(|p| json!(p)).chain([Value::Null]).collect::<Vec<_>>() },
            },
            "required": ["clip", "path", "object_id", "target_part"],
            "additionalProperties": false,
        } });
        s["required"].as_array_mut().expect("required").push("steps".into());
    }
    s
}

/// The parts a pick may aim at (schema 21 adds the neck).
fn parts(ctx: &PickCtx) -> Vec<&'static str> {
    let mut p = PARTS.to_vec();
    if ctx.steps {
        p.insert(1, "neck");
    }
    p
}

pub fn system_prompt() -> String {
    format!(
        "You stage one blow of a typed duel in FINAL NIGHT 7. A player typed a short line (often a command with typos: \
\"jimpkick!\", \"atack\", \"judo throw\"). Read what they MEAN and pick from the lists given: the move class, one to three \
clips (the first is the move itself; the others are what the figure does first, in order), a path if the figure moves \
in a special way, a scene object if the line uses one, and the body part aimed at. Pick only names from the lists. \
You decide nothing else: dice and rules decide whether it works.\n\
Prefer the clip whose description matches the words; with a weapon class other than unarmed prefer that weapon's clips. \
A line with no move in it (an insult, a boast) is a taunt. A body part named as the target is not the move (\"kick his \
knee\" is a kick aimed at the legs).\n{UNTRUSTED_RULE}"
    )
}

/// The user message: the situation, the list (one line per entry) and the line.
pub fn user_prompt(list: &Value, ctx: &PickCtx) -> String {
    let allowed = clips(list);
    let class_lines: Vec<String> = list["classes"].as_array().into_iter().flatten()
        .map(|c| format!("- {}: {}", c["class"].as_str().unwrap_or(""), c["about"].as_str().unwrap_or(""))).collect();
    let clip_lines: Vec<String> = list["clips"].as_array().into_iter().flatten()
        .filter(|c| c["clip"].as_str().is_some_and(|n| allowed.iter().any(|a| a == n)))
        .map(|c| format!("- {} [{}]: {}", c["clip"].as_str().unwrap_or(""), c["for"].as_str().unwrap_or(""), c["about"].as_str().unwrap_or("")))
        .collect();
    let objects: Vec<String> = ctx.objects.iter().map(|(id, kind, aff)| format!("- {id}: a {kind} (affords {})", if aff.is_empty() { "nothing".into() } else { aff.join(", ") })).collect();
    format!(
        "Situation: range {}; {} the initiative; your stance {}, the foe's {}{}; your weapon class {}{}.\n\n\
MOVE CLASSES\n{}\n\nCLIPS [what they serve]\n{}\n\nPATHS\n- dash: running along the ground\n- leap: a jump\n- vault: over an object\n\
- slide: under an object\n- wallkick: off an object\n\nSCENE OBJECTS\n{}\n\n{}\nReply with JSON: move_class, clips, path, object_id, target_part{}.",
        ctx.range,
        if ctx.initiative { "you hold" } else { "the foe holds" },
        ctx.my_stance,
        ctx.foe_stance,
        ctx.foe_set.as_deref().map(|s| format!(" (the foe has set a {s})")).unwrap_or_default(),
        ctx.weapon_class,
        if ctx.held.is_empty() { String::new() } else { format!(" (holding {})", ctx.held.join(", ")) },
        class_lines.join("\n"),
        clip_lines.join("\n"),
        if objects.is_empty() { "- none".to_string() } else { objects.join("\n") },
        untrusted("DUEL LINE", &ctx.line),
        if ctx.steps { STEPS_NOTE } else { "" },
    )
}

/// Schema 21: what the reply's `steps` are.
const STEPS_NOTE: &str = ", steps (a line with several actions, e.g. \"hide behind that tree then pop up and slit his throat\", as up to 4 \
steps in order, each a clip, a step path (cover: to the far side of object_id; left, right, back, in, leap, circle) and the part aimed \
at (neck for a throat); [] for a single move)";

/// The request: [`MODEL`], temperature 0 (the Anthropic backend sends none: current models take the default), a
/// small output budget, the enum schema.
pub fn build_request(list: &Value, ctx: &PickCtx) -> LlmRequest {
    let mut req = request(&system_prompt(), &user_prompt(list, ctx), 300, 0.0).json_schema(schema(list, ctx));
    req.model = Some(MODEL.into());
    req
}

/// A reply → a pick, checked against the same enums: an unknown class is junk; unknown clips, paths and objects are
/// dropped (no clip left is junk); at most three clips.
pub fn parse(reply: &str, list: &Value, ctx: &PickCtx) -> Result<Pick, String> {
    let body = reply.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let mut p: Pick = serde_json::from_str(body).map_err(|e| format!("not a pick: {e}"))?;
    if !classes(list).contains(&p.move_class) {
        return Err(format!("unknown class {:?}", p.move_class));
    }
    let allowed = clips(list);
    p.clips.retain(|c| allowed.contains(c));
    p.clips.dedup();
    p.clips.truncate(3);
    if p.clips.is_empty() {
        return Err("no clip from the list".into());
    }
    p.path = p.path.filter(|x| PICK_PATHS.contains(&x.as_str()));
    p.object_id = p.object_id.filter(|id| ctx.objects.iter().any(|o| &o.0 == id));
    if !parts(ctx).contains(&p.target_part.as_str()) {
        p.target_part = "chest".into();
    }
    if ctx.steps {
        let step_allowed = step_clips(list);
        p.steps.retain(|s| step_allowed.contains(&s.clip));
        p.steps.truncate(MAX_STEPS);
        for s in p.steps.iter_mut() {
            s.path = s.path.take().filter(|x| STEP_PATHS.contains(&x.as_str()));
            s.object_id = s.object_id.take().filter(|id| ctx.objects.iter().any(|o| &o.0 == id));
            s.target_part = s.target_part.take().filter(|x| parts(ctx).contains(&x.as_str()));
        }
    } else {
        p.steps.clear();
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Value {
        json!({ "classes": [{ "class": "strike", "about": "a hit" }, { "class": "guard", "about": "a guard" }],
                "clips": [{ "clip": "kick", "for": "strike", "about": "a kick" }, { "clip": "block", "for": "guard", "about": "a block" },
                          { "clip": "defeat", "for": "reaction", "about": "down" }] })
    }

    #[test]
    fn enums_come_from_the_list_and_the_scene() {
        let ctx = PickCtx { line: "kick him".into(), range: "mid".into(), objects: vec![("crate_a".into(), "crate".into(), vec!["vault".into()])],
                            weapon_class: "unarmed".into(), ..Default::default() };
        let s = schema(&list(), &ctx);
        assert_eq!(s["properties"]["move_class"]["enum"], json!(["strike", "guard"]));
        assert_eq!(s["properties"]["clips"]["items"]["enum"], json!(["kick", "block"]), "reactions are never picked");
        assert_eq!(s["properties"]["object_id"]["enum"], json!(["crate_a", null]));
        let req = build_request(&list(), &ctx);
        assert_eq!(req.model.as_deref(), Some(MODEL));
        assert!(req.messages[0].1.contains("<<<BEGIN UNTRUSTED DUEL LINE"));
        // Junk is dropped or refused.
        let p = parse(r#"{"move_class":"strike","clips":["defeat","kick","moonwalk"],"path":"flip","object_id":"tree_z","target_part":"soul"}"#, &list(), &ctx).unwrap();
        assert_eq!((p.clips, p.path, p.object_id, p.target_part.as_str()), (vec!["kick".to_string()], None, None, "chest"));
        assert!(parse(r#"{"move_class":"dance","clips":["kick"],"path":null,"object_id":null,"target_part":"head"}"#, &list(), &ctx).is_err());
        assert!(parse("no", &list(), &ctx).is_err());
    }
}
