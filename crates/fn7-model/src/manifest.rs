//! Demigods' manifestations (state schema 5, designed in `docs/DEMIGODS_AND_ITEMS.md`).
//!
//! The designer (BUILDLOG 2026-11-14, "demigods: two per player, items they manifest, persistent identity"): each
//! player's Left and Right hands **are** two demigods, and demigods "are picking up and manifesting things obviously
//! all throughout ... raising armies banners of 100,000,000 armies ... all of these high level items must be created
//! and stored, data structure per-match".
//!
//! This module holds:
//! - the two new per-match tables' rows: [`HandDemigod`] (`hd:`, one per hand: the player's two demigods) and
//!   [`Manifestation`] (`mf:`, one per attempt, successful or not, with its full provenance and the item engine's
//!   [`ManifestVerdict`]);
//! - [`ItemExt`], the schema-5 fields flattened into the existing [`crate::Item`] (`it:` stays the item ref);
//! - the **item engine's** pure rules: [`classify`] (scale tiers, a trinket up to a world-army), [`cost`] (bolts, or
//!   debt for what can't be paid), [`odds`] (success odds from tier, bolts, a god's interest, Entropy and repetition)
//!   and [`roll`] (the outcome from one seeded draw). The verdict is always the rules'; an LLM only ever writes words
//!   (names, descriptions, banners) around it, so recordings replay exactly;
//! - the rules fallback for reading a request out of free text: [`parse_count`] ("100,000,000", "a hundred million",
//!   "1.5bn") and [`classify_kind`].
//!
//! Wired in rules 0.7.0 (state schema 5): the player's two demigods are two [`crate::Demigod`] rows per seat (the
//! designer chose separate health per body, so the [`HandDemigod`] sidecar sketched in the design is not used as a
//! table); every attempt is a row of `World.manifests`; made things are [`crate::Item`]s with [`ItemExt`] flattened in.
//! `fn7-plugin`'s `items` module runs the engine in the beat.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::refs::Ref;
use crate::society::{Hand, LonLat};

/// The state schema these tables belong to (the `World` bumps [`crate::STATE_SCHEMA`] to this at integration).
pub const MANIFEST_SCHEMA: u32 = 5;

macro_rules! new_id {
    ($(#[$m:meta])* $name:ident, $prefix:literal) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl $name {
            pub const PREFIX: &'static str = $prefix;
            #[inline]
            pub fn ix(self) -> usize {
                self.0 as usize
            }
            #[inline]
            pub fn from_ix(i: usize) -> Self {
                Self(i as u32)
            }
            /// Parse `"<prefix>:<n>"` (until [`Ref`] carries this prefix).
            pub fn parse(s: &str) -> Option<Self> {
                let (p, n) = s.split_once(':')?;
                (p == $prefix).then(|| n.parse().ok().map(Self)).flatten()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}:{}", $prefix, self.0)
            }
        }
    };
}

new_id!(/// One of a player's two demigods: the Left or Right hand, given its own identity (schema 5).
    HandDemigodId, "hd");
new_id!(/// One manifestation attempt (schema 5).
    ManifestationId, "mf");

// ---------------------------------------------------------------------------------------------------------------------
// The player's two demigods
// ---------------------------------------------------------------------------------------------------------------------

/// One of the player's two demigods: a hand orchestrator ([`crate::AgentKind::Hand`]) with its own name, look,
/// position on the globe, presence on the table, items and deeds.
///
/// Phase 1 (schema 5): the seat's [`crate::Demigod`] row (`dg:`) stays the battle-royale contestant and the **shared
/// purse**: health, bolts, debts, elimination and standings stay per seat, so balance is unchanged. Phase 2 (a
/// designer decision, see the doc's open questions) would give each hand demigod its own `health`, with the seat out
/// when both fall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HandDemigod {
    pub id: HandDemigodId,
    /// The seat's contestant row (the shared purse in phase 1).
    pub demigod: DemigodId,
    pub seat: u8,
    pub hand: Hand,
    /// The hand orchestrator that speaks and acts for it (`ag:`).
    pub agent: AgentId,
    /// Display name (unique in the match).
    pub name: String,
    /// The normalized name: the key of its visual sheet, portrait and diorama (the same look in every match).
    pub identity: String,
    pub pos: LonLat,
    pub place: PlaceId,
    /// Its home turf, fixed at setup (the diorama's base).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<PlaceId>,
    pub alive: bool,
    /// Phase 2 only: its own health. `None` = it shares its seat demigod's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<f32>,
    /// Items it holds now (its manifestations and what it picked up, stole or was given).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ItemId>,
    #[serde(default)]
    pub deeds: Deeds,
    /// The player's profile key (normalized display name); `None` for the built-in AI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// A hand demigod's running tally, for the table, the history and the statue.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Deeds {
    /// Ledger rows it (or its subagents) did.
    pub acts: u32,
    /// Manifestation attempts, and how many produced an item (success, twisted or partial).
    pub manifests: u32,
    pub manifested: u32,
    /// The biggest tier it ever manifested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_tier: Option<Tier>,
    /// Soldiers it ever raised by manifestation (an f64: armies can be absurd).
    #[serde(default)]
    pub soldiers: f64,
    /// Its most notable rows (highest magnitude; at most [`DEEDS_KEPT`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notable: Vec<LedgerId>,
}

/// Notable rows kept per hand demigod.
pub const DEEDS_KEPT: usize = 12;

// ---------------------------------------------------------------------------------------------------------------------
// Items: kinds, tiers, states
// ---------------------------------------------------------------------------------------------------------------------

/// What a manifested thing is. Drives its unit, the god whose interest matters, and how it acts in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ItemKind {
    /// A magic object.
    Artifact,
    Weapon,
    Armor,
    /// A holy or historic object.
    Relic,
    /// A building, fortress, wall, tower, city.
    Structure,
    /// Soldiers under a banner (`count` = soldiers).
    Army,
    /// Ships or aircraft (`count` = vessels).
    Fleet,
    /// Beasts, swarms, monsters (`count` = creatures).
    Creature,
    /// A storm, an aurora, a second moon: a thing that is mostly an effect.
    Phenomenon,
    #[default]
    Other,
}

impl ItemKind {
    /// Kinds whose `count` is a host of individuals (soldiers, ships, beasts): their tier comes from the count.
    pub fn is_host(self) -> bool {
        matches!(self, ItemKind::Army | ItemKind::Fleet | ItemKind::Creature)
    }
    /// The unit `count` is measured in ("" = copies of an object).
    pub fn unit(self) -> &'static str {
        match self {
            ItemKind::Army => "soldiers",
            ItemKind::Fleet => "vessels",
            ItemKind::Creature => "creatures",
            _ => "",
        }
    }
    /// The god whose interest (favor toward the manifester) sways the odds, by pantheon index (`gd:N`; the order of
    /// `data/pantheon.json`: 0 Time, 1 Nature, 2 Consequence, 3 Death, 4 Chance, 5 Memory, 6 Entropy). Data the
    /// designer may change; anything without an obvious patron is Entropy's.
    pub fn patron(self) -> GodId {
        GodId(match self {
            ItemKind::Weapon | ItemKind::Armor => 3,
            ItemKind::Army | ItemKind::Fleet => 4,
            ItemKind::Structure | ItemKind::Creature => 1,
            ItemKind::Relic => 5,
            ItemKind::Artifact | ItemKind::Phenomenon | ItemKind::Other => 6,
        })
    }
}

