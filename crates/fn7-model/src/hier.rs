//! The hierarchical society (schema 4): planet → region → city → district → crowd → cell → person, every level
//! simulated by rules every beat (`fn7_society::sim`).
//!
//! - **Places are the upper nodes.** Each place has one [`Node`] (`World::nodes[place]`) with the rolled-up population,
//!   mood, needs, allegiance, worship, supply, stance and an optional leader. On the planet: the arena (Earth), regions
//!   (`PlaceKind::District`), cities (`PlaceKind::Site`) and city districts (`PlaceKind::Quarter`); in a city arena the
//!   arena, its districts and sites.
//! - **Crowds** are the existing clusters (`cl:`), living in a district (planet) or district (city).
//! - **Cells** (`ce:`, [`Cell`]) are 10–500 people instantiated lazily out of a crowd when it acts: a gang when it is
//!   angry, a congregation when it worships, a crew when it loots, squads when an army is raised.
//! - **Mobs** (`mb:`, [`Mob`]) are cells merged under a ringleader: they move, riot, loot, attack and dissolve.
//! - **People** are characters (`ch:`); the ones the simulation produced carry a [`Life`] sheet (role, rank, competence,
//!   ambition, cruelty, courage, grudges, needs, goal). Their loyalty per demigod is `Character::allegiance`.
//! - **Armies** keep their cluster (`Cluster::army`) and gain a command chain: demigod → general → captains → units.
//!
//! Aggregates roll up (sums, size-weighted means); effects on a node distribute down by rules. All of it is plain data
//! here; the rules live in `fn7-society`.

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::refs::Ref;
use crate::society::{LonLat, Mood};

/// A level of the hierarchy (a node's or a person's rank level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Level {
    Planet,
    Region,
    City,
    District,
    Crowd,
    Cell,
    #[default]
    Person,
}

/// Unmet needs, each 0 (met) .. 1 (desperate).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Need {
    /// Hunger: supply short of what the place needs.
    #[serde(default)]
    pub food: f32,
    /// Danger: war, mobs, disasters, Time's blight outside the arena.
    #[serde(default)]
    pub safety: f32,
    /// Spiritual hunger: hardship with no god or demigod answering.
    #[serde(default)]
    pub faith: f32,
}

/// What a node, crowd, cell, mob or army is doing as a whole (derived each beat from mood and orders).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Stance {
    #[default]
    Calm,
    Restless,
    Rioting,
    Fleeing,
    Worshipping,
    Fighting,
    Marching,
    Holding,
    Besieging,
    Raiding,
    Retreating,
    /// An army that answers to no demigod (its general mutinied).
    Rogue,
}

/// One upper node of the hierarchy: the rolled-up state of a place (index = the place's id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub place: PlaceId,
    pub level: Level,
    /// People living here now (crowds, not armies passing through).
    pub population: u64,
    /// Population at setup (load = population / base).
    pub base: u64,
    pub mood: Mood,
    pub need: Need,
    /// Size-weighted allegiance per demigod (-1..1), sparse, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allegiance: Vec<(DemigodId, f32)>,
    /// Size-weighted worship share per god (0..1), sparse, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worship: Vec<(GodId, f32)>,
    /// Food and goods stock: 1 = normal. Kept per city (a city arena: per district); upper nodes roll it up.
    pub supply: f32,
    pub stance: Stance,
    /// Who leads here (a governor, a ringleader holding the streets, a general occupying it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<CharId>,
    /// Deaths here so far.
    #[serde(default)]
    pub dead: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CellKind {
    /// Angry young people with a grievance (mob material).
    Gang,
    /// An army unit under a captain.
    Squad,
    /// Worshippers around a prophet.
    Congregation,
    /// Looters, smugglers, a work crew.
    Crew,
    /// Self-armed locals.
    Militia,
}

/// A cell: 10–500 people instantiated out of a crowd when it acts. Its people are still counted in the crowd's size
/// (an army's squads are shares of the army's size).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub id: CellId,
    pub kind: CellKind,
    pub name: String,
    /// The crowd it came out of (an army cluster for squads).
    pub crowd: ClusterId,
    pub size: u32,
    pub mood: Mood,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<CharId>,
    pub stance: Stance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mob: Option<MobId>,
    pub formed: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<u32>,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum MobAction {
    #[default]
    Gathering,
    Rioting,
    Looting,
    /// Attacking a crowd, an army or a demigod (`Mob::target`).
    Attacking,
    /// Moving to another district or city.
    Marching,
    Dispersed,
}

