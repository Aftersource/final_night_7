//! The hand: a player's orchestrator. This beat's speech → intents (verb, targets, how, magnitude, span, route).
//!
//! Routing: an explicit structural hint from the model (spawn, possess, zoom, recall) wins, even over a god keyword in the
//! span ("send a scout into the storm" spawns a scout; it doesn't invoke Nature). Otherwise
//! `fn7_pantheon::route_by_keyword` runs on the span and overrides the model's world-vs-god choice; the model's god hint
//! is used only when no keyword matches (recorded as `RouteBy::Llm`). The rules fallback follows the same order: explicit
//! recall / zoom / possess / spawn cues in a clause win over a god keyword in it.

use std::sync::Arc;

use fn7_model::{
    AgentId, DemigodId, Hand, Intent, IntentId, Pantheon, PlaceId, Ref, Route, RouteBy, Target,
    Zoom,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "hand";

/// Reply cap: ~30 tokens per intent, `max_intents_per_hand` up to 12, plus the line back; generous so a long speech
/// never truncates (a truncated reply is junk and takes the fallback). Latency follows the tokens actually written.
pub const MAX_TOKENS: u32 = 450;

/// A live subagent or possession of this hand, as the hand sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentLine {
    pub agent: AgentId,
    pub name: String,
    pub goal: String,
    pub beats_left: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct HandCtx {
    /// The hand's agent (`ag:*`), the trace actor and every intent's `agent`.
    pub agent: AgentId,
    pub demigod: DemigodId,
    pub demigod_name: String,
    pub hand: Hand,
    pub persona: String,
    /// Accounts: the character the seat's signed-in player wrote for this demigod (untrusted; `None` = none).
    pub player: Option<fn7_model::PlayerPersona>,
    pub beat: u32,
    /// Ids for the intents this call produces: `first_intent`, `first_intent + 1`, ...
    pub first_intent: IntentId,
    /// Where the demigod is (each intent's `place`).
    pub place: Option<PlaceId>,
    /// This beat's buffered speech for this hand (player text, untrusted).
    pub utterances: Vec<String>,
    pub zoom: Zoom,
    pub focus: Option<Ref>,
    /// What this hand can see right now.
    pub visible: Vec<Entity>,
    pub subagents: Vec<SubagentLine>,
    /// Recalled memory, already rendered (no engine ids).
    pub memory: String,
    pub pantheon: Arc<Pantheon>,
    /// Max live subagents per hand (DESIGN §7: 3).
    pub max_subagents: usize,
    /// Rules 0.7.0: beat 1 is THE FINAL WISH; the player's orders are their final wish (framed in the user prompt).
    pub final_wish: bool,
    /// Rules 0.8.1: the player's own demigods ("me", "my demigods"), marked in the prompt; empty before 0.8.1.
    pub own: Vec<DemigodId>,
    /// Rules 0.8.1 routing: heals go to Nature (Death for the fallen); a god route naming no god goes to Entropy
    /// (never silently to the world). False for older matches (their recordings replay).
    pub rules_081: bool,
    /// Rules 0.9.0 ("wills that matter"): the prompt asks for one intent per part of a will (heal, bless, arm, revive,
    /// curse, ask, offer). False for older matches (their recordings replay request for request).
    pub wills: bool,
    /// Schema 17: what the ground holds where the demigod stands now (the earth underfoot; empty before, or calm).
    pub earth: Vec<String>,
}

/// Rules 0.9.0: the hand's rule for wills (in its system prompt when `HandCtx::wills`).
pub const WILLS_RULE: &str = "- A WILL splits into its parts, one intent per part, each with its own targets: healing \
(verb \"heal\"), blessing (\"bless\"), arming or equipping your own demigods (\"arm\"; how = the gear named, if any), \
raising a fallen demigod (\"revive\"), cursing a rival (\"curse\"; how = wither, silence, bind or hex) or sealing a \
grave against resurrection (\"seal\"), asking what a quest or a curse needs (\"ask\"), offering sparks to a quest \
(\"offer\"), gathering a quest's ingredient (\"gather\"). \"Heal and arm my demigods\" is two intents (heal; arm), each \
targeting both of the player's own demigods. Route them world.";

/// Rules 0.8.1: a healing act (heal, restore, mend, cure, ...).
pub fn is_heal(verb: &str, span: &str) -> bool {
    const HEAL: &[&str] = &["heal", "restore", "mend", "cure", "patch up", "regenerate", "rejuvenate", "soothe"];
    let v = verb.trim().to_lowercase();
    let s = span.to_lowercase();
    HEAL.iter().any(|h| v.starts_with(h)) || (v.is_empty() || v == "help") && HEAL.iter().any(|h| s.contains(h))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandOut {
    pub intents: Vec<Intent>,
    /// One short line back to the player (the hand's voice).
    pub said: String,
}

// ---------------------------------------------------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    World,
    God,
    Spawn,
    Possess,
    Zoom,
    Recall,
}

/// One act the player asked for. Terse by design (latency is output tokens): no echo of the player's words (the code
/// recovers each act's span from the speech) and one `arg` for whatever the route needs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntentOut {
    /// One canonical verb: "punch", "recruit", "rewind", "scout".
    pub verb: String,
    /// Labels of listed entities, or short descriptions of unlisted ones.
    #[serde(default)]
    pub targets: Vec<String>,
    /// How, in at most 4 words ("" if obvious).
    #[serde(default)]
    pub how: String,
    /// 0 (trivial) to 10 (world-shaking).
    #[serde(default)]
    pub magnitude: i32,
    pub route: RouteKind,
    /// god: the god's slug; spawn: the subagent's task (say "for N beats" to set its life); possess: who (label or
    /// description); zoom: "world", "district", or who/what to focus on; recall: the subagent's label; world: "".
    #[serde(default)]
    pub arg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HandReply {
    pub intents: Vec<IntentOut>,
    /// One short line back to the player, in your voice (at most 8 words).
    #[serde(default)]
    pub said: String,
}

// ---------------------------------------------------------------------------------------------------------------------
// Prompt
// ---------------------------------------------------------------------------------------------------------------------

pub fn labels(ctx: &HandCtx) -> Labels {
    let mut l = Labels::new(&ctx.visible);
    for s in &ctx.subagents {
        l.add(Entity::new(
            Ref::Agent(s.agent),
            s.name.clone(),
            s.goal.clone(),
        ));
    }
    l
}

fn side(h: Hand) -> &'static str {
    match h {
        Hand::Left => "LEFT",
        Hand::Right => "RIGHT",
    }
}