/// How big a manifestation is, from a pocket trinket to a world-army. Ordered; serialized as its snake_case name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Tier {
    /// A pocket object, a squad (≤ 12).
    #[default]
    Trinket = 0,
    /// A magic object, a warband (≤ 1,000).
    Artifact = 1,
    /// A legendary object, an army (≤ 100,000).
    Relic = 2,
    /// A wonder of the world, a host (≤ 1,000,000).
    Wonder = 3,
    /// Nation-scale, a grand host (≤ 10,000,000).
    Legend = 4,
    /// Continent-scale, a horde (≤ 100,000,000: the designer's banners).
    Myth = 5,
    /// Planet-shaking, a world-host (≤ 1,000,000,000).
    Titan = 6,
    /// Beyond: a world-army larger than humanity, a second moon.
    Apocalypse = 7,
}

impl Tier {
    pub const ALL: [Tier; 8] = [
        Tier::Trinket,
        Tier::Artifact,
        Tier::Relic,
        Tier::Wonder,
        Tier::Legend,
        Tier::Myth,
        Tier::Titan,
        Tier::Apocalypse,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> Tier {
        Tier::ALL[i.min(7)]
    }
    pub fn up(self, n: usize) -> Tier {
        Tier::from_index(self.index() + n)
    }
    pub fn down(self, n: usize) -> Tier {
        Tier::from_index(self.index().saturating_sub(n))
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Trinket => "trinket",
            Tier::Artifact => "artifact",
            Tier::Relic => "relic",
            Tier::Wonder => "wonder",
            Tier::Legend => "legend",
            Tier::Myth => "myth",
            Tier::Titan => "titan",
            Tier::Apocalypse => "apocalypse",
        }
    }
    /// What a host of this tier is called ("a horde of 100,000,000 soldiers").
    pub fn host_word(self) -> &'static str {
        match self {
            Tier::Trinket => "squad",
            Tier::Artifact => "warband",
            Tier::Relic => "army",
            Tier::Wonder => "host",
            Tier::Legend => "grand host",
            Tier::Myth => "horde",
            Tier::Titan => "world-host",
            Tier::Apocalypse => "world-army",
        }
    }
}

/// Upper bounds (inclusive) of each tier's host size; above the last is [`Tier::Apocalypse`].
pub const HOST_BANDS: [f64; 7] = [12.0, 1e3, 1e5, 1e6, 1e7, 1e8, 1e9];

/// The largest count the engine keeps (anything "countless" is this). Exact integers in JSON up to 2^53; past that a
/// count is an approximation, which is fine for a horde.
pub const MAX_COUNT: f64 = 1e15;

/// Where an item is now. Items are never deleted (Final Night rule: everything ends on a pile); a destroyed item stays
/// as its wreck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ItemState {
    /// Carried or kept by its manifester (or whoever made it).
    #[default]
    Held,
    /// Out in the world doing its thing: an army marching, a fortress standing, a storm raging.
    Deployed,
    /// Gone; kept as a wreck for the pile.
    Destroyed,
    /// Given away: held now by someone its holder chose.
    Transferred,
    /// Taken: held now by someone who took it.
    Stolen,
}

/// One change of an item's state, with its ledger row (the item's chain of custody).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ItemEvent {
    pub beat: u32,
    pub state: ItemState,
    /// Who holds it after this change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<Ref>,
    /// Who caused the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<LedgerId>,
    /// For hosts: the count after this change (casualties, desertion, a partial).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<f64>,
}

/// An army's (or a fleet's) banner: drawn from its hand demigod's visual sheet, so a horde looks like its master.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Banner {
    /// The device on the field ("a stag's head"): the hand demigod's emblem.
    pub device: String,
    /// Two field colors: the hand demigod's signature colors.
    pub colors: [String; 2],
    /// A short war cry (LLM flavor for big tiers; else none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motto: Option<String>,
}

fn one() -> f64 {
    1.0
}

/// The schema-5 fields of an [`crate::Item`], flattened into it at integration (`#[serde(flatten)] pub ext:
/// ItemExt`), so schema-4 items load with these defaults: one held object of kind `other`, tier `trinket`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ItemExt {
    #[serde(default)]
    pub kind: ItemKind,
    #[serde(default)]
    pub tier: Tier,
    /// Soldiers, vessels, creatures, or copies. f64 so a horde of 1e8 (or more) is just a number.
    #[serde(default = "one")]
    pub count: f64,
    /// The unit of `count` ("" = copies).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    #[serde(default)]
    pub state: ItemState,
    /// The attempt that made it (`None` for items from before schema 5 or from other effects).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifestation: Option<ManifestationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<Banner>,
    /// The image subject (no style; the media layer adds it and attaches `image` with `image_kind: "item"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_prompt: Option<String>,
    /// Where it is (deployed things) or was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<LonLat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceId>,
    /// A deployed host's footprint in the society simulation (an army cluster of [`muster_size`] people).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muster: Option<ClusterId>,
    /// The chain of custody, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<ItemEvent>,
}

impl ItemExt {
    /// Nothing the item engine set (a schema-4 item, or one an old path made): serialized as nothing, so schema-4
    /// states stay byte-identical.
    pub fn is_plain(&self) -> bool {
        *self == ItemExt::default()
    }
}

impl Default for ItemExt {
    fn default() -> Self {
        Self {
            kind: ItemKind::Other,
            tier: Tier::Trinket,
            count: 1.0,
            unit: String::new(),
            state: ItemState::Held,
            manifestation: None,
            banner: None,
            image_prompt: None,
            at: None,
            place: None,
            muster: None,
            history: vec![],
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// A manifestation attempt
// ---------------------------------------------------------------------------------------------------------------------

/// What a demigod tried to manifest, as read from its orders (the hand mind's `manifest` field, or the rules
/// fallback: [`parse_count`] and [`classify_kind`] over the intent's words).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ManifestRequest {
    pub kind: ItemKind,
    /// The name asked for ("the Banner of the Hundred Million"); the rules name it if empty.
    #[serde(default)]
    pub name: String,
    #[serde(default = "one")]
    pub count: f64,
    /// 0–10, the intent's magnitude.
    pub magnitude: u8,
    /// The words it came from (player text: never put in a prompt outside a fenced user message).
    #[serde(default)]
    pub span: String,
    /// Bolts the player offered beyond the tier's price (each one helps the odds).
    #[serde(default)]
    pub offer_bolts: u32,
    /// Take on Consequence's debt for bolts the purse can't pay (the default; `false` = refuse instead).
    #[serde(default = "yes")]
    pub accept_debt: bool,
    /// A god the orders invoked for it, if any (its ruling then counts, see [`GodStance`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub god: Option<GodId>,
}

fn yes() -> bool {
    true
}

/// What the item engine decided, and every input that went into it (so a client, the history and a test can show
/// why).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ManifestVerdict {
    pub outcome: Outcome,
    /// The tier asked for (a partial's item is smaller: see the item's own tier).
    pub tier: Tier,
    pub odds: Odds,
    /// The seeded draw in [0, 1) (`Rng::derive(seed, [MANIFEST_SALT, beat, mf id])`).
    pub roll: f64,
    pub cost: Cost,
    /// The god whose interest counted (the request's god, else the kind's patron).
    pub god: GodId,
    /// Inputs as they were: the god's favor toward the payer (−10..10), its stance, and Entropy (0..100).
    pub favor: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stance: Option<GodStance>,
    pub entropy: f32,
    /// Short reasons, rules-written ("relic tier: 1 bolt", "Death favors you", "Entropy loves this").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    /// Who wrote the item's words: the rules, the LLM, or the rules after an LLM call failed.
    pub flavor: Flavor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Outcome {
    /// Made, as asked.
    Success,
    /// Made, but not as intended (Entropy's touch): the flavor writes the twist; the item may serve someone else.
    Twisted,
    /// Made smaller: a tenth of the host, or one tier down for an object.
    Partial,
    /// Nothing appears; the price is still paid.
    Failed,
    /// Nothing appears and it hurts: damage to the payer ([`backfire_damage`]).
    Backfired,
    /// Not attempted: the purse couldn't pay and debt was refused, or the invoked god refused.
    Refused,
}

impl Outcome {
    /// An item exists afterwards.
    pub fn made(self) -> bool {
        matches!(self, Outcome::Success | Outcome::Twisted | Outcome::Partial)
    }
}

/// An invoked god's ruling on the attempt, as it bears on the odds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum GodStance {
    Grant,
    Bargain,
    /// Success becomes a twist.
    Twist,
    /// Not attempted.
    Refuse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Flavor {
    Rules,
    Llm,
    /// An LLM call was made and failed or was late; the rules wrote it.
    Fallback,
}

/// What an attempt costs: bolts from the purse, and Consequence's debt for the rest.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Cost {
    /// The tier's price in bolts.
    pub required: u32,
    /// Bolts actually paid (price plus any offer, as far as the purse goes).
    pub bolts: u32,
    /// Bolts paid beyond the price (they help the odds).
    pub extra: u32,
    /// Bolts of the price that couldn't be paid, financed as debt.
    pub missing: u32,
    /// Debt taken on (damage points, due in `debt_due_beats`, owed to Consequence).
    pub debt: f32,
}

/// The engine's odds, with the log-odds terms that made them.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Odds {
    /// P(an item appears as asked or twisted).
    pub success: f64,
    /// Of the successes, the share that are twisted.
    pub twist: f64,
    /// Of the failures, the share that are partial (made smaller).
    pub partial: f64,
    /// Of the failures, the share that backfire.
    pub backfire: f64,
    pub terms: OddsTerms,
}

