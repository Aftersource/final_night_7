//! The table of consequence: one append-only row per resolved act. Knowledge spreads along group overlaps, debts come due,
//! and `cause` links rows into Final Destination chains.

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::refs::{GroupSet, Ref};

/// Who produced a row or a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "by")]
pub enum Source {
    /// The world resolver (ordinary acts, per vicinity).
    Resolver,
    God {
        god: GodId,
        ruling: Option<RulingId>,
    },
    Agent {
        agent: AgentId,
    },
    /// Deterministic rules (zone, mood drift, debt collection, fallbacks).
    Rules,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DebtStatus {
    Open,
    /// Pushed back by Time; `due_beat` moved and `amount` grew.
    Delayed,
    Collected,
    /// Settled on the Final Night.
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Debt {
    pub owed_by: Ref,
    /// The god it is owed to (usually Consequence; a god's price is owed to that god).
    pub owed_to: GodId,
    /// In damage points (0–10 scale), collected as an accident chain.
    pub amount: f32,
    pub due_beat: u32,
    pub delays: u8,
    pub status: DebtStatus,
    /// The row that collected it.
    pub collected_by: Option<LedgerId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerRow {
    pub id: LedgerId,
    pub beat: u32,
    pub source: Source,
    pub actor: Ref,
    /// Canonical verb ("burn", "bless", "recruit", "rewind").
    pub verb: String,
    /// One line, prompt-ready, with no engine ids in it.
    pub act: String,
    pub targets: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceId>,
    /// 0–10, same scale as Final Night damage.
    pub magnitude: u8,
    /// -5 (atrocity) .. +5 (miracle), as the affected see it.
    pub valence: i8,
    /// Directly saw it happen.
    pub witnessed_by: Vec<Ref>,
    /// Groups it has spread to so far (grows each beat along overlaps).
    pub known_by: GroupSet,
    /// The beat each group in `known_by` learned it, in the order learned (the witnesses' groups at `beat`).
    /// Empty in schema-1 states.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known_at: Vec<(GroupId, u32)>,
    /// Individuals told directly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub told_to: Vec<Ref>,
    /// Erased from these minds by Memory (the row itself stays).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub erased_from: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debt: Option<Debt>,
    /// The row that caused this one (chains).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<LedgerId>,
    /// The intent that started it, if a player's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentId>,
    /// Where it happened (`[lon, lat]`): a bolt's strike point, else the target's or actor's body, else the place's
    /// center. Set when the row is appended (schema 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<crate::society::LonLat>,
}

impl LedgerRow {
    /// The beat `group` learned this row, if recorded in `known_at`.
    pub fn known_since(&self, group: GroupId) -> Option<u32> {
        self.known_at.iter().find(|(g, _)| *g == group).map(|x| x.1)
    }
}

/// One hop of news: `row` became known to `group` through overlap with `via`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpreadHop {
    pub row: LedgerId,
    pub group: GroupId,
    pub via: GroupId,
    pub beat: u32,
    /// The chance it had.
    pub p: f32,
}

/// Old rows folded into one summary to bound state size. The originals stay in the recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerFold {
    pub from: LedgerId,
    pub to: LedgerId,
    pub beats: (u32, u32),
    pub summary: String,
    pub actors: Vec<Ref>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub rows: Vec<LedgerRow>,
    pub folds: Vec<LedgerFold>,
}

impl Ledger {
    pub fn next_id(&self) -> LedgerId {
        LedgerId::from_ix(self.rows.len())
    }
    pub fn get(&self, id: LedgerId) -> Option<&LedgerRow> {
        self.rows.get(id.ix())
    }
    pub fn open_debts(&self) -> impl Iterator<Item = &LedgerRow> {
        self.rows.iter().filter(|r| {
            matches!(
                r.debt,
                Some(Debt {
                    status: DebtStatus::Open | DebtStatus::Delayed,
                    ..
                })
            )
        })
    }
}
