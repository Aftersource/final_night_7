//! The Dungeon Master (rules 0.8.0, state schema 6): the rulebook's rules (docs/DM_RULEBOOK.md), its hierarchical
//! place × time memory (`World::dm`, [`fn7_model::dm`]) and the eight features it governs.
//!
//! - **Memory.** [`remember`] records every decision about a place at a beat (cell → settlement → region → world);
//!   [`roll_d20`] reuses a recorded roll for the same query; [`recall`] answers "what was decided about place P (or
//!   any ancestor or descendant) at or before beat B"; [`prompt_lines`] feeds the LLM DM a bounded slice of it.
//! - **The d20** ([`roll_d20`], [`record_roll`]): seeded by the match seed and the query, recorded once, shown to
//!   everyone as `dice.rolled`.
//! - **The beat's DM step** ([`step`], after the society and the occupancy rule): encounters stepped on are revealed and
//!   resolved, shrines are built, grow and grant, loot drops and is taken, new encounters are marked, every demigod's
//!   story piece is updated, settlements grow or shrink, and traffic wears paths into roads.
//! - **Day and night** ([`time_of_day`]): derived from the beat (`beats_per_day`), recorded when the phase changes.
//!
//! Deterministic: every draw is `Rng::derive(seed, [salt, beat or query hash, ...])`; iteration is in id order.
//! Nothing here runs before schema 6 ([`World::dm_rules`]).

use std::collections::BTreeMap;

use fn7_model::dm::*;
use fn7_model::hexgrid::{cell_at, HexCell, HexLevel};
use fn7_model::manifest::{icon_of, ItemEvent, ItemKind, ItemState, Tier};
use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use fn7_society::geo::{destination, distance_km, region_at};
use serde_json::{json, Value};

use crate::settings::Settings;
use crate::world::event;

/// Salts for the DM's seeded draws.
pub const DICE_SALT: u64 = 0xD20;
const ENC_SALT: u64 = 0xE7C0;
const SHRINE_SALT: u64 = 0x5B1E;

/// Most `dice.rolled` and other DM events in one beat's outcome (the memory keeps every one).
pub const DICE_PER_BEAT: usize = 60;
pub const EVENTS_PER_BEAT: usize = 40;

/// A health drop in one beat that leaves a scar.
pub const SCAR_MIN: f32 = 1.5;
/// Deeds for each wardrobe stage (plain, marked, adorned, regalia, mythic).
pub const WARDROBE_DEEDS: [f32; 5] = [0.0, 3.0, 8.0, 16.0, 30.0];
/// Follower milestones the memory records.
const FOLLOWER_MILESTONES: [u64; 5] = [10, 100, 1_000, 10_000, 100_000];

// ---------------------------------------------------------------------------------------------------------------------
// Places and keys
// ---------------------------------------------------------------------------------------------------------------------

/// Where something is, at every scope of the memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Where {
    pub cell: String,
    pub at: LonLat,
    pub settlement: Option<PlaceId>,
    pub region: PlaceId,
}

impl Where {
    /// The key at `scope` and its ancestors (nearest first).
    pub fn key(&self, scope: Scope) -> (String, Vec<String>) {
        let s = self.settlement.map(|p| Ref::Place(p).to_string());
        let r = Ref::Place(self.region).to_string();
        let w = WORLD_KEY.to_string();
        match scope {
            Scope::Cell => (self.cell.clone(), s.into_iter().chain([r, w]).collect()),
            Scope::Settlement => match s {
                Some(s) => (s, vec![r, w]),
                None => (r, vec![w]),
            },
            Scope::Region => (r, vec![w]),
            Scope::World => (w, vec![]),
        }
    }
}

/// The step radius (km): a demigod "steps on" a hex when its move this beat passes within it. The setting is the
/// planet's; a city arena uses a hundredth.
pub fn reach_km(w: &World, st: &Settings) -> f64 {
    let r = st.dm_reach_km.max(0.01) as f64;
    if w.is_planet() {
        r
    } else {
        r / 100.0
    }
}

/// The settlement a point lies in: the nearest settlement (a `Site`) within 1.5 × its radius (at least 15 km on the
/// planet, 0.5 km in a city).
pub fn settlement_at(w: &World, pos: LonLat) -> Option<PlaceId> {
    let min = if w.is_planet() { 15.0 } else { 0.5 };
    w.places
        .iter()
        .filter(|p| p.kind == PlaceKind::Site)
        .map(|p| (distance_km(p.center, pos), p))
        .filter(|(d, p)| *d <= (p.radius_km * 1.5).max(min))
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)))
        .map(|(_, p)| p.id)
}

/// The nearest settlement within `max_km` (for traffic between settlements).
fn nearest_settlement(w: &World, pos: LonLat, max_km: f64) -> Option<PlaceId> {
    w.places
        .iter()
        .filter(|p| p.kind == PlaceKind::Site)
        .map(|p| (distance_km(p.center, pos), p.id))
        .filter(|(d, _)| *d <= max_km)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|x| x.1)
}

/// [`Where`] a point is, with its hex at `level`.
pub fn locate(w: &World, pos: LonLat, level: HexLevel) -> Where {
    let settlement = settlement_at(w, pos);
    let region = match settlement {
        Some(s) => fn7_society::geo::district_of(w, s),
        None => region_at(w, pos),
    };
    Where { cell: cell_at(pos, level).key(), at: pos, settlement, region }
}

/// A place key's ancestors (for [`recall`]): a cell key, a `pl:` ref, or `"world"`. `Err` for anything else.
pub fn ancestors_of_key(w: &World, key: &str) -> Result<Vec<String>, String> {
    if key == WORLD_KEY {
        return Ok(vec![]);
    }
    if let Some(c) = HexCell::parse(key) {
        let wh = locate(w, c.center(), c.level);
        return Ok(wh.key(Scope::Cell).1);
    }
    match key.parse::<Ref>() {
        Ok(Ref::Place(p)) if p.ix() < w.places.len() => {
            let pl = &w.places[p.ix()];
            Ok(match pl.kind {
                PlaceKind::Arena => vec![],
                PlaceKind::District => vec![WORLD_KEY.into()],
                _ => {
                    let r = fn7_society::geo::district_of(w, p);
                    let mut v = vec![];
                    if let Some(c) = fn7_society::geo::city_of(w, p).filter(|c| *c != p && *c != r) {
                        v.push(Ref::Place(c).to_string());
                    }
                    v.push(Ref::Place(r).to_string());
                    v.push(WORLD_KEY.into());
                    v
                }
            })
        }
        _ => Err(format!("`{key}` is not a place: a hex cell key (\"0:people:12:-3\"), a `pl:` ref or \"world\"")),
    }
}

/// The arena place stands for the whole world in the memory.
fn place_key(w: &World, p: PlaceId) -> String {
    if p == w.meta.arena {
        WORLD_KEY.into()
    } else {
        Ref::Place(p).to_string()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The memory
// ---------------------------------------------------------------------------------------------------------------------

/// One decision to record.
pub struct Decision {
    pub scope: Scope,
    pub kind: &'static str,
    pub query: Option<String>,
    pub public: bool,
    pub text: String,
    pub data: Value,
}

/// Record a decision about `wh` at `beat` (at `d.scope`). Returns the entry id.
pub fn remember(w: &mut World, beat: u32, wh: &Where, d: Decision) -> u32 {
    let (place, up) = wh.key(d.scope);
    remember_at(w, beat, d.scope, place, up, d)
}

fn remember_at(w: &mut World, beat: u32, scope: Scope, place: String, up: Vec<String>, d: Decision) -> u32 {
    // COMPARE against priors: a question already answered keeps its recorded answer (a different one is logged).
    if let Some(q) = &d.query {
        if let Some(e) = w.dm.decided(q) {
            if e.data != d.data {
                conflict(beat, q, &e.data, &d.data, "the record stands");
            }
            return e.id;
        }
    }
    let id = w.dm.memory.len() as u32;
    event("dm_memory", None, json!({ "beat": beat, "id": id, "scope": scope, "place": place, "kind": d.kind,
                                       "query": d.query, "public": d.public }));
    w.dm.memory.push(DmEntry { id, beat, scope, place, up, kind: d.kind.into(), query: d.query, public: d.public,
                               text: d.text, data: d.data });
    id
}

/// A world-scope decision.
fn remember_world(w: &mut World, beat: u32, d: Decision) -> u32 {
    remember_at(w, beat, Scope::World, WORLD_KEY.into(), vec![], d)
}

/// Public entries about `key` (or above or below it) at or before `until`, newest first, at most `limit`.
pub fn recall<'a>(w: &'a World, key: &str, until: u32, limit: usize) -> Result<Vec<&'a DmEntry>, String> {
    let anc = ancestors_of_key(w, key)?;
    let mut v: Vec<&DmEntry> = w.dm.memory.iter().filter(|e| e.beat <= until && e.public && e.concerns(key, &anc)).collect();
    v.reverse();
    v.truncate(limit);
    Ok(v)
}

/// A memory entry for payloads.
pub fn entry_json(e: &DmEntry) -> Value {
    json!({ "id": e.id, "beat": e.beat, "scope": e.scope, "place": e.place, "up": e.up, "kind": e.kind,
            "query": e.query, "text": e.text, "data": e.data })
}

/// The settled lines the LLM DM reads about region/district `p` (bounded: the newest `n`, oldest first; rolls left
/// out: they are per act).
pub fn prompt_lines(w: &World, p: PlaceId, beat: u32, n: usize) -> Vec<String> {
    let key = place_key(w, p);
    let anc = ancestors_of_key(w, &key).unwrap_or_default();
    let mut v: Vec<String> = w
        .dm
        .recall(&key, &anc, beat)
        .filter(|e| e.public && !matches!(e.kind.as_str(), "dice.rolled" | "place.observed" | "memory.rollup") && !e.text.is_empty())
        .map(|e| format!("beat {}: {}", e.beat, e.text))
        .collect();
    let skip = v.len().saturating_sub(n);
    v.drain(..skip);
    v
}

// ---------------------------------------------------------------------------------------------------------------------
// The d20
// ---------------------------------------------------------------------------------------------------------------------

/// The verdict of `roll` + `modifier` against `dc`: a natural 1 always fails badly, a natural 20 always succeeds
/// gloriously.
pub fn verdict(roll: u8, total: i32, dc: i32) -> DiceVerdict {
    match roll {
        1 => DiceVerdict::CritFail,
        20 => DiceVerdict::CritSuccess,
        _ if total >= dc => DiceVerdict::Success,
        _ => DiceVerdict::Fail,
    }
}

/// The d20 for `query` (no state change): the recorded roll if the DM was asked this before, else a fresh seeded one.
pub fn roll_d20(w: &World, query: &str, modifier: i32, dc: i32) -> DiceRoll {
    if let Some(r) = w.dm.decided(query).and_then(|e| serde_json::from_value::<DiceRoll>(e.data.clone()).ok()) {
        return r;
    }
    let mut r = Rng::derive(w.meta.seed, &[DICE_SALT, hash_str(query)]);
    let roll = 1 + r.below(20) as u8;
    let confirm = r.f64();
    let total = roll as i32 + modifier;
    DiceRoll { roll, modifier, total, target_dc: dc, verdict: verdict(roll, total, dc), confirm }
}

/// Record a roll (once per query) and show it: the `dice.rolled` entry, public.
pub fn record_roll(w: &mut World, beat: u32, query: &str, who: Ref, d: &DiceRoll, reason: &str, pos: LonLat) {
    if w.dm.decided(query).is_some() {
        return;
    }
    let wh = locate(w, pos, HexLevel::People);
    let data = json!({ "who": who, "die": "d20", "roll": d.roll, "modifier": d.modifier, "total": d.total,
                       "target_dc": d.target_dc, "verdict": d.verdict, "reason": reason, "at": pos, "cell": wh.cell,
                       "confirm": d.confirm, "beat": beat });
    let text = format!("{} rolled {} for {} ({})", w.name_of(who), d.roll, reason, d.verdict.as_str());
    remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "dice.rolled", query: Some(query.into()), public: true,
                                      text, data });
}

