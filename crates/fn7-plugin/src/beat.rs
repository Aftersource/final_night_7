//! `resolve_round` = one beat: intake → mentions → route → act → merge → consequence → narrate → broadcast.
//! Every stage is timed; the `beat` event carries the wall-clock timings (the state keeps only the deterministic counts,
//! so a replay reproduces every snapshot byte for byte).

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::{select, Either};

use fn7_agents::common::Fallback;
use fn7_agents::fold::FoldCtx;
use fn7_agents::god::GodCtx;
use fn7_agents::hand::{HandCtx, HandOut, SubagentLine};
use fn7_agents::materialize::MaterializeCtx;
use fn7_agents::narrator::{Narration, NarratorCtx};
use fn7_agents::resolver::{ActLine, ResolverCtx};
use fn7_agents::step::{StepCtx, StepKind};
use fn7_agents::{Entity, MindCall, MindReply, Outcome, RunReport};
use fn7_memory::NewMemory;
use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::{LlmProvider, Seat};

use crate::actions::{resolve_focus, Input};
use crate::merge;
use crate::settings::Settings;
use crate::world::*;

/// Run a batch of minds: concurrently under the per-call timeout, or (no LLM) straight to every fallback.
pub async fn run_batch(
    llm: Option<&dyn LlmProvider>,
    calls: &[MindCall],
    timeout: Duration,
) -> RunReport {
    match llm {
        Some(l) if !calls.is_empty() => fn7_agents::run_minds(l, calls, timeout, 0).await,
        _ => RunReport {
            replies: calls
                .iter()
                .map(|c| Outcome::fell_back(c.fallback(), Fallback::Error("llm off".into())))
                .collect(),
            fell_back: (0..calls.len()).collect(),
            rules_only: 0,
        },
    }
}

/// The background batch of a beat: last beat's narrator and the LLM memory folds, started when the beat starts and
/// driven alongside the hands and the act batch, so it never adds to the beat unless it outlasts both.
type Background<'a> = Pin<Box<dyn Future<Output = RunReport> + Send + 'a>>;

/// Await `main` while also driving `bg` (which keeps its progress if `main` finishes first); `bg`'s report lands in
/// `got` when it completes. Where the background's results are applied is fixed by the caller (the narrate stage), so
/// timing never changes the state.
async fn overlap<T>(
    main: impl Future<Output = T>,
    bg: &mut Background<'_>,
    got: &mut Option<RunReport>,
) -> T {
    let main = std::pin::pin!(main);
    if got.is_some() {
        return main.await;
    }
    match select(main, bg).await {
        Either::Left((r, _)) => r,
        Either::Right((rep, main)) => {
            *got = Some(rep);
            main.await
        }
    }
}

/// Owners over their memory budget (ref order) whose fold the fold mind writes: the first `k` with a fold plan.
fn llm_fold_plans(w: &World, per_owner: usize, k: usize) -> Vec<fn7_memory::FoldPlan> {
    let mut counts: BTreeMap<Ref, usize> = BTreeMap::new();
    for m in w.memory.items.iter().filter(|m| fn7_memory::is_active(m)) {
        *counts.entry(m.owner).or_default() += 1;
    }
    counts
        .into_iter()
        // Minor people (smaller budgets) always fold by rules: the fold mind is for the people the story is about.
        .filter(|(o, n)| *n > per_owner && budget_of(w, *o, per_owner) == per_owner)
        .filter_map(|(owner, _)| fn7_memory::fold_plan(&w.memory, owner, per_owner / 2, per_owner))
        .take(k)
        .collect()
}

/// `fn7_memory::enforce_budget` (rules folds, keeping the newest half) for every owner except `skip` (owners whose
/// fold the fold mind writes next beat, off the critical path).
fn enforce_budget_except(w: &mut World, per_owner: usize, beat: u32, skip: &[Ref]) -> Vec<MemoryId> {
    let per_owner = per_owner.max(2);
    let mut counts: BTreeMap<Ref, usize> = BTreeMap::new();
    for m in w.memory.items.iter().filter(|m| fn7_memory::is_active(m)) {
        *counts.entry(m.owner).or_default() += 1;
    }
    let mut out = vec![];
    for (owner, n) in counts {
        let per_owner = budget_of(w, owner, per_owner).max(2);
        if n <= per_owner || skip.contains(&owner) {
            continue;
        }
        if let Some(plan) = fn7_memory::fold_plan(&w.memory, owner, per_owner / 2, per_owner) {
            let text = fn7_memory::rules_summary(w, &plan);
            out.push(fn7_memory::apply_fold(&mut w.memory, &plan, &text, beat));
        }
    }
    out
}

/// What the narrator is told about beat `b`, from the state after it (run during beat `b + 1`, or in the summary): the
/// applied rows with the effect that landed and on whom, the public rulings, who fell, who stands.
pub fn narrator_ctx(w: &World, b: u32) -> NarratorCtx {
    let mut rows: Vec<&LedgerRow> = w
        .ledger
        .rows
        .iter()
        .rev()
        .take_while(|r| r.beat >= b)
        .filter(|r| r.beat == b)
        .collect();
    rows.reverse();
    let rows: Vec<String> = rows
        .into_iter()
        .filter(|r| {
            r.magnitude >= 4
                || matches!(
                    r.verb.as_str(),
                    "collect" | "eliminate" | "revive" | "dispatch"
                )
        })
        .take(12)
        .map(|r| {
            let tg: Vec<String> = r
                .targets
                .iter()
                .filter(|t| !matches!(t, Ref::Ledger(_)))
                .map(|t| w.name_of(*t))
                .collect();
            if tg.is_empty() {
                format!("{} [{}]", r.act, r.verb)
            } else {
                format!("{} [{}: {}]", r.act, r.verb, tg.join(", "))
            }
        })
        .collect();
    let fell_in = |x: u32| -> Vec<DemigodId> {
        w.beats
            .iter()
            .filter(|r| r.beat == x)
            .flat_map(|r| r.eliminated.iter().copied())
            .collect()
    };
    let eliminated_ids = fell_in(b);
    let later: Vec<DemigodId> = w
        .beats
        .iter()
        .filter(|r| r.beat > b)
        .flat_map(|r| r.eliminated.iter().copied())
        .collect();
    let hp = |x: f32| (x * 10.0).round() / 10.0;
    let mut standing = vec![];
    let mut survivors = vec![];
    let mut out_before = vec![];
    for d in &w.demigods {
        if d.alive {
            standing.push(format!("{} (health {}/{})", d.name, hp(d.health), hp(d.max_health)));
            survivors.push(d.name.clone());
        } else if later.contains(&d.id) {
            standing.push(d.name.clone());
            survivors.push(d.name.clone());
        } else if !eliminated_ids.contains(&d.id) {
            out_before.push(d.name.clone());
        }
    }
    NarratorCtx {
        lens: w.dm_rules(),
        beat: b,
        beats_total: w.meta.beats_total,
        alive_demigods: standing.len() as u32,
        rows,
        rulings: w
            .rulings
            .iter()
            .filter(|r| r.beat == b && ruling_is_public(w, r))
            .map(|r| (w.name_of(Ref::God(r.god)), r.speech.clone()))
            .collect(),
        eliminated: eliminated_ids
            .iter()
            .map(|d| w.demigods[d.ix()].name.clone())
            .collect(),
        standing,
        out_before,
        survivors,
        champion: None,
        world: narrator_world(w, b),
        society: crate::society::narrator_lines(w, b, 6),
    }
}

/// Rules 0.7.0: beat 1 of a schema-5 match is THE FINAL WISH: each player's orders are their final wish (anything they
/// can do): both hands get `max_intents_per_window` acts, every act gets `final_wish_magnitude` more magnitude, the
/// hand and resolver prompts frame it, the narrator calls them wishes, and the built-in AI makes one too. Every wish
/// costs as normal.
pub fn is_final_wish(w: &World, beat: u32) -> bool {
    beat == 1 && w.two_bodies()
}

/// The resolver's and narrator's line for the Final Wish beat.
pub const FINAL_WISH_LINE: &str = "THE FINAL WISH: this first beat, every demigod's orders are their final wish; grant \
them grandly (they still cost what anything costs) and call them wishes.";

/// The narrator's world lines for beat `b`: the Entropy line, who slowed time, heat death.
fn narrator_world(w: &World, b: u32) -> Vec<String> {
    let mut v = vec![fn7_society::entropy_prose(w)];
    if is_final_wish(w, b) {
        v.push(FINAL_WISH_LINE.into());
    }
    for d in w.demigods.iter().filter(|d| d.slows.contains(&b)) {
        v.push(format!("{} slowed time this beat.", d.name));
    }
    if w.meta.heat_death == Some(b) {
        v.push("HEAT DEATH: Entropy hit its maximum; this is the Final Night, all debts fall due, one survives.".into());
    }
    v
}

/// Was beat `b` loud (someone spoke, a power was cast, someone fell, or the society did something notable: a big
/// battle, a mutiny, a defection, a general's death)? Its narrator then uses the LLM (one cheap call); quiet beats get
/// the template, captioned from the society's own action.
pub fn beat_was_loud(w: &World, b: u32) -> bool {
    w.beats.iter().any(|r| r.beat == b && r.words > 0)
        || w.ledger
            .rows
            .iter()
            .rev()
            .take_while(|r| r.beat >= b)
            .any(|r| {
                r.beat == b
                    && (r.verb == "cast"
                        || r.verb == "eliminate"
                        || (SocKind::of(&r.verb).is_some() && r.magnitude >= NOTABLE_SOCIETY))
            })
}

/// Society events at or above this magnitude make a beat notable (its narrator uses the LLM).
pub const NOTABLE_SOCIETY: u8 = 7;

/// Run calls with the LLM where `use_llm` says so and by rules elsewhere, concurrently; replies in call order.
pub async fn run_split(
    llm: Option<&dyn LlmProvider>,
    calls: Vec<MindCall>,
    use_llm: Vec<bool>,
    timeout: Duration,
) -> RunReport {
    type Indexed = Vec<(usize, MindCall)>;
    let (mut a, mut b): (Indexed, Indexed) = (vec![], vec![]);
    for (i, c) in calls.into_iter().enumerate() {
        if use_llm.get(i).copied().unwrap_or(true) && llm.is_some() {
            a.push((i, c));
        } else {
            b.push((i, c));
        }
    }
    let ca: Vec<MindCall> = a.iter().map(|x| x.1.clone()).collect();
    let cb: Vec<MindCall> = b.iter().map(|x| x.1.clone()).collect();
    let (ra, rb) = futures::join!(run_batch(llm, &ca, timeout), run_batch(None, &cb, timeout));
    let n = a.len() + b.len();
    let mut replies: Vec<Option<Outcome<MindReply>>> = (0..n).map(|_| None).collect();
    for ((i, _), o) in a.iter().zip(ra.replies) {
        replies[*i] = Some(o);
    }
    for ((i, _), o) in b.iter().zip(rb.replies) {
        replies[*i] = Some(o);
    }
    // Rules-by-design calls are not LLM fallbacks: only the LLM batch counts as calls (and its fallbacks).
    let replies: Vec<Outcome<MindReply>> = replies.into_iter().map(|o| o.expect("every call replied")).collect();
    let fell_back = ra.fell_back.iter().map(|k| a[*k].0).collect();
    RunReport {
        replies,
        fell_back,
        rules_only: if llm.is_some() { b.len() } else { 0 },
    }
}

/// Bolts Entropy hands an invoker who reaches into its domain.
pub const ENTROPY_GIFT: u32 = 2;
/// What a standing-order intent's `how` says.
pub const STANDING_HOW: &str = "standing order";
/// Standing orders kept per hand.
pub const STANDING_PER_HAND: usize = 2;

/// Bolts a god charges for an invocation: 1 + magnitude/5 (Entropy charges nothing: it wants you to have more).
pub fn invoke_cost(def: &GodDef, magnitude: u8) -> u32 {
    if def.slug == "entropy" {
        0
    } else {
        1 + magnitude.min(10) as u32 / 5
    }
}

/// Rules 0.8.1: the orders of a player whose demigods have all fallen (they used to vanish: match f18d76a3, beat 44,
/// "my guys come back to life!").
#[derive(Debug, Clone, PartialEq)]
pub struct FallenWill {
    pub seat: u8,
    pub text: String,
    pub seq: u64,
}

/// What became of a fallen player's will: sent to Death (`intent`) or refused (`reason`).
#[derive(Debug, Clone)]
pub struct WillNote {
    pub seat: u8,
    pub text: String,
    pub intent: Option<IntentId>,
    pub reason: String,
}

