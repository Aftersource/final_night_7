//! # fn7-model
//!
//! Every type in Final Night 7, and nothing else: no logic beyond accessors, no I/O, no LLM. The other crates
//! (society, memory, pantheon, agents, plugin, tools), the recorder projections and the client payloads all build on
//! these, so the data is designed once.
//!
//! Layout of the game state ([`World`]): dense `Vec` tables indexed by typed ids, a [`Ref`] string (`"ch:12"`) that
//! points at any row, an append-only [`Ledger`] (the table of consequence), a [`MemoryStore`], the agent table, god
//! state, and the rewind ring. The whole `World` is the plugin state the Theatre snapshots each commit.

pub mod agents;
pub mod beat;
pub mod gods;
pub mod hier;
pub mod ids;
pub mod intent;
pub mod ledger;
pub mod manifest;
pub mod memory;
pub mod refs;
pub mod seed;
pub mod society;

pub use agents::*;
pub use beat::*;
pub use gods::*;
pub use hier::*;
pub use ids::*;
pub use intent::*;
pub use ledger::*;
pub use memory::*;
pub use refs::*;
pub use society::*;

use serde::{Deserialize, Serialize};

/// Version of the [`World`] layout. Bump on any breaking change; recordings carry it so old runs still load.
pub const STATE_SCHEMA: u32 = 4;

/// Match-wide settings and clocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub schema: u32,
    pub rules_version: String,
    pub pantheon_version: String,
    pub seed: u64,
    pub beat: u32,
    /// Time's clock: the match ends (the Final Night) on this beat.
    pub beats_total: u32,
    pub beat_ms: u32,
    pub arena: PlaceId,
    /// Current safe radius around the arena center (Time shrinks it).
    pub arena_radius_km: f64,
    /// Safe radius on the Final Night (`beats_total`), set at setup. 0 in schema-1 states (readers fall back to their
    /// own rule).
    #[serde(default)]
    pub arena_end_radius_km: f64,
    /// The plugin's validated room settings (schema 2+, set by `fn7-plugin`; `null` elsewhere).
    #[serde(default)]
    pub settings: serde_json::Value,
    /// What the arena is: the whole planet (regions → cities) or one city (districts → sites). Schema-2 states are
    /// cities.
    #[serde(default)]
    pub arena_kind: ArenaKind,
    /// The world's Entropy meter, 0..=[`ENTROPY_MAX`]. Every bolt spent raises it; thresholds make the world wackier;
    /// at the maximum, heat death brings the Final Night at once.
    #[serde(default)]
    pub entropy: f32,
    /// The beat heat death struck (entropy reached its maximum; that beat became the Final Night).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat_death: Option<u32>,
}

/// Settled slow windows remembered in [`World::slow_settled`].
pub const SETTLED_KEPT: usize = 16;

/// Most live world mods; a new one past this ends the oldest.
pub const MAX_MODS: usize = 24;

/// A persistent modifier on a patch of the world (made by a cast's ramifications): applied by the rules every beat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldMod {
    pub id: u32,
    pub region: PlaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceId>,
    pub at: LonLat,
    pub radius_km: f32,
    /// Short name ("Permanent thunderstorm").
    pub name: String,
    /// One line, no engine ids.
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub per_beat: ModEffects,
    pub since_beat: u32,
    /// `None` = until something ends it (or it is pushed out by newer mods).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_beat: Option<u32>,
    pub stacks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<DemigodId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<String>,
}

/// What a world mod does each beat inside its radius (× its stacks): damage to demigods and armies, healing of
/// demigods, mood pushes on crowds. All small (≤ 1 per beat per stack).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ModEffects {
    #[serde(default)]
    pub harm: f32,
    #[serde(default)]
    pub heal: f32,
    #[serde(default)]
    pub anger: f32,
    #[serde(default)]
    pub fear: f32,
    #[serde(default)]
    pub hope: f32,
    #[serde(default)]
    pub devotion: f32,
}

/// One power of the palette (`data/powers.json`): what a demigod can cast from the UX, paid in bolts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PowerDef {
    pub slug: String,
    pub name: String,
    /// Icon key for the client's spell bar.
    pub icon: String,
    /// Bolts it costs.
    pub cost: u32,
    pub radius_km: f32,
    /// What it is aimed at: a point, a whole region, or a unit (a demigod, person, crowd or army).
    pub target: PowerTarget,
    /// 0..10: how big it is (scales damage and the number of ramifications).
    pub magnitude: u8,
    /// Direct damage to a body it hits.
    pub damage: f32,
    pub tags: Vec<String>,
    /// One line for the ramifications mind ("a column of fire and ash...").
    pub blurb: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum PowerTarget {
    Point,
    Region,
    Unit,
}

/// Entropy's maximum: heat death.
pub const ENTROPY_MAX: f32 = 100.0;