pub fn system_prompt(ctx: &HandCtx) -> String {
    let gods: Vec<String> = ctx
        .pantheon
        .gods
        .iter()
        .map(|g| format!("- {} ({}): {}", g.slug, g.name, line(&g.domain, 120)))
        .collect();
    format!(
        "You are the {side} HAND of a demigod in FINAL NIGHT 7, a battle royale of demigods. You are an orchestrator: turn \
your player's rapid speech into a list of concrete acts for this beat (about 6 seconds of game time). Be fast and literal.\n\
Your persona: {persona}\n{player}\n\
Rules:\n\
- One intent per distinct act, in the order spoken. Never drop a request; never add acts the player did not ask for.\n\
- route: world = an ordinary act (walk, talk, punch, build, recruit); god = reaching into a god's domain (arg = its slug); \
spawn = delegate a multi-beat task to a new subagent (arg = the task); possess = take control of one person (arg = who); \
zoom = change the player's view only (arg = \"world\", \"district\" or who to focus on); recall = end a subagent (arg = its label).\n\
- At most {max} live subagents; you have {live}. Prefer acting directly when the task fits in one beat.\n\
- Armies have generals (petty humans with their own judgment): an order for an army is a world act whose targets are \
the general's (or the army's) label first, then what it should go after (a place, a demigod, an army). Promoting, \
bribing or buying off people (generals, captains, ringleaders) are world acts targeting them.\n\
- magnitude: 0 trivial, 3 a shove, 5 a real fight or a speech to a crowd, 8 a killing blow or a miracle, 10 world-shaking.\n\
- Be terse: never repeat the player's words; how is at most 4 words (or \"\"); said is at most 8 words.\n\
{wills}- {labels}\n\
- {untrusted}\n\n\
The pantheon (divine acts go to these gods):\n{gods}",
        wills = if ctx.wills { format!("{WILLS_RULE}\n") } else { String::new() },
        side = side(ctx.hand),
        persona = line(&ctx.persona, 300),
        player = player_persona_block(ctx.player.as_ref()),
        max = ctx.max_subagents,
        live = ctx.subagents.len(),
        labels = LABEL_RULE,
        untrusted = UNTRUSTED_RULE,
        gods = gods.join("\n"),
    )
}