/// A `memory_conflict` trace event: a new result contradicted a recorded fact and was reconciled to the record.
pub fn conflict(beat: u32, query: &str, recorded: &Value, proposed: &Value, resolution: &str) {
    event("memory_conflict", None, json!({ "beat": beat, "query": query, "recorded": recorded, "proposed": proposed,
                                           "resolution": resolution }));
}

/// Roll and record in one go.
#[allow(clippy::too_many_arguments)]
pub fn d20(w: &mut World, beat: u32, query: &str, who: Ref, modifier: i32, dc: i32, reason: &str, pos: LonLat) -> DiceRoll {
    let r = roll_d20(w, query, modifier, dc);
    if w.dm.decided(query).is_some() && (r.modifier != modifier || r.target_dc != dc) {
        conflict(beat, query, &json!({ "modifier": r.modifier, "target_dc": r.target_dc, "roll": r.roll }),
                 &json!({ "modifier": modifier, "target_dc": dc }), "the recorded roll stands");
    }
    record_roll(w, beat, query, who, &r, reason, pos);
    r
}

/// The DM's modifier for a demigod: +1 hale (health ≥ 7), −1 badly hurt (≤ 3), + half its wardrobe stage.
pub fn demigod_modifier(w: &World, d: DemigodId) -> i32 {
    let Some(x) = w.demigods.get(d.ix()) else { return 0 };
    let h = if x.health >= 7.0 {
        1
    } else if x.health <= 3.0 {
        -1
    } else {
        0
    };
    h + w.dm.story(d).map(|s| s.wardrobe as i32 / 2).unwrap_or(0)
}

/// What a resolver act's die is called in public (never the order's words): the act's class.
pub fn act_reason(verb: &str) -> &'static str {
    use fn7_agents::common::{classify, ActClass};
    match classify(verb) {
        ActClass::Move => "a journey",
        ActClass::Attack => "a strike",
        ActClass::Social => "a parley",
        ActClass::Scare => "a threat",
        ActClass::Make => "a making",
        ActClass::Other => "a deed",
    }
}

/// Every act's d20 for a resolver vicinity (rules 0.8.0): DC 8 + magnitude/2, the actor's modifier (+2 on the Final
/// Wish). Recorded and shown. `None` for acts whose actor is not a demigod's.
pub fn resolver_dice(w: &mut World, beat: u32, acts: &[fn7_agents::resolver::ActLine]) -> Vec<Option<DiceRoll>> {
    let wish = crate::beat::is_final_wish(w, beat);
    // Rules 0.9.0: gear, boons and hexes join the modifier (strikes use the weapon, journeys the fleet gear, other
    // deeds the charm).
    let wills = crate::wills::on(w).then(|| crate::world::settings(w));
    acts.iter()
        .map(|a| {
            let d = a.intent.demigod;
            let pos = fn7_society::geo::pos_of(w, a.actor).or_else(|| w.demigods.get(d.ix()).map(|x| x.pos))?;
            let dc = 8 + a.intent.magnitude.min(10) as i32 / 2;
            let gear = wills.as_ref().map(|st| crate::wills::arms::act_mod(w, d, &a.intent.verb, beat, st)).unwrap_or(0);
            let m = demigod_modifier(w, d) + if wish { 2 } else { 0 } + gear;
            let q = format!("act:{}", a.intent.id.0);
            Some(d20(w, beat, &q, Ref::Demigod(d), m, dc, act_reason(&a.intent.verb), pos))
        })
        .collect()
}