impl WillNote {
    /// The private `wills[]` entry: `{text, routed_to, verdict, speech, reason}`.
    pub fn json(&self, w: &World) -> Value {
        let ruling = self.intent.and_then(|i| w.rulings.iter().rev().find(|r| r.intent == i));
        json!({ "text": self.text,
                "routed_to": ruling.map(|r| w.name_of(Ref::God(r.god))),
                "verdict": ruling.map(|r| json!(r.verdict)).unwrap_or_else(|| json!("refused")),
                "speech": ruling.map(|r| r.speech.clone()),
                "reason": self.reason })
    }
}

/// The revive words of a fallen player's plea.
pub fn is_revive(text: &str) -> bool {
    const W: &[&str] = &["back to life", "come back", "revive", "resurrect", "raise", "rise again", "rise up", "bring back",
                         "bring us back", "return from the dead", "live again", "alive again", "undie", "respawn"];
    let t = text.to_lowercase();
    W.iter().any(|k| t.contains(k))
}

/// Rules 0.8.1: every fallen player's will gets a visible ruling. A plea to come back becomes an invocation of Death
/// by the player's first demigod for all of the player's fallen demigods (magnitude 6: Death's normal price, ruled
/// like any invocation; without the bolts Death twists or refuses, and says so); any other order is refused with the
/// reason (the fallen can't act, only plead). Each is a `fallen_will` trace event and a private `wills[]` entry.
fn fallen_wills(w: &mut World, beat: u32, wills: &[FallenWill], pantheon: &Pantheon) -> Vec<WillNote> {
    let mut out = vec![];
    let death = pantheon.id_of("death");
    for will in wills {
        let fallen: Vec<DemigodId> = w.bodies_of_seat(will.seat).filter(|d| !d.alive).map(|d| d.id).collect();
        let Some(&lead) = fallen.first() else { continue };
        let mut note = WillNote { seat: will.seat, text: will.text.clone(), intent: None, reason: String::new() };
        match death.filter(|_| is_revive(&will.text) && !out.iter().any(|n: &WillNote| n.seat == will.seat && n.intent.is_some())) {
            Some(god) => {
                let d = &w.demigods[lead.ix()];
                let def = pantheon.get(god).cloned();
                let magnitude = 6;
                let cost = def.as_ref().map(|x| invoke_cost(x, magnitude)).unwrap_or(0);
                let id = IntentId::from_ix(w.intents.len());
                let i = Intent {
                    id,
                    beat,
                    agent: d.hands[0],
                    demigod: lead,
                    verb: "revive".into(),
                    targets: fallen.iter().map(|x| Target::Known(Ref::Demigod(*x))).collect(),
                    how: String::new(),
                    place: Some(d.place),
                    magnitude,
                    span: will.text.chars().take(240).collect(),
                    route: Route::God { god, by: RouteBy::Keyword },
                };
                note.reason = if d.bolts < cost {
                    format!("a fallen player's will goes to Death; Death's price is {cost} bolts and {} has {}", d.name, d.bolts)
                } else {
                    format!("a fallen player's will goes to Death (price {cost} bolts)")
                };
                event("intent", Some(Ref::Agent(i.agent).to_string()),
                      json!({ "beat": beat, "intent": i, "routed_by": "keyword", "fallback": null, "fallen_will": true }));
                w.intents.push(i);
                note.intent = Some(id);
            }
            None => {
                note.reason = if is_revive(&will.text) {
                    "one plea to Death a beat: this one was already heard".into()
                } else {
                    "your demigods have fallen: they can't act, only plead with Death to come back".into()
                };
            }
        }
        event("fallen_will", Some(format!("seat:{}", will.seat)),
              json!({ "beat": beat, "seat": will.seat, "seq": will.seq, "text": will.text, "intent": note.intent, "reason": note.reason }));
        out.push(note);
    }
    out
}

/// Rules 0.8.1: a heal's targets: rival demigods only when the order names them; "me" / "my demigods" / "all my
/// demigods" (or no demigod at all) mean the player's own standing ones; a fallen one of its own sends the heal to
/// Death (a raising).
fn heal_targets(w: &World, i: &mut Intent, pantheon: &Pantheon) {
    if !matches!(i.route, Route::God { .. }) || !fn7_agents::hand::is_heal(&i.verb, &i.span) {
        return;
    }
    let span = i.span.to_lowercase();
    let me = i.demigod;
    let named = |d: DemigodId| {
        let n = w.demigods[d.ix()].name.to_lowercase();
        span.contains(&n) || n.split("'s").next().is_some_and(|p| p.len() > 2 && span.contains(p.trim()))
    };
    let before = i.targets.len();
    i.targets.retain(|t| match t {
        Target::Known(Ref::Demigod(d)) => w.same_player(*d, me) || named(*d),
        _ => true,
    });
    let has_own = i.targets.iter().any(|t| matches!(t, Target::Known(Ref::Demigod(d)) if w.same_player(*d, me)));
    let mine = ["my demigod", "all my", "both of us", "myself", "my hands", "us both", " me", "me "].iter().any(|k| span.contains(k))
        || span.trim() == "me";
    let any_dg = i.targets.iter().any(|t| matches!(t, Target::Known(Ref::Demigod(_))));
    if !has_own && (mine || !any_dg) {
        let seat = w.demigods[me.ix()].seat;
        for d in w.bodies_of_seat(seat) {
            i.targets.push(Target::Known(Ref::Demigod(d.id)));
        }
    }
    let fallen = i.targets.iter().any(|t| matches!(t, Target::Known(Ref::Demigod(d)) if !w.demigods[d.ix()].alive));
    if fallen {
        if let Some(god) = pantheon.id_of("death") {
            i.route = Route::God { god, by: RouteBy::Keyword };
        }
    }
    if i.targets.len() != before || fallen {
        event("heal_targets", Some(Ref::Demigod(me).to_string()),
              json!({ "intent": i.id, "targets": i.targets, "dropped_rivals": before > i.targets.len(), "to_death": fallen }));
    }
}

/// Rules 0.8.1: the heal a god granted: +1..+3 health (1 + magnitude/4) per standing target demigod or person it
/// didn't already heal, half when twisted (capped by max health in the merge); fallen targets are Death's to raise.
fn heal_props(w: &World, ruling: &Ruling, intent: &Intent, theirs: &[Proposal]) -> Vec<Proposal> {
    let amount = (1.0 + intent.magnitude as f32 / 4.0).clamp(1.0, 3.0) * if ruling.verdict == Verdict::Twist { 0.5 } else { 1.0 };
    let mut out = vec![];
    for t in &intent.targets {
        let Target::Known(r @ (Ref::Demigod(_) | Ref::Char(_))) = t else { continue };
        if theirs.iter().any(|p| matches!(&p.effect, Effect::Heal { who, .. } | Effect::Revive { who } if who == r)) {
            continue;
        }
        let alive = match r {
            Ref::Demigod(d) => w.demigods[d.ix()].alive,
            Ref::Char(c) => w.characters[c.ix()].alive,
            _ => false,
        };
        let effect = if alive { Effect::Heal { who: *r, amount } } else { Effect::Revive { who: *r } };
        if !alive && w.name_of(Ref::God(ruling.god)) != "Death" {
            continue;
        }
        out.push(god_prop(ruling, intent, effect, format!("{} mends {}", w.name_of(Ref::God(ruling.god)), w.name_of(*r))));
    }
    out
}

pub(crate) fn god_prop(ruling: &Ruling, intent: &Intent, effect: Effect, act: String) -> Proposal {
    Proposal {
        source: Source::God {
            god: ruling.god,
            ruling: Some(ruling.id),
        },
        intent: Some(intent.id),
        effect,
        act,
        actor: Ref::God(ruling.god),
        magnitude: 0,
        valence: 0,
        witnesses: vec![ruling.invoker],
        cause: None,
    }
}

pub fn is_standing(_w: &World, i: &Intent) -> bool {
    i.how == STANDING_HOW
}

/// Words that make a world act an army act.
const ARMY_WORDS: &[&str] = &[
    "army", "armies", "legion", "legions", "horde", "troops", "militia", "soldiers", "crusade", "warband", "host",
];
const RAISE_WORDS: &[&str] = &[
    "raise", "summon", "muster", "recruit", "rally", "form", "gather", "build", "conscript", "assemble", "call up",
];

fn has_word(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| fn7_agents::common::has_phrase(text, w))
}

/// An ordinary act about armies (raising one, or sending one), handled by the rules.
pub fn is_army_act(w: &World, i: &Intent) -> bool {
    let t = format!("{} {}", i.verb, i.span).to_lowercase();
    has_word(&t, ARMY_WORDS)
        || (has_word(&t, &["march", "invade", "conquer", "besiege"]) && own_army(w, i.demigod).is_some())
}

/// A demigod's biggest live army.
pub fn own_army(w: &World, d: DemigodId) -> Option<ClusterId> {
    fn7_society::armies(w)
        .filter(|(_, a)| a.demigod == d)
        .max_by(|x, y| x.0.size.cmp(&y.0.size).then(y.0.id.cmp(&x.0.id)))
        .map(|(c, _)| c.id)
}

/// Raise an army from a crowd (the named one, else the biggest in the demigod's region), or aim the demigod's army at
/// what the act names: a place, a demigod to hunt, a crowd.
pub fn army_intent(w: &World, i: &Intent, beat: u32) -> Vec<Proposal> {
    let d = i.demigod;
    let me = &w.demigods[d.ix()];
    let text = format!("{} {}", i.verb, i.span).to_lowercase();
    let known: Vec<Ref> = i
        .targets
        .iter()
        .filter_map(|t| match t {
            Target::Known(r) => Some(*r),
            _ => None,
        })
        .collect();
    let dest_ref = known.iter().copied().find(|r| {
        matches!(r, Ref::Place(_) | Ref::Demigod(_))
            || matches!(r, Ref::Cluster(c) if w.clusters[c.ix()].army.is_some())
    });
    let mk = |effect: Effect, act: String, witnesses: Vec<Ref>| Proposal {
        source: Source::Agent { agent: i.agent },
        intent: Some(i.id),
        effect,
        act,
        actor: Ref::Demigod(d),
        magnitude: i.magnitude.max(5),
        valence: -1,
        witnesses,
        cause: None,
    };
    let mut out = vec![];
    let army = own_army(w, d);
    if army.is_none() || has_word(&text, RAISE_WORDS) {
        let region = fn7_society::geo::district_of(w, me.place);
        let named = known.iter().find_map(|r| match r {
            Ref::Cluster(c) if w.clusters[c.ix()].army.is_none() && w.clusters[c.ix()].size > 1 => Some(*c),
            _ => None,
        });
        let src = named.or_else(|| {
            w.clusters
                .iter()
                .filter(|c| c.army.is_none() && c.size > 1)
                .max_by(|a, b| {
                    let inr = |c: &Cluster| fn7_society::geo::district_of(w, c.place) == region;
                    inr(a).cmp(&inr(b)).then(a.size.cmp(&b.size)).then(b.id.cmp(&a.id))
                })
                .map(|c| c.id)
        });
        let Some(src) = src else { return out };
        let cl = &w.clusters[src.ix()];
        let share = 0.1 + 0.05 * i.magnitude.min(10) as f32;
        let size = ((cl.size as f32 * share).round() as u32).clamp(1, cl.size);
        let city = w.name_of(Ref::Place(cl.place));
        let n = fn7_society::armies(w).filter(|(_, a)| a.demigod == d).count()
            + w.clusters.iter().filter(|c| c.army.as_ref().is_some_and(|a| a.demigod == d)).count();
        let name = format!("{}'s Host of {city}{}", me.name, if n > 0 { format!(" ({})", n + 1) } else { String::new() });
        out.push(mk(
            Effect::RaiseArmy {
                demigod: d,
                from: src,
                size,
                name: name.clone(),
                orders: i.span.clone(),
            },
            format!("{} raises {name} from the {}: {size} strong", me.name, cl.label),
            vec![Ref::Demigod(d), Ref::Cluster(src)],
        ));
        // A freshly raised army gets its march next beat (its cluster id is the next one).
        if let Some(t) = dest_ref {
            let new_id = ClusterId::from_ix(w.clusters.len());
            let pos = w.places.get(cl.place.ix()).map(|p| p.center).unwrap_or(me.pos);
            out.push(mk(
                Effect::March {
                    army: new_id,
                    to: pos,
                    dest: fn7_society::geo::pos_of(w, t),
                    target: Some(t),
                    orders: Some(i.span.clone()),
                },
                format!("{name} is sent against {}", w.name_of(t)),
                vec![Ref::Demigod(d)],
            ));
        }
        let _ = beat;
        return out;
    }
    let army = army.expect("checked");
    let a = w.clusters[army.ix()].army.as_ref().expect("an army");
    let dest = dest_ref.and_then(|t| fn7_society::geo::pos_of(w, t));
    out.push(mk(
        Effect::March {
            army,
            to: a.pos,
            dest,
            target: dest_ref,
            orders: Some(i.span.clone()),
        },
        match dest_ref {
            Some(t) => format!("{} turns {} toward {}", me.name, w.clusters[army.ix()].label, w.name_of(t)),
            None => format!("{} gives {} new orders", me.name, w.clusters[army.ix()].label),
        },
        vec![Ref::Demigod(d), Ref::Cluster(army)],
    ));
    out
}

