//! The simulator inside a match. Casts (and god-tier powers, and the physical ramifications Entropy leaves as world
//! mods) are logged as inputs; once a beat, before the DM's step, the field is stepped and what it did goes back
//! through the rules that already exist: bounded damage to demigods standing in it (shaped by gear like any damage),
//! fear and awe on crowds, ruin that lowers settlements' level targets, roads cut, shrines toppled, encounters
//! uncovered, and place × time entries in the DM's memory that become the beat's public events.

use std::collections::BTreeMap;

use fn7_model::dm::Scope;
use fn7_model::earth::{EarthInput, EarthState};
use fn7_model::hexgrid::HexLevel;
use fn7_model::*;
use fn7_society::geo::pos_of;
use serde_json::{json, Value};

use super::codec::Sparse;
use super::grid::{grid, NONE, SEA_LEVEL};
use super::sim::*;
use crate::dm::{locate, remember, Decision};
use crate::settings::Settings;
use crate::world::event;

/// Is the simulator running in this match (a schema-13 planet with `earth_sim_enabled`; [`crate::world::settings`]
/// has already folded the first two into the setting).
pub fn on(w: &World, st: &Settings) -> bool {
    st.earth_sim_enabled && w.earth_rules() && w.is_planet()
}

/// The simulation's settings in fixed point.
pub fn cfg(st: &Settings) -> Cfg {
    let q = |x: f32| (x * 16.0).round().clamp(0.0, 4096.0) as u32;
    Cfg { max_cells: st.earth_max_cells, max_phenomena: st.earth_max_phenomena, substeps: st.earth_substeps,
          keyframe_beats: st.earth_keyframe_beats.max(1), plate_beats: st.earth_plate_beats, spread_q: q(st.earth_spread_scale),
          spontaneous_q: q(st.earth_spontaneous_scale), energy_q: q(st.earth_energy_scale).max(1), mini_q: q(st.earth_mini_scale) }
}

/// The simulation as the match holds it now (`None` when it is off or has not begun).
pub fn sim_of(w: &World, st: &Settings) -> Option<Sim> {
    let s = w.dm.earth.as_deref()?;
    on(w, st).then(|| Sim::from_state(w.meta.seed, cfg(st), s))
}

/// Which phenomenon a power is. A free-form power is read from its words; one that names nothing physical is none.
pub fn kind_for(slug: &str, text: Option<&str>) -> Option<u8> {
    Some(match slug {
        "lightning" => LIGHTNING,
        "storm_clouds" => STORM,
        "volcano" => VOLCANO,
        "earthquake" => EARTHQUAKE,
        "flood" => FLOOD,
        "wildfire" => WILDFIRE,
        "meteor" => METEOR,
        "raise_land" => RAISE_LAND,
        "sink_land" => SINK_LAND,
        "grow_forest" => FOREST,
        "swarm" => SWARM,
        "god_nature" => FOREST,
        "god_chance" => STORM,
        "god_death" => BLIZZARD,
        "god_consequence" => EARTHQUAKE,
        "god_entropy" => EARTHQUAKE,
        _ => return kind_from_words(text?),
    })
}