/// The resolver's DM view of vicinity `place` (rules 0.8.0): its memory lines, each act's d20 and the time of day.
pub fn resolver_view(
    w: &mut World,
    beat: u32,
    place: PlaceId,
    acts: &[fn7_agents::resolver::ActLine],
    st: &Settings,
) -> fn7_agents::resolver::DmView {
    let dice = resolver_dice(w, beat, acts);
    let (day, phase, _, _) = phase_of(beat, st.beats_per_day);
    fn7_agents::resolver::DmView {
        memory: prompt_lines(w, place, beat, st.dm_memory_lines as usize),
        dice,
        time: format!("{}, day {day}", capitalize(phase)),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The manifest roll as a d20 (rules 0.8.0): the success odds become a DC (`target_dc` from the tier's base odds, the
/// rest a modifier), so `roll + modifier ≥ target_dc` ⇔ the roll's face reaches `21 − round(20 × odds)` (a natural 1
/// always fails, a natural 20 always succeeds). The unit draw the outcome table reads is placed inside the success or
/// failure share by the confirm draw, so twists, partials and backfires keep their shares. Returns the roll and that
/// draw.
pub fn manifest_d20(w: &World, mf: u32, success: f64, base: f64) -> (DiceRoll, f64) {
    let s20 = ((success * 20.0).round() as i32).clamp(1, 19);
    let target = (21 - (base * 20.0).round() as i32).clamp(2, 20);
    let modifier = target - (21 - s20);
    let r = roll_d20(w, &format!("mf:{mf}"), modifier, target);
    let s = success.clamp(1e-6, 1.0 - 1e-6);
    let u = match r.verdict {
        DiceVerdict::CritSuccess => 0.0,
        v if v.ok() => s * r.confirm,
        // A natural 1 never comes out partial: it fails or backfires.
        DiceVerdict::CritFail => s + (1.0 - s) * (0.5 + 0.5 * r.confirm).min(0.999_999),
        _ => s + (1.0 - s) * r.confirm,
    };
    (r, u)
}

// ---------------------------------------------------------------------------------------------------------------------
// Day and night
// ---------------------------------------------------------------------------------------------------------------------

/// `(day, phase, light, beat_in_day)` of a beat: `beats_per_day` beats a day, the first eighth dawn, then day until
/// half, dusk until five eighths, then night. Light follows the sun: 0.5 at dawn and dusk, 1 at noon, 0 at midnight.
pub fn phase_of(beat: u32, beats_per_day: u32) -> (u32, &'static str, f64, u32) {
    let n = beats_per_day.max(2);
    let b = beat.max(1) - 1;
    let pos = b % n;
    let frac = pos as f64 / n as f64;
    let phase = if frac < 0.125 {
        "dawn"
    } else if frac < 0.5 {
        "day"
    } else if frac < 0.625 {
        "dusk"
    } else {
        "night"
    };
    let mid = (pos as f64 + 0.5) / n as f64 - 0.5 / n as f64;
    let light = 0.5 + 0.5 * (std::f64::consts::TAU * mid).sin();
    (b / n + 1, phase, (light * 100.0).round() / 100.0, pos + 1)
}

pub fn is_night(beat: u32, st: &Settings) -> bool {
    phase_of(beat, st.beats_per_day).1 == "night"
}

/// `{day, phase, light, beat_in_day, beats_per_day}`.
pub fn time_of_day(beat: u32, st: &Settings) -> Value {
    let (day, phase, light, pos) = phase_of(beat, st.beats_per_day);
    json!({ "day": day, "phase": phase, "light": light, "beat_in_day": pos, "beats_per_day": st.beats_per_day.max(2) })
}

/// Add `time_of_day` and `time_of_day_phase` to a payload.
pub fn add_time_of_day(v: &mut Value, beat: u32, st: &Settings) {
    let t = time_of_day(beat, st);
    v["time_of_day_phase"] = t["phase"].clone();
    v["time_of_day"] = t;
}

// ---------------------------------------------------------------------------------------------------------------------
// The beat's DM step
// ---------------------------------------------------------------------------------------------------------------------

/// What the beat looked like at its start (captured by `beat::resolve`).
pub struct Start<'a> {
    pub alive: &'a [DemigodId],
    pub pos: &'a [LonLat],
    pub health: &'a [f32],
    pub armies: &'a [(ClusterId, LonLat)],
    pub row_start: usize,
    pub intent_start: usize,
    pub manifest_start: usize,
    pub memory_start: usize,
}

/// Run the DM's rules for the beat (schema 6 only). Order: time of day, encounters stepped on, shrines, loot dropped
/// and taken, new encounters, story pieces, settlements, roads.
pub fn step(w: &mut World, beat: u32, s: &Start, pantheon: &Pantheon, st: &Settings) {
    if !w.dm_rules() {
        return;
    }
    ensure_stories(w);
    time_step(w, beat, st);
    reveal_encounters(w, beat, s, pantheon, st);
    shrines_step(w, beat, s, pantheon, st);
    loot_drops(w, beat, s);
    loot_pickups(w, beat, s, st);
    fade_encounters(w, beat, st);
    place_encounters(w, beat, st);
    stories_step(w, beat, s);
    settlements_step(w, beat, s, st);
    trails_step(w, beat, s, st);
    // Memory and time: look at the hexes that matter now, then roll the beat's facts up the hierarchy.
    observe_step(w, beat, s, st);
    integrate(w, beat, s.memory_start);
}

fn apply(w: &mut World, props: Vec<Proposal>, pantheon: &Pantheon, beat: u32, st: &Settings) -> crate::merge::Applied {
    crate::merge::run(w, props, pantheon, beat, st.max_witnesses as usize).1
}

fn rules_prop(effect: Effect, act: String, actor: Ref, magnitude: u8, valence: i8, witnesses: Vec<Ref>) -> Proposal {
    Proposal { source: Source::Rules, intent: None, effect, act, actor, magnitude, valence, witnesses, cause: None }
}

/// Does the segment `a → b` pass within `km` of `p`? Returns the closest distance (km) if so. Local flat projection
/// around `p` (fine at these distances).
pub fn passes_within(a: LonLat, b: LonLat, p: LonLat, km: f64) -> Option<f64> {
    let k = p[1].to_radians().cos().max(0.01);
    let xy = |q: LonLat| {
        let mut dl = q[0] - p[0];
        if dl > 180.0 {
            dl -= 360.0;
        } else if dl < -180.0 {
            dl += 360.0;
        }
        (dl * k * 111.32, (q[1] - p[1]) * 110.57)
    };
    let (ax, ay) = xy(a);
    let (bx, by) = xy(b);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 1e-12 { 1.0 } else { (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    let d = (cx * cx + cy * cy).sqrt();
    (d <= km).then_some(d)
}

/// COMPARE against priors (rules 0.8.0): a resolver's proposals (the LLM's or the rules') checked against the recorded
/// d20 of their act. A natural 1 lands nothing on anyone else (damage, healing, gifts, recruits become narration); a
/// failure deals at most 3 damage. Each change is a `memory_conflict` with both values; the record wins.
pub fn reconcile_resolver(w: &World, beat: u32, props: Vec<Proposal>) -> Vec<Proposal> {
    props
        .into_iter()
        .map(|mut p| {
            let Some(i) = p.intent else { return p };
            if p.source != Source::Resolver {
                return p;
            }
            let q = format!("act:{}", i.0);
            let Some(r) = w.dm.decided(&q).and_then(|e| serde_json::from_value::<DiceRoll>(e.data.clone()).ok()) else {
                return p;
            };
            let on_other = |who: &Ref| *who != p.actor;
            let proposed = json!(p.effect);
            let fix = match (&mut p.effect, r.verdict) {
                (Effect::Damage { who, .. } | Effect::Heal { who, .. }, DiceVerdict::CritFail) if on_other(who) => Some(None),
                (Effect::CreateItem { holder, .. }, DiceVerdict::CritFail) if on_other(holder) => Some(None),
                (Effect::Join { member, .. }, DiceVerdict::CritFail) if on_other(member) => Some(None),
                (Effect::Damage { who, amount, .. }, DiceVerdict::Fail) if on_other(who) && *amount > 3.0 => {
                    *amount = 3.0;
                    Some(Some(()))
                }
                _ => None,
            };
            match fix {
                Some(None) => {
                    conflict(beat, &q, &json!({ "verdict": r.verdict, "roll": r.roll }), &proposed,
                             "a natural 1 lands nothing: narration only");
                    p.effect = Effect::Note { text: format!("{} (it comes to nothing)", p.act) };
                    p
                }
                Some(Some(())) => {
                    conflict(beat, &q, &json!({ "verdict": r.verdict, "roll": r.roll }), &proposed, "a failure: at most 3 damage");
                    p
                }
                None => p,
            }
        })
        .collect()
}

// ---- memory and time: integrate, observe, extrapolate ---------------------------------------------------------------

const OBS_SALT: u64 = 0x0B5E;
const WEATHER: [&str; 8] = ["clear", "overcast", "light rain", "fog", "wind", "storm", "drizzle", "haze"];

/// INTEGRATE (rules 0.8.0): the beat's new facts, already recorded at their place and beat, roll up the hierarchy:
/// one `memory.rollup` entry per ancestor touched this beat (settlement, region, world), with the counts by kind, the
/// notable events and the ancestor's state now, so a coarse zoom reads the integrated truth of everything beneath it.
fn integrate(w: &mut World, beat: u32, memory_start: usize) {
    let mut by: BTreeMap<String, (BTreeMap<String, u32>, Vec<String>)> = BTreeMap::new();
    for e in &w.dm.memory[memory_start.min(w.dm.memory.len())..] {
        if !e.public || matches!(e.kind.as_str(), "memory.rollup" | "place.observed") {
            continue;
        }
        let notable = !matches!(e.kind.as_str(), "dice.rolled" | "time.phase") && !e.text.is_empty();
        for k in e.up.iter().chain(std::iter::once(&e.place)).filter(|k| **k != e.place || e.scope != Scope::Cell) {
            let x = by.entry(k.clone()).or_default();
            *x.0.entry(e.kind.clone()).or_default() += 1;
            if notable && x.1.len() < 3 && !x.1.contains(&e.text) {
                x.1.push(e.text.clone());
            }
        }
    }
    for (key, (counts, notable)) in by {
        let anc = ancestors_of_key(w, &key).unwrap_or_default();
        let (scope, state, name) = match key.parse::<Ref>() {
            Ok(Ref::Place(p)) => {
                let name = w.name_of(Ref::Place(p));
                match w.places.get(p.ix()).map(|x| x.kind) {
                    Some(PlaceKind::Site) => (Scope::Settlement, settlement_json(w, p).unwrap_or(Value::Null), name),
                    _ => (Scope::Region, w.nodes.get(p.ix()).map(|n| json!({ "population": n.population, "stance": n.stance,
                                                                             "supply": n.supply })).unwrap_or(Value::Null), name),
                }
            }
            _ => (Scope::World, json!({ "entropy": w.meta.entropy, "alive": w.alive_demigods().count(),
                                         "players_alive": w.alive_seats().len() }), "the world".to_string()),
        };
        let n: u32 = counts.values().sum();
        let text = if notable.is_empty() {
            format!("{n} things happened in {name}")
        } else {
            format!("in {name}: {}", notable.join("; "))
        };
        remember_at(w, beat, scope, key.clone(), anc, Decision { scope, kind: "memory.rollup", query: Some(format!("rollup:{key}:{beat}")),
                                                                 public: true, text,
                                                                 data: json!({ "counts": counts, "notable": notable, "state": state, "facts": n }) });
    }
}

/// A hex's genesis: what the DM would have found there at the start (seeded by the match and the cell).
fn genesis(w: &World, key: &str, c: &HexCell, wh: &Where) -> Value {
    let mut r = Rng::derive(w.meta.seed, &[OBS_SALT, hash_str(key), 0]);
    let parent = wh.settlement.unwrap_or(wh.region);
    let (pop, radius) = (
        w.nodes.get(parent.ix()).map(|n| n.population).unwrap_or(0),
        w.places.get(parent.ix()).map(|p| p.radius_km).unwrap_or(100.0).max(0.5),
    );
    let s = c.level.size_m() / 1000.0;
    let hex_km2 = 1.5 * 3f64.sqrt() * s * s;
    let share = (hex_km2 / (std::f64::consts::PI * radius * radius)).min(1.0);
    let people = (pop as f64 * share * (0.2 + 1.6 * r.f64())).round() as u64;
    let level = wh.settlement.and_then(|p| w.dm.settlement(p)).map(|s| s.level).unwrap_or(0);
    let buildings = match c.level {
        HexLevel::People => (r.below(3) as u32).min(level as u32),
        HexLevel::Houses => level as u32 * 2 + r.below(4) as u32,
        HexLevel::Villages => level as u32 * 40 + r.below(60) as u32,
        HexLevel::Regions => level as u32 * 900 + r.below(900) as u32,
    };
    json!({ "people": people, "buildings": buildings, "parent_population": pop, "settlement_level": level,
            "terrain": w.places.get(parent.ix()).map(|p| p.terrain.clone()).unwrap_or_default() })
}

/// EXTRAPOLATE (rules 0.8.0): a hex's state now. The server never simulates every 3 m hex every beat: a hex observed
/// at beat B (a `place.observed` entry) is carried forward from that record by its parent's integrated changes since
/// B (people scale with the parent's population; buildings follow the settlement's level), seeded by the cell and the
/// two beats, plus what the DM records on it (encounters, loot, shrines, roads) and the weather and light now. Returns
/// `(state, observed_beat, extrapolated)`: an observation this beat comes back as is. Pure: the same hex at the same
/// beat gives the same answer, and [`observe`] records it as the new fact.
pub fn cell_state(w: &World, key: &str, beat: u32) -> Result<(Value, Option<u32>, bool), String> {
    let c = HexCell::parse(key).ok_or_else(|| format!("`{key}` is not a hex cell key"))?;
    let center = c.center();
    let wh = locate(w, center, c.level);
    let last = w.dm.memory.iter().rev().find(|e| e.kind == "place.observed" && e.place == key && e.beat <= beat);
    if let Some(e) = last.filter(|e| e.beat == beat) {
        return Ok((e.data.clone(), Some(beat), false));
    }
    let (base, since) = match last {
        Some(e) => (e.data.clone(), e.beat),
        None => (genesis(w, key, &c, &wh), 0),
    };
    let parent = wh.settlement.unwrap_or(wh.region);
    let pop_now = w.nodes.get(parent.ix()).map(|n| n.population).unwrap_or(0);
    let pop_then = base["parent_population"].as_u64().unwrap_or(pop_now).max(1);
    let level_now = wh.settlement.and_then(|p| w.dm.settlement(p)).map(|s| s.level).unwrap_or(0);
    let level_then = base["settlement_level"].as_u64().unwrap_or(level_now as u64) as i64;
    let mut r = Rng::derive(w.meta.seed, &[OBS_SALT, hash_str(key), since as u64, beat as u64]);
    let elapsed = beat.saturating_sub(since) as f64;
    let drift = 1.0 + (r.f64() - 0.5) * 0.04 * elapsed.sqrt();
    let people = (base["people"].as_u64().unwrap_or(0) as f64 * pop_now as f64 / pop_then as f64 * drift).round().max(0.0) as u64;
    let per_level: i64 = match c.level {
        HexLevel::People => 0,
        HexLevel::Houses => 2,
        HexLevel::Villages => 40,
        HexLevel::Regions => 900,
    };
    let buildings = (base["buildings"].as_i64().unwrap_or(0) + (level_now as i64 - level_then) * per_level).max(0);
    let size_km = c.level.size_m() / 1000.0;
    let in_cell = |at: LonLat| cell_at(at, c.level) == c;
    let encounters: Vec<Value> = w.dm.encounters.iter().filter(|e| e.faded_beat.is_none() && in_cell(e.at)).map(|e| encounter_json(w, e)).collect();
    let loot: usize = w.dm.loot.iter().filter(|l| l.taken_beat.is_none() && in_cell(l.at)).map(|l| l.items.len()).sum();
    let shrines: Vec<u32> = w.dm.shrines.iter().filter(|s| in_cell(s.at)).map(|s| s.id).collect();
    let on_road = w.dm.roads().any(|t| {
        passes_within(w.places[t.a.ix()].center, w.places[t.b.ix()].center, center, size_km.max(1.0)).is_some()
    });
    let changes: Vec<String> = w
        .dm
        .memory
        .iter()
        .filter(|e| e.kind == "memory.rollup" && e.beat > since && e.beat <= beat && e.place == Ref::Place(parent).to_string())
        .flat_map(|e| e.data["notable"].as_array().cloned().unwrap_or_default())
        .filter_map(|v| v.as_str().map(str::to_string))
        .rev()
        .take(5)
        .collect();
    let st = crate::world::settings(w);
    let (day, phase, light, _) = phase_of(beat, st.beats_per_day);
    let weather = WEATHER[Rng::derive(w.meta.seed, &[OBS_SALT, wh.region.0 as u64, beat as u64]).below(WEATHER.len() as u64) as usize];
    let round6 = |x: f64| (x * 1e6).round() / 1e6;
    let mut state = json!({
        "cell": key, "level": c.level.as_str(), "center": [round6(center[0]), round6(center[1])],
        "settlement": wh.settlement.map(Ref::Place), "region": Ref::Place(wh.region),
        "people": people, "buildings": buildings, "parent_population": pop_now, "settlement_level": level_now,
        "terrain": base["terrain"].clone(), "encounters": encounters, "loot_items": loot, "shrines": shrines,
        "on_road": on_road, "weather": weather, "time_of_day": { "day": day, "phase": phase, "light": light },
        "changes_since": changes,
    });
    // The Earth simulator (schema 13): the hex as the field has it now (a fine hex is derived from its coarse cell:
    // docs/EARTH_SIM.md), and the weather the field really has there. Once observed it is part of the record.
    if let Some(e) = crate::earth::rules::sim_of(w, &st).filter(|e| e.beat == beat) {
        let cellv = crate::earth::view::hex_json(&e, &c);
        if let Some(wx) = cellv["weather"].as_str() {
            state["weather"] = wx.into();
        }
        state["earth"] = cellv;
    }
    Ok((state, last.map(|e| e.beat), true))
}

/// Record a hex's state now as the new fact (the next look starts from it). No-op when it was observed this beat.
pub fn observe(w: &mut World, beat: u32, key: &str) {
    let Ok((state, _, extrapolated)) = cell_state(w, key, beat) else { return };
    if !extrapolated {
        return;
    }
    let Some(c) = HexCell::parse(key) else { return };
    let wh = locate(w, c.center(), c.level);
    let (_, up) = wh.key(Scope::Cell);
    remember_at(w, beat, Scope::Cell, key.to_string(), up, Decision {
        scope: Scope::Cell, kind: "place.observed", query: Some(format!("observed:{key}:{beat}")), public: true,
        text: String::new(), data: state,
    });
}

/// The hexes the DM looks at this beat: where each standing demigod arrived (or every day where it stands), each
/// seat's zoom focus, and every hex the DM decided something about this beat.
fn observe_step(w: &mut World, beat: u32, s: &Start, st: &Settings) {
    let mut keys: Vec<String> = vec![];
    for d in w.alive_demigods() {
        let moved = s.pos.get(d.id.ix()).is_some_and(|p| people_cell(*p) != people_cell(d.pos));
        if moved || beat % st.beats_per_day.max(2) == 1 {
            keys.push(people_cell(d.pos));
        }
    }
    for seat in &w.seats {
        if let Some(at) = seat.focus.and_then(|f| fn7_society::geo::pos_of(w, f)) {
            if !matches!(seat.focus, Some(Ref::Place(_))) {
                keys.push(cell_at(at, HexLevel::Houses).key());
            }
        }
    }
    for e in &w.dm.memory[s.memory_start.min(w.dm.memory.len())..] {
        if e.scope == Scope::Cell && e.kind != "dice.rolled" && e.kind != "place.observed" {
            keys.push(e.place.clone());
        }
    }
    keys.sort();
    keys.dedup();
    for k in keys {
        observe(w, beat, &k);
    }
}

// ---- time ---------------------------------------------------------------------------------------------------------

fn time_step(w: &mut World, beat: u32, st: &Settings) {
    let (day, phase, light, _) = phase_of(beat, st.beats_per_day);
    let prev = (beat > 1).then(|| phase_of(beat - 1, st.beats_per_day).1);
    if prev == Some(phase) {
        return;
    }
    let text = match phase {
        "dawn" => format!("Dawn breaks on day {day}"),
        "day" => format!("Full day, day {day}"),
        "dusk" => format!("Dusk falls on day {day}"),
        _ => format!("Night falls, day {day}: the strange things come out"),
    };
    remember_world(w, beat, Decision { scope: Scope::World, kind: "time.phase", query: Some(format!("time:{beat}")),
                                       public: true, text, data: json!({ "day": day, "phase": phase, "light": light }) });
}

// ---- stories ------------------------------------------------------------------------------------------------------

pub fn ensure_stories(w: &mut World) {
    for d in &w.demigods {
        if w.dm.story(d.id).is_none() {
            w.dm.stories.push(Story { demigod: d.id, followers: 0, wardrobe: 0, deeds: 0.0, condition: None, scars: vec![],
                                      tales: vec![], last_summon: None });
        }
    }
    w.dm.stories.sort_by_key(|s| s.demigod);
}

pub fn story_mut(w: &mut World, d: DemigodId) -> Option<&mut Story> {
    w.dm.stories.iter_mut().find(|s| s.demigod == d)
}

fn add_deeds(w: &mut World, d: DemigodId, x: f32) {
    if let Some(s) = story_mut(w, d) {
        s.deeds += x;
    }
}

/// Ledger verbs that are bookkeeping, not harm (a bill, a payment, a favor): they never name a scar.
const BOOKKEEPING: &[&str] = &["debt", "favor", "bolts", "price", "delay", "note", "tell", "entropy"];

const SCORCH: &[&str] = &["fire", "burn", "scorch", "flame", "lava", "lightning", "meteor", "volcan", "blaze", "ash"];

fn stories_step(w: &mut World, beat: u32, s: &Start) {
    let rows = s.row_start..w.ledger.rows.len();
    let ids: Vec<DemigodId> = w.demigods.iter().map(|d| d.id).collect();
    for d in ids {
        let me = Ref::Demigod(d);
        let (alive, health, pos, place, following, cause) = {
            let x = &w.demigods[d.ix()];
            (x.alive, x.health, x.pos, x.place, x.following, x.cause.clone())
        };
        let was_alive = s.alive.contains(&d);
        // Scars: a big hit this beat (the heaviest row that hurt it names the cause).
        let drop = if was_alive { s.health.get(d.ix()).copied().unwrap_or(health) - if alive { health } else { 0.0 } } else { 0.0 };
        let hits: Vec<&LedgerRow> = w.ledger.rows[rows.clone()]
            .iter()
            .filter(|r| r.targets.contains(&me) && r.valence < 0 && !BOOKKEEPING.contains(&r.verb.as_str()))
            .collect();
        let scorched = hits.iter().any(|r| {
            let t = r.act.to_lowercase();
            SCORCH.iter().any(|k| t.contains(k))
        });
        let mut scar = None;
        if drop >= SCAR_MIN {
            let worst = hits.iter().max_by_key(|r| (r.magnitude, std::cmp::Reverse(r.id))).map(|r| r.act.clone());
            let c = if alive { worst } else { cause.clone().or(worst) };
            let c = fn7_agents::common::line(&c.unwrap_or_else(|| "a wound nobody saw coming".into()), 80);
            scar = Some(Scar { beat, cause: c, place: w.name_of(Ref::Place(place)), cell: people_cell(pos) });
        }
        // Deeds: landed public acts of weight (at most 3 a beat) and things made.
        let selves = crate::world::body_selves(w, d);
        let acts = w.ledger.rows[rows.clone()]
            .iter()
            .filter(|r| selves.contains(&r.actor) && r.magnitude >= 5 && crate::payload::is_public_event(w, r))
            .count()
            .min(3) as f32;
        let made = w.manifests[s.manifest_start.min(w.manifests.len())..]
            .iter()
            .filter(|m| m.demigod == d && m.item.is_some())
            .count() as f32;
        let followers = fn7_society::headcount(w, following);
        let condition = if !alive {
            None
        } else if scorched {
            Some("scorched".to_string())
        } else if health <= 3.0 {
            Some("ragged".to_string())
        } else {
            None
        };
        let (old_followers, old_stage, deeds) = {
            let st = story_mut(w, d).expect("story");
            st.deeds += acts + made;
            st.condition = condition;
            (st.followers, st.wardrobe, st.deeds)
        };
        let stage = (0..WARDROBE_DEEDS.len()).rev().find(|k| deeds >= WARDROBE_DEEDS[*k]).unwrap_or(0) as u8;
        let wh = locate(w, pos, HexLevel::People);
        if let Some(sc) = scar {
            let text = format!("{} is scarred: {}", w.name_of(me), sc.cause);
            remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "story.scar", query: None, public: true, text,
                                              data: json!({ "demigod": me, "scar": sc }) });
            let st = story_mut(w, d).expect("story");
            st.scars.push(sc);
            if st.scars.len() > SCARS_KEPT {
                st.scars.remove(0);
            }
        }
        if stage > old_stage && alive {
            let text = format!("{} now wears {}", w.name_of(me), WARDROBE[stage as usize]);
            remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "story.wardrobe", query: None, public: true, text,
                                              data: json!({ "demigod": me, "stage": stage, "name": WARDROBE[stage as usize] }) });
            story_mut(w, d).expect("story").wardrobe = stage;
        }
        if let Some(m) = FOLLOWER_MILESTONES.iter().rev().find(|m| followers >= **m && old_followers < **m) {
            let text = format!("{} has {}+ followers", w.name_of(me), m);
            remember(w, beat, &wh, Decision { scope: Scope::Region, kind: "story.followers", query: None, public: true,
                                              text, data: json!({ "demigod": me, "followers": followers, "milestone": m }) });
        }
        story_mut(w, d).expect("story").followers = followers;
    }
}

