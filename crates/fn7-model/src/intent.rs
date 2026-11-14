//! From speech to state change: utterances → intents → routes → proposals (effects) → merged effects.
//! `Effect` is the only way anything changes the world, so the merge, the ledger and the replay all speak one language.

use serde::{Deserialize, Serialize};

use crate::gods::PriceKind;
use crate::ids::*;
use crate::ledger::Source;
use crate::refs::Ref;
use crate::society::{Hand, LonLat};

/// Raw input for one hand in one beat, as the client sent it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Utterance {
    pub seat: u8,
    pub hand: Hand,
    pub beat: u32,
    pub text: String,
    /// "type" | "voice" (client-reported; the speech-to-text happens on the client).
    pub mode: String,
    /// Client-measured span, for words-per-minute.
    pub spoken_ms: Option<u32>,
    pub words: u32,
}

/// A reference to someone not yet resolved ("the baker who saw the fire").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mention {
    pub text: String,
    /// "person" | "group" | "place" | "item" | "god".
    pub kind: String,
    pub near: Option<PlaceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Target {
    Known(Ref),
    Mention(Mention),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Zoom {
    World,
    /// A district (on the planet: a region).
    District,
    /// One city (on the planet; in a city arena, a site).
    City,
    Character,
}

/// One thing a hand decided to do this beat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub id: IntentId,
    pub beat: u32,
    /// The agent that will carry it out (a hand, or a subagent/possession under it).
    pub agent: AgentId,
    pub demigod: DemigodId,
    pub verb: String,
    pub targets: Vec<Target>,
    pub how: String,
    pub place: Option<PlaceId>,
    /// 0–10: how big the act is (drives Consequence debt and god prices).
    pub magnitude: u8,
    /// The words it came from.
    pub span: String,
    pub route: Route,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "to")]
