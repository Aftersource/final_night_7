//! The Earth simulator's part of the match state (state schema 13, room setting `earth_sim_enabled`;
//! docs/EARTH_SIM.md). The rules live in `fn7-plugin::earth`; this is only what is stored.
//!
//! The simulation is a pure function of (the match seed, the settings, the ordered [`EarthInput`]s and the Entropy
//! meter at each beat). The state keeps that input log, the field as it stands now (the phenomena and the sparse
//! "dirty" cells: the ones that differ from the derived base terrain) and a keyframe every K beats, so any beat can be
//! rebuilt by re-simulating at most K beats. Nothing derivable is stored: no base terrain, no wind, no top materials.

use serde::{Deserialize, Serialize};

/// The state schema the Earth simulator is gated on ([`crate::World::earth_rules`]).
pub const EARTH_SCHEMA: u32 = 13;

/// The state schema the earth underfoot is gated on ([`crate::World::earth_prompt_rules`]): from it, every LLM prompt
/// that acts in a place carries one short line of what the field holds there now (docs/EARTH_SIM.md §12).
pub const EARTH_PROMPT_SCHEMA: u32 = 17;

/// The state schema what the earth did, thing by thing, is gated on ([`crate::World::earth_effects_rules`]): from it
/// each beat's step names what happened to which thing where (`earth.effects[]`: forests burned, crowds fled to
/// shelters, shelters destroyed, people died ...), crowds really move to shelters and shelters really fall, and the
/// state keeps them (docs/EARTH_SIM.md §13). 18 and 19 are taken by other branches.
pub const EARTH_EFFECTS_SCHEMA: u32 = 20;

/// One beat's effects (schema 20): the list the beat's `earth.effects` carried, worst first, capped, packed
/// (`fn7-plugin::earth::effects`), with how many there were before the cap and how many of each `what`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EarthEffects {
    pub beat: u32,
    /// Effects before the cap.
    pub total: u32,
    /// Per `what` (in `fn7-plugin::earth::effects::WHATS` order), before the cap.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counts: Vec<u32>,
    /// The kept effects, packed (base64).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rows: String,
}

/// People of a crowd who fled to a shelter (schema 20). Never deleted: `until` closes it (they went home, the shelter
/// fell, they moved on), so the shelters at any past beat are the entries open then.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Refuge {
    /// The crowd (cluster index).
    pub crowd: u32,
    pub count: u32,
    /// The villages-level hex they fled from.
    pub home: String,
    /// The houses-level hex of the building they shelter in.
    pub at: String,
    /// The settlement (place index) the building belongs to.
    pub place: u32,
    pub since: u32,
    /// The beat they left it (0: still there).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub until: u32,
    /// The phenomenon they fled from.
    pub cause: u32,
}

fn is_zero(x: &u32) -> bool {
    *x == 0
}

/// A building the field destroyed (schema 20): the wreck stays.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Wreck {
    /// The houses-level hex.
    pub at: String,
    /// The settlement (place index).
    pub place: u32,
    pub beat: u32,
    /// How it fell: `destroyed`, `burned`, `buried` or `swept`.
    pub how: String,
    /// The phenomenon that did it (0: ruin with no live cause).
    pub cause: u32,
}

/// One live phenomenon. Stored as a 16-element array (in the order of the fields) to keep snapshots small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "PhenomenonRow", into = "PhenomenonRow")]
pub struct Phenomenon {
    /// 1-based, in spawn order.
    pub id: u32,
    /// The kind code (`fn7-plugin::earth::Kind`).
    pub kind: u8,
    /// Where it is now: an index into the regions-level grid.
    pub cell: u16,
    /// Where it was cast, in thousandths of a degree (a mini is much smaller than a cell).
    pub lon_e3: i32,
    pub lat_e3: i32,
    /// The beat it began.
    pub born: u32,
    /// Its power now (16 per point of magnitude at the start).
    pub power: i16,
    /// Its reach in cells (0 = one cell).
    pub radius: u8,
    /// Its heading: a hex direction 0..=5, or 6 for none.
    pub dir: u8,
    /// Beats left.
    pub dur: u8,
    /// The seat that cast it (255 = nobody: the world's own).
    pub owner: u8,
    /// Why it exists (`fn7-plugin::earth::Cause`).
    pub cause: u8,
    /// The phenomenon that set it off (0 = none).
    pub parent: u32,
    /// Two counters whose meaning depends on the kind (a wave's front, a plate's moves left, a volcano's sleep).
    pub a: i16,
    pub b: i16,
    /// 0 mini, 1 mega, 2 god-tier, 3 the world's own.
    pub tier: u8,
}