/// A demigod's story piece: `{followers, items: [{ref, icon}], wardrobe: {stage, name, condition}, scars: [...]}`.
pub fn story_json(w: &World, d: DemigodId) -> Value {
    let me = Ref::Demigod(d);
    let wills = crate::wills::on(w);
    let art = crate::look::on(w).then(|| crate::look::book(w));
    let items: Vec<Value> = w
        .items
        .iter()
        .filter(|i| i.holders.last() == Some(&me) && i.ext.state != ItemState::Destroyed)
        .map(|i| {
            let mut v = json!({ "ref": Ref::Item(i.id), "icon": icon_of(i.ext.kind, &i.name) });
            // Rules 0.9.0: a gear peg says what it is and does (public: gear is worn on the piece).
            if wills {
                if let Some(a) = crate::wills::arms::arm_of(w, i, None) {
                    v["name"] = i.name.clone().into();
                    v["slot"] = a.slot.into();
                    v["tier"] = json!(a.tier);
                    v["mods"] = json!(a.mods);
                    v["power"] = json!(a.power);
                    v["does"] = crate::wills::arms::describe(w, &a, false).into();
                }
            }
            // Gear looks: a peg whose item has a picture says so (`name`, `tier`, `image_prompt`).
            if let Some(b) = &art {
                v["name"] = i.name.clone().into();
                v["tier"] = json!(i.ext.tier);
                crate::look::art_fields(w, b, i, &mut v);
            }
            v
        })
        .collect();
    let s = w.dm.story(d);
    let stage = s.map(|s| s.wardrobe).unwrap_or(0);
    json!({
        "followers": s.map(|s| s.followers).unwrap_or(0),
        "items": items,
        "wardrobe": { "stage": stage, "name": WARDROBE[stage.min(4) as usize], "condition": s.and_then(|s| s.condition.clone()) },
        "deeds": s.map(|s| s.deeds).unwrap_or(0.0),
        "scars": s.map(|s| s.scars.clone()).unwrap_or_default(),
        "tales": s.map(|s| s.tales.clone()).unwrap_or_default(),
    })
}

// ---- settlements --------------------------------------------------------------------------------------------------

/// A settlement's level from its people alone (planet cities; a city arena's sites count their district's crowds).
pub fn base_level(pop: u64) -> u8 {
    match pop {
        0..=199_999 => 1,
        200_000..=999_999 => 2,
        1_000_000..=2_999_999 => 3,
        3_000_000..=7_999_999 => 4,
        _ => 5,
    }
}

const BUILD_VERBS: &[&str] = &["build", "construct", "fortify", "rebuild", "repair", "found", "raise", "erect", "restore"];

fn has_any(text: &str, words: &[&str]) -> bool {
    let t = text.to_lowercase();
    t.split(|c: char| !c.is_alphanumeric()).any(|x| words.iter().any(|w| x == *w || (x.starts_with(w) && w.len() >= 5)))
}

fn settlement_pop(w: &World, p: PlaceId) -> u64 {
    let own = w.nodes.get(p.ix()).map(|n| n.population).unwrap_or(0);
    if w.is_planet() {
        own
    } else {
        // A city arena's sites have no crowds of their own: the district's, shared among its sites.
        let parent = w.places[p.ix()].parent;
        let sites = w.places.iter().filter(|x| x.kind == PlaceKind::Site && x.parent == parent).count().max(1) as u64;
        own.max(parent.and_then(|q| w.nodes.get(q.ix())).map(|n| n.population).unwrap_or(0) / sites)
    }
}

fn settlements_step(w: &mut World, beat: u32, s: &Start, st: &Settings) {
    // Founded once (the memory answers "how big was it" for every later beat).
    if w.dm.settlements.is_empty() {
        let sites: Vec<PlaceId> = w.places.iter().filter(|p| p.kind == PlaceKind::Site).map(|p| p.id).collect();
        for p in sites {
            let size = settlement_pop(w, p);
            let level = base_level(size);
            let center = w.places[p.ix()].center;
            w.dm.settlements.push(Settlement { place: p, level, size, prosperity: 0.5, target: level, since_beat: beat,
                                               cell: people_cell(center) });
            let wh = locate(w, center, HexLevel::People);
            let wh = Where { settlement: Some(p), ..wh };
            let text = format!("{} stands at level {level}", w.places[p.ix()].name);
            remember(w, beat, &wh, Decision { scope: Scope::Settlement, kind: "settlement.level",
                                              query: Some(format!("settlement:{}:founded", p.0)), public: false, text,
                                              data: json!({ "place": Ref::Place(p), "level": level, "size": size }) });
        }
    }
    // Building acts this beat, remembered per settlement.
    let mut built: BTreeMap<PlaceId, u32> = BTreeMap::new();
    for i in s.intent_start.min(w.intents.len())..w.intents.len() {
        let it = &w.intents[i];
        if !has_any(&format!("{} {}", it.verb, it.span), BUILD_VERBS) {
            continue;
        }
        let Some(d) = w.demigods.get(it.demigod.ix()).filter(|d| d.alive) else { continue };
        if let Some(p) = settlement_at(w, d.pos) {
            *built.entry(p).or_default() += 1;
        }
    }
    for (p, n) in &built {
        let center = w.places[p.ix()].center;
        let wh = Where { settlement: Some(*p), ..locate(w, center, HexLevel::People) };
        let text = format!("builders are at work in {}", w.places[p.ix()].name);
        remember(w, beat, &wh, Decision { scope: Scope::Settlement, kind: "settlement.building", query: None, public: true,
                                          text, data: json!({ "place": Ref::Place(*p), "acts": n }) });
    }
    let since = beat.saturating_sub(8);
    // The Earth simulator (schema 13): what a settlement's cell has suffered pushes its target down.
    let earth = crate::earth::rules::sim_of(w, st);
    for k in 0..w.dm.settlements.len() {
        let p = w.dm.settlements[k].place;
        let size = settlement_pop(w, p);
        let (supply, hope, safety, top_alleg) = match w.nodes.get(p.ix()) {
            Some(n) => (n.supply, n.mood.hope, n.need.safety, n.allegiance.iter().map(|x| x.1).fold(0.0f32, f32::max)),
            None => (1.0, 0.2, 0.0, 0.0),
        };
        let prosperity = (0.4 * (supply.clamp(0.0, 1.5) / 1.5) + 0.3 * hope.clamp(0.0, 1.0) + 0.3 * (1.0 - safety.clamp(0.0, 1.0)))
            .clamp(0.0, 1.0);
        let key = Ref::Place(p).to_string();
        let building = w.dm.memory.iter().rev().take_while(|e| e.beat >= since)
            .filter(|e| e.kind == "settlement.building" && e.place == key).count() > 0;
        let shrine = w.dm.shrines.iter().any(|x| x.place == p);
        let mut target = base_level(size) as i32;
        if prosperity >= 0.62 {
            target += 1;
        }
        if prosperity < 0.3 {
            target -= 1;
        }
        if building {
            target += 1;
        }
        if shrine || top_alleg >= 0.5 {
            target += 1;
        }
        let ruined = earth.as_ref().map(|e| crate::earth::rules::ruin_levels(e, st, w.places[p.ix()].center)).unwrap_or(0);
        target -= ruined;
        let target = target.clamp(1, 7) as u8;
        let s0 = &w.dm.settlements[k];
        let (level, since_beat, held) = (s0.level, s0.since_beat, s0.target == target);
        let mut new_level = level;
        // One step at a time, toward a target held two beats running, after the level has stood its time.
        if held && target != level && beat.saturating_sub(since_beat) >= st.dm_settlement_step_beats.max(1) {
            new_level = if target > level { level + 1 } else { level - 1 };
        }
        {
            let x = &mut w.dm.settlements[k];
            x.size = size;
            x.prosperity = (prosperity * 100.0).round() / 100.0;
            x.target = target;
        }
        if new_level != level {
            let grew = new_level > level;
            let name = w.places[p.ix()].name.clone();
            let center = w.places[p.ix()].center;
            let wh = Where { settlement: Some(p), ..locate(w, center, HexLevel::People) };
            let kind = if grew { "settlement.grew" } else { "settlement.shrank" };
            let text = format!("{name} {} to level {new_level}", if grew { "grows" } else { "shrinks" });
            remember(w, beat, &wh, Decision { scope: Scope::Settlement, kind, query: None, public: true, text,
                                              data: json!({ "place": Ref::Place(p), "name": name, "from": level,
                                                            "to": new_level, "size": size, "center_cell": people_cell(center),
                                                            "settlement_cells": settlement_cells(center, new_level) }) });
            let x = &mut w.dm.settlements[k];
            x.level = new_level;
            x.since_beat = beat;
        }
    }
}

