//! The manifest mind (rules 0.7.0): words around an item-engine verdict. The rules already decided everything (tier,
//! cost, odds, the roll, the outcome); this mind only **names and describes** what appeared, writes a banner motto for
//! a host, and the twist of a twisted outcome. Asked only for big things (tier ≥ wonder) or twists, at most
//! `manifest_calls_per_beat` a beat; everything else, and every failed or late call, takes the rules fallback
//! ([`fallback`]: `fn7_model::manifest::rules_name` plus a description from the request).

use fn7_model::manifest::{ItemKind, Outcome as Verdict, Tier};
use fn7_model::DemigodId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "manifest";

#[derive(Debug, Clone)]
pub struct ManifestCtx {
    pub demigod: DemigodId,
    /// The manifester's name.
    pub by_name: String,
    /// Its look, one line (its visual sheet's brief), so the item suits its maker.
    pub look: String,
    pub kind: ItemKind,
    pub tier: Tier,
    pub count: f64,
    pub unit: String,
    pub outcome: Verdict,
    pub place_name: String,
    /// The player's words it came from (untrusted).
    pub span: String,
    /// The name the player asked for, if any (untrusted).
    pub asked_name: String,
    /// The rules' name (used when the mind is not asked or fails).
    pub rules_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestOut {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motto: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twist: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ManifestReply {
    /// A proper name for the thing, under 8 words.
    pub name: String,
    /// One line: what it looks like and does. Under 30 words.
    pub description: String,
    /// For an army or fleet: a short war cry for its banner (under 10 words); else empty.
    pub motto: String,
    /// For a twisted outcome: how it went wrong (under 25 words); else empty.
    pub twist: String,
}

pub fn system_prompt() -> String {
    format!(
        "You name the things demigods conjure in FINAL NIGHT 7. The rules already decided what appeared and how big it \
is; never change the size, the count or the outcome. Give it a proper name (under 8 words), one line describing it \
(under 30 words, concrete, a little grandiose), a banner motto for an army or fleet, and for a twisted outcome how it \
went wrong. No engine ids.\n{UNTRUSTED_RULE}"
    )
}

fn count_words(ctx: &ManifestCtx) -> String {
    if ctx.kind.is_host() {
        format!("{} {}", fn7_model::manifest::group_digits(ctx.count), ctx.unit)
    } else if ctx.count > 1.0 {
        format!("{} copies", fn7_model::manifest::group_digits(ctx.count))
    } else {
        "one".into()
    }
}

pub fn user_prompt(ctx: &ManifestCtx) -> String {
    let mut u = format!(
        "{by} ({look}) conjured a {kind:?} at {place}: tier {tier}, {count}. Outcome: {outcome:?}.\n",
        by = line(&ctx.by_name, 60),
        look = line(&ctx.look, 160),
        kind = ctx.kind,
        place = line(&ctx.place_name, 60),
        tier = ctx.tier.as_str(),
        count = count_words(ctx),
        outcome = ctx.outcome,
    );
    u.push_str(&format!("What they asked for:\n{}\n", untrusted("ORDERS", &line(&ctx.span, 300))));
    if !ctx.asked_name.trim().is_empty() {
        u.push_str(&format!("The name they wanted:\n{}\n", untrusted("NAME", &line(&ctx.asked_name, 80))));
    }
    u.push_str("Reply with JSON: name, description, motto, twist.");
    u
}

pub fn build_request(ctx: &ManifestCtx) -> LlmRequest {
    request(&system_prompt(), &user_prompt(ctx), 220, 0.9)
}

/// The rules' words: the asked-for name (if it is a usable name) or [`ManifestCtx::rules_name`], and a description
/// from the outcome and the count. Never a bare verb.
pub fn fallback(ctx: &ManifestCtx) -> ManifestOut {
    let asked = line(&scrub_ids(&ctx.asked_name), 60);
    let name = if asked.split_whitespace().count() >= 2 { asked } else { ctx.rules_name.clone() };
    let what = match ctx.outcome {
        Verdict::Partial => "smaller than asked for",
        Verdict::Twisted => "not quite as intended",
        _ => "as asked",
    };
    ManifestOut {
        description: line(
            &format!("Conjured by {} at {}, {what}: {}.", ctx.by_name, ctx.place_name, count_words(ctx)),
            200,
        ),
        name,
        motto: None,
        twist: (ctx.outcome == Verdict::Twisted).then(|| "Entropy had its own ideas about it.".to_string()),
    }
}

pub async fn run(llm: &dyn LlmProvider, ctx: &ManifestCtx) -> Outcome<ManifestOut> {
    let actor = ctx.demigod.to_string();
    let got = ask::<ManifestReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(
        STEP,
        Some(&actor),
        got,
        |r| {
            let name = line(&strip_labels(&scrub_ids(&r.name)), 80);
            let description = line(&strip_labels(&scrub_ids(&r.description)), 240);
            if name.is_empty() || description.is_empty() {
                return Err("empty name or description".into());
            }
            let opt = |s: &str| Some(line(&strip_labels(&scrub_ids(s)), 120)).filter(|x| !x.is_empty());
            Ok(ManifestOut {
                name,
                description,
                motto: if ctx.kind.is_host() { opt(&r.motto) } else { None },
                twist: if ctx.outcome == Verdict::Twisted { opt(&r.twist) } else { None },
            })
        },
        || fallback(ctx),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(asked: &str) -> ManifestCtx {
        ManifestCtx {
            demigod: DemigodId(0),
            by_name: "Chris's Left Hand".into(),
            look: "copper and teal".into(),
            kind: ItemKind::Army,
            tier: Tier::Myth,
            count: 1e8,
            unit: "soldiers".into(),
            outcome: Verdict::Success,
            place_name: "Paris".into(),
            span: "raise a banner army of 100,000,000".into(),
            asked_name: asked.into(),
            rules_name: "Banner of the Horde of 100,000,000".into(),
        }
    }

    #[test]
    fn fallback_never_names_a_bare_verb() {
        assert_eq!(fallback(&ctx("")).name, "Banner of the Horde of 100,000,000");
        assert_eq!(fallback(&ctx("raise")).name, "Banner of the Horde of 100,000,000");
        assert_eq!(fallback(&ctx("The Copper Tide")).name, "The Copper Tide");
        assert!(fallback(&ctx("")).description.contains("100,000,000 soldiers"));
    }
}
