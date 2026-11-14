//! # fn7-server
//!
//! The Theatre server with Final Night 7 compiled in (`fn7-server`), and `fn7-sim`: whole matches in-process through the
//! `GamePlugin` trait, recorded in the core's JSONL format ([`record`]), and replay verification of a core recording
//! ([`replay`]).

pub mod anthropic;
pub mod llm;
pub mod media;
pub mod record;
pub mod replay;