/// Hands that got words this beat keep their ordinary (non-army) acts as standing orders (the last
/// [`STANDING_PER_HAND`] per hand).
fn set_standing_orders(w: &mut World, from: usize, beat: u32) {
    let mut fresh: BTreeMap<(DemigodId, Hand), Vec<StandingOrder>> = BTreeMap::new();
    for i in &w.intents[from..] {
        let Some(hand) = (match w.agents.get(i.agent.ix()).map(|a| a.kind) {
            Some(AgentKind::Hand { hand, .. }) => Some(hand),
            _ => None,
        }) else {
            continue;
        };
        if !matches!(i.route, Route::World | Route::Pending)
            || crate::items::is_manifest_act(w, i)
            || is_army_act(w, i)
            || i.how == STANDING_HOW
            || crate::society::is_society_act(w, i)
        {
            continue;
        }
        fresh.entry((i.demigod, hand)).or_default().push(StandingOrder {
            hand,
            verb: i.verb.clone(),
            targets: i
                .targets
                .iter()
                .filter_map(|t| match t {
                    Target::Known(r) => Some(*r),
                    _ => None,
                })
                .collect(),
            place: i.place,
            magnitude: i.magnitude,
            span: i.span.chars().take(160).collect(),
            since: beat,
        });
    }
    for ((d, hand), mut v) in fresh {
        let keep = v.len().saturating_sub(STANDING_PER_HAND);
        v.drain(..keep);
        let orders = &mut w.demigods[d.ix()].orders;
        orders.retain(|o| o.hand != hand);
        orders.extend(v);
        event(
            "standing_orders",
            Some(Ref::Demigod(d).to_string()),
            json!({ "beat": beat, "demigod": d, "hand": hand, "orders": orders }),
        );
    }
}

/// Silent demigods' hands carry out their standing orders by rules, every other beat: one intent per order, routed to
/// the resolver of its place (which runs without the LLM unless someone there spoke).
fn continue_standing_orders(w: &mut World, beat: u32, spoke: &std::collections::BTreeSet<DemigodId>) {
    let mut new = vec![];
    for d in w.demigods.iter().filter(|d| d.alive && !spoke.contains(&d.id)) {
        if !(beat + d.id.0).is_multiple_of(2) {
            continue;
        }
        for o in &d.orders {
            let agent = d.hands[if o.hand == Hand::Left { 0 } else { 1 }];
            if !w.agents[agent.ix()].is_live() {
                continue;
            }
            new.push(Intent {
                id: IntentId(0),
                beat,
                agent,
                demigod: d.id,
                verb: o.verb.clone(),
                targets: o
                    .targets
                    .iter()
                    .filter(|r| fn7_society::is_alive(w, **r))
                    .map(|r| Target::Known(*r))
                    .collect(),
                how: STANDING_HOW.into(),
                place: Some(d.place),
                magnitude: o.magnitude.min(4),
                span: o.span.clone(),
                route: Route::World,
            });
        }
    }
    for mut i in new {
        i.id = IntentId::from_ix(w.intents.len());
        event(
            "intent",
            Some(i.agent.to_string()),
            json!({ "beat": beat, "intent": i, "routed_by": null, "fallback": true, "standing": true }),
        );
        w.intents.push(i);
    }
}

/// Set beat `b`'s caption (narrated after the fact) and record it as a `caption` event.
pub fn set_caption(w: &mut World, b: u32, n: &Narration, fallback: bool) {
    if let Some(r) = w.beats.iter_mut().find(|r| r.beat == b) {
        r.caption = n.caption.clone();
    }
    event(
        "caption",
        None,
        json!({ "beat": b, "caption": n.caption, "callouts": n.callouts, "fallback": fallback }),
    );
}

/// One demigod's input this beat (`seat` ≥ SYNTHETIC_SEAT for the plugin's AI).
pub struct Turn {
    pub demigod: DemigodId,
    pub seat: u8,
    pub input: Input,
}

/// A seat's input as turns, one per standing demigod (rules 0.7.0). One demigod per seat (schema 4): one turn for
/// `lead`, unchanged. Two: the left text goes to the Left demigod and the right text to the Right one (a fallen
/// demigod's text goes to its standing sibling); zoom and casts ride on the lead demigod's turn (casts are the
/// player's, paid from the shared purse).
pub fn split_turn(w: &World, seat: u8, lead: DemigodId, input: Input) -> Vec<Turn> {
    let bodies: Vec<&Demigod> = w.bodies_of_seat(seat).collect();
    if bodies.len() < 2 {
        return vec![Turn { demigod: lead, seat, input }];
    }
    let standing: Vec<&Demigod> = bodies.iter().copied().filter(|d| d.alive).collect();
    let mut out = vec![];
    for d in &standing {
        let right = d.side == Some(Hand::Right);
        let (mine, theirs) = if right { (&input.right, &input.left) } else { (&input.left, &input.right) };
        let text = if standing.len() == 1 { crate::join(mine, theirs) } else { mine.clone() };
        let (left, right_text) = if right { (String::new(), text) } else { (text, String::new()) };
        let is_lead = d.id == lead;
        out.push(Turn {
            demigod: d.id,
            seat,
            input: Input {
                left,
                right: right_text,
                zoom: if is_lead { input.zoom.clone() } else { None },
                casts: if is_lead { input.casts.clone() } else { vec![] },
                summons: if is_lead { input.summons.clone() } else { vec![] },
                god_powers: if is_lead { input.god_powers.clone() } else { vec![] },
                challenge: None,
                tick: None,
                blow: None,
                keys: None,
            },
        });
    }
    out
}

/// What the beat returns to the Theatre.
pub struct BeatOut {
    pub public: Value,
    pub per_seat: Vec<(Seat, Value)>,
}

struct Clock {
    t: Instant,
    ms: Vec<(Stage, f64)>,
}
impl Clock {
    fn lap(&mut self, s: Stage) {
        let now = Instant::now();
        self.ms.push((s, (now - self.t).as_secs_f64() * 1000.0));
        self.t = now;
    }
}

fn record_mut(w: &mut World, beat: u32) -> &mut BeatRecord {
    if w.beats.last().is_none_or(|b| b.beat != beat) {
        w.beats.push(new_record(beat));
    }
    w.beats.last_mut().expect("just pushed")
}

pub fn new_record(beat: u32) -> BeatRecord {
    BeatRecord {
        beat,
        started_at_ms: 0,
        timing: StageTiming::default(),
        utterances: 0,
        words: 0,
        intents: 0,
        proposals: 0,
        applied: 0,
        rows: (None, None),
        memorized: vec![],
        rulings: vec![],
        spawned: vec![],
        eliminated: vec![],
        caption: String::new(),
        fidelity: None,
    }
}

fn recall(w: &World, owner: Ref, about: &[Ref], n: usize) -> String {
    let items = fn7_memory::recall(
        w,
        &Recall {
            owner,
            about: about.to_vec(),
            max_items: n,
            now: w.meta.beat,
        },
    );
    fn7_memory::render(w, &items)
}

fn entities(w: &World, refs: &[Ref]) -> Vec<Entity> {
    refs.iter().filter_map(|r| entity(w, *r)).collect()
}

/// A god's ruling is public (an omen everyone sees) when it is big or sends a paladin.
pub fn ruling_is_public(w: &World, r: &Ruling) -> bool {
    r.dispatch_mission.is_some()
        || w.intents
            .get(r.intent.ix())
            .is_some_and(|i| i.magnitude >= 6)
}

