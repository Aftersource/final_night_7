//! The hierarchical society simulation (schema 4), all rules, every level every beat:
//! planet → region → city → district → crowd → cell → person.
//!
//! [`step`] runs once per beat after the merge. In order:
//! 1. [`crowds::needs_and_moods`]: supply, hunger, danger, faith; mood dynamics from war, famine, Time's blight outside
//!    the arena, world mods, nearby demigods; hardship shocks.
//! 2. [`crowds::migrate`]: people leave danger and hunger for safety and allegiance.
//! 3. [`crowds::cults`]: worship movements around gods and demigods, with prophets (and paladins).
//! 4. [`mobs::step`]: angry crowds throw off gangs; gangs merge into mobs under a ringleader; mobs riot, loot, attack
//!    and dissolve.
//! 5. [`crowds::clashes`]: crowd-vs-crowd violence between rival allegiances and faiths.
//! 6. [`armies::step`]: the command chain: volunteer hosts, generals' judgment, marches, battles (heroes, cowards,
//!    fallen), desertion, recruitment, strain → mutiny and defection, strikes on demigods.
//! 7. [`people::step`]: petty humans pursue their goals (promotion, revenge, flight, preaching).
//! 8. [`hier::rollup`]: totals roll up the tree; stances settle.
//!
//! The step mutates the society tables directly (like `drift`) and returns what happened as [`SocEvent`]s (the plugin
//! records each as an `Effect::Society` ledger row plus a trace event of its kind) and as proposals for anything that
//! touches a demigod (merged like every other proposal). Deterministic: every roll is `Rng::derive(seed, [what, beat,
//! id])`. Work is linear in crowds, cells, mobs, armies and people, bounded per beat (emergence budget, event caps).

pub mod armies;
pub mod crowds;
pub mod hier;
pub mod mobs;
pub mod people;

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde_json::Value;

/// People the simulation may bring into the story per beat (structural appointments, such as a new army's general,
/// may go past it).
pub const EMERGE_PER_BEAT: u32 = 5;
/// Most society events recorded per beat (the rest are counted only; big ones first).
pub const EVENTS_PER_BEAT: usize = 24;
/// Below this magnitude an event is recorded only if it happens in focus (level of detail).
pub const DETAIL_MAGNITUDE: u8 = 4;

/// One thing the society did this beat.
#[derive(Debug, Clone, PartialEq)]
pub struct SocEvent {
    pub kind: SocKind,
    /// One line, no engine ids (the ledger row's `act`).
    pub text: String,
    /// Who did it (alive or a place).
    pub actor: Ref,
    /// Who and what it is about.
    pub refs: Vec<Ref>,
    pub at: LonLat,
    pub place: Option<PlaceId>,
    pub magnitude: u8,
    pub valence: i8,
    /// Extra typed detail for the trace event (sizes, casualties, orders...).
    pub data: Value,
}

/// What the step produced.
#[derive(Debug, Default)]
pub struct StepOut {
    pub events: Vec<SocEvent>,
    /// Proposals touching demigods (armies and mobs storming them), for the plugin's merge.
    pub proposals: Vec<Proposal>,
    /// People who entered the story this beat (the plugin writes their `memorized` events and memories).
    pub new_people: Vec<CharId>,
    pub fidelity: Fidelity,
    /// Events dropped by the per-beat cap or the detail rule (still counted in the totals).
    pub dropped: u32,
}

/// The step's per-beat context: the beat, the focus (level of detail) and the emergence budget.
pub struct Ctx {
    pub beat: u32,
    pub seed: u64,
    /// Places, crowds, armies, mobs and people in focus (seat zoom/focus, addressed this beat).
    pub focus: Vec<Ref>,
    /// Cities in focus (derived from `focus`).
    pub focus_cities: Vec<PlaceId>,
    pub emerge_left: u32,
    pub out: StepOut,
}

impl Ctx {
    pub fn rng(&self, what: &str, id: u64) -> Rng {
        Rng::derive(self.seed, &[hash_str(what), self.beat as u64, id])
    }
    pub fn roll(&self, what: &str, id: u64) -> f32 {
        self.rng(what, id).f64() as f32
    }
    /// Is this city (or anything in it) in focus?
    pub fn in_focus(&self, city: Option<PlaceId>) -> bool {
        city.is_some_and(|c| self.focus_cities.contains(&c))
    }
    /// May one more person emerge this beat?
    pub fn can_emerge(&self) -> bool {
        self.emerge_left > 0
    }
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &mut self,
        w: &World,
        kind: SocKind,
        text: String,
        actor: Ref,
        refs: Vec<Ref>,
        at: LonLat,
        place: Option<PlaceId>,
        magnitude: u8,
        valence: i8,
        data: Value,
    ) {
        let city = place.and_then(|p| crate::geo::city_of(w, p));
        let focused = self.in_focus(city) || refs.iter().any(|r| self.focus.contains(r));
        if magnitude < DETAIL_MAGNITUDE && !focused {
            self.out.dropped += 1;
            return;
        }
        self.out.events.push(SocEvent {
            kind,
            text,
            actor,
            refs,
            at,
            place,
            magnitude,
            valence,
            data,
        });
    }
}

