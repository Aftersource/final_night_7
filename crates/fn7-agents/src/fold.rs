//! Folding: old memory items (or ledger rows) → one summary from the owner's point of view.

use fn7_model::Ref;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "fold";

#[derive(Debug, Clone)]
pub struct FoldCtx {
    /// The memory owner (trace actor).
    pub owner: Ref,
    pub owner_name: String,
    /// The items to fold, oldest first, prose.
    pub items: Vec<String>,
    /// Target length in words.
    pub max_words: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FoldReply {
    /// The summary, from the owner's point of view.
    pub summary: String,
}

pub fn system_prompt() -> String {
    format!(
        "You compress memories in FINAL NIGHT 7. Summarize the items into one short paragraph from the owner's point of view. \
Keep names, debts, grudges, promises and who saw what; drop the rest.\n{UNTRUSTED_RULE}"
    )
}

pub fn user_prompt(ctx: &FoldCtx) -> String {
    let items = ctx
        .items
        .iter()
        .map(|i| format!("- {}", line(i, 300)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Owner: {}.\nItems:\n{}\nReply with JSON: the summary, under {} words.",
        line(&ctx.owner_name, 60),
        untrusted("MEMORIES", &items),
        ctx.max_words.max(10)
    )
}

pub fn build_request(ctx: &FoldCtx) -> LlmRequest {
    request(
        &system_prompt(),
        &user_prompt(ctx),
        (ctx.max_words.max(10) * 2 + 40).min(600),
        0.3,
    )
}

/// The first line of each item, joined, capped.
pub fn fallback(ctx: &FoldCtx) -> String {
    let joined = ctx
        .items
        .iter()
        .filter_map(|i| i.lines().next())
        .map(|l| line(l, 120))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" / ");
    line(&joined, (ctx.max_words.max(10) as usize) * 7)
}

pub async fn run(llm: &dyn LlmProvider, ctx: &FoldCtx) -> Outcome<String> {
    if ctx.items.is_empty() {
        return Outcome::llm(String::new());
    }
    let actor = ctx.owner.to_string();
    let got = ask::<FoldReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(
        STEP,
        Some(&actor),
        got,
        |r| {
            let s = line(&strip_labels(&r.summary), 2000);
            if s.is_empty() {
                Err("empty summary".into())
            } else {
                Ok(s)
            }
        },
        || fallback(ctx),
    )
}
