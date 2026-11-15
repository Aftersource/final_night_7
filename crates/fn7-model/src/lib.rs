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
pub mod dm;
pub mod earth;
pub mod gods;
pub mod hexgrid;
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
pub use hexgrid::people_cell;

use serde::{Deserialize, Serialize};

/// Version of the [`World`] layout. Bump on any breaking change; recordings carry it so old runs still load.
///
/// Schema 5 (rules 0.7.0): two demigods per seat (`Demigod::side`, `Demigod::player`), each with its own health,
/// position and elimination; manifestations (`World::manifests`) and item-engine fields on items (`Item::ext`). Rules
/// that changed in 0.7.0 read [`World::two_bodies`] (or `meta.schema`), so a schema-4 recording replays exactly.
///
/// Schema 6 (rules 0.8.0): the Dungeon Master's place × time memory and the eight features it governs
/// ([`World::dm`], [`dm::Dm`]; docs/DM_RULEBOOK.md). Rules that changed in 0.8.0 read [`World::dm_rules`], so schema-4
/// and schema-5 recordings replay exactly.
///
/// Schema 7 (rules 0.8.1): challenge mode (open duels in [`dm::Dm::duels`]), the summon's damage check at the accept,
/// heals routed to Nature, the takeover rename fix. Rules that changed in 0.8.1 read [`World::challenge_rules`], so
/// schema-4..6 recordings replay exactly.
///
/// Schema 8 (exact resume): an open duel keeps every blow so far ([`dm::Dm::duels`]: `blows`, `pauses`), the state
/// carries the newest instant action a resolve was handed and the beat's clocks ([`dm::Dm::mark`]), duel input is
/// ordered by arrival stamp. Rules that changed read [`World::resume_rules`], so schema-4..7 recordings replay exactly.
///
/// Schema 9 (rules 0.8.2): duel panels (a bout's `scene`, `panels`, `cycle`; `fields.duel_tick`). Rules that changed
/// read [`World::panel_rules`], so schema-4..8 recordings replay exactly.
///
/// Schema 10 (rules 0.9.0): "wills that matter" ([`dm::Dm::wills`]); read [`World::wills_rules`].
///
/// Schema 11 (gear looks): the loadout each demigod carries and the image identity it shows ([`dm::Dm::looks`]: the
/// `look` records and the item art granted this match), `look.changed` in the DM's memory, item art by descriptor.
/// Rules that changed read [`World::look_rules`], so schema-4..10 recordings replay exactly.
///
/// Schema 12 (AI duellists type): in a duel with a clock an AI fighter types a line in every input window: a bout's
/// `ai` (the typing scripts not yet entered), blows marked `ai`, panels with `typing_script` and a `play_ms` that
/// fits the line. Rules that changed read [`World::ai_duel_rules`], so schema-4..11 recordings replay exactly.
///
/// Schema 13 (the Earth simulator, room setting `earth_sim_enabled`): the voxel field the god powers act on
/// ([`dm::Dm::earth`], [`earth::EarthState`]; docs/EARTH_SIM.md). Rules that changed read [`World::earth_rules`], so
/// schema-4..12 recordings replay exactly.
///
/// Schema 14 (duel weapons; 13 is reserved for the earth simulation): a bout's recorded `picks` (the LLM picker's
/// choices, ordered with the duel's actions), its fighters' `weapon` and the `wield` drawn mid-duel (hammer-space),
/// `projectiles` and seg trims (`at`, `speed`) on panels. Rules that changed read [`World::duel_weapon_rules`], so
/// schema-4..12 recordings replay exactly.
///
/// Schema 15 (item names; 13 and 14 belong to other branches): a thing made outside the item engine (a resolver's
/// find, a god's gift, an encounter's find) whose name is an entity's (a demigod, a person, a player, a god) or empty
/// is named after the object its words describe, and loadouts and item art skip things that are not real objects.
/// Rules that changed read [`World::item_name_rules`], so schema-4..12 recordings replay exactly.
///
/// Schema 16 (fault junctions and tiers; 13..15 are taken by the Earth simulator, duel weapons and item names on their
/// branches): a mega near a plate-boundary junction gets extra multipliers, every mega lands in a tier (`junction` ..
/// `tiny`) that scales its ramifications, and a tiny one's lasting mods stay local. The gate is
/// `fn7_plugin::faults::JUNCTION_SCHEMA` (with the room setting `fault_junctions_enabled`), so older recordings
/// replay exactly.
///
/// Schema 17 (the earth underfoot): every LLM prompt that acts in a place (a hand's orders, the resolver, a subagent's
/// step, a god's ruling, Entropy's ramifications, a general, the narrator, a duel's deed) carries a short line of what
/// the Earth simulator's field holds there now, and a duel's scene carries the ground (`ground`, hazard props). Rules
/// that changed read [`World::earth_prompt_rules`], so schema-4..16 recordings replay exactly.
///
/// Schema 18 (the next move): every human seat's SeatView (`round.brief.private`, `round.private_state`, `game.me`)
/// carries a private `hint`: the one thing to do now, why it matters, the gesture and where to point, picked by rules
/// from the state (no LLM). Derived, never stored; payloads read [`World::hint_rules`], so schema-4..17 recordings
/// replay exactly (their briefs carry no hint).
pub const STATE_SCHEMA: u32 = 18;

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

