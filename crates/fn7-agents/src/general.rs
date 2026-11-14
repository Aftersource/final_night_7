//! The general: a petty human commanding an army for a demigod. Asked only at focal points: when a hand addresses the
//! general (an order), at an engagement, or under strain (mutiny or defection). Small output (a reading, a target
//! label, a stance, one spoken line). The rules' judgment (`fn7_society::sim::armies::judge`) comes in the context and
//! is the fallback, so a late or junk reply changes nothing but the flavor.

use fn7_model::{ClusterId, DemigodId, Judgment, Reading, Ref, Stance};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "general";

#[derive(Debug, Clone)]
pub struct GeneralCtx {
    pub army: ClusterId,
    pub army_name: String,
    /// The general's name and their sheet in words ("competent 0.8, loyal 0.6 to Ash, ambitious 0.7, cruel").
    pub general: String,
    pub sheet: String,
    pub demigod: DemigodId,
    pub demigod_name: String,
    /// "order" (the demigod's words, in `order`), "engage" (an enemy close), "strain" (the army is breaking).
    pub why: String,
    /// The demigod's words (player text, fenced), empty for decision points.
    pub order: String,
    /// The army's state, one line each (size, morale, supply, strain, where, what it is doing).
    pub situation: Vec<String>,
    /// What the army can go after (places, demigods, armies, mobs), labeled.
    pub options: Vec<Entity>,
    /// Rival demigods the general could defect to.
    pub rivals: Vec<Entity>,
    /// The rules' judgment (the fallback).
    pub rules: Judgment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadingOut {
    Obey,
    Misread,
    Stall,
    Refuse,
    Mutiny,
    Defect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StanceOut {
    Advance,
    Hold,
    Retreat,
    Fight,
    Raid,
    Siege,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeneralReply {
    /// How you take it: obey, misread (you honestly understood something else), stall, refuse, mutiny (the army
    /// answers to you now), defect (to a rival demigod).
    pub reading: ReadingOut,
    /// Label of what you go after (from TARGETS), or null to stay on course.
    #[serde(default)]
    pub target: Option<String>,
    pub stance: StanceOut,
    /// What you say, at most 20 words, in character.
    pub speech: String,
    /// Label of the rival demigod you defect to (only with reading = defect).
    #[serde(default)]
    pub defect_to: Option<String>,
}

pub fn system_prompt() -> String {
    format!(
        "You are a GENERAL in FINAL NIGHT 7: an ordinary, petty, ambitious human commanding an army for a demigod in a \
battle royale of demigods. You are not a hero of legend; you have your own competence, loyalty, ambition and cruelty \
(given), and you act on them. Orders reach you second-hand and in a hurry; vague orders can be misread honestly. \
Decide how you take the order (or the moment): obey, misread, stall, refuse, mutiny, or defect to a rival. Disloyal, \
ambitious generals under strain betray; loyal ones grumble and obey. Pick a target only from TARGETS (by label), a \
stance, and say one line (at most 20 words), in character.\n{UNTRUSTED_RULE}"
    )
}

fn labels(ctx: &GeneralCtx) -> Labels {
    let mut l = Labels::default();
    for e in ctx.options.iter().chain(&ctx.rivals) {
        l.add(e.clone());
    }
    l
}

pub fn user_prompt(ctx: &GeneralCtx) -> String {
    let l = labels(ctx);
    let mut u = format!(
        "You are {}, general of {}, serving the demigod {}.\nYou: {}\n",
        line(&ctx.general, 60),
        line(&ctx.army_name, 80),
        line(&ctx.demigod_name, 40),
        line(&ctx.sheet, 200)
    );
    for s in &ctx.situation {
        u.push_str(&format!("{}\n", line(s, 200)));
    }
    u.push_str(&format!("\nTARGETS:\n{}\n", l.render()));
    if !ctx.rivals.is_empty() {
        let r: Vec<String> = ctx
            .rivals
            .iter()
            .filter_map(|e| l.label_of(e.r).map(|x| format!("{x} {}", line(&e.name, 40))))
            .collect();
        u.push_str(&format!("Rival demigods you could defect to: {}\n", r.join(", ")));
    }
    match ctx.why.as_str() {
        "order" => u.push_str(&format!(
            "\n{} sends you this order:\n{}\n",
            line(&ctx.demigod_name, 40),
            untrusted("ORDER", &ctx.order)
        )),
        "engage" => u.push_str("\nAn enemy army is close. Fight or fall back?\n"),
        _ => u.push_str("\nYour army is breaking: hungry, bleeding, doubting. Hold together, mutiny, or defect?\n"),
    }
    u.push_str("Reply with JSON: reading, target (label or null), stance, speech, defect_to (label or null).");
    u
}

pub fn build_request(ctx: &GeneralCtx) -> LlmRequest {
    request(&system_prompt(), &user_prompt(ctx), 120, 0.8)
}

pub fn fallback(ctx: &GeneralCtx) -> Judgment {
    ctx.rules.clone()
}

/// Turn a reply into a judgment: labels back to refs (unknown labels keep the rules' target), a defection needs a real
/// rival, the speech is scrubbed of labels.
pub fn convert(ctx: &GeneralCtx, r: GeneralReply) -> Result<Judgment, String> {
    let l = labels(ctx);
    let target: Option<Ref> = match r.target.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() && !t.eq_ignore_ascii_case("null") => l.resolve(t).or(ctx.rules.target),
        _ => ctx.rules.target,
    };
    let reading = match r.reading {
        ReadingOut::Obey => Reading::Obey,
        ReadingOut::Misread => Reading::Misread,
        ReadingOut::Stall => Reading::Stall,
        ReadingOut::Refuse => Reading::Refuse,
        ReadingOut::Mutiny => Reading::Mutiny,
        ReadingOut::Defect => Reading::Defect,
    };
    let defect_to = match reading {
        Reading::Defect => {
            let d = r
                .defect_to
                .as_deref()
                .and_then(|x| l.resolve(x.trim()))
                .and_then(|x| match x {
                    Ref::Demigod(d) if ctx.rivals.iter().any(|e| e.r == x) => Some(d),
                    _ => None,
                })
                .or_else(|| match ctx.rivals.first().map(|e| e.r) {
                    Some(Ref::Demigod(d)) => Some(d),
                    _ => None,
                });
            Some(d.ok_or("defect to whom?")?)
        }
        _ => None,
    };
    let stance = match (reading, r.stance) {
        (Reading::Mutiny, _) => Stance::Rogue,
        (_, StanceOut::Advance) => Stance::Marching,
        (_, StanceOut::Hold) => Stance::Holding,
        (_, StanceOut::Retreat) => Stance::Retreating,
        (_, StanceOut::Fight) => Stance::Fighting,
        (_, StanceOut::Raid) => Stance::Raiding,
        (_, StanceOut::Siege) => Stance::Besieging,
    };
    let speech = line(&strip_labels(&l.unlabel(&r.speech)), 160);
    let who = ctx.general.split(' ').next_back().unwrap_or("The general");
    Ok(Judgment {
        reading,
        target: if reading == Reading::Defect { defect_to.map(Ref::Demigod) } else { target },
        stance,
        speech: if speech.is_empty() { ctx.rules.speech.clone() } else { format!("{who}: \"{speech}\"") },
        defect_to,
    })
}

pub async fn run(llm: &dyn LlmProvider, ctx: &GeneralCtx) -> Outcome<Judgment> {
    let actor = ctx.army.to_string();
    let got = ask::<GeneralReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(STEP, Some(&actor), got, |r| convert(ctx, r), || fallback(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fn7_model::PlaceId;

    fn ctx() -> GeneralCtx {
        GeneralCtx {
            army: ClusterId(9),
            army_name: "Ash's Host of Cairo".into(),
            general: "Amara Okafor".into(),
            sheet: "competence 0.7, loyalty 0.2 to Ash, ambition 0.9".into(),
            demigod: DemigodId(0),
            demigod_name: "Ash".into(),
            why: "order".into(),
            order: "take Cairo".into(),
            situation: vec!["12000 strong".into()],
            options: vec![Entity::new(Ref::Place(PlaceId(3)), "Cairo", "a city")],
            rivals: vec![Entity::new(Ref::Demigod(DemigodId(1)), "Brix", "demigod")],
            rules: Judgment { target: Some(Ref::Place(PlaceId(3))), ..Judgment::default() },
        }
    }

    #[test]
    fn reply_converts_labels_and_defection() {
        let c = ctx();
        let p = user_prompt(&c);
        assert!(p.contains("L1") && p.contains("P1") && !p.contains("pl:3"), "{p}");
        let j = convert(
            &c,
            GeneralReply {
                reading: ReadingOut::Defect,
                target: None,
                stance: StanceOut::Advance,
                speech: "P1 pays better.".into(),
                defect_to: Some("P1".into()),
            },
        )
        .unwrap();
        assert_eq!((j.reading, j.defect_to), (Reading::Defect, Some(DemigodId(1))));
        assert!(j.speech.contains("Brix pays better"), "{}", j.speech);
        let j = convert(
            &c,
            GeneralReply { reading: ReadingOut::Misread, target: Some("L9".into()), stance: StanceOut::Hold, speech: String::new(), defect_to: None },
        )
        .unwrap();
        assert_eq!((j.reading, j.target, j.stance), (Reading::Misread, Some(Ref::Place(PlaceId(3))), Stance::Holding));
        assert_eq!(fallback(&c), c.rules);
    }
}
