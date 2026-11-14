//! # fn7-server
//!
//! The Theatre server with Final Night 7 compiled in (`fn7-server`), and `fn7-sim`: whole matches in-process through the
//! `GamePlugin` trait, recorded to SQLite, and replay verification of a recording.

pub mod anthropic;
pub mod llm;
pub mod media;
pub mod replay;
