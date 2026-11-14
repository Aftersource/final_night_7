//! The summon mind (rules 0.8.0): the DM's transit deed for a demigod its player summoned across the board. The rules
//! already moved it and decided the mishap (its d20 and the health lost); this mind only tells, in one hilarious line,
//! how it got from what it was doing there to the new place. Its rules fallback ([`fallback`]) assembles one from seeded
//! parts.

use fn7_model::seed::Rng;
use fn7_model::DemigodId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "summon";

#[derive(Debug, Clone)]
pub struct SummonCtx {
    pub demigod: DemigodId,
    pub name: String,
    /// Its look, one line.
    pub look: String,
    pub from_place: String,
    pub to_place: String,
    /// What it was doing there (its last deed or standing order, one line; may be player words: untrusted).
    pub doing: String,
    pub km: f64,
    /// Health it lost on the way (0 = it landed clean).
    pub health_lost: f32,
    /// The time of day where it lands ("night").
    pub time: String,
    /// Seed for the fallback.
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummonOut {
    pub deed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SummonReply {
    /// One hilarious line, under 40 words: what it was doing, how it was yanked across the world, how it landed.
    pub deed: String,
}

pub fn system_prompt() -> String {
    format!(
        "You are the DUNGEON MASTER of FINAL NIGHT 7. A player just summoned their demigod across the world, instantly. \
Write ONE hilarious line (under 40 words) that accounts for what it was doing where it was, how it was yanked across \
the world, and how it landed in the new place. If it lost health, the landing hurt; if not, it landed with dignity (or \
nearly). Never change where it went or how hurt it is. No engine ids.\n{UNTRUSTED_RULE}\n\n{}",
        crate::rulebook::LENS
    )
}

pub fn user_prompt(ctx: &SummonCtx) -> String {
    format!(
        "{name} ({look}) was in {from}, {km:.0} km away, doing this:\n{doing}\nNow it lands in {to} ({time}). Health \
lost on the way: {lost}.\nReply with JSON: deed.",
        name = line(&ctx.name, 60),
        look = line(&ctx.look, 160),
        from = line(&ctx.from_place, 60),
        km = ctx.km,
        doing = untrusted("DOING", &line(&ctx.doing, 200)),
        to = line(&ctx.to_place, 60),
        time = line(&ctx.time, 20),
        lost = ctx.health_lost,
    )
}

pub fn build_request(ctx: &SummonCtx) -> LlmRequest {
    request(&system_prompt(), &user_prompt(ctx), 160, 1.0)
}

const YANKS: &[&str] = &[
    "was yanked skyward by its own ego",
    "was sucked up a storm drain marked EXPRESS",
    "was folded into a paper airplane by an impatient god",
    "was fired out of a cathedral organ pipe",
    "hitched a ride on a migrating cloud of pigeons",
    "slipped on a banana peel that turned out to be a portal",
    "was mailed, unstamped, by the God of Time",
    "sneezed so hard it changed continents",
];
const CLEAN: &[&str] = &[
    "landed in a perfect superhero crouch, to scattered applause",
    "touched down so gently a nearby cat didn't notice",
    "arrived mid-sentence and finished it as if nothing had happened",
    "stepped out of thin air and straightened its collar",
];
const ROUGH: &[&str] = &[
    "landed face-first in a fish market",
    "came down through a greenhouse roof, then the greenhouse floor",
    "bounced twice off a tram before it stuck the landing",
    "crash-landed into a wedding cake, uninvited",
    "skidded across a frozen fountain and into a statue of itself",
];

/// The rules' deed: what it was doing, a seeded yank, a seeded landing (rough when it got hurt).
pub fn fallback(ctx: &SummonCtx) -> SummonOut {
    let mut r = Rng::new(ctx.seed);
    let yank = YANKS[r.below(YANKS.len() as u64) as usize];
    let land = if ctx.health_lost > 0.0 {
        ROUGH[r.below(ROUGH.len() as u64) as usize]
    } else {
        CLEAN[r.below(CLEAN.len() as u64) as usize]
    };
    let doing = line(&scrub_ids(&ctx.doing), 60);
    let doing = if doing.is_empty() { "minding its own business".to_string() } else { doing };
    SummonOut {
        deed: line(&format!("Mid-{doing} in {}, {} {yank} and {land} in {}.", ctx.from_place, ctx.name, ctx.to_place), 240),
    }
}

pub async fn run(llm: &dyn LlmProvider, ctx: &SummonCtx) -> Outcome<SummonOut> {
    let actor = ctx.demigod.to_string();
    let got = ask::<SummonReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(
        STEP,
        Some(&actor),
        got,
        |r| {
            let deed = line(&strip_labels(&scrub_ids(&r.deed)), 240);
            if deed.is_empty() {
                return Err("empty deed".into());
            }
            Ok(SummonOut { deed })
        },
        || fallback(ctx),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_tells_where_it_was_and_where_it_landed() {
        let mut ctx = SummonCtx {
            demigod: DemigodId(3), name: "Vex's Left Hand".into(), look: "a tall figure".into(), from_place: "Lagos".into(),
            to_place: "Reykjavik".into(), doing: "sermon".into(), km: 6200.0, health_lost: 2.0, time: "night".into(), seed: 9,
        };
        let a = fallback(&ctx);
        assert_eq!(a, fallback(&ctx), "seeded");
        assert!(a.deed.contains("Lagos") && a.deed.contains("Reykjavik") && a.deed.contains("Vex's Left Hand"), "{}", a.deed);
        assert!(ROUGH.iter().any(|x| a.deed.contains(x)));
        ctx.health_lost = 0.0;
        assert!(CLEAN.iter().any(|x| fallback(&ctx).deed.contains(x)));
    }
}