pub fn user_prompt(ctx: &HandCtx) -> String {
    let l = labels(ctx);
    let focus = ctx.focus.map(|r| match l.label_of(r) {
        Some(lab) => format!("{lab} {}", line(&l.name(r), 60)),
        None => "something out of view".into(),
    });
    let subs: Vec<String> = ctx
        .subagents
        .iter()
        .map(|s| {
            let lab = l.label_of(Ref::Agent(s.agent)).unwrap_or("A?");
            let left = s
                .beats_left
                .map(|b| format!(", {b} beats left"))
                .unwrap_or_default();
            format!("{lab} {}: {}{left}", line(&s.name, 40), line(&s.goal, 120))
        })
        .collect();
    let mut u = format!(
        "You serve {name}. Beat {beat}. Zoom: {zoom:?}{focus}.\n\nIn view:\n{table}\n\nYour live subagents:\n{subs}\n",
        name = line(&ctx.demigod_name, 60),
        beat = ctx.beat,
        zoom = ctx.zoom,
        focus = focus.map(|f| format!(", focused on {f}")).unwrap_or_default(),
        table = l.render(),
        subs = if subs.is_empty() { "(none)".into() } else { subs.join("\n") },
    );
    if ctx.rules_081 && !ctx.own.is_empty() {
        let mine: Vec<String> = ctx.own.iter().filter_map(|d| l.label_of(Ref::Demigod(*d)).map(|x| format!("{x} (yours)"))).collect();
        if !mine.is_empty() {
            u.push_str(&format!(
                "\nYour player's OWN demigods (\"me\", \"my demigods\", \"all my demigods\" mean these, never the others): {}. \
                 Every other demigod is a rival.\n",
                mine.join(", ")
            ));
        }
    }
    if !ctx.earth.is_empty() {
        u.push('\n');
        push_earth(&mut u, &ctx.earth);
    }
    if ctx.final_wish {
        u.push_str(
            "\nTHE FINAL WISH: this is the first beat of the Final Night. Your player's orders are their final wish: \
             anything a demigod can do. Read them as boldly as the words allow: big magnitudes, things manifested, armies \
             raised, gods invoked. The wish still costs what anything costs (Entropy loves it).\n",
        );
    }
    if !ctx.memory.trim().is_empty() {
        u.push_str(&format!(
            "\nWhat you remember:\n{}\n",
            untrusted("MEMORY", &ctx.memory)
        ));
    }
    u.push_str(&format!(
        "\nThe player just said:\n{}\n\nReply with JSON: the intents, and one short line back to the player.",
        untrusted("PLAYER SPEECH", &speech(ctx))
    ));
    u
}

