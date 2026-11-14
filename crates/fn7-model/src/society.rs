//! The society: places, groups (Venn sets), clusters (unaddressed populations), memorized characters, demigods, items and
//! relations. All numbers here are updated by rules every beat; the LLM writes only when something is addressed.

use serde::{Deserialize, Serialize};

use crate::hier::{ArmyOrder, Life, Need, Stance};
use crate::ids::*;
use crate::refs::{GroupSet, Ref};

/// `[lon, lat]` degrees (same convention as Final Night and `theatre_toolkit` adapters).
pub type LonLat = [f64; 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// The whole arena (the planet, or one real city).
    Arena,
    /// A district of the arena (on the planet: a world region such as "Western Africa").
    District,
    /// A site inside a district (a cathedral, a port, a market; on the planet: a major city).
    Site,
    /// A district of a planet city (schema 4): where its crowds live ("Docks of Lagos").
    Quarter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: PlaceId,
    pub name: String,
    pub kind: PlaceKind,
    pub parent: Option<PlaceId>,
    pub center: LonLat,
    pub radius_km: f64,
    /// Real-world terrain prose from the toolkit (city / open country / water), fixed at setup.
    pub terrain: String,
    /// Free tags for prompts and rules ("holy", "industrial", "flooded").
    pub tags: Vec<String>,
    /// Society-template archetype key of a district ("docks", "old_town"); `None` for the arena and sites.
    #[serde(default)]
    pub archetype: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    Faction,
    Trade,
    Faith,
    Neighborhood,
    Family,
    /// Followers of a demigod.
    Following,
    /// Created during play (a mob, a cult, a union).
    Emergent,
}

/// One set in the society Venn diagram. Membership lives on the members (`GroupSet`), not here, so a member's
/// position in the diagram is one sorted vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub kind: GroupKind,
    /// How readily news crosses into this group from an overlapping one (0..1).
    pub permeability: f32,
    /// Patron god, if the group is a faith or has one.
    pub patron: Option<GodId>,
    /// Demigod this group follows (for `Following`).
    pub follows: Option<DemigodId>,
    pub founded_beat: u32,
    pub dissolved_beat: Option<u32>,
}

/// Collective mood, each 0..1. Updated by rules from the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Mood {
    pub anger: f32,
    pub fear: f32,
    pub hope: f32,
    pub devotion: f32,
}

impl Mood {
    /// The neutral resting mood (0.25 each); the baseline of clusters in schema-1 states.
    pub const REST: Mood = Mood {
        anger: 0.25,
        fear: 0.25,
        hope: 0.25,
        devotion: 0.25,
    };
}

fn rest_mood() -> Mood {
    Mood::REST
}

/// Starting (and default) maximum health, on Final Night's 0–10 scale.
pub const DEFAULT_MAX_HEALTH: f32 = 10.0;

fn default_max_health() -> f32 {
    DEFAULT_MAX_HEALTH
}

/// An active time-rate change on a body (Time's slow/haste): `factor` < 1 slows, > 1 hastens, until `until_beat`
/// (exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tempo {
    pub factor: f32,
    pub until_beat: u32,
}

/// Combined time-rate at `beat`: the product of the active entries' factors (1.0 when none).
pub fn tempo_factor(tempo: &[Tempo], beat: u32) -> f32 {
    tempo
        .iter()
        .filter(|t| beat < t.until_beat)
        .map(|t| t.factor)
        .product()
}

