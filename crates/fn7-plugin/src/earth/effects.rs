//! What the earth did, thing by thing (state schema 20; docs/EARTH_SIM.md §13).
//!
//! > "if i drop a flood on someone i need to see it, if i set wildfire to forrests i wanna see them burn down and have
//! > nothing left or however much was affected, if i toss a hurricane at a village people should be running to shelter
//! > and shelters should get destroyed" (Benito, BUILDLOG)
//!
//! Each beat, after the field has stepped and its consequences have gone through the rules, this names every thing the
//! earth touched, at the level the thing lives at (houses for buildings, people for persons and demigods, villages for
//! settlements and crowds, regions for forests, roads and armies), with the phenomenon that did it: forests burned cell
//! by cell, crowds fleeing to the nearest standing shelter, shelters destroyed with the people in them, deaths with
//! counts, buildings and settlements ruined, roads cut, shrines toppled, demigods and people hurt.
//!
//! What changes the world lives in the state (`EarthState::refuges`, `wrecks`), so time travel shows it; the list of
//! effects is kept per beat too (packed, capped, worst first), for `game.earth_at`. Everything here is integer
//! thresholds and seeded hashes (no draw from a shared stream), so the same beat names the same effects.

use std::collections::{BTreeMap, BTreeSet};

use fn7_model::earth::{EarthEffects, EarthState, Refuge, Wreck};
use fn7_model::hexgrid::{cell_at, HexCell, HexLevel};
use fn7_model::seed::hash_str;
use fn7_model::*;
use fn7_society::geo::{city_of, distance_km};
use serde_json::{json, Map, Value};

use super::codec::{b64, un64};
use super::grid::{grid, mix, NONE, SEA_LEVEL};
use super::sim::*;
use super::view::fine;
use crate::settings::Settings;

/// What happened to the thing (the order of `EarthEffects::counts`).
pub const WHATS: [&str; 10] = ["burned", "flooded", "destroyed", "damaged", "fled", "died", "sheltered", "buried", "swept", "returned"];
pub const W_BURNED: u8 = 0;
pub const W_FLOODED: u8 = 1;
pub const W_DESTROYED: u8 = 2;
pub const W_DAMAGED: u8 = 3;
pub const W_FLED: u8 = 4;
pub const W_DIED: u8 = 5;
pub const W_SHELTERED: u8 = 6;
pub const W_BURIED: u8 = 7;
pub const W_SWEPT: u8 = 8;
pub const W_RETURNED: u8 = 9;

/// What kind of thing it happened to.
pub const THINGS: [&str; 9] = ["building", "forest", "crowd", "road", "shrine", "demigod", "person", "army", "settlement"];
pub const K_BUILDING: u8 = 0;
pub const K_FOREST: u8 = 1;
pub const K_CROWD: u8 = 2;
pub const K_ROAD: u8 = 3;
pub const K_SHRINE: u8 = 4;
pub const K_DEMIGOD: u8 = 5;
pub const K_PERSON: u8 = 6;
pub const K_ARMY: u8 = 7;
pub const K_SETTLEMENT: u8 = 8;

/// A cell's hazard at which crowds flee or take shelter (0..=255).
pub const MILD: u32 = 64;
/// A hazard at which a shelter can be destroyed: floodwater over a man's head (160), a fire of 160, an open rift or
/// shaking that topples shrines (160+), a storm of power 80 at the cell.
pub const STRONG: u32 = 160;
/// A forest is a cell with at least this much vegetation; it has burned when the fire takes it under.
pub const FOREST_FUEL: u8 = 64;
/// How many beats the dead lie where they fell (`scars.fallen`).
pub const FALLEN_BEATS: u32 = 12;
/// People one building holds: this × (1 + its settlement's level).
pub const SHELTER_PER_LEVEL: u32 = 250;
/// The most buildings one group of fleeing people spreads over in a beat.
pub const SHELTER_SPLIT: usize = 6;
/// How far people go for a shelter (km).
pub const SHELTER_KM: u32 = 2500;
/// How long a phenomenon that ended still owns what it left (a fire that spread on, a flood draining), in beats.
pub const ENDED_BEATS: u32 = 32;
/// The most people (characters) the field hurts in one beat.
pub const PERSONS_MAX: usize = 16;

/// The kind of hazard a cell holds: people run from the first four and take cover from the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hz {
    None,
    Fire,
    Lava,
    Flood,
    Quake,
    Storm,
    Cold,
    Strike,
    Swarm,
}

impl Hz {
    /// People run from it (else they take cover where they are).
    pub fn flee(self) -> bool {
        matches!(self, Hz::Fire | Hz::Lava | Hz::Flood | Hz::Quake | Hz::None)
    }
    /// The phenomena that make it, the likeliest first.
    fn kinds(self) -> &'static [u8] {
        match self {
            Hz::Fire => &[WILDFIRE, LIGHTNING, METEOR, VOLCANO],
            Hz::Lava => &[VOLCANO, PLATE_DRIFT],
            Hz::Flood => &[FLOOD, TIDAL_WAVE, STORM, SINK_LAND, METEOR, BLIZZARD],
            Hz::Quake => &[EARTHQUAKE, METEOR, PLATE_DRIFT, VOLCANO, SINK_LAND, RAISE_LAND],
            Hz::Storm => &[STORM, BLIZZARD],
            Hz::Cold => &[BLIZZARD, STORM],
            Hz::Strike => &[LIGHTNING, STORM],
            Hz::Swarm => &[SWARM],
            Hz::None => &[],
        }
    }
    /// How a building falls to it.
    fn wreck(self) -> u8 {
        match self {
            Hz::Lava => W_BURIED,
            Hz::Flood => W_SWEPT,
            Hz::Fire => W_BURNED,
            _ => W_DESTROYED,
        }
    }
}