fn words_of(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Resolve a place or group mention by name tokens (no LLM): the place/group sharing the most content words.
fn match_named(w: &World, m: &Mention) -> Option<Ref> {
    let mt = fn7_society::tokens(&m.text);
    if mt.is_empty() {
        return None;
    }
    let score = |name: &str| {
        let nt = fn7_society::tokens(name);
        mt.iter().filter(|t| nt.contains(t)).count()
    };
    let pick = |cands: Vec<(Ref, String)>| {
        cands
            .into_iter()
            .map(|(r, n)| (score(&n), r))
            .filter(|(s, _)| *s > 0)
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
            .map(|x| x.1)
    };
    match m.kind.as_str() {
        "place" => pick(
            w.places
                .iter()
                .map(|p| (Ref::Place(p.id), p.name.clone()))
                .collect(),
        ),
        "group" => pick(
            w.groups
                .iter()
                .filter(|g| g.dissolved_beat.is_none())
                .map(|g| (Ref::Group(g.id), g.name.clone()))
                .collect(),
        )
        .or_else(|| {
            pick(
                w.clusters
                    .iter()
                    .filter(|c| c.size > 0)
                    .map(|c| (Ref::Cluster(c.id), c.label.clone()))
                    .collect(),
            )
        }),
        _ => None,
    }
}

/// The beat.
#[allow(clippy::too_many_arguments)]
pub async fn resolve(
    w: &mut World,
    beat: u32,
    turns: Vec<Turn>,
    subs: &[crate::flow::Sub],
    duel_acts: &crate::duel::Acts,
    wills: &[FallenWill],
    closed: &[theatre_plugin_api::ClosedWindow],
    llm: Option<&dyn LlmProvider>,
    pantheon: &Arc<Pantheon>,
) -> BeatOut {
    let st: Settings = settings(w);
    let timeout = Duration::from_millis(st.mind_timeout_ms as u64);
    let started = Instant::now();
    let started_at = theatre_trace::now_ms();
    let mut clock = Clock {
        t: started,
        ms: vec![],
    };
    let mut llm_calls = 0u32;
    let mut llm_fallbacks = 0u32;
    let mut llm_ms_max = 0f64;
    let mut count = |r: &RunReport, ms: f64| {
        if llm.is_some() {
            llm_calls += r.llm_calls() as u32;
            llm_fallbacks += r.fallbacks() as u32;
            llm_ms_max = llm_ms_max.max(ms);
        }
    };
    w.meta.beat = beat;
    record_mut(w, beat);
    let alive_at_start: Vec<DemigodId> = w.alive_demigods().map(|d| d.id).collect();
    let pos_at_start: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
    // Rules 0.8.0 (the DM's step reads these): health, armies and the memory/manifest heads at the beat's start.
    let health_at_start: Vec<f32> = w.demigods.iter().map(|d| if d.alive { d.health } else { 0.0 }).collect();
    let armies_at_start: Vec<(ClusterId, LonLat)> =
        w.clusters.iter().filter_map(|c| c.army.as_ref().map(|a| (c.id, a.pos))).collect();
    let memory_start = w.dm.memory.len();
    let manifest_start = w.manifests.len();
    let entropy_at_start = w.meta.entropy;
    let row_start = w.ledger.rows.len();
    let first_new_intent = w.intents.len();
    // Rules 0.8.1: this beat's duels, fought first from the committed state (as they were shown live) and applied.
    let duels = crate::duel::process(w, beat, duel_acts, pantheon, &st);

    // ---- (0) view changes from the action's zoom field ------------------------------------------------------------
    for t in &turns {
        if let Some((zoom, focus)) = &t.input.zoom {
            let cands = visible(w, t.demigod, Zoom::District, None, 128);
            let focus = focus.as_deref().and_then(|f| resolve_focus(w, f, &cands));
            set_zoom(w, t.demigod, *zoom, focus, beat);
        }
    }

    // ---- (0a) text entry (the God of Time interaction): settle the core's slow windows (Time's bill) ---------------
    // The demigods that received orders, per seat (Time's debt is split between them; rules 0.7.0).
    let mut ordered: BTreeMap<u8, Vec<DemigodId>> = BTreeMap::new();
    for t in turns.iter().filter(|t| t.input.has_orders()) {
        ordered.entry(t.seat).or_default().push(t.demigod);
    }
    let flow = crate::flow::process(w, beat, subs, closed, pantheon, &st, &ordered);
    // ---- (0a') casts: god powers from the UX, settled in arrival order (ramifications run in the act batch) --------
    let casts = crate::casts::process(w, beat, &turns, pantheon, &st);
    // Rules 0.8.0: summonings land now (the move, the mishap's d20, the health); their deeds are told in the act batch.
    let summons = if w.dm_rules() { crate::summon::process(w, beat, &turns, pantheon, &st, &health_at_start) } else { vec![] };
    // Who spoke this beat (their hands, subagents and resolvers use the LLM; everyone else continues by rules).
    let spoke: std::collections::BTreeSet<DemigodId> = turns
        .iter()
        .filter(|t| t.input.has_orders())
        .map(|t| t.demigod)
        .collect();
    let loud = !spoke.is_empty() || !casts.ramify.is_empty();
    // Intents from here on are the hands' (cast intents above are Entropy's, already settled).
    let route_from = w.intents.len();
    // Rules 0.8.0: god-tier powers are paid now and become invocations the route sends to their gods.
    let god_tier = if w.dm_rules() { crate::godtier::process(w, beat, &turns, pantheon, &st) } else { vec![] };
    let llm_all = llm;

    // ---- (0b) background: last beat's narrator and this beat's LLM folds, alongside everything until the narrate stage
    let narrate = beat
        .checked_sub(1)
        .filter(|b| w.beats.iter().any(|r| r.beat == *b && r.caption.is_empty()));
    let per_owner = st.memory_per_owner as usize;
    let fold_plans = llm_fold_plans(w, per_owner, st.llm_folds_per_beat as usize);
    let narrate_loud = narrate.is_some_and(|b| beat_was_loud(w, b));
    let mut bg_calls: Vec<MindCall> = narrate
        .map(|b| MindCall::Narrator(narrator_ctx(w, b)))
        .into_iter()
        .collect();
    for p in &fold_plans {
        bg_calls.push(MindCall::Fold(FoldCtx {
            owner: p.owner,
            owner_name: w.name_of(p.owner),
            items: p
                .items
                .iter()
                .map(|i| w.memory.items[i.ix()].text.clone())
                .collect(),
            max_words: 60,
        }));
    }
    let bg_t0 = Instant::now();
    // The narrator uses the LLM when the beat it narrates was loud; the folds when this beat is.
    let bg_use: Vec<bool> = bg_calls
        .iter()
        .enumerate()
        .map(|(i, _)| if i == 0 && narrate.is_some() { narrate_loud } else { loud })
        .collect();
    let mut bg: Background<'_> = Box::pin(run_split(llm_all, bg_calls.clone(), bg_use, timeout));
    let mut bg_rep: Option<RunReport> = None;

    // ---- (1) intake: utterances → hands --------------------------------------------------------------------------
    let mut utterances = 0u32;
    let mut words_by: BTreeMap<DemigodId, u32> = BTreeMap::new();
    let mut hand_calls: Vec<MindCall> = vec![];
    let mut hand_text: BTreeMap<AgentId, Vec<String>> = BTreeMap::new();
    for t in &turns {
        let d = t.demigod;
        let Some(dg) = w.demigods.get(d.ix()).cloned() else {
            continue;
        };
        if !dg.alive {
            continue;
        }
        let (zoom, focus) = seat_of(w, d)
            .map(|s| (s.zoom, s.focus))
            .unwrap_or((Zoom::District, None));
        for (h, text) in [(Hand::Left, &t.input.left), (Hand::Right, &t.input.right)] {
            let lines = words_of(text);
            if lines.is_empty() {
                continue;
            }
            let agent = dg.hands[if h == Hand::Left { 0 } else { 1 }];
            if !w.agents[agent.ix()].is_live() {
                continue;
            }
            let n = words(text);
            utterances += 1;
            *words_by.entry(d).or_default() += n;
            let u = Utterance {
                seat: dg.seat,
                hand: h,
                beat,
                text: text.clone(),
                mode: "type".into(),
                spoken_ms: None,
                words: n,
            };
            event("utterance", Some(Ref::Demigod(d).to_string()), json!(u));
            hand_text.insert(agent, lines.clone());
            let mut vis = visible(w, d, zoom, focus, 40);
            // Fallen demigods stay nameable (Death's "raise Kai from the dead" needs a target that isn't standing).
            vis.extend(w.demigods.iter().filter(|x| !x.alive && x.id != d).map(|x| Ref::Demigod(x.id)));
            let subs = live_children(w, agent)
                .into_iter()
                .map(|a| {
                    let ag = &w.agents[a.ix()];
                    SubagentLine {
                        agent: a,
                        name: ag.name.clone(),
                        goal: ag.goal.clone(),
                        beats_left: ag
                            .budget_beats
                            .map(|b| (ag.spawned_beat + b).saturating_sub(beat)),
                    }
                })
                .collect();
            hand_calls.push(MindCall::Hand(HandCtx {
                agent,
                demigod: d,
                demigod_name: dg.name.clone(),
                hand: h,
                persona: w.agents[agent.ix()].persona.clone(),
                player: dg.persona.clone(),
                beat,
                first_intent: IntentId::from_ix(w.intents.len()),
                place: Some(dg.place),
                utterances: lines,
                zoom,
                focus,
                visible: entities(w, &vis),
                subagents: subs,
                memory: recall(w, Ref::Agent(agent), &vis, 8),
                pantheon: pantheon.clone(),
                max_subagents: st.max_subagents_per_hand as usize,
                final_wish: is_final_wish(w, beat),
                own: if st.rules_0_8_1 { w.bodies_of_seat(dg.seat).map(|x| x.id).collect() } else { vec![] },
                rules_081: st.rules_0_8_1,
            }));
        }
    }
    for (d, n) in &words_by {
        w.demigods[d.ix()].words_spoken += *n as u64;
    }
    let t0 = Instant::now();
    let rep = overlap(run_batch(llm, &hand_calls, timeout), &mut bg, &mut bg_rep).await;
    count(&rep, t0.elapsed().as_secs_f64() * 1000.0);
    let mut said: BTreeMap<AgentId, String> = BTreeMap::new();
    for (call, o) in hand_calls.iter().zip(rep.replies) {
        let (
            MindCall::Hand(ctx),
            MindReply::Hand(HandOut {
                intents,
                said: line,
            }),
        ) = (call, o.value)
        else {
            continue;
        };
        if !line.is_empty() {
            event(
                "hand_said",
                Some(ctx.agent.to_string()),
                json!({ "agent": ctx.agent, "beat": beat, "said": line }),
            );
        }
        said.insert(ctx.agent, line);
        let wish = is_final_wish(w, beat);
        let cap = if flow.windowed.contains(&ctx.demigod) || wish {
            st.max_intents_per_window
        } else {
            st.max_intents_per_hand
        };
        for mut i in intents.into_iter().take(cap as usize) {
            i.id = IntentId::from_ix(w.intents.len());
            if st.rules_0_8_1 {
                heal_targets(w, &mut i, pantheon);
            }
            if wish {
                // The Final Wish: every act of the wish is grander (rules 0.7.0).
                i.magnitude = (i.magnitude as u32 + st.final_wish_magnitude).min(10) as u8;
            }
            let by = match i.route {
                Route::God { by, .. } => Some(by),
                _ => None,
            };
            event(
                "intent",
                Some(ctx.agent.to_string()),
                json!({ "beat": beat, "intent": i, "routed_by": by, "fallback": o.fallback }),
            );
            w.intents.push(i);
        }
    }
    // Standing orders: a hand that spoke keeps its world acts as standing orders; silent demigods' hands carry theirs
    // out by rules every other beat (no LLM).
    set_standing_orders(w, route_from, beat);
    if !w.demigods.is_empty() {
        continue_standing_orders(w, beat, &spoke);
    }
    // Rules 0.8.1: fallen players' wills (a plea to Death, ruled on like any invocation; else a refusal with why).
    let will_notes = fallen_wills(w, beat, wills, pantheon);
    clock.lap(Stage::Intake);

    // ---- (2) mentions → refs (memorizing people as they are addressed) -------------------------------------------
    let mut flavor_calls: Vec<MindCall> = vec![];
    let mut memorized_budget = st.max_memorize_per_beat as usize;
    for ii in route_from..w.intents.len() {
        for ti in 0..w.intents[ii].targets.len() {
            let Target::Mention(m) = w.intents[ii].targets[ti].clone() else {
                continue;
            };
            let intent = w.intents[ii].clone();
            let (to, score, how): (Option<Ref>, f32, &str) = match match_named(w, &m) {
                Some(r) => (Some(r), 1.0, "named"),
                None => match fn7_society::resolve_mention(w, &m, m.near) {
                    fn7_society::Resolution::Existing { char, score } => {
                        (Some(Ref::Char(char)), score, "existing")
                    }
                    fn7_society::Resolution::Materialize { cluster, score }
                        if memorized_budget > 0 =>
                    {
                        memorized_budget -= 1;
                        let p = Proposal {
                            source: Source::Agent {
                                agent: intent.agent,
                            },
                            intent: Some(intent.id),
                            effect: Effect::Memorize {
                                mention: m.clone(),
                                cluster: Some(cluster),
                            },
                            act: format!(
                                "{} singles out {}",
                                w.name_of(Ref::Demigod(intent.demigod)),
                                line(&m.text)
                            ),
                            actor: Ref::Agent(intent.agent),
                            magnitude: 1,
                            valence: 0,
                            witnesses: vec![],
                            cause: None,
                        };
                        let (_, out) =
                            merge::run(w, vec![p], pantheon, beat, st.max_witnesses as usize);
                        match out.new_chars.first() {
                            Some(c) => {
                                let ch = &w.characters[c.ix()];
                                let cl = &w.clusters[cluster.ix()];
                                flavor_calls.push(MindCall::Materialize(MaterializeCtx {
                                    cluster,
                                    cluster_label: cl.label.clone(),
                                    place_name: w.name_of(Ref::Place(cl.place)),
                                    traits: cl.traits.clone(),
                                    needs: cl.needs.clone(),
                                    name: ch.name.clone(),
                                    personal: ch.traits.clone(),
                                    addressed_as: m.text.clone(),
                                }));
                                record_mut(w, beat).memorized.push(*c);
                                (Some(Ref::Char(*c)), score, "materialized")
                            }
                            None => (None, score, "none"),
                        }
                    }
                    _ => (None, 0.0, "none"),
                },
            };
            event(
                "mention_resolved",
                Some(Ref::Agent(intent.agent).to_string()),
                json!({ "beat": beat, "intent": intent.id, "mention": m, "to": to, "score": score, "how": how }),
            );
            if let Some(r) = to {
                w.intents[ii].targets[ti] = Target::Known(r);
            }
        }
    }

    // ---- (3) route --------------------------------------------------------------------------------------------------
    let mut vicinities: BTreeMap<PlaceId, Vec<ActLine>> = BTreeMap::new();
    let mut god_calls: Vec<MindCall> = vec![];
    let mut army_props: Vec<Proposal> = vec![];
    let mut general_meta: Vec<(crate::society::GeneralMeta, Option<Ref>)> = vec![];
    // Rules 0.7.0: manifestations judged by the item engine (named after the act batch).
    let mut judged: Vec<crate::items::Judged> = vec![];
    let mut spent: BTreeMap<u8, u32> = BTreeMap::new();
    for ii in route_from..w.intents.len() {
        let intent = w.intents[ii].clone();
        let d = intent.demigod;
        let hand = intent.agent;
        let world_act = |w: &World, v: &mut BTreeMap<PlaceId, Vec<ActLine>>| {
            let place = intent.place.unwrap_or(w.demigods[d.ix()].place);
            v.entry(district_of(w, place)).or_default().push(ActLine {
                intent: intent.clone(),
                actor: Ref::Demigod(d),
            });
        };
        match &intent.route {
            _ if crate::items::is_manifest_act(w, &intent) => {
                if let Some(j) = crate::items::judge(w, &intent, beat, judged.len(), &mut spent, pantheon, &st) {
                    event(
                        "manifest_judged",
                        Some(Ref::Demigod(d).to_string()),
                        json!({ "beat": beat, "intent": intent.id, "manifestation": j.m }),
                    );
                    // Rules 0.8.0: the attempt's d20 is shown (and remembered: the same attempt, the same roll).
                    if let Some(r) = &j.dice {
                        let pos = j.m.at.unwrap_or(w.demigods[d.ix()].pos);
                        let reason = format!("manifesting a {}", j.m.verdict.tier.as_str());
                        crate::dm::record_roll(w, beat, &j.m.id.to_string(), Ref::Demigod(d), r, &reason, pos);
                    }
                    judged.push(j);
                }
            }
            Route::World | Route::Pending => match crate::society::society_intent(w, &intent) {
                // The order goes TO THE GENERAL, who takes it by their own judgment (the general mind).
                Some(crate::society::SocietyAct::Order { army, target, text }) => {
                    if !general_meta.iter().any(|(m, _)| m.army == army) {
                        general_meta.push((
                            crate::society::GeneralMeta {
                                army,
                                by: d,
                                why: "order",
                                text,
                                agent: Some(hand),
                                intent: Some(intent.id),
                            },
                            target,
                        ));
                    }
                }
                Some(crate::society::SocietyAct::Props(p)) => army_props.extend(p),
                None if is_army_act(w, &intent) => army_props.extend(army_intent(w, &intent, beat)),
                None => world_act(w, &mut vicinities),
            },
            Route::God { god, .. } => {
                // An unmade god (health 0) can't rule: Entropy answers instead.
                let god = match (w.gods.get(god.ix()), pantheon.id_of("entropy")) {
                    (Some(g), Some(e)) if g.health <= 0.0 => e,
                    _ => *god,
                };
                let god = &god;
                let Some(def) = pantheon.get(*god).cloned() else {
                    world_act(w, &mut vicinities);
                    continue;
                };
                let invoker = Ref::Demigod(d);
                let me = &w.demigods[d.ix()];
                let mut refs = vec![invoker];
                refs.extend(intent.targets.iter().filter_map(|t| match t {
                    Target::Known(r) => Some(*r),
                    _ => None,
                }));
                refs.extend(near(w, me.pos, 3.0).into_iter().take(8));
                refs.dedup();
                let gs = &w.gods[god.ix()];
                let paladins: Vec<Ref> = gs
                    .paladins
                    .iter()
                    .filter(|c| w.characters[c.ix()].alive)
                    .map(|c| Ref::Char(*c))
                    .collect();
                let context: Vec<String> = w
                    .ledger
                    .rows
                    .iter()
                    .rev()
                    .filter(|r| r.actor == invoker || r.targets.contains(&invoker))
                    .take(5)
                    .map(|r| r.act.clone())
                    .collect();
                let ruling = RulingId::from_ix(w.rulings.len() + god_calls.len());
                god_calls.push(MindCall::God(GodCtx {
                    god: *god,
                    def: def.clone(),
                    beat,
                    ruling,
                    intent: intent.clone(),
                    invoker,
                    invoker_name: me.name.clone(),
                    favor: gs.favor.get(d.ix()).copied().unwrap_or(0.0),
                    open_debt: open_debt(w, invoker, Some(*god)),
                    entities: entities(w, &refs[1..]),
                    paladins: entities(w, &paladins),
                    rivals: def
                        .rivals
                        .iter()
                        .filter_map(|s| pantheon.id_of(s))
                        .map(|g| w.name_of(Ref::God(g)))
                        .collect(),
                    context: context.join("\n"),
                    bolts: me.bolts,
                    // A god-tier power's bolts were paid when it was cast.
                    cost: if intent.how == crate::godtier::HOW { 0 } else { invoke_cost(&def, intent.magnitude) },
                    world: fn7_society::entropy_prose(w),
                    dm: w.dm_rules(),
                    dm_memory: if w.dm_rules() {
                        crate::dm::prompt_lines(w, district_of(w, me.place), beat, st.dm_memory_lines as usize)
                    } else {
                        vec![]
                    },
                }));
                let g = &mut w.gods[god.ix()];
                g.attention = 1.0;
                g.invocations += 1;
            }
            Route::Spawn { goal, budget_beats } => {
                if live_children(w, hand).len() >= st.max_subagents_per_hand as usize {
                    world_act(w, &mut vicinities);
                    continue;
                }
                let scope: Vec<Ref> = intent
                    .targets
                    .iter()
                    .filter_map(|t| match t {
                        Target::Known(r) => Some(*r),
                        _ => None,
                    })
                    .collect();
                // Numbered across both hands, so two subagents of one demigod never share a name (nor their news).
                let n = hands_of(&w.demigods[d.ix()])
                    .iter()
                    .map(|h| w.agents[h.ix()].children.len())
                    .sum::<usize>()
                    + 1;
                let name = format!("{}'s {} #{n}", w.demigods[d.ix()].name, title(&intent.verb));
                let a = spawn_agent(
                    w,
                    AgentKind::Subagent { parent: hand },
                    name,
                    goal.clone(),
                    scope,
                    Some(*budget_beats),
                    beat,
                );
                record_mut(w, beat).spawned.push(a);
            }
            Route::Possess => {
                let body = intent.targets.iter().find_map(|t| match t {
                    Target::Known(Ref::Char(c))
                        if w.characters[c.ix()].alive
                            && w.characters[c.ix()].possessed_by.is_none()
                            && w.characters[c.ix()].serves.is_none() =>
                    {
                        Some(*c)
                    }
                    _ => None,
                });
                match body {
                    Some(c)
                        if live_children(w, hand).len() < st.max_subagents_per_hand as usize =>
                    {
                        let name = format!(
                            "{} (steered by {})",
                            w.characters[c.ix()].name,
                            w.demigods[d.ix()].name
                        );
                        let goal = format!(
                            "Act as {}'s puppet: {}",
                            w.demigods[d.ix()].name,
                            intent.span
                        );
                        let a = spawn_agent(
                            w,
                            AgentKind::Possession {
                                parent: hand,
                                body: c,
                            },
                            name,
                            goal,
                            vec![],
                            None,
                            beat,
                        );
                        w.characters[c.ix()].possessed_by = Some(a);
                        record_mut(w, beat).spawned.push(a);
                    }
                    _ => world_act(w, &mut vicinities),
                }
            }
            Route::Zoom { zoom, focus } => {
                let f = match focus {
                    Some(Target::Known(r)) => Some(*r),
                    Some(Target::Mention(m)) => match_named(w, m),
                    None => None,
                };
                set_zoom(w, d, *zoom, f, beat);
            }
            Route::Recall { agent } => {
                let ok = w
                    .agents
                    .get(agent.ix())
                    .is_some_and(|a| a.is_live() && a.parent().is_some())
                    && hand_of(w, *agent) == Some(hand);
                if ok {
                    end_agent(
                        w,
                        *agent,
                        AgentStatus::Recalled,
                        "Recalled by its hand.".into(),
                        beat,
                    );
                }
            }
        }
    }
    // Generals: the orders hands gave this beat, then decision points (engagements, strain) worth the general mind.
    let focus = crate::society::focus(w, route_from);
    let ordered: Vec<ClusterId> = general_meta.iter().map(|(m, _)| m.army).collect();
    let n_orders = general_meta.len();
    if llm.is_some() {
        general_meta.extend(crate::society::decision_calls(w, &focus, &ordered, st.general_calls_per_beat as usize));
    }
    let mut general_calls: Vec<MindCall> = vec![];
    let mut general_use: Vec<bool> = vec![];
    let mut general_of: Vec<crate::society::GeneralMeta> = vec![];
    for (k, (m, target)) in general_meta.iter().enumerate() {
        if let Some(ctx) = crate::society::general_call(w, m, *target, beat) {
            general_calls.push(MindCall::General(ctx));
            // Orders from a demigod who spoke always ask the general; decision points only with the LLM on.
            general_use.push(k >= n_orders || spoke.contains(&m.by));
            general_of.push(m.clone());
        }
    }
    // Steps: every live subagent, possession and paladin.
    let mut step_calls: Vec<MindCall> = vec![];
    let mut step_llm: Vec<bool> = vec![];
    let ruled: Vec<GodId> = god_calls
        .iter()
        .filter_map(|c| match c {
            MindCall::God(g) => Some(g.god),
            _ => None,
        })
        .collect();
    for a in w.agents.iter().filter(|a| a.is_live()) {
        let (kind, body) = match a.kind {
            AgentKind::Subagent { .. } => (StepKind::Subagent, None),
            AgentKind::Possession { body, .. } => (StepKind::Possession, Some(body)),
            AgentKind::Paladin { body, .. } => (StepKind::Paladin, Some(body)),
            _ => continue,
        };
        if body.is_some_and(|b| !w.characters[b.ix()].alive) {
            continue;
        }
        let demigod = fn7_society::demigod_of(w, Ref::Agent(a.id));
        if demigod.is_some_and(|d| !w.demigods[d.ix()].alive) {
            continue;
        }
        let master_name = match (demigod, a.kind) {
            (Some(d), _) => w.demigods[d.ix()].name.clone(),
            (None, AgentKind::Paladin { god, .. }) => {
                format!("the god {}", w.name_of(Ref::God(god)))
            }
            _ => "someone".into(),
        };
        let place = body
            .map(|b| w.characters[b.ix()].place)
            .or(demigod.map(|d| w.demigods[d.ix()].place))
            .unwrap_or(w.meta.arena);
        let mut vis = present_in(w, district_of(w, place));
        vis.truncate(24);
        let orders = match a.kind {
            AgentKind::Possession { parent, .. } => {
                hand_text.get(&parent).cloned().unwrap_or_default()
            }
            _ => vec![],
        };
        step_llm.push(match a.kind {
            AgentKind::Paladin { god, .. } => ruled.contains(&god),
            _ => demigod.is_some_and(|d| spoke.contains(&d)),
        });
        step_calls.push(MindCall::Step(StepCtx {
            kind,
            agent: a.id,
            name: a.name.clone(),
            persona: a.persona.clone(),
            player: demigod.and_then(|d| w.demigods.get(d.ix())).and_then(|d| d.persona.clone()),
            goal: a.goal.clone(),
            beat,
            steps_taken: a.steps,
            beats_left: a
                .budget_beats
                .map(|b| (a.spawned_beat + b).saturating_sub(beat)),
            demigod,
            master_name,
            body: body.and_then(|b| entity(w, Ref::Char(b))),
            intent: None,
            place: Some(place),
            scope: entities(w, &a.scope),
            visible: entities(w, &vis),
            orders,
            memory: recall(w, Ref::Agent(a.id), &a.scope, 6),
            items_by_engine: w.two_bodies(),
        }));
    }
    let eliminated_names: Vec<String> = w
        .demigods
        .iter()
        .filter(|d| !d.alive)
        .map(|d| d.name.clone())
        .collect();
    let alive = w.alive_demigods().count() as u32;
    let mut resolver_calls: Vec<MindCall> = vec![];
    let mut resolver_llm: Vec<bool> = vec![];
    let entropy_line = fn7_society::entropy_prose(w);
    for (place, acts) in vicinities {
        // Rules 0.8.0: each act's d20 and the DM's memory of this place.
        let dm_view = w.dm_rules().then(|| crate::dm::resolver_view(w, beat, place, &acts, &st));
        resolver_llm.push(acts.iter().any(|a| spoke.contains(&a.intent.demigod) && a.intent.beat == beat && !is_standing(w, &a.intent)));
        let mut world_lines = vec![entropy_line.clone()];
        if is_final_wish(w, beat) {
            world_lines.push(FINAL_WISH_LINE.into());
        }
        world_lines.extend(
            w.mods
                .iter()
                .filter(|m| m.region == place)
                .take(4)
                .map(|m| format!("Lasting here: {} (x{}): {}", m.name, m.stacks, m.description)),
        );
        let p = &w.places[place.ix()];
        let mut present = present_in(w, place);
        present.truncate(30);
        for a in &acts {
            if !present.contains(&a.actor) {
                present.push(a.actor);
            }
        }
        let recent: Vec<String> = w
            .ledger
            .rows
            .iter()
            .rev()
            .filter(|r| r.place.is_some_and(|x| district_of(w, x) == place) && r.verb != "note")
            .take(6)
            .map(|r| r.act.clone())
            .collect();
        resolver_calls.push(MindCall::Resolver(ResolverCtx {
            place,
            place_name: p.name.clone(),
            place_blurb: format!("{}; {}", p.terrain, p.tags.join(", ")),
            center: p.center,
            beat,
            beats_total: w.meta.beats_total,
            alive_demigods: alive,
            present: entities(w, &present),
            acts,
            recent,
            overused: vec![],
            eliminated: eliminated_names.clone(),
            world: world_lines,
            accident_scale: st.accident_scale,
            items_by_engine: w.two_bodies(),
            dm: dm_view,
        }));
    }
    clock.lap(Stage::Route);

    // ---- (4a) act: resolvers, gods, steps and flavor in one concurrent batch ---------------------------------------
    let (nr, ng, ns) = (resolver_calls.len(), god_calls.len(), step_calls.len());
    let first_ramify_ruling = w.rulings.len() + god_calls.len();
    let mut use_llm: Vec<bool> = resolver_llm;
    use_llm.extend(std::iter::repeat_n(true, god_calls.len()));
    use_llm.extend(step_llm);
    use_llm.extend(std::iter::repeat_n(true, flavor_calls.len()));
    use_llm.extend(general_use.iter().copied());
    let mut calls = resolver_calls;
    calls.extend(god_calls);
    calls.extend(step_calls);
    calls.extend(flavor_calls);
    let first_general = calls.len();
    calls.extend(general_calls);
    for (k, mut c) in casts.ramify.iter().cloned().enumerate() {
        c.ruling = RulingId::from_ix(first_ramify_ruling + k);
        calls.push(MindCall::Ramify(c));
        use_llm.push(true);
    }
    // The manifest mind: words for what appeared (LLM only for big tiers and twists by a demigod whose player spoke,
    // within the beat's budget; the rules name everything else).
    let first_manifest = calls.len();
    let mut manifest_of: Vec<usize> = vec![];
    let mut manifest_budget = st.manifest_calls_per_beat;
    for (k, j) in judged.iter().enumerate() {
        if let Some(c) = &j.call {
            let ask = j.ask && manifest_budget > 0 && spoke.contains(&j.m.demigod);
            if ask {
                manifest_budget -= 1;
            }
            calls.push(MindCall::Manifest(c.clone()));
            use_llm.push(ask);
            manifest_of.push(k);
        }
    }
    // Rules 0.8.0: the summon mind tells each summoning's transit (the LLM for a player's summon when it is on).
    let first_summon = calls.len();
    for p in &summons {
        calls.push(MindCall::Summon(p.ctx.clone()));
        use_llm.push(true);
    }
    let mut summon_deeds: Vec<String> = vec![String::new(); summons.len()];
    // Rules 0.8.1: the duel mind tells each finished duel (the LLM when a human fought in it).
    let first_duel = calls.len();
    for p in &duels.ended {
        calls.push(MindCall::Duel(p.ctx.clone()));
        use_llm.push(p.human);
    }
    let mut duel_deeds: Vec<String> = vec![String::new(); duels.ended.len()];
    let mut manifest_words: BTreeMap<usize, (fn7_agents::manifest::ManifestOut, bool)> = BTreeMap::new();
    let t0 = Instant::now();
    let rep = overlap(run_split(llm, calls.clone(), use_llm.clone(), timeout), &mut bg, &mut bg_rep).await;
    count(&rep, t0.elapsed().as_secs_f64() * 1000.0);
    clock.lap(Stage::Act);

    // ---- (4b) merge -----------------------------------------------------------------------------------------------
    let mut props: Vec<Proposal> = std::mem::take(&mut w.pending);
    let mut llm_refs: Vec<Ref> = vec![];
    let mut finished: Vec<(AgentId, String)> = vec![];
    let mut step_said: BTreeMap<AgentId, String> = BTreeMap::new();
    let time = pantheon.id_of("time");
    for (k, (call, o)) in calls.iter().zip(rep.replies).enumerate() {
        match (call, o.value) {
            (MindCall::Resolver(_), MindReply::Resolver(r)) => {
                // Rules 0.8.0: a result that contradicts the recorded dice is reconciled to the record.
                if w.dm_rules() {
                    props.extend(crate::dm::reconcile_resolver(w, beat, r.proposals));
                } else {
                    props.extend(r.proposals);
                }
            }
            (MindCall::God(ctx), MindReply::God(g)) => {
                let mut ruling = g.ruling;
                ruling.fallback = ruling.fallback || o.fallback.is_some();
                ruling.at = fn7_society::geo::pos_of(w, ruling.invoker);
                // The god's price in bolts (paid unless refused), Entropy's gift, Time's cooling.
                let inv = fn7_society::demigod_of(w, ruling.invoker);
                if let Some(d) = inv {
                    if ruling.verdict != Verdict::Refuse && ctx.cost > 0 && ctx.can_pay() {
                        ruling.bolts = ctx.cost;
                        props.push(god_prop(
                            &ruling,
                            &ctx.intent,
                            Effect::Bolts { who: d, delta: -(ctx.cost as i32), to: Some(ruling.god), toll: false },
                            format!("{} pays {} {} bolts", w.demigods[d.ix()].name, ctx.def.name, ctx.cost),
                        ));
                    }
                    if ctx.is_entropy() && ruling.verdict != Verdict::Refuse {
                        props.push(god_prop(
                            &ruling,
                            &ctx.intent,
                            Effect::Bolts { who: d, delta: ENTROPY_GIFT as i32, to: Some(ruling.god), toll: false },
                            format!("Entropy showers {} with fresh bolts", w.demigods[d.ix()].name),
                        ));
                    }
                }
                let text0 = format!("{} {} {}", ctx.intent.verb, ctx.intent.span, ctx.intent.how).to_lowercase();
                if Some(ruling.god) == time
                    && ruling.verdict != Verdict::Refuse
                    && ["entropy", "cool", "chaos", "order", "calm"].iter().any(|k| text0.contains(k))
                {
                    let delta = -(4.0 + 1.2 * ctx.intent.magnitude as f32);
                    props.push(god_prop(
                        &ruling,
                        &ctx.intent,
                        Effect::Entropy { delta },
                        "Time cools the world down, a few degrees of order bought back".into(),
                    ));
                }
                event("ruling", Some(ruling.god.to_string()), json!(ruling));
                record_mut(w, beat).rulings.push(ruling.id);
                // Rules 0.8.0: a god-tier power's signature effect, unless the god refused.
                if ctx.intent.how == crate::godtier::HOW && ruling.verdict != Verdict::Refuse {
                    props.extend(crate::godtier::signature(w, pantheon, &ruling, &ctx.intent));
                }
                // Time, asked to put off a debt, delays the invoker's open debts (a Delay beats a Collect).
                let text = format!("{} {} {}", ctx.intent.verb, ctx.intent.span, ctx.intent.how)
                    .to_lowercase();
                if Some(ruling.god) == time
                    && ruling.verdict != Verdict::Refuse
                    && ["delay", "postpone", "later", "debt", "put off"]
                        .iter()
                        .any(|k| text.contains(k))
                {
                    let mut debts: Vec<&LedgerRow> = w
                        .ledger
                        .open_debts()
                        .filter(|r| r.debt.is_some_and(|d| d.owed_by == ruling.invoker))
                        .collect();
                    debts.sort_by_key(|r| (r.debt.map(|d| d.due_beat), r.id));
                    for r in debts.into_iter().take(3) {
                        props.push(Proposal {
                            source: Source::God {
                                god: ruling.god,
                                ruling: Some(ruling.id),
                            },
                            intent: Some(ruling.intent),
                            effect: Effect::Delay {
                                row: r.id,
                                beats: 3,
                            },
                            act: format!(
                                "{} pushes {}'s debt back, with interest",
                                w.name_of(Ref::God(ruling.god)),
                                w.name_of(ruling.invoker)
                            ),
                            actor: Ref::God(ruling.god),
                            magnitude: ctx.intent.magnitude,
                            valence: 1,
                            witnesses: vec![ruling.invoker],
                            cause: Some(r.id),
                        });
                    }
                }
                // Rules 0.8.1: a heal the god didn't refuse heals its targets (by magnitude, half when twisted),
                // whatever words the god chose.
                if st.rules_0_8_1 && ruling.verdict != Verdict::Refuse && fn7_agents::hand::is_heal(&ctx.intent.verb, &ctx.intent.span) {
                    props.extend(heal_props(w, &ruling, &ctx.intent, &g.proposals));
                }
                w.rulings.push(ruling);
                props.extend(g.proposals);
            }
            (MindCall::Step(ctx), MindReply::Step(s)) => {
                let ag = &mut w.agents[ctx.agent.ix()];
                ag.steps += 1;
                event(
                    "agent_step",
                    Some(ctx.agent.to_string()),
                    json!({ "beat": beat, "agent": ctx.agent, "step": ag.steps, "said": s.said, "done": s.done,
                            "proposals": s.proposals.len(), "fallback": o.fallback }),
                );
                if !s.said.is_empty() {
                    step_said.insert(ctx.agent, s.said.clone());
                }
                if let Some(rep) = s.done {
                    finished.push((ctx.agent, rep));
                }
                props.extend(s.proposals);
            }
            (MindCall::Ramify(ctx), MindReply::Ramify(r)) => {
                let k = ctx.ruling.ix() - first_ramify_ruling;
                let (intent, by, at) = casts.ramify_intents[k];
                let ruling = Ruling {
                    id: ctx.ruling,
                    beat,
                    god: ctx.god,
                    intent,
                    invoker: Ref::Demigod(by),
                    verdict: Verdict::Grant,
                    price: None,
                    speech: r.speech.clone(),
                    dispatched: None,
                    dispatch_mission: None,
                    fallback: o.fallback.is_some(),
                    bolts: ctx.casts.iter().map(|c| c.cost).sum(),
                    at: Some(at),
                };
                event("ruling", Some(ruling.god.to_string()), json!(ruling));
                event(
                    "ramifications",
                    Some(Ref::Place(ctx.region).to_string()),
                    json!({ "beat": beat, "region": ctx.region, "n": ctx.n, "casts": ctx.casts.len(),
                            "proposals": r.proposals.len(), "ruling": ruling.id, "fallback": o.fallback }),
                );
                record_mut(w, beat).rulings.push(ruling.id);
                w.rulings.push(ruling);
                props.extend(r.proposals);
            }
            (MindCall::General(ctx), MindReply::General(j)) => {
                let m = &general_of[k - first_general];
                let asked = use_llm.get(k).copied().unwrap_or(false) && llm.is_some() && o.fallback.is_none();
                if asked {
                    llm_refs.push(Ref::Cluster(ctx.army));
                }
                props.push(crate::society::orders_prop(w, m, j, asked));
            }
            (MindCall::Duel(_), MindReply::Duel(out)) => {
                if let Some(x) = duel_deeds.get_mut(k - first_duel) {
                    *x = out.deed;
                }
            }
            (MindCall::Summon(_), MindReply::Summon(out)) => {
                if let Some(x) = summon_deeds.get_mut(k - first_summon) {
                    *x = out.deed;
                }
            }
            (MindCall::Manifest(_), MindReply::Manifest(words)) => {
                let asked = use_llm.get(k).copied().unwrap_or(false) && llm.is_some() && o.fallback.is_none();
                if let Some(j) = manifest_of.get(k - first_manifest) {
                    manifest_words.insert(*j, (words, asked));
                }
            }
            (MindCall::Materialize(ctx), MindReply::Materialize(flavor)) => {
                if let Some(c) = w
                    .characters
                    .iter()
                    .rev()
                    .find(|c| c.name == ctx.name)
                    .map(|c| c.id)
                {
                    let id = fn7_memory::write(
                        &mut w.memory,
                        NewMemory {
                            owner: Ref::Char(c),
                            beat,
                            kind: MemoryKind::Summary,
                            text: format!("Who I am: {flavor}"),
                            about: vec![Ref::Cluster(ctx.cluster)],
                            ledger: None,
                            from: None,
                            salience: 5,
                            confidence: 1.0,
                        },
                    );
                    event(
                        "memory_write",
                        Some(Ref::Char(c).to_string()),
                        json!({ "beat": beat, "items": [w.memory.items[id.ix()]], "flavor": true }),
                    );
                }
            }
            _ => {}
        }
        let _ = (k, nr, ng, ns);
    }
    // Rules 0.8.0: the summonings' deeds, in the story and the memory at both places.
    if !summons.is_empty() {
        crate::summon::record(w, beat, &summons, &summon_deeds);
    }
    // Rules 0.8.1: the duels' deeds (duel.end in the memory, a tale each).
    let duel_ends = crate::duel::record(w, beat, &duels.ended, &duel_deeds);
    // Manifestations: the payment, debt, backfire and Entropy, then the attempt itself (its row and its item).
    for (k, j) in judged.iter().enumerate() {
        props.extend(j.props.iter().cloned());
        let (words, asked) = match manifest_words.get(&k) {
            Some((wd, a)) => (Some(wd), *a),
            None => (None, false),
        };
        props.push(crate::items::proposal(w, j, words, asked, &st));
    }
    // The world's own rules this beat: armies the hands raised or sent, armies marching and fighting, Entropy's
    // wackiness, the lasting world mods.
    props.extend(army_props);
    props.extend(fn7_society::entropy_proposals(w, beat, pantheon.id_of("entropy")));
    props.extend(fn7_society::mod_proposals(w, beat));
    // Rules 0.8.0: a demigod summoned this beat stays where it was summoned (its hands' travel this beat is void).
    // Rules 0.8.1: so does a built-in AI demigod that leapt to its duel, and every duellist (locked in its duel).
    let leapt: Vec<DemigodId> = duels.leapt.iter().chain(&duels.pinned).copied().collect();
    if !summons.is_empty() || !leapt.is_empty() {
        props.retain(|p| {
            !matches!(&p.effect, Effect::Move { who: Ref::Demigod(d), .. }
                      if summons.iter().any(|s| s.demigod == *d) || leapt.contains(d))
        });
    }
    let entropy_before = entropy_at_start;
    let n_props = props.len();
    let (props, applied) = merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
    // Consequence's bill: one debt per demigod per beat, for every player-originated act that landed.
    let consequence = pantheon.id_of("consequence");
    let mut bills: BTreeMap<DemigodId, (BTreeMap<String, u8>, Option<LedgerId>)> = BTreeMap::new();
    for (i, row) in &applied.rows {
        let p = &props[*i];
        let (d, key) = match p.source {
            Source::Rules => continue,
            Source::Agent { agent } => (
                fn7_society::demigod_of(w, Ref::Agent(agent)),
                p.intent.map(|x| x.to_string()).unwrap_or(agent.to_string()),
            ),
            Source::Resolver => (
                p.intent
                    .and_then(|x| w.intents.get(x.ix()))
                    .map(|x| x.demigod),
                format!("{:?}", p.intent),
            ),
            Source::God { ruling, .. } => (
                ruling
                    .and_then(|r| w.rulings.get(r.ix()))
                    .and_then(|r| fn7_society::demigod_of(w, r.invoker)),
                format!("{ruling:?}"),
            ),
        };
        let Some(d) = d else { continue };
        if matches!(
            p.effect,
            Effect::Debt { .. } | Effect::Price { .. } | Effect::Favor { .. } | Effect::Note { .. }
        ) && p.magnitude == 0
        {
            continue;
        }
        let e = bills.entry(d).or_default();
        let m = e.0.entry(key).or_default();
        *m = (*m).max(p.magnitude);
        e.1.get_or_insert(*row);
    }
    let mut bill_props = vec![];
    if let Some(cg) = consequence {
        for (d, (acts, cause)) in &bills {
            if !w.demigods[d.ix()].alive {
                continue;
            }
            let words = words_by.get(d).copied().unwrap_or(0);
            let amount: f32 = acts
                .values()
                .map(|m| fn7_society::debt_for(*m, words))
                .sum::<f32>()
                * st.debt_scale;
            let amount = (amount.min(10.0) * 100.0).round() / 100.0;
            if amount < 0.05 {
                continue;
            }
            bill_props.push(Proposal {
                source: Source::God {
                    god: cg,
                    ruling: None,
                },
                intent: None,
                effect: Effect::Debt {
                    owed_by: Ref::Demigod(*d),
                    god: cg,
                    amount,
                    due_in: st.debt_due_beats,
                },
                act: format!(
                    "Consequence writes up {}'s bill for this beat ({} acts)",
                    w.demigods[d.ix()].name,
                    acts.len()
                ),
                actor: Ref::God(cg),
                magnitude: amount.round().clamp(1.0, 10.0) as u8,
                valence: -1,
                witnesses: vec![Ref::Demigod(*d)],
                cause: *cause,
            });
        }
    }
    let n_bills = bill_props.len();
    let (_, billed) = merge::run(w, bill_props, pantheon, beat, st.max_witnesses as usize);
    let applied_n = (applied.rows.len() + billed.rows.len()) as u32;
    // ---- (4c) the society: every level of the hierarchy, by rules (generals already decided above) -----------------
    let soc = crate::society::run_step(w, beat, &focus, llm_refs, pantheon, &st);
    for (a, report) in finished {
        if w.agents[a.ix()].is_live() {
            end_agent(w, a, AgentStatus::Done, report, beat);
        }
    }
    // Rules 0.7.0: manifested hosts follow their armies; no two demigods on one people-level hex unless engaged.
    if w.two_bodies() {
        crate::items::sync_hosts(w, beat);
        crate::occupancy::enforce(w, beat, row_start, &pos_at_start);
    }
    // Rules 0.8.0: the Dungeon Master's step (encounters, shrines, loot, story pieces, settlements, roads).
    if w.dm_rules() {
        let start = crate::dm::Start {
            alive: &alive_at_start,
            pos: &pos_at_start,
            health: &health_at_start,
            armies: &armies_at_start,
            row_start,
            intent_start: first_new_intent,
            manifest_start,
            memory_start,
        };
        crate::dm::step(w, beat, &start, pantheon, &st);
    }
    clock.lap(Stage::Merge);

    // ---- (5) consequence: memories, rumor spread, mood drift, memory budget --------------------------------------
    let row_end = w.ledger.rows.len();
    for r in row_start..row_end {
        let ids = write_memories(w, LedgerId::from_ix(r), beat);
        if !ids.is_empty() {
            let items: Vec<&MemoryItem> = ids.iter().map(|i| &w.memory.items[i.ix()]).collect();
            event(
                "memory_write",
                None,
                json!({ "beat": beat, "row": LedgerId::from_ix(r), "items": items }),
            );
        }
    }
    let hops = fn7_society::spread(w, beat);
    let mut spread_rows: Vec<LedgerId> = vec![];
    for h in &hops {
        event("spread", Some(h.row.to_string()), json!(h));
        if spread_rows.last() != Some(&h.row) {
            spread_rows.push(h.row);
        }
    }
    for r in spread_rows {
        let ids = write_memories(w, r, beat);
        if !ids.is_empty() {
            let items: Vec<&MemoryItem> = ids.iter().map(|i| &w.memory.items[i.ix()]).collect();
            event(
                "memory_write",
                None,
                json!({ "beat": beat, "row": r, "items": items, "rumor": true }),
            );
        }
    }
    fn7_society::drift(w, beat);
    // Rows past the spread horizon never spread again: their per-group learn beats (`known_at`, all in the recording's
    // `spread` and `ledger_row` events) leave the live state; `known_by` stays.
    if let Some(old) = beat.checked_sub(fn7_society::SPREAD_HORIZON + 1) {
        for r in w.ledger.rows.iter_mut().filter(|r| r.beat == old) {
            r.known_at.clear();
        }
    }
    let entropy_after = w.meta.entropy;
    let (lb, la) = (
        fn7_society::entropy_level(entropy_before),
        fn7_society::entropy_level(entropy_after),
    );
    if entropy_after != entropy_before {
        event(
            "entropy",
            None,
            json!({ "beat": beat, "before": entropy_before, "after": entropy_after,
                    "delta": entropy_after - entropy_before, "level": la }),
        );
    }
    if la != lb {
        event(
            "entropy_level",
            None,
            json!({ "beat": beat, "from": lb, "to": la, "key": fn7_society::entropy_key(la) }),
        );
    }
    if w.meta.heat_death == Some(beat) {
        event(
            "heat_death",
            None,
            json!({ "beat": beat, "entropy": entropy_after, "beats_total": w.meta.beats_total }),
        );
    }
    clock.lap(Stage::Consequence);

    // ---- (6) narrate: collect the background batch (usually long done), last beat's caption, the memory folds ------
    let bg_rep = match bg_rep.take() {
        Some(r) => r,
        None => bg.await,
    };
    count(&bg_rep, bg_t0.elapsed().as_secs_f64() * 1000.0);
    let mut replies = bg_rep.replies.into_iter();
    let mut previous: Option<(u32, Narration)> = None;
    if let Some(b) = narrate {
        if let Some(o) = replies.next() {
            if let MindReply::Narrator(n) = o.value {
                set_caption(w, b, &n, o.fallback.is_some());
                previous = Some((b, n));
            }
        }
    }
    let mut folds: Vec<MemoryId> = vec![];
    for (plan, o) in fold_plans.iter().zip(replies) {
        let MindReply::Fold(text) = o.value else {
            continue;
        };
        // Items a Forget took this beat stay out of the summary.
        let mut plan = plan.clone();
        plan.items
            .retain(|i| w.memory.items.get(i.ix()).is_some_and(fn7_memory::is_active));
        if plan.items.len() < 2 {
            continue;
        }
        let text = if text.trim().is_empty() {
            fn7_memory::rules_summary(w, &plan)
        } else {
            fn7_memory::scrub(w, &text)
        };
        folds.push(fn7_memory::apply_fold(&mut w.memory, &plan, &text, beat));
    }
    // Owners still over budget fold by rules now, except the next few, which the fold mind writes next beat.
    let deferred: Vec<Ref> = if llm.is_some() {
        llm_fold_plans(w, per_owner, st.llm_folds_per_beat as usize)
            .into_iter()
            .map(|p| p.owner)
            .collect()
    } else {
        vec![]
    };
    folds.extend(enforce_budget_except(w, per_owner, beat, &deferred));
    for f in folds {
        let s = w.memory.items[f.ix()].clone();
        let originals: Vec<&MemoryItem> = w
            .memory
            .items
            .iter()
            .filter(|m| m.folded_into == Some(f))
            .collect();
        let from: Vec<MemoryId> = originals.iter().map(|m| m.id).collect();
        event(
            "memory_fold",
            Some(s.owner.to_string()),
            json!({ "beat": beat, "summary": s, "from": from, "originals": originals }),
        );
        compact_folded(w, f);
    }
    clock.lap(Stage::Narrate);

    // ---- (7) broadcast --------------------------------------------------------------------------------------------
    let now_dead: Vec<String> = alive_at_start
        .iter()
        .filter(|d| !w.demigods[d.ix()].alive)
        .map(|d| w.demigods[d.ix()].name.clone())
        .collect();
    let public_omens: Vec<Omen> = w
        .rulings
        .iter()
        .filter(|r| r.beat == beat && ruling_is_public(w, r))
        .map(|r| crate::payload::omen(w, r))
        .collect();
    let row_end = w.ledger.rows.len();
    let mut per_seat = seat_deltas(w, beat, row_start, row_end, &said, &step_said, &flow);
    // Rules 0.8.1: what became of each fallen player's will (and Death's verdict on a plea to come back).
    for (seat, v) in per_seat.iter_mut() {
        let mine: Vec<Value> = will_notes.iter().filter(|n| n.seat == *seat).map(|n| n.json(w)).collect();
        if !mine.is_empty() {
            v["wills"] = mine.into();
        }
    }
    let (prev_beat, prev) = match previous {
        Some((b, n)) => (Some(b), n),
        None => (
            None,
            Narration {
                caption: String::new(),
                callouts: vec![],
            },
        ),
    };
    // Beat N's caption arrives with beat N+1 (its narrator ran off the critical path); `caption` / `callouts` are the
    // same text, kept for older clients, with `caption_beat` saying which beat they describe.
    let public = json!({
        "beat": beat,
        "beats_total": w.meta.beats_total,
        "previous_caption": prev.caption,
        "previous_callouts": prev.callouts,
        "caption": prev.caption,
        "callouts": prev.callouts,
        "caption_beat": prev_beat,
        "eliminated": now_dead,
        "alive": w.alive_demigods().count(),
        "arena_radius_km": w.meta.arena_radius_km,
        "omens": public_omens,
        "entropy": crate::payload::entropy_json(w, Some(w.meta.entropy - entropy_at_start)),
        "slow": flow.public,
        "casts": casts.lines,
        "events": crate::payload::events(w, &w.ledger.rows[row_start..row_end]),
        "society": crate::society::public_json(w),
    });
    let mut public = public;
    if w.two_bodies() {
        public["players_alive"] = w.alive_seats().len().into();
        public["manifests"] = w
            .manifests
            .iter()
            .filter(|m| m.beat == beat && m.public)
            .map(|m| crate::items::manifest_json(w, m, false))
            .collect();
    }
    if w.dm_rules() {
        crate::dm::beat_public(w, beat, memory_start, &st, &mut public);
        public["god_powers"] = god_tier.into();
        let summoned: Vec<Value> = public["dm_events"]
            .as_array()
            .map(|e| e.iter().filter(|x| x["event"] == "summoned").cloned().collect())
            .unwrap_or_default();
        // The client's contract: each `summoned` also rides in `events[]` (kind "summoned", at its landing).
        if let Some(ev) = public["events"].as_array_mut() {
            for x in &summoned {
                let mut e = x.clone();
                e["kind"] = "summoned".into();
                e["at"] = x["to"]["pos"].clone();
                ev.push(e);
            }
        }
        public["summons"] = summoned.into();
        if !duel_ends.is_empty() {
            public["duels"] = duel_ends.into();
        }
        if !duels.open.is_empty() {
            public["duels_open"] = duels.open.clone().into();
        }
    }
    clock.lap(Stage::Broadcast);

    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let n_intents = (w.intents.len() - first_new_intent) as u32;
    let rec = record_mut(w, beat);
    rec.utterances = utterances;
    rec.words = words_by.values().sum();
    rec.intents = n_intents;
    rec.proposals = (n_props + n_bills) as u32;
    rec.applied = applied_n;
    rec.rows = (
        (row_end > row_start).then(|| LedgerId::from_ix(row_start)),
        (row_end > row_start).then(|| LedgerId::from_ix(row_end - 1)),
    );
    rec.timing.llm_calls = llm_calls;
    rec.timing.llm_fallbacks = llm_fallbacks;
    // Wall-clock values go to the `beat` event only (the state stays replayable byte for byte).
    let mut timed = rec.clone();
    timed.started_at_ms = started_at;
    timed.timing.ms = clock.ms.clone();
    timed.timing.llm_ms_max = llm_ms_max;
    event(
        "beat",
        None,
        json!({ "beat": beat, "record": timed, "total_ms": total_ms, "society_ms": soc.ms }),
    );
    public["timing"] = json!({
        "total_ms": total_ms,
        "society_ms": soc.ms,
        "society_events": soc.events,
        "society_dropped": soc.dropped,
        "society_emerged": soc.emerged,
        "stages": clock.ms.iter().map(|(s, ms)| json!({"stage": s, "ms": ms})).collect::<Vec<_>>(),
        "llm_calls": llm_calls,
        "llm_fallbacks": llm_fallbacks,
    });
    crate::payload::tidy(&mut public);
    BeatOut { public, per_seat }
}

/// Society rows below this magnitude don't become group rumors (and at most one per row per beat).
pub const SOCIETY_RUMOR_MIN: u8 = 8;
/// Live memory items kept by a person the simulation produced and nobody has addressed (the rest fold).
pub const MINOR_MEMORY_BUDGET: usize = 10;

/// Memory budget of an owner: people the society produced and no player addressed keep fewer items.
pub fn budget_of(w: &World, owner: Ref, per_owner: usize) -> usize {
    match owner {
        Ref::Char(c) => match w.characters.get(c.ix()) {
            Some(x) if x.life.is_some() && x.memorized_by.is_none() && x.possessed_by.is_none() => MINOR_MEMORY_BUDGET.min(per_owner),
            _ => per_owner,
        },
        _ => per_owner,
    }
}

/// Rows below this magnitude don't become group rumors (the groups still know them: `known_by` grows regardless).
pub const RUMOR_MIN_MAGNITUDE: u8 = 6;
/// Most groups that get a rumor memory of one row in one beat.
pub const RUMORS_PER_ROW: usize = 2;
/// Most refs a fold summary keeps in `about` (the most mentioned first).
pub const SUMMARY_ABOUT_MAX: usize = 12;
/// Bookkeeping verbs: the god keeping the books doesn't remember each entry (the debtor does).
const BOOKKEEPING: &[&str] = &["debt", "favor", "price", "collect", "delay"];

/// `fn7_memory::from_row` with three cuts so live state stays bounded: no `Did` memory for a god's bookkeeping rows,
/// no group rumors below [`RUMOR_MIN_MAGNITUDE`], and at most [`RUMORS_PER_ROW`] rumors per row per beat.
fn write_memories(w: &mut World, row: LedgerId, beat: u32) -> Vec<MemoryId> {
    let Some(r) = w.ledger.get(row) else {
        return vec![];
    };
    let (mag, books, actor) = (r.magnitude, BOOKKEEPING.contains(&r.verb.as_str()), r.actor);
    // Society rows (schema 4) are remembered by the people in them (and demigods, minds, gods); a crowd keeps only the
    // big ones, and only the biggest become group rumors, so the society's own churn doesn't flood memory.
    let society = SocKind::of(&r.verb).is_some();
    let mut rumors = 0;
    let news: Vec<NewMemory> = fn7_memory::from_row(w, row, beat)
        .into_iter()
        .filter(|m| match m.kind {
            MemoryKind::Rumor => {
                rumors += 1;
                if society {
                    mag >= SOCIETY_RUMOR_MIN && rumors <= 1
                } else {
                    mag >= RUMOR_MIN_MAGNITUDE && rumors <= RUMORS_PER_ROW
                }
            }
            MemoryKind::Did => !(books && matches!(actor, Ref::God(_))),
            _ if society => match m.owner {
                Ref::Cluster(_) => mag >= 6,
                Ref::Group(_) => false,
                _ => true,
            },
            _ => true,
        })
        .collect();
    news.into_iter()
        .map(|m| fn7_memory::write(&mut w.memory, m))
        .collect()
}

fn line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

fn title(verb: &str) -> String {
    let mut c = verb.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Task".into(),
    }
}