/// Many unaddressed people with shared traits. Individuals materialize out of it when addressed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cluster {
    pub id: ClusterId,
    pub label: String,
    /// Unmaterialized head count (drops by one per memorization, and with deaths).
    pub size: u32,
    pub place: PlaceId,
    pub traits: Vec<String>,
    pub needs: Vec<String>,
    pub mood: Mood,
    /// The mood it relaxes back toward (its template's mood, set at setup).
    #[serde(default = "rest_mood")]
    pub baseline: Mood,
    pub groups: GroupSet,
    /// Per-god worship share (sparse, sorted by god).
    pub worship: Vec<(GodId, f32)>,
    /// Per-demigod allegiance -1..1 (sparse, sorted by demigod).
    pub allegiance: Vec<(DemigodId, f32)>,
    /// How many individuals have been materialized from it (the per-person seed index).
    pub materialized: u32,
    pub seed: u64,
    /// Set when this cluster is an army a hand raised: it has a position, orders and a march.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub army: Option<Army>,
    /// Unmet needs (schema 4; updated by the society rules every beat).
    #[serde(default)]
    pub need: Need,
    /// What the crowd is doing as a whole (schema 4).
    #[serde(default)]
    pub stance: Stance,
    /// Its leader, once someone stood up (a ringleader, a prophet, a notable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<CharId>,
    /// Cells instantiated out of it (live and ended), in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<CellId>,
    /// Deaths so far.
    #[serde(default)]
    pub dead: u32,
}

/// An army: a cluster raised from a crowd by a demigod's hand. It marches across the globe toward `dest` (rules move
/// it [`ARMY_KM_PER_BEAT`] a beat), fights rival armies it meets and hits rival demigods it reaches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Army {
    pub demigod: DemigodId,
    /// Standing orders, in the player's words (shown to its owner only).
    pub orders: String,
    pub pos: LonLat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest: Option<LonLat>,
    /// What it marches on: a place, or a demigod it hunts (re-aimed each beat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Ref>,
    pub raised_beat: u32,
    /// The crowd it was raised from.
    pub from: ClusterId,
    /// The command chain (schema 4): the general the demigod's orders go to, the captains under them, the units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub general: Option<CharId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captains: Vec<CharId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<CellId>,
    /// 0..1: will to fight.
    #[serde(default = "one")]
    pub morale: f32,
    /// 0..1: food and kit.
    #[serde(default = "one")]
    pub supply: f32,
    #[serde(default)]
    pub stance: Stance,
    /// Answers to no demigod (its general mutinied); `demigod` is who it used to serve.
    #[serde(default)]
    pub rogue: bool,
    /// The order it is acting on, as the general understood it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<ArmyOrder>,
    /// 0..1: losses, hunger, defeats, hated orders. High strain is a decision point (mutiny, defection).
    #[serde(default)]
    pub strain: f32,
    #[serde(default)]
    pub battles: u32,
    #[serde(default)]
    pub losses: u32,
    /// Raised by the society itself (a volunteer host), not by a hand.
    #[serde(default)]
    pub volunteer: bool,
}

fn one() -> f32 {
    1.0
}

/// How far an army marches per beat.
pub const ARMY_KM_PER_BEAT: f64 = 900.0;

/// One standing order of a hand: the last act its player asked for, which the hand keeps carrying out (by rules, no LLM)
/// in beats when the player says nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandingOrder {
    pub hand: Hand,
    pub verb: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceId>,
    pub magnitude: u8,
    /// The words it came from.
    pub span: String,
    pub since: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Materialized from a cluster when first addressed.
    Materialized { cluster: ClusterId, index: u32 },
    /// Spawned by a god as a paladin.
    Paladin { god: GodId },
    /// Created outright (summoned, born, built).
    Created,
    /// Seeded at match setup (notables).
    Seeded,
}

/// A memorized individual. Permanent once created.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Character {
    pub id: CharId,
    pub name: String,
    /// Other names and descriptions the resolver has matched to this person ("the baker who saw the fire").
    pub aliases: Vec<String>,
    pub origin: Origin,
    pub place: PlaceId,
    pub pos: LonLat,
    pub traits: Vec<String>,
    pub groups: GroupSet,
    pub health: f32,
    /// Lowered by `Price { kind: Life }`; heals cap here.
    #[serde(default = "default_max_health")]
    pub max_health: f32,
    pub alive: bool,
    pub allegiance: Vec<(DemigodId, f32)>,
    pub serves: Option<GodId>,
    /// The possession subagent steering this character, if any.
    pub possessed_by: Option<AgentId>,
    pub memorized_beat: u32,
    pub memorized_by: Option<Ref>,
    /// Active slow/haste effects.
    #[serde(default)]
    pub tempo: Vec<Tempo>,
    /// The person's sheet in the society (schema 4): role, rank, competence, ambition, cruelty, courage, grudges,
    /// needs, goal. Set for everyone the simulation produced (and anyone given a role).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub life: Option<Box<Life>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Hand {
    Left,
    Right,
}

