//! Dense, typed ids. Every table in [`crate::World`] is a `Vec` indexed by its id, so lookups are O(1), iteration order is
//! deterministic, and ids are stable for the life of a match (rows are never removed, only marked).

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! id {
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
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}:{}", $prefix, self.0)
            }
        }
    };
}

id!(/// A player's demigod (one per seat).
    DemigodId, "dg");
id!(/// A memorized individual (materialized from a cluster, or created directly).
    CharId, "ch");
id!(/// A population aggregate: many unaddressed people with shared traits.
    ClusterId, "cl");
id!(/// A set in the society Venn diagram (faction, trade, faith, neighborhood...).
    GroupId, "gr");
id!(/// A god of the pantheon (index into the loaded pantheon definition).
    GodId, "gd");
id!(/// A district or site of the arena.
    PlaceId, "pl");
id!(/// An item in the world (summoned, found, echoed).
    ItemId, "it");
id!(/// Any agent mind: a hand, a subagent, a paladin's mind, a possession.
    AgentId, "ag");
id!(/// A row of the table of consequence.
    LedgerId, "lg");
id!(/// A cell of the society: a squad, gang, congregation or crew of 10–500 people out of a crowd (schema 4).
    CellId, "ce");
id!(/// A mob: cells merged under a ringleader (schema 4).
    MobId, "mb");
id!(/// A memory item in any store.
    MemoryId, "mm");
id!(/// A player intent parsed from speech.
    IntentId, "in");
id!(/// A god's ruling on an invocation.
    RulingId, "ru");