/// Folded originals keep their row (ids stay stable) but drop their prose from the live state: the `memory_fold`
/// event carries them in full, so nothing is lost from the recording.
pub fn compact_folded(w: &mut World, summary: MemoryId) {
    let mut counts: BTreeMap<Ref, usize> = BTreeMap::new();
    for m in w
        .memory
        .items
        .iter()
        .filter(|m| m.folded_into == Some(summary))
    {
        for r in &m.about {
            *counts.entry(*r).or_default() += 1;
        }
    }
    let mut top: Vec<(Ref, usize)> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut keep: Vec<Ref> = top
        .into_iter()
        .take(SUMMARY_ABOUT_MAX)
        .map(|x| x.0)
        .collect();
    keep.sort_unstable();
    if let Some(s) = w.memory.items.get_mut(summary.ix()) {
        s.about.retain(|r| keep.contains(r));
    }
    for m in w
        .memory
        .items
        .iter_mut()
        .filter(|m| m.folded_into == Some(summary))
    {
        m.text.clear();
        m.about.clear();
    }
}

pub fn spawn_agent(
    w: &mut World,
    kind: AgentKind,
    name: String,
    goal: String,
    scope: Vec<Ref>,
    budget: Option<u32>,
    beat: u32,
) -> AgentId {
    let id = AgentId::from_ix(w.agents.len());
    let parent = match kind {
        AgentKind::Subagent { parent } | AgentKind::Possession { parent, .. } => Some(parent),
        _ => None,
    };
    let persona = match kind {
        AgentKind::Possession { body, .. } => format!(
            "The body of {} ({}), steered by a demigod's hand; it keeps its own voice.",
            w.characters[body.ix()].name,
            w.characters[body.ix()].traits.join(", ")
        ),
        _ => "A subagent: fast, literal and loyal to its hand; it reports back when done.".into(),
    };
    w.agents.push(Agent {
        id,
        kind,
        name,
        persona,
        goal,
        scope,
        spawned_beat: beat,
        budget_beats: budget,
        status: AgentStatus::Live,
        ended_beat: None,
        report: None,
        steps: 0,
        children: vec![],
    });
    if let Some(p) = parent {
        w.agents[p.ix()].children.push(id);
    }
    event(
        "agent_spawned",
        Some(id.to_string()),
        json!({ "beat": beat, "agent": w.agents[id.ix()], "by": parent }),
    );
    id
}