pub enum Route {
    /// Not routed yet.
    Pending,
    /// An ordinary act, to the world resolver.
    World,
    /// An act in a god's domain, to that god.
    God { god: GodId, by: RouteBy },
    /// Spawn a subagent for a task.
    Spawn { goal: String, budget_beats: u32 },
    /// Take control of a memorized (or about-to-be) character.
    Possess,
    /// Change the player's view only (no world effect).
    Zoom { zoom: Zoom, focus: Option<Target> },
    /// Recall a subagent or end a possession.
    Recall { agent: AgentId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteBy {
    Keyword,
    Llm,
}

/// Rules 0.7.0: the two kinds of power. MEGA powers act at map scale (region hexes, hundreds to thousands of km) and
/// cost a lightning bolt; MINI powers act on the table board (a few hexes, 3-30 km) and cost a spark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CastScale {
    Mega,
    Mini,
}

/// Share of a mega cast's damage that reaches every other rival demigod within its radius.
pub const MEGA_AREA_SHARE: f32 = 0.4;

pub(crate) fn is_zero_f32(x: &f32) -> bool {
    *x == 0.0
}

/// The only way the world changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "fx")]
pub enum Effect {
    Move {
        who: Ref,
        to: LonLat,
        place: Option<PlaceId>,
    },
    Damage {
        who: Ref,
        amount: f32,
        cause: String,
    },
    Heal {
        who: Ref,
        amount: f32,
    },
    /// Cluster deaths or departures.
    Shrink {
        cluster: ClusterId,
        count: u32,
    },
    MoodShift {
        cluster: ClusterId,
        anger: f32,
        fear: f32,
        hope: f32,
        devotion: f32,
    },
    Allegiance {
        who: Ref,
        demigod: DemigodId,
        delta: f32,
    },
    Relation {
        a: Ref,
        b: Ref,
        trust: f32,
        fear: f32,
        loyalty: f32,
    },
    Join {
        member: Ref,
        group: GroupId,
    },
    Leave {
        member: Ref,
        group: GroupId,
    },
    FoundGroup {
        name: String,
        kind: String,
        members: Vec<Ref>,
    },
    Memorize {
        mention: Mention,
        cluster: Option<ClusterId>,
    },
    CreateItem {
        name: String,
        description: String,
        holder: Ref,
    },
    Echo {
        item: ItemId,
        holder: Ref,
    },
    Favor {
        god: GodId,
        demigod: DemigodId,
        delta: f32,
    },
    Debt {
        owed_by: Ref,
        god: GodId,
        amount: f32,
        due_in: u32,
    },
    /// Time: push a debt back (it grows).
    Delay {
        row: LedgerId,
        beats: u32,
    },
    Collect {
        row: LedgerId,
    },
    /// Time: restore a target's rewindable fields from N beats ago.
    Rewind {
        target: Ref,
        beats: u32,
    },
    Price {
        who: Ref,
        god: GodId,
        kind: PriceKind,
        amount: f32,
    },
    Forget {
        owner: Ref,
        about: Vec<Ref>,
    },
    Plant {
        owner: Ref,
        text: String,
        about: Vec<Ref>,
    },
    Tell {
        to: Ref,
        row: LedgerId,
    },
    /// A god sends a paladin after `target`: the existing `paladin` (retasked) if given and still serving, else a new
    /// one is raised.
    Dispatch {
        god: GodId,
        mission: String,
        target: Ref,
        #[serde(default)]
        paladin: Option<CharId>,
    },
    Eliminate {
        who: Ref,
        cause: String,
    },
    /// Death: bring a dead body (demigod or character) back, at a fraction of its max health. Invalid if alive.
    Revive {
        who: Ref,
    },
    /// Time: slow (`factor` < 1) or hasten (`factor` > 1) a body for `beats` beats (stored as [`crate::Tempo`]).
    Tempo {
        who: Ref,
        factor: f32,
        beats: u32,
    },
    /// A god power cast by `by` at `at` (a point anywhere on the globe; a lightning bolt, a volcano, a flood... from the
    /// power palette), hitting `target` if any. Costs `by` `cost` bolts; every other live demigod gains two per bolt;
    /// Entropy rises. Its ramifications follow as separate rows.
    Cast {
        by: DemigodId,
        power: String,
        at: LonLat,
        #[serde(default)]
        target: Option<Ref>,
        cost: u32,
        damage: f32,
        /// Rules 0.7.0: a mega cast (1 bolt; every other player gains `bolt_gift` bolts and `spark_gift` sparks; its
        /// damage also reaches rival demigods within `radius_km` at [`MEGA_AREA_SHARE`]) or a mini one (1 spark, no
        /// gift, its target only). `None`: the old cast (`cost` bolts, every other live demigod gains 2 per bolt).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<CastScale>,
        #[serde(default, skip_serializing_if = "crate::intent::is_zero_f32")]
        radius_km: f32,
    },
    /// Bolts paid (`delta` < 0: to `to`, a god, or for slowing time; every other live demigod gains 2 per bolt,
    /// except for a `toll`) or granted (`delta` > 0, by Entropy). Either way Entropy rises.
    Bolts {
        who: DemigodId,
        delta: i32,
        #[serde(default)]
        to: Option<GodId>,
        /// Time's toll for an order or a slow window (rules 0.6.0): no gift to rivals. Absent = false.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        toll: bool,
    },
    /// Entropy rises (or, by Time's cooling, falls); clamped to 0..=ENTROPY_MAX; reaching the max is heat death.
    Entropy { delta: f32 },
    /// A hand raises an army of `size` from a crowd: a new army cluster at the crowd's place.
    RaiseArmy {
        demigod: DemigodId,
        from: ClusterId,
        size: u32,
        name: String,
        orders: String,
    },
    /// An army marches: to `to` this beat (rules move it at most ARMY_KM_PER_BEAT), aimed at `dest` / `target`.
    March {
        army: ClusterId,
        to: LonLat,
        #[serde(default)]
        dest: Option<LonLat>,
        #[serde(default)]
        target: Option<Ref>,
        #[serde(default)]
        orders: Option<String>,
    },
    /// A god takes damage (Entropy at its height).
    Wound { god: GodId, amount: f32 },
    /// Schema 5: one attempt the item engine judged (its verdict is already in the row). Applying it appends the row to
    /// `World.manifests`; a made thing becomes an item held by the manifester (with its `ext`), and a host with
    /// `muster > 0` also musters an army cluster of that many under the manifester.
    Manifest {
        manifestation: Box<crate::manifest::Manifestation>,
        #[serde(default)]
        item: Option<crate::manifest::NewItem>,
    },
    /// A persistent world mod appears (or, with the same name in the same region, stacks): a lasting storm, gravity at
    /// half strength... applied by the rules every beat until it expires.
    Mod { m: crate::WorldMod },
    /// A world mod ends.
    Unmod { id: u32 },
    /// Something happens at a point on the globe (narration with a place: a ramification, freak weather).
    Happen { text: String, at: LonLat },
    /// Pure narration with no state change (kept so it lands in the ledger and memory).
    Note {
        text: String,
    },
    /// A society event (schema 4): the society rules already changed the tables this beat (a mob formed, a battle was
    /// fought, a person emerged); this records it as a geotagged row (verb = the kind) so it spreads as news, lands in
    /// memories and reaches the narrator. `refs` are who and what it is about.
    Society {
        kind: crate::hier::SocKind,
        refs: Vec<Ref>,
        at: LonLat,
        #[serde(default)]
        place: Option<PlaceId>,
    },
    /// A general's reading of an order or of a decision point (schema 4): sets the army's order, target and stance;
    /// `Mutiny` sends it rogue, `Defect` hands it (and its general) to `judgment.defect_to`.
    Orders {
        army: ClusterId,
        by: DemigodId,
        /// The demigod's words ("" = the general's own initiative or a decision point).
        text: String,
        judgment: crate::hier::Judgment,
        /// `order` | `engage` | `strain` | `initiative`.
        why: String,
        #[serde(default)]
        llm: bool,
    },
    /// A demigod raises someone in the hierarchy: to general (replacing the old one, who takes it badly) or captain of
    /// `army` (the promoter's biggest army when absent), or to ringleader/prophet of where they stand.
    Promote {
        who: CharId,
        by: DemigodId,
        role: crate::hier::Role,
        #[serde(default)]
        army: Option<ClusterId>,
    },
}

/// One source's suggestion, before the merge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub source: Source,
    pub intent: Option<IntentId>,
    pub effect: Effect,
    /// The prose line for the ledger row this becomes.
    pub act: String,
    pub actor: Ref,
    pub magnitude: u8,
    pub valence: i8,
    pub witnesses: Vec<Ref>,
    pub cause: Option<LedgerId>,
}

/// What the merge decided for one proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeResolution {
    pub proposal: usize,
    pub outcome: MergeOutcome,
    pub row: Option<LedgerId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum MergeOutcome {
    Applied,
    /// Combined with others (e.g. damage median); `into` is the proposal kept.
    Merged {
        into: usize,
    },
    /// Lost to a higher-precedence source.
    Overruled {
        by: usize,
        rule: String,
    },
    /// Broke an invariant (dead target, no charges, out of scope).
    Invalid {
        reason: String,
    },
}
