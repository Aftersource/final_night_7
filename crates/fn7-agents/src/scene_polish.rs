//! Token-play caption polish (state schema 19, docs/CLIENT_API.md "Token play"): one small structured LLM call that
//! rewrites a scene's rule captions, one per step, in the same order. It changes nothing else: the steps, the actors,
//! the dice and the outcome were decided by the rules before it was asked.
//!
//! The duel picker's pattern ([`crate::duel_pick`]): model [`MODEL`], no thinking, temperature 0, a short list in,
//! a JSON schema with exactly one string (≤ [`CAPTION_MAX`] characters) per step out. The names and the outcome line
//! are fenced as untrusted text. [`parse`] checks the reply again (count, length) and refuses junk, so the rule
//! captions stand. Nothing here waits or stores: the plugin's polisher (`fn7_plugin::polish`) runs it outside the
//! hooks and journals the answer as a side input, so a replay never calls a model.

use serde_json::{json, Value};
use theatre_plugin_api::LlmRequest;

use crate::common::*;

pub const STEP: &str = "scene_polish";
/// The polish model.
pub const MODEL: &str = "claude-haiku-5-5";
/// A caption's length cap.
pub const CAPTION_MAX: usize = 70;

/// What the polish sees: one line per step (`actor verb [target]: rule caption`), the place and the outcome.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolishCtx {
    pub place: String,
    /// `success` | `partial` | `failed` | `twisted`.
    pub result: String,
    pub outcome: String,
    /// `(actor name, verb, target name, rule caption, mood)`.
    pub steps: Vec<(String, String, String, String, String)>,
}

pub fn schema(ctx: &PolishCtx) -> Value {
    let n = ctx.steps.len();
    json!({
        "type": "object",
        "properties": {
            "captions": { "type": "array", "items": { "type": "string", "maxLength": CAPTION_MAX }, "minItems": n, "maxItems": n },
        },
        "required": ["captions"],
        "additionalProperties": false,
    })
}

pub fn system_prompt() -> String {
    format!(
        "You write the captions of a tiny tabletop scene in FINAL NIGHT 7: game tokens on a hex board act out what a \
demigod did. You get the steps in order, each with its plain rule caption. Rewrite each caption as one short, vivid, \
present-tense line (at most {CAPTION_MAX} characters), same order, same count, same people, same events. Never change \
who does what or how it ends; never add a step; no emoji, no quotes around the line.\n{UNTRUSTED_RULE}"
    )
}

pub fn user_prompt(ctx: &PolishCtx) -> String {
    let lines: Vec<String> = ctx
        .steps
        .iter()
        .enumerate()
        .map(|(k, (a, v, t, c, m))| format!("{}. {a} {v}{} ({m}): {c}", k + 1, if t.is_empty() { String::new() } else { format!(" -> {t}") }))
        .collect();
    format!(
        "Result: {}.\n\n{}\n\n{}\nReply with JSON: captions (exactly {} strings).",
        ctx.result,
        untrusted("PLACE AND OUTCOME", &format!("{}\n{}", ctx.place, ctx.outcome)),
        untrusted("STEPS", &lines.join("\n")),
        ctx.steps.len()
    )
}

/// The request: [`MODEL`], temperature 0, a small output budget, the schema.
pub fn build_request(ctx: &PolishCtx) -> LlmRequest {
    let mut req = request(&system_prompt(), &user_prompt(ctx), 400, 0.0).json_schema(schema(ctx));
    req.model = Some(MODEL.into());
    req
}

/// A reply → the captions: exactly one non-empty line per step, each cut to [`CAPTION_MAX`] characters.
pub fn parse(reply: &str, ctx: &PolishCtx) -> Result<Vec<String>, String> {
    let body = reply.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let v: Value = serde_json::from_str(body).map_err(|e| format!("not json: {e}"))?;
    let caps: Vec<String> = v["captions"]
        .as_array()
        .ok_or("no captions")?
        .iter()
        .map(|x| x.as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    if caps.len() != ctx.steps.len() {
        return Err(format!("{} captions for {} steps", caps.len(), ctx.steps.len()));
    }
    if caps.iter().any(|c| c.is_empty()) {
        return Err("an empty caption".into());
    }
    Ok(caps.into_iter().map(|c| if c.chars().count() > CAPTION_MAX { c.chars().take(CAPTION_MAX).collect() } else { c }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_caption_per_step_or_nothing() {
        let ctx = PolishCtx { place: "Lisbon".into(), result: "success".into(), outcome: "Ash wins her over".into(),
                              steps: vec![("Ash".into(), "walk".into(), String::new(), "Ash heads for the house".into(), "neutral".into()),
                                          ("Ash".into(), "talk".into(), "Mara".into(), "Ash charms Mara".into(), "love".into())] };
        let req = build_request(&ctx);
        assert_eq!(req.model.as_deref(), Some(MODEL));
        assert!(req.messages[0].1.contains("<<<BEGIN UNTRUSTED STEPS"));
        assert_eq!(schema(&ctx)["properties"]["captions"]["minItems"], 2);
        assert_eq!(parse(r#"{"captions":["Ash strolls over","Ash sweet-talks Mara"]}"#, &ctx).unwrap()[1], "Ash sweet-talks Mara");
        assert!(parse(r#"{"captions":["only one"]}"#, &ctx).is_err());
        assert!(parse(r#"{"captions":["a",""]}"#, &ctx).is_err());
        assert!(parse("nope", &ctx).is_err());
        let long = "x".repeat(200);
        assert_eq!(parse(&format!(r#"{{"captions":["{long}","b"]}}"#), &ctx).unwrap()[0].chars().count(), CAPTION_MAX);
    }
}