/// End an agent (done, recalled, expired): report to its parent's memory, free its body.
pub fn end_agent(w: &mut World, a: AgentId, status: AgentStatus, report: String, beat: u32) {
    let ag = &mut w.agents[a.ix()];
    ag.status = status;
    ag.ended_beat = Some(beat);
    ag.report = Some(report.clone());
    let ag = ag.clone();
    if let AgentKind::Possession { body, .. } = ag.kind {
        if w.characters[body.ix()].possessed_by == Some(a) {
            w.characters[body.ix()].possessed_by = None;
        }
    }
    let owner = match ag.kind {
        AgentKind::Subagent { parent } | AgentKind::Possession { parent, .. } => {
            Some(Ref::Agent(parent))
        }
        AgentKind::Paladin { god, .. } => Some(Ref::Agent(w.gods[god.ix()].agent)),
        _ => None,
    };
    let mut items = vec![];
    if let Some(o) = owner {
        let id = fn7_memory::write(
            &mut w.memory,
            NewMemory {
                owner: o,
                beat,
                kind: MemoryKind::Report,
                text: format!("{} reported: {}", ag.name, report),
                about: ag.scope.clone(),
                ledger: None,
                from: Some(Ref::Agent(a)),
                salience: 5,
                confidence: 1.0,
            },
        );
        items.push(w.memory.items[id.ix()].clone());
    }
    event(
        "agent_ended",
        Some(a.to_string()),
        json!({ "beat": beat, "agent": ag, "status": status, "report": report }),
    );
    if !items.is_empty() {
        event(
            "memory_write",
            owner.map(|o| o.to_string()),
            json!({ "beat": beat, "items": items }),
        );
    }
}