/// A mob: cells merged under a ringleader. It moves, riots, loots, attacks and dissolves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mob {
    pub id: MobId,
    pub name: String,
    /// The city it is in, and the district (a city arena: the district twice).
    pub city: PlaceId,
    pub place: PlaceId,
    pub pos: LonLat,
    pub cells: Vec<CellId>,
    pub size: u32,
    pub anger: f32,
    pub fear: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ringleader: Option<CharId>,
    /// What it is angry at, if anything (a crowd, an army, a demigod, a person, a place).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Ref>,
    pub action: MobAction,
    /// Why it formed, one line.
    pub cause: String,
    pub formed: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<u32>,
    #[serde(default)]
    pub casualties: u32,
    #[serde(default)]
    pub kills: u32,
    /// Demigod it rallies behind (from the crowds' allegiance), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<DemigodId>,
}

/// A person's part in the society.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Role {
    #[default]
    Commoner,
    Notable,
    Ringleader,
    General,
    Captain,
    Soldier,
    Hero,
    Coward,
    Witness,
    Prophet,
    Paladin,
    Deserter,
    Avenger,
}

/// Where a person stands: a level of the hierarchy, the node or unit they hold rank in, and a title.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Rank {
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<Ref>,
    /// "General of Ash's Host of Cairo", "ringleader of the Lagos bread mob".
    pub title: String,
}

/// What a petty human is after (pursued by rules each beat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "goal", content = "on")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Goal {
    #[default]
    Survive,
    /// Ambition: a higher rank.
    Promotion,
    /// A grudge: get even with someone.
    Revenge(Ref),
    /// Fear: get away.
    Flee,
    /// Spread the faith.
    Preach,
    /// Grab what can be grabbed.
    Loot,
    /// Serve the master well.
    Serve,
}

/// A simulated person's sheet (characters the society produced; addressed ones may get one when given a role).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Life {
    pub role: Role,
    pub rank: Rank,
    /// 0..1: how good they are at their job.
    pub competence: f32,
    /// 0..1: how much they want more.
    pub ambition: f32,
    /// -1 (merciful) .. 1 (cruel).
    pub cruelty: f32,
    /// 0..1.
    pub courage: f32,
    /// People, armies, demigods they hold a grudge against.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grudges: Vec<Ref>,
    pub need: Need,
    pub goal: Goal,
    /// How they entered the story, and when.
    pub emerged_as: Role,
    pub emerged: u32,
    #[serde(default)]
    pub deeds: u32,
    #[serde(default)]
    pub kills: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<CellId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub army: Option<ClusterId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mob: Option<MobId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub died: Option<u32>,
}

/// How a general read an order (or decided at a decision point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Reading {
    #[default]
    Obey,
    /// Understood it wrong (vague orders, low competence): acts on something else.
    Misread,
    /// Drags their feet (holds where they are).
    Stall,
    /// Won't do it.
    Refuse,
    /// Turns the army against its demigod: it goes rogue.
    Mutiny,
    /// Takes the army over to a rival demigod.
    Defect,
}

/// A general's judgment: the rules' or the general mind's (`fn7-agents::general`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Judgment {
    pub reading: Reading,
    /// What the army goes after (a place, a demigod, an army, a mob).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Ref>,
    pub stance: Stance,
    /// The general's one line.
    #[serde(default)]
    pub speech: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defect_to: Option<DemigodId>,
}

/// The order an army is acting on, as its general understood it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmyOrder {
    /// The demigod's words (empty for the general's own initiative).
    pub text: String,
    pub by: DemigodId,
    pub beat: u32,
    pub judgment: Judgment,
    /// Why this judgment was made: `order`, `engage`, `strain`, `initiative`.
    pub why: String,
    /// The general mind decided it (vs the rules).
    #[serde(default)]
    pub llm: bool,
}

/// One side of a battle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleSide {
    /// The army, crowd or mob that fought (a Ref: `cl:` army/crowd or `mb:`).
    pub who: Ref,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demigod: Option<DemigodId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub general: Option<CharId>,
    pub before: u32,
    pub losses: u32,
}

/// A battle the rules resolved (the last [`BATTLES_KEPT`] stay in state; every one is a `battle` event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Battle {
    pub id: u32,
    pub beat: u32,
    pub at: LonLat,
    pub place: PlaceId,
    pub sides: [BattleSide; 2],
    /// 0 or 1 (the index into `sides`); `None` = a bloody draw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heroes: Vec<CharId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cowards: Vec<CharId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallen: Vec<CharId>,
    /// One line.
    pub text: String,
}