/// The log-odds terms summed into [`Odds::success`] (before the clamp).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OddsTerms {
    pub base: f64,
    pub extra_bolts: f64,
    pub debt: f64,
    pub favor: f64,
    pub stance: f64,
    pub entropy: f64,
    pub repeat: f64,
}

/// One manifestation attempt: who, when, where, what was asked, the verdict, the item (if one appeared) and the ledger
/// rows it wrote. Successful or not, every attempt is a row; the table only grows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Manifestation {
    pub id: ManifestationId,
    pub beat: u32,
    /// Who manifested it: a hand demigod (`hd:`) once `Ref` carries it; a demigod, agent or person otherwise.
    pub by: Ref,
    /// Who paid (the seat's purse).
    pub demigod: DemigodId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<Hand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentId>,
    pub request: ManifestRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<LonLat>,
    pub verdict: ManifestVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ItemId>,
    /// The rows it wrote: the attempt itself first, then payment, debt, backfire.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<LedgerId>,
    /// Who could see it happen: `true` = public (tier ≥ [`PUBLIC_TIER`], or deployed), else only the payer's seat and
    /// those whose groups learn of its row.
    pub public: bool,
}

/// A thing an attempt made, before it is an item (`Effect::Manifest`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NewItem {
    pub name: String,
    pub description: String,
    pub ext: ItemExt,
    /// A host's footprint in the society simulation ([`muster_size`]); 0 = none (objects, or a host kept in reserve).
    #[serde(default)]
    pub muster: u32,
}

/// The client's piece or icon for an item: a short enumerable string (`sword`, `banner_army`, `relic`, `structure`,
/// `beast`, `vehicle`, `artifact`, `armor`, `phenomenon`, `thing`), from its kind and, for a weapon, its words.
pub fn icon_of(kind: ItemKind, name: &str) -> &'static str {
    let n = name.to_lowercase();
    match kind {
        ItemKind::Army => "banner_army",
        ItemKind::Fleet => "vehicle",
        ItemKind::Creature => "beast",
        ItemKind::Structure => "structure",
        ItemKind::Relic => "relic",
        ItemKind::Artifact => "artifact",
        ItemKind::Armor => "armor",
        ItemKind::Phenomenon => "phenomenon",
        ItemKind::Weapon if ["bow", "crossbow", "gun", "cannon", "railgun"].iter().any(|w| n.contains(w)) => "ranged",
        ItemKind::Weapon => "sword",
        ItemKind::Other => "thing",
    }
}

/// Every [`icon_of`] value (for schemas and the client).
pub const ICONS: &[&str] = &[
    "sword", "ranged", "armor", "banner_army", "vehicle", "beast", "structure", "relic", "artifact", "phenomenon", "thing",
];

/// From this tier up, an attempt is public at once (everyone sees a wonder rise or a horde appear).
pub const PUBLIC_TIER: Tier = Tier::Relic;

// ---------------------------------------------------------------------------------------------------------------------
// The item engine: tiers
// ---------------------------------------------------------------------------------------------------------------------

/// The tier of a host of `count` individuals.
pub fn tier_for_count(count: f64) -> Tier {
    if count.is_nan() {
        return Tier::Trinket;
    }
    HOST_BANDS
        .iter()
        .position(|b| count <= *b)
        .map(Tier::from_index)
        .unwrap_or(Tier::Apocalypse)
}

/// The tier of an act's magnitude (0–10).
pub fn tier_for_magnitude(m: u8) -> Tier {
    Tier::from_index(match m {
        0..=1 => 0,
        2..=3 => 1,
        4 => 2,
        5 => 3,
        6 => 4,
        7 => 5,
        8..=9 => 6,
        _ => 7,
    })
}

/// The tier of a request: hosts by their count or their magnitude, whichever is bigger; objects by magnitude, one tier
/// up for a thousand copies or more and two for a million or more.
pub fn classify(kind: ItemKind, count: f64, magnitude: u8) -> Tier {
    let by_mag = tier_for_magnitude(magnitude);
    if kind.is_host() {
        return by_mag.max(tier_for_count(count));
    }
    let bump = if count >= 1e6 {
        2
    } else if count >= 1e3 {
        1
    } else {
        0
    };
    by_mag.up(bump)
}

// ---------------------------------------------------------------------------------------------------------------------
// The item engine: cost and odds
// ---------------------------------------------------------------------------------------------------------------------

/// Bolts each tier costs.
pub const TIER_BOLTS: [u32; 8] = [0, 0, 1, 1, 2, 3, 4, 6];
/// Base success odds per tier, before any modifier.
pub const TIER_BASE_P: [f64; 8] = [0.97, 0.90, 0.80, 0.66, 0.52, 0.38, 0.25, 0.14];
/// Debt per bolt of the price that couldn't be paid, × (1 + tier/4).
pub const DEBT_PER_MISSING_BOLT: f32 = 0.6;
/// Log-odds per extra bolt offered (at most [`MAX_EXTRA_BOLTS`] count).
pub const EXTRA_BOLT_LOGIT: f64 = 0.35;
pub const MAX_EXTRA_BOLTS: u32 = 4;
/// Log-odds per bolt financed by debt (Consequence watching makes things go wrong).
pub const DEBT_BOLT_LOGIT: f64 = -0.15;
/// Log-odds at full favor (±10).
pub const FAVOR_LOGIT: f64 = 1.2;
/// Log-odds at maximum Entropy, for tiers ≥ Wonder ("Entropy loves superpowers").
pub const ENTROPY_LOGIT: f64 = 0.8;
/// Log-odds per recent attempt by the same hand demigod (at most [`MAX_REPEAT`] count).
pub const REPEAT_LOGIT: f64 = -0.3;
pub const MAX_REPEAT: u32 = 4;
/// Beats an attempt counts as recent.
pub const REPEAT_WINDOW: u32 = 5;
/// Success odds never leave this range.
pub const P_MIN: f64 = 0.02;
pub const P_MAX: f64 = 0.99;
/// Salt for the attempt's seeded draw (`Rng::derive(seed, [MANIFEST_SALT, beat, id])`).
pub const MANIFEST_SALT: u64 = 0x6d61_6e69_6665_7374;

/// Why an attempt wasn't made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The purse is short and debt was refused.
    CantPay { required: u32, have: u32 },
    /// The invoked god refused.
    GodRefused,
}