/// The village-level hexes a settlement covers: its centre's, plus rings out to level − 1 (at most 4 rings).
pub fn settlement_cells(center: LonLat, level: u8) -> Vec<String> {
    let c = cell_at(center, HexLevel::Villages);
    let mut v = vec![c.key()];
    for k in 1..(level as i64).min(5) {
        v.extend(c.ring(k).into_iter().map(|x| x.key()));
    }
    v
}

/// A settlement for payloads: `{level, size, prosperity, target, since_beat, center_cell, settlement_cells}`.
pub fn settlement_json(w: &World, p: PlaceId) -> Option<Value> {
    let s = w.dm.settlement(p)?;
    let center = w.places.get(p.ix())?.center;
    Some(json!({ "level": s.level, "size": s.size, "prosperity": s.prosperity, "target": s.target,
                 "since_beat": s.since_beat, "center_cell": s.cell, "settlement_cells": settlement_cells(center, s.level) }))
}

// ---- encounters ---------------------------------------------------------------------------------------------------

fn kind_dc(k: EncounterKind) -> i32 {
    match k {
        EncounterKind::Treasure => 10,
        EncounterKind::Stranger => 11,
        EncounterKind::Monster | EncounterKind::Ghost | EncounterKind::Thief => 12,
        EncounterKind::Trap | EncounterKind::Undead => 13,
    }
}

fn live_hidden(w: &World) -> impl Iterator<Item = &Encounter> {
    w.dm.encounters.iter().filter(|e| e.outcome.is_none() && e.faded_beat.is_none())
}

fn place_encounters(w: &mut World, beat: u32, st: &Settings) {
    if st.dm_encounters_per_beat == 0 || w.alive_demigods().next().is_none() {
        return;
    }
    let reach = reach_km(w, st);
    // Anchors: the regions' centres (where travel ends), one hidden encounter per region at most, weighted by activity:
    // the region where Time's circle converges 3, a region with a hotspot 2, any other 1. Never a centre a standing
    // demigod is within reach of.
    let arena = w.places[w.meta.arena.ix()].center;
    let converge = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .min_by(|a, b| distance_km(a.center, arena).total_cmp(&distance_km(b.center, arena)).then(a.id.cmp(&b.id)))
        .map(|p| p.id);
    let hot = crate::hotspots::hotspots(w, crate::hotspots::recent_rows(w));
    let night = is_night(beat, st);
    let mut rng = Rng::derive(w.meta.seed, &[ENC_SALT, beat as u64]);
    for _ in 0..st.dm_encounters_per_beat {
        if live_hidden(w).count() >= st.dm_encounters_max as usize {
            break;
        }
        let anchors: Vec<(LonLat, u32)> = w
            .places
            .iter()
            .filter(|p| p.kind == PlaceKind::District)
            .filter(|p| !live_hidden(w).any(|e| e.region == p.id))
            .filter(|p| !w.alive_demigods().any(|d| distance_km(d.pos, p.center) <= reach))
            .map(|p| (p.center, if Some(p.id) == converge { 3 } else if hot.contains_key(&p.id) { 2 } else { 1 }))
            .collect();
        let total_w: u32 = anchors.iter().map(|a| a.1).sum();
        if total_w == 0 {
            break;
        }
        let mut x = rng.below(total_w as u64) as u32;
        let mut c = anchors[0].0;
        for (p, n) in &anchors {
            if x < *n {
                c = *p;
                break;
            }
            x -= n;
        }
        let mut spot = None;
        for _ in 0..4 {
            // On the way in: inside the step radius of the place's centre, so whoever travels there steps on it.
            let km = reach * (0.2 + 0.6 * rng.f64());
            let p = destination(c, rng.f64() * 360.0, km);
            let near_demigod = w.alive_demigods().any(|d| distance_km(d.pos, p) <= reach);
            let near_other = live_hidden(w).any(|e| distance_km(e.at, p) <= reach);
            if !near_demigod && !near_other {
                spot = Some(p);
                break;
            }
        }
        let Some(at) = spot else { continue };
        let kinds: &[(EncounterKind, u32)] = if night {
            &[(EncounterKind::Monster, 2), (EncounterKind::Treasure, 1), (EncounterKind::Trap, 2), (EncounterKind::Stranger, 1),
              (EncounterKind::Undead, 3), (EncounterKind::Ghost, 2), (EncounterKind::Thief, 2)]
        } else {
            &[(EncounterKind::Monster, 3), (EncounterKind::Treasure, 3), (EncounterKind::Trap, 2), (EncounterKind::Stranger, 2)]
        };
        let total: u32 = kinds.iter().map(|k| k.1).sum();
        let mut x = rng.below(total as u64) as u32;
        let mut kind = kinds[0].0;
        for (k, n) in kinds {
            if x < *n {
                kind = *k;
                break;
            }
            x -= n;
        }
        let level = match kind {
            EncounterKind::Treasure | EncounterKind::Trap | EncounterKind::Thief => HexLevel::People,
            _ => HexLevel::Houses,
        };
        let wh = locate(w, at, level);
        let id = w.dm.encounters.len() as u32;
        w.dm.encounters.push(Encounter { id, kind, cell: wh.cell.clone(), at, region: wh.region, settlement: wh.settlement,
                                         placed_beat: beat, night, faded_beat: None, outcome: None });
        let near = wh.settlement.map(|p| w.name_of(Ref::Place(p))).unwrap_or_else(|| w.name_of(Ref::Place(wh.region)));
        remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "encounter.marked", query: Some(format!("encounter:{id}:placed")),
                                          public: true, text: format!("a marked hex appears near {near}"),
                                          data: json!({ "id": id, "cell": wh.cell, "marked": true }) });
        event("encounter_placed", None, json!({ "beat": beat, "id": id, "kind": kind, "cell": wh.cell, "at": at }));
    }
}

/// The Earth simulator (schema 13): a rupture, a crater or risen land uncovers something at `at`: a marked hex like
/// any other (within the live cap, never where one already lies within reach). Returns the memory entry's id.
pub(crate) fn uncover_encounter(w: &mut World, beat: u32, at: LonLat, why: &str, st: &Settings) -> Option<u32> {
    if st.dm_encounters_max == 0 || live_hidden(w).count() >= st.dm_encounters_max as usize {
        return None;
    }
    let reach = reach_km(w, st);
    if w.dm.encounters.iter().any(|e| e.faded_beat.is_none() && distance_km(e.at, at) <= reach * 4.0) {
        return None;
    }
    let mut rng = Rng::derive(w.meta.seed, &[ENC_SALT, hash_str("uncovered"), beat as u64]);
    let kind = [EncounterKind::Treasure, EncounterKind::Monster, EncounterKind::Treasure, EncounterKind::Trap][rng.below(4) as usize];
    let wh = locate(w, at, HexLevel::Houses);
    let id = w.dm.encounters.len() as u32;
    w.dm.encounters.push(Encounter { id, kind, cell: wh.cell.clone(), at, region: wh.region, settlement: wh.settlement,
                                     placed_beat: beat, night: false, faded_beat: None, outcome: None });
    let near = w.name_of(Ref::Place(wh.region));
    let entry = remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "encounter.marked", query: Some(format!("encounter:{id}:placed")),
                                                  public: true, text: format!("{why} uncovers a marked hex in {near}"),
                                                  data: json!({ "id": id, "cell": wh.cell, "marked": true, "uncovered_by": why }) });
    event("encounter_placed", None, json!({ "beat": beat, "id": id, "kind": kind, "cell": wh.cell, "at": at, "uncovered_by": why }));
    Some(entry)
}

fn fade_encounters(w: &mut World, beat: u32, st: &Settings) {
    let ttl = st.dm_encounter_fade_beats.max(1);
    let gone: Vec<u32> = live_hidden(w).filter(|e| beat >= e.placed_beat + ttl).map(|e| e.id).collect();
    for id in gone {
        let e = w.dm.encounters[id as usize].clone();
        w.dm.encounters[id as usize].faded_beat = Some(beat);
        let wh = Where { cell: e.cell.clone(), at: e.at, settlement: e.settlement, region: e.region };
        remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "encounter.faded", query: Some(format!("encounter:{id}:faded")),
                                          public: true, text: "a marked hex fades, unfound".into(),
                                          data: json!({ "id": id, "cell": e.cell }) });
    }
}