/// A power's mega or mini variant (rules 0.7.0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PowerScale {
    pub radius_km: f32,
    pub magnitude: u8,
    pub damage: f32,
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
    /// Rules 0.7.0: its MEGA variant (map scale, a bolt) and MINI variant (the board, a spark).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mega: Option<PowerScale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mini: Option<PowerScale>,
}

impl PowerDef {
    /// The variant for `scale` (from the palette, else derived: mega = radius × 4 within 600-2,500 km, magnitude + 2,
    /// damage × 1.5; mini = radius / 20 within 3-30 km, magnitude - 3, damage × 0.6).
    pub fn variant(&self, scale: CastScale) -> PowerScale {
        match scale {
            CastScale::Mega => self.mega.unwrap_or(PowerScale {
                radius_km: (self.radius_km * 4.0).clamp(600.0, 2500.0),
                magnitude: (self.magnitude + 2).min(10),
                damage: (self.damage * 1.5 * 10.0).round() / 10.0,
            }),
            CastScale::Mini => self.mini.unwrap_or(PowerScale {
                radius_km: (self.radius_km / 20.0).clamp(3.0, 30.0),
                magnitude: self.magnitude.saturating_sub(3).max(1),
                damage: (self.damage * 0.6 * 10.0).round() / 10.0,
            }),
        }
    }
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
    /// Schema 5: every manifestation attempt (`mf:N` = index), made or not. Append-only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manifests: Vec<manifest::Manifestation>,
    /// Schema 6: the Dungeon Master's memory and the features it governs (empty, and absent from the JSON, before).
    #[serde(default, skip_serializing_if = "dm::Dm::is_empty")]
    pub dm: dm::Dm,
}

/// One seat's view: what it is zoomed on. Changed only by `Route::Zoom` intents or the action's `zoom` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeatState {
    pub seat: u8,
    pub demigod: DemigodId,
    pub is_bot: bool,
    pub zoom: Zoom,
    pub focus: Option<Ref>,
    /// Schema 5: the seat's display name (the player's name; both of its demigods carry it as `Demigod::player`).
    /// Empty in schema-4 states (where the demigod's name is the player's).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    /// Accounts: the public id of the account whose bound session holds this seat (docs/ACCOUNTS.md "The game
    /// connection"). Absent for every other seat and in every older state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