/// The arena: the whole planet (default for new matches) or one real city (the schema-2 game).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ArenaKind {
    Planet,
    #[default]
    City,
}

/// A slow-time window as the plugin last saw it (engine v2.2 action windows). The Theatre core owns the window
/// itself (it opens on the initiator's `action.begin`, holds everyone's orders hidden, and closes on the initiator's
/// Enter or its deadline); the plugin records it here while it is open so views can show who slowed time, and
/// settles Time's penalty from its real open/close times when it closes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlowWindow {
    pub initiator: DemigodId,
    /// The first beat a resolve saw this window.
    pub opened_beat: u32,
    /// The core's window id (`window.opened.window_id`).
    #[serde(default)]
    pub window_id: u64,
    /// When it opened (epoch ms).
    #[serde(default)]
    pub opened_ms: i64,
    /// When it closes at the latest (epoch ms).
    #[serde(default)]
    pub deadline_ms: i64,
    /// The plugin's clock at the initiator's `action.begin` (with `window_id`, the window's identity).
    #[serde(default)]
    pub begun_ms: i64,
}

/// A slow window whose penalty is settled (kept briefly so a late delivery is never charged twice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettledWindow {
    pub window_id: u64,
    /// The plugin's clock at the initiator's `action.begin` (with the id, the window's identity).
    pub begun_ms: i64,
}

/// Everything match setup needs besides seats and the pantheon. Plain data, so the server and tools can build one
/// without the society engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetupConfig {
    pub seed: u64,
    /// Arena city by name ("Paris"); fuzzy-matched. `None` (or no match) = seeded pick among cities ≥ 1M on land.
    pub city: Option<String>,
    pub rules_version: String,
    pub beats_total: u32,
    pub beat_ms: u32,
    /// Starting safe radius; `None` = derived from the city's population (8–20 km).
    pub arena_radius_km: Option<f64>,
    /// Number of clusters to create.
    pub clusters: u32,
    pub notables_per_district: u32,
    pub echo_charges: u32,
    /// Frames kept in the rewind ring.
    pub rewind_cap: usize,
    /// Planet (regions → cities, `city` = where the Final Night converges) or one city.
    pub arena: ArenaKind,
    /// Lightning bolts each demigod starts with.
    pub start_bolts: u32,
}

impl Default for SetupConfig {
    fn default() -> Self {
        Self {
            seed: 7,
            city: None,
            // The society engine shares the workspace version.
            rules_version: concat!("fn7-society ", env!("CARGO_PKG_VERSION")).into(),
            beats_total: 100,
            beat_ms: 6000,
            arena_radius_km: None,
            clusters: 200,
            notables_per_district: 2,
            echo_charges: 3,
            rewind_cap: 10,
            arena: ArenaKind::City,
            start_bolts: 3,
        }
    }
}

/// The whole game state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct World {
    pub meta: Meta,
    pub places: Vec<Place>,
    pub groups: Vec<Group>,
    pub clusters: Vec<Cluster>,
    pub characters: Vec<Character>,
    pub demigods: Vec<Demigod>,
    pub items: Vec<Item>,
    /// Sorted by `(a, b)`.
    pub relations: Vec<Relation>,
    pub agents: Vec<Agent>,
    pub gods: Vec<GodState>,
    pub rulings: Vec<Ruling>,
    pub intents: Vec<Intent>,
    pub ledger: Ledger,
    pub memory: MemoryStore,
    pub rewind: RewindRing,
    pub beats: Vec<BeatRecord>,
    /// Per-seat view state (zoom and focus), set by the plugin; sorted by seat.
    #[serde(default)]
    pub seats: Vec<SeatState>,
    /// Proposals queued for the next merge (Consequence's collections of due debts, queued at the beat's start so a
    /// Time delay in the same beat can beat them).
    #[serde(default)]
    pub pending: Vec<Proposal>,
    /// Persistent world mods (live only; at most [`MAX_MODS`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<WorldMod>,
    /// Next world-mod id.
    #[serde(default)]
    pub mod_seq: u32,
    /// The open slow-time window, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slow: Option<SlowWindow>,
    /// The most recently settled slow windows (at most [`SETTLED_KEPT`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slow_settled: Vec<SettledWindow>,
    /// Seated demigods reported eliminated to the Theatre and not yet revived (a revive reports them back).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub out_reported: Vec<DemigodId>,
    /// The hierarchy's upper nodes (schema 4): one per place, same index as `places`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
    /// Cells (squads, gangs, congregations, crews), instantiated lazily out of crowds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<Cell>,
    /// Mobs (live and dissolved).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mobs: Vec<Mob>,
    /// The last [`BATTLES_KEPT`] battles (every one is a `battle` event).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub battles: Vec<Battle>,
    /// Running totals of the society's action; `battles` doubles as the next battle id.
    #[serde(default)]
    pub soc: SocietyTotals,
}