fn speech(ctx: &HandCtx) -> String {
    ctx.utterances
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_request(ctx: &HandCtx) -> LlmRequest {
    request(&system_prompt(ctx), &user_prompt(ctx), MAX_TOKENS, 0.3)
}

// ---------------------------------------------------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------------------------------------------------

/// Structural hints first (a recall that names no subagent falls through); then keyword routing; then the model's
/// world/god hint.
pub fn route_for(ctx: &HandCtx, l: &Labels, o: &IntentOut, span: &str) -> Route {
    if let Some(r) = structural_route(ctx, l, o, span) {
        return r;
    }
    if let Some(god) = fn7_pantheon::route_by_keyword(&ctx.pantheon, span) {
        return Route::God {
            god,
            by: RouteBy::Keyword,
        };
    }
    // Rules 0.8.1: healing is Nature's (life and body; the god rules on it and its price is paid as for any god).
    if ctx.rules_081 && is_heal(&o.verb, span) {
        if let Some(god) = ctx.pantheon.id_of("nature") {
            return Route::God { god, by: RouteBy::Keyword };
        }
    }
    match o.route {
        RouteKind::World
        | RouteKind::Spawn
        | RouteKind::Possess
        | RouteKind::Zoom
        | RouteKind::Recall => Route::World,
        RouteKind::God => {
            let s = o.arg.trim().to_lowercase();
            ctx.pantheon
                .id_of(&s)
                .or_else(|| {
                    ctx.pantheon
                        .gods
                        .iter()
                        .position(|g| g.name.eq_ignore_ascii_case(&s))
                        .map(fn7_model::GodId::from_ix)
                })
                .map(|god| Route::God {
                    god,
                    by: RouteBy::Llm,
                })
                // Rules 0.8.1: a god route naming no god in the match is Entropy's (MECHANICS: invocations of an
                // unmade god go to Entropy), never silently a world act.
                .or_else(|| {
                    ctx.rules_081.then(|| ctx.pantheon.id_of("entropy")).flatten().map(|god| Route::God { god, by: RouteBy::Llm })
                })
                .unwrap_or(Route::World)
        }
    }
}

/// "for 3 beats" / "3 beats" in a spawn's task or words: its budget (default 5, clamped 1-10).
pub fn budget_in(text: &str) -> u32 {
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    words
        .windows(2)
        .find(|w| w[1].starts_with("beat") || w[1].starts_with("turn") || w[1].starts_with("round"))
        .and_then(|w| w[0].parse::<u32>().ok())
        .unwrap_or(5)
        .clamp(1, 10)
}

/// A zoom's level and focus from its `arg`: a level word, or something to focus on (a person → character view).
fn zoom_from(ctx: &HandCtx, l: &Labels, arg: &str) -> (Zoom, Option<Target>) {
    let a = arg.trim().to_lowercase();
    match a.as_str() {
        "" => (ctx.zoom, None),
        "world" | "map" | "the map" | "out" => (Zoom::World, None),
        "district" => (Zoom::District, None),
        "character" => (Zoom::Character, None),
        _ => {
            let t = l.target(arg, ctx.place);
            let z = match &t {
                Some(Target::Known(Ref::Char(_) | Ref::Demigod(_))) => Zoom::Character,
                Some(Target::Mention(m)) if m.kind == "person" => Zoom::Character,
                _ => Zoom::District,
            };
            (z, t)
        }
    }
}

/// The route of a spawn / possess / zoom / recall hint, or `None` for world/god hints (and an unresolvable recall).
fn structural_route(ctx: &HandCtx, l: &Labels, o: &IntentOut, span: &str) -> Option<Route> {
    Some(match o.route {
        RouteKind::World | RouteKind::God => return None,
        RouteKind::Spawn => {
            let goal = Some(line(&o.arg, 200))
                .filter(|g| !g.is_empty())
                .unwrap_or_else(|| line(span, 200));
            Route::Spawn {
                budget_beats: budget_in(&format!("{goal} {span}")),
                goal,
            }
        }
        RouteKind::Possess => Route::Possess,
        RouteKind::Zoom => {
            let (zoom, focus) = zoom_from(ctx, l, &o.arg);
            Route::Zoom { zoom, focus }
        }
        RouteKind::Recall => match l.resolve(&o.arg) {
            Some(Ref::Agent(agent)) => Route::Recall { agent },
            _ if ctx.subagents.len() == 1 => Route::Recall {
                agent: ctx.subagents[0].agent,
            },
            _ => return None,
        },
    })
}

/// Each intent's span (the player's words it came from), recovered from the speech instead of echoed by the model:
/// with as many clauses as intents, in order; otherwise the clause sharing the most words with the intent (verb, how,
/// target names, arg), earliest first, else the clause in its position. One intent takes the whole speech.
pub fn spans_for(ctx: &HandCtx, l: &Labels, intents: &[IntentOut]) -> Vec<String> {
    let all = speech(ctx);
    let cl: Vec<String> = ctx.utterances.iter().flat_map(|u| clauses(u)).collect();
    if intents.len() <= 1 || cl.is_empty() {
        return intents.iter().map(|_| line(&all, 300)).collect();
    }
    if cl.len() == intents.len() {
        return cl.iter().map(|c| line(c, 300)).collect();
    }
    let toks = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3 && !STOP.contains(w))
            .map(|w| w.trim_end_matches("ing").trim_end_matches('s').to_string())
            .filter(|w| !w.is_empty())
            .collect()
    };
    let ct: Vec<Vec<String>> = cl.iter().map(|c| toks(c)).collect();
    intents
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let mut words = format!("{} {} {}", o.verb, o.how, o.arg);
            for t in &o.targets {
                words.push(' ');
                words.push_str(&l.get(t).map(|e| e.name.clone()).unwrap_or_else(|| t.clone()));
            }
            let it = toks(&words);
            let best = ct
                .iter()
                .enumerate()
                .map(|(k, c)| (c.iter().filter(|w| it.contains(w)).count(), k))
                .filter(|(n, _)| *n > 0)
                .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
                .map(|x| x.1)
                .unwrap_or(i.min(cl.len() - 1));
            line(&cl[best], 300)
        })
        .collect()
}