/// Who steps on `at` this beat: the standing demigod whose move came closest (within reach), ties by id.
fn stepper(w: &World, s: &Start, at: LonLat, reach: f64, exclude: Option<DemigodId>) -> Option<DemigodId> {
    w.alive_demigods()
        .filter(|d| Some(d.id) != exclude)
        .filter_map(|d| {
            let from = s.pos.get(d.id.ix()).copied().unwrap_or(d.pos);
            passes_within(from, d.pos, at, reach).map(|x| (x, d.id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|x| x.1)
}

struct Find {
    name: &'static str,
    desc: &'static str,
    kind: ItemKind,
}

const TROPHIES: &[Find] = &[
    Find { name: "Ghoul-fang Dagger", desc: "a dagger cut from the fang of the thing on the marked hex", kind: ItemKind::Weapon },
    Find { name: "Wyrmhide Cloak", desc: "a cloak of scaled hide that turns blades", kind: ItemKind::Armor },
    Find { name: "Horn of the Beast", desc: "a horn that still smells of the beast's lair", kind: ItemKind::Relic },
];
const TREASURES: &[Find] = &[
    Find { name: "Gilded Compass", desc: "a compass that points at what you want most", kind: ItemKind::Artifact },
    Find { name: "Old Silver Crown", desc: "a dented crown from a kingdom nobody remembers", kind: ItemKind::Relic },
    Find { name: "Lucky Coin", desc: "a coin that lands the way you need", kind: ItemKind::Other },
    Find { name: "Sunsteel Blade", desc: "a blade that glows at dusk", kind: ItemKind::Weapon },
];
const CACHES: &[Find] = &[
    Find { name: "Trapper's Kit", desc: "wire, hooks and a map of every snare for miles", kind: ItemKind::Other },
    Find { name: "Spring-loaded Bracer", desc: "a bracer that snaps shut on a blade", kind: ItemKind::Armor },
];
const NIGHT_FINDS: &[Find] = &[
    Find { name: "Grave-iron Ring", desc: "a ring pulled from the dark that is never warm", kind: ItemKind::Relic },
    Find { name: "Moonlit Shroud", desc: "a shroud that hides its wearer from the dead", kind: ItemKind::Armor },
    Find { name: "Thief's Purse", desc: "a purse full of things that belonged to other people", kind: ItemKind::Other },
];
const STRANGERS: &[&str] = &[
    "a scarred mercenary looking for a cause",
    "a wandering healer with a mule",
    "a disgraced knight without a lord",
    "a fortune-teller who already knew your name",
    "a runaway apprentice with a forbidden book",
];

/// Rube Goldberg flourishes for the rules' encounter lines (the Lens and the Engine).
const FLOURISHES: &[&str] = &[
    "because a crow landed on the wrong side of the road at dawn",
    "because someone had washed the keystone in platinum dust",
    "because three bells rang out of order in the next valley",
    "because a merchant spilled exactly forty-one lentils",
    "because the moon was briefly the shape of a key",
    "because a child whistled the old forbidden tune",
];

/// What an encounter does, by kind and verdict.
struct Fate {
    text: String,
    damage: f32,
    find: Option<&'static Find>,
    stranger: Option<&'static str>,
    sparks: i32,
    deeds: f32,
}

fn fate(e: &Encounter, v: DiceVerdict, name: &str, pick: u64) -> Fate {
    use DiceVerdict::*;
    use EncounterKind::*;
    let pickf = |xs: &'static [Find]| Some(&xs[(pick % xs.len() as u64) as usize]);
    let f = |text: String, damage: f32| Fate { text, damage, find: None, stranger: None, sparks: 0, deeds: 0.0 };
    match (e.kind, v) {
        (Monster, CritSuccess) => Fate { find: pickf(TROPHIES), deeds: 3.0, ..f(format!("{name} slays the monster on the marked hex and takes a trophy"), 0.0) },
        (Monster, Success) => Fate { deeds: 2.0, ..f(format!("{name} drives off the monster on the marked hex"), 0.0) },
        (Monster, Fail) => f(format!("the monster on the marked hex mauls {name}"), 1.5),
        (Monster, CritFail) => f(format!("the monster on the marked hex savages {name}"), 3.0),
        (Treasure, CritSuccess) => Fate { find: pickf(TREASURES), deeds: 2.0, ..f(format!("{name} uncovers a hoard on the marked hex"), 0.0) },
        (Treasure, Success) => Fate { find: pickf(&TREASURES[2..]), deeds: 1.0, ..f(format!("{name} finds treasure on the marked hex"), 0.0) },
        (Treasure, Fail) => f(format!("{name} digs up an empty chest"), 0.0),
        (Treasure, CritFail) => f(format!("the chest bites {name}: it was a mimic"), 1.0),
        (Trap, CritSuccess) => Fate { find: pickf(CACHES), deeds: 2.0, ..f(format!("{name} springs the trap harmlessly and finds the trapper's cache"), 0.0) },
        (Trap, Success) => Fate { deeds: 1.0, ..f(format!("{name} spots the trap and steps around it"), 0.0) },
        (Trap, Fail) => f(format!("a snare on the marked hex catches {name}"), 1.0),
        (Trap, CritFail) => f(format!("a pit trap on the marked hex swallows {name}"), 2.5),
        (Stranger, CritSuccess) => Fate { stranger: Some(STRANGERS[(pick % STRANGERS.len() as u64) as usize]), deeds: 2.0,
                                          ..f(format!("a stranger on the marked hex swears to follow {name} to the end"), 0.0) },
        (Stranger, Success) => Fate { stranger: Some(STRANGERS[(pick % STRANGERS.len() as u64) as usize]), deeds: 1.0,
                                      ..f(format!("a stranger on the marked hex joins {name}"), 0.0) },
        (Stranger, Fail) => f(format!("the stranger on the marked hex shrugs at {name} and walks away"), 0.0),
        (Stranger, CritFail) => f(format!("the stranger on the marked hex was an assassin: a knife for {name}"), 1.5),
        (Undead, CritSuccess) => Fate { find: pickf(&NIGHT_FINDS[..2]), deeds: 3.0, ..f(format!("{name} lays the risen dead to rest and takes their grave-goods"), 0.0) },
        (Undead, Success) => Fate { deeds: 2.0, ..f(format!("{name} cuts down the risen dead"), 0.0) },
        (Undead, Fail) => f(format!("the risen dead claw {name}"), 2.0),
        (Undead, CritFail) => f(format!("the risen dead drag {name} into the dark"), 3.5),
        (Ghost, CritSuccess) => Fate { sparks: 2, deeds: 2.0, ..f(format!("a ghost whispers a god's secret to {name}"), 0.0) },
        (Ghost, Success) => Fate { sparks: 1, deeds: 1.0, ..f(format!("a ghost grants {name} a spark of the old power"), 0.0) },
        (Ghost, Fail) => f(format!("a ghost's wail chills {name} to the bone"), 1.0),
        (Ghost, CritFail) => f(format!("a ghost passes through {name}'s heart"), 2.0),
        (Thief, CritSuccess) => Fate { find: pickf(&NIGHT_FINDS[2..]), deeds: 2.0, ..f(format!("{name} catches the night thief and takes their haul"), 0.0) },
        (Thief, Success) => Fate { deeds: 1.0, ..f(format!("{name} sees the night thief off"), 0.0) },
        (Thief, Fail) => Fate { sparks: -1, ..f(format!("a night thief picks {name}'s pocket"), 0.0) },
        (Thief, CritFail) => Fate { sparks: -1, ..f(format!("a night thief knifes {name} and runs"), 1.0) },
    }
}

fn reveal_encounters(w: &mut World, beat: u32, s: &Start, pantheon: &Pantheon, st: &Settings) {
    let reach = reach_km(w, st);
    let night = is_night(beat, st);
    let ids: Vec<u32> = live_hidden(w).filter(|e| e.placed_beat < beat).map(|e| e.id).collect();
    for id in ids {
        let e = w.dm.encounters[id as usize].clone();
        let Some(d) = stepper(w, s, e.at, reach, None) else { continue };
        if !w.demigods[d.ix()].alive {
            continue;
        }
        let name = w.demigods[d.ix()].name.clone();
        let mut m = demigod_modifier(w, d);
        if night && matches!(e.kind, EncounterKind::Monster | EncounterKind::Trap) {
            m -= 1;
        }
        if w.dm.shrines.iter().any(|x| w.same_player(x.demigod, d) && distance_km(x.at, e.at) <= reach * 3.0) {
            m += 1;
        }
        let q = format!("encounter:{id}");
        let roll = d20(w, beat, &q, Ref::Demigod(d), m, kind_dc(e.kind), &format!("a {} encounter", e.kind.as_str()), e.at);
        let pick = Rng::derive(w.meta.seed, &[ENC_SALT, hash_str(&q)]).next_u64();
        let mut fx = fate(&e, roll.verdict, &name, pick);
        // The Lens and the Engine: a small Rube Goldberg flourish that rhymes with what this region remembers.
        let prior = w.dm.memory.iter().rev()
            .filter(|m| m.public && m.beat < beat && m.up.contains(&Ref::Place(e.region).to_string())
                && !matches!(m.kind.as_str(), "dice.rolled" | "memory.rollup" | "place.observed" | "time.phase"))
            .map(|m| m.beat)
            .next();
        let flourish = FLOURISHES[(pick >> 8) as usize % FLOURISHES.len()];
        fx.text = match prior {
            Some(b) => format!("{}, {flourish}, as the omen of beat {b} foretold", fx.text),
            None => format!("{}, {flourish}", fx.text),
        };
        let actor = Ref::Place(e.settlement.unwrap_or(e.region));
        let mut props = vec![];
        let damage = (fx.damage * st.dm_encounter_damage_scale).max(0.0);
        if damage > 0.0 {
            props.push(rules_prop(
                Effect::Damage { who: Ref::Demigod(d), amount: (damage * 10.0).round() / 10.0, cause: format!("encounter: {}", fx.text) },
                fx.text.clone(), actor, (3.0 + damage).min(9.0) as u8, -2, vec![Ref::Demigod(d)]));
        } else {
            props.push(rules_prop(Effect::Happen { text: fx.text.clone(), at: e.at }, fx.text.clone(), actor, 4,
                                  if roll.verdict.ok() { 2 } else { -1 }, vec![Ref::Demigod(d)]));
        }
        let first_item = w.items.len();
        if let Some(f) = fx.find {
            props.push(rules_prop(Effect::CreateItem { name: f.name.into(), description: f.desc.into(), holder: Ref::Demigod(d) },
                                  format!("{name} takes the {}", f.name), actor, 4, 2, vec![Ref::Demigod(d)]));
        }
        let first_char = w.characters.len();
        if let Some(t) = fx.stranger {
            let near = Some(w.demigods[d.ix()].place);
            props.push(rules_prop(
                Effect::Memorize { mention: Mention { text: t.into(), kind: "person".into(), near }, cluster: None },
                format!("{name} meets {t}"), actor, 4, 1, vec![Ref::Demigod(d)]));
        }
        apply(w, props, pantheon, beat, st);
        let item = (w.items.len() > first_item).then(|| ItemId::from_ix(first_item));
        if let (Some(i), Some(f)) = (item, fx.find) {
            let it = &mut w.items[i.ix()];
            it.ext.kind = f.kind;
            it.ext.unit = f.kind.unit().to_string();
            it.ext.tier = if roll.verdict == DiceVerdict::CritSuccess { Tier::Artifact } else { Tier::Trinket };
            it.ext.at = Some(e.at);
            it.ext.history.push(ItemEvent { beat, state: ItemState::Held, holder: Some(Ref::Demigod(d)), by: None, row: None, count: None });
        }
        // The stranger becomes a named character who follows the demigod.
        let character = (w.characters.len() > first_char).then(|| CharId::from_ix(first_char)).or_else(|| {
            fx.stranger.and_then(|_| {
                w.ledger.rows.iter().rev().take(4).find(|r| r.verb == "memorize").and_then(|r| {
                    r.targets.iter().find_map(|t| match t {
                        Ref::Char(c) => Some(*c),
                        _ => None,
                    })
                })
            })
        });
        if let Some(c) = character {
            let following = w.demigods[d.ix()].following;
            let cname = w.characters[c.ix()].name.clone();
            apply(w, vec![rules_prop(Effect::Join { member: Ref::Char(c), group: following },
                                     format!("{cname} joins {name}'s following"), actor, 3, 2, vec![Ref::Demigod(d)])],
                  pantheon, beat, st);
        }
        if fx.sparks != 0 {
            let cur = w.demigods[d.ix()].sparks as i32;
            w.set_sparks(d, (cur + fx.sparks).max(0) as u32);
        }
        add_deeds(w, d, fx.deeds);
        let outcome = EncounterOutcome { beat, by: d, roll: roll.clone(), text: fx.text.clone(), damage, item, character,
                                         sparks: fx.sparks };
        w.dm.encounters[id as usize].outcome = Some(outcome);
        let wh = Where { cell: e.cell.clone(), at: e.at, settlement: e.settlement, region: e.region };
        let data = json!({ "id": id, "cell": e.cell, "type": e.kind, "by": Ref::Demigod(d), "verdict": roll.verdict,
                           "roll": roll.roll, "text": fx.text, "damage": damage, "item": item.map(Ref::Item),
                           "character": character.map(Ref::Char), "sparks": fx.sparks });
        remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "encounter.revealed", query: Some(format!("encounter:{id}:revealed")),
                                          public: true, text: fx.text, data });
    }
}

/// An encounter as the public sees it: hidden `{id, cell, marked}`; revealed adds `revealed, type, beat, outcome`;
/// faded adds `faded_beat`.
pub fn encounter_json(w: &World, e: &Encounter) -> Value {
    let mut v = json!({ "id": e.id, "cell": e.cell, "marked": true });
    if let Some(o) = &e.outcome {
        v["revealed"] = true.into();
        v["type"] = json!(e.kind);
        v["night"] = e.night.into();
        v["at"] = json!(e.at);
        v["region_ref"] = json!(Ref::Place(e.region));
        v["outcome"] = json!({ "beat": o.beat, "by": Ref::Demigod(o.by), "by_name": w.name_of(Ref::Demigod(o.by)),
                               "roll": o.roll.roll, "modifier": o.roll.modifier, "total": o.roll.total,
                               "target_dc": o.roll.target_dc, "verdict": o.roll.verdict, "text": o.text,
                               "damage": o.damage, "item": o.item.map(Ref::Item),
                               "item_icon": o.item.and_then(|i| w.items.get(i.ix())).map(|i| icon_of(i.ext.kind, &i.name)),
                               "character": o.character.map(Ref::Char), "sparks": o.sparks });
    } else if let Some(b) = e.faded_beat {
        v["faded_beat"] = b.into();
    }
    v
}

/// Encounters on the board (hidden and revealed; not faded), optionally under one place.
pub fn encounters_under(w: &World, p: Option<PlaceId>) -> Vec<Value> {
    w.dm.encounters
        .iter()
        .filter(|e| e.faded_beat.is_none())
        .filter(|e| p.is_none_or(|p| under(w, p, e.region, e.settlement)))
        .map(|e| encounter_json(w, e))
        .collect()
}

/// Is something in `region` / `settlement` under place `p` (the arena, its region, its settlement)?
pub fn under(w: &World, p: PlaceId, region: PlaceId, settlement: Option<PlaceId>) -> bool {
    p == w.meta.arena || p == region || settlement == Some(p)
        || settlement.is_some_and(|s| w.places.get(p.ix()).is_some_and(|x| x.kind == PlaceKind::Quarter && x.parent == Some(s)))
}

// ---- loot ---------------------------------------------------------------------------------------------------------

/// The items a demigod holds that aren't already lying in a pile (what it would drop).
pub fn held_items(w: &World, d: DemigodId) -> Vec<ItemId> {
    let me = Ref::Demigod(d);
    w.items
        .iter()
        .filter(|i| i.holders.last() == Some(&me) && i.ext.state == ItemState::Held && i.ext.muster.is_none())
        .filter(|i| !w.dm.loot.iter().any(|l| l.taken_beat.is_none() && l.items.contains(&i.id)))
        .map(|i| i.id)
        .collect()
}

fn drop_loot(w: &mut World, beat: u32, d: DemigodId, items: Vec<ItemId>, why: &str) {
    let pos = w.demigods[d.ix()].pos;
    drop_loot_at(w, beat, d, items, why, pos);
}

/// Drop `items` of `d` as a pile on the people-level hex at `pos` (rules 0.8.1: a duel's loser drops on the duel's hex).
pub fn drop_loot_at(w: &mut World, beat: u32, d: DemigodId, items: Vec<ItemId>, why: &str, pos: LonLat) {
    if items.is_empty() {
        return;
    }
    let wh = locate(w, pos, HexLevel::People);
    let id = w.dm.loot.len() as u32;
    w.dm.loot.push(LootPile { id, cell: wh.cell.clone(), at: pos, region: wh.region, items: items.clone(), since_beat: beat,
                              from: d, taken_beat: None, taken_by: None });
    let name = w.demigods[d.ix()].name.clone();
    let text = format!("{name}'s things lie on the ground after {why}");
    let icons: Vec<Value> = items.iter().map(|i| json!({ "ref": Ref::Item(*i), "icon": icon_of(w.items[i.ix()].ext.kind, &w.items[i.ix()].name) })).collect();
    remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "loot.dropped", query: Some(format!("loot:{id}:dropped")),
                                      public: true, text, data: json!({ "pile": id, "cell": wh.cell, "from": Ref::Demigod(d),
                                                                         "items": icons, "why": why }) });
}