/// What an attempt at `tier` costs a purse of `have` bolts, offering `offer` extra; the price's shortfall becomes debt
/// unless `accept_debt` is false.
pub fn cost(tier: Tier, have: u32, offer: u32, accept_debt: bool) -> Result<Cost, Refusal> {
    cost_scaled(tier, have, offer, accept_debt, 1.0)
}

/// [`cost`] with the tier's price multiplied by `scale` (the room's `manifest_scale`), rounded to whole bolts.
pub fn cost_scaled(tier: Tier, have: u32, offer: u32, accept_debt: bool, scale: f32) -> Result<Cost, Refusal> {
    let required = (TIER_BOLTS[tier.index()] as f32 * scale.max(0.0)).round() as u32;
    let paid_price = required.min(have);
    let missing = required - paid_price;
    if missing > 0 && !accept_debt {
        return Err(Refusal::CantPay { required, have });
    }
    let extra = offer.min(have - paid_price);
    let debt = missing as f32 * DEBT_PER_MISSING_BOLT * (1.0 + tier.index() as f32 / 4.0);
    Ok(Cost {
        required,
        bolts: paid_price + extra,
        extra,
        missing,
        debt,
    })
}

/// Everything [`odds`] reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OddsInputs {
    pub tier: Tier,
    pub cost: Cost,
    /// The god's favor toward the payer, −10..10.
    pub favor: f32,
    pub stance: Option<GodStance>,
    /// The world's Entropy, 0..100.
    pub entropy: f32,
    /// The same hand demigod's attempts in the last [`REPEAT_WINDOW`] beats.
    pub recent: u32,
}

fn logit(p: f64) -> f64 {
    (p / (1.0 - p)).ln()
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// The odds of an attempt.
///
/// `success = clamp(σ(logit(base[tier]) + 0.35·extra + (−0.15)·missing + 1.2·favor/10 + stance + 0.8·e·[tier ≥ wonder]
/// − 0.3·recent), 0.02, 0.99)` with `e = entropy/100` and stance Grant +1.0, Bargain +0.3, Twist 0, Refuse −∞.
/// Of the successes, `twist = min(0.04 + 0.30·e, 0.5)` are twisted (all of them under a Twist ruling); of the failures,
/// `partial = 0.25 + 0.05·tier` are partial and `backfire = (0.05 + 0.25·e)·tier/7` backfire.
pub fn odds(i: &OddsInputs) -> Odds {
    let t = i.tier.index();
    let e = (i.entropy as f64 / 100.0).clamp(0.0, 1.0);
    let terms = OddsTerms {
        base: logit(TIER_BASE_P[t]),
        extra_bolts: EXTRA_BOLT_LOGIT * i.cost.extra.min(MAX_EXTRA_BOLTS) as f64,
        debt: DEBT_BOLT_LOGIT * i.cost.missing as f64,
        favor: FAVOR_LOGIT * (i.favor as f64 / 10.0).clamp(-1.0, 1.0),
        stance: match i.stance {
            Some(GodStance::Grant) => 1.0,
            Some(GodStance::Bargain) => 0.3,
            Some(GodStance::Twist) | None => 0.0,
            Some(GodStance::Refuse) => f64::NEG_INFINITY,
        },
        entropy: if i.tier >= Tier::Wonder {
            ENTROPY_LOGIT * e
        } else {
            0.0
        },
        repeat: REPEAT_LOGIT * i.recent.min(MAX_REPEAT) as f64,
    };
    let sum = terms.base + terms.extra_bolts + terms.debt + terms.favor + terms.stance + terms.entropy + terms.repeat;
    let success = if i.stance == Some(GodStance::Refuse) {
        0.0
    } else {
        sigmoid(sum).clamp(P_MIN, P_MAX)
    };
    Odds {
        success,
        twist: if i.stance == Some(GodStance::Twist) {
            1.0
        } else {
            (0.04 + 0.30 * e).min(0.5)
        },
        partial: 0.25 + 0.05 * t as f64,
        backfire: (0.05 + 0.25 * e) * t as f64 / 7.0,
        terms,
    }
}

/// The outcome of one draw `u` in [0, 1): `[0, s·(1−twist))` success, `[s·(1−twist), s)` twisted, then the failure
/// span split into partial (its first `partial` share), backfired (its last `backfire` share) and failed.
pub fn roll(o: &Odds, u: f64) -> Outcome {
    let s = o.success;
    if s <= 0.0 {
        return Outcome::Refused;
    }
    if u < s * (1.0 - o.twist) {
        return Outcome::Success;
    }
    if u < s {
        return Outcome::Twisted;
    }
    let r = (u - s) / (1.0 - s);
    if r < o.partial {
        Outcome::Partial
    } else if r >= 1.0 - o.backfire {
        Outcome::Backfired
    } else {
        Outcome::Failed
    }
}

/// A partial's size: a tenth of a host (its tier recomputed), or one tier down for an object (same count).
pub fn partial_of(kind: ItemKind, count: f64, tier: Tier) -> (f64, Tier) {
    if kind.is_host() {
        let c = (count / 10.0).floor().max(1.0);
        (c, tier_for_count(c).min(tier))
    } else {
        (count, tier.down(1))
    }
}

/// Damage a backfire does to the payer: 0.5 + 0.25 per tier (at most 2.25).
pub fn backfire_damage(tier: Tier) -> f32 {
    0.5 + 0.25 * tier.index() as f32
}

/// Entropy a manifestation raises on top of the usual +0.4 per bolt spent: 0.15 per tier above Relic.
pub fn entropy_rise(tier: Tier) -> f32 {
    0.15 * tier.index().saturating_sub(Tier::Relic.index()) as f32
}

/// People linear in the simulation below this; above it a host's footprint grows with the log of its count.
pub const MUSTER_LINEAR: f64 = 50_000.0;
/// The largest army cluster a manifested host puts into the society simulation.
pub const MUSTER_CAP: u32 = 400_000;

/// A deployed host's footprint in the society simulation: the narrative count is unbounded (a horde of 1e8), but the
/// army cluster the rules march and clash with is `count` up to 50,000, then `50,000 · (1 + ln(count/50,000))`, at most
/// 400,000. So a hundred million soldiers fight like ~430k (capped at 400k), not like a hundred million.
pub fn muster_size(count: f64) -> u32 {
    if count.is_nan() || count <= 0.0 {
        return 0;
    }
    let c = if count <= MUSTER_LINEAR {
        count
    } else {
        MUSTER_LINEAR * (1.0 + (count / MUSTER_LINEAR).ln())
    };
    (c.round() as u32).min(MUSTER_CAP)
}

// ---------------------------------------------------------------------------------------------------------------------
// Rules fallback: reading a request from free text
// ---------------------------------------------------------------------------------------------------------------------

const SMALL: &[(&str, f64)] = &[
    ("zero", 0.0),
    ("a", 1.0),
    ("an", 1.0),
    ("one", 1.0),
    ("two", 2.0),
    ("three", 3.0),
    ("four", 4.0),
    ("five", 5.0),
    ("six", 6.0),
    ("seven", 7.0),
    ("eight", 8.0),
    ("nine", 9.0),
    ("ten", 10.0),
    ("eleven", 11.0),
    ("twelve", 12.0),
    ("thirteen", 13.0),
    ("fourteen", 14.0),
    ("fifteen", 15.0),
    ("sixteen", 16.0),
    ("seventeen", 17.0),
    ("eighteen", 18.0),
    ("nineteen", 19.0),
    ("twenty", 20.0),
    ("thirty", 30.0),
    ("forty", 40.0),
    ("fifty", 50.0),
    ("sixty", 60.0),
    ("seventy", 70.0),
    ("eighty", 80.0),
    ("ninety", 90.0),
];

const SCALES: &[(&str, f64)] = &[
    ("thousand", 1e3),
    ("million", 1e6),
    ("billion", 1e9),
    ("trillion", 1e12),
];

const COUNTLESS: &[&str] = &["countless", "infinite", "endless", "innumerable", "uncountable", "limitless"];

fn suffix_mult(s: &str) -> Option<f64> {
    Some(match s {
        "k" => 1e3,
        "m" | "mm" | "mil" | "mn" => 1e6,
        "b" | "bn" => 1e9,
        "t" | "tn" => 1e12,
        _ => return None,
    })
}

/// A numeric literal with an optional attached suffix: "100,000,000", "1_000", "1.5bn", "100m", "1e8", "10k".
fn literal(tok: &str) -> Option<f64> {
    let t: String = tok.chars().filter(|c| *c != ',' && *c != '_').collect();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, rest) = t.split_at(split);
    if num.is_empty() || num == "." {
        return None;
    }
    let mut v: f64 = num.parse().ok()?;
    if rest.is_empty() {
        return Some(v);
    }
    if let Some(m) = suffix_mult(rest) {
        return Some(v * m);
    }
    // An exponent: "1e8".
    if let Some(exp) = rest.strip_prefix('e') {
        let e: i32 = exp.parse().ok()?;
        v *= 10f64.powi(e.clamp(-3, 15));
        return Some(v);
    }
    None
}