/// A demigod: one body on the globe. Schema ≤ 4: one per seat. Schema 5: two per seat, the player's Left and Right
/// hands ([`Demigod::side`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Demigod {
    pub id: DemigodId,
    pub seat: u8,
    pub name: String,
    pub pos: LonLat,
    pub place: PlaceId,
    pub health: f32,
    /// Lowered by `Price { kind: Life }`; heals cap here.
    #[serde(default = "default_max_health")]
    pub max_health: f32,
    pub alive: bool,
    pub eliminated_beat: Option<u32>,
    pub cause: Option<String>,
    /// `[left, right]` hand orchestrators.
    pub hands: [AgentId; 2],
    /// The `Following` group of this demigod.
    pub following: GroupId,
    pub items: Vec<ItemId>,
    /// Echo (duplicator) charges left.
    pub echo_charges: u32,
    /// Lifetime words spoken to the hands (the speed mechanic's input).
    pub words_spoken: u64,
    /// Active slow/haste effects.
    #[serde(default)]
    pub tempo: Vec<Tempo>,
    /// Lightning bolts: the god-energy unit. Throwing or paying one costs it; every other demigod gains 2.
    #[serde(default)]
    pub bolts: u32,
    /// Beats at which this demigod slowed time (Time's penalty grows with recent slows).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slows: Vec<u32>,
    /// The hands' standing orders (continued by rules in beats when the player is silent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orders: Vec<StandingOrder>,
    /// The region (planet) or district (city) it started in: its home turf, fixed for the match (the diorama's
    /// theme). Absent in states from before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<PlaceId>,
    /// Schema 5: which of its player's two hands this body is. Each player has two demigods (Left and Right), each a
    /// row here with its own health, position, items and elimination; both share the seat (and its purse of bolts).
    /// `None` in schema-4 states (one demigod per seat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Hand>,
    /// Schema 5: the player's display name (both of a player's demigods share it). Empty in schema-4 states, where
    /// the demigod's `name` is the player's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub player: String,
    /// Rules 0.7.0: sparks, the mini-power currency (a per-player purse like `bolts`, mirrored on both demigods).
    /// 0 (absent) in schema-4 states.
    #[serde(default, skip_serializing_if = "crate::society::is_zero_u32")]
    pub sparks: u32,
}

pub(crate) fn is_zero_u32(x: &u32) -> bool {
    *x == 0
}

impl Demigod {
    /// The player's display name: [`Demigod::player`], or the demigod's own name before schema 5.
    pub fn player_name(&self) -> &str {
        if self.player.is_empty() {
            &self.name
        } else {
            &self.player
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ItemOrigin {
    Starting,
    Summoned,
    Found,
    Echoed { from: ItemId },
    Granted { god: GodId },
}

/// Items are never lost (Final Night rule): the holder list only grows, and everything ends on a pile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: ItemId,
    pub name: String,
    pub description: String,
    pub origin: ItemOrigin,
    pub created_beat: u32,
    pub created_by: Option<Ref>,
    pub holders: Vec<Ref>,
    pub uses: u32,
    /// Schema 5: what the item engine knows about it (kind, tier, count, state, provenance, custody). Flattened, so
    /// schema-4 items load with the defaults (one held object of kind `other`, tier `trinket`) and serialize as before.
    #[serde(flatten, default, skip_serializing_if = "crate::manifest::ItemExt::is_plain")]
    pub ext: crate::manifest::ItemExt,
}

/// Pairwise feelings, stored sparse and sorted by `(a, b)`. Directed: `a` about `b`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub a: Ref,
    pub b: Ref,
    pub trust: f32,
    pub fear: f32,
    /// Positive: `a` owes `b`.
    pub debt: f32,
    pub loyalty: f32,
    pub updated_beat: u32,
}