fn loot_drops(w: &mut World, beat: u32, s: &Start) {
    // A death drops everything it carried.
    let dead: Vec<DemigodId> = s.alive.iter().copied().filter(|d| !w.demigods[d.ix()].alive).collect();
    for d in &dead {
        let items = held_items(w, *d);
        drop_loot(w, beat, *d, items, "a death");
    }
    // A fight: of two engaged demigods, the one hurt worse rolls to keep hold of its newest thing (DC 11).
    for (a, b) in crate::occupancy::engaged_pairs(w, s.row_start) {
        if dead.contains(&a) || dead.contains(&b) {
            continue;
        }
        let hurt = |x: DemigodId| s.health.get(x.ix()).copied().unwrap_or(0.0) - w.demigods[x.ix()].health;
        let (ha, hb) = (hurt(a), hurt(b));
        if ha.max(hb) < 1.0 {
            continue;
        }
        let loser = if ha >= hb { a } else { b };
        let Some(newest) = held_items(w, loser).last().copied() else { continue };
        let pos = w.demigods[loser.ix()].pos;
        let m = demigod_modifier(w, loser);
        let r = d20(w, beat, &format!("fight:{beat}:{}:{}", a.0, b.0), Ref::Demigod(loser), m, 11, "keeping hold in a fight", pos);
        if !r.verdict.ok() {
            drop_loot(w, beat, loser, vec![newest], "a fight");
        }
    }
}

fn loot_pickups(w: &mut World, beat: u32, s: &Start, st: &Settings) {
    let reach = reach_km(w, st);
    let piles: Vec<u32> = w.dm.loot.iter().filter(|l| l.taken_beat.is_none() && l.since_beat < beat).map(|l| l.id).collect();
    for id in piles {
        let l = w.dm.loot[id as usize].clone();
        // The one who dropped it may take it back only after 3 beats.
        let exclude = (beat < l.since_beat + 3).then_some(l.from);
        let Some(d) = stepper(w, s, l.at, reach, exclude) else { continue };
        let me = Ref::Demigod(d);
        let rival = !w.same_player(d, l.from);
        for i in &l.items {
            let it = &mut w.items[i.ix()];
            it.holders.push(me);
            it.ext.state = ItemState::Held;
            it.ext.history.push(ItemEvent { beat, state: if rival { ItemState::Stolen } else { ItemState::Transferred },
                                            holder: Some(me), by: Some(me), row: None, count: None });
            if let Some(x) = w.demigods.iter_mut().find(|x| x.id == l.from) {
                x.items.retain(|y| y != i);
            }
            let dg = &mut w.demigods[d.ix()];
            if !dg.items.contains(i) {
                dg.items.push(*i);
            }
        }
        let p = &mut w.dm.loot[id as usize];
        p.taken_beat = Some(beat);
        p.taken_by = Some(d);
        let wh = Where { cell: l.cell.clone(), at: l.at, settlement: settlement_at(w, l.at), region: l.region };
        let text = format!("{} takes what {} left on the ground", w.name_of(me), w.name_of(Ref::Demigod(l.from)));
        remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "loot.taken", query: Some(format!("loot:{id}:taken")),
                                          public: true, text, data: json!({ "pile": id, "cell": l.cell, "by": me,
                                                                             "items": l.items.iter().map(|i| Ref::Item(*i)).collect::<Vec<_>>() }) });
        add_deeds(w, d, 1.0);
    }
}

/// Loot on the board, flattened per item: `[{ref, icon, cell, since_beat, pile, at}]`, optionally under one place.
pub fn loot_under(w: &World, p: Option<PlaceId>) -> Vec<Value> {
    let art = crate::look::on(w).then(|| crate::look::book(w));
    let art = &art;
    w.dm.loot
        .iter()
        .filter(|l| l.taken_beat.is_none())
        .filter(|l| p.is_none_or(|p| under(w, p, l.region, settlement_at(w, l.at))))
        .flat_map(|l| {
            l.items.iter().filter_map(move |i| {
                let it = w.items.get(i.ix())?;
                let mut v = json!({ "ref": Ref::Item(*i), "icon": icon_of(it.ext.kind, &it.name), "cell": l.cell,
                                    "since_beat": l.since_beat, "pile": l.id, "at": l.at });
                if let Some(b) = art {
                    v["name"] = it.name.clone().into();
                    v["tier"] = json!(it.ext.tier);
                    crate::look::art_fields(w, b, it, &mut v);
                }
                Some(v)
            })
        })
        .collect()
}

// ---- roads --------------------------------------------------------------------------------------------------------

fn wear(w: &mut World, a: PlaceId, b: PlaceId, x: f32, beat: u32) {
    if a == b {
        return;
    }
    let (a, b) = if a < b { (a, b) } else { (b, a) };
    match w.dm.trails.iter_mut().find(|t| t.a == a && t.b == b) {
        Some(t) => {
            t.wear += x;
            t.last_used = beat;
        }
        None => w.dm.trails.push(Trail { a, b, wear: x, road_since: None, last_used: beat }),
    }
}

fn trails_step(w: &mut World, beat: u32, s: &Start, st: &Settings) {
    let max = if w.is_planet() { 400.0 } else { 4.0 };
    let min_move = reach_km(w, st) / 10.0;
    // Demigods.
    for d in s.alive {
        let Some(x) = w.demigods.get(d.ix()).filter(|x| x.alive) else { continue };
        let from = s.pos.get(d.ix()).copied().unwrap_or(x.pos);
        if distance_km(from, x.pos) < min_move {
            continue;
        }
        if let (Some(a), Some(b)) = (nearest_settlement(w, from, max), nearest_settlement(w, x.pos, max)) {
            wear(w, a, b, 1.0, beat);
        }
    }
    // Armies.
    for (c, from) in s.armies {
        let Some((to, size)) = w.clusters.get(c.ix()).and_then(|x| x.army.as_ref().map(|a| (a.pos, x.size))) else { continue };
        if distance_km(*from, to) < min_move {
            continue;
        }
        if let (Some(a), Some(b)) = (nearest_settlement(w, *from, max), nearest_settlement(w, to, max)) {
            wear(w, a, b, (1.0 + size as f32 / 20_000.0).min(3.0), beat);
        }
    }
    // Crowds on the move (the society's migrations this beat).
    let migr: Vec<(PlaceId, PlaceId, f32)> = w.ledger.rows[s.row_start..]
        .iter()
        .filter(|r| r.verb == "migration")
        .filter_map(|r| {
            let from = fn7_society::geo::city_of(w, r.place?)?;
            let to = r.targets.iter().find_map(|t| match t {
                Ref::Place(p) => fn7_society::geo::city_of(w, *p),
                _ => None,
            })?;
            Some((from, to, 0.5))
        })
        .collect();
    for (a, b, x) in migr {
        wear(w, a, b, x, beat);
    }
    // Wear in, fade out.
    let threshold = st.dm_road_wear.max(0.1);
    for k in 0..w.dm.trails.len() {
        let t = &mut w.dm.trails[k];
        if t.last_used < beat {
            t.wear = (t.wear - st.dm_road_decay).max(0.0);
        }
        let (formed, faded) = (t.road_since.is_none() && t.wear >= threshold, t.road_since.is_some() && t.wear < threshold / 2.0);
        if formed {
            t.road_since = Some(beat);
        }
        if faded {
            t.road_since = None;
        }
        if formed || faded {
            let (a, b, wear) = (t.a, t.b, t.wear);
            let (an, bn) = (w.name_of(Ref::Place(a)), w.name_of(Ref::Place(b)));
            let center = w.places[a.ix()].center;
            let wh = Where { settlement: Some(a), ..locate(w, center, HexLevel::People) };
            let (kind, text) = if formed {
                ("road.formed", format!("a road wears in between {an} and {bn}"))
            } else {
                ("road.faded", format!("the road between {an} and {bn} grows over"))
            };
            remember(w, beat, &wh, Decision { scope: Scope::Region, kind, query: None, public: true, text,
                                              data: json!({ "from_ref": Ref::Place(a), "to_ref": Ref::Place(b), "wear": wear,
                                                            "from_cell": people_cell(w.places[a.ix()].center),
                                                            "to_cell": people_cell(w.places[b.ix()].center) }) });
        }
    }
    w.dm.trails.retain(|t| t.wear > 0.0 || t.road_since.is_some());
}

/// A road: `{from_cell, to_cell, wear, since_beat, from_ref, to_ref, from_name, to_name, path}` (`path`: the two
/// settlement centres, `[lon, lat]`).
pub fn road_json(w: &World, t: &Trail) -> Value {
    let (a, b) = (&w.places[t.a.ix()], &w.places[t.b.ix()]);
    json!({ "from_cell": people_cell(a.center), "to_cell": people_cell(b.center), "wear": (t.wear * 100.0).round() / 100.0,
            "since_beat": t.road_since, "from_ref": Ref::Place(t.a), "to_ref": Ref::Place(t.b), "from_name": a.name,
            "to_name": b.name, "path": [a.center, b.center] })
}