/// A phenomenon named by free words (a custom power's text, a ramification's name and tags).
pub fn kind_from_words(text: &str) -> Option<u8> {
    let t = text.to_lowercase();
    const WORDS: &[(u8, &[&str])] = &[
        (VOLCANO, &["volcan", "lava", "erupt", "magma"]),
        (METEOR, &["meteor", "asteroid", "comet", "falling star"]),
        (EARTHQUAKE, &["quake", "tremor", "seism", "aftershock"]),
        (TIDAL_WAVE, &["tsunami", "tidal"]),
        (BLIZZARD, &["blizzard", "snow", "frost", "freez", "ice ", "icy", "glacier", "hail"]),
        (WILDFIRE, &["fire", "burn", "blaze", "inferno", "flame", "scorch"]),
        (FLOOD, &["flood", "deluge", "lake", "drown", "river", "monsoon", "inundat"]),
        (SINK_LAND, &["sink", "swallow", "chasm", "sinkhole", "collapse"]),
        (RAISE_LAND, &["raise", "mountain", "island", "uplift"]),
        (LIGHTNING, &["lightning", "thunderbolt"]),
        (STORM, &["storm", "rain", "thunder", "cloud", "hurricane", "cyclone", "tempest", "gale", "tornado"]),
        (SWARM, &["swarm", "locust", "plague", "insect", "vermin", "rats"]),
        (FOREST, &["forest", "grow", "bloom", "jungle", "overgrow", "vines", "trees"]),
    ];
    WORDS.iter().find(|(_, ws)| ws.iter().any(|x| t.contains(x))).map(|x| x.0)
}

fn state_mut(w: &mut World) -> &mut EarthState {
    w.dm.earth.get_or_insert_with(Default::default)
}

fn e3(p: LonLat) -> (i32, i32) {
    ((p[0] * 1000.0).round() as i32, (p[1] * 1000.0).round() as i32)
}

/// Log an input for this beat's step.
#[allow(clippy::too_many_arguments)]
pub fn note(w: &mut World, beat: u32, st: &Settings, kind: u8, at: LonLat, magnitude: u8, radius_km: f32, tier: u8, owner: u8, cause: u8) {
    if !on(w, st) {
        return;
    }
    let cell = grid().index_of(at);
    if cell == NONE {
        return;
    }
    let radius = if tier == TIER_MINI { 0 } else { (radius_km / st.earth_km_per_cell.max(1.0)).round().clamp(1.0, 4.0) as u8 };
    let (lon_e3, lat_e3) = e3(at);
    let input = EarthInput { beat, kind, cell, lon_e3, lat_e3, energy: magnitude.clamp(1, 10), radius, owner, cause, tier };
    event("earth_input", None, json!({ "beat": beat, "kind": kind_name(kind), "cell": grid().key(cell), "at": at,
                                         "energy": input.energy, "radius": radius, "tier": TIERS[tier.min(3) as usize],
                                         "cause": CAUSES[cause.min(4) as usize], "owner": owner }));
    state_mut(w).inputs.push(input);
}

/// A cast that landed (from `casts::process`): the power as it landed (a mega already scaled by its fault line).
pub fn note_cast(w: &mut World, beat: u32, st: &Settings, p: &PowerDef, text: Option<&str>, at: LonLat, scale: Option<CastScale>, by: DemigodId) {
    if !on(w, st) {
        return;
    }
    let Some(kind) = kind_for(&p.slug, text) else { return };
    let tier = if scale == Some(CastScale::Mega) { TIER_MEGA } else { TIER_MINI };
    let seat = w.demigods[by.ix()].seat;
    note(w, beat, st, kind, at, p.magnitude, p.radius_km, tier, seat, CAUSE_CAST);
}

/// A god-tier power that was called (from `godtier::process`). Entropy's Chaos Burst shakes three faults at once.
pub fn note_god(w: &mut World, beat: u32, st: &Settings, slug: &str, at: LonLat, by: DemigodId) {
    if !on(w, st) {
        return;
    }
    let seat = w.demigods[by.ix()].seat;
    let Some(kind) = kind_for(&format!("god_{slug}"), None) else { return };
    note(w, beat, st, kind, at, 8, 800.0, TIER_GOD, seat, CAUSE_GOD);
    match slug {
        "nature" => note(w, beat, st, SWARM, at, 6, 400.0, TIER_GOD, seat, CAUSE_GOD),
        "chance" => note(w, beat, st, LIGHTNING, at, 8, 800.0, TIER_GOD, seat, CAUSE_GOD),
        "entropy" => {
            let g = grid();
            for k in 0..2u64 {
                let d = super::grid::mix(w.meta.seed ^ ((beat as u64) << 16) ^ (k + 0xC4A05));
                let fc = g.fault_cells[d as usize % g.fault_cells.len()];
                note(w, beat, st, if k == 0 { VOLCANO } else { EARTHQUAKE }, g.center[fc as usize], 7, 400.0, TIER_GOD, seat, CAUSE_GOD);
            }
        }
        _ => {}
    }
}

