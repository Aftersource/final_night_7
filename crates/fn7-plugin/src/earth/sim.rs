//! The Earth simulator's core: the field, the phenomena and one beat's step. Pure: no `World`, no clock, no floats.
//!
//! A [`Sim`] is rebuilt from the stored state each beat (`from_state`), stepped once (`step`: the beat's inputs, then
//! one or more sub-steps of "phenomena act, then every dirty cell reacts"), and stored again (`to_state`). Every draw
//! is a hash of (seed, beat, sub-step, a salt, the cells involved): there is no generator state to carry, so a field
//! restored from a keyframe continues exactly as the one that was never interrupted.
//!
//! The numbers of every rule are the constants and the `react` arms below; docs/EARTH_SIM.md states them in words.

use fn7_model::earth::{EarthInput, EarthKeyframe, EarthState, Phenomenon};

use super::codec;
use super::grid::{grid, mix, Grid, H_MAX, M_ICE, M_ROCK, M_SAND, M_SOIL, M_WATER, SEA_LEVEL};

// ---------------------------------------------------------------------------------------------------------------------
// Kinds, causes, layers
// ---------------------------------------------------------------------------------------------------------------------

pub const STORM: u8 = 1;
pub const LIGHTNING: u8 = 2;
pub const FLOOD: u8 = 3;
pub const WILDFIRE: u8 = 4;
pub const VOLCANO: u8 = 5;
pub const EARTHQUAKE: u8 = 6;
pub const METEOR: u8 = 7;
pub const RAISE_LAND: u8 = 8;
pub const SINK_LAND: u8 = 9;
pub const FOREST: u8 = 10;
pub const SWARM: u8 = 11;
pub const BLIZZARD: u8 = 12;
pub const TIDAL_WAVE: u8 = 13;
pub const PLATE_DRIFT: u8 = 14;

pub const KINDS: [(u8, &str); 14] = [
    (STORM, "storm"), (LIGHTNING, "lightning"), (FLOOD, "flood"), (WILDFIRE, "wildfire"), (VOLCANO, "volcano"),
    (EARTHQUAKE, "earthquake"), (METEOR, "meteor"), (RAISE_LAND, "raise_land"), (SINK_LAND, "sink_land"),
    (FOREST, "forest"), (SWARM, "swarm"), (BLIZZARD, "blizzard"), (TIDAL_WAVE, "tidal_wave"), (PLATE_DRIFT, "plate_drift"),
];

pub fn kind_name(k: u8) -> &'static str {
    KINDS.iter().find(|x| x.0 == k).map(|x| x.1).unwrap_or("unknown")
}

pub fn kind_of(name: &str) -> Option<u8> {
    KINDS.iter().find(|x| x.1 == name).map(|x| x.0)
}

/// Why a phenomenon exists.
pub const CAUSE_CAST: u8 = 0;
pub const CAUSE_GOD: u8 = 1;
pub const CAUSE_RAMIFICATION: u8 = 2;
pub const CAUSE_ENTROPY: u8 = 3;
pub const CAUSE_INTERACTION: u8 = 4;
pub const CAUSES: [&str; 5] = ["cast", "god", "ramification", "entropy", "interaction"];

pub const TIER_MINI: u8 = 0;
pub const TIER_MEGA: u8 = 1;
pub const TIER_GOD: u8 = 2;
pub const TIER_WORLD: u8 = 3;
pub const TIERS: [&str; 4] = ["mini", "mega", "god", "world"];

pub const NOBODY: u8 = 255;
pub const NO_DIR: u8 = 6;

/// The dynamic layers of a cell, in storage order.
pub const N_LAYERS: usize = 17;
pub const LAYERS: [&str; N_LAYERS] = [
    "dh", "mat", "water", "lava", "ash", "fire", "snow", "cloud", "charge", "heat", "stress", "crack", "ruin", "dfuel",
    "swarm", "shake", "plume",
];

/// One cell's difference from the base terrain. All zero = the base terrain (not stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    /// Height change in levels (lava cooled to rock, a crater, land raised or sunk, a rift, a ridge).
    pub dh: i8,
    /// Surface material override (0 = the base's; else the material code + 1).
    pub mat: u8,
    /// Standing water above the ground (land) or above the sea's level (sea): 32 = one height level.
    pub water: u8,
    /// Molten lava, 32 = one height level.
    pub lava: u8,
    /// Ash on the ground.
    pub ash: u8,
    /// Fire intensity.
    pub fire: u8,
    /// Snow and ice on the ground.
    pub snow: u8,
    /// Cloud cover; it rains above 112.
    pub cloud: u8,
    /// Electric charge in the cloud; it strikes at 200.
    pub charge: u8,
    /// Heat above (or below) what is normal here.
    pub heat: i8,
    /// Stress above (or, after a rupture, below) what the faults here carry.
    pub stress: i8,
    /// Cracks; 128 and more is an open rift.
    pub crack: u8,
    /// What settlements here have suffered.
    pub ruin: u8,
    /// Vegetation change (burnt, eaten, grown).
    pub dfuel: i8,
    /// Swarm density.
    pub swarm: u8,
    /// How hard the ground shakes this beat.
    pub shake: u8,
    /// Ash and smoke in the air.
    pub plume: u8,
}

impl Cell {
    pub fn bytes(&self) -> [u8; N_LAYERS] {
        [self.dh as u8, self.mat, self.water, self.lava, self.ash, self.fire, self.snow, self.cloud, self.charge,
         self.heat as u8, self.stress as u8, self.crack, self.ruin, self.dfuel as u8, self.swarm, self.shake, self.plume]
    }
    pub fn from_bytes(b: &[u8; N_LAYERS]) -> Cell {
        Cell { dh: b[0] as i8, mat: b[1], water: b[2], lava: b[3], ash: b[4], fire: b[5], snow: b[6], cloud: b[7],
               charge: b[8], heat: b[9] as i8, stress: b[10] as i8, crack: b[11], ruin: b[12], dfuel: b[13] as i8,
               swarm: b[14], shake: b[15], plume: b[16] }
    }
    pub fn is_base(&self) -> bool {
        *self == Cell::default()
    }
}

fn add8(a: u8, x: i32) -> u8 {
    (a as i32 + x).clamp(0, 255) as u8
}
fn addi(a: i8, x: i32) -> i8 {
    (a as i32 + x).clamp(-127, 127) as i8
}

/// What a client draws on top of a column.
pub const TOPS: [&str; 13] = ["water", "rock", "soil", "sand", "ice", "forest", "lava", "fire", "ash", "snow", "flood", "rift", "swarm"];
pub const T_WATER: u8 = 0;
pub const T_FOREST: u8 = 5;
pub const T_LAVA: u8 = 6;
pub const T_FIRE: u8 = 7;
pub const T_ASH: u8 = 8;
pub const T_SNOW: u8 = 9;
pub const T_FLOOD: u8 = 10;
pub const T_RIFT: u8 = 11;
pub const T_SWARM: u8 = 12;

// ---------------------------------------------------------------------------------------------------------------------
// Settings the simulation reads (all integers; the room settings in fixed point)
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cfg {
    pub max_cells: u32,
    pub max_phenomena: u32,
    pub substeps: u32,
    pub keyframe_beats: u32,
    pub plate_beats: u32,
    /// Sixteenths.
    pub spread_q: u32,
    pub spontaneous_q: u32,
    pub energy_q: u32,
    pub mini_q: u32,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg { max_cells: 1500, max_phenomena: 48, substeps: 1, keyframe_beats: 16, plate_beats: 3, spread_q: 16,
              spontaneous_q: 16, energy_q: 16, mini_q: 8 }
    }
}

/// Entropy's level, 0..=5 (20, 40, 60, 80, 100).
pub fn level(entropy: u8) -> u32 {
    (entropy as u32 / 20).min(5)
}

// ---------------------------------------------------------------------------------------------------------------------
// What a step did (for the rules: consequences, memory, events)
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub spawned: Vec<u32>,
    pub ended: Vec<Phenomenon>,
    pub strikes: Vec<u16>,
    pub land_rose: Vec<u16>,
    pub drowned: Vec<u16>,
    /// (phenomenon, epicentre, cells ruptured).
    pub ruptures: Vec<(u32, u16, u16)>,
    pub aftershocks: Vec<u16>,
    /// (phenomenon, its new centre).
    pub plate_moves: Vec<(u32, u16)>,
    pub wave_hits: Vec<u16>,
    /// Volcanoes a quake woke.
    pub woke: Vec<u32>,
    pub eruptions: Vec<u16>,
    pub impacts: Vec<u16>,
    pub evicted: Vec<u16>,
    /// Cells that shook this beat, with how hard (the layer itself settles within the step).
    pub shaken: Vec<(u16, u8)>,
    /// Interactions counted by name.
    pub quenched: u32,
    pub steamed: u32,
    pub ignitions: u32,
    pub melted: u32,
    pub rock_made: u32,
    pub spontaneous: u32,
    pub refused: u32,
    pub substeps: u32,
}

