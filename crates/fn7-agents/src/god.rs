//! A god's mind: rule on one invocation in its domain. Returns the `Ruling` (verdict, price from the god's price table,
//! speech in the god's voice, optional paladin) and the proposals that implement it.
//!
//! Prices always come from the god's table: the kind is the table's, and the amount is the table base scaled by the
//! intent's magnitude and the invoker's favor; the model may only nudge the amount within [0.5x, 2x].

use fn7_model::{
    CharId, Effect, GodDef, GodId, Intent, PriceKind, PriceRule, Proposal, Ref, Ruling, RulingId,
    Source, Target, Verdict,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "god";

/// Beats until a god's debt price falls due.
pub const DEBT_DUE_IN: u32 = 3;

#[derive(Debug, Clone)]
pub struct GodCtx {
    pub god: GodId,
    pub def: GodDef,
    pub beat: u32,
    /// Id the ruling will get.
    pub ruling: RulingId,
    /// The invocation (its span and how are player-derived: shown only inside untrusted fences).
    pub intent: Intent,
    pub invoker: Ref,
    pub invoker_name: String,
    /// The invoker's favor with this god, -10..10.
    pub favor: f32,
    /// Open debt the invoker owes this god.
    pub open_debt: f32,
    /// Entities in play (the intent's targets, the invoker, people nearby).
    pub entities: Vec<Entity>,
    /// Paladins this god can dispatch.
    pub paladins: Vec<Entity>,
    /// Names of rival gods (for bickering).
    pub rivals: Vec<String>,
    /// Recent relevant context (ledger rows, memory), rendered.
    pub context: String,
    /// Lightning bolts the invoker holds, and what this invocation costs (0 for Entropy, who wants you to have more).
    pub bolts: u32,
    pub cost: u32,
    /// How wacky the world is (the Entropy line).
    pub world: String,
    /// Rules 0.8.0 (state schema 6): the Dungeon Master's core rules join the system prompt and `dm_memory` the user
    /// prompt. `false` for older worlds (their recordings replay with the old prompts).
    pub dm: bool,
    /// Settled memory lines about the invoker's region (rules 0.8.0; empty before).
    pub dm_memory: Vec<String>,
}

impl GodCtx {
    /// Can the invoker pay the bolts?
    pub fn can_pay(&self) -> bool {
        self.bolts >= self.cost
    }
    pub fn is_entropy(&self) -> bool {
        self.def.slug == "entropy"
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GodOut {
    pub ruling: Ruling,
    pub proposals: Vec<Proposal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerdictOut {
    Grant,
    Refuse,
    Bargain,
    Twist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GodEffectKind {
    /// Restore the target's position/health/life from `beats` ago.
    Rewind,
    /// Slow or hasten time around the target for `beats` beats (1-5): slowing unless the request or `text` speaks of
    /// speed/haste.
    Tempo,
    Heal,
    Harm,
    /// Bring the dead target back.
    Raise,
    /// The target forgets the `about` entities.
    Forget,
    /// Plant `text` as a memory in the target.
    Plant,
    /// A summoned thing or gift: `text` is "name: description", held by the target.
    Summon,
    /// Pure narration.
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GodEffectOut {
    pub kind: GodEffectKind,
    /// Label or description; null = the invoker.
    #[serde(default)]
    pub target: Option<String>,
    /// Damage/heal points on the 0-10 scale.
    #[serde(default)]
    pub amount: f32,
    /// Beats (rewind, tempo), 1-5.
    #[serde(default)]
    pub beats: i32,
    #[serde(default)]
    pub about: Vec<String>,
    /// What happens, at most 12 words.
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GodReply {
    pub verdict: VerdictOut,
    /// Which row of your price table applies ("rewind", "slow"...).
    #[serde(default)]
    pub price_act: Option<String>,
    /// Multiplier on the table price, 0.5-2 (1 = standard).
    #[serde(default)]
    pub price_scale: f32,
    /// Your words to the invoker, in your voice: at most 30 words.
    pub speech: String,
    /// What actually happens: at most 2 (empty when refused).
    #[serde(default)]
    pub effects: Vec<GodEffectOut>,
    /// Label of a paladin to send, or null.
    #[serde(default)]
    pub dispatch: Option<String>,
    /// The paladin's mission, at most 12 words (null when no paladin).
    #[serde(default)]
    pub mission: Option<String>,
    /// Favor change toward the invoker, -2..2.
    #[serde(default)]
    pub favor_delta: f32,
}

fn labels(ctx: &GodCtx) -> Labels {
    let mut l = Labels::default();
    l.add(Entity::new(
        ctx.invoker,
        ctx.invoker_name.clone(),
        "the invoker",
    ));
    for e in ctx.entities.iter().chain(&ctx.paladins) {
        l.add(e.clone());
    }
    l
}

fn kind_word(k: &PriceKind) -> &'static str {
    match k {
        PriceKind::Life => "years of life (max health)",
        PriceKind::Debt => "a debt, collected later",
        PriceKind::Soul => "a soul",
        PriceKind::Forget => "something forgotten",
        PriceKind::Wild => "something summoned turns wild",
        PriceKind::Clause => "a secret clause",
    }
}

pub fn system_prompt(ctx: &GodCtx) -> String {
    let d = &ctx.def;
    let prices: Vec<String> = d
        .prices
        .iter()
        .map(|p| format!("- {}: {} (base {:.1})", p.act, kind_word(&p.price), p.base))
        .collect();
    format!(
        "You are {name}, a god of FINAL NIGHT 7, a battle royale of demigods. Your domain: {domain}.\n\
Persona: {persona}\nAgenda: {agenda}\nRivals: {rivals}\n\n\
A demigod has reached into your domain. Rule on it:\n\
- grant (free; only for favorites), bargain (granted for your price), twist (granted, but genie rules: it comes true in the \
worst funny way), or refuse.\n\
- Your price table (the price kind is fixed; you may scale the amount 0.5-2):\n{prices}\n\
- effects: what actually happens, concretely: at most 2, each at most 12 words (empty when refused).\n\
- You may dispatch one of your paladins to enforce the price or punish insolence.\n\
- speech: your own voice, at most 30 words; you may bicker with rival gods. Make every word land.\n\
- {labels}\n- {untrusted}\n\n{voice}",
        name = line(&d.name, 40),
        domain = line(&d.domain, 200),
        persona = line(&d.persona, 400),
        agenda = line(&d.agenda, 300),
        rivals = if ctx.rivals.is_empty() { "none".into() } else { ctx.rivals.join(", ") },
        prices = prices.join("\n"),
        labels = LABEL_RULE,
        untrusted = UNTRUSTED_RULE,
        voice = VOICE,
    )
}

pub fn user_prompt(ctx: &GodCtx) -> String {
    let l = labels(ctx);
    let pal: Vec<&str> = ctx
        .paladins
        .iter()
        .filter_map(|e| l.label_of(e.r))
        .collect();
    let mut u = format!(
        "Beat {beat}. Invoker: {inv} {name}. Their favor with you: {favor:+.1} (-10..10). Debt they already owe you: {debt:.1}.\n\
Magnitude of the request: {mag}/10. Lightning bolts they hold: {bolts}; this costs {cost}.{pay}\n{world}\n\nIn play:\n{table}\nYour paladins: {pal}\n",
        bolts = ctx.bolts,
        cost = ctx.cost,
        pay = if ctx.is_entropy() {
            " You ALWAYS say yes (grant or twist), and you love handing out power."
        } else if ctx.can_pay() {
            ""
        } else {
            " They CAN'T PAY: refuse, or twist it."
        },
        world = line(&ctx.world, 200),
        beat = ctx.beat,
        inv = l.label_of(ctx.invoker).unwrap_or("P?"),
        name = line(&ctx.invoker_name, 60),
        favor = ctx.favor,
        debt = ctx.open_debt,
        mag = ctx.intent.magnitude,
        table = l.render(),
        pal = if pal.is_empty() { "(none yet; one can be raised for you)".into() } else { pal.join(", ") },
    );
    if !ctx.context.trim().is_empty() {
        u.push_str(&format!(
            "\nRecently:\n{}\n",
            untrusted("RECENT", &ctx.context)
        ));
    }
    let targets: Vec<String> = ctx
        .intent
        .targets
        .iter()
        .map(|t| match t {
            Target::Known(r) => l
                .label_of(*r)
                .map(str::to_string)
                .unwrap_or_else(|| "someone".into()),
            Target::Mention(m) => m.text.clone(),
        })
        .collect();
    let ask = format!(
        "verb: {}\ntargets: {}\nhow: {}\nwords: {}",
        ctx.intent.verb,
        if targets.is_empty() {
            "none".into()
        } else {
            targets.join(", ")
        },
        ctx.intent.how,
        ctx.intent.span
    );
    u.push_str(&format!(
        "\nThe invocation:\n{}\n\nReply with JSON: your ruling.",
        untrusted("INVOCATION", &ask)
    ));
    u
}

/// Reply cap: a ruling is ~100-150 tokens; generous so it never truncates.
pub const MAX_TOKENS: u32 = 400;

pub fn build_request(ctx: &GodCtx) -> LlmRequest {
    let (mut sys, mut user) = (system_prompt(ctx), user_prompt(ctx));
    if ctx.dm {
        sys = format!("{sys}\n\n{}\n{}", crate::rulebook::DM_CORE, crate::rulebook::LENS);
        if !ctx.dm_memory.is_empty() {
            user.push_str(&format!("\n{}\n", crate::rulebook::MEMORY_HEADING));
            for m in &ctx.dm_memory {
                user.push_str(&format!("- {}\n", line(m, 200)));
            }
        }
    }
    request(&sys, &user, MAX_TOKENS, 0.9)
}

/// The price-table row for this invocation: the named act, else the act word found in the request, else the first row.
pub fn price_rule<'a>(
    def: &'a GodDef,
    act: Option<&str>,
    intent: &Intent,
) -> Option<&'a PriceRule> {
    let named = act.map(|a| a.trim().to_lowercase());
    if let Some(r) = named
        .as_deref()
        .and_then(|a| def.prices.iter().find(|p| p.act == a))
    {
        return Some(r);
    }
    let text = format!("{} {} {}", intent.verb, intent.span, intent.how).to_lowercase();
    def.prices
        .iter()
        .find(|p| {
            text.contains(&p.act)
                || (p.act == "slow" && text.contains("slow"))
                || (p.act == "haste" && text.contains("speed"))
        })
        .or_else(|| def.prices.first())
}

/// Table price: base × magnitude/5 (at least 0.2) × favor multiplier (favor 10 → 0.5x, -10 → 1.5x).
pub fn table_price(rule: &PriceRule, magnitude: u8, favor: f32) -> f32 {
    let favor = finite(favor, -10.0, 10.0);
    let m = (magnitude as f32 / 5.0).max(0.2);
    ((rule.base * m * (1.0 - favor / 20.0)) * 100.0).round() / 100.0
}

fn price_proposal(
    ctx: &GodCtx,
    kind: PriceKind,
    amount: f32,
    mk: &dyn Fn(Effect, &str) -> Proposal,
) -> Proposal {
    match kind {
        PriceKind::Debt => mk(
            Effect::Debt {
                owed_by: ctx.invoker,
                god: ctx.god,
                amount,
                due_in: DEBT_DUE_IN,
            },
            &format!("{} now owes {} {amount:.1}", ctx.invoker_name, ctx.def.name),
        ),
        k => {
            let text = format!(
                "{} pays {} its price ({} {amount:.1})",
                ctx.invoker_name,
                ctx.def.name,
                price_word(&k)
            );
            mk(
                Effect::Price {
                    who: ctx.invoker,
                    god: ctx.god,
                    kind: k,
                    amount,
                },
                &text,
            )
        }
    }
}

fn price_word(k: &PriceKind) -> &'static str {
    match k {
        PriceKind::Life => "life",
        PriceKind::Debt => "debt",
        PriceKind::Soul => "a soul",
        PriceKind::Forget => "a memory",
        PriceKind::Wild => "a summons gone wild",
        PriceKind::Clause => "a hidden clause",
    }
}

/// Time-rate of a tempo effect, read from its text and the request: freezing 0.25, slowing 0.5, hastening 2.
pub fn tempo_factor_for(intent: &Intent, text: &str) -> f32 {
    let t = format!("{} {} {} {}", text, intent.verb, intent.span, intent.how).to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| has_phrase(&t, w));
    if has(&["freeze", "stop time", "standstill", "frozen"]) {
        0.25
    } else if has(&["slow", "slower", "dawdle", "crawl"]) {
        0.5
    } else if has(&[
        "speed",
        "haste",
        "hasten",
        "fast",
        "faster",
        "quicken",
        "accelerate",
        "fast forward",
    ]) {
        2.0
    } else {
        0.5
    }
}

fn target_or_invoker(ctx: &GodCtx, l: &Labels, t: Option<&str>) -> Target {
    t.and_then(|t| l.target(t, ctx.intent.place))
        .unwrap_or(Target::Known(ctx.invoker))
}

fn effect_proposals(
    ctx: &GodCtx,
    l: &Labels,
    e: &GodEffectOut,
    mk: &dyn Fn(Effect, &str) -> Proposal,
) -> Vec<Proposal> {
    let text = if e.text.trim().is_empty() {
        format!("{} acts", ctx.def.name)
    } else {
        line(&e.text, 200)
    };
    let who = match target_or_invoker(ctx, l, e.target.as_deref()) {
        Target::Known(r) => r,
        Target::Mention(m) => {
            return vec![
                mk(
                    Effect::Memorize {
                        mention: m.clone(),
                        cluster: None,
                    },
                    &memorize_act(&ctx.def.name, &m.text),
                ),
                mk(Effect::Note { text: text.clone() }, &text),
            ]
        }
    };
    let amount = finite(e.amount, 0.0, 10.0);
    let fx = match e.kind {
        GodEffectKind::Rewind => Effect::Rewind {
            target: who,
            beats: e.beats.clamp(1, 5) as u32,
        },
        GodEffectKind::Tempo => Effect::Tempo {
            who,
            factor: tempo_factor_for(&ctx.intent, &text),
            beats: if e.beats <= 0 {
                2
            } else {
                e.beats.clamp(1, 5) as u32
            },
        },
        GodEffectKind::Note => Effect::Note { text: text.clone() },
        GodEffectKind::Heal => Effect::Heal {
            who,
            amount: amount.max(1.0),
        },
        GodEffectKind::Harm => Effect::Damage {
            who,
            amount: amount.max(1.0),
            cause: text.clone(),
        },
        GodEffectKind::Raise => Effect::Revive { who },
        GodEffectKind::Forget => Effect::Forget {
            owner: who,
            about: e.about.iter().filter_map(|a| l.resolve(a)).collect(),
        },
        GodEffectKind::Plant => Effect::Plant {
            owner: who,
            text: text.clone(),
            about: e.about.iter().filter_map(|a| l.resolve(a)).collect(),
        },
        GodEffectKind::Summon => {
            let (name, desc) = text
                .split_once(':')
                .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
                .unwrap_or((text.clone(), text.clone()));
            Effect::CreateItem {
                name: line(&name, 40),
                description: desc,
                holder: who,
            }
        }
    };
    vec![mk(fx, &text)]
}

/// Build the ruling + proposals from a decision (shared by the LLM path and the fallback).
#[allow(clippy::too_many_arguments)]
fn assemble(
    ctx: &GodCtx,
    verdict: Verdict,
    price_act: Option<&str>,
    price_scale: f32,
    speech: String,
    effects: &[GodEffectOut],
    dispatch: Option<(Option<CharId>, String)>,
    favor_delta: f32,
    fallback: bool,
) -> GodOut {
    // Entropy always says yes; a god whose price in bolts can't be paid never grants it straight.
    let verdict = match verdict {
        Verdict::Refuse if ctx.is_entropy() => Verdict::Grant,
        Verdict::Grant | Verdict::Bargain if !ctx.can_pay() && !ctx.is_entropy() => Verdict::Twist,
        v => v,
    };
    let l = labels(ctx);
    let mag = ctx.intent.magnitude;
    let valence: i8 = match verdict {
        Verdict::Refuse => -1,
        Verdict::Twist => -2,
        _ => 1,
    };
    let mk = |effect: Effect, act: &str| Proposal {
        source: Source::God {
            god: ctx.god,
            ruling: Some(ctx.ruling),
        },
        intent: Some(ctx.intent.id),
        effect,
        act: line(act, 200),
        actor: Ref::God(ctx.god),
        magnitude: mag,
        valence,
        witnesses: vec![],
        cause: None,
    };
    let rule = price_rule(&ctx.def, price_act, &ctx.intent);
    let free = verdict == Verdict::Grant && ctx.favor >= 5.0;
    let verdict = if verdict == Verdict::Grant && !free {
        Verdict::Bargain
    } else {
        verdict
    };
    let price = match (verdict, rule) {
        (Verdict::Refuse, _) | (_, None) => None,
        _ if free => None,
        (_, Some(r)) => {
            let scale = if price_scale > 0.0 {
                finite(price_scale, 0.5, 2.0)
            } else {
                1.0
            };
            Some((
                r.price.clone(),
                ((table_price(r, mag, ctx.favor) * scale) * 100.0).round() / 100.0,
            ))
        }
    };
    let mut proposals = Vec::new();
    if verdict != Verdict::Refuse {
        for e in effects.iter().take(3) {
            proposals.extend(effect_proposals(ctx, &l, e, &mk));
        }
    }
    if let Some((kind, amount)) = &price {
        proposals.push(price_proposal(ctx, kind.clone(), *amount, &mk));
    }
    let (mut dispatched, mut dispatch_mission) = (None, None);
    if let Some((char, mission)) = dispatch {
        let mission = line(&mission, 200);
        dispatched = char;
        dispatch_mission = Some(mission.clone());
        proposals.push(mk(
            Effect::Dispatch {
                god: ctx.god,
                mission: mission.clone(),
                target: ctx.invoker,
                paladin: char,
            },
            &mission,
        ));
    }
    let favor_delta = finite(favor_delta, -2.0, 2.0);
    if favor_delta.abs() >= 0.05 {
        if let Ref::Demigod(d) = ctx.invoker {
            proposals.push(mk(
                Effect::Favor {
                    god: ctx.god,
                    demigod: d,
                    delta: favor_delta,
                },
                &format!("{}'s favor with {} shifts", ctx.invoker_name, ctx.def.name),
            ));
        }
    }
    let ruling = Ruling {
        id: ctx.ruling,
        beat: ctx.beat,
        god: ctx.god,
        intent: ctx.intent.id,
        invoker: ctx.invoker,
        verdict,
        price,
        speech: line(&strip_speaker(&speech, &ctx.def.name), 400),
        dispatched,
        dispatch_mission,
        fallback,
        bolts: 0,
        at: None,
    };
    GodOut { ruling, proposals }
}

pub fn convert(ctx: &GodCtx, r: GodReply) -> Result<GodOut, String> {
    let l = labels(ctx);
    let verdict = match r.verdict {
        VerdictOut::Grant => Verdict::Grant,
        VerdictOut::Refuse => Verdict::Refuse,
        VerdictOut::Bargain => Verdict::Bargain,
        VerdictOut::Twist => Verdict::Twist,
    };
    let speech = if r.speech.trim().is_empty() {
        template_speech(ctx, verdict)
    } else {
        l.unlabel(&r.speech)
    };
    let mut r = r;
    for e in r.effects.iter_mut() {
        e.text = l.unlabel(&e.text);
    }
    r.mission = r.mission.map(|m| l.unlabel(&m));
    let dispatch = r
        .dispatch
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|p| {
            let char = match l.resolve(p) {
                Some(Ref::Char(c)) if ctx.paladins.iter().any(|e| e.r == Ref::Char(c)) => Some(c),
                _ => None,
            };
            (
                char,
                r.mission
                    .clone()
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| ctx.def.paladin.mission.clone()),
            )
        });
    Ok(assemble(
        ctx,
        verdict,
        r.price_act.as_deref(),
        r.price_scale,
        speech,
        &r.effects,
        dispatch,
        r.favor_delta,
        false,
    ))
}