/// A god-tier power whose god did not refuse (from the resolve, where its signature lands): aimed where the
/// signature is.
pub fn note_god_ruling(w: &mut World, beat: u32, st: &Settings, pantheon: &Pantheon, ruling: &Ruling, intent: &Intent) {
    if !on(w, st) {
        return;
    }
    let Some(def) = pantheon.get(ruling.god) else { return };
    let d = intent.demigod;
    let target = intent.targets.iter().find_map(|t| match t {
        Target::Known(r) => Some(*r),
        _ => None,
    });
    let at = target
        .and_then(|r| pos_of(w, r))
        .or_else(|| intent.place.and_then(|p| w.places.get(p.ix()).map(|x| x.center)))
        .unwrap_or(w.demigods[d.ix()].pos);
    let slug = def.slug.clone();
    note_god(w, beat, st, &slug, at, d);
}

/// What the beat's step produced, for the public payload.
pub struct EarthBeat {
    pub before: Sparse,
    pub sim: Sim,
    pub report: Report,
    /// The memory entries written (ids).
    pub entries: Vec<u32>,
    pub damaged: u32,
    pub micros: u64,
}

impl EarthBeat {
    /// The field as it stood before this beat's step (for the beat's delta).
    pub fn before_sim(&self) -> Sim {
        let mut s = EarthState::default();
        s.cells = super::codec::pack(&[], &self.before);
        Sim::from_state(self.sim.seed, self.sim.cfg, &s)
    }
}

/// One beat of the Earth: log this beat's physical ramifications, step the field, keep a keyframe when one is due,
/// and apply the consequences. Call after the society and before the DM's step.
pub fn step(w: &mut World, beat: u32, pantheon: &Pantheon, st: &Settings) -> Option<EarthBeat> {
    if !on(w, st) {
        return None;
    }
    // Entropy's ramifications that are physical: each world mod born this beat whose name or tags name a phenomenon.
    let mods: Vec<(u8, LonLat, Option<DemigodId>)> = w
        .mods
        .iter()
        .filter(|m| m.since_beat == beat && m.stacks <= 1)
        .filter_map(|m| kind_from_words(&format!("{} {}", m.name, m.tags.join(" "))).map(|k| (k, m.at, m.by)))
        .collect();
    for (kind, at, by) in mods {
        let seat = by.and_then(|d| w.demigods.get(d.ix())).map(|d| d.seat).unwrap_or(NOBODY);
        note(w, beat, st, kind, at, 3, 0.0, TIER_MINI, seat, CAUSE_RAMIFICATION);
    }
    let seed = w.meta.seed;
    let c = cfg(st);
    let entropy = w.meta.entropy.round().clamp(0.0, 100.0) as u8;
    let started = std::time::Instant::now();
    let (sim, report, before) = Sim::advance(seed, c, state_mut(w), beat, entropy);
    let micros = started.elapsed().as_micros() as u64;
    let mut eb = EarthBeat { before, sim, report, entries: vec![], damaged: 0, micros };
    consequences(w, beat, pantheon, st, &mut eb);
    let r = &eb.report;
    event("earth_step", None, json!({
        "beat": beat, "entropy": entropy, "hash": format!("{:016x}", eb.sim.hash()), "cells": eb.sim.list.len(),
        "phenomena": eb.sim.phen.len(), "spawned": r.spawned.len(), "ended": r.ended.len(), "substeps": r.substeps,
        "spontaneous": r.spontaneous, "evicted": r.evicted.len(), "damaged": eb.damaged, "entries": eb.entries.len(),
    }));
    Some(eb)
}

