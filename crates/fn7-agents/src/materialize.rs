//! Materialization: a newly memorized person gets one line of flavor (appearance, quirk, what they want). The name and
//! traits come from the cluster's seed; the LLM only writes the flavor.

use fn7_model::ClusterId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "materialize";

#[derive(Debug, Clone)]
pub struct MaterializeCtx {
    pub cluster: ClusterId,
    pub cluster_label: String,
    pub place_name: String,
    pub traits: Vec<String>,
    pub needs: Vec<String>,
    /// The seeded name.
    pub name: String,
    /// The seeded personal traits.
    pub personal: Vec<String>,
    /// How they were addressed ("the baker who saw the fire"); may be player text.
    pub addressed_as: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlavorReply {
    /// One line: appearance, a quirk, what they want. Under 30 words.
    pub flavor: String,
}

pub fn system_prompt() -> String {
    format!(
        "You give one ordinary person in FINAL NIGHT 7 a face. Write ONE line, under 30 words: their appearance, one quirk, \
and what they want right now. Concrete and specific; a little funny. Keep their name and traits as given.\n{UNTRUSTED_RULE}"
    )
}

pub fn user_prompt(ctx: &MaterializeCtx) -> String {
    let mut u = format!(
        "{name}, one of the {label} in {place}.\nCrowd traits: {traits}. Crowd needs: {needs}.\nPersonal traits: {personal}.\n",
        name = line(&ctx.name, 60),
        label = line(&ctx.cluster_label, 80),
        place = line(&ctx.place_name, 60),
        traits = line(&ctx.traits.join(", "), 200),
        needs = line(&ctx.needs.join(", "), 200),
        personal = line(&ctx.personal.join(", "), 200),
    );
    if !ctx.addressed_as.trim().is_empty() {
        u.push_str(&format!(
            "They were noticed as:\n{}\n",
            untrusted("ADDRESSED AS", &ctx.addressed_as)
        ));
    }
    u.push_str("Reply with JSON: the flavor line.");
    u
}

pub fn build_request(ctx: &MaterializeCtx) -> LlmRequest {
    request(&system_prompt(), &user_prompt(ctx), 90, 0.9)
}

pub fn fallback(ctx: &MaterializeCtx) -> String {
    let t = ctx
        .personal
        .iter()
        .chain(&ctx.traits)
        .take(2)
        .cloned()
        .collect::<Vec<_>>()
        .join(" and ");
    let want = ctx
        .needs
        .first()
        .map(|n| format!("; wants {n}"))
        .unwrap_or_default();
    let t = if t.is_empty() {
        "unremarkable until now".to_string()
    } else {
        t
    };
    line(
        &format!("{}, one of the {}: {t}{want}.", ctx.name, ctx.cluster_label),
        200,
    )
}

pub async fn run(llm: &dyn LlmProvider, ctx: &MaterializeCtx) -> Outcome<String> {
    let actor = ctx.cluster.to_string();
    let got = ask::<FlavorReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(
        STEP,
        Some(&actor),
        got,
        |r| {
            let f = line(&strip_labels(&r.flavor), 240);
            if f.is_empty() {
                Err("empty flavor".into())
            } else {
                Ok(f)
            }
        },
        || fallback(ctx),
    )
}