impl World {
    /// The seat's first demigod (its Left hand in schema 5; the only one before).
    pub fn demigod_of_seat(&self, seat: u8) -> Option<&Demigod> {
        self.demigods.iter().find(|d| d.seat == seat)
    }
    /// Schema 5 rules are on: two demigods per player, the people-level occupancy rule, the item engine.
    pub fn two_bodies(&self) -> bool {
        self.meta.schema >= 5
    }
    /// Schema 6 rules are on (rules 0.8.0): the Dungeon Master's rulebook, its place × time memory and its features.
    pub fn dm_rules(&self) -> bool {
        self.meta.schema >= dm::DM_SCHEMA
    }
    /// Schema 7 rules are on (rules 0.8.1): challenge mode and the 0.8.1 fixes.
    pub fn challenge_rules(&self) -> bool {
        self.meta.schema >= dm::CHALLENGE_SCHEMA
    }
    /// Schema 8 rules are on (exact resume): duels keep their whole timeline in the state, the delivery mark and clocks.
    pub fn resume_rules(&self) -> bool {
        self.meta.schema >= dm::RESUME_SCHEMA
    }
    /// Schema 9 rules are on (rules 0.8.2): a duel plays as panels.
    pub fn panel_rules(&self) -> bool {
        self.meta.schema >= dm::PANEL_SCHEMA
    }
    /// Schema 10 rules may be on (rules 0.9.0, "wills that matter"; the room setting `wills_enabled` decides).
    pub fn wills_rules(&self) -> bool {
        self.meta.schema >= dm::WILLS_SCHEMA
    }
    /// Schema 11 rules may be on (gear looks; the room setting `looks_enabled` decides).
    pub fn look_rules(&self) -> bool {
        self.meta.schema >= dm::LOOK_SCHEMA
    }
    /// Schema 12 rules may be on (AI duellists type; the room setting `duel_ai_lines_per_window` decides).
    pub fn ai_duel_rules(&self) -> bool {
        self.meta.schema >= dm::AI_DUEL_SCHEMA
    }
    /// Schema 15 rules are on (items are never named after entities).
    pub fn item_name_rules(&self) -> bool {
        self.meta.schema >= dm::ITEM_NAME_SCHEMA
    }
    /// Does `name` name an entity of this world rather than a thing: a demigod (with or without a `" (n)"` suffix),
    /// a named person, a player or a god, compared as name keys ([`manifest::name_key`]); an empty name too.
    pub fn names_entity(&self, name: &str) -> bool {
        let k = manifest::name_key(name);
        if k.is_empty() {
            return true;
        }
        let is = |n: &str| !n.trim().is_empty() && manifest::name_key(n) == k;
        let strip = |n: &str| -> String {
            let t = n.trim();
            match t.strip_suffix(')').and_then(|x| x.rfind(" (").map(|i| (i, x))) {
                Some((i, x)) if x[i + 2..].chars().all(|c| c.is_ascii_digit()) && i + 2 < x.len() => x[..i].to_string(),
                _ => t.to_string(),
            }
        };
        self.demigods.iter().any(|d| is(&d.name) || is(&strip(&d.name)) || is(d.player_name()))
            || self.seats.iter().any(|s| is(&s.display_name))
            || self.characters.iter().any(|c| is(&c.name))
            || self.gods.iter().any(|g| is(&g.name))
    }
    /// The Earth simulator (schema 13): the god powers act on a simulated voxel field.
    pub fn earth_rules(&self) -> bool {
        self.meta.schema >= earth::EARTH_SCHEMA
    }
    /// Schema 17 rules: the earth underfoot reaches the prompts that act in a place (and a duel's scene).
    pub fn earth_prompt_rules(&self) -> bool {
        self.meta.schema >= earth::EARTH_PROMPT_SCHEMA
    }
    /// Schema 18: human seats' SeatViews carry the next-move hint (payload only; the state is unchanged).
    pub fn hint_rules(&self) -> bool {
        self.meta.schema >= dm::HINT_SCHEMA
    }
    /// Schema 14 rules: duel weapons, hammer-space, projectiles, the picker's recorded picks.
    pub fn duel_weapon_rules(&self) -> bool {
        self.meta.schema >= dm::DUEL_WEAPONS_SCHEMA
    }
    /// A seat's demigods, in id order (Left then Right in schema 5).
    pub fn bodies_of_seat(&self, seat: u8) -> impl Iterator<Item = &Demigod> {
        self.demigods.iter().filter(move |d| d.seat == seat)
    }
    /// `d` and its sibling (the same player's other demigod), in id order. Just `[d]` before schema 5.
    pub fn siblings(&self, d: DemigodId) -> Vec<DemigodId> {
        match self.demigods.get(d.ix()) {
            Some(x) => self.bodies_of_seat(x.seat).map(|y| y.id).collect(),
            None => vec![],
        }
    }
    /// Do two demigods belong to the same player (the same seat)? Rivals are demigods of other players.
    pub fn same_player(&self, a: DemigodId, b: DemigodId) -> bool {
        a == b
            || matches!((self.demigods.get(a.ix()), self.demigods.get(b.ix())), (Some(x), Some(y)) if x.seat == y.seat)
    }
    /// Is any of the seat's demigods standing? A player is out only when all of theirs have fallen.
    pub fn seat_alive(&self, seat: u8) -> bool {
        self.bodies_of_seat(seat).any(|d| d.alive)
    }
    /// The seat's lead demigod: its first one still standing, else its first. Who pays and answers for the player's
    /// seat-level acts (Time's bill for orders, casts).
    pub fn lead_of_seat(&self, seat: u8) -> Option<DemigodId> {
        self.bodies_of_seat(seat)
            .find(|d| d.alive)
            .or_else(|| self.demigod_of_seat(seat))
            .map(|d| d.id)
    }
    /// The players (seats) with at least one demigod standing, in order of their first demigod's id.
    pub fn alive_seats(&self) -> Vec<u8> {
        let mut v: Vec<u8> = vec![];
        for d in self.demigods.iter().filter(|d| d.alive) {
            if !v.contains(&d.seat) {
                v.push(d.seat);
            }
        }
        v
    }
    /// The players (seats) in order of their first demigod's id.
    pub fn player_seats(&self) -> Vec<u8> {
        let mut v: Vec<u8> = vec![];
        for d in &self.demigods {
            if !v.contains(&d.seat) {
                v.push(d.seat);
            }
        }
        v
    }
    /// Set the player's purse of bolts: bolts are per player, so every demigod of the seat holds the same count (a
    /// schema-4 seat has one demigod, so this is that demigod's bolts).
    pub fn set_sparks(&mut self, d: DemigodId, sparks: u32) {
        let Some(seat) = self.demigods.get(d.ix()).map(|x| x.seat) else {
            return;
        };
        for x in self.demigods.iter_mut().filter(|x| x.seat == seat) {
            x.sparks = sparks;
        }
    }
    /// See [`World::set_sparks`]: the player's purse of bolts (the mega currency), on every demigod of the seat.
    pub fn set_bolts(&mut self, d: DemigodId, bolts: u32) {
        let Some(seat) = self.demigods.get(d.ix()).map(|x| x.seat) else {
            return;
        };
        for x in self.demigods.iter_mut().filter(|x| x.seat == seat) {
            x.bolts = bolts;
        }
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
