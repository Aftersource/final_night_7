//! # fn7-tools
//!
//! Binaries: `fn7-agents` (AI players over WebSocket, streaming fast commands per hand; client recording in the core's
//! JSONL format), `fn7-project` (loads core JSONL recordings into SQLite and projects their events into typed SQL tables;
//! see docs/DATA_MODEL.md) and `fn7-schemas`. `fn7-sim` and replay verification live in `fn7-server`. Fork, timeline
//! and state-at-a-round come from the Theatre's admin API (`/admin/*`, see README "Recording, replay and the admin
//! API"); FN7 writes no save files of its own.
//!
//! Library modules: [`ingest`] (core JSONL → SQLite), [`project`] (the SQL projection behind `fn7-project`), [`players`] (protocol builders, the scripted
//! phrase bank and the Claude client behind `fn7-agents`), and [`schemas`] (the client payload JSON Schemas behind
//! `fn7-schemas`, see docs/CLIENT_API.md).

pub mod ingest;
pub mod players;
pub mod project;
pub mod schemas;