/// Battles kept in live state.
pub const BATTLES_KEPT: usize = 24;

/// Kinds of society events: each is a ledger row (`Effect::Society`, verb = the kind) and a trace event of that kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SocKind {
    MobFormed,
    MobDissolved,
    RingleaderEmerged,
    GeneralAppointed,
    OrderReceived,
    OrderMisread,
    Battle,
    Casualties,
    Hero,
    Coward,
    Mutiny,
    Defection,
    Migration,
    CultFounded,
    PersonEmerged,
    Promotion,
    Death,
    Riot,
    Looting,
    Clash,
    Desertion,
    Hardship,
    Revenge,
    HostRaised,
    Preaching,
}

impl SocKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SocKind::MobFormed => "mob_formed",
            SocKind::MobDissolved => "mob_dissolved",
            SocKind::RingleaderEmerged => "ringleader_emerged",
            SocKind::GeneralAppointed => "general_appointed",
            SocKind::OrderReceived => "order_received",
            SocKind::OrderMisread => "order_misread",
            SocKind::Battle => "battle",
            SocKind::Casualties => "casualties",
            SocKind::Hero => "hero",
            SocKind::Coward => "coward",
            SocKind::Mutiny => "mutiny",
            SocKind::Defection => "defection",
            SocKind::Migration => "migration",
            SocKind::CultFounded => "cult_founded",
            SocKind::PersonEmerged => "person_emerged",
            SocKind::Promotion => "promotion",
            SocKind::Death => "death",
            SocKind::Riot => "riot",
            SocKind::Looting => "looting",
            SocKind::Clash => "clash",
            SocKind::Desertion => "desertion",
            SocKind::Hardship => "hardship",
            SocKind::Revenge => "revenge",
            SocKind::HostRaised => "host_raised",
            SocKind::Preaching => "preaching",
        }
    }
    pub const ALL: [SocKind; 25] = [
        SocKind::MobFormed,
        SocKind::MobDissolved,
        SocKind::RingleaderEmerged,
        SocKind::GeneralAppointed,
        SocKind::OrderReceived,
        SocKind::OrderMisread,
        SocKind::Battle,
        SocKind::Casualties,
        SocKind::Hero,
        SocKind::Coward,
        SocKind::Mutiny,
        SocKind::Defection,
        SocKind::Migration,
        SocKind::CultFounded,
        SocKind::PersonEmerged,
        SocKind::Promotion,
        SocKind::Death,
        SocKind::Riot,
        SocKind::Looting,
        SocKind::Clash,
        SocKind::Desertion,
        SocKind::Hardship,
        SocKind::Revenge,
        SocKind::HostRaised,
        SocKind::Preaching,
    ];
    /// The kind of a verb string, if it is one.
    pub fn of(verb: &str) -> Option<SocKind> {
        SocKind::ALL.into_iter().find(|k| k.as_str() == verb)
    }
}

/// Running totals of the society's own action (cheap counters for payloads and reports).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SocietyTotals {
    #[serde(default)]
    pub mobs_formed: u32,
    #[serde(default)]
    pub mobs_dissolved: u32,
    #[serde(default)]
    pub people_emerged: u32,
    #[serde(default)]
    pub migrations: u32,
    #[serde(default)]
    pub migrants: u64,
    #[serde(default)]
    pub cults: u32,
    #[serde(default)]
    pub battles: u32,
    #[serde(default)]
    pub clashes: u32,
    #[serde(default)]
    pub deaths: u64,
    #[serde(default)]
    pub mutinies: u32,
    #[serde(default)]
    pub defections: u32,
    #[serde(default)]
    pub promotions: u32,
    #[serde(default)]
    pub deserters: u64,
    #[serde(default)]
    pub hosts: u32,
}

/// Which nodes ran at which fidelity in one beat (level of detail): everything ran by rules; these got LLM attention or
/// were in focus.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Fidelity {
    /// Nodes/units stepped by rules this beat (count; every node runs every beat).
    pub rules: u32,
    /// In focus this beat (seat zoom/focus, addressed): their events are kept in detail and their minds may use the LLM.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus: Vec<Ref>,
    /// Got an LLM call this beat (generals addressed or at a decision point).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub llm: Vec<Ref>,
    /// Society events this beat, and people who emerged.
    #[serde(default)]
    pub events: u32,
    #[serde(default)]
    pub emerged: u32,
}
