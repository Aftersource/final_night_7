//! The world resolver: Final Night's game master, one call per active vicinity per beat. Everyone's ordinary acts in one
//! place → one outcome and the proposals that make it so.

use fn7_model::seed::Rng;
use fn7_model::{Effect, Intent, LonLat, Mention, PlaceId, Proposal, Ref, Source, Target};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "resolver";

/// The rules fallback's chance of a Final Destination accident on the actor of a non-attack act: 1 in this.
pub const ACCIDENT_ODDS: u64 = 20;

/// One ordinary act to resolve here.
#[derive(Debug, Clone)]
pub struct ActLine {
    pub intent: Intent,
    /// Who does it: the demigod, or the possessed body / subagent acting for them.
    pub actor: Ref,
}

#[derive(Debug, Clone)]
pub struct ResolverCtx {
    pub place: PlaceId,
    pub place_name: String,
    /// Terrain and tags, one line.
    pub place_blurb: String,
    pub center: LonLat,
    pub beat: u32,
    pub beats_total: u32,
    pub alive_demigods: u32,
    /// Who and what is here: demigods, characters, clusters, groups, items, neighboring places. Blurbs carry condition.
    pub present: Vec<Entity>,
    pub acts: Vec<ActLine>,
    /// Recent ledger rows around here, prose.
    pub recent: Vec<String>,
    /// Motifs to avoid (already overused this match).
    pub overused: Vec<String>,
    /// Eliminated demigods' names.
    pub eliminated: Vec<String>,
    /// How wacky the world is (the Entropy line) and the lasting world mods here, one line each.
    pub world: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolverOut {
    /// The singular outcome, prose (for captions and memory).
    pub outcome: String,
    pub proposals: Vec<Proposal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResKind {
    /// actor goes to target (a place or someone).
    Move,
    /// target takes `amount` damage (0 flashbang .. 10 instant elimination).
    Damage,
    Heal,
    /// a crowd's feeling (`feel`) shifts by `amount` (-1..1).
    Mood,
    /// target's feeling (`feel`) toward actor shifts by `amount` (-1..1).
    Relation,
    /// actor joins the target group.
    Join,
    /// actor finds or makes an item; text = "name: description".
    Item,
    /// someone unlisted steps into the story (target = their description; any effect aimed at a description does
    /// this already).
    Memorize,
    /// narration only.
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Feel {
    Anger,
    Fear,
    Hope,
    Devotion,
    Trust,
    Loyalty,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResEffectOut {
    pub kind: ResKind,
    /// Label of who does it.
    pub actor: String,
    /// Label (or description, for memorize) of who/what it lands on.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub amount: f32,
    #[serde(default)]
    pub feel: Option<Feel>,
    /// What happens, at most 12 words, in the voice (item: "name: description").
    pub text: String,
    /// Number of the act this comes from (0 = none: an accident or the world).
    #[serde(default)]
    pub act: i32,
    /// 0-10.
    #[serde(default)]
    pub magnitude: i32,
    /// -5 atrocity .. +5 miracle, as the affected see it.
    #[serde(default)]
    pub valence: i32,
    /// Labels of who saw it ([] = everyone here).
    #[serde(default)]
    pub witnesses: Vec<String>,
}

/// Effects only: no outcome paragraph (latency is output tokens; the rows' lines are the story, and the narrator tells
/// it later off the critical path).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResolverReply {
    /// One per act (its result, even a failure), plus at most one accident.
    pub effects: Vec<ResEffectOut>,
}

fn labels(ctx: &ResolverCtx) -> Labels {
    let mut l = Labels::default();
    l.add(
        Entity::new(
            Ref::Place(ctx.place),
            ctx.place_name.clone(),
            ctx.place_blurb.clone(),
        )
        .at(ctx.center, Some(ctx.place)),
    );
    for e in &ctx.present {
        l.add(e.clone());
    }
    for a in &ctx.acts {
        if l.label_of(a.actor).is_none() {
            l.add(Entity::new(a.actor, "someone", ""));
        }
    }
    l
}

/// Pacing, so the match actually ends (Final Night).
pub fn pacing(beat: u32, total: u32, alive: u32) -> String {
    let n = total.max(1);
    let t = beat as f64 / n as f64;
    let mood = if beat >= n {
        "THE FINAL NIGHT. Every debt is due; fate collects tonight."
    } else if t >= 0.8 {
        "Endgame: the arena is nearly closed and fate is impatient. Most fights end with someone eliminated."
    } else if t >= 0.5 {
        "Late game: fate is closing in. Fights are deadly and accidents are lethal."
    } else if t >= 0.2 {
        "Mid game: danger is real. Fights and accidents can eliminate people."
    } else {
        "Early game: everyone is scattered, but fate already has a sense of humour."
    };
    format!("Beat {beat} of {n}, {alive} demigods still standing. {mood}")
}

pub const GAME_MASTER: &str = "You are the GAME MASTER of FINAL NIGHT 7, deciding the singular outcome of everyone's acts in \
one place during one beat (about 6 seconds). Be grounded in who is actually here, their condition and what they asked for, then \
let fate get creative.

RULES:
- hostile by default: everyone is hostile unless their acts say otherwise (or they trick, seduce or befriend someone).
- accidents: if an act has no explicit intent to harm, any harm becomes a Final Destination-style Rube Goldberg accident, with a \
small chance of backfiring on the actor.
- foreknowledge: nobody knows anyone else's plans.
- items are never lost, stolen or destroyed: holders only grow. People only use items they have or find this beat.
- the eliminated are gone for good; they cannot act or be described as alive.
- damage scale: 0 flashbang (no lasting harm), 3 a bad punch, 5 a stab, 8 a car hit, 10 instant elimination. Health is 10.
- crowds (K) are many people: harming one shrinks it and shifts its mood; talking to one shifts its mood.
- addressing an unlisted person (\"the baker\") makes them part of the story: aim the effect at their description.
- the luck is heavily skewed: very good and very bad things happen far more often than you'd expect.
- resolve every listed act with ONE effect (its result, even if it fails), then at most one accident. Keep it to what \
happens THIS beat.
- be terse: each effect's text is one punchy line of at most 12 words, in the voice. No outcome paragraph.";

pub fn system_prompt() -> String {
    format!("{GAME_MASTER}\n- {LABEL_RULE}\n- {UNTRUSTED_RULE}\n\n{VOICE}")
}

pub fn user_prompt(ctx: &ResolverCtx) -> String {
    let l = labels(ctx);
    let mut u = format!(
        "{pace}\nPlace: L1 {name} ({blurb}).\n",
        pace = pacing(ctx.beat, ctx.beats_total, ctx.alive_demigods),
        name = line(&ctx.place_name, 60),
        blurb = line(&ctx.place_blurb, 160),
    );
    for w in &ctx.world {
        u.push_str(&format!("{}\n", line(w, 200)));
    }
    if !ctx.eliminated.is_empty() {
        u.push_str(&format!(
            "Already eliminated (gone for good): {}.\n",
            ctx.eliminated
                .iter()
                .map(|n| line(n, 40))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    u.push_str(&format!("\nHere:\n{}\n", l.render()));
    if !ctx.recent.is_empty() {
        let rows = ctx
            .recent
            .iter()
            .map(|r| line(r, 200))
            .collect::<Vec<_>>()
            .join("\n");
        u.push_str(&format!(
            "\nPreviously around here (already happened; escalate from it, don't replay it):\n{}\n",
            untrusted("RECENT", &rows)
        ));
    }
    if !ctx.overused.is_empty() {
        u.push_str(&format!(
            "Already overused this match (do not use again): {}.\n",
            ctx.overused.join(", ")
        ));
    }
    u.push_str("\nThe acts this beat:\n");
    for (i, a) in ctx.acts.iter().enumerate() {
        let who = l.label_of(a.actor).unwrap_or("someone");
        let targets: Vec<String> = a
            .intent
            .targets
            .iter()
            .map(|t| match t {
                Target::Known(r) => l
                    .label_of(*r)
                    .map(str::to_string)
                    .unwrap_or_else(|| "someone out of reach".into()),
                Target::Mention(m) => m.text.clone(),
            })
            .collect();
        let body = format!(
            "verb: {}\ntargets: {}\nhow: {}\nmagnitude: {}/10\nwords: {}",
            a.intent.verb,
            if targets.is_empty() {
                "none".into()
            } else {
                targets.join(", ")
            },
            a.intent.how,
            a.intent.magnitude,
            a.intent.span
        );
        u.push_str(&format!(
            "Act {n} by {who}:\n{}\n",
            untrusted(&format!("ACT {n}", n = i + 1), &body),
            n = i + 1
        ));
    }
    u.push_str("\nReply with JSON: the effects (one per act, at most one accident; each text at most 12 words).");
    u
}

/// Reply cap: ~50 tokens per effect, one per act plus an accident; generous so a busy vicinity never truncates.
pub fn max_tokens(acts: usize) -> u32 {
    (300 + 60 * acts as u32).min(900)
}

/// Most effects kept from one reply: one per act plus two (an accident, a stray memorize).
pub fn max_effects(acts: usize) -> usize {
    acts + 2
}

pub fn build_request(ctx: &ResolverCtx) -> LlmRequest {
    request(
        &system_prompt(),
        &user_prompt(ctx),
        max_tokens(ctx.acts.len()),
        1.0,
    )
}

fn default_witnesses(ctx: &ResolverCtx) -> Vec<Ref> {
    let mut w: Vec<Ref> = ctx
        .present
        .iter()
        .map(|e| e.r)
        .filter(|r| matches!(r, Ref::Demigod(_) | Ref::Char(_) | Ref::Cluster(_)))
        .collect();
    for a in &ctx.acts {
        if !w.contains(&a.actor) && a.actor.is_actor() {
            w.push(a.actor);
        }
    }
    w
}

struct Draft<'a> {
    kind: ResKind,
    actor: Ref,
    target: Option<Target>,
    amount: f32,
    feel: Option<Feel>,
    text: &'a str,
    intent: Option<fn7_model::IntentId>,
    magnitude: u8,
    valence: i8,
    witnesses: Vec<Ref>,
}

fn proposals_for(ctx: &ResolverCtx, l: &Labels, d: Draft<'_>) -> Vec<Proposal> {
    let mk = |effect: Effect| Proposal {
        source: Source::Resolver,
        intent: d.intent,
        effect,
        act: line(d.text, 200),
        actor: d.actor,
        magnitude: d.magnitude,
        valence: d.valence,
        witnesses: d.witnesses.clone(),
        cause: None,
    };
    let note = || Effect::Note {
        text: line(d.text, 200),
    };
    let known = match &d.target {
        Some(Target::Known(r)) => Some(*r),
        _ => None,
    };
    let fx = match d.kind {
        ResKind::Note => note(),
        ResKind::Memorize => match &d.target {
            Some(Target::Mention(m)) => Effect::Memorize {
                mention: m.clone(),
                cluster: None,
            },
            _ => note(),
        },
        ResKind::Move => {
            let dest = known.and_then(|r| l.entity(r).cloned());
            let (to, place) = match dest {
                Some(e) => (
                    e.pos.unwrap_or(ctx.center),
                    match e.r {
                        Ref::Place(p) => Some(p),
                        _ => e.place.or(Some(ctx.place)),
                    },
                ),
                None => (ctx.center, Some(ctx.place)),
            };
            Effect::Move {
                who: d.actor,
                to,
                place,
            }
        }
        ResKind::Damage => match known {
            Some(Ref::Cluster(c)) => Effect::Shrink {
                cluster: c,
                count: (finite(d.amount, 0.0, 10.0) * 2.0).round().max(1.0) as u32,
            },
            Some(r) if r.is_actor() => Effect::Damage {
                who: r,
                amount: finite(d.amount, 0.0, 10.0),
                cause: line(d.text, 120),
            },
            _ => note(),
        },
        ResKind::Heal => match known {
            Some(r) if r.is_actor() => Effect::Heal {
                who: r,
                amount: finite(d.amount, 0.0, 10.0),
            },
            _ => note(),
        },
        ResKind::Mood | ResKind::Relation => {
            let a = finite(d.amount, -1.0, 1.0);
            match (known, d.feel) {
                (Some(Ref::Cluster(c)), f) => {
                    let f = match f {
                        Some(Feel::Trust) => Feel::Hope,
                        Some(Feel::Loyalty) => Feel::Devotion,
                        Some(f) => f,
                        None => Feel::Anger,
                    };
                    Effect::MoodShift {
                        cluster: c,
                        anger: if f == Feel::Anger { a } else { 0.0 },
                        fear: if f == Feel::Fear { a } else { 0.0 },
                        hope: if f == Feel::Hope { a } else { 0.0 },
                        devotion: if f == Feel::Devotion { a } else { 0.0 },
                    }
                }
                (Some(r), f) if r.is_actor() && r != d.actor => {
                    let f = f.unwrap_or(Feel::Trust);
                    Effect::Relation {
                        a: r,
                        b: d.actor,
                        trust: match f {
                            Feel::Trust | Feel::Hope | Feel::Devotion => a,
                            Feel::Anger => -a,
                            _ => 0.0,
                        },
                        fear: if f == Feel::Fear { a } else { 0.0 },
                        loyalty: if f == Feel::Loyalty { a } else { 0.0 },
                    }
                }
                _ => note(),
            }
        }
        ResKind::Join => match known {
            Some(Ref::Group(g)) => Effect::Join {
                member: d.actor,
                group: g,
            },
            _ => note(),
        },
        ResKind::Item => {
            let (name, desc) = d
                .text
                .split_once(':')
                .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
                .unwrap_or((d.text.to_string(), d.text.to_string()));
            let holder = match known {
                Some(r) if r.is_actor() => r,
                _ => d.actor,
            };
            Effect::CreateItem {
                name: line(&name, 40),
                description: line(&desc, 160),
                holder,
            }
        }
    };
    let mut out = Vec::new();
    // Addressing an unlisted person memorizes them, whatever the act.
    if let (Some(Target::Mention(m)), false) = (&d.target, d.kind == ResKind::Memorize) {
        out.push(Proposal {
            act: memorize_act(&l.name(d.actor), &m.text),
            ..mk(Effect::Memorize {
                mention: m.clone(),
                cluster: None,
            })
        });
    }
    out.push(mk(fx));
    out
}

pub fn convert(ctx: &ResolverCtx, r: ResolverReply) -> Result<ResolverOut, String> {
    let l = labels(ctx);
    let mut r = r;
    for e in r.effects.iter_mut() {
        e.text = l.unlabel(&e.text);
    }
    let everyone = default_witnesses(ctx);
    let mut proposals = Vec::new();
    for e in r.effects.iter().take(max_effects(ctx.acts.len())) {
        if e.text.trim().is_empty() {
            continue;
        }
        let act = (e.act >= 1)
            .then(|| ctx.acts.get(e.act as usize - 1))
            .flatten();
        let actor = l
            .resolve(&e.actor)
            .or(act.map(|a| a.actor))
            .unwrap_or(Ref::Place(ctx.place));
        let mut witnesses: Vec<Ref> = e
            .witnesses
            .iter()
            .filter_map(|w| l.resolve(w))
            .filter(|r| r.is_actor())
            .collect();
        if witnesses.is_empty() {
            witnesses = everyone.clone();
        }
        proposals.extend(proposals_for(
            ctx,
            &l,
            Draft {
                kind: e.kind,
                actor,
                target: e
                    .target
                    .as_deref()
                    .and_then(|t| l.target(t, Some(ctx.place))),
                amount: e.amount,
                feel: e.feel,
                text: &e.text,
                intent: act.map(|a| a.intent.id),
                magnitude: clamp_u8(e.magnitude, 0, 10),
                valence: clamp_i8(e.valence, -5, 5),
                witnesses,
            },
        ));
    }
    if proposals.is_empty() && !ctx.acts.is_empty() {
        return Err("no effects for the acts".into());
    }
    let mut lines: Vec<String> = r
        .effects
        .iter()
        .take(max_effects(ctx.acts.len()))
        .map(|e| line(&e.text, 200))
        .filter(|t| !t.is_empty())
        .collect();
    lines.dedup();
    let outcome = if lines.is_empty() {
        template_outcome(ctx, &l)
    } else {
        format!("{}.", lines.join(". ").trim_end_matches('.'))
    };
    Ok(ResolverOut { outcome, proposals })
}

fn template_outcome(ctx: &ResolverCtx, l: &Labels) -> String {
    let parts: Vec<String> = ctx
        .acts
        .iter()
        .map(|a| {
            let tgt = a.intent.targets.first().map(|t| match t {
                Target::Known(r) => l.name(*r),
                Target::Mention(m) => m.text.clone(),
            });
            format!(
                "{} goes to {}{}",
                l.name(a.actor),
                a.intent.verb,
                tgt.map(|t| format!(" {t}")).unwrap_or_default()
            )
        })
        .collect();
    format!("At {}: {}.", line(&ctx.place_name, 60), parts.join("; "))
}

/// Rules: moves happen, attacks deal median-ish damage by magnitude, talk shifts feelings, the rest is narrated; acts
/// without harmful intent have a seeded small chance of a Final Destination accident on the actor.
pub fn fallback(ctx: &ResolverCtx) -> ResolverOut {
    let l = labels(ctx);
    let everyone = default_witnesses(ctx);
    let mut proposals = Vec::new();
    for a in &ctx.acts {
        let i = &a.intent;
        let target = i.targets.first().cloned();
        let class = classify(&i.verb);
        let m = i.magnitude as f32;
        let who = l.name(a.actor);
        let tname = target.as_ref().map(|t| match t {
            Target::Known(r) => l.name(*r),
            Target::Mention(m) => m.text.clone(),
        });
        let tn = tname
            .clone()
            .unwrap_or_else(|| "nothing in particular".into());
        let (kind, amount, feel, text, valence) = match class {
            ActClass::Move => (ResKind::Move, 0.0, None, format!("{who} heads for {tn}"), 0),
            ActClass::Attack => (
                ResKind::Damage,
                (m / 2.0).round().clamp(1.0, 6.0),
                None,
                format!("{who} goes after {tn}"),
                -3,
            ),
            ActClass::Social => {
                let feel = if matches!(target, Some(Target::Known(Ref::Cluster(_)))) {
                    Feel::Hope
                } else {
                    Feel::Trust
                };
                (
                    ResKind::Relation,
                    (m * 0.04).clamp(0.05, 0.4),
                    Some(feel),
                    format!("{who} works on {tn}"),
                    1,
                )
            }
            ActClass::Scare => (
                ResKind::Relation,
                (m * 0.05).clamp(0.05, 0.5),
                Some(Feel::Fear),
                format!("{who} menaces {tn}"),
                -2,
            ),
            ActClass::Make => (
                ResKind::Item,
                0.0,
                None,
                format!(
                    "{}: something {who} turned up while trying to {}",
                    line(&i.verb, 20),
                    line(&i.verb, 20)
                ),
                1,
            ),
            ActClass::Other => (
                ResKind::Note,
                0.0,
                None,
                format!(
                    "{who} tries to {} {}",
                    line(&i.verb, 30),
                    tname.unwrap_or_default()
                )
                .trim()
                .to_string(),
                0,
            ),
        };
        let kind = if kind == ResKind::Relation
            && matches!(target, Some(Target::Known(Ref::Cluster(_))))
        {
            ResKind::Mood
        } else {
            kind
        };
        proposals.extend(proposals_for(
            ctx,
            &l,
            Draft {
                kind,
                actor: a.actor,
                target: target.clone(),
                amount,
                feel,
                text: &text,
                intent: Some(i.id),
                magnitude: i.magnitude,
                valence,
                witnesses: everyone.clone(),
            },
        ));
        if class != ActClass::Attack {
            let mut rng = Rng::derive(ctx.beat as u64, &[ctx.place.0 as u64, i.id.0 as u64]);
            if rng.below(ACCIDENT_ODDS) == 0 {
                let text = format!(
                    "a Final Destination moment: a loose sign, a wet floor and {who} meet at speed"
                );
                proposals.push(Proposal {
                    source: Source::Rules,
                    intent: Some(i.id),
                    effect: Effect::Damage {
                        who: a.actor,
                        amount: 1.0 + rng.below(3) as f32,
                        cause: text.clone(),
                    },
                    act: text,
                    actor: Ref::Place(ctx.place),
                    magnitude: 3,
                    valence: -2,
                    witnesses: everyone.clone(),
                    cause: None,
                });
            }
        }
    }
    ResolverOut {
        outcome: template_outcome(ctx, &l),
        proposals,
    }
}

pub async fn run(llm: &dyn LlmProvider, ctx: &ResolverCtx) -> Outcome<ResolverOut> {
    if ctx.acts.is_empty() {
        return Outcome::llm(ResolverOut {
            outcome: String::new(),
            proposals: vec![],
        });
    }
    let actor = ctx.place.to_string();
    let got = ask::<ResolverReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(
        STEP,
        Some(&actor),
        got,
        |r| convert(ctx, r),
        || fallback(ctx),
    )
}

/// A mention near this vicinity (for callers building acts by hand).
pub fn mention_here(ctx: &ResolverCtx, text: &str) -> Mention {
    Mention {
        text: line(text, 120),
        kind: guess_kind(text).into(),
        near: Some(ctx.place),
    }
}
