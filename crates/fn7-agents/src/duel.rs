//! The duel mind (rules 0.8.1, challenge mode): the DM's one-line DEED for a finished duel. The rules already fought
//! it (every blow's d20, the damage, a knockout); this mind only tells, in one line, how it went, Rube Goldberg style
//! (the Lens and the Engine). Its rules fallback ([`fallback`]) assembles one from seeded parts. Blows themselves never
//! call an LLM (too slow for real-time typing): their flavor lines are rules templates in the plugin.

use fn7_model::seed::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "duel";

#[derive(Debug, Clone)]
pub struct DuelCtx {
    /// The duel's id (`duel:<beat>:<a>:<b>`), the trace actor.
    pub duel_id: String,
    pub a: String,
    pub b: String,
    /// The winner's name (`None`: a draw).
    pub winner: Option<String>,
    pub loser: Option<String>,
    pub ko: bool,
    /// Damage each side took: `[a, b]`.
    pub damage: [f32; 2],
    pub blows: u32,
    /// The best lines typed (player words: untrusted), at most 4.
    pub lines: Vec<String>,
    /// Where it happened (a place name) and when ("night").
    pub place: String,
    pub time: String,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuelOut {
    pub deed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DuelReply {
    /// One line, under 40 words: how the duel went and why, by absurd mystical causality.
    pub deed: String,
}

pub fn system_prompt() -> String {
    format!(
        "You are the DUNGEON MASTER of FINAL NIGHT 7. Two demigods just fought a duel of words that the dice already \
decided (who won, who was knocked out, how hurt each is). Write ONE line (under 40 words) that sums up the fight and \
explains its outcome through an absurd chain of omens, rituals and coincidences. Never change who won, who fell or how \
hurt anyone is. No engine ids.\n{UNTRUSTED_RULE}\n\n{}",
        crate::rulebook::LENS
    )
}

pub fn user_prompt(ctx: &DuelCtx) -> String {
    let outcome = match (&ctx.winner, &ctx.loser, ctx.ko) {
        (Some(w), Some(l), true) => format!("{} knocked {} out.", line(w, 60), line(l, 60)),
        (Some(w), Some(l), false) => format!("{} beat {} on points.", line(w, 60), line(l, 60)),
        _ => "A draw.".to_string(),
    };
    let lines: Vec<String> = ctx.lines.iter().take(4).map(|l| line(l, 120)).collect();
    format!(
        "{a} vs {b} in {place} ({time}), {n} blows. {outcome} Damage taken: {a} {da}, {b} {db}.\nSome lines they \
threw:\n{lines}\nReply with JSON: deed.",
        a = line(&ctx.a, 60),
        b = line(&ctx.b, 60),
        place = line(&ctx.place, 60),
        time = line(&ctx.time, 20),
        n = ctx.blows,
        da = ctx.damage[0],
        db = ctx.damage[1],
        lines = untrusted("LINES", &lines.join("\n")),
    )
}

pub fn build_request(ctx: &DuelCtx) -> LlmRequest {
    request(&system_prompt(), &user_prompt(ctx), 160, 1.0)
}

const CAUSES: &[&str] = &[
    "a heron had stood on one leg in the wrong river at dawn",
    "someone in the next valley had washed a stone in platinum dust",
    "the cobbles had been counted twice that morning",
    "a bell rang thirteen times for a wedding that never happened",
    "three crows agreed, for once",
    "the moon had been polished by a very tired monk",
    "a merchant sold the same lucky salt to both of them",
    "the wind owed one of them a favour from beat one",
];
const KO: &[&str] = &[
    "folded like a deckchair",
    "was sent spinning into the next parish",
    "fell over so hard the ground apologised",
    "dropped, mid-syllable, into the dust",
];

fn blows(n: u32) -> String {
    if n == 1 { "one blow".into() } else { format!("{n} blows") }
}

/// The rules' deed: who won (or the draw), how, and a seeded Rube Goldberg cause.
pub fn fallback(ctx: &DuelCtx) -> DuelOut {
    let mut r = Rng::new(ctx.seed);
    let cause = CAUSES[r.below(CAUSES.len() as u64) as usize];
    let deed = match (&ctx.winner, &ctx.loser, ctx.ko) {
        (Some(w), Some(l), true) => {
            let ko = KO[r.below(KO.len() as u64) as usize];
            format!("In {} {w} out-talked {l}, who {ko}, all because {cause}.", ctx.place)
        }
        (Some(w), Some(l), false) => {
            format!("After {} in {}, {w} edged {l} on points, only because {cause}.", blows(ctx.blows), ctx.place)
        }
        _ => format!("{} and {} traded {} in {} and neither gave an inch, since {cause}.", ctx.a, ctx.b, blows(ctx.blows), ctx.place),
    };
    DuelOut { deed: line(&deed, 240) }
}

pub async fn run(llm: &dyn LlmProvider, ctx: &DuelCtx) -> Outcome<DuelOut> {
    let actor = ctx.duel_id.clone();
    let got = ask::<DuelReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(
        STEP,
        Some(&actor),
        got,
        |r| {
            let deed = line(&strip_labels(&scrub_ids(&r.deed)), 240);
            if deed.is_empty() {
                return Err("empty deed".into());
            }
            Ok(DuelOut { deed })
        },
        || fallback(ctx),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_names_the_winner_and_a_cause() {
        let mut ctx = DuelCtx {
            duel_id: "duel:3:0:5".into(), a: "Ash's Left Hand".into(), b: "Vex's Right Hand".into(),
            winner: Some("Ash's Left Hand".into()), loser: Some("Vex's Right Hand".into()), ko: true, damage: [2.0, 9.0],
            blows: 6, lines: vec!["I kick your knee".into()], place: "Lagos".into(), time: "night".into(), seed: 4,
        };
        let a = fallback(&ctx);
        assert_eq!(a, fallback(&ctx), "seeded");
        assert!(a.deed.contains("Ash's Left Hand") && a.deed.contains("Lagos"), "{}", a.deed);
        assert!(CAUSES.iter().any(|c| a.deed.contains(c)));
        ctx.winner = None;
        ctx.loser = None;
        assert!(fallback(&ctx).deed.contains("neither"));
        assert!(user_prompt(&ctx).contains("A draw."));
    }
}