/// The phenomenon behind an effect: its id (0: none live), kind, the cast's magnitude (0: not a cast) and seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cause {
    pub id: u32,
    pub kind: u8,
    pub power: u8,
    pub seat: u8,
}

const NO_CAUSE: Cause = Cause { id: 0, kind: 0, power: 0, seat: NOBODY };

/// A thing's ref: 0 none, 1 crowd/army (`cl`), 2 demigod (`dg`), 3 person (`ch`), 4 place (`pl`), 5 shrine, 6 road.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub t: u8,
    pub a: u32,
    pub b: u32,
}

impl Tag {
    const NONE: Tag = Tag { t: 0, a: 0, b: 0 };
    fn of(r: Ref) -> Tag {
        match r {
            Ref::Cluster(c) => Tag { t: 1, a: c.0, b: 0 },
            Ref::Demigod(d) => Tag { t: 2, a: d.0, b: 0 },
            Ref::Char(c) => Tag { t: 3, a: c.0, b: 0 },
            Ref::Place(p) => Tag { t: 4, a: p.0, b: 0 },
            _ => Tag::NONE,
        }
    }
    fn json(&self) -> Option<String> {
        Some(match self.t {
            1 => format!("cl:{}", self.a),
            2 => format!("dg:{}", self.a),
            3 => format!("ch:{}", self.a),
            4 => format!("pl:{}", self.a),
            5 => format!("shrine:{}", self.a),
            6 => format!("pl:{}~pl:{}", self.a, self.b),
            _ => return None,
        })
    }
}

/// One effect.
#[derive(Debug, Clone, PartialEq)]
pub struct Fx {
    pub what: u8,
    pub thing: u8,
    pub cell: HexCell,
    pub to: Option<HexCell>,
    /// People (0: not a count).
    pub count: u32,
    /// Damage in hundredths (0: none).
    pub amount_c: u32,
    pub tag: Tag,
    pub cause: Cause,
    /// For the order (worst first); not kept.
    pub score: u32,
}