// ---------------------------------------------------------------------------------------------------------------------
// Consequences
// ---------------------------------------------------------------------------------------------------------------------

/// What stands in a cell this beat, 0..=1 each.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Hazard {
    pub fire: f32,
    pub lava: f32,
    pub flood: f32,
    pub quake: f32,
    pub strike: f32,
    pub cold: f32,
    pub swarm: f32,
}

impl Hazard {
    pub fn worst(&self) -> f32 {
        [self.fire, self.lava, self.flood, self.quake, self.strike, self.cold, self.swarm].into_iter().fold(0.0, f32::max)
    }
}

/// The hazards of cell `i` after this beat's step.
pub fn hazard(sim: &Sim, rep: &Report, i: u16) -> Hazard {
    let c = sim.c(i);
    let sea = sim.is_sea(i);
    let mut h = Hazard::default();
    if c.fire >= 32 {
        h.fire = c.fire as f32 / 255.0;
    }
    if c.lava >= 8 {
        h.lava = (c.lava as f32 / 128.0).min(1.0);
    }
    if !sea && c.water >= 48 {
        h.flood = c.water as f32 / 255.0;
    }
    if rep.drowned.contains(&i) {
        h.flood = 1.0;
    } else if rep.wave_hits.contains(&i) {
        h.flood = (h.flood + 0.5).min(1.0);
    }
    if let Some(s) = rep_shake(rep, i) {
        h.quake = s as f32 / 255.0;
    }
    if rep.strikes.contains(&i) {
        h.strike = 1.0;
    }
    if c.snow >= 96 && c.heat <= -32 {
        h.cold = 1.0;
    }
    if c.swarm >= 64 {
        h.swarm = c.swarm as f32 / 255.0;
    }
    h
}

/// The hardest a cell shook this beat (the field's `shake` layer has already begun to settle when the step returns).
fn rep_shake(rep: &Report, i: u16) -> Option<u8> {
    rep.shaken.iter().filter(|x| x.0 == i).map(|x| x.1).max()
}

fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

fn rules_prop(effect: Effect, act: String, actor: Ref, magnitude: u8, valence: i8, witnesses: Vec<Ref>) -> Proposal {
    Proposal { source: Source::Rules, intent: None, effect, act, actor, magnitude, valence, witnesses, cause: None }
}