/// The largest count mentioned in `text`, if any: digits ("100,000,000", "1.5bn", "10k", "1e8"), digits with a scale
/// word ("100 million"), number words ("a hundred million", "two hundred thousand", "a dozen"), or a word for
/// countless (→ [`MAX_COUNT`]). Capped at [`MAX_COUNT`]. A lone "a"/"an" is not a count.
pub fn parse_count(text: &str) -> Option<f64> {
    let lower = text.to_lowercase();
    let toks: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == ',' || c == '.' || c == '_'))
        .map(|t| t.trim_matches(|c| c == ',' || c == '.'))
        .filter(|t| !t.is_empty())
        .collect();
    let mut best: Option<f64> = None;
    let mut keep = |v: f64| {
        let v = v.min(MAX_COUNT);
        if best.is_none_or(|b| v > b) {
            best = Some(v);
        }
    };
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if COUNTLESS.contains(&t) {
            keep(MAX_COUNT);
            i += 1;
            continue;
        }
        if let Some(mut v) = literal(t) {
            // A scale word may follow ("100 million", "2.5 billion", "3 hundred thousand").
            let mut j = i + 1;
            while j < toks.len() {
                if toks[j] == "hundred" {
                    v *= 100.0;
                } else if let Some((_, m)) = SCALES.iter().find(|(w, _)| *w == toks[j]) {
                    v *= m;
                } else {
                    break;
                }
                j += 1;
            }
            keep(v);
            i = j;
            continue;
        }
        // Number words: total += current × scale at each scale word; "hundred" multiplies current.
        let small = |w: &str| SMALL.iter().find(|(s, _)| *s == w).map(|x| x.1);
        let is_word = |w: &str| small(w).is_some() || w == "hundred" || w == "dozen" || SCALES.iter().any(|(s, _)| *s == w);
        if is_word(t) {
            let (mut total, mut current, mut seen_number, mut j) = (0.0f64, 0.0f64, false, i);
            while j < toks.len() && (is_word(toks[j]) || (toks[j] == "and" && seen_number)) {
                let w = toks[j];
                if let Some(n) = small(w) {
                    // "a"/"an" only count right before a multiplier ("a hundred", "a million").
                    if (w == "a" || w == "an") && seen_number {
                        break;
                    }
                    current += n;
                    if w != "a" && w != "an" {
                        seen_number = true;
                    }
                } else if w == "hundred" || w == "dozen" {
                    current = current.max(1.0) * if w == "dozen" { 12.0 } else { 100.0 };
                    seen_number = true;
                } else if let Some((_, m)) = SCALES.iter().find(|(s, _)| *s == w) {
                    total += current.max(1.0) * m;
                    current = 0.0;
                    seen_number = true;
                }
                j += 1;
            }
            if seen_number {
                keep(total + current);
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    best
}

const KIND_WORDS: &[(ItemKind, &[&str])] = &[
    (
        ItemKind::Army,
        &[
            "army", "armies", "host", "hosts", "legion", "legions", "soldier", "soldiers", "troops", "warriors",
            "horde", "hordes", "battalion", "regiment", "militia", "knights", "infantry", "cavalry", "warband",
            "banner", "banners", "crusade", "platoon", "division", "divisions",
        ],
    ),
    (
        ItemKind::Fleet,
        &[
            "fleet", "fleets", "armada", "navy", "ships", "warships", "galleons", "airships", "squadron", "zeppelins",
            "aircraft", "spaceships",
        ],
    ),
    (
        ItemKind::Creature,
        &[
            "dragon", "dragons", "beast", "beasts", "swarm", "swarms", "monster", "monsters", "kraken", "wolves",
            "locusts", "golem", "golems", "titan", "phoenix", "hydra", "creatures", "giants", "behemoth",
        ],
    ),
    (
        ItemKind::Structure,
        &[
            "fortress", "castle", "tower", "wall", "walls", "citadel", "temple", "palace", "bridge", "city", "keep",
            "pyramid", "cathedral", "monument", "ziggurat", "statue", "colossus", "lighthouse", "dam", "gate",
            "stronghold", "bastion",
        ],
    ),
    (
        ItemKind::Weapon,
        &[
            "sword", "spear", "axe", "hammer", "bow", "blade", "dagger", "lance", "cannon", "gun", "trident", "mace",
            "scythe", "whip", "halberd", "weapon", "crossbow", "railgun",
        ],
    ),
    (
        ItemKind::Armor,
        &["armor", "armour", "shield", "helm", "helmet", "breastplate", "gauntlets", "mail", "aegis", "cloak"],
    ),
    (
        ItemKind::Relic,
        &["relic", "relics", "grail", "bones", "shroud", "reliquary", "idol", "tablet", "scripture", "chalice", "ark"],
    ),
    (
        ItemKind::Artifact,
        &[
            "artifact", "artefact", "amulet", "ring", "crown", "orb", "staff", "wand", "talisman", "gem", "crystal",
            "scepter", "sceptre", "mirror", "lamp", "key", "book", "tome", "compass", "hourglass",
        ],
    ),
    (
        ItemKind::Phenomenon,
        &["storm", "aurora", "moon", "sun", "eclipse", "comet", "rainbow", "fog", "plague", "miracle", "vortex"],
    ),
];

/// The kind of thing `text` asks for (rules fallback for the hand mind's `manifest.kind`): the first kind, in the
/// order army, fleet, creature, structure, weapon, armor, relic, artifact, phenomenon, whose words appear; else
/// [`ItemKind::Other`].
pub fn classify_kind(text: &str) -> ItemKind {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    KIND_WORDS
        .iter()
        .find(|(_, ws)| words.iter().any(|w| ws.contains(w)))
        .map(|(k, _)| *k)
        .unwrap_or(ItemKind::Other)
}

/// A request read from an intent's words by rules alone (the fallback when the hand mind gave no `manifest` field or
/// was late): kind, count (hosts default to the tier the magnitude implies: 10^(2+magnitude/2)), magnitude.
pub fn request_from_text(span: &str, magnitude: u8) -> ManifestRequest {
    let kind = classify_kind(span);
    let count = parse_count(span)
        .filter(|c| *c >= 1.0)
        .unwrap_or_else(|| {
            // "Summon a dragon" is one beast (tiered by magnitude); armies and fleets default to the magnitude's size.
            if kind.is_host() && kind != ItemKind::Creature {
                10f64.powf(2.0 + magnitude.min(10) as f64 / 2.0).round()
            } else {
                1.0
            }
        });
    ManifestRequest {
        kind,
        name: String::new(),
        count,
        magnitude: magnitude.min(10),
        span: span.to_string(),
        offer_bolts: 0,
        // "…, no debt" / "without debt": refuse rather than borrow the shortfall from Consequence.
        accept_debt: !["no debt", "without debt", "debt-free", "debt free"].iter().any(|k| span.to_lowercase().contains(k)),
        god: None,
    }
}

/// A rules-written name for a made item (the fallback when no LLM names it): "Banner of the Horde of 100,000,000",
/// "Relic-tier Sword", "the Fortress".
pub fn rules_name(kind: ItemKind, tier: Tier, count: f64) -> String {
    if kind == ItemKind::Army {
        format!("Banner of the {} of {}", title(tier.host_word()), group_digits(count))
    } else if kind == ItemKind::Fleet {
        format!("Armada of {}", group_digits(count))
    } else if kind == ItemKind::Creature && count > 1.0 {
        format!("Swarm of {} Creatures", group_digits(count))
    } else if kind == ItemKind::Creature {
        format!("{} Beast", title(tier.as_str()))
    } else {
        let k = match kind {
            ItemKind::Other => "Thing".to_string(),
            k => title(&format!("{k:?}")),
        };
        format!("{} {}", title(tier.as_str()), k)
    }
}

/// [`rules_name`] using the thing's own noun from the request when there is one ("Relic Sword", "Wonder Fortress",
/// "Legend Dragon"; hosts keep their banner names). Never a verb: the noun is one of the kind's words.
pub fn rules_name_for(kind: ItemKind, tier: Tier, count: f64, span: &str) -> String {
    let lower = span.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let noun = KIND_WORDS
        .iter()
        .find(|(k, _)| *k == kind)
        .and_then(|(_, ws)| words.iter().find(|w| ws.contains(w)))
        .map(|w| title(w));
    match noun {
        Some(n) if !(kind == ItemKind::Army || kind == ItemKind::Fleet || (kind == ItemKind::Creature && count > 1.0)) => {
            format!("{} {n}", title(tier.as_str()))
        }
        _ => rules_name(kind, tier, count),
    }
}

/// A name as a comparable key: lowercased, possessives (`'s`) and punctuation dropped, leading articles dropped, words
/// joined by one space. `"Bot Kestrel's Right Hand"` and `"a bot kestrel right hand"` are the same key.
pub fn name_key(name: &str) -> String {
    let lower = name.to_lowercase().replace('\u{2019}', "'");
    let words: Vec<String> = lower
        .split_whitespace()
        .map(|w| {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
            w.strip_suffix("'s").unwrap_or(w).to_string()
        })
        .collect();
    let clean: String = words.join(" ").chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    clean.split_whitespace().skip_while(|w| matches!(*w, "the" | "a" | "an")).collect::<Vec<_>>().join(" ")
}

/// Words that end an object's noun phrase in a sentence ("a rusted knife AMONG the trout").
const PHRASE_STOPS: &[&str] = &[
    "among", "amongst", "in", "inside", "at", "on", "onto", "upon", "from", "with", "without", "into", "to", "toward",
    "towards", "near", "by", "under", "over", "beneath", "below", "above", "behind", "beside", "between", "for",
    "and", "or", "but", "that", "which", "who", "whom", "while", "as", "where", "when", "then", "after", "before",
    "is", "was", "are", "were", "lies", "lay", "lying", "left", "half", "out", "off", "up", "down", "around", "through",
];

/// The name of the object `text` describes: the first noun phrase after an article or a possessive ("finds a rusted
/// fish-gutting knife among the trout." → `"Rusted Fish-Gutting Knife"`), at most five words, in title case. `None`
/// when the words name no object.
pub fn object_name(text: &str) -> Option<String> {
    let lower = text.to_lowercase().replace('\u{2019}', "'");
    let mut words: Vec<(String, bool)> = vec![];
    for raw in lower.split_whitespace() {
        let ends = raw.ends_with(['.', ',', ';', ':', '!', '?', ')', '"']);
        let w: String = raw.trim_matches(|c: char| !c.is_alphanumeric()).chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '\'').collect();
        words.push((w, ends));
    }
    let start = words.iter().position(|(w, _)| matches!(w.as_str(), "a" | "an" | "the" | "some" | "his" | "her" | "their" | "its" | "my" | "your"))?;
    let mut out: Vec<&str> = vec![];
    for (w, ends) in &words[start + 1..] {
        if w.is_empty() || PHRASE_STOPS.contains(&w.as_str()) || matches!(w.as_str(), "a" | "an" | "the") {
            break;
        }
        out.push(w);
        if *ends || out.len() >= 5 {
            break;
        }
    }
    while out.last().is_some_and(|w| matches!(*w, "of" | "the" | "and")) {
        out.pop();
    }
    if out.is_empty() {
        return None;
    }
    Some(out.iter().enumerate().map(|(i, w)| if i > 0 && *w == "of" { w.to_string() } else { title(w) }).collect::<Vec<_>>().join(" "))
}

