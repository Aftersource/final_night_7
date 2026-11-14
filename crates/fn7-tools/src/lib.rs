//! # fn7-tools
//!
//! Binaries (added as they're built): `fn7-sim` (whole match in the terminal), `fn7-agents` (AI players over WebSocket,
//! streaming fast commands per hand), `fn7-replay`, `fn7-fork`, `fn7-timeline`, and `fn7-project` (projects a recording's
//! events into typed SQL tables; see docs/DATA_MODEL.md).
//!
//! Library modules: [`project`] (the SQL projection behind `fn7-project`) and [`players`] (protocol builders, the scripted
//! phrase bank and the Claude client behind `fn7-agents`), and [`schemas`] (the client payload JSON Schemas behind
//! `fn7-schemas`, see docs/CLIENT_API.md).

pub mod players;
pub mod project;
pub mod schemas;
