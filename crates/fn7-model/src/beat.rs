//! One beat: its record (every stage timed), the rewind ring, and the per-seat delta the client renders.

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::intent::Zoom;
use crate::refs::Ref;
use crate::society::LonLat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Stage {
    Intake,
    Route,
    Act,
    Merge,
    Consequence,
    Narrate,
    Broadcast,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StageTiming {
    pub ms: Vec<(Stage, f64)>,
    pub llm_calls: u32,
    pub llm_fallbacks: u32,
    pub llm_ms_max: f64,
}

/// Summary of one beat, kept in state (small) and recorded in full by the tracer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeatRecord {
    pub beat: u32,
    pub started_at_ms: i64,
    pub timing: StageTiming,
    pub utterances: u32,
    pub words: u32,
    pub intents: u32,
    pub proposals: u32,
    pub applied: u32,
    pub rows: (Option<LedgerId>, Option<LedgerId>),
    pub memorized: Vec<CharId>,
    pub rulings: Vec<RulingId>,
    pub spawned: Vec<AgentId>,
    pub eliminated: Vec<DemigodId>,
    /// The Narrator's caption.
    pub caption: String,
    /// Level of detail this beat (schema 4): how many nodes the rules stepped, which were in focus or got the LLM.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fidelity: Option<crate::hier::Fidelity>,
}

/// Rewindable fields of one entity at one beat (Time's power reads these).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewindEntry {
    pub who: Ref,
    pub pos: Option<LonLat>,
    pub health: Option<f32>,
    pub alive: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewindFrame {
    pub beat: u32,
    pub entries: Vec<RewindEntry>,
}

/// Fixed-length ring of recent frames.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RewindRing {
    pub cap: usize,
    pub frames: Vec<RewindFrame>,
}

impl RewindRing {
    pub fn push(&mut self, f: RewindFrame) {
        if self.cap == 0 {
            return;
        }
        if self.frames.len() == self.cap {
            self.frames.remove(0);
        }
        self.frames.push(f);
    }
    pub fn at(&self, beat: u32) -> Option<&RewindFrame> {
        self.frames.iter().rev().find(|f| f.beat <= beat)
    }
}

/// What one seat receives after a beat (`round.private`), shaped by its zoom. Built by `fn7-plugin` (`payload.rs`);
/// every id is a `Ref` string and floats are rounded at the payload layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SeatDelta {
    pub beat: u32,
    pub seat: u8,
    pub zoom: Zoom,
    /// The focus as an Entity object (the same shape as the seat view's `focus`), or null.
    pub focus: Option<serde_json::Value>,
    /// Ledger rows this seat knows about, new this beat, newest first (prose, with refs for the client to link).
    pub news: Vec<NewsLine>,
    pub you: YouLine,
    pub hands: Vec<HandLine>,
    pub omens: Vec<Omen>,
    /// The hierarchy at your zoom (schema 4): the node in view with its path, children, crowds and cells, people,
    /// armies with their chains of command, mobs and recent battles. Shape: `docs/CLIENT_API.md` ("Hierarchy").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hierarchy: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NewsLine {
    pub row: Ref,
    pub text: String,
    pub refs: Vec<Ref>,
    pub how: String,
    /// Where it happened, `[lon, lat]` (null only for bookkeeping with no place).
    pub at: Option<LonLat>,
    /// The region (planet) or district (city) it happened in.
    pub region: Option<String>,
}

/// A god's favor toward one demigod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FavorLine {
    pub god: String,
    pub god_ref: Ref,
    pub favor: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct YouLine {
    pub health: f32,
    pub pos: LonLat,
    pub debt_open: f32,
    pub favor: Vec<FavorLine>,
    pub followers: u32,
    pub echo_charges: u32,
    /// Lightning bolts you hold.
    pub bolts: u32,
    /// Rules 0.7.0: sparks you hold (the mini-power currency).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sparks: Option<u32>,
}

/// A god's ruling as a client sees it (public omens and your own invocations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Omen {
    pub god: String,
    pub god_ref: Ref,
    pub speech: String,
    /// Where it was invoked, `[lon, lat]`.
    pub at: Option<LonLat>,
    /// The region (planet) or district (city).
    pub region: Option<String>,
}

/// What kind of agent a subagent line is: a subagent, a possession (steers a body) or a paladin (a god's champion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SubagentKind {
    Subagent,
    Possession,
    Paladin,
}

/// A live subagent, possession or paladin, in every payload that lists one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SubagentLine {
    pub agent: Ref,
    pub name: String,
    pub goal: String,
    pub line: String,
    pub beats_left: Option<u32>,
    pub kind: SubagentKind,
    pub parent: Option<Ref>,
    pub body: Option<Ref>,
}

/// One hand in `round.private`: what it said back this beat and its live subagents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HandLine {
    pub agent: Ref,
    pub name: String,
    pub said: String,
    pub subagents: Vec<SubagentLine>,
}

/// One hand in the seat view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SeatHand {
    pub agent: Ref,
    pub name: String,
    pub live: bool,
    pub subagents: Vec<SubagentLine>,
    /// The hand's own portrait prompt (it is one of the player's two demigods): identity only, from its name and its
    /// name-keyed visual sheet, the same in every match. The server adds `portrait_image` beside it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub portrait_prompt: String,
    /// The hand's own diorama prompt (identity only, like `portrait_prompt`). The server adds `diorama_image`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub diorama_prompt: String,
    /// The hand's visual sheet: `{sheet_hash, traits: {build, face, outfit, colors, prop, emblem}}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual: Option<serde_json::Value>,
    /// Rules 0.7.0: the demigod this hand is (each hand is one of the player's two demigods; `seat_view.demigods[]`
    /// has it in full).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demigod: Option<Ref>,
    /// Rules 0.7.0: `left` or `right`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<crate::society::Hand>,
    /// Rules 0.7.0: that demigod's health (a seat sees its own) and whether it stands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alive: Option<bool>,
}