pub fn set_zoom(w: &mut World, d: DemigodId, zoom: Zoom, focus: Option<Ref>, beat: u32) {
    let seat = w.demigods.get(d.ix()).map(|x| x.seat);
    if let Some(s) = w.seats.iter_mut().find(|s| s.demigod == d || Some(s.seat) == seat) {
        s.zoom = zoom;
        s.focus = focus;
        let seat = s.seat;
        event(
            "zoom",
            Some(Ref::Demigod(d).to_string()),
            json!({ "beat": beat, "seat": seat, "zoom": zoom, "focus": focus }),
        );
    }
}

fn how_known(w: &World, me: &[Ref], following: GroupId, r: &LedgerRow) -> &'static str {
    if me.contains(&r.actor) {
        "you did it"
    } else if r.targets.iter().any(|x| me.contains(x)) {
        "it happened to you"
    } else if r.witnessed_by.iter().any(|x| me.contains(x)) {
        "you saw it"
    } else if r.told_to.iter().any(|x| me.contains(x)) {
        "you were told"
    } else if r.known_by.contains(following) {
        let _ = w;
        "your followers heard"
    } else {
        "rumor"
    }
}

/// The rows `d` knows among `rows`, as news lines: newest first, one line per row, refs without repeats.
pub fn news_for(w: &World, d: DemigodId, rows: &[LedgerRow]) -> Vec<NewsLine> {
    let me = selves(w, d);
    let following = w.demigods[d.ix()].following;
    let lines = rows
        .iter()
        .filter(|r| knows(&me, following, r))
        .map(|r| {
            let mut refs = vec![r.actor];
            refs.extend(
                r.targets
                    .iter()
                    .copied()
                    .filter(|t| !matches!(t, Ref::Ledger(_))),
            );
            let (at, region) = crate::payload::geotag(w, r);
            NewsLine {
                row: Ref::Ledger(r.id),
                text: r.act.clone(),
                refs,
                how: how_known(w, &me, following, r).into(),
                at,
                region,
            }
        })
        .collect();
    crate::payload::news_lines(lines)
}