impl Fx {
    pub fn json(&self) -> Value {
        let mut v = json!({ "cell": self.cell.key(), "level": self.cell.level.as_str(),
                            "thing": { "kind": THINGS[self.thing as usize % THINGS.len()] },
                            "what": WHATS[self.what as usize % WHATS.len()], "cause": self.cause.id });
        if let Some(r) = self.tag.json() {
            v["thing"]["ref"] = r.into();
        }
        if let Some(t) = &self.to {
            v["to_cell"] = t.key().into();
        }
        if self.count > 0 {
            v["count"] = self.count.into();
        }
        if self.amount_c > 0 {
            v["amount"] = json!(self.amount_c as f64 / 100.0);
        }
        if self.cause.id > 0 {
            v["phenomenon"] = kind_name(self.cause.kind).into();
        }
        if self.cause.power > 0 {
            v["power"] = self.cause.power.into();
        }
        if self.cause.seat != NOBODY {
            v["seat"] = self.cause.seat.into();
        }
        v
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Packing (one beat's kept effects in the state)
// ---------------------------------------------------------------------------------------------------------------------

fn level_ix(l: HexLevel) -> u8 {
    match l {
        HexLevel::Regions => 0,
        HexLevel::Villages => 1,
        HexLevel::Houses => 2,
        HexLevel::People => 3,
    }
}

const LEVELS: [HexLevel; 4] = [HexLevel::Regions, HexLevel::Villages, HexLevel::Houses, HexLevel::People];

fn put(out: &mut Vec<u8>, mut x: u64) {
    loop {
        let b = (x & 0x7F) as u8;
        x >>= 7;
        if x == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn zz(x: i64) -> u64 {
    ((x << 1) ^ (x >> 63)) as u64
}

fn unzz(x: u64) -> i64 {
    ((x >> 1) as i64) ^ -((x & 1) as i64)
}

struct Rd<'a> {
    b: &'a [u8],
    k: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> Option<u8> {
        let x = *self.b.get(self.k)?;
        self.k += 1;
        Some(x)
    }
    fn var(&mut self) -> Option<u64> {
        let mut x = 0u64;
        for s in 0..10 {
            let b = self.u8()?;
            x |= ((b & 0x7F) as u64) << (7 * s);
            if b & 0x80 == 0 {
                return Some(x);
            }
        }
        None
    }
}

fn put_cell(out: &mut Vec<u8>, c: &HexCell) {
    out.push(c.face);
    put(out, zz(c.q));
    put(out, zz(c.r));
}

fn get_cell(r: &mut Rd, level: HexLevel) -> Option<HexCell> {
    let face = r.u8()?;
    let q = unzz(r.var()?);
    let rr = unzz(r.var()?);
    Some(HexCell { face, level, q, r: rr })
}

/// Pack effects: per row `what, thing, levels (cell | to << 2 | has_to << 4), cell, [to], count, amount, tag, [a],
/// [b], cause, kind, power, seat` (varints LEB128, q and r zigzag), base64.
pub fn pack(rows: &[Fx]) -> String {
    let mut out = vec![];
    for f in rows {
        out.push(f.what);
        out.push(f.thing);
        let lv = level_ix(f.cell.level) | f.to.map(|t| level_ix(t.level) << 2 | 16).unwrap_or(0);
        out.push(lv);
        put_cell(&mut out, &f.cell);
        if let Some(t) = &f.to {
            put_cell(&mut out, t);
        }
        put(&mut out, f.count as u64);
        put(&mut out, f.amount_c as u64);
        out.push(f.tag.t);
        if f.tag.t != 0 {
            put(&mut out, f.tag.a as u64);
        }
        if f.tag.t == 6 {
            put(&mut out, f.tag.b as u64);
        }
        put(&mut out, f.cause.id as u64);
        out.extend([f.cause.kind, f.cause.power, f.cause.seat]);
    }
    b64(&out)
}

pub fn unpack(s: &str) -> Vec<Fx> {
    let bytes = un64(s);
    let mut r = Rd { b: &bytes, k: 0 };
    let mut out = vec![];
    let one = |r: &mut Rd| -> Option<Fx> {
        let what = r.u8()?;
        let thing = r.u8()?;
        let lv = r.u8()?;
        let cell = get_cell(r, LEVELS[(lv & 3) as usize])?;
        let to = if lv & 16 != 0 { Some(get_cell(r, LEVELS[((lv >> 2) & 3) as usize])?) } else { None };
        let count = r.var()? as u32;
        let amount_c = r.var()? as u32;
        let t = r.u8()?;
        let a = if t != 0 { r.var()? as u32 } else { 0 };
        let b = if t == 6 { r.var()? as u32 } else { 0 };
        let id = r.var()? as u32;
        let (kind, power, seat) = (r.u8()?, r.u8()?, r.u8()?);
        Some(Fx { what, thing, cell, to, count, amount_c, tag: Tag { t, a, b }, cause: Cause { id, kind, power, seat }, score: 0 })
    };
    while r.k < bytes.len() {
        match one(&mut r) {
            Some(f) => out.push(f),
            None => break,
        }
    }
    out
}

/// The kept effects of one beat as the client gets them: `{effects, effects_total, effects_counts}`.
pub fn beat_fields(rows: &[Fx], total: u32, counts: &[u32]) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("effects".into(), rows.iter().map(Fx::json).collect::<Vec<_>>().into());
    m.insert("effects_total".into(), total.into());
    let c: Map<String, Value> = WHATS.iter().zip(counts.iter()).filter(|x| *x.1 > 0).map(|(w, n)| ((*w).to_string(), (*n).into())).collect();
    m.insert("effects_counts".into(), c.into());
    m
}

/// A stored beat's effects as the client gets them (an empty list for a beat with none).
pub fn stored_fields(s: &EarthState, beat: u32) -> Map<String, Value> {
    match s.effects.iter().find(|e| e.beat == beat) {
        Some(e) => beat_fields(&unpack(&e.rows), e.total, &e.counts),
        None => beat_fields(&[], 0, &[]),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The hazard at a cell, in integers
// ---------------------------------------------------------------------------------------------------------------------

/// The field after this beat's step, read for hazards.
pub struct Field<'a> {
    pub sim: &'a Sim,
    pub rep: &'a Report,
    /// A storm's (or blizzard's) power at each cell it covers, halving per ring.
    storms: BTreeMap<u16, u32>,
}

impl<'a> Field<'a> {
    pub fn new(sim: &'a Sim, rep: &'a Report) -> Field<'a> {
        let g = grid();
        let mut storms: BTreeMap<u16, u32> = BTreeMap::new();
        for p in sim.phen.iter().filter(|p| matches!(p.kind, STORM | BLIZZARD)) {
            for (j, d) in g.disc(p.cell, p.radius) {
                let v = (p.power.max(0) as u32) >> d;
                let e = storms.entry(j).or_default();
                *e = (*e).max(v);
            }
        }
        Field { sim, rep, storms }
    }

    /// How bad cell `i` is now (0..=255) and with what.
    pub fn at(&self, i: u16) -> (u32, Hz) {
        if i == NONE {
            return (0, Hz::None);
        }
        let (sim, rep) = (self.sim, self.rep);
        let c = sim.c(i);
        let sea = sim.is_sea(i);
        let mut best = (0u32, Hz::None);
        let mut see = |v: u32, h: Hz| {
            if v > best.0 {
                best = (v.min(255), h);
            }
        };
        if c.fire >= 32 {
            see(c.fire as u32, Hz::Fire);
        }
        if c.lava >= 8 {
            see(64 + c.lava as u32 * 2, Hz::Lava);
        }
        if !sea && c.water >= 48 {
            see(c.water as u32, Hz::Flood);
        }
        if rep.drowned.contains(&i) {
            see(255, Hz::Flood);
        } else if rep.wave_hits.contains(&i) {
            see(192, Hz::Flood);
        }
        if let Some(s) = rep.shaken.iter().filter(|x| x.0 == i).map(|x| x.1).max() {
            see(s as u32, Hz::Quake);
        }
        if c.crack >= 128 {
            see(200, Hz::Quake);
        }
        if let Some(&v) = self.storms.get(&i).filter(|v| **v >= 16) {
            see(v * 2, Hz::Storm);
        }
        if rep.strikes.contains(&i) {
            see(160, Hz::Strike);
        }
        if c.snow >= 96 && c.heat <= -32 {
            see(128, Hz::Cold);
        }
        if c.swarm >= 64 {
            see(c.swarm as u32, Hz::Swarm);
        }
        best
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The book of one beat's effects
// ---------------------------------------------------------------------------------------------------------------------

/// A settlement that can shelter people: its buildings (houses-level hexes, seeded by the match and the place).
pub struct Site {
    pub place: PlaceId,
    /// Its settlement level (2 before the DM has founded it).
    pub level: u8,
    pub center: LonLat,
    pub buildings: Vec<HexCell>,
}

/// The shelters of the match: every planet city's buildings, 3 + its level of them (at most 9), picked by seeded hash
/// among the houses-level hexes within two rings of its centre. A pure function of the world's places and levels.
pub fn sites(w: &World) -> Vec<Site> {
    w.places
        .iter()
        .filter(|p| p.kind == PlaceKind::Site)
        .map(|p| {
            let level = w.dm.settlement(p.id).map(|s| s.level).unwrap_or(2) as usize;
            let c0 = cell_at(p.center, HexLevel::Houses);
            let mut hexes: Vec<(u64, HexCell)> =
                (0..=2).flat_map(|k| c0.ring(k)).map(|c| (mix(w.meta.seed ^ hash_str(&c.key()) ^ 0x5E17), c)).collect();
            hexes.sort();
            Site { place: p.id, level: level as u8, center: p.center, buildings: hexes.into_iter().take((3 + level).min(9)).map(|x| x.1).collect() }
        })
        .collect()
}

/// One beat's effects as they are found.
pub struct Book {
    pub list: Vec<Fx>,
    seed: u64,
    beat: u32,
    /// Phenomena live now or ended this beat: (id, kind, cell, cast magnitude, seat).
    phen: Vec<(u32, u8, u16, u8, u8)>,
    /// The phenomena live after the step (ids).
    live: Vec<u32>,
    /// Crowd sizes and people's health before the beat's damage went through the rules.
    sizes: Vec<u32>,
    health: Vec<(CharId, f32)>,
    /// Demigods the field hurt: (who, amount, hazard, cell).
    demigods: Vec<(DemigodId, f32, Hz, u16)>,
    /// People the field hurt.
    persons: Vec<(CharId, f32, Hz, u16)>,
}

fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

impl Book {
    pub fn new(w: &World, beat: u32, sim: &Sim, rep: &Report) -> Book {
        let s = w.dm.earth.as_deref();
        let magnitude = |p: &fn7_model::earth::Phenomenon| -> u8 {
            if !matches!(p.cause, CAUSE_CAST | CAUSE_GOD) {
                return 0;
            }
            s.and_then(|s| s.inputs.iter().find(|i| i.beat == p.born && i.kind == p.kind && i.lon_e3 == p.lon_e3 && i.lat_e3 == p.lat_e3))
                .map(|i| i.energy)
                .unwrap_or(0)
        };
        let mut phen: Vec<(u32, u8, u16, u8, u8)> = sim.phen.iter().chain(rep.ended.iter()).map(|p| (p.id, p.kind, p.cell, magnitude(p), p.owner)).collect();
        // Those that ended in the last beats: what they left (a fire that spread on, a flood draining) is still theirs.
        if let Some(s) = s {
            phen.extend(s.ended.iter().filter(|e| e.5 + ENDED_BEATS > beat).map(|e| (e.0, e.1, e.2, e.3, e.4)));
        }
        Book { list: vec![], seed: w.meta.seed, beat, phen, live: sim.phen.iter().map(|p| p.id).collect(), sizes: w.clusters.iter().map(|c| c.size).collect(),
               health: w.characters.iter().filter(|c| c.alive).map(|c| (c.id, c.health)).collect(), demigods: vec![], persons: vec![] }
    }

    /// The phenomenon behind a hazard at cell `i`: the nearest live (or just ended) one of a kind that makes it.
    pub fn cause(&self, hz: Hz, i: u16) -> Cause {
        let g = grid();
        if i == NONE {
            return NO_CAUSE;
        }
        let at = g.center[i as usize];
        self.phen
            .iter()
            .filter(|p| hz.kinds().contains(&p.1))
            .min_by_key(|p| (distance_km(g.center[p.2 as usize], at) as u32, p.0))
            .map(|p| Cause { id: p.0, kind: p.1, power: p.3, seat: p.4 })
            .unwrap_or(NO_CAUSE)
    }

    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, what: u8, thing: u8, cell: HexCell, to: Option<HexCell>, count: u32, amount_c: u32, tag: Tag, cause: Cause) {
        let n = count.min(99_999);
        let score = match (what, thing) {
            (W_DIED, _) => 1_000_000 + n,
            (W_DESTROYED | W_BURIED | W_SWEPT, K_SETTLEMENT) => 700_000,
            (W_DESTROYED | W_BURIED | W_SWEPT | W_BURNED, K_SHRINE) => 650_000,
            (W_DESTROYED | W_BURIED | W_SWEPT, K_ROAD) => 620_000,
            (W_DESTROYED | W_BURIED | W_SWEPT | W_BURNED, K_BUILDING) => 600_000,
            (W_DAMAGED, K_DEMIGOD) => 500_000 + amount_c.min(9_999),
            (W_DAMAGED, K_PERSON) => 450_000 + amount_c.min(9_999),
            (W_FLED | W_SHELTERED, _) => 400_000 + n,
            (W_BURNED | W_BURIED, K_FOREST) => 300_000 + amount_c.min(9_999),
            (W_RETURNED, _) => 200_000 + n,
            (W_DAMAGED, _) => 150_000 + amount_c.min(9_999),
            _ => 100_000,
        };
        self.list.push(Fx { what, thing, cell, to, count, amount_c, tag, cause, score });
    }

    /// Record a demigod the field hurt (from the consequences' damage).
    pub fn demigod(&mut self, d: DemigodId, amount: f32, part: usize, i: u16) {
        const PARTS: [Hz; 7] = [Hz::Fire, Hz::Lava, Hz::Flood, Hz::Quake, Hz::Strike, Hz::Cold, Hz::Swarm];
        self.demigods.push((d, amount, PARTS[part.min(6)], i));
    }

    /// Schema 20's own damage through the rules: armies and people standing in a hazard.
    pub fn extra_props(&mut self, w: &World, f: &Field) -> Vec<Proposal> {
        let g = grid();
        let mut props = vec![];
        for c in w.clusters.iter().filter(|c| c.size > 0) {
            let Some(a) = &c.army else { continue };
            let (s, hz) = f.at(g.index_of(a.pos));
            if s < MILD {
                continue;
            }
            props.push(super::rules::rules_prop(
                Effect::Damage { who: Ref::Cluster(c.id), amount: round2(s as f32 * 4.0 / 255.0), cause: hz_word(hz).into() },
                format!("{} is caught in {}", c.label, hz_word(hz)),
                Ref::Cluster(c.id),
                3,
                -2,
                vec![Ref::Cluster(c.id)],
            ));
        }
        let mut hit: Vec<(u32, CharId, Hz, u16)> = w
            .characters
            .iter()
            .filter(|c| c.alive)
            .filter_map(|c| {
                let i = g.index_of(c.pos);
                let (s, hz) = f.at(i);
                (s >= MILD).then_some((s, c.id, hz, i))
            })
            .collect();
        hit.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (s, id, hz, i) in hit.into_iter().take(PERSONS_MAX) {
            let amount = round2(0.5 * s as f32 / 255.0);
            self.persons.push((id, amount, hz, i));
            let name = w.characters[id.ix()].name.clone();
            props.push(super::rules::rules_prop(
                Effect::Damage { who: Ref::Char(id), amount, cause: hz_word(hz).into() },
                format!("{name} is caught in {}", hz_word(hz)),
                Ref::Char(id),
                3,
                -2,
                vec![Ref::Char(id)],
            ));
        }
        props
    }

    /// After the damage went through the rules: who was hurt, who died, how many of each crowd and army.
    pub fn after_damage(&mut self, w: &World, f: &Field) {
        let g = grid();
        for (d, amount, hz, i) in std::mem::take(&mut self.demigods) {
            let dg = &w.demigods[d.ix()];
            let cell = cell_at(dg.pos, HexLevel::People);
            let cause = self.cause(hz, i);
            self.push(W_DAMAGED, K_DEMIGOD, cell, None, 0, (amount * 100.0).round() as u32, Tag::of(Ref::Demigod(d)), cause);
            if !dg.alive {
                self.push(W_DIED, K_DEMIGOD, cell, None, 1, 0, Tag::of(Ref::Demigod(d)), cause);
            }
        }
        for k in 0..self.sizes.len().min(w.clusters.len()) {
            let c = &w.clusters[k];
            let lost = self.sizes[k].saturating_sub(c.size);
            if lost == 0 {
                continue;
            }
            let (pos, thing, level) = match &c.army {
                Some(a) => (a.pos, K_ARMY, HexLevel::Regions),
                None => (w.places[c.place.ix()].center, K_CROWD, HexLevel::Villages),
            };
            let i = g.index_of(pos);
            let (_, hz) = f.at(i);
            let cause = self.cause(hz, i);
            self.push(W_DIED, thing, cell_at(pos, level), None, lost, 0, Tag::of(Ref::Cluster(c.id)), cause);
        }
        for (id, amount, hz, i) in std::mem::take(&mut self.persons) {
            let ch = &w.characters[id.ix()];
            let cell = cell_at(ch.pos, HexLevel::People);
            let cause = self.cause(hz, i);
            let was = self.health.iter().find(|x| x.0 == id).map(|x| x.1).unwrap_or(ch.health);
            let lost = (was - ch.health).max(0.0).min(amount.max(0.0));
            if lost > 0.0 {
                self.push(W_DAMAGED, K_PERSON, cell, None, 0, (lost * 100.0).round().max(1.0) as u32, Tag::of(Ref::Char(id)), cause);
            }
            if !ch.alive {
                self.push(W_DIED, K_PERSON, cell, None, 1, 0, Tag::of(Ref::Char(id)), cause);
            }
        }
    }

    /// Crowds to shelters, shelters destroyed, the people in them dying or fleeing again, people going home. Changes
    /// the state's refuges and wrecks; returns the deaths for the rules (`Shrink` per crowd).
    pub fn shelters(&mut self, w: &mut World, f: &Field, before: &Sim) -> Vec<Proposal> {
        let g = grid();
        let beat = self.beat;
        let sites = sites(w);
        let Some(mut state) = w.dm.earth.take() else { return vec![] };
        let mut wrecked: BTreeSet<String> = state.wrecks.iter().map(|x| x.at.clone()).collect();
        let mut deaths: BTreeMap<u32, u32> = BTreeMap::new();
        // ---- people go home when the hazard has left their home ---------------------------------------------------
        for k in 0..state.refuges.len() {
            let r = &state.refuges[k];
            if r.until != 0 || r.since >= beat {
                continue;
            }
            let Some(home) = HexCell::parse(&r.home) else { continue };
            if f.at(g.index_of(home.center())).0 >= MILD {
                continue;
            }
            let (at, count, crowd, cause_id) = (HexCell::parse(&r.at), r.count, r.crowd, r.cause);
            state.refuges[k].until = beat;
            if let Some(at) = at {
                let cause = self.phen.iter().find(|p| p.0 == cause_id).map(|p| Cause { id: p.0, kind: p.1, power: p.3, seat: p.4 })
                    .unwrap_or(Cause { id: cause_id, ..NO_CAUSE });
                self.push(W_RETURNED, K_CROWD, at, Some(home), count, 0, Tag { t: 1, a: crowd, b: 0 }, cause);
            }
        }
        // Who is in which building now.
        let mut occ: BTreeMap<String, u32> = BTreeMap::new();
        for r in state.refuges.iter().filter(|r| r.until == 0) {
            *occ.entry(r.at.clone()).or_default() += r.count;
        }
        // ---- shelters in a strong hazard can be destroyed; ruin past a building's own threshold destroys it ---------
        for site in &sites {
            for b in &site.buildings {
                let key = b.key();
                if wrecked.contains(&key) {
                    continue;
                }
                let i = g.index_of(b.center());
                if i == NONE || f.sim.is_sea(i) && before.is_sea(i) {
                    continue;
                }
                let (s, hz) = f.at(i);
                let h = mix(self.seed ^ hash_str(&key) ^ 0xB11D);
                let threshold = 96 + (h % 160) as u32;
                let (rb, ra) = (before.c(i).ruin as u32, f.sim.c(i).ruin as u32);
                let by_hazard = s >= STRONG && (mix(h ^ ((beat as u64) << 20) ^ 0xFA11) & 0xFFFF) < ((s - 128) * 256) as u64;
                let by_ruin = rb < threshold && ra >= threshold;
                let tag = Tag::of(Ref::Place(site.place));
                let cause = self.cause(hz, i);
                if by_hazard || by_ruin {
                    let how = hz.wreck();
                    wrecked.insert(key.clone());
                    occ.remove(&key);
                    state.wrecks.push(Wreck { at: key.clone(), place: site.place.0, beat, how: WHATS[how as usize].into(), cause: cause.id });
                    self.push(how, K_BUILDING, *b, None, 0, 0, tag, cause);
                    // The people sheltering in it: some die, the rest flee again (or scatter home).
                    for k in 0..state.refuges.len() {
                        let r = &state.refuges[k];
                        if r.until != 0 || r.at != key {
                            continue;
                        }
                        let r = r.clone();
                        state.refuges[k].until = beat;
                        let died = (r.count * s.max(128)).div_ceil(512).min(r.count);
                        let left = r.count - died;
                        let ctag = Tag { t: 1, a: r.crowd, b: 0 };
                        if died > 0 {
                            *deaths.entry(r.crowd).or_default() += died;
                            self.push(W_DIED, K_CROWD, *b, None, died, 0, ctag, cause);
                        }
                        if left == 0 {
                            continue;
                        }
                        let home_site = w.clusters.get(r.crowd as usize).and_then(|c| city_of(w, c.place));
                        let flee_hz = if hz.flee() { hz } else { Hz::Quake };
                        let spots = shelter_for(&sites, f, &wrecked, &mut occ, home_site, b.center(), flee_hz, left,
                                                mix(h ^ r.crowd as u64 ^ beat as u64));
                        let mut placed = 0;
                        for (p, to, n) in spots {
                            placed += n;
                            state.refuges.push(Refuge { crowd: r.crowd, count: n, home: r.home.clone(), at: to.key(), place: p.0,
                                                        since: beat, until: 0, cause: cause.id });
                            self.push(W_FLED, K_CROWD, *b, Some(to), n, 0, ctag, cause);
                            self.push(W_SHELTERED, K_CROWD, to, None, n, 0, ctag, cause);
                        }
                        // With no room anywhere near, the rest scatter home.
                        if left > placed {
                            self.push(W_FLED, K_CROWD, *b, HexCell::parse(&r.home), left - placed, 0, ctag, cause);
                        }
                    }
                } else {
                    if ra >= rb + 24 && ra >= 48 {
                        self.push(W_DAMAGED, K_BUILDING, *b, None, 0, ra * 100 / 255, tag, cause);
                    }
                    if !f.sim.is_sea(i) && fine(before, b, i).water == 0 && fine(f.sim, b, i).water > 0 {
                        let cause = self.cause(Hz::Flood, i);
                        self.push(W_FLOODED, K_BUILDING, *b, None, 0, 0, tag, cause);
                    }
                }
            }
        }
        // ---- crowds in a hazard: part of them run to the nearest standing shelter ---------------------------------
        for c in w.clusters.iter().filter(|c| c.army.is_none() && c.size > 0) {
            let home = w.places[c.place.ix()].center;
            let i = g.index_of(home);
            let (s, hz) = f.at(i);
            if s < MILD {
                continue;
            }
            let away: u32 = state.refuges.iter().filter(|r| r.until == 0 && r.crowd == c.id.0).map(|r| r.count).sum();
            let avail = c.size.saturating_sub(away + deaths.get(&c.id.0).copied().unwrap_or(0));
            if avail == 0 {
                continue;
            }
            let n = ((avail as u64 * s as u64 / 512) as u32).clamp(1, avail);
            let home_cell = cell_at(home, HexLevel::Villages);
            let spots = shelter_for(&sites, f, &wrecked, &mut occ, city_of(w, c.place), home, hz, n,
                                    mix(self.seed ^ c.id.0 as u64 ^ ((beat as u64) << 24)));
            let cause = self.cause(hz, i);
            let tag = Tag::of(Ref::Cluster(c.id));
            for (p, to, n) in spots {
                state.refuges.push(Refuge { crowd: c.id.0, count: n, home: home_cell.key(), at: to.key(), place: p.0, since: beat, until: 0,
                                            cause: cause.id });
                self.push(W_FLED, K_CROWD, home_cell, Some(to), n, 0, tag, cause);
                self.push(W_SHELTERED, K_CROWD, to, None, n, 0, tag, cause);
            }
        }
        w.dm.earth = Some(state);
        deaths
            .into_iter()
            .filter_map(|(crowd, n)| {
                let c = w.clusters.get(crowd as usize)?;
                let n = n.min(c.size);
                (n > 0).then(|| {
                    super::rules::rules_prop(Effect::Shrink { cluster: c.id, count: n },
                                             format!("{n} of {} die as their shelter falls", c.label), Ref::Cluster(c.id), 4, -2,
                                             vec![Ref::Cluster(c.id)])
                })
            })
            .collect()
    }

    /// A road the field cut (its blocked end at `at`).
    pub fn road(&mut self, f: &Field, a: PlaceId, b: PlaceId, at: LonLat) {
        let i = grid().index_of(at);
        let (_, hz) = f.at(i);
        let hz = if hz == Hz::None { blocked_by(f.sim, i) } else { hz };
        let cause = self.cause(hz, i);
        self.push(W_DESTROYED, K_ROAD, cell_at(at, HexLevel::Regions), None, 0, 0, Tag { t: 6, a: a.0, b: b.0 }, cause);
    }

    /// A shrine the field toppled.
    pub fn shrine(&mut self, f: &Field, id: u32, cell: &str, at: LonLat) {
        let i = grid().index_of(at);
        let (_, hz) = f.at(i);
        let hz = if hz == Hz::None { Hz::Quake } else { hz };
        let cause = self.cause(hz, i);
        let c = HexCell::parse(cell).unwrap_or_else(|| cell_at(at, HexLevel::People));
        self.push(W_DESTROYED, K_SHRINE, c, None, 0, 0, Tag { t: 5, a: id, b: 0 }, cause);
    }

    /// What the field itself shows changed: forests burned (or buried) cell by cell, settlements ruined or flooded.
    pub fn field(&mut self, w: &World, st: &Settings, f: &Field, before: &Sim) {
        let g = grid();
        let sim = f.sim;
        let mut cells: Vec<u16> = before.list.iter().chain(sim.list.iter()).copied().collect();
        cells.sort_unstable();
        cells.dedup();
        for i in cells {
            if before.is_sea(i) || sim.is_sea(i) {
                continue;
            }
            let (fb, fa) = (before.fuel(i), sim.fuel(i));
            if fb < FOREST_FUEL || fa >= FOREST_FUEL {
                continue;
            }
            let (cb, ca) = (before.c(i), sim.c(i));
            let (what, hz) = if ca.lava >= 8 || cb.lava >= 8 {
                (W_BURIED, Hz::Lava)
            } else if ca.fire > 0 || cb.fire > 0 {
                (W_BURNED, Hz::Fire)
            } else {
                continue;
            };
            let cause = self.cause(hz, i);
            // `amount`: the share of what stood there (at rest or grown) that is gone now (1 = nothing left).
            let stood = (fb as u32).max(g.fuel[i as usize] as u32).max(1);
            let gone = (stood - fa as u32) * 100 / stood;
            self.push(what, K_FOREST, g.cells[i as usize], None, 0, gone.max(1), Tag::NONE, cause);
        }
        for p in w.places.iter().filter(|p| p.kind == PlaceKind::Site) {
            let i = g.index_of(p.center);
            if i == NONE {
                continue;
            }
            let cell = cell_at(p.center, HexLevel::Villages);
            let (lb, la) = (super::rules::ruin_levels(before, st, p.center), super::rules::ruin_levels(sim, st, p.center));
            let tag = Tag::of(Ref::Place(p.id));
            if la > lb {
                let drowned = sim.is_sea(i) && g.h[i as usize] >= SEA_LEVEL;
                let (what, hz) = if drowned { (W_SWEPT, Hz::Flood) } else if la >= 3 { (W_DESTROYED, f.at(i).1) } else { (W_DAMAGED, f.at(i).1) };
                let cause = self.cause(if hz == Hz::None { Hz::Quake } else { hz }, i);
                self.push(what, K_SETTLEMENT, cell, None, 0, (la * 100 / 3) as u32, tag, cause);
            } else if !sim.is_sea(i) && fine(before, &cell, i).water == 0 && fine(sim, &cell, i).water > 0 {
                let cause = self.cause(Hz::Flood, i);
                self.push(W_FLOODED, K_SETTLEMENT, cell, None, 0, 0, tag, cause);
            }
        }
    }

    /// Worst first, capped; kept in the state. Returns what the beat's `earth` carries.
    pub fn finish(mut self, w: &mut World, st: &Settings) -> BeatFx {
        let total = self.list.len() as u32;
        let mut counts = vec![0u32; WHATS.len()];
        for f in &self.list {
            counts[f.what as usize] += 1;
        }
        // A stable sort: ties keep the order they were found in (deterministic).
        self.list.sort_by(|a, b| b.score.cmp(&a.score));
        self.list.truncate(st.earth_effects_max as usize);
        if let Some(s) = w.dm.earth.as_deref_mut() {
            if total > 0 {
                s.effects.push(EarthEffects { beat: self.beat, total, counts: counts.clone(), rows: pack(&self.list) });
            }
            // Remember what ended this beat (and forget what ended `ENDED_BEATS` ago).
            let beat = self.beat;
            s.ended.retain(|e| e.5 + ENDED_BEATS > beat);
            let live: BTreeSet<u32> = self.live.iter().copied().collect();
            let known: BTreeSet<u32> = s.ended.iter().map(|e| e.0).collect();
            for p in self.phen.iter().filter(|p| !live.contains(&p.0) && !known.contains(&p.0)) {
                s.ended.push((p.0, p.1, p.2, p.3, p.4, beat));
            }
        }
        // What the client sees is exactly what the state keeps (the amounts in hundredths).
        BeatFx { rows: self.list, total, counts }
    }
}

/// The effects a beat carries.
#[derive(Debug, Clone, Default)]
pub struct BeatFx {
    pub rows: Vec<Fx>,
    pub total: u32,
    pub counts: Vec<u32>,
}

fn hz_word(hz: Hz) -> &'static str {
    match hz {
        Hz::Fire => "the fire",
        Hz::Lava => "the lava",
        Hz::Flood => "the flood",
        Hz::Quake => "the quake",
        Hz::Storm => "the storm",
        Hz::Cold => "the blizzard",
        Hz::Strike => "the lightning",
        Hz::Swarm => "the swarm",
        Hz::None => "the earth",
    }
}

fn blocked_by(sim: &Sim, i: u16) -> Hz {
    if i == NONE {
        return Hz::None;
    }
    let c = sim.c(i);
    if c.lava >= 32 {
        Hz::Lava
    } else if c.crack >= 128 {
        Hz::Quake
    } else {
        Hz::Flood
    }
}

/// Where `n` people at `from` running from (or taking cover from) `hz` go: standing buildings with room, those of their
/// own city first, then of the nearest cities within [`SHELTER_KM`]; at most [`SHELTER_SPLIT`] buildings, each holding
/// [`SHELTER_PER_LEVEL`] × (1 + its city's level) people (`occ` is who is in each now; it is updated). From fire, lava,
/// flood or quake a building counts only out of it (its cell under [`MILD`], or under [`STRONG`] with its own
/// houses-level hex clear of fire, water, lava and rifts); from a storm, a blizzard, lightning or a swarm any standing
/// building is cover. Within a city the buildings are tried from one picked by `salt`. Those who find no room stay.
#[allow(clippy::too_many_arguments)]
fn shelter_for(sites: &[Site], f: &Field, wrecked: &BTreeSet<String>, occ: &mut BTreeMap<String, u32>, home: Option<PlaceId>,
               from: LonLat, hz: Hz, n: u32, salt: u64) -> Vec<(PlaceId, HexCell, u32)> {
    let g = grid();
    let mut order: Vec<(u32, u32, usize)> = sites
        .iter()
        .enumerate()
        .map(|(k, s)| (if Some(s.place) == home { 0 } else { distance_km(from, s.center) as u32 + 1 }, s.place.0, k))
        .filter(|x| x.0 <= SHELTER_KM)
        .collect();
    order.sort();
    let mut out = vec![];
    let mut left = n;
    for (_, _, k) in order {
        if left == 0 || out.len() >= SHELTER_SPLIT {
            break;
        }
        let site = &sites[k];
        let cap = SHELTER_PER_LEVEL * (1 + site.level as u32);
        let ok: Vec<HexCell> = site
            .buildings
            .iter()
            .filter(|b| !wrecked.contains(&b.key()))
            .filter(|b| {
                let i = g.index_of(b.center());
                if i == NONE || f.sim.is_sea(i) {
                    return false;
                }
                if !hz.flee() {
                    return true;
                }
                let s = f.at(i).0;
                if s < MILD {
                    return true;
                }
                let x = fine(f.sim, b, i);
                s < STRONG && x.water == 0 && x.fire == 0 && x.lava == 0 && x.crack < 128
            })
            .copied()
            .collect();
        for j in 0..ok.len() {
            if left == 0 || out.len() >= SHELTER_SPLIT {
                break;
            }
            let b = ok[(salt as usize + j) % ok.len()];
            let used = occ.entry(b.key()).or_default();
            let room = cap.saturating_sub(*used).min(left);
            if room == 0 {
                continue;
            }
            *used += room;
            left -= room;
            out.push((site.place, b, room));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// What stays: the scars at a beat
// ---------------------------------------------------------------------------------------------------------------------

/// The most of each scar list an answer carries.
pub const SCARS_MAX: usize = 600;

/// What the earth left at the end of `sim.beat`, per cell, so a client draws lasting damage at any zoom: `burnt`
/// (vegetation burned off, regrowing slowly: `fuel` now of `base`), `mud` (floodwater drying: land with 1..23 water
/// left), `ruins` (ruin ≥ 96), `wrecks` (buildings destroyed, at houses level), `sheltering` (crowds' people in
/// shelters then) and `fallen` (people who died in the last [`FALLEN_BEATS`] beats, per cell, with counts).
/// `near`: only what lies within (centre, km).
pub fn scars_json(sim: &Sim, s: &EarthState, near: Option<(LonLat, f64)>) -> Value {
    let g = grid();
    let beat = sim.beat;
    let close = |p: LonLat| near.is_none_or(|(c, km)| distance_km(c, p) <= km);
    let mut burnt = vec![];
    let mut mud = vec![];
    let mut ruins = vec![];
    for &i in &sim.list {
        let p = g.center[i as usize];
        if sim.is_sea(i) || !close(p) {
            continue;
        }
        let c = sim.c(i);
        if c.dfuel <= -24 && burnt.len() < SCARS_MAX {
            burnt.push(json!({ "cell": g.key(i), "fuel": sim.fuel(i), "base": g.fuel[i as usize] }));
        }
        if c.water > 0 && c.water < 24 && mud.len() < SCARS_MAX {
            mud.push(json!({ "cell": g.key(i), "water": c.water }));
        }
        if c.ruin >= 96 && ruins.len() < SCARS_MAX {
            ruins.push(json!({ "cell": g.key(i), "ruin": c.ruin }));
        }
    }
    let at = |k: &str| HexCell::parse(k).map(|c| c.center());
    let wrecks: Vec<Value> = s
        .wrecks
        .iter()
        .filter(|x| x.beat <= beat && at(&x.at).is_some_and(close))
        .take(SCARS_MAX)
        .map(|x| json!({ "cell": x.at, "settlement": format!("pl:{}", x.place), "beat": x.beat, "how": x.how, "cause": x.cause }))
        .collect();
    let sheltering: Vec<Value> = s
        .refuges
        .iter()
        .filter(|r| r.since <= beat && (r.until == 0 || r.until > beat) && at(&r.at).is_some_and(close))
        .take(SCARS_MAX)
        .map(|r| json!({ "crowd": format!("cl:{}", r.crowd), "from": r.home, "at": r.at, "settlement": format!("pl:{}", r.place),
                         "count": r.count, "since": r.since, "cause": r.cause }))
        .collect();
    let mut fallen: BTreeMap<String, (String, u32, u32)> = BTreeMap::new();
    for e in s.effects.iter().filter(|e| e.beat <= beat && e.beat + FALLEN_BEATS > beat) {
        for f in unpack(&e.rows).into_iter().filter(|f| f.what == W_DIED && f.count > 0 && close(f.cell.center())) {
            let x = fallen.entry(f.cell.key()).or_insert((f.cell.level.as_str().to_string(), 0, 0));
            x.1 += f.count;
            x.2 = x.2.max(e.beat);
        }
    }
    let fallen: Vec<Value> =
        fallen.into_iter().take(SCARS_MAX).map(|(k, (l, n, b))| json!({ "cell": k, "level": l, "count": n, "beat": b })).collect();
    json!({ "beat": beat, "burnt": burnt, "mud": mud, "ruins": ruins, "wrecks": wrecks, "sheltering": sheltering, "fallen": fallen })
}