fn consequences(w: &mut World, beat: u32, pantheon: &Pantheon, st: &Settings, eb: &mut EarthBeat) {
    let g = grid();
    let (sim, rep) = (&eb.sim, &eb.report);
    let mut props = vec![];
    // ---- demigods standing in it: one bounded hit, named for its worst part (so gear that resists it applies) -------
    for d in w.demigods.iter().filter(|d| d.alive) {
        let i = g.index_of(d.pos);
        if i == NONE {
            continue;
        }
        let h = hazard(sim, rep, i);
        let parts = [
            (h.fire * st.earth_fire_damage, "burned by the wildfire"),
            (h.lava * st.earth_lava_damage, "caught in the lava"),
            (h.flood * st.earth_flood_damage, "swept up in the flood"),
            (h.quake * st.earth_quake_damage, "thrown down by the quake"),
            (h.strike * st.earth_storm_damage, "struck by lightning from the storm"),
            (h.cold * st.earth_cold_damage, "frozen in the blizzard"),
            (h.swarm * st.earth_swarm_damage, "bitten by the swarm"),
        ];
        let total: f32 = parts.iter().map(|p| p.0).sum();
        let amount = round2(total.min(st.earth_damage_cap) * st.earth_damage_scale);
        if amount < 0.05 {
            continue;
        }
        let cause = parts.iter().fold((0.0f32, ""), |a, p| if p.0 > a.0 { *p } else { a }).1;
        let region = locate(w, d.pos, HexLevel::Regions).region;
        props.push(rules_prop(
            Effect::Damage { who: Ref::Demigod(d.id), amount, cause: cause.into() },
            format!("{} is {cause}", d.name),
            Ref::Place(region),
            if amount >= 0.5 { 4 } else { 3 },
            -2,
            vec![Ref::Demigod(d.id)],
        ));
    }
    eb.damaged = props.len() as u32;
    // ---- crowds: the worst-hit fear the gods (and some die), every other beat each ---------------------------------
    if st.earth_crowd_effects > 0 && st.earth_crowd_scale > 0.0 {
        let mut hit: Vec<(f32, ClusterId, Hazard)> = w
            .clusters
            .iter()
            .filter(|c| c.army.is_none() && c.size > 0 && (beat + c.id.0).is_multiple_of(2))
            .filter_map(|c| {
                let i = g.index_of(pos_of(w, Ref::Cluster(c.id))?);
                if i == NONE || sim.c(i).is_base() {
                    return None;
                }
                let h = hazard(sim, rep, i);
                (h.worst() >= 0.25).then_some((h.worst(), c.id, h))
            })
            .collect();
        hit.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for (s, c, h) in hit.into_iter().take(st.earth_crowd_effects as usize) {
            let k = s * st.earth_crowd_scale;
            let what = if h.lava >= s { "the lava" } else if h.fire >= s { "the fire" } else if h.flood >= s { "the flood" }
                       else if h.quake >= s { "the quake" } else if h.strike >= s { "the lightning" } else if h.cold >= s { "the blizzard" }
                       else { "the swarm" };
            let name = w.clusters[c.ix()].label.clone();
            let region = Ref::Place(fn7_society::geo::district_of(w, w.clusters[c.ix()].place));
            props.push(rules_prop(
                Effect::MoodShift { cluster: c, anger: round2(0.05 * k), fear: round2(0.15 * k), hope: round2(-0.05 * k), devotion: round2(0.05 * k) },
                format!("{name} cower before {what}"),
                region,
                3,
                -1,
                vec![Ref::Cluster(c)],
            ));
            if s >= 0.5 {
                props.push(rules_prop(
                    Effect::Damage { who: Ref::Cluster(c), amount: round2((2.0 * k).min(5.0)), cause: what.trim_start_matches("the ").into() },
                    format!("{what} takes its toll on {name}"),
                    region,
                    4,
                    -2,
                    vec![Ref::Cluster(c)],
                ));
            }
        }
    }
    if !props.is_empty() {
        crate::merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
    }
    // ---- roads: a rift, lava, deep water or the sea over either end cuts the road (it fades by the rules' own step) --
    let blocked = |at: LonLat| {
        let i = g.index_of(at);
        if i == NONE {
            return false;
        }
        let c = sim.c(i);
        c.crack >= 128 || c.lava >= 32 || c.water >= 96 || (sim.is_sea(i) && g.h[i as usize] >= SEA_LEVEL)
    };
    let threshold = st.dm_road_wear.max(0.1);
    let mut cut = vec![];
    for (k, t) in w.dm.trails.iter().enumerate() {
        if t.road_since.is_some() && t.wear >= threshold / 2.0 && (blocked(w.places[t.a.ix()].center) || blocked(w.places[t.b.ix()].center)) {
            cut.push(k);
        }
    }
    let mut budget = st.earth_events_per_beat as usize;
    for k in cut {
        let (a, b) = (w.dm.trails[k].a, w.dm.trails[k].b);
        w.dm.trails[k].wear = (threshold / 2.0 - 0.01).max(0.0);
        w.dm.trails[k].last_used = 0;
        if budget > 0 {
            budget -= 1;
            let at = w.places[a.ix()].center;
            let text = format!("the road between {} and {} is cut", w.name_of(Ref::Place(a)), w.name_of(Ref::Place(b)));
            eb.entries.push(remember(w, beat, &locate(w, at, HexLevel::Regions), Decision {
                scope: Scope::Region, kind: "earth.road_cut", query: None, public: true, text,
                data: json!({ "from_ref": Ref::Place(a), "to_ref": Ref::Place(b), "at": at }) }));
        }
    }
    // ---- shrines: hard shaking, lava, a strike from the sky or the sea topples one (half its worshippers leave) -----
    for k in 0..w.dm.shrines.len() {
        let sh = &w.dm.shrines[k];
        let i = g.index_of(sh.at);
        if i == NONE || sh.worshippers == 0 {
            continue;
        }
        let c = sim.c(i);
        let fell = rep_shake(rep, i).is_some_and(|s| s >= 160) || c.lava >= 32 || rep.drowned.contains(&i) || rep.impacts.contains(&i);
        if !fell {
            continue;
        }
        let (id, name, at, left) = (sh.id, sh.name.clone(), sh.at, sh.worshippers / 2);
        w.dm.shrines[k].worshippers = left;
        if budget > 0 {
            budget -= 1;
            eb.entries.push(remember(w, beat, &locate(w, at, HexLevel::People), Decision {
                scope: Scope::Cell, kind: "earth.shrine_toppled", query: None, public: true,
                text: format!("{name} is thrown down; half its worshippers scatter"),
                data: json!({ "id": id, "name": name, "at": at, "worshippers": left }) }));
        }
    }
    // ---- the memory: place × time ----------------------------------------------------------------------------------
    let name_at = |w: &World, i: u16| {
        let wh = locate(w, g.center[i as usize], HexLevel::Regions);
        (w.name_of(Ref::Place(wh.region)), wh)
    };
    let keys = |cells: &[u16]| cells.iter().map(|&i| g.key(i)).collect::<Vec<_>>();
    let mut facts: Vec<(&'static str, u16, String, Value)> = vec![];
    // One entry per region for what happened to several cells at once.
    let by_region = |w: &World, cells: &[u16]| {
        let mut m: BTreeMap<PlaceId, Vec<u16>> = BTreeMap::new();
        for &i in cells {
            let r = locate(w, g.center[i as usize], HexLevel::Regions).region;
            let v = m.entry(r).or_default();
            if !v.contains(&i) {
                v.push(i);
            }
        }
        m
    };
    for (r, cells) in by_region(w, &rep.drowned) {
        let name = w.name_of(Ref::Place(r));
        facts.push(("earth.drowned", cells[0], format!("the coast of {name} drowned at beat {beat}"), json!({ "cells": keys(&cells) })));
    }
    for (r, cells) in by_region(w, &rep.land_rose) {
        let name = w.name_of(Ref::Place(r));
        facts.push(("earth.land_rose", cells[0], format!("new land rose from the sea off {name}: ground a demigod can stand on"),
                    json!({ "cells": keys(&cells), "at": g.center[cells[0] as usize] })));
    }
    for &(id, c) in &rep.plate_moves {
        let (name, _) = name_at(w, c);
        facts.push(("earth.plate_moved", c, format!("the plate under {name} lurched a cell: a rift behind it, a ridge ahead"),
                    json!({ "phenomenon": id, "cell": g.key(c) })));
    }
    for &c in &rep.impacts {
        let (name, _) = name_at(w, c);
        facts.push(("earth.impact", c, format!("a meteor struck {name} and left a crater"), json!({ "cell": g.key(c) })));
    }
    for &(id, c, n) in &rep.ruptures {
        let (name, _) = name_at(w, c);
        let p = sim.phen.iter().find(|p| p.id == id);
        if p.is_some_and(|p| p.tier == TIER_MINI) {
            continue;
        }
        let text = if n > 1 { format!("the fault under {name} ruptured along {n} cells") } else { format!("the ground shook under {name}, far from any fault") };
        facts.push(("earth.quake", c, text, json!({ "phenomenon": id, "cell": g.key(c), "cells": n })));
    }
    let mut seen = vec![];
    for &c in &rep.eruptions {
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        let Some(p) = sim.phen.iter().find(|p| p.kind == VOLCANO && p.cell == c) else { continue };
        if p.tier == TIER_MINI {
            continue;
        }
        let (name, _) = name_at(w, c);
        let text = if rep.woke.contains(&p.id) { format!("the quake woke the volcano in {name}") } else { format!("a volcano broke open in {name}") };
        facts.push(("earth.eruption", c, text, json!({ "phenomenon": p.id, "cell": g.key(c), "woken": rep.woke.contains(&p.id) })));
    }
    for (r, cells) in by_region(w, &rep.wave_hits) {
        let name = w.name_of(Ref::Place(r));
        facts.push(("earth.wave", cells[0], format!("a tidal wave broke over the coast of {name}"), json!({ "cells": keys(&cells) })));
    }
    // The world's own (Entropy's) phenomena and the ones an interaction set off.
    for &id in &rep.spawned {
        let Some(p) = sim.phen.iter().find(|p| p.id == id) else { continue };
        if !matches!(p.cause, CAUSE_ENTROPY | CAUSE_INTERACTION) || matches!(p.kind, EARTHQUAKE | VOLCANO) {
            continue;
        }
        let (name, _) = name_at(w, p.cell);
        let what = match p.kind {
            TIDAL_WAVE => "a tidal wave set out from",
            PLATE_DRIFT => "the plate began to move under",
            STORM => "a freak storm gathered over",
            BLIZZARD => "a freak blizzard fell on",
            LIGHTNING => "lightning fell from a clear sky on",
            _ => "the world stirred in",
        };
        facts.push(("earth.omen", p.cell, format!("{what} {name}"),
                    json!({ "phenomenon": id, "kind": kind_name(p.kind), "cell": g.key(p.cell), "cause": CAUSES[p.cause.min(4) as usize] })));
    }
    for (kind, cell, text, mut data) in facts {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let (_, wh) = name_at(w, cell);
        data["at"] = json!(g.center[cell as usize]);
        eb.entries.push(remember(w, beat, &wh, Decision { scope: Scope::Region, kind, query: None, public: true, text, data }));
    }
    // The cap: what settled back to base is remembered (always: this is the record that those scars existed).
    if !rep.evicted.is_empty() {
        let n = rep.evicted.len();
        let first = rep.evicted[0];
        let (name, wh) = name_at(w, first);
        eb.entries.push(remember(w, beat, &wh, Decision {
            scope: Scope::World, kind: "earth.settled", query: None, public: true,
            text: format!("{n} quiet scars healed over and the land settled back (the first near {name})"),
            data: json!({ "cells": n, "sample": keys(&rep.evicted[..n.min(8)]) }) }));
    }
    // ---- encounters: a rupture, a crater or new land uncovers something (one a beat at most) -----------------------
    let spot = rep.land_rose.first().or(rep.impacts.first()).copied()
        .or(rep.ruptures.iter().find(|r| r.2 > 1).map(|r| r.1));
    if let Some(i) = spot.filter(|&i| !sim.is_sea(i)) {
        let why = if rep.land_rose.contains(&i) { "the risen land" } else if rep.impacts.contains(&i) { "the crater" } else { "the rift" };
        if let Some(id) = crate::dm::uncover_encounter(w, beat, g.center[i as usize], why, st) {
            eb.entries.push(id);
        }
    }
}

/// How many levels a settlement at `center` is pushed down by what its cell has suffered: one per 96 points of ruin
/// (× `earth_ruin_scale`), three if the sea has taken it. The rules' own stepping does the rest (one level at a time).
pub fn ruin_levels(sim: &Sim, st: &Settings, center: LonLat) -> i32 {
    let g = grid();
    let i = g.index_of(center);
    if i == NONE {
        return 0;
    }
    if sim.is_sea(i) && g.h[i as usize] >= SEA_LEVEL {
        return 3;
    }
    ((sim.c(i).ruin as f32 * st.earth_ruin_scale / 96.0).floor() as i32).clamp(0, 3)
}