/// A request to spawn.
#[derive(Debug, Clone, Copy)]
pub struct Spawn {
    pub kind: u8,
    pub cell: u16,
    pub lon_e3: i32,
    pub lat_e3: i32,
    pub energy: u8,
    pub radius: u8,
    pub owner: u8,
    pub cause: u8,
    pub parent: u32,
    pub tier: u8,
}

impl From<&EarthInput> for Spawn {
    fn from(i: &EarthInput) -> Spawn {
        Spawn { kind: i.kind, cell: i.cell, lon_e3: i.lon_e3, lat_e3: i.lat_e3, energy: i.energy, radius: i.radius,
                owner: i.owner, cause: i.cause, parent: 0, tier: i.tier }
    }
}

const ADD: u8 = 0;
const MAX: u8 = 1;
const L_WATER: u8 = 2;
const L_LAVA: u8 = 3;
const L_FIRE: u8 = 5;
const L_CLOUD: u8 = 7;
const L_SWARM: u8 = 14;
const L_PLUME: u8 = 16;

/// Salts of the draws.
const S_FIRE: u64 = 1;
const S_ROCK: u64 = 2;
const S_SPONT: u64 = 3;
const S_SPAWN: u64 = 4;
const S_PHEN: u64 = 5;
const S_WIND: u64 = 6;
const S_EVICT: u64 = 7;

// ---------------------------------------------------------------------------------------------------------------------
// The simulation
// ---------------------------------------------------------------------------------------------------------------------

pub struct Sim {
    pub seed: u64,
    pub cfg: Cfg,
    pub beat: u32,
    pub next_id: u32,
    pub tension: u16,
    pub phen: Vec<Phenomenon>,
    /// Every cell of the grid (dense while the simulation is in memory).
    pub cells: Vec<Cell>,
    /// The cells that may differ from base: sorted and exact after [`Sim::normalize`].
    pub list: Vec<u16>,
    flag: Vec<bool>,
    entropy: u8,
    sub: u32,
    g: &'static Grid,
}

impl Sim {
    pub fn new(seed: u64, cfg: Cfg) -> Sim {
        let g = grid();
        Sim { seed, cfg, beat: 0, next_id: 1, tension: 0, phen: vec![], cells: vec![Cell::default(); g.len()], list: vec![],
              flag: vec![false; g.len()], entropy: 0, sub: 0, g }
    }