fn title(s: &str) -> String {
    s.split(['-', ' '])
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(if s.contains('-') { "-" } else { " " })
}

/// `100000000` → `"100,000,000"` (counts past 1e15 are capped before they get here).
pub fn group_digits(count: f64) -> String {
    let n = count.clamp(0.0, MAX_COUNT).round() as u64;
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(tier: Tier) -> OddsInputs {
        OddsInputs {
            tier,
            cost: cost(tier, 99, 0, true).unwrap(),
            favor: 0.0,
            stance: None,
            entropy: 0.0,
            recent: 0,
        }
    }

    #[test]
    fn name_keys_and_object_names() {
        assert_eq!(name_key("Bot Kestrel's Right Hand"), "bot kestrel right hand");
        assert_eq!(name_key("a bot kestrel right hand"), name_key("Bot Kestrel\u{2019}s Right Hand"));
        assert_eq!(name_key("  "), "");
        // The live case (match ae54b0af): the object, not the finder.
        assert_eq!(object_name("finds a rusted fish-gutting knife among the trout.").as_deref(), Some("Rusted Fish-Gutting Knife"));
        assert_eq!(object_name("pulls the vial of salt from the river").as_deref(), Some("Vial of Salt"));
        assert_eq!(object_name("lifts her mother's ring, then runs").as_deref(), Some("Mother's Ring"));
        assert_eq!(object_name("a sword").as_deref(), Some("Sword"));
        assert_eq!(object_name("wanders and wonders"), None);
        assert_eq!(object_name("finds the"), None);
    }

    #[test]
    fn ids_print_and_parse() {
        assert_eq!(HandDemigodId(3).to_string(), "hd:3");
        assert_eq!(ManifestationId::parse("mf:12"), Some(ManifestationId(12)));
        assert_eq!(ManifestationId::parse("hd:12"), None);
        assert_eq!(HandDemigodId::parse("hd:x"), None);
        // Neither prefix collides with an existing ref prefix.
        for p in [HandDemigodId::PREFIX, ManifestationId::PREFIX] {
            assert!(format!("{p}:0").parse::<Ref>().is_err(), "{p} is already a Ref prefix");
        }
    }

    #[test]
    fn host_tiers_by_count() {
        assert_eq!(tier_for_count(1.0), Tier::Trinket);
        assert_eq!(tier_for_count(12.0), Tier::Trinket);
        assert_eq!(tier_for_count(13.0), Tier::Artifact);
        assert_eq!(tier_for_count(1_000.0), Tier::Artifact);
        assert_eq!(tier_for_count(50_000.0), Tier::Relic);
        assert_eq!(tier_for_count(1e6), Tier::Wonder);
        assert_eq!(tier_for_count(5e6), Tier::Legend);
        // The designer's banners: a hundred million is a horde (myth), one more is a world-host.
        assert_eq!(tier_for_count(100_000_000.0), Tier::Myth);
        assert_eq!(tier_for_count(100_000_001.0), Tier::Titan);
        assert_eq!(tier_for_count(1e9), Tier::Titan);
        assert_eq!(tier_for_count(8e9), Tier::Apocalypse);
        assert_eq!(tier_for_count(MAX_COUNT), Tier::Apocalypse);
        assert_eq!(tier_for_count(f64::NAN), Tier::Trinket);
        assert_eq!(Tier::Apocalypse.host_word(), "world-army");
    }

    #[test]
    fn classify_hosts_and_objects() {
        // Hosts: the bigger of count and magnitude.
        assert_eq!(classify(ItemKind::Army, 1e8, 3), Tier::Myth);
        assert_eq!(classify(ItemKind::Army, 10.0, 9), Tier::Titan);
        assert_eq!(classify(ItemKind::Fleet, 300.0, 0), Tier::Artifact);
        // Objects: magnitude, bumped for mass copies.
        assert_eq!(classify(ItemKind::Weapon, 1.0, 1), Tier::Trinket);
        assert_eq!(classify(ItemKind::Relic, 1.0, 5), Tier::Wonder);
        assert_eq!(classify(ItemKind::Weapon, 5_000.0, 4), Tier::Wonder);
        assert_eq!(classify(ItemKind::Weapon, 2e6, 4), Tier::Legend);
        assert_eq!(classify(ItemKind::Structure, 1.0, 10), Tier::Apocalypse);
        assert_eq!(classify(ItemKind::Artifact, 1e9, 10), Tier::Apocalypse, "never past the top");
        // Tiers are ordered and serialize by name.
        assert!(Tier::Myth > Tier::Wonder);
        assert_eq!(serde_json::to_string(&Tier::Myth).unwrap(), "\"myth\"");
    }

    #[test]
    fn costs_and_debt() {
        // Free tiers cost nothing, even with an empty purse.
        assert_eq!(cost(Tier::Artifact, 0, 0, false).unwrap().bolts, 0);
        // Myth costs 3: paid in full, plus an offer as far as the purse goes.
        let c = cost(Tier::Myth, 5, 4, true).unwrap();
        assert_eq!((c.required, c.bolts, c.extra, c.missing, c.debt), (3, 5, 2, 0, 0.0));
        // Short: the rest is debt at 0.6 × (1 + tier/4) per bolt.
        let c = cost(Tier::Myth, 1, 0, true).unwrap();
        assert_eq!((c.bolts, c.missing), (1, 2));
        assert!((c.debt - 2.0 * 0.6 * (1.0 + 5.0 / 4.0)).abs() < 1e-6);
        // Short with debt refused: refused.
        assert_eq!(cost(Tier::Apocalypse, 2, 0, false), Err(Refusal::CantPay { required: 6, have: 2 }));
    }

    #[test]
    fn base_odds_match_the_table() {
        for t in Tier::ALL {
            let o = odds(&inputs(t));
            assert!((o.success - TIER_BASE_P[t.index()]).abs() < 1e-9, "{t:?}: {}", o.success);
        }
        // Bigger tiers are always harder, all else equal.
        let ps: Vec<f64> = Tier::ALL.iter().map(|t| odds(&inputs(*t)).success).collect();
        assert!(ps.windows(2).all(|w| w[0] > w[1]));
    }

    #[test]
    fn modifiers_move_the_odds() {
        let base = odds(&inputs(Tier::Myth)).success;
        // Extra bolts help, up to four of them.
        let mut i = inputs(Tier::Myth);
        i.cost = cost(Tier::Myth, 99, 4, true).unwrap();
        let four = odds(&i).success;
        i.cost = cost(Tier::Myth, 99, 9, true).unwrap();
        assert!(four > base);
        assert_eq!(odds(&i).success, four, "the fifth bolt buys nothing");
        // logit(0.38) + 4 × 0.35, by hand.
        let expect = 1.0 / (1.0 + (-((0.38f64 / 0.62).ln() + 1.4)).exp());
        assert!((four - expect).abs() < 1e-12);
        // Favor: ±10 is ±1.2 log-odds.
        let mut i = inputs(Tier::Myth);
        i.favor = 10.0;
        let fav = odds(&i);
        assert!((fav.terms.favor - 1.2).abs() < 1e-12 && fav.success > base);
        i.favor = -10.0;
        assert!(odds(&i).success < base);
        // Debt-financed bolts hurt a little.
        let mut i = inputs(Tier::Myth);
        i.cost = cost(Tier::Myth, 0, 0, true).unwrap();
        assert!(odds(&i).success < base);
        // Repetition hurts, capped at four.
        let mut i = inputs(Tier::Myth);
        i.recent = 4;
        let r4 = odds(&i).success;
        i.recent = 40;
        assert!(r4 < base && odds(&i).success == r4);
    }

    #[test]
    fn entropy_loves_big_things_and_twists_them() {
        let mut big = inputs(Tier::Myth);
        let calm = odds(&big);
        big.entropy = 100.0;
        let mad = odds(&big);
        assert!(mad.success > calm.success, "Entropy helps wonders and up");
        assert!((mad.twist - 0.34).abs() < 1e-12 && (calm.twist - 0.04).abs() < 1e-12);
        assert!(mad.backfire > calm.backfire);
        // Below Wonder, Entropy doesn't help the odds (it still twists).
        let mut small = inputs(Tier::Relic);
        let a = odds(&small).success;
        small.entropy = 100.0;
        assert_eq!(odds(&small).success, a);
    }

    #[test]
    fn god_stances() {
        let mut i = inputs(Tier::Titan);
        let none = odds(&i).success;
        i.stance = Some(GodStance::Grant);
        assert!(odds(&i).success > none);
        i.stance = Some(GodStance::Twist);
        let tw = odds(&i);
        assert_eq!(tw.twist, 1.0);
        assert_eq!(roll(&tw, 0.0), Outcome::Twisted, "a Twist ruling makes every success a twist");
        i.stance = Some(GodStance::Refuse);
        let no = odds(&i);
        assert_eq!(no.success, 0.0);
        assert_eq!(roll(&no, 0.0), Outcome::Refused);
    }

    #[test]
    fn odds_stay_in_range() {
        let mut i = inputs(Tier::Trinket);
        i.favor = 10.0;
        i.stance = Some(GodStance::Grant);
        i.cost = cost(Tier::Trinket, 99, 9, true).unwrap();
        assert_eq!(odds(&i).success, P_MAX);
        let mut i = inputs(Tier::Apocalypse);
        i.favor = -10.0;
        i.recent = 9;
        i.cost = cost(Tier::Apocalypse, 0, 0, true).unwrap();
        assert_eq!(odds(&i).success, P_MIN);
    }

    #[test]
    fn roll_partitions_the_unit_interval() {
        let o = Odds {
            success: 0.5,
            twist: 0.2,
            partial: 0.4,
            backfire: 0.1,
            terms: OddsTerms::default(),
        };
        assert_eq!(roll(&o, 0.0), Outcome::Success);
        assert_eq!(roll(&o, 0.399), Outcome::Success);
        assert_eq!(roll(&o, 0.40), Outcome::Twisted);
        assert_eq!(roll(&o, 0.499), Outcome::Twisted);
        assert_eq!(roll(&o, 0.5), Outcome::Partial);
        assert_eq!(roll(&o, 0.699), Outcome::Partial);
        assert_eq!(roll(&o, 0.701), Outcome::Failed);
        assert_eq!(roll(&o, 0.949), Outcome::Failed);
        assert_eq!(roll(&o, 0.951), Outcome::Backfired);
        assert_eq!(roll(&o, 0.999_999), Outcome::Backfired);
        // Over many evenly spaced draws the shares match the odds.
        let n = 100_000;
        let made = (0..n).filter(|k| roll(&o, *k as f64 / n as f64).made()).count();
        assert!((made as f64 / n as f64 - (0.5 + 0.5 * 0.4)).abs() < 1e-3);
    }

    #[test]
    fn partials_backfires_and_musters() {
        assert_eq!(partial_of(ItemKind::Army, 1e8, Tier::Myth), (1e7, Tier::Legend));
        assert_eq!(partial_of(ItemKind::Army, 5.0, Tier::Trinket), (1.0, Tier::Trinket));
        assert_eq!(partial_of(ItemKind::Weapon, 1.0, Tier::Relic), (1.0, Tier::Artifact));
        assert_eq!(backfire_damage(Tier::Apocalypse), 2.25);
        assert_eq!(entropy_rise(Tier::Relic), 0.0);
        assert!((entropy_rise(Tier::Myth) - 0.45).abs() < 1e-6);
        assert_eq!(muster_size(0.0), 0);
        assert_eq!(muster_size(f64::NAN), 0);
        assert_eq!(muster_size(30_000.0), 30_000);
        let m = muster_size(1e6);
        assert!(m > 50_000 && m < 250_000, "{m}");
        assert_eq!(muster_size(1e8), MUSTER_CAP);
        assert_eq!(muster_size(MAX_COUNT), MUSTER_CAP);
    }

    #[test]
    fn counts_from_text() {
        let c = |s: &str| parse_count(s);
        assert_eq!(c("raise banners of 100,000,000 soldiers"), Some(1e8));
        assert_eq!(c("an army of a hundred million"), Some(1e8));
        assert_eq!(c("two hundred thousand knights"), Some(2e5));
        assert_eq!(c("1.5bn locusts"), Some(1.5e9));
        assert_eq!(c("100M troops"), Some(1e8));
        assert_eq!(c("10k archers and 3 dragons"), Some(1e4), "the largest count wins");
        assert_eq!(c("100 million"), Some(1e8));
        assert_eq!(c("2.5 billion"), Some(2.5e9));
        assert_eq!(c("a dozen golems"), Some(12.0));
        assert_eq!(c("1e8 soldiers"), Some(1e8));
        assert_eq!(c("one million and five"), Some(1e6 + 5.0));
        assert_eq!(c("countless legions"), Some(MAX_COUNT));
        assert_eq!(c("a sword"), None, "a lone 'a' is not a count");
        assert_eq!(c("build a fortress"), None);
        assert_eq!(c("999999999999999999999 ants"), Some(MAX_COUNT));
        assert_eq!(c(""), None);
    }

    #[test]
    fn kinds_from_text() {
        assert_eq!(classify_kind("Raise BANNERS of 100,000,000"), ItemKind::Army);
        assert_eq!(classify_kind("summon an armada of airships"), ItemKind::Fleet);
        assert_eq!(classify_kind("a dragon made of glass"), ItemKind::Creature);
        assert_eq!(classify_kind("a citadel on the moon"), ItemKind::Structure, "structure before phenomenon");
        assert_eq!(classify_kind("forge a sword of lightning"), ItemKind::Weapon);
        assert_eq!(classify_kind("the crown of Ys"), ItemKind::Artifact);
        assert_eq!(classify_kind("make it rain soup"), ItemKind::Other);
        assert_eq!(ItemKind::Army.patron(), GodId(4));
        assert_eq!(ItemKind::Army.unit(), "soldiers");
    }

    #[test]
    fn requests_and_names_by_rules() {
        let r = request_from_text("raise banners of 100,000,000 soldiers", 7);
        assert_eq!((r.kind, r.count, r.accept_debt), (ItemKind::Army, 1e8, true));
        assert_eq!(classify(r.kind, r.count, r.magnitude), Tier::Myth);
        // A host with no number gets the magnitude's size.
        assert_eq!(request_from_text("raise an army", 4).count, 10_000.0);
        assert_eq!(request_from_text("forge a sword", 3).count, 1.0);
        assert!(request_from_text("forge a sword", 3).accept_debt);
        assert!(!request_from_text("forge a sword, no debt", 3).accept_debt);
        assert_eq!(rules_name(ItemKind::Army, Tier::Myth, 1e8), "Banner of the Horde of 100,000,000");
        assert_eq!(rules_name(ItemKind::Army, Tier::Apocalypse, 9e9), "Banner of the World-Army of 9,000,000,000");
        assert_eq!(rules_name(ItemKind::Weapon, Tier::Relic, 1.0), "Relic Weapon");
        assert_eq!(group_digits(1234.0), "1,234");
        assert_eq!(group_digits(12.0), "12");
    }

    /// Schema-4 items (no schema-5 fields) load with the documented defaults once `ItemExt` is flattened in.
    #[test]
    fn item_ext_flattens_with_defaults() {
        #[derive(Serialize, Deserialize)]
        struct ItemV5 {
            id: ItemId,
            name: String,
            #[serde(flatten)]
            ext: ItemExt,
        }
        let old: ItemV5 = serde_json::from_str(r#"{"id":4,"name":"Rusty Pipe"}"#).unwrap();
        assert_eq!(old.ext, ItemExt::default());
        assert_eq!((old.ext.count, old.ext.state), (1.0, ItemState::Held));
        let horde = ItemV5 {
            id: ItemId(9),
            name: "Banner".into(),
            ext: ItemExt {
                kind: ItemKind::Army,
                tier: Tier::Myth,
                count: 1e8,
                unit: "soldiers".into(),
                state: ItemState::Deployed,
                manifestation: Some(ManifestationId(2)),
                banner: Some(Banner {
                    device: "a stag's head".into(),
                    colors: ["copper".into(), "teal".into()],
                    motto: None,
                }),
                history: vec![ItemEvent {
                    beat: 3,
                    state: ItemState::Deployed,
                    holder: Some(Ref::Demigod(DemigodId(0))),
                    by: None,
                    row: Some(LedgerId(40)),
                    count: Some(1e8),
                }],
                ..ItemExt::default()
            },
        };
        let j = serde_json::to_value(&horde).unwrap();
        assert_eq!(j["count"], 1e8);
        assert_eq!(j["tier"], "myth");
        assert_eq!(j["manifestation"], 2);
        let back: ItemV5 = serde_json::from_value(j).unwrap();
        assert_eq!(back.ext, horde.ext);
    }

    #[test]
    fn manifestation_roundtrips() {
        let req = request_from_text("raise banners of 100,000,000 soldiers", 7);
        let tier = classify(req.kind, req.count, req.magnitude);
        let c = cost(tier, 2, 0, true).unwrap();
        let o = odds(&OddsInputs {
            tier,
            cost: c,
            favor: 2.0,
            stance: None,
            entropy: 35.0,
            recent: 1,
        });
        let m = Manifestation {
            id: ManifestationId(0),
            beat: 5,
            by: Ref::Agent(AgentId(4)),
            demigod: DemigodId(0),
            hand: Some(Hand::Left),
            agent: Some(AgentId(4)),
            intent: Some(IntentId(11)),
            request: req,
            place: Some(PlaceId(3)),
            at: Some([2.35, 48.85]),
            verdict: ManifestVerdict {
                outcome: roll(&o, 0.1),
                tier,
                odds: o,
                roll: 0.1,
                cost: c,
                god: ItemKind::Army.patron(),
                favor: 2.0,
                stance: None,
                entropy: 35.0,
                reasons: vec!["myth tier: 3 bolts".into()],
                flavor: Flavor::Rules,
            },
            item: Some(ItemId(7)),
            rows: vec![LedgerId(40), LedgerId(41)],
            public: tier >= PUBLIC_TIER,
        };
        assert_eq!(m.verdict.outcome, Outcome::Success);
        assert!(m.public);
        let j = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Manifestation>(&j).unwrap(), m);
    }
}