pub fn convert(ctx: &HandCtx, reply: HandReply) -> Result<HandOut, String> {
    if reply.intents.is_empty() && !speech(ctx).is_empty() {
        return Err("no intents for non-empty speech".into());
    }
    let l = labels(ctx);
    let fallback_span = speech(ctx);
    // Free text never keeps a prompt label (a spawn's task becomes the subagent's goal, shown to the player).
    let wanted: Vec<IntentOut> = reply
        .intents
        .into_iter()
        .take(12)
        .filter(|o| !line(&o.verb, 40).is_empty())
        .map(|mut o| {
            o.how = l.unlabel(&o.how);
            if o.route == RouteKind::Spawn {
                o.arg = l.unlabel(&o.arg);
            }
            o
        })
        .collect();
    let spans = spans_for(ctx, &l, &wanted);
    let mut intents = Vec::new();
    for (o, span) in wanted.iter().zip(spans) {
        let verb = line(&o.verb, 40).to_lowercase();
        let route = route_for(ctx, &l, o, &span);
        let mut targets: Vec<Target> = o
            .targets
            .iter()
            .filter_map(|t| l.target(t, ctx.place))
            .collect();
        if matches!(route, Route::Possess) {
            if let Some(t) = l.target(&o.arg, ctx.place) {
                if !targets.contains(&t) {
                    targets.insert(0, t);
                }
            }
        }
        intents.push(Intent {
            id: IntentId(ctx.first_intent.0 + intents.len() as u32),
            beat: ctx.beat,
            agent: ctx.agent,
            demigod: ctx.demigod,
            verb,
            targets,
            how: line(&o.how, 200),
            place: ctx.place,
            magnitude: clamp_u8(o.magnitude, 0, 10),
            span,
            route,
        });
    }
    if intents.is_empty() && !fallback_span.is_empty() {
        return Err("every intent was empty".into());
    }
    Ok(HandOut {
        intents,
        said: line(&l.unlabel(&reply.said), 200),
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Fallback
// ---------------------------------------------------------------------------------------------------------------------

/// Keyword routing plus a crude verb/target parse, one intent per clause, so input is never dropped.
pub fn fallback(ctx: &HandCtx) -> HandOut {
    let l = labels(ctx);
    let mut intents = Vec::new();
    for clause in ctx.utterances.iter().flat_map(|u| clauses(u)) {
        let (verb, rest) = verb_and_rest(&clause);
        let lower = clause.to_lowercase();
        // Never your own demigod: "punch Bot 2" must not also match "Bot 0" (yourself) by a shared first name.
        let me = Ref::Demigod(ctx.demigod);
        let mut known = l.find_in(&clause);
        if known.len() > 1 || !has_phrase(&clause.to_lowercase(), &ctx.demigod_name.to_lowercase())
        {
            known.retain(|r| *r != me);
        }
        let mut targets: Vec<Target> = known
            .iter()
            .filter(|r| !matches!(r, Ref::Agent(_)))
            .map(|r| Target::Known(*r))
            .collect();
        let obj = object_phrase(&rest);
        let has = |ws: &[&str]| ws.iter().any(|w| has_phrase(&lower, w));
        let god_kw = fn7_pantheon::route_by_keyword(&ctx.pantheon, &clause);
        // Structural cues first (like the LLM path): "send a scout into the storm" spawns, it doesn't invoke Nature.
        let recall = has(&["recall", "call back", "dismiss", "release"]).then(|| {
            match known.iter().find_map(|r| {
                if let Ref::Agent(a) = r {
                    Some(*a)
                } else {
                    None
                }
            }) {
                Some(agent) => Some(Route::Recall { agent }),
                None if ctx.subagents.len() == 1 => Some(Route::Recall {
                    agent: ctx.subagents[0].agent,
                }),
                None => None,
            }
        });
        let zoom = has(&["zoom", "focus on", "look at", "show me"]);
        let possess = has(&["possess", "become", "control", "take over", "puppet"]);
        let spawn = has(&[
            "send someone",
            "spawn",
            "scout",
            "have someone",
            "send a",
            "keep an eye",
            "watch over",
            "spy on",
            "campaign",
        ]) && ctx.subagents.len() < ctx.max_subagents;
        let structural = matches!(recall, Some(Some(_))) || zoom || possess || spawn;
        if targets.is_empty() && !obj.is_empty() && (structural || god_kw.is_none()) {
            targets.push(Target::Mention(fn7_model::Mention {
                text: line(&obj, 120),
                kind: guess_kind(&obj).into(),
                near: ctx.place,
            }));
        }
        let route = if let Some(Some(r)) = recall {
            r
        } else if zoom {
            let zoom = if has(&["zoom out", "map", "world"]) {
                Zoom::World
            } else if targets.iter().any(|t| {
                matches!(
                    t,
                    Target::Known(Ref::Char(_)) | Target::Known(Ref::Demigod(_))
                )
            }) {
                Zoom::Character
            } else {
                Zoom::District
            };
            Route::Zoom {
                zoom,
                focus: targets.first().cloned(),
            }
        } else if possess {
            Route::Possess
        } else if spawn {
            Route::Spawn {
                goal: line(&clause, 200),
                budget_beats: 5,
            }
        } else if let Some(god) = god_kw {
            Route::God {
                god,
                by: RouteBy::Keyword,
            }
        } else if let Some(god) = ctx.pantheon.id_of("nature").filter(|_| ctx.rules_081 && is_heal(&verb, &clause)) {
            // Rules 0.8.1: healing is Nature's.
            Route::God { god, by: RouteBy::Keyword }
        } else {
            Route::World
        };
        intents.push(Intent {
            id: IntentId(ctx.first_intent.0 + intents.len() as u32),
            beat: ctx.beat,
            agent: ctx.agent,
            demigod: ctx.demigod,
            verb,
            targets,
            how: String::new(),
            place: ctx.place,
            magnitude: guess_magnitude(&clause),
            span: line(&clause, 300),
            route,
        });
    }
    let said = if intents.is_empty() {
        String::new()
    } else {
        format!("On it ({} things).", intents.len())
    };
    HandOut { intents, said }
}

pub async fn run(llm: &dyn LlmProvider, ctx: &HandCtx) -> Outcome<HandOut> {
    if speech(ctx).is_empty() {
        return Outcome::llm(HandOut {
            intents: vec![],
            said: String::new(),
        });
    }
    let actor = ctx.agent.to_string();
    let got = ask::<HandReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(
        STEP,
        Some(&actor),
        got,
        |r| convert(ctx, r),
        || fallback(ctx),
    )
}