/// Roads, optionally only those with an end under place `p`.
pub fn roads_under(w: &World, p: Option<PlaceId>) -> Vec<Value> {
    w.dm.roads()
        .filter(|t| {
            p.is_none_or(|p| {
                [t.a, t.b].iter().any(|s| under(w, p, fn7_society::geo::district_of(w, *s), Some(*s)))
            })
        })
        .map(|t| road_json(w, t))
        .collect()
}

// ---- shrines ------------------------------------------------------------------------------------------------------

const SHRINE_NOUNS: &[&str] = &["shrine", "temple", "monument", "altar", "statue", "idol", "obelisk", "sanctuary", "chapel"];
const SHRINE_VERBS: &[&str] =
    &["build", "raise", "erect", "construct", "found", "consecrate", "carve", "make", "manifest", "create", "dedicate"];

fn shrine_power(text: &str, seed: u64, id: u32) -> ShrinePower {
    let t = text.to_lowercase();
    if ["heal", "mercy", "life", "rest", "spring"].iter().any(|k| t.contains(k)) {
        ShrinePower::Heal
    } else if ["ward", "war", "wrath", "fear", "guard", "fort"].iter().any(|k| t.contains(k)) {
        ShrinePower::Ward
    } else if ["spark", "light", "power", "lightning", "star"].iter().any(|k| t.contains(k)) {
        ShrinePower::Spark
    } else {
        [ShrinePower::Spark, ShrinePower::Heal, ShrinePower::Ward][Rng::derive(seed, &[SHRINE_SALT, id as u64]).below(3) as usize]
    }
}

/// Shrine level from its worshippers: 1 below 1,000, then one more per tenfold (at most 5).
pub fn shrine_level(worshippers: u64) -> u8 {
    let mut l = 1u8;
    let mut t = 1_000u64;
    while worshippers >= t && l < 5 {
        l += 1;
        t *= 10;
    }
    l
}

fn build_shrine(w: &mut World, beat: u32, d: DemigodId, at: LonLat, text: &str, st: &Settings) {
    let x = &w.demigods[d.ix()];
    if !x.alive {
        return;
    }
    let mine = w.dm.shrines.iter().filter(|s| s.demigod == d).count();
    if mine >= st.dm_shrines_per_demigod as usize || w.dm.shrines.iter().any(|s| s.demigod == d && s.built_beat == beat) {
        return;
    }
    let (seat, place, name) = (x.seat, x.place, x.name.clone());
    let wh = locate(w, at, HexLevel::Houses);
    let id = w.dm.shrines.len() as u32;
    let power = shrine_power(text, w.meta.seed, id);
    let sname = format!("Shrine of {name}");
    w.dm.shrines.push(Shrine { id, cell: wh.cell.clone(), at, region: wh.region, place: wh.settlement.unwrap_or(place), demigod: d,
                               seat, name: sname.clone(), worshippers: 0, level: 1, power, built_beat: beat, last_grant: beat });
    add_deeds(w, d, 3.0);
    remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "shrine.built", query: Some(format!("shrine:{id}:built")), public: true,
                                      text: format!("the {sname} is raised ({})", power.as_str()),
                                      data: json!({ "shrine": id, "cell": wh.cell, "owner": Ref::Demigod(d), "power": power }) });
}

fn shrines_step(w: &mut World, beat: u32, s: &Start, pantheon: &Pantheon, st: &Settings) {
    // Built: by order (a making verb and a shrine noun) or by the item engine (a made structure).
    let mut builds: Vec<(DemigodId, LonLat, String)> = vec![];
    for i in s.intent_start.min(w.intents.len())..w.intents.len() {
        let it = &w.intents[i];
        let text = format!("{} {}", it.verb, it.span);
        if has_any(&text, SHRINE_VERBS) && has_any(&text, SHRINE_NOUNS) {
            if let Some(d) = w.demigods.get(it.demigod.ix()).filter(|d| d.alive) {
                builds.push((d.id, d.pos, text));
            }
        }
    }
    for m in &w.manifests[s.manifest_start.min(w.manifests.len())..] {
        let Some(it) = m.item.and_then(|i| w.items.get(i.ix())) else { continue };
        if it.ext.kind == ItemKind::Structure && !builds.iter().any(|b| b.0 == m.demigod) {
            builds.push((m.demigod, m.at.unwrap_or(w.demigods[m.demigod.ix()].pos), format!("{} {}", it.name, it.description)));
        }
    }
    for (d, at, text) in builds {
        build_shrine(w, beat, d, at, &text, st);
    }
    // Worshippers come from crowds near the shrine (more with devotion and spiritual hunger) and as pilgrims from its
    // demigod's Following; an abandoned shrine (its demigod fallen) loses a tenth a beat.
    let radius = if w.is_planet() { 300.0 } else { 3.0 };
    let reach = reach_km(w, st) * 3.0;
    let mut props = vec![];
    for k in 0..w.dm.shrines.len() {
        let sh = w.dm.shrines[k].clone();
        let owner_alive = w.demigods[sh.demigod.ix()].alive;
        let gain: f64 = if owner_alive {
            w.clusters
                .iter()
                .filter(|c| c.army.is_none() && c.size > 0)
                .filter_map(|c| {
                    let at = fn7_society::geo::pos_of(w, Ref::Cluster(c.id))?;
                    (distance_km(at, sh.at) <= radius).then(|| {
                        let faith = w.nodes.get(c.place.ix()).map(|n| n.need.faith).unwrap_or(0.0) as f64;
                        c.size as f64 * (0.001 + 0.004 * c.mood.devotion.clamp(0.0, 1.0) as f64) * (1.0 + faith)
                    })
                })
                .sum::<f64>()
                .min(250_000.0)
                // Pilgrims from its demigod's Following, wherever they are.
                + (fn7_society::headcount(w, w.demigods[sh.demigod.ix()].following) as f64 / 1000.0).clamp(5.0, 500.0)
        } else {
            -(sh.worshippers as f64) / 10.0
        };
        let worshippers = (sh.worshippers as f64 + gain).max(0.0).round() as u64;
        let level = shrine_level(worshippers);
        let x = &mut w.dm.shrines[k];
        x.worshippers = worshippers;
        if level != sh.level {
            x.level = level;
            let wh = Where { cell: sh.cell.clone(), at: sh.at, settlement: settlement_at(w, sh.at), region: sh.region };
            remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "shrine.level", query: None, public: true,
                                              text: format!("the {} reaches level {level}", sh.name),
                                              data: json!({ "shrine": sh.id, "level": level, "worshippers": worshippers }) });
        }
        if !owner_alive {
            continue;
        }
        let level = level as f32;
        match sh.power {
            ShrinePower::Spark => {
                let every = st.dm_shrine_spark_every.saturating_sub(level as u32 - 1).max(2);
                if beat >= sh.last_grant + every {
                    let cur = w.demigods[sh.demigod.ix()].sparks;
                    w.set_sparks(sh.demigod, cur + 1);
                    w.dm.shrines[k].last_grant = beat;
                    event("shrine_grant", None, json!({ "beat": beat, "shrine": sh.id, "sparks": 1 }));
                }
            }
            ShrinePower::Heal => {
                for d in w.demigods.iter().filter(|d| d.alive && d.seat == sh.seat && d.health < d.max_health) {
                    if distance_km(d.pos, sh.at) <= reach {
                        props.push(rules_prop(Effect::Heal { who: Ref::Demigod(d.id), amount: st.dm_shrine_heal * level },
                                              format!("the {} mends {}", sh.name, d.name), Ref::Place(sh.place), 2, 1,
                                              vec![Ref::Demigod(d.id)]));
                    }
                }
            }
            ShrinePower::Ward => {
                for d in w.demigods.iter().filter(|d| d.alive && d.seat != sh.seat) {
                    if distance_km(d.pos, sh.at) <= reach {
                        let text = format!("the {} wards off {}", sh.name, d.name);
                        props.push(rules_prop(Effect::Damage { who: Ref::Demigod(d.id), amount: st.dm_shrine_ward * level,
                                                               cause: text.clone() },
                                              text, Ref::Place(sh.place), 2, -1, vec![Ref::Demigod(d.id)]));
                    }
                }
            }
        }
    }
    if !props.is_empty() {
        apply(w, props, pantheon, beat, st);
    }
}

/// A shrine: `{id, cell, owner_seat, side, worshippers, power, level, owner, owner_name, player, name, at, region_ref,
/// built_beat}`.
pub fn shrine_json(w: &World, s: &Shrine) -> Value {
    let d = &w.demigods[s.demigod.ix()];
    json!({ "id": s.id, "cell": s.cell, "owner_seat": (s.seat < crate::SYNTHETIC_SEAT).then_some(s.seat), "side": d.side,
            "worshippers": s.worshippers, "power": s.power, "level": s.level, "owner": Ref::Demigod(s.demigod),
            "owner_name": d.name, "player": d.player_name(), "name": s.name, "at": s.at, "region_ref": Ref::Place(s.region),
            "built_beat": s.built_beat, "owner_alive": d.alive })
}

pub fn shrines_under(w: &World, p: Option<PlaceId>) -> Vec<Value> {
    w.dm.shrines
        .iter()
        .filter(|s| p.is_none_or(|p| under(w, p, s.region, Some(s.place))))
        .map(|s| shrine_json(w, s))
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// The beat's public payload
// ---------------------------------------------------------------------------------------------------------------------

/// The DM's part of a beat's outcome (from the memory written this beat): `dice` (`dice.rolled` events), `dm_events`
/// (everything else public: settlement.grew, encounter.revealed, loot.dropped...) and the time of day.
pub fn beat_public(w: &World, beat: u32, memory_start: usize, st: &Settings, public: &mut Value) {
    let new = &w.dm.memory[memory_start.min(w.dm.memory.len())..];
    let mut dice = vec![];
    let mut events = vec![];
    for e in new.iter().filter(|e| e.public) {
        let mut v = e.data.clone();
        if !v.is_object() {
            v = json!({});
        }
        v["event"] = e.kind.clone().into();
        if e.kind == "dice.rolled" {
            if let Some(o) = v.as_object_mut() {
                o.remove("confirm");
            }
            if dice.len() < DICE_PER_BEAT {
                dice.push(v);
            }
        } else if matches!(e.kind.as_str(), "memory.rollup" | "place.observed") {
            continue;
        } else if events.len() < EVENTS_PER_BEAT {
            v["text"] = e.text.clone().into();
            v["memory"] = e.id.into();
            events.push(v);
        }
    }
    public["dice"] = dice.into();
    public["dm_events"] = events.into();
    add_time_of_day(public, beat, st);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_and_night_cycle() {
        let ph: Vec<&str> = (1..=9).map(|b| phase_of(b, 8).1).collect();
        assert_eq!(ph, vec!["dawn", "day", "day", "day", "dusk", "night", "night", "night", "dawn"]);
        assert_eq!(phase_of(9, 8).0, 2);
        assert_eq!(phase_of(1, 8).2, 0.5);
        assert_eq!(phase_of(3, 8).2, 1.0);
        assert_eq!(phase_of(5, 8).2, 0.5);
        assert_eq!(phase_of(7, 8).2, 0.0);
    }

    #[test]
    fn verdicts_and_crits() {
        assert_eq!(verdict(1, 30, 5), DiceVerdict::CritFail);
        assert_eq!(verdict(20, 2, 25), DiceVerdict::CritSuccess);
        assert_eq!(verdict(10, 12, 12), DiceVerdict::Success);
        assert_eq!(verdict(10, 11, 12), DiceVerdict::Fail);
    }

    #[test]
    fn segments_and_levels() {
        assert!(passes_within([0.0, 0.0], [2.0, 0.0], [1.0, 0.1], 20.0).is_some());
        assert!(passes_within([0.0, 0.0], [2.0, 0.0], [1.0, 1.0], 20.0).is_none());
        assert_eq!((shrine_level(0), shrine_level(999), shrine_level(1000), shrine_level(5_000_000)), (1, 1, 2, 5));
        assert_eq!((base_level(10), base_level(1_500_000), base_level(20_000_000)), (1, 3, 5));
        assert_eq!(settlement_cells([2.35, 48.85], 3).len(), 19);
    }
}
