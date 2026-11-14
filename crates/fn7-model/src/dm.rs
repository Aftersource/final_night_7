//! The Dungeon Master's state (state schema 6, rules 0.8.0): the hierarchical place × time memory and the eight
//! features it governs (docs/DM_RULEBOOK.md).
//!
//! **The memory.** Every decision the DM makes about a place at a time is a [`DmEntry`]: its place key at its own
//! scope (a people- or house-level hex cell → a settlement → a region → the world) with its ancestors, its beat, a
//! kind, an optional query key, and the result. Lookups ask "what was decided about place P (or any ancestor or
//! descendant) at or before beat B" ([`Dm::recall`]); a decision with a query key is made once and reused ever after
//! ([`Dm::decided`]): the same query gives the same die, an uncovered encounter stays uncovered, a grown village keeps
//! its size. Append-only.
//!
//! **The features.** Growing settlements ([`Settlement`]), each demigod's story piece ([`Story`]), the visible d20
//! ([`DiceRoll`]), map encounters ([`Encounter`]), loot left behind ([`LootPile`]), day and night (derived from the
//! beat; recorded as memory), paths that wear into roads ([`Trail`]) and shrines ([`Shrine`]).
//!
//! Everything here is plain data; the rules live in `fn7-plugin::dm`. An empty [`Dm`] serializes as nothing, so
//! states before schema 6 stay byte-identical.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::*;
use crate::society::LonLat;

/// The schema the DM's rules switch on at.
pub const DM_SCHEMA: u32 = 6;

/// The schema challenge mode and the other rules 0.8.1 changes switch on at.
pub const CHALLENGE_SCHEMA: u32 = 7;

/// The schema exact resume switches on at (an open duel's whole timeline and the delivery mark live in the state).
pub const RESUME_SCHEMA: u32 = 8;

/// Scars kept per demigod (the newest).
pub const SCARS_KEPT: usize = 12;

/// A place key's scope, finest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Scope {
    Cell,
    Settlement,
    Region,
    World,
}

/// The world's place key.
pub const WORLD_KEY: &str = "world";

/// One decision the DM made about a place at a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DmEntry {
    pub id: u32,
    pub beat: u32,
    pub scope: Scope,
    /// The place key at `scope`: a hex cell key (`"<face>:<level>:<q>:<r>"`), a `pl:` ref, or `"world"`.
    pub place: String,
    /// The ancestors' keys, nearest first (a cell: its settlement if any, its region, `"world"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub up: Vec<String>,
    /// What was decided: `dice.rolled`, `settlement.level`, `encounter.placed`, `encounter.revealed`, `story.scar`,
    /// `story.wardrobe`, `story.followers`, `loot.dropped`, `loot.taken`, `road.formed`, `road.faded`, `shrine.built`,
    /// `shrine.level`, `time.phase`...
    pub kind: String,
    /// The question this answers, when it is one the DM may be asked again (`"encounter:3"`, `"check:12"`): asked
    /// again, the DM reuses this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Shown by `game.memory` and fed to the LLM DM (a hidden encounter's placement is not public).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub public: bool,
    /// One line, no engine ids (what the LLM DM reads).
    pub text: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

impl DmEntry {
    /// Is this entry about `key` or anything above or below it? `ancestors` are `key`'s own ancestors.
    pub fn concerns(&self, key: &str, ancestors: &[String]) -> bool {
        self.place == key || self.up.iter().any(|u| u == key) || ancestors.contains(&self.place)
    }
}

/// The verdict of a d20 roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DiceVerdict {
    CritFail,
    Fail,
    Success,
    CritSuccess,
}

impl DiceVerdict {
    pub fn ok(self) -> bool {
        matches!(self, DiceVerdict::Success | DiceVerdict::CritSuccess)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            DiceVerdict::CritFail => "crit_fail",
            DiceVerdict::Fail => "fail",
            DiceVerdict::Success => "success",
            DiceVerdict::CritSuccess => "crit_success",
        }
    }
}

/// One d20 roll (recorded in memory under its query; the same query gives the same roll).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiceRoll {
    /// The face, 1..=20.
    pub roll: u8,
    pub modifier: i32,
    pub total: i32,
    pub target_dc: i32,
    pub verdict: DiceVerdict,
    /// A second seeded draw in [0, 1) for the rules that need finer chance under one face (a natural 1's confirm).
    #[serde(default)]
    pub confirm: f64,
}

/// A settlement's growth state (a city on the planet, a site in a city arena).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settlement {
    pub place: PlaceId,
    /// 1 hamlet .. 5 great city; 6+ legendary.
    pub level: u8,
    /// People living there (its node's population).
    pub size: u64,
    /// 0..1: supply, hope and safety.
    pub prosperity: f32,
    /// The level the rules aim at now (it moves one step at a time).
    pub target: u8,
    /// The beat its level last changed.
    pub since_beat: u32,
    /// People-level cell at its centre.
    pub cell: String,
}

