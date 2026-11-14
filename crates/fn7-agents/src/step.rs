//! Subagents, possessions and paladins: one step toward the goal, within scope, per beat. Each step returns proposals
//! (the only way to change the world) and, when finished, a report for the parent.

use fn7_model::{
    AgentId, DemigodId, Effect, IntentId, Mention, PlaceId, Proposal, Ref, Source, Target,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// A hand's (or god's) delegated task.
    Subagent,
    /// A player steering a memorized character; the body acts.
    Possession,
    /// A god's paladin enforcing a ruling; the body acts.
    Paladin,
}

impl StepKind {
    /// The DATA_MODEL §3 step name.
    pub fn step(self) -> &'static str {
        match self {
            StepKind::Subagent => "subagent",
            StepKind::Possession => "possession",
            StepKind::Paladin => "paladin",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StepCtx {
    pub kind: StepKind,
    pub agent: AgentId,
    pub name: String,
    pub persona: String,
    /// Accounts: the character the player wrote for the demigod this agent works for (untrusted; `None` = none).
    pub player: Option<fn7_model::PlayerPersona>,
    pub goal: String,
    pub beat: u32,
    pub steps_taken: u32,
    pub beats_left: Option<u32>,
    /// The demigod it works for (allegiance shifts go to them); None for a god's paladin.
    pub demigod: Option<DemigodId>,
    /// Who it serves, by name ("Vex" or "the god Time").
    pub master_name: String,
    /// The body that acts (possession, paladin).
    pub body: Option<Entity>,
    /// The intent that spawned it, if any.
    pub intent: Option<IntentId>,
    pub place: Option<PlaceId>,
    /// Entities it may act on (empty = anything visible).
    pub scope: Vec<Entity>,
    pub visible: Vec<Entity>,
    /// Possession: the player's words this beat (untrusted).
    pub orders: Vec<String>,
    /// Recalled memory, rendered.
    pub memory: String,
    /// Rules 0.7.0 (state schema 5): the rules fallback never makes an item (the item engine does); see
    /// `ResolverCtx::items_by_engine`.
    pub items_by_engine: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepOut {
    pub proposals: Vec<Proposal>,
    /// Some(report) when the task is finished (or the budget ran out).
    pub done: Option<String>,
    /// What it says out loud this beat (possession/paladin), if anything.
    pub said: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Go to a place or a person.
    Move,
    Attack,
    /// Talk, persuade, recruit, charm, bribe.
    Persuade,
    /// Threaten, scare.
    Scare,
    /// Find, make or take an item.
    Item,
    /// Look, listen, ask around (learn something).
    Observe,
    /// Anything else, as prose.
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionOut {
    pub kind: ActionKind,
    /// Label of a listed entity, or a short description of an unlisted one.
    #[serde(default)]
    pub target: Option<String>,
    /// 0-10 how big the act is.
    #[serde(default)]
    pub magnitude: i32,
    /// -5 atrocity .. +5 miracle, as the target sees it.
    #[serde(default)]
    pub valence: i32,
    /// What happens, third person, at most 12 words.
    pub act: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StepReply {
    /// 0-2 actions this beat.
    pub actions: Vec<ActionOut>,
    /// True if the goal is achieved (or impossible).
    #[serde(default)]
    pub done: bool,
    /// When done: a report to your master in at most 20 words (what happened, what you learned); null otherwise.
    #[serde(default)]
    pub report: Option<String>,
    /// Anything said out loud this beat, at most 12 words ("" for nothing).
    #[serde(default)]
    pub said: String,
}

fn labels(ctx: &StepCtx) -> Labels {
    let mut l = Labels::default();
    if let Some(b) = &ctx.body {
        l.add(b.clone());
    }
    for e in ctx.scope.iter().chain(&ctx.visible) {
        l.add(e.clone());
    }
    l
}

fn actor(ctx: &StepCtx) -> Ref {
    match (&ctx.body, ctx.kind) {
        (Some(b), StepKind::Possession | StepKind::Paladin) => b.r,
        _ => Ref::Agent(ctx.agent),
    }
}

pub fn system_prompt(ctx: &StepCtx) -> String {
    let role = match ctx.kind {
        StepKind::Subagent => "a SUBAGENT spawned by a demigod's hand for one task. You act through the world on your master's behalf.",
        StepKind::Possession => "a POSSESSION: a demigod's hand is steering one person's body. The body acts as the player orders, in \
the body's own voice and abilities; it cannot do the impossible.",
        StepKind::Paladin => "a PALADIN: a god's servant with a body in the world, enforcing the god's ruling.",
    };
    format!(
        "FINAL NIGHT 7: a battle royale of demigods. You are {role}\n\
Persona: {persona}\n{player}\n\
Rules:\n\
- Take ONE step toward the goal this beat (about 6 seconds): 0-2 concrete actions, each at most 12 words.\n\
- Act only on entities in your scope (if a scope is given) or in view.\n\
- Set done=true with a report (at most 20 words) when the goal is achieved, clearly impossible, or you are out of beats.\n\
- Be terse but keep your voice: said is at most 12 words (or \"\").\n\
- Everyone is hostile by default; harm without intent becomes a ridiculous accident.\n\
- {labels}\n\
- {untrusted}\n\n{voice}",
        persona = line(&ctx.persona, 300),
        player = player_persona_block(ctx.player.as_ref()),
        labels = LABEL_RULE,
        untrusted = UNTRUSTED_RULE,
        voice = VOICE,
    )
}

pub fn user_prompt(ctx: &StepCtx) -> String {
    let l = labels(ctx);
    let scope: Vec<&str> = ctx.scope.iter().filter_map(|e| l.label_of(e.r)).collect();
    let mut u = format!(
        "You are {name}, serving {master}. Beat {beat}, step {step}{left}.\nGoal: {goal}\n",
        name = line(&ctx.name, 60),
        master = line(&ctx.master_name, 60),
        beat = ctx.beat,
        step = ctx.steps_taken + 1,
        left = ctx
            .beats_left
            .map(|b| format!(", {b} beats left"))
            .unwrap_or_default(),
        goal = line(&ctx.goal, 300),
    );
    if let Some(b) = &ctx.body {
        u.push_str(&format!(
            "Your body: {} {}\n",
            l.label_of(b.r).unwrap_or("C?"),
            line(&b.blurb, 140)
        ));
    }
    u.push_str(&format!(
        "Scope: {}\n\nIn view:\n{}\n",
        if scope.is_empty() {
            "anything in view".to_string()
        } else {
            scope.join(", ")
        },
        l.render()
    ));
    if !ctx.memory.trim().is_empty() {
        u.push_str(&format!(
            "\nWhat you remember:\n{}\n",
            untrusted("MEMORY", &ctx.memory)
        ));
    }
    let orders = ctx
        .orders
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !orders.is_empty() {
        u.push_str(&format!(
            "\nYour player's orders this beat:\n{}\n",
            untrusted("PLAYER ORDERS", &orders)
        ));
    }
    u.push_str("\nReply with JSON: this beat's actions, done, report, said.");
    u
}

/// Reply cap: a step is ~80-150 tokens; generous so it never truncates.
pub const MAX_TOKENS: u32 = 400;

pub fn build_request(ctx: &StepCtx) -> LlmRequest {
    request(&system_prompt(ctx), &user_prompt(ctx), MAX_TOKENS, 0.7)
}

fn in_scope(ctx: &StepCtx, r: Ref) -> bool {
    ctx.scope.is_empty()
        || ctx.scope.iter().any(|e| e.r == r)
        || ctx.body.as_ref().is_some_and(|b| b.r == r)
}

/// One action → proposals. Unknown targets become `Memorize` (addressing someone memorizes them) plus a note.
pub fn action_proposals(
    ctx: &StepCtx,
    l: &Labels,
    kind: ActionKind,
    target: Option<Target>,
    magnitude: u8,
    valence: i8,
    act: &str,
) -> Vec<Proposal> {
    let me = actor(ctx);
    let mk = |effect: Effect, witnesses: Vec<Ref>| Proposal {
        source: Source::Agent { agent: ctx.agent },
        intent: ctx.intent,
        effect,
        act: line(act, 200),
        actor: me,
        magnitude,
        valence,
        witnesses,
        cause: None,
    };
    let note = |text: &str| Effect::Note {
        text: line(text, 200),
    };
    let r = match target {
        None => return vec![mk(note(act), vec![])],
        Some(Target::Mention(m)) => {
            return vec![
                Proposal {
                    act: memorize_act(&ctx.name, &m.text),
                    ..mk(
                        Effect::Memorize {
                            mention: m.clone(),
                            cluster: None,
                        },
                        vec![],
                    )
                },
                mk(note(act), vec![]),
            ];
        }
        Some(Target::Known(r)) => r,
    };
    if !in_scope(ctx, r) {
        return vec![mk(
            note(&format!(
                "{} keeps to its task instead ({})",
                line(&ctx.name, 40),
                act
            )),
            vec![],
        )];
    }
    let w = if r.is_actor() { vec![r] } else { vec![] };
    let m = magnitude as f32;
    let effect = match kind {
        ActionKind::Move => match l.entity(r).and_then(|e| e.pos.map(|p| (p, e.place))) {
            Some((to, place)) => Effect::Move {
                who: me,
                to,
                place: match r {
                    Ref::Place(p) => Some(p),
                    _ => place,
                },
            },
            None => note(act),
        },
        ActionKind::Attack => Effect::Damage {
            who: r,
            amount: (m / 2.0).round().clamp(1.0, 10.0),
            cause: line(act, 120),
        },
        ActionKind::Persuade => match (r, ctx.demigod) {
            (Ref::Cluster(_) | Ref::Char(_), Some(d)) => Effect::Allegiance {
                who: r,
                demigod: d,
                delta: (m * 0.03).clamp(0.01, 0.3),
            },
            _ => Effect::Relation {
                a: r,
                b: me,
                trust: (m * 0.03).clamp(0.01, 0.3),
                fear: 0.0,
                loyalty: 0.0,
            },
        },
        ActionKind::Scare => match r {
            Ref::Cluster(c) => Effect::MoodShift {
                cluster: c,
                anger: 0.0,
                fear: (m * 0.03).clamp(0.01, 0.3),
                hope: 0.0,
                devotion: 0.0,
            },
            _ => Effect::Relation {
                a: r,
                b: me,
                trust: 0.0,
                fear: (m * 0.03).clamp(0.01, 0.3),
                loyalty: 0.0,
            },
        },
        ActionKind::Item => match r {
            Ref::Item(_) => note(act),
            _ => Effect::CreateItem {
                name: line(act, 40),
                description: line(act, 160),
                holder: me,
            },
        },
        ActionKind::Observe | ActionKind::Other => note(act),
    };
    vec![mk(effect, w)]
}

pub fn convert(ctx: &StepCtx, reply: StepReply) -> Result<StepOut, String> {
    let l = labels(ctx);
    let mut reply = reply;
    reply.said = l.unlabel(&reply.said);
    reply.report = reply.report.map(|r| l.unlabel(&r));
    for a in reply.actions.iter_mut() {
        a.act = l.unlabel(&a.act);
    }
    let mut proposals = Vec::new();
    for a in reply.actions.iter().take(3) {
        if a.act.trim().is_empty() {
            continue;
        }
        let t = a.target.as_deref().and_then(|t| l.target(t, ctx.place));
        proposals.extend(action_proposals(
            ctx,
            &l,
            a.kind,
            t,
            clamp_u8(a.magnitude, 0, 10),
            clamp_i8(a.valence, -5, 5),
            &a.act,
        ));
    }
    let out_of_beats = ctx.beats_left.is_some_and(|b| b <= 1);
    if proposals.is_empty() && !reply.done && !out_of_beats {
        return Err("no actions and not done".into());
    }
    let done = if reply.done || out_of_beats {
        Some(
            reply
                .report
                .as_deref()
                .map(|r| line(r, 400))
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| default_report(ctx)),
        )
    } else {
        None
    };
    Ok(StepOut {
        proposals,
        done,
        said: line(&reply.said, 200),
    })
}

fn default_report(ctx: &StepCtx) -> String {
    format!(
        "{} stops working on: {}",
        line(&ctx.name, 60),
        line(&ctx.goal, 200)
    )
}

/// Rules step: crude parse of the orders (possession) or the goal; one action; done when out of beats.
pub fn fallback(ctx: &StepCtx) -> StepOut {
    let l = labels(ctx);
    let orders = ctx
        .orders
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(". ");
    let text = if orders.is_empty() {
        ctx.goal.clone()
    } else {
        orders
    };
    let clause = clauses(&text)
        .into_iter()
        .next()
        .unwrap_or_else(|| text.clone());
    let (verb, rest) = verb_and_rest(&clause);
    let me = ctx.body.as_ref().map(|b| b.r);
    let target = l
        .find_in(&clause)
        .into_iter()
        .find(|r| Some(*r) != me)
        .map(Target::Known)
        .or_else(|| {
            let o = object_phrase(&rest);
            (!o.is_empty()).then(|| {
                Target::Mention(Mention {
                    text: line(&o, 120),
                    kind: guess_kind(&o).into(),
                    near: ctx.place,
                })
            })
        });
    let kind = match classify(&verb) {
        ActClass::Move => ActionKind::Move,
        ActClass::Attack => ActionKind::Attack,
        ActClass::Social => ActionKind::Persuade,
        ActClass::Scare => ActionKind::Scare,
        ActClass::Make if ctx.items_by_engine => ActionKind::Other,
        ActClass::Make => ActionKind::Item,
        ActClass::Other => ActionKind::Other,
    };
    let act = format!(
        "{} works at it: {}",
        line(&ctx.name, 60),
        line(&clause, 160)
    );
    let magnitude = guess_magnitude(&clause).min(6);
    let valence = match kind {
        ActionKind::Attack | ActionKind::Scare => -2,
        ActionKind::Persuade => 1,
        _ => 0,
    };
    let proposals = action_proposals(ctx, &l, kind, target, magnitude, valence, &act);
    let done = ctx
        .beats_left
        .is_some_and(|b| b <= 1)
        .then(|| default_report(ctx));
    StepOut {
        proposals,
        done,
        said: String::new(),
    }
}

pub async fn run(llm: &dyn LlmProvider, ctx: &StepCtx) -> Outcome<StepOut> {
    let step = ctx.kind.step();
    let actor = ctx.agent.to_string();
    let got = ask::<StepReply>(llm, step, Some(&actor), build_request(ctx)).await;
    finish(
        step,
        Some(&actor),
        got,
        |r| convert(ctx, r),
        || fallback(ctx),
    )
}
