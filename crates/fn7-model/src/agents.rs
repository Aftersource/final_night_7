//! Agent minds: hands (player orchestrators), subagents, gods' minds, paladins and possessions. One table, one lifecycle.

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::refs::Ref;
use crate::society::Hand;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum AgentKind {
    /// A player's orchestrator.
    Hand { demigod: DemigodId, hand: Hand },
    /// Spawned by a hand (or a god) for one task.
    Subagent { parent: AgentId },
    /// A god's own mind.
    God { god: GodId },
    /// The mind of a paladin's body.
    Paladin { god: GodId, body: CharId },
    /// A player steering a memorized character through a hand.
    Possession { parent: AgentId, body: CharId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Live,
    Done,
    /// Ran out of beats.
    Expired,
    /// Its body died or its parent was eliminated.
    Ended,
    /// Recalled by its parent.
    Recalled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub id: AgentId,
    pub kind: AgentKind,
    pub name: String,
    pub persona: String,
    /// The task, for subagents; standing orders for hands and gods.
    pub goal: String,
    /// Entities it may act on (empty = anything in reach).
    pub scope: Vec<Ref>,
    pub spawned_beat: u32,
    /// Beats it may live (None = unlimited).
    pub budget_beats: Option<u32>,
    pub status: AgentStatus,
    pub ended_beat: Option<u32>,
    /// Final report to the parent (also stored as a `Report` memory).
    pub report: Option<String>,
    pub steps: u32,
    /// Children spawned (for the orchestration tree view).
    pub children: Vec<AgentId>,
}

impl Agent {
    pub fn parent(&self) -> Option<AgentId> {
        match self.kind {
            AgentKind::Subagent { parent } | AgentKind::Possession { parent, .. } => Some(parent),
            _ => None,
        }
    }
    pub fn is_live(&self) -> bool {
        self.status == AgentStatus::Live
    }
}