/// A demigod's wardrobe stage names, 0..=4.
pub const WARDROBE: [&str; 5] = ["plain", "marked", "adorned", "regalia", "mythic"];

/// One scar: when, how and where.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scar {
    pub beat: u32,
    pub cause: String,
    pub place: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cell: String,
}

/// A demigod's story piece: what its playing piece shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub demigod: DemigodId,
    /// Its Following's headcount, last beat.
    pub followers: u64,
    /// 0 plain .. 4 mythic (never goes down).
    pub wardrobe: u8,
    /// Deeds so far (the wardrobe's input): landed public acts, encounters won, shrines built.
    pub deeds: f32,
    /// `ragged` (health 3 or less) or `scorched` (hit by a power or fire this beat), else none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scars: Vec<Scar>,
    /// Its tales: the deeds the DM told about it (summonings), the newest [`TALES_KEPT`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tales: Vec<Tale>,
    /// The beat it was last summoned (the cooldown's clock).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_summon: Option<u32>,
}

/// Tales kept per demigod (the newest).
pub const TALES_KEPT: usize = 8;

/// One deed the DM told about a demigod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tale {
    pub beat: u32,
    pub kind: String,
    pub text: String,
}

/// What lies on an encounter hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum EncounterKind {
    Monster,
    Treasure,
    Trap,
    Stranger,
    /// Night only.
    Undead,
    /// Night only.
    Ghost,
    /// Night only.
    Thief,
}

impl EncounterKind {
    pub const DAY: [EncounterKind; 4] =
        [EncounterKind::Monster, EncounterKind::Treasure, EncounterKind::Trap, EncounterKind::Stranger];
    pub const NIGHT: [EncounterKind; 3] = [EncounterKind::Undead, EncounterKind::Ghost, EncounterKind::Thief];
    pub fn as_str(self) -> &'static str {
        match self {
            EncounterKind::Monster => "monster",
            EncounterKind::Treasure => "treasure",
            EncounterKind::Trap => "trap",
            EncounterKind::Stranger => "stranger",
            EncounterKind::Undead => "undead",
            EncounterKind::Ghost => "ghost",
            EncounterKind::Thief => "thief",
        }
    }
    pub fn night_only(self) -> bool {
        EncounterKind::NIGHT.contains(&self)
    }
}

/// How an encounter came out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncounterOutcome {
    pub beat: u32,
    pub by: DemigodId,
    pub roll: DiceRoll,
    /// One line ("Ash's Left Hand drives off the ghoul").
    pub text: String,
    #[serde(default, skip_serializing_if = "crate::dm::is_zero")]
    pub damage: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ItemId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<CharId>,
    #[serde(default, skip_serializing_if = "crate::dm::is_zero_i32")]
    pub sparks: i32,
}

pub(crate) fn is_zero(x: &f32) -> bool {
    *x == 0.0
}
pub(crate) fn is_zero_i32(x: &i32) -> bool {
    *x == 0
}

/// A marked hex: hidden until a demigod steps on it, then revealed and resolved for good.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Encounter {
    pub id: u32,
    pub kind: EncounterKind,
    /// The hex (people or houses level).
    pub cell: String,
    pub at: LonLat,
    pub region: PlaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settlement: Option<PlaceId>,
    pub placed_beat: u32,
    /// Placed at night (night-only kinds only come out then).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub night: bool,
    /// Set when it fades unfound (it leaves the board).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faded_beat: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<EncounterOutcome>,
}

/// Item tokens left on a hex after a fight or a death, until someone takes them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LootPile {
    pub id: u32,
    pub cell: String,
    pub at: LonLat,
    pub region: PlaceId,
    pub items: Vec<ItemId>,
    pub since_beat: u32,
    /// Whose items they were.
    pub from: DemigodId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken_beat: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken_by: Option<DemigodId>,
}

/// Traffic between two settlements (`a < b`); a road once it wore in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trail {
    pub a: PlaceId,
    pub b: PlaceId,
    pub wear: f32,
    /// The beat it became a road (`None`: just a path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub road_since: Option<u32>,
    pub last_used: u32,
}

/// What a shrine grants its demigod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ShrinePower {
    /// +1 spark to the owner's purse every few beats (fewer at higher levels).
    Spark,
    /// Heals the owner's demigods standing within reach.
    Heal,
    /// Wounds rival demigods standing within reach (a little).
    Ward,
}

impl ShrinePower {
    pub fn as_str(self) -> &'static str {
        match self {
            ShrinePower::Spark => "spark",
            ShrinePower::Heal => "heal",
            ShrinePower::Ward => "ward",
        }
    }
}

/// A monument to a demigod on a hex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shrine {
    pub id: u32,
    pub cell: String,
    pub at: LonLat,
    pub region: PlaceId,
    pub place: PlaceId,
    pub demigod: DemigodId,
    pub seat: u8,
    pub name: String,
    pub worshippers: u64,
    pub level: u8,
    pub power: ShrinePower,
    pub built_beat: u32,
    /// The beat its power last granted a spark.
    #[serde(default)]
    pub last_grant: u32,
}