/// The fallback speech (without a speaker tag: payloads carry the god's name separately).
fn template_speech(ctx: &GodCtx, v: Verdict) -> String {
    if ctx.is_entropy() {
        return "\"YES! Yes yes yes, take it, take ALL of it, ahahhaahhaaa! More power!\"".into();
    }
    if !ctx.can_pay() && v != Verdict::Grant {
        return "\"No bolts, no miracle. You may have this instead.\"".into();
    }
    match v {
        Verdict::Grant => "\"Granted. Consider it a gift. I keep a list of gifts.\"".into(),
        Verdict::Bargain => "\"Granted, for the usual price. Read the fine print later.\"".into(),
        Verdict::Twist => "\"Oh, you'll get exactly what you asked for.\"".into(),
        Verdict::Refuse => "\"No.\"".into(),
    }
}

/// Deterministic ruling from favor and the price table.
/// favor ≥ 5 → grant (free); favor ≤ -5 → refuse (and a paladin if one is free); -5 < favor < -2 → twist; else bargain.
pub fn fallback(ctx: &GodCtx) -> GodOut {
    let f = finite(ctx.favor, -10.0, 10.0);
    let verdict = if ctx.is_entropy() {
        Verdict::Grant
    } else if !ctx.can_pay() {
        if f >= 0.0 {
            Verdict::Twist
        } else {
            Verdict::Refuse
        }
    } else if f >= 5.0 {
        Verdict::Grant
    } else if f <= -5.0 {
        Verdict::Refuse
    } else if f < -2.0 {
        Verdict::Twist
    } else {
        Verdict::Bargain
    };
    let text = format!("{} {} {}", ctx.intent.verb, ctx.intent.span, ctx.intent.how).to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| has_phrase(&text, w));
    let target = ctx.intent.targets.iter().find_map(|t| match t {
        Target::Known(r) => labels(ctx).label_of(*r).map(str::to_string),
        Target::Mention(m) => Some(m.text.clone()),
    });
    let mag = ctx.intent.magnitude as f32;
    let name = &ctx.def.name;
    let effect =
        |kind, target: Option<String>, amount: f32, beats: i32, text: String| GodEffectOut {
            kind,
            target,
            amount,
            beats,
            about: vec![],
            text,
        };
    let e = if has(&["rewind", "undo", "go back", "turn back"]) {
        effect(
            GodEffectKind::Rewind,
            target.clone(),
            0.0,
            (1 + ctx.intent.magnitude as i32 / 4).min(5),
            format!("{name} rewinds the moment"),
        )
    } else if has(&[
        "slow",
        "speed",
        "haste",
        "hasten",
        "freeze",
        "stop time",
        "fast forward",
    ]) {
        effect(
            GodEffectKind::Tempo,
            target.clone(),
            0.0,
            0,
            format!("{name} bends the seconds around {}", ctx.invoker_name),
        )
    } else if has(&["raise", "resurrect", "revive", "bring back"]) {
        effect(
            GodEffectKind::Raise,
            target.clone(),
            10.0,
            0,
            format!("{name} sends someone back up, slightly used"),
        )
    } else if has(&["forget", "erase", "mind wipe"]) {
        effect(
            GodEffectKind::Forget,
            target.clone(),
            0.0,
            0,
            format!("{name} takes a memory"),
        )
    } else if has(&["heal", "bless", "protect"]) {
        effect(
            GodEffectKind::Heal,
            target.clone(),
            (mag / 2.0).max(1.0),
            0,
            format!("{name} mends what was broken"),
        )
    } else if target.is_some()
        && has(&[
            "kill",
            "curse",
            "storm",
            "lightning",
            "strike",
            "flood",
            "earthquake",
            "tornado",
            "smite",
            "wildfire",
        ])
    {
        effect(
            GodEffectKind::Harm,
            target.clone(),
            (mag * 0.6).clamp(1.0, 8.0),
            0,
            format!("{name} lands a blow on the target"),
        )
    } else {
        effect(
            GodEffectKind::Note,
            None,
            0.0,
            0,
            format!("{name} does as asked, more or less"),
        )
    };
    let dispatch = (verdict == Verdict::Refuse).then(|| {
        let char = ctx.paladins.iter().find_map(|e| {
            if let Ref::Char(c) = e.r {
                Some(c)
            } else {
                None
            }
        });
        (char, ctx.def.paladin.mission.clone())
    });
    let speech = template_speech(ctx, verdict);
    assemble(ctx, verdict, None, 1.0, speech, &[e], dispatch, 0.0, true)
}

pub async fn run(llm: &dyn LlmProvider, ctx: &GodCtx) -> Outcome<GodOut> {
    let actor = ctx.god.to_string();
    let got = ask::<GodReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(
        STEP,
        Some(&actor),
        got,
        |r| convert(ctx, r),
        || fallback(ctx),
    )
}