/// What to call where something happens: its city, else the nearest city within 1,500 km (planet; any in a city arena),
/// else the place itself (a region of the open map).
pub fn near_name(w: &World, pos: LonLat) -> String {
    let p = crate::geo::place_at(w, pos);
    if let Some(c) = crate::geo::city_of(w, p) {
        return w.name_of(Ref::Place(c));
    }
    let reach = if w.is_planet() { 1500.0 } else { f64::INFINITY };
    w.places
        .iter()
        .filter(|x| crate::geo::city_of(w, x.id) == Some(x.id))
        .map(|x| (crate::geo::distance_km(x.center, pos), x.id))
        .filter(|x| x.0 <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|x| w.name_of(Ref::Place(x.1)))
        .unwrap_or_else(|| w.name_of(Ref::Place(p)))
}

/// Round to 3 decimals (keeps the state's JSON short and stable).
pub fn r3(x: f32) -> f32 {
    (x * 1000.0).round() / 1000.0
}

pub fn clamp01(x: f32) -> f32 {
    r3(x.clamp(0.0, 1.0))
}

/// The focus cities of a focus list (places resolved to their city; crowds, people, armies, mobs to where they are).
pub fn focus_cities(w: &World, focus: &[Ref]) -> Vec<PlaceId> {
    let mut v: Vec<PlaceId> = focus
        .iter()
        .filter_map(|r| {
            let p = match r {
                Ref::Cluster(c) => match w.clusters.get(c.ix())?.army.as_ref() {
                    Some(a) => crate::geo::place_at(w, a.pos),
                    None => crate::util::place_of(w, *r)?,
                },
                _ => crate::util::place_of(w, *r)?,
            };
            crate::geo::city_of(w, p).or(Some(p))
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// One beat of the whole society. `focus` = refs in focus (level of detail).
pub fn step(w: &mut World, beat: u32, focus: &[Ref]) -> StepOut {
    if w.nodes.len() != w.places.len() {
        hier::init_nodes(w);
    }
    let mut cx = Ctx {
        beat,
        seed: w.meta.seed,
        focus: focus.to_vec(),
        focus_cities: focus_cities(w, focus),
        emerge_left: EMERGE_PER_BEAT,
        out: StepOut::default(),
    };
    let chars_before = w.characters.len();
    crowds::needs_and_moods(w, &mut cx);
    crowds::migrate(w, &mut cx);
    crowds::cults(w, &mut cx);
    mobs::step(w, &mut cx);
    crowds::clashes(w, &mut cx);
    armies::step(w, &mut cx);
    people::step(w, &mut cx);
    hier::rollup(w);
    let mut out = cx.out;
    out.new_people = (chars_before..w.characters.len()).map(CharId::from_ix).collect();
    // Big events first when over the cap (stable: ties keep their order).
    if out.events.len() > EVENTS_PER_BEAT {
        let mut ix: Vec<usize> = (0..out.events.len()).collect();
        ix.sort_by_key(|i| std::cmp::Reverse(out.events[*i].magnitude));
        let keep: std::collections::BTreeSet<usize> = ix.into_iter().take(EVENTS_PER_BEAT).collect();
        let n = out.events.len();
        out.events = std::mem::take(&mut out.events)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| keep.contains(i))
            .map(|x| x.1)
            .collect();
        out.dropped += (n - EVENTS_PER_BEAT) as u32;
    }
    let live_cells = w.cells.iter().filter(|c| c.ended.is_none()).count();
    let live_mobs = w.mobs.iter().filter(|m| m.ended.is_none()).count();
    let armies = w.clusters.iter().filter(|c| c.army.is_some() && c.size > 0).count();
    let people = w
        .characters
        .iter()
        .filter(|c| c.alive && c.life.is_some())
        .count();
    out.fidelity = Fidelity {
        rules: (w.nodes.len() + w.clusters.len() + live_cells + live_mobs + armies + people) as u32,
        focus: focus.to_vec(),
        llm: vec![],
        events: out.events.len() as u32,
        emerged: out.new_people.len() as u32,
    };
    out
}