/// The DM's whole state (`World::dm`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Dm {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory: Vec<DmEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settlements: Vec<Settlement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stories: Vec<Story>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encounters: Vec<Encounter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loot: Vec<LootPile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trails: Vec<Trail>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shrines: Vec<Shrine>,
    /// Rules 0.8.1 (schema 7): duels still under way after a resolve (challenge mode without an engine hold lasts
    /// across beats). The plugin owns the shape (`fn7-plugin::duel::Bout`); empty = nothing serialized.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duels: Vec<Value>,
    /// Schema 8 (exact resume): what the last resolve was handed, and the beat's clocks. Absent before the first one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<Mark>,
}

/// Schema 8: the delivery mark and clocks of a state (all epoch ms; 0 = unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Mark {
    /// The newest instant action a resolve has been handed: its receive stamp and `seq`. Instant actions stamped after
    /// it (in `(at_ms, seq)` order) are not in this state yet.
    pub at_ms: i64,
    pub seq: u64,
    /// The newest receive stamp of any action the last resolve was handed (its only clock).
    pub resolved_ms: i64,
    /// This beat's Start on the server's clock (a recorded side input; 0 in simulations).
    #[serde(default)]
    pub round_ms: i64,
}

impl Mark {
    /// Was an instant action stamped `(at_ms, seq)` handed to a resolve already?
    pub fn delivered(&self, at_ms: i64, seq: u64) -> bool {
        (at_ms, seq) <= (self.at_ms, self.seq)
    }
}

impl Dm {
    pub fn is_empty(&self) -> bool {
        *self == Dm::default()
    }
    /// The entry that already answers `query`, if any (the DM reuses it).
    pub fn decided(&self, query: &str) -> Option<&DmEntry> {
        self.memory.iter().rev().find(|e| e.query.as_deref() == Some(query))
    }
    /// Everything decided about `key` (or anything above or below it) at or before `until`, oldest first.
    /// `ancestors` are `key`'s own ancestors (a cell's settlement, region and the world).
    pub fn recall<'a>(&'a self, key: &'a str, ancestors: &'a [String], until: u32) -> impl Iterator<Item = &'a DmEntry> {
        self.memory.iter().filter(move |e| e.beat <= until && e.concerns(key, ancestors))
    }
    pub fn story(&self, d: DemigodId) -> Option<&Story> {
        self.stories.iter().find(|s| s.demigod == d)
    }
    pub fn settlement(&self, p: PlaceId) -> Option<&Settlement> {
        self.settlements.iter().find(|s| s.place == p)
    }
    /// Roads now (trails that wore in and have not faded).
    pub fn roads(&self) -> impl Iterator<Item = &Trail> {
        self.trails.iter().filter(|t| t.road_since.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, beat: u32, scope: Scope, place: &str, up: &[&str], query: Option<&str>) -> DmEntry {
        DmEntry {
            id,
            beat,
            scope,
            place: place.into(),
            up: up.iter().map(|s| s.to_string()).collect(),
            kind: "x".into(),
            query: query.map(str::to_string),
            public: true,
            text: String::new(),
            data: Value::Null,
        }
    }

    #[test]
    fn recall_walks_up_and_down_the_hierarchy() {
        let mut dm = Dm::default();
        dm.memory.push(entry(0, 1, Scope::Cell, "0:people:1:1", &["pl:5", "pl:2", WORLD_KEY], Some("encounter:0")));
        dm.memory.push(entry(1, 2, Scope::Settlement, "pl:5", &["pl:2", WORLD_KEY], None));
        dm.memory.push(entry(2, 3, Scope::Region, "pl:2", &[WORLD_KEY], None));
        dm.memory.push(entry(3, 4, Scope::Region, "pl:3", &[WORLD_KEY], None));
        dm.memory.push(entry(4, 5, Scope::World, WORLD_KEY, &[], None));
        // A region: its own entry, what lies below it, and the world above; not another region.
        let up = vec![WORLD_KEY.to_string()];
        let ids: Vec<u32> = dm.recall("pl:2", &up, 99).map(|e| e.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 4]);
        // At or before a beat.
        let ids: Vec<u32> = dm.recall("pl:2", &up, 2).map(|e| e.id).collect();
        assert_eq!(ids, vec![0, 1]);
        // A cell: itself and its ancestors.
        let anc = vec!["pl:5".to_string(), "pl:2".to_string(), WORLD_KEY.to_string()];
        let ids: Vec<u32> = dm.recall("0:people:1:1", &anc, 99).map(|e| e.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 4]);
        assert_eq!(dm.decided("encounter:0").map(|e| e.id), Some(0));
        assert!(dm.decided("encounter:1").is_none());
        assert!(Dm::default().is_empty() && !dm.is_empty());
        assert_eq!(serde_json::to_string(&Dm::default()).unwrap(), "{}");
    }
}