/// One seat's view: what it is zoomed on. Changed only by `Route::Zoom` intents or the action's `zoom` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeatState {
    pub seat: u8,
    pub demigod: DemigodId,
    pub is_bot: bool,
    pub zoom: Zoom,
    pub focus: Option<Ref>,
}

impl World {
    pub fn demigod_of_seat(&self, seat: u8) -> Option<&Demigod> {
        self.demigods.iter().find(|d| d.seat == seat)
    }
    pub fn is_planet(&self) -> bool {
        self.meta.arena_kind == ArenaKind::Planet
    }
    pub fn alive_demigods(&self) -> impl Iterator<Item = &Demigod> {
        self.demigods.iter().filter(|d| d.alive)
    }
    pub fn live_agents(&self) -> impl Iterator<Item = &Agent> {
        self.agents.iter().filter(|a| a.is_live())
    }
    /// Groups a ref belongs to (empty for things that can't belong).
    pub fn groups_of(&self, r: Ref) -> GroupSet {
        match r {
            Ref::Char(c) => self
                .characters
                .get(c.ix())
                .map(|c| c.groups.clone())
                .unwrap_or_default(),
            Ref::Cluster(c) => self
                .clusters
                .get(c.ix())
                .map(|c| c.groups.clone())
                .unwrap_or_default(),
            Ref::Demigod(d) => self
                .demigods
                .get(d.ix())
                .map(|d| GroupSet::new(vec![d.following]))
                .unwrap_or_default(),
            _ => GroupSet::default(),
        }
    }
    /// Display name with no engine id (for prompts and captions). Gods use `GodState::name` (schema-1 states, which
    /// lack it, fall back to the god's mind agent); a ledger row is "an earlier event".
    pub fn name_of(&self, r: Ref) -> String {
        match r {
            Ref::Demigod(i) => self.demigods.get(i.ix()).map(|x| x.name.clone()),
            Ref::Char(i) => self.characters.get(i.ix()).map(|x| x.name.clone()),
            Ref::Cluster(i) => self.clusters.get(i.ix()).map(|x| x.label.clone()),
            Ref::Group(i) => self.groups.get(i.ix()).map(|x| x.name.clone()),
            Ref::Place(i) => self.places.get(i.ix()).map(|x| x.name.clone()),
            Ref::Item(i) => self.items.get(i.ix()).map(|x| x.name.clone()),
            Ref::Agent(i) => self.agents.get(i.ix()).map(|x| x.name.clone()),
            Ref::God(i) => self.gods.get(i.ix()).map(|g| {
                if g.name.is_empty() {
                    self.agents
                        .get(g.agent.ix())
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "a god".into())
                } else {
                    g.name.clone()
                }
            }),
            Ref::Ledger(_) => Some("an earlier event".into()),
            Ref::Cell(i) => self.cells.get(i.ix()).map(|x| x.name.clone()),
            Ref::Mob(i) => self.mobs.get(i.ix()).map(|x| x.name.clone()),
        }
        .unwrap_or_else(|| "someone".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Schema-1 rows (no new fields) still load, with the documented defaults.
    #[test]
    fn schema_1_rows_load() {
        let ch: Character = serde_json::from_str(
            r#"{"id":0,"name":"Ada","aliases":[],"origin":"created","place":0,"pos":[3.4,6.4],"traits":[],"groups":[],
            "health":7.0,"alive":true,"allegiance":[],"serves":null,"possessed_by":null,"memorized_beat":0,"memorized_by":null}"#,
        )
        .unwrap();
        assert_eq!((ch.max_health, ch.tempo.len()), (DEFAULT_MAX_HEALTH, 0));
        let cl: Cluster = serde_json::from_str(
            r#"{"id":0,"label":"dockers","size":10,"place":0,"traits":[],"needs":[],
            "mood":{"anger":0.5,"fear":0.1,"hope":0.2,"devotion":0.0},"groups":[1],"worship":[],"allegiance":[],
            "materialized":0,"seed":1}"#,
        )
        .unwrap();
        assert_eq!(cl.baseline, Mood::REST);
        let gs: GodState = serde_json::from_str(
            r#"{"id":0,"favor":[],"attention":0.0,"paladins":[],"agent":3,"invocations":0}"#,
        )
        .unwrap();
        assert!(gs.name.is_empty());
    }

    #[test]
    fn tempo_combines_active_entries() {
        let t = [
            Tempo {
                factor: 0.5,
                until_beat: 5,
            },
            Tempo {
                factor: 4.0,
                until_beat: 3,
            },
        ];
        assert_eq!(tempo_factor(&t, 2), 2.0);
        assert_eq!(tempo_factor(&t, 3), 0.5);
        assert_eq!(tempo_factor(&t, 5), 1.0);
    }
}