    pub fn grid(&self) -> &'static Grid {
        self.g
    }

    fn load(&mut self, cells: &[(u16, [u8; N_LAYERS])]) {
        for (i, b) in cells {
            if (*i as usize) < self.cells.len() {
                *self.m(*i) = Cell::from_bytes(b);
            }
        }
        self.normalize();
    }

    /// The simulation as the state holds it.
    pub fn from_state(seed: u64, cfg: Cfg, s: &EarthState) -> Sim {
        let mut sim = Sim::new(seed, cfg);
        sim.beat = s.beat;
        sim.next_id = s.next_id.max(1);
        sim.tension = s.tension;
        sim.phen = s.phenomena.clone();
        sim.load(&codec::unpack(&s.cells));
        sim
    }

    /// The dirty cells, sorted.
    pub fn sparse(&self) -> Vec<(u16, [u8; N_LAYERS])> {
        self.list.iter().map(|&i| (i, self.cells[i as usize].bytes())).collect()
    }

    /// Store the simulation back (the input log and the keyframes are the caller's).
    pub fn to_state(&self, s: &mut EarthState) {
        s.beat = self.beat;
        s.next_id = self.next_id;
        s.tension = self.tension;
        s.phenomena = self.phen.clone();
        s.cells = codec::pack(&[], &self.sparse());
    }

    /// The field at the end of `beat`, rebuilt from the newest keyframe at or before it and at most K beats of the
    /// input log. `None` if the state never reached that beat.
    pub fn at_beat(seed: u64, cfg: Cfg, s: &EarthState, beat: u32) -> Option<Sim> {
        Self::at_beat_from(seed, cfg, s, beat, true)
    }

    /// As [`Sim::at_beat`]; `keyframes: false` replays from the first beat (the proof that they agree).
    pub fn at_beat_from(seed: u64, cfg: Cfg, s: &EarthState, beat: u32, keyframes: bool) -> Option<Sim> {
        if beat > s.beat || s.first_beat == 0 {
            return None;
        }
        let mut sim = Sim::new(seed, cfg);
        sim.beat = s.first_beat.saturating_sub(1);
        if keyframes {
            let n = s.keyframes.iter().take_while(|k| k.beat <= beat).count();
            if n > 0 {
                let mut cells = vec![];
                for k in &s.keyframes[..n] {
                    cells = codec::apply(cells, &k.cells);
                }
                let k = &s.keyframes[n - 1];
                sim.load(&cells);
                sim.beat = k.beat;
                sim.next_id = k.next_id;
                sim.tension = k.tension;
                sim.phen = k.phenomena.clone();
            }
        }
        while sim.beat < beat {
            let b = sim.beat + 1;
            let e = *s.entropy.get((b - s.first_beat) as usize)?;
            let spawns: Vec<Spawn> = s.inputs.iter().filter(|i| i.beat == b).map(Spawn::from).collect();
            sim.step(b, e, &spawns);
        }
        Some(sim)
    }

    /// Step the stored state to `beat` reading `entropy` (0..=100): the whole of one beat's bookkeeping. The state's
    /// inputs for the beat must already be logged. Returns the simulation after, what the last step did, and the
    /// dirty cells before (for the beat's delta). Beats are consecutive; if one was skipped the field steps through it
    /// with the same Entropy (logged, so a replay agrees).
    pub fn advance(seed: u64, cfg: Cfg, s: &mut EarthState, beat: u32, entropy: u8) -> (Sim, Report, codec::Sparse) {
        if s.first_beat == 0 {
            s.first_beat = beat.max(1);
            s.beat = s.first_beat - 1;
            s.next_id = 1;
        }
        let mut sim = Sim::from_state(seed, cfg, s);
        let before = sim.sparse();
        let mut report = Report::default();
        while sim.beat < beat {
            let b = sim.beat + 1;
            s.entropy.push(entropy.min(100));
            let spawns: Vec<Spawn> = s.inputs.iter().filter(|i| i.beat == b).map(Spawn::from).collect();
            report = sim.step(b, entropy, &spawns);
            s.evicted += report.evicted.len() as u32;
            sim.to_state(s);
            if (b + 1 - s.first_beat).is_multiple_of(cfg.keyframe_beats.max(1)) {
                let k = sim.keyframe(s);
                s.keyframes.push(k);
            }
        }
        (sim, report, before)
    }

    /// A keyframe of the field now, as a delta against the keyframes already in `s`.
    pub fn keyframe(&self, s: &EarthState) -> EarthKeyframe {
        let mut prev = vec![];
        for k in &s.keyframes {
            prev = codec::apply(prev, &k.cells);
        }
        EarthKeyframe { beat: self.beat, cells: codec::pack(&prev, &self.sparse()), phenomena: self.phen.clone(),
                        next_id: self.next_id, tension: self.tension }
    }

    /// A 64-bit fingerprint of the field and the phenomena.
    pub fn hash(&self) -> u64 {
        let mut h = mix(self.beat as u64 ^ ((self.tension as u64) << 32) ^ ((self.next_id as u64) << 48));
        for &i in &self.list {
            let b = self.cells[i as usize].bytes();
            h = mix(h ^ i as u64);
            for c in b.chunks(8) {
                let mut x = 0u64;
                for (k, v) in c.iter().enumerate() {
                    x |= (*v as u64) << (8 * k);
                }
                h = mix(h ^ x);
            }
        }
        for p in &self.phen {
            for x in [p.id as u64, p.kind as u64 | (p.cell as u64) << 8 | (p.radius as u64) << 24 | (p.dir as u64) << 32 | (p.dur as u64) << 40,
                      p.power as u16 as u64 | (p.a as u16 as u64) << 16 | (p.b as u16 as u64) << 32 | (p.owner as u64) << 48,
                      p.born as u64 | (p.parent as u64) << 32, p.lon_e3 as u32 as u64 | (p.lat_e3 as u32 as u64) << 32,
                      p.cause as u64 | (p.tier as u64) << 8] {
                h = mix(h ^ x);
            }
        }
        h
    }

    // ---- cells ------------------------------------------------------------------------------------------------------

    /// A cell to write.
    pub fn m(&mut self, i: u16) -> &mut Cell {
        let k = i as usize;
        if !self.flag[k] {
            self.flag[k] = true;
            self.list.push(i);
        }
        &mut self.cells[k]
    }

    pub fn c(&self, i: u16) -> &Cell {
        &self.cells[i as usize]
    }

    /// Sort the dirty list and drop the cells that are back at base.
    pub fn normalize(&mut self) {
        self.list.sort_unstable();
        let (cells, flag) = (&self.cells, &mut self.flag);
        self.list.retain(|&i| {
            let keep = !cells[i as usize].is_base();
            if !keep {
                flag[i as usize] = false;
            }
            keep
        });
    }

    /// The height of a column now, 0..=15.
    pub fn height(&self, i: u16) -> u8 {
        (self.g.h[i as usize] as i32 + self.cells[i as usize].dh as i32).clamp(0, H_MAX as i32) as u8
    }

    pub fn is_sea(&self, i: u16) -> bool {
        self.height(i) < SEA_LEVEL
    }

    /// The surface material under the dynamic layers.
    pub fn material(&self, i: u16) -> u8 {
        let c = &self.cells[i as usize];
        if self.is_sea(i) {
            M_WATER
        } else if c.mat > 0 {
            c.mat - 1
        } else if self.g.mat[i as usize] == M_WATER {
            M_ROCK
        } else {
            self.g.mat[i as usize]
        }
    }

    /// What can burn here now, 0..=240.
    pub fn fuel(&self, i: u16) -> u8 {
        if self.is_sea(i) {
            return 0;
        }
        (self.g.fuel[i as usize] as i32 + self.cells[i as usize].dfuel as i32).clamp(0, 240) as u8
    }

    fn set_fuel(&mut self, i: u16, fuel: i32) {
        let base = self.g.fuel[i as usize] as i32;
        self.m(i).dfuel = (fuel.clamp(0, 240) - base).clamp(-127, 127) as i8;
    }

    /// Is it cold here (snow instead of rain): the polar bands, the peaks, or a cold spell.
    pub fn cold(&self, i: u16) -> bool {
        let lat = self.g.center[i as usize][1].abs();
        let c = &self.cells[i as usize];
        c.heat <= -24 || ((lat >= 58.0 || self.height(i) >= 14) && c.heat < 24)
    }

    /// The top material a client draws.
    pub fn top(&self, i: u16) -> u8 {
        let c = &self.cells[i as usize];
        let sea = self.is_sea(i);
        if c.lava >= 8 {
            T_LAVA
        } else if c.fire >= 16 {
            T_FIRE
        } else if sea {
            let lat = self.g.center[i as usize][1];
            if (lat >= 78.0 || lat <= -70.0 || c.heat <= -64) && c.heat < 32 {
                M_ICE
            } else {
                T_WATER
            }
        } else if c.crack >= 128 {
            T_RIFT
        } else if c.water >= 24 {
            if c.heat <= -48 { M_ICE } else { T_FLOOD }
        } else if c.snow >= 32 {
            T_SNOW
        } else if c.ash >= 48 {
            T_ASH
        } else if c.swarm >= 96 {
            T_SWARM
        } else if self.fuel(i) >= 90 && self.material(i) == M_SOIL {
            T_FOREST
        } else {
            self.material(i)
        }
    }

    /// Change a column's height by `d` levels; records land that rose from the sea or drowned.
    fn lift(&mut self, i: u16, d: i32, rep: &mut Report) {
        let before = self.height(i);
        let after = (before as i32 + d).clamp(0, H_MAX as i32) as u8;
        if after == before {
            return;
        }
        let base = self.g.h[i as usize] as i32;
        let base_mat = self.g.mat[i as usize];
        let c = self.m(i);
        c.dh = (after as i32 - base) as i8;
        if before < SEA_LEVEL && after >= SEA_LEVEL {
            c.mat = M_ROCK + 1;
            c.water = 0;
            rep.land_rose.push(i);
        } else if before >= SEA_LEVEL && after < SEA_LEVEL {
            c.fire = 0;
            c.snow = 0;
            c.swarm = 0;
            c.ruin = 255;
            c.mat = 0;
            rep.drowned.push(i);
        } else if after >= SEA_LEVEL && d > 0 && base_mat == M_WATER && c.mat == 0 {
            c.mat = M_ROCK + 1;
        }
    }

    fn shake_disc(&mut self, center: u16, r: u8, peak: i32) {
        for (j, d) in self.g.disc(center, r) {
            let v = (peak * (r as i32 + 1 - d as i32) / (r as i32 + 1)).clamp(0, 255) as u8;
            let c = self.m(j);
            c.shake = c.shake.max(v);
        }
    }

    // ---- draws and wind ---------------------------------------------------------------------------------------------

    fn draw(&self, salt: u64, a: u64, b: u64) -> u64 {
        mix(self.seed ^ mix(salt ^ ((self.beat as u64) << 8) ^ ((self.sub as u64) << 40)) ^ mix((a << 24) ^ b ^ 0x5EED))
    }

    /// True with probability `p16 / 65536`.
    fn chance(&self, salt: u64, a: u64, b: u64, p16: u32) -> bool {
        (self.draw(salt, a, b) & 0xFFFF) < p16 as u64
    }

    /// The wind at a cell this beat: the neighbour index it blows toward and its strength 1..=3. The global
    /// circulation (trades blow west below 30°, westerlies east to 60°, polar easterlies beyond) with a seeded wobble
    /// per 30° patch that changes every 4 beats; one notch stronger under a storm.
    pub fn wind(&self, i: u16) -> (u8, u8) {
        wind(self.g, self.seed, self.beat, i, self.cells[i as usize].cloud)
    }

    // ---- spawning ---------------------------------------------------------------------------------------------------

    /// Bring a phenomenon into the world (its immediate effects land now). `None` when the list is full of stronger
    /// ones.
    pub fn spawn(&mut self, s: Spawn, rep: &mut Report) -> Option<u32> {
        if s.cell as usize >= self.cells.len() || kind_name(s.kind) == "unknown" {
            return None;
        }
        let e = s.energy.clamp(1, 10) as i32;
        let scale = if s.tier == TIER_MINI { self.cfg.mini_q * self.cfg.energy_q / 16 } else { self.cfg.energy_q } as i32;
        let power = (e * scale).clamp(4, 400);
        if self.phen.len() >= self.cfg.max_phenomena as usize {
            // Full: the weakest (a sleeping volcano first, then the lowest power, then the oldest) makes room for a
            // stronger newcomer.
            let (k, weakest) = self.phen.iter().enumerate().min_by_key(|(_, p)| (p.power, p.id)).map(|(k, p)| (k, *p))?;
            if weakest.power as i32 >= power {
                rep.refused += 1;
                return None;
            }
            self.phen.remove(k);
            rep.ended.push(weakest);
        }
        let id = self.next_id;
        self.next_id += 1;
        let mega = s.tier != TIER_MINI;
        let mut kind = s.kind;
        if kind == STORM && self.cold(s.cell) {
            kind = BLIZZARD;
        }
        let radius = if mega { s.radius.min(4) } else { 0 };
        let mut p = Phenomenon { id, kind, cell: s.cell, lon_e3: s.lon_e3, lat_e3: s.lat_e3, born: self.beat, power: power as i16,
                                 radius, dir: NO_DIR, dur: 1, owner: s.owner, cause: s.cause, parent: s.parent, a: 0,
                                 b: e as i16, tier: s.tier };
        let c0 = s.cell;
        match kind {
            STORM | BLIZZARD => p.dur = (4 + e / 2) as u8,
            LIGHTNING => p.dur = 1,
            FLOOD => p.dur = (2 + e / 3) as u8,
            WILDFIRE => {
                p.dur = (3 + e / 2) as u8;
                for (j, d) in self.g.disc(c0, radius) {
                    let dry = self.c(j).water < 8 && self.c(j).snow < 32;
                    if self.fuel(j) >= 16 && dry && (d == 0 || self.chance(S_SPAWN, id as u64, j as u64, 32768)) {
                        let f = if d == 0 { (96 + power / 2).min(255) as u8 } else { 64 };
                        let c = self.m(j);
                        c.fire = c.fire.max(f);
                        rep.ignitions += 1;
                    } else if d == 0 {
                        let c = self.m(j);
                        c.heat = addi(c.heat, power / 2);
                    }
                }
            }
            VOLCANO => {
                p.dur = (3 + e / 2) as u8;
                if mega {
                    self.lift(c0, if e >= 9 { 2 } else { 1 }, rep);
                    if !self.is_sea(c0) {
                        self.m(c0).mat = M_ROCK + 1;
                    }
                }
                self.shake_disc(c0, radius.max(1), 160);
                rep.eruptions.push(c0);
            }
            EARTHQUAKE => {
                p.dur = (2 + e / 3) as u8;
                let rupt = self.rupture(c0, e, mega);
                p.a = rupt.len() as i16;
                let on_fault = self.g.fault[rupt[0] as usize] == 0;
                for &r in &rupt {
                    let c = self.m(r);
                    c.crack = add8(c.crack, if on_fault { 48 + power / 2 } else { power / 2 });
                    c.stress = addi(c.stress, -96);
                    c.shake = if on_fault { (96 + power * 2).min(255) } else { (64 + power / 2).min(255) } as u8;
                }
                self.shake_disc(rupt[0], radius + 1, if on_fault { 200 + power / 4 } else { 96 + power / 2 });
                rep.ruptures.push((id, rupt[0], rupt.len() as u16));
                // A quake near a volcano sets it off (a sleeping one wakes).
                let near: Vec<u16> = rupt.iter().flat_map(|&r| self.g.disc(r, 2)).map(|x| x.0).collect();
                for k in 0..self.phen.len() {
                    let v = self.phen[k];
                    if v.kind == VOLCANO && near.contains(&v.cell) {
                        let v = &mut self.phen[k];
                        v.power = (v.power as i32 + power / 2).min(400) as i16;
                        v.dur = (v.dur + 2).min(12);
                        v.a = 0;
                        rep.woke.push(v.id);
                        rep.eruptions.push(v.cell);
                    }
                }
                let wet = rupt.iter().filter(|&&r| self.is_sea(r)).count();
                self.phen.push(p);
                rep.spawned.push(id);
                // An offshore quake raises a tidal wave; a great one on a boundary sets the plate moving.
                if e >= 4 && (wet * 2 >= rupt.len() || self.is_sea(c0)) {
                    if let Some(&sea) = rupt.iter().find(|&&r| self.is_sea(r)).or(self.is_sea(c0).then_some(&c0)) {
                        self.spawn(Spawn { kind: TIDAL_WAVE, cell: sea, energy: (e - 1) as u8, cause: CAUSE_INTERACTION, parent: id,
                                           tier: TIER_WORLD, radius: 0, ..s }, rep);
                    }
                }
                if mega && e >= 9 && on_fault {
                    self.spawn(Spawn { kind: PLATE_DRIFT, cell: rupt[0], energy: (e - 2) as u8, cause: CAUSE_INTERACTION, parent: id,
                                       tier: TIER_WORLD, radius: 1, ..s }, rep);
                }
                return Some(id);
            }
            METEOR => {
                p.dur = 2;
                let was_sea = self.is_sea(c0);
                if mega {
                    self.lift(c0, if e >= 9 { -2 } else { -1 }, rep);
                    if radius >= 2 {
                        for &j in &self.g.nb[c0 as usize].clone() {
                            if j != c0 && self.chance(S_SPAWN, id as u64, j as u64, 32768) {
                                self.lift(j, -1, rep);
                            }
                        }
                    }
                    for (j, d) in self.g.disc(c0, radius.max(1)) {
                        if d >= 1 && self.fuel(j) >= 24 && self.chance(S_SPAWN, id as u64 ^ 0x77, j as u64, 43690) {
                            let c = self.m(j);
                            c.fire = c.fire.max(160);
                            rep.ignitions += 1;
                        }
                        if d == 1 {
                            let c = self.m(j);
                            c.plume = c.plume.max(128);
                        }
                    }
                    let sea_now = self.is_sea(c0);
                    let c = self.m(c0);
                    if !sea_now {
                        c.mat = M_ROCK + 1;
                    }
                    c.heat = addi(c.heat, 100);
                    c.plume = 255;
                    c.ruin = 255;
                    self.shake_disc(c0, radius + 1, 255);
                } else {
                    let fuel = self.fuel(c0);
                    let c = self.m(c0);
                    c.crack = add8(c.crack, 96);
                    c.heat = addi(c.heat, 60);
                    c.plume = c.plume.max(128);
                    c.ruin = add8(c.ruin, 96);
                    c.shake = c.shake.max(160);
                    if fuel >= 24 && c.water < 8 {
                        c.fire = c.fire.max(128);
                    }
                }
                rep.impacts.push(c0);
                if was_sea {
                    let c = self.m(c0);
                    c.cloud = add8(c.cloud, 200);
                    rep.steamed += 1;
                    self.phen.push(p);
                    rep.spawned.push(id);
                    if e >= 4 {
                        self.spawn(Spawn { kind: TIDAL_WAVE, cell: c0, energy: e as u8, cause: CAUSE_INTERACTION, parent: id,
                                           tier: TIER_WORLD, radius: 0, ..s }, rep);
                    }
                    return Some(id);
                }
            }
            RAISE_LAND | SINK_LAND => p.dur = (1 + e / 3).min(3) as u8,
            FOREST => p.dur = (3 + e / 2) as u8,
            SWARM => p.dur = (4 + e / 2) as u8,
            TIDAL_WAVE => {
                p.dur = 12;
                p.radius = 0;
            }
            PLATE_DRIFT => {
                p.dur = 255;
                p.radius = radius.clamp(1, 2);
                p.a = (2 + e / 3) as i16;
                p.b = self.cfg.plate_beats.max(1) as i16;
                // Along the boundary if it can, else toward it, else anywhere.
                let nb = self.g.nb[c0 as usize];
                let along: Vec<u8> = (0..6u8).filter(|&k| nb[k as usize] != c0 && self.g.fault[nb[k as usize] as usize] == 0).collect();
                let pick = self.draw(S_SPAWN, id as u64, 0xD1) as usize;
                p.dir = if !along.is_empty() {
                    along[pick % along.len()]
                } else {
                    (0..6u8).filter(|&k| nb[k as usize] != c0).min_by_key(|&k| (self.g.fault[nb[k as usize] as usize], (k as usize + pick) % 6)).unwrap_or(0)
                };
            }
            _ => {}
        }
        self.phen.push(p);
        rep.spawned.push(id);
        Some(id)
    }

    /// The cells an earthquake at `c0` ruptures: from the nearest plate boundary within 4 hops, along it. Far from any
    /// boundary: the epicentre alone.
    fn rupture(&self, c0: u16, e: i32, mega: bool) -> Vec<u16> {
        let g = self.g;
        let mut c = c0;
        for _ in 0..4 {
            if g.fault[c as usize] == 0 {
                break;
            }
            let next = g.nb[c as usize].iter().copied().min_by_key(|&j| g.fault[j as usize]).unwrap_or(c);
            if g.fault[next as usize] >= g.fault[c as usize] {
                break;
            }
            c = next;
        }
        if g.fault[c as usize] != 0 {
            return vec![c0];
        }
        let n = if mega { 2 + 2 * e as usize } else { 1 + e as usize / 2 };
        let mut out = vec![c];
        let mut k = 0;
        while k < out.len() && out.len() < n {
            let i = out[k];
            k += 1;
            for &j in &g.nb[i as usize] {
                if out.len() < n && g.fault[j as usize] == 0 && !out.contains(&j) {
                    out.push(j);
                }
            }
        }
        out
    }

    // ---- one beat ---------------------------------------------------------------------------------------------------

    /// Step the field to `beat`: this beat's `spawns` in order, the world's own (Entropy), then the sub-steps, then the
    /// cap.
    pub fn step(&mut self, beat: u32, entropy: u8, spawns: &[Spawn]) -> Report {
        let mut rep = Report::default();
        self.beat = beat;
        self.entropy = entropy.min(100);
        self.sub = 0;
        let l = level(self.entropy);
        for s in spawns {
            self.spawn(*s, &mut rep);
        }
        self.spontaneous(l, &mut rep);
        // Heat death: everything live flares.
        if l >= 5 {
            for p in self.phen.iter_mut().filter(|p| p.power > 0) {
                p.power = (p.power as i32 * 3 / 2).min(400) as i16;
                if p.dur < 20 {
                    p.dur += 1;
                }
            }
        }
        // The timeline runs fast when the world is coming apart: more sub-steps per beat.
        let subs = (self.cfg.substeps + (l >= 3) as u32 + (l >= 4) as u32 + (l >= 5) as u32).clamp(1, 6);
        rep.substeps = subs;
        for sub in 0..subs {
            self.sub = sub;
            self.phenomena(&mut rep);
            self.normalize();
            self.cells_react(&mut rep);
            self.normalize();
        }
        self.sub = 0;
        self.age(&mut rep);
        self.evict(&mut rep);
        rep
    }

    /// The world's own phenomena: Entropy stresses every fault, and the stressed ones let go.
    fn spontaneous(&mut self, l: u32, rep: &mut Report) {
        if l == 0 || self.g.fault_cells.is_empty() {
            return;
        }
        self.tension = (self.tension as u32 + 4 * l).min(1000) as u16;
        let rolls = [0, 1, 1, 2, 4, 8][l as usize];
        let odds = [0u32, 40, 90, 160, 256, 600][l as usize];
        for k in 0..rolls {
            let d = self.draw(S_SPONT, k, 0);
            let fc = self.g.fault_cells[(d >> 16) as usize % self.g.fault_cells.len()];
            let stress = (96 + self.tension as i32 / 4 + self.c(fc).stress as i32).clamp(0, 255) as u32;
            let p16 = (stress * odds * self.cfg.spontaneous_q / 16).min(65536);
            if (d & 0xFFFF) as u32 >= p16 {
                continue;
            }
            let pick = (d >> 40) % 100;
            let kind = if l >= 3 && pick < 15 {
                PLATE_DRIFT
            } else if l >= 2 && pick < 40 {
                VOLCANO
            } else {
                EARTHQUAKE
            };
            let [lon, lat] = self.g.center[fc as usize];
            let s = Spawn { kind, cell: fc, lon_e3: (lon * 1000.0).round() as i32, lat_e3: (lat * 1000.0).round() as i32,
                            energy: (2 + l + ((d >> 50) & 1) as u32) as u8, radius: 1, owner: NOBODY, cause: CAUSE_ENTROPY, parent: 0,
                            tier: TIER_WORLD };
            if self.spawn(s, rep).is_some() {
                rep.spontaneous += 1;
                self.tension = self.tension.saturating_sub(24);
            }
        }
        // Freak weather anywhere (Entropy 20+).
        let d = self.draw(S_SPONT, 99, 1);
        if ((d & 0xFFFF) as u32) < (3277 * l * self.cfg.spontaneous_q / 16).min(65536) {
            let cell = ((d >> 16) % self.g.len() as u64) as u16;
            let kind = [STORM, STORM, LIGHTNING, BLIZZARD][((d >> 40) % 4) as usize];
            let [lon, lat] = self.g.center[cell as usize];
            let s = Spawn { kind, cell, lon_e3: (lon * 1000.0).round() as i32, lat_e3: (lat * 1000.0).round() as i32,
                            energy: (2 + l) as u8, radius: 1, owner: NOBODY, cause: CAUSE_ENTROPY, parent: 0, tier: TIER_WORLD };
            if self.spawn(s, rep).is_some() {
                rep.spontaneous += 1;
            }
        }
    }

    /// Every phenomenon acts (in id order).
    fn phenomena(&mut self, rep: &mut Report) {
        let n = self.phen.len();
        let first = self.sub == 0;
        for k in 0..n {
            let mut p = self.phen[k];
            let age = self.beat.saturating_sub(p.born);
            let (c0, power, e) = (p.cell, p.power as i32, p.b as i32);
            match p.kind {
                STORM | BLIZZARD => {
                    for (j, d) in self.g.disc(c0, p.radius) {
                        let land = !self.is_sea(j);
                        let c = self.m(j);
                        c.cloud = add8(c.cloud, (power / 2) >> d);
                        if p.kind == BLIZZARD {
                            c.heat = addi(c.heat, -((power / 8) >> d));
                            if land {
                                c.snow = add8(c.snow, (power / 8) >> d);
                            }
                        }
                    }
                    if first && age >= 1 {
                        let (dir, _) = self.wind(c0);
                        let next = self.g.nb[c0 as usize][dir as usize];
                        p.dir = dir;
                        p.cell = next;
                        let lat = self.g.center[next as usize][1].abs();
                        let mut pw = power - power / 8;
                        if p.kind == STORM && self.is_sea(next) && lat < 35.0 {
                            pw += 4;
                        }
                        if self.c(next).fire >= 64 {
                            pw += 8;
                        }
                        p.power = pw.min(400) as i16;
                    }
                }
                LIGHTNING => {
                    if first && age == 0 {
                        let n = 1 + if p.tier != TIER_MINI { e / 3 } else { 0 };
                        let disc = self.g.disc(c0, p.radius);
                        for s in 0..n {
                            let j = if s == 0 { c0 } else { disc[self.draw(S_PHEN, p.id as u64, s as u64) as usize % disc.len()].0 };
                            self.strike(j, power, rep);
                        }
                    }
                }
                FLOOD => {
                    for (j, d) in self.g.disc(c0, p.radius) {
                        if self.is_sea(j) {
                            continue;
                        }
                        let coastal = self.g.coast[j as usize] <= 1;
                        let c = self.m(j);
                        c.water = add8(c.water, ((power / 2) >> d) * if coastal { 2 } else { 1 });
                    }
                    if first && age >= 1 {
                        p.power = (power - power / 8 + if self.c(c0).cloud >= 128 { 4 } else { 0 }) as i16;
                    }
                }
                WILDFIRE => {
                    if first && age >= 1 {
                        for (j, _) in self.g.disc(c0, p.radius) {
                            if self.c(j).fire > 0 {
                                let c = self.m(j);
                                c.fire = add8(c.fire, power / 8);
                            }
                        }
                        p.power = (power - power / 4) as i16;
                    }
                }
                VOLCANO => {
                    if p.dur > 0 && power > 0 {
                        let c = self.m(c0);
                        c.lava = add8(c.lava, power / 4);
                        c.plume = add8(c.plume, power / 2);
                        c.heat = addi(c.heat, power / 8);
                        if p.tier != TIER_MINI && p.radius >= 1 {
                            let k = self.draw(S_PHEN, p.id as u64, 1) as usize % 6;
                            let j = self.g.nb[c0 as usize][k];
                            let c = self.m(j);
                            c.lava = add8(c.lava, power / 8);
                        }
                        if first {
                            let disc = self.g.disc(c0, 2);
                            let j = disc[self.draw(S_PHEN, p.id as u64, 2) as usize % disc.len()].0;
                            if self.fuel(j) >= 24 && self.c(j).water < 8 {
                                let c = self.m(j);
                                c.fire = c.fire.max(96);
                                rep.ignitions += 1;
                            }
                            if age >= 1 {
                                p.power = (power - power / 8) as i16;
                            }
                        }
                    }
                }
                EARTHQUAKE => {
                    if first && age >= 1 && self.chance(S_PHEN, p.id as u64, 3, 39322) {
                        let rupt = self.rupture(c0, e, p.tier != TIER_MINI);
                        let j = rupt[self.draw(S_PHEN, p.id as u64, 4) as usize % rupt.len()];
                        self.shake_disc(j, 1, 128);
                        let c = self.m(j);
                        c.crack = add8(c.crack, 16);
                        rep.aftershocks.push(j);
                    }
                }
                RAISE_LAND => {
                    if first {
                        if p.tier != TIER_MINI {
                            self.lift(c0, 1, rep);
                            if age == 1 && p.radius >= 1 {
                                for j in self.g.nb[c0 as usize] {
                                    if j != c0 {
                                        self.lift(j, 1, rep);
                                    }
                                }
                            }
                            self.shake_disc(c0, p.radius, 64);
                            for (j, _) in self.g.disc(c0, p.radius) {
                                if self.g.fault[j as usize] == 0 {
                                    let c = self.m(j);
                                    c.stress = addi(c.stress, 16);
                                }
                            }
                        } else {
                            let fuel = self.fuel(c0) as i32;
                            let c = self.m(c0);
                            c.ruin = add8(c.ruin, -32);
                            c.crack = add8(c.crack, -64);
                            c.shake = c.shake.max(32);
                            self.set_fuel(c0, fuel + 8);
                        }
                    }
                }
                SINK_LAND => {
                    if first {
                        if p.tier != TIER_MINI {
                            self.lift(c0, -1, rep);
                            if age == 0 && p.radius >= 1 {
                                for j in self.g.nb[c0 as usize] {
                                    if j != c0 && (self.g.coast[j as usize] <= 2 || self.chance(S_PHEN, p.id as u64, j as u64, 32768)) {
                                        self.lift(j, -1, rep);
                                    }
                                }
                            }
                            self.shake_disc(c0, p.radius, 96);
                        } else {
                            let c = self.m(c0);
                            c.water = add8(c.water, power);
                            c.crack = add8(c.crack, 64);
                            c.shake = c.shake.max(64);
                        }
                    }
                }
                FOREST => {
                    for (j, d) in self.g.disc(c0, p.radius) {
                        let c = *self.c(j);
                        if self.is_sea(j) || self.material(j) == M_ICE || self.height(j) >= 14 || c.lava > 0 || c.water >= 64 {
                            continue;
                        }
                        let fuel = self.fuel(j) as i32;
                        let mat = self.material(j);
                        self.set_fuel(j, fuel + ((8 + e) >> d).max(1));
                        let c = self.m(j);
                        c.ash = add8(c.ash, -8);
                        c.ruin = add8(c.ruin, -4);
                        // Where enough grows, rock and sand become soil.
                        if fuel >= 48 && matches!(mat, M_ROCK | M_SAND) {
                            c.mat = M_SOIL + 1;
                        }
                    }
                }
                SWARM => {
                    if !self.is_sea(c0) {
                        let c = self.m(c0);
                        c.swarm = add8(c.swarm, power / 2);
                        if p.radius >= 1 {
                            for j in self.g.nb[c0 as usize] {
                                if j != c0 && !self.is_sea(j) {
                                    let c = self.m(j);
                                    c.swarm = add8(c.swarm, power / 8);
                                }
                            }
                        }
                    }
                    if first && age >= 1 {
                        // Toward the richest neighbour.
                        let rot = self.draw(S_PHEN, p.id as u64, 5) as usize;
                        let best = (0..6usize).map(|k| (k + rot) % 6)
                            .filter(|&k| !self.is_sea(self.g.nb[c0 as usize][k]))
                            .max_by_key(|&k| self.fuel(self.g.nb[c0 as usize][k]));
                        if let Some(k) = best {
                            if self.fuel(self.g.nb[c0 as usize][k]) > self.fuel(c0) / 2 {
                                p.dir = k as u8;
                                p.cell = self.g.nb[c0 as usize][k];
                            }
                        }
                    }
                }
                TIDAL_WAVE => {
                    p.a += 1;
                    p.radius = p.a.clamp(0, 255) as u8;
                    for j in self.wave_front(c0, p.a as u8) {
                        let c = self.m(j);
                        c.water = c.water.max((power / 2).min(255) as u8);
                        for n in self.g.nb[j as usize] {
                            if n != j && !self.is_sea(n) && self.height(n) <= 9 {
                                let c = self.m(n);
                                if (c.water as i32) < power.min(255) {
                                    c.water = power.min(255) as u8;
                                    c.ruin = add8(c.ruin, power / 8);
                                    rep.wave_hits.push(n);
                                }
                            }
                        }
                    }
                    p.power = (power * 13 / 16) as i16;
                    if p.power < 12 || p.a >= 12 {
                        p.dur = 0;
                    }
                }
                PLATE_DRIFT => {
                    if first {
                        p.b -= 1;
                        if p.b <= 0 {
                            p.b = self.cfg.plate_beats.max(1) as i16;
                            let next = self.g.nb[c0 as usize][p.dir.min(5) as usize];
                            if next == c0 {
                                p.a = 0;
                            } else {
                                self.drift(c0, p.radius, p.dir.min(5), rep);
                                p.cell = next;
                                p.a -= 1;
                                rep.plate_moves.push((p.id, next));
                            }
                        }
                        if p.a <= 0 {
                            p.dur = 0;
                        }
                    }
                }
                _ => {}
            }
            self.phen[k] = p;
        }
    }

    /// A lightning strike.
    fn strike(&mut self, j: u16, power: i32, rep: &mut Report) {
        rep.strikes.push(j);
        let fuel = self.fuel(j);
        let c = self.m(j);
        c.heat = addi(c.heat, 16);
        c.cloud = add8(c.cloud, 32);
        if fuel >= 24 && c.water < 8 && c.snow < 32 {
            c.fire = c.fire.max((64 + power / 2).min(200) as u8);
            rep.ignitions += 1;
        }
    }

    /// The sea cells exactly `r` hops from `origin` across open water.
    fn wave_front(&self, origin: u16, r: u8) -> Vec<u16> {
        let mut seen = vec![(origin, 0u8)];
        let mut k = 0;
        while k < seen.len() {
            let (i, d) = seen[k];
            k += 1;
            if d >= r {
                continue;
            }
            for &j in &self.g.nb[i as usize] {
                if self.is_sea(j) && !seen.iter().any(|x| x.0 == j) {
                    seen.push((j, d + 1));
                }
            }
        }
        seen.into_iter().filter(|x| x.1 == r).map(|x| x.0).collect()
    }

    /// Move a patch of plate one cell: its columns shift along `dir`, a rift opens behind, a ridge rises ahead.
    fn drift(&mut self, c0: u16, r: u8, dir: u8, rep: &mut Report) {
        let patch: Vec<u16> = self.g.disc(c0, r).into_iter().map(|x| x.0).collect();
        let snap: Vec<(u16, u8, u8, u8)> = patch.iter().map(|&i| (self.g.nb[i as usize][dir as usize], self.height(i), self.material(i), self.fuel(i))).collect();
        let targets: Vec<u16> = snap.iter().map(|x| x.0).collect();
        for &(t, h, mat, fuel) in &snap {
            let d = h as i32 - self.height(t) as i32;
            self.lift(t, d, rep);
            if !self.is_sea(t) {
                let base = self.g.mat[t as usize];
                self.m(t).mat = if mat == base { 0 } else { mat + 1 };
                self.set_fuel(t, fuel as i32);
            }
            let c = self.m(t);
            c.shake = c.shake.max(96);
            c.ruin = add8(c.ruin, 24);
        }
        // Behind: the rift.
        for &i in &patch {
            if !targets.contains(&i) {
                self.lift(i, -2, rep);
                let c = self.m(i);
                c.crack = 255;
                c.lava = add8(c.lava, 32);
                c.shake = c.shake.max(200);
                c.stress = addi(c.stress, 24);
            }
        }
        // Ahead: the ridge.
        for &t in &targets {
            if !patch.contains(&t) {
                let ahead = self.g.nb[t as usize][dir as usize];
                if ahead != t && !targets.contains(&ahead) {
                    self.lift(ahead, 1, rep);
                    let c = self.m(ahead);
                    c.shake = c.shake.max(128);
                    c.stress = addi(c.stress, 32);
                }
            }
        }
    }

    /// Every dirty cell reacts to itself and pushes into its neighbours (read first, then written: no cell sees a
    /// half-updated neighbour).
    fn cells_react(&mut self, rep: &mut Report) {
        let dirty = self.list.clone();
        let mut moves: Vec<(u16, u8, u8, u8)> = Vec::with_capacity(dirty.len());
        let mut next: Vec<Cell> = Vec::with_capacity(dirty.len());
        for &i in &dirty {
            let n = self.react(i, &mut moves, rep);
            next.push(n);
        }
        for (k, &i) in dirty.iter().enumerate() {
            self.cells[i as usize] = next[k];
        }
        for (j, layer, op, amt) in moves {
            let c = self.m(j);
            let v = match layer {
                L_WATER => &mut c.water,
                L_LAVA => &mut c.lava,
                L_FIRE => &mut c.fire,
                L_CLOUD => &mut c.cloud,
                L_SWARM => &mut c.swarm,
                L_PLUME => &mut c.plume,
                _ => continue,
            };
            *v = if op == MAX { (*v).max(amt) } else { v.saturating_add(amt) };
        }
    }

    /// One cell's rules for one sub-step. Returns the cell as it will be; flows to neighbours go to `moves`.
    fn react(&self, i: u16, moves: &mut Vec<(u16, u8, u8, u8)>, rep: &mut Report) -> Cell {
        let g = self.g;
        let c = self.cells[i as usize];
        let mut n = c;
        let h = self.height(i) as i32;
        let sea = h < SEA_LEVEL as i32;
        let fuel = self.fuel(i) as i32;
        let nb = g.nb[i as usize];
        let (wdir, wstr) = self.wind(i);
        let l = level(self.entropy);
        // Entropy quickens every spread: ×1 at rest, ×2.25 at heat death (× the room's `earth_spread_scale`).
        let m = (16 + 4 * l) * self.cfg.spread_q / 16;
        let first = self.sub == 0;
        let mut dfuel = 0i32;
        let rain = if c.cloud >= 112 { (c.cloud as i32 - 96) / 8 } else { 0 };
        let cold = self.cold(i);

        // ---- fire: burns fuel, spreads on fuel downwind, dies in water -----------------------------------------------
        if c.fire > 0 {
            let f = c.fire as i32;
            if sea || c.water >= 8 {
                // A flood quenches fire into steam.
                n.cloud = add8(n.cloud, f / 2);
                n.water = add8(n.water, -(f / 16));
                n.fire = 0;
                rep.quenched += 1;
            } else {
                dfuel -= 2 + f / 32;
                n.heat = addi(n.heat, f / 16);
                n.ruin = add8(n.ruin, 1 + f / 64);
                // Fire feeds a storm: smoke, cloud and charge rise off it.
                n.cloud = add8(n.cloud, f / 16);
                n.charge = add8(n.charge, f / 64);
                n.plume = add8(n.plume, f / 32);
                let mut nf = if fuel < 16 {
                    n.ash = add8(n.ash, 8);
                    f - 64
                } else {
                    f + fuel / 8
                };
                nf -= rain * 4 + c.snow as i32 / 4;
                n.snow = add8(n.snow, -(f / 8));
                n.fire = nf.clamp(0, 255) as u8;
                for k in 0..6u8 {
                    let j = nb[k as usize];
                    if j == i {
                        continue;
                    }
                    let cj = &self.cells[j as usize];
                    let fj = self.fuel(j) as u32;
                    // Burnt ground does not burn again until half of what grew there is back.
                    if cj.fire > 0 || fj < 24 || fj < g.fuel[j as usize] as u32 / 2 || cj.water >= 8 || cj.snow >= 32 {
                        continue;
                    }
                    let wf = if k == wdir { 2 + wstr as u32 } else if (k + 3) % 6 == wdir { 1 } else { 2 };
                    let dry = 64 + cj.heat.max(0) as u32;
                    // Odds in 65536ths = fire × fuel × Entropy × wind × dryness / 256: a full fire (255) in forest (100)
                    // lights a neighbour with odds 0.19 across the wind, 0.29–0.49 down it, 0.10 against it, per
                    // sub-step; a young one (96) a third of that. Heavy cloud (it is raining there) stops it.
                    if cj.cloud >= 160 {
                        continue;
                    }
                    let p16 = (f as u32 * fj * m / 16 * wf * dry / 256).min(65536);
                    if self.chance(S_FIRE, i as u64, j as u64, p16) {
                        moves.push((j, L_FIRE, MAX, (48 + f / 4) as u8));
                        rep.ignitions += 1;
                    }
                }
            }
        }

        // ---- water: flows downhill, pools, evaporates ---------------------------------------------------------------
        if c.water > 0 {
            let w = c.water as i32;
            if sea {
                n.water = (n.water as i32 / 2) as u8;
            } else {
                let surf = h * 32 + w;
                let mut lower: [(u16, i32); 6] = [(0, 0); 6];
                let mut cnt = 0;
                for &j in &nb {
                    if j == i {
                        continue;
                    }
                    let hj = self.height(j) as i32;
                    let sj = if hj < SEA_LEVEL as i32 { SEA_LEVEL as i32 * 32 - 32 } else { hj * 32 + self.cells[j as usize].water as i32 };
                    if sj + 8 < surf {
                        lower[cnt] = (j, surf - sj);
                        cnt += 1;
                    }
                }
                let mut out = 0;
                if cnt > 0 {
                    let cap = w / 3 * m as i32 / 16;
                    let share = (cap / cnt as i32).max(1);
                    for &(j, diff) in &lower[..cnt] {
                        let amt = share.min(diff / 2).min(w - out).min(255);
                        if amt > 0 {
                            out += amt;
                            // The sea takes what reaches it.
                            if !self.is_sea(j) {
                                moves.push((j, L_WATER, ADD, amt as u8));
                            }
                        }
                    }
                }
                let evap = 1 + w / 16 + c.heat.max(0) as i32 / 16 + if c.crack >= 64 { 4 } else { 0 };
                let mut nw = n.water as i32 - out - evap;
                if w >= 64 {
                    n.ruin = add8(n.ruin, w / 64);
                }
                if c.heat <= -48 && nw > 0 {
                    // A blizzard freezes standing water.
                    let fr = nw.min(8);
                    nw -= fr;
                    n.snow = add8(n.snow, fr);
                }
                n.water = nw.clamp(0, 255) as u8;
            }
        }

        // ---- lava: flows downhill, cools to rock that raises the column; lava and water make rock and steam ---------
        if c.lava > 0 {
            let lv = c.lava as i32;
            n.heat = addi(n.heat, lv / 8);
            let mut cool = 2 + lv / 16;
            if sea || c.water >= 8 || c.snow >= 16 {
                cool *= 4;
                n.cloud = add8(n.cloud, (lv / 2).min(64));
                n.water = add8(n.water, -cool);
                n.snow = add8(n.snow, -cool);
                rep.steamed += 1;
            }
            let cool = cool.min(lv);
            let mut out = 0;
            if lv >= 16 {
                let surf = h * 32 + lv;
                for &j in &nb {
                    if j == i {
                        continue;
                    }
                    let sj = self.height(j) as i32 * 32 + self.cells[j as usize].lava as i32;
                    if sj + 8 < surf {
                        let amt = (lv / 8).min((surf - sj) / 4).min(lv - cool - out);
                        if amt > 0 {
                            out += amt;
                            moves.push((j, L_LAVA, ADD, amt as u8));
                        }
                    }
                }
            }
            n.lava = (lv - cool - out).clamp(0, 255) as u8;
            // 32 units of cooled lava are one level of new rock (drawn: each cooled unit is a 1/32 chance).
            if h < H_MAX as i32 && self.chance(S_ROCK, i as u64, 0, (cool as u32 * 2048).min(65536)) {
                n.dh = (h + 1 - g.h[i as usize] as i32) as i8;
                rep.rock_made += 1;
                if h + 1 >= SEA_LEVEL as i32 {
                    n.mat = M_ROCK + 1;
                    if h + 1 == SEA_LEVEL as i32 {
                        rep.land_rose.push(i);
                        n.water = 0;
                    }
                }
            }
            if !sea {
                if fuel >= 24 && c.water < 8 && c.fire == 0 {
                    n.fire = n.fire.max(96);
                    rep.ignitions += 1;
                }
                dfuel -= 16;
                n.ruin = add8(n.ruin, 1 + lv / 16);
                n.ash = add8(n.ash, 2);
            }
        }

        // ---- cloud: rains, charges, rides the wind ---------------------------------------------------------------------
        if c.cloud > 0 {
            let cl = c.cloud as i32;
            let mut nc = n.cloud as i32;
            if rain > 0 {
                nc -= rain;
                if !sea {
                    if cold {
                        n.snow = add8(n.snow, rain);
                    } else {
                        // A storm feeds a flood.
                        n.water = add8(n.water, rain);
                    }
                }
                if c.ash > 0 {
                    n.ash = add8(n.ash, -rain);
                }
            }
            if cl >= 128 {
                n.charge = add8(n.charge, cl / 32);
            }
            let j = nb[wdir as usize];
            if j != i && cl >= 16 {
                let adv = cl * wstr as i32 / 8;
                moves.push((j, L_CLOUD, ADD, adv as u8));
                nc -= adv;
            }
            nc -= 2 + cl / 16;
            if sea && cl >= 96 && g.center[i as usize][1].abs() < 35.0 {
                // Warm sea feeds a storm (never enough to keep one alive by itself).
                nc += 2;
            }
            n.cloud = nc.clamp(0, 255) as u8;
        }
        if c.charge >= 200 {
            // The cloud's own lightning.
            n.charge = add8(n.charge, -160);
            rep.strikes.push(i);
            if fuel >= 24 && c.water < 8 && c.snow < 32 && !sea && c.fire == 0 {
                n.fire = n.fire.max(64);
                rep.ignitions += 1;
            }
        } else if c.charge > 0 && c.cloud < 64 {
            n.charge = add8(n.charge, -8);
        }

        // ---- plume: ash in the air rides the wind, falls, and cools ---------------------------------------------------
        if c.plume > 0 {
            let pl = c.plume as i32;
            let j = nb[wdir as usize];
            let adv = if j != i && pl >= 8 { pl * wstr as i32 / 4 } else { 0 };
            if adv > 0 {
                moves.push((j, L_PLUME, ADD, adv as u8));
            }
            let fall = 1 + pl / 8;
            n.ash = add8(n.ash, if sea { 0 } else { fall / 2 });
            n.heat = addi(n.heat, -(pl / 32));
            n.plume = (pl - adv - fall).clamp(0, 255) as u8;
        }
        if c.ash > 0 {
            if c.ash >= 64 {
                dfuel -= 1;
            }
            if c.ash >= 32 && c.heat > 0 {
                // Ash cools heat.
                n.heat = addi(n.heat, -2);
            }
            if first {
                n.ash = add8(n.ash, -1);
            }
            if sea {
                n.ash = 0;
            }
        }

        // ---- snow: melts into water where it is warm -----------------------------------------------------------------
        if c.snow > 0 {
            let lat = g.center[i as usize][1].abs();
            let mut melt = 0;
            if c.heat > 0 {
                melt += 1 + c.heat as i32 / 8;
            }
            if lat < 50.0 && h < 13 && c.heat > -24 {
                melt += 4;
            }
            let melt = melt.min(c.snow as i32);
            if melt > 0 {
                n.snow = add8(n.snow, -melt);
                n.water = add8(n.water, (melt + 1) / 2);
                rep.melted += 1;
            }
            if sea {
                n.snow = 0;
            }
        }

        // ---- swarm: eats what grows, spreads toward more, dies of fire, cold and hunger -------------------------------
        if c.swarm > 0 {
            let s = c.swarm as i32;
            dfuel -= 1 + s / 32;
            n.ruin = add8(n.ruin, s / 64);
            let mut ns = s - 2;
            if fuel < 16 {
                ns -= 16;
            }
            ns -= c.fire as i32 / 2;
            if c.snow >= 32 || c.heat <= -32 {
                ns -= 32;
            }
            if c.cloud >= 160 {
                ns -= s / 4;
            }
            if sea {
                ns = 0;
            } else if s >= 32 {
                for &j in &nb {
                    if j == i {
                        continue;
                    }
                    let cj = &self.cells[j as usize];
                    if self.fuel(j) >= 24 && (cj.swarm as i32) < s / 2 && !self.is_sea(j) {
                        let amt = s / 8 * m as i32 / 16;
                        moves.push((j, L_SWARM, ADD, amt.min(255) as u8));
                        ns -= amt;
                    }
                }
            }
            n.swarm = ns.clamp(0, 255) as u8;
        }

        // ---- shaking passes; what it broke stays ----------------------------------------------------------------------
        if c.shake > 0 {
            if c.shake >= 48 {
                rep.shaken.push((i, c.shake));
            }
            n.ruin = add8(n.ruin, c.shake as i32 / 16);
            n.crack = add8(n.crack, c.shake as i32 / 64);
            n.shake = if c.shake < 16 { 0 } else { c.shake / 4 };
        }

        // ---- everything relaxes ---------------------------------------------------------------------------------------
        if n.heat != 0 {
            let d = 1 + (n.heat as i32).abs() / 16;
            n.heat = if n.heat > 0 { (n.heat as i32 - d).max(0) } else { (n.heat as i32 + d).min(0) } as i8;
        }
        if first {
            if n.stress != 0 {
                n.stress = if n.stress > 0 { (n.stress - 2).max(0) } else { (n.stress + 2).min(0) };
            }
            if n.crack > 0 && n.crack < 128 {
                n.crack -= 1;
            }
            let quiet = n.fire == 0 && n.lava == 0 && n.water < 64 && n.swarm == 0 && n.shake == 0;
            if n.ruin > 0 && quiet {
                n.ruin -= 1;
            }
            if c.dfuel < 0 && dfuel == 0 && quiet && n.ash < 64 {
                dfuel += 1;
            }
        }
        if dfuel != 0 && !sea {
            let f2 = (fuel + dfuel).clamp(0, 240);
            n.dfuel = (f2 - g.fuel[i as usize] as i32).clamp(-127, 127) as i8;
        }
        if sea {
            n.dfuel = 0;
        }
        n
    }

    /// The end of the beat: phenomena age and end; a spent volcano sleeps.
    fn age(&mut self, rep: &mut Report) {
        let mut keep = Vec::with_capacity(self.phen.len());
        for mut p in std::mem::take(&mut self.phen) {
            match p.kind {
                VOLCANO => {
                    if p.dur > 0 {
                        p.dur -= 1;
                        if p.dur == 0 {
                            p.power = 0;
                            p.a = 12;
                        }
                    } else {
                        p.a -= 1;
                        if p.a <= 0 {
                            rep.ended.push(p);
                            continue;
                        }
                    }
                }
                PLATE_DRIFT | TIDAL_WAVE => {
                    if p.dur == 0 {
                        rep.ended.push(p);
                        continue;
                    }
                }
                _ => {
                    p.dur = p.dur.saturating_sub(1);
                    if p.dur == 0 || (p.power < 8 && matches!(p.kind, STORM | BLIZZARD | FLOOD)) {
                        rep.ended.push(p);
                        continue;
                    }
                }
            }
            keep.push(p);
        }
        self.phen = keep;
    }

    /// How much a cell matters (the cap evicts the lowest first): what is happening on it counts double what was left.
    pub fn weight(&self, i: u16) -> u32 {
        let c = &self.cells[i as usize];
        let now = c.water as u32 / 4 + c.lava as u32 + c.fire as u32 + c.cloud as u32 / 8 + c.plume as u32 / 8 + c.swarm as u32 / 4
            + c.shake as u32 / 4 + c.snow as u32 / 8;
        let left = (c.dh as i32).unsigned_abs() * 200 + if c.mat != 0 { 50 } else { 0 } + c.crack as u32 / 2 + c.ruin as u32 / 4
            + (c.dfuel as i32).unsigned_abs() / 4 + c.ash as u32 / 4 + (c.heat as i32).unsigned_abs() / 8
            + (c.stress as i32).unsigned_abs() / 8 + c.charge as u32 / 8;
        now * 2 + left
    }

    /// The cap: past `max_cells` dirty cells, the calmest settle back to base (down to 15/16 of the cap).
    fn evict(&mut self, rep: &mut Report) {
        let cap = self.cfg.max_cells.max(16) as usize;
        if self.list.len() <= cap {
            return;
        }
        let keep = cap * 15 / 16;
        let mut order: Vec<(u32, u64, u16)> = self.list.iter().map(|&i| (self.weight(i), mix(self.seed ^ S_EVICT ^ ((i as u64) << 8)), i)).collect();
        order.sort_unstable();
        for &(_, _, i) in &order[..self.list.len() - keep] {
            self.cells[i as usize] = Cell::default();
            rep.evicted.push(i);
        }
        rep.evicted.sort_unstable();
        self.normalize();
    }
}

/// The wind at cell `i` on `beat` (see [`Sim::wind`]).
pub fn wind(g: &Grid, seed: u64, beat: u32, i: u16, cloud: u8) -> (u8, u8) {
    let [lon, lat] = g.center[i as usize];
    let a = lat.abs();
    let patch = (((lon + 180.0) / 30.0).floor() as u64) | ((((lat + 90.0) / 30.0).floor() as u64) << 8);
    let d = mix(seed ^ S_WIND.wrapping_mul(0xA24B_AED4_963E_E407) ^ ((beat as u64 / 4) << 20) ^ patch);
    let base = if (30.0..60.0).contains(&a) { 0 } else { 1 };
    let which = match d % 8 {
        0 => 2,
        1 => 3,
        _ => base,
    };
    let strength = 1 + ((d >> 8) & 1) as u8 + (cloud >= 160) as u8;
    (g.dirs[i as usize][which], strength.min(3))
}