pub fn you_line(w: &World, d: DemigodId) -> YouLine {
    let x = &w.demigods[d.ix()];
    YouLine {
        health: x.health,
        pos: x.pos,
        debt_open: open_debt(w, Ref::Demigod(d), None),
        favor: crate::payload::favor(w, d),
        followers: fn7_society::headcount(w, x.following).min(u32::MAX as u64) as u32,
        echo_charges: x.echo_charges,
        bolts: x.bolts,
        sparks: w.two_bodies().then_some(x.sparks),
    }
}

fn seat_deltas(
    w: &World,
    beat: u32,
    row_start: usize,
    row_end: usize,
    said: &BTreeMap<AgentId, String>,
    step_said: &BTreeMap<AgentId, String>,
    _flow: &crate::flow::FlowOut,
) -> Vec<(Seat, Value)> {
    let rows = &w.ledger.rows[row_start..row_end];
    let mut out = vec![];
    for s in w.seats.iter().filter(|s| s.seat < crate::SYNTHETIC_SEAT) {
        // The seat's lead demigod (its first standing one); news and omens cover both of the player's demigods.
        let d = w.lead_of_seat(s.seat).unwrap_or(s.demigod);
        let mine = w.siblings(d);
        let hands = seat_hands(w, s.seat)
            .into_iter()
            .map(|(h, _)| HandLine {
                agent: Ref::Agent(h),
                name: w.agents[h.ix()].name.clone(),
                said: said.get(&h).cloned().unwrap_or_default(),
                subagents: crate::payload::subagents(w, h, step_said),
            })
            .collect();
        let omens = w
            .rulings
            .iter()
            .filter(|r| {
                r.beat == beat
                    && (ruling_is_public(w, r) || matches!(r.invoker, Ref::Demigod(i) if mine.contains(&i)))
            })
            .map(|r| crate::payload::omen(w, r))
            .collect();
        let delta = SeatDelta {
            beat,
            seat: s.seat,
            zoom: s.zoom,
            focus: s.focus.map(|f| entity_json(w, f)),
            news: news_for(w, d, rows),
            you: you_line(w, d),
            hands,
            omens,
            hierarchy: Some(crate::society::hierarchy_json(w, d, s.zoom, s.focus)).filter(|v| !v.is_null()),
        };
        let mut v = json!(delta);
        if w.two_bodies() && mine.len() > 1 {
            // Rules 0.7.0: each of the player's demigods this beat, and what they manifested.
            v["demigods"] = mine
                .iter()
                .map(|x| {
                    let b = &w.demigods[x.ix()];
                    json!({ "ref": Ref::Demigod(b.id), "side": b.side, "name": b.name, "health": b.health,
                            "max_health": b.max_health, "alive": b.alive, "pos": b.pos,
                            "cell": b.alive.then(|| people_cell(b.pos)) })
                })
                .collect();
            v["manifests"] = w
                .manifests
                .iter()
                .filter(|m| m.beat == beat && mine.contains(&m.demigod))
                .map(|m| crate::items::manifest_json(w, m, true))
                .collect();
        }
        crate::payload::tidy(&mut v);
        out.push((s.seat, v));
    }
    out
}