type PhenomenonRow = (u32, u8, u16, i32, i32, u32, i16, u8, u8, u8, u8, u8, u32, i16, i16, u8);

impl From<PhenomenonRow> for Phenomenon {
    fn from(r: PhenomenonRow) -> Self {
        Phenomenon { id: r.0, kind: r.1, cell: r.2, lon_e3: r.3, lat_e3: r.4, born: r.5, power: r.6, radius: r.7, dir: r.8,
                     dur: r.9, owner: r.10, cause: r.11, parent: r.12, a: r.13, b: r.14, tier: r.15 }
    }
}

impl From<Phenomenon> for PhenomenonRow {
    fn from(p: Phenomenon) -> Self {
        (p.id, p.kind, p.cell, p.lon_e3, p.lat_e3, p.born, p.power, p.radius, p.dir, p.dur, p.owner, p.cause, p.parent, p.a,
         p.b, p.tier)
    }
}

/// One input to the simulation: something was cast (or a ramification landed) at a beat. Stored as a 10-element array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "InputRow", into = "InputRow")]
pub struct EarthInput {
    pub beat: u32,
    pub kind: u8,
    pub cell: u16,
    pub lon_e3: i32,
    pub lat_e3: i32,
    /// The landed power's magnitude, 1..=10 (already scaled by the fault rule for a mega).
    pub energy: u8,
    /// Its reach in cells.
    pub radius: u8,
    pub owner: u8,
    pub cause: u8,
    pub tier: u8,
}

type InputRow = (u32, u8, u16, i32, i32, u8, u8, u8, u8, u8);

impl From<InputRow> for EarthInput {
    fn from(r: InputRow) -> Self {
        EarthInput { beat: r.0, kind: r.1, cell: r.2, lon_e3: r.3, lat_e3: r.4, energy: r.5, radius: r.6, owner: r.7,
                     cause: r.8, tier: r.9 }
    }
}

impl From<EarthInput> for InputRow {
    fn from(i: EarthInput) -> Self {
        (i.beat, i.kind, i.cell, i.lon_e3, i.lat_e3, i.energy, i.radius, i.owner, i.cause, i.tier)
    }
}

/// The field at a beat, to restart the simulation from: the dirty cells as a delta against the keyframe before it
/// (the first against the bare base terrain), and the phenomena then.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EarthKeyframe {
    pub beat: u32,
    /// Packed cells that differ from the previous keyframe (base64; `fn7-plugin::earth::codec`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cells: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phenomena: Vec<Phenomenon>,
    pub next_id: u32,
    pub tension: u16,
}

/// `World::dm.earth`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EarthState {
    /// The last beat stepped.
    pub beat: u32,
    pub next_id: u32,
    /// Stress built up along every fault by Entropy (released by the world's own quakes).
    pub tension: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phenomena: Vec<Phenomenon>,
    /// The dirty cells now, packed (base64).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cells: String,
    /// Every input so far, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<EarthInput>,
    /// The Entropy meter (rounded, 0..=100) the simulation read at each stepped beat, with the beat it belongs to
    /// implied by the position: `entropy[i]` is beat `first_beat + i`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entropy: Vec<u8>,
    /// The first beat stepped (1 for a match that began with the simulator on).
    pub first_beat: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyframes: Vec<EarthKeyframe>,
    /// Cells settled back to base by the cap so far.
    #[serde(default)]
    pub evicted: u32,
    /// Schema 20: what the earth did each beat, thing by thing (beats with nothing are absent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<EarthEffects>,
    /// Schema 20: crowds' people in shelters, past and present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refuges: Vec<Refuge>,
    /// Schema 20: buildings the field destroyed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wrecks: Vec<Wreck>,
    /// Schema 20: phenomena that ended in the last 32 beats, `[id, kind, cell, cast magnitude, seat, beat ended]`, so
    /// what they left behind (a fire that spread on) is still theirs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ended: Vec<(u32, u8, u16, u8, u8, u32)>,
}
