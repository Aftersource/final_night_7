//! # fn7-society
//!
//! The world below the gods, all deterministic rules over `fn7_model` tables (no LLM, no I/O):
//! - **setup**: seed one real city's districts, groups and clusters from toolkit geography plus a society template
//! - **resolve a mention** to a memorized character (fuzzy name/alias match), or **materialize** one from the best cluster
//!   with a seed per cluster and person (`Rng::derive(cluster.seed, &[index])`), so replay is exact
//! - **Venn queries**: members of A ∩ B ∖ C over clusters and characters
//! - **consequence**: append ledger rows from merged effects; spread `known_by` one hop per beat along group overlaps,
//!   weighted by permeability; mood, allegiance and worship drift; debts come due
//! - **invariants** the merge relies on (dead can't act, items are never lost, echo charges)
//!
//! Positions are `[lon, lat]` (`fn7_model::LonLat`); the arena city and terrain prose come from the toolkit's embedded
//! Natural Earth data. The society template (`data/society_template.json`) is compiled in.

mod effects;
pub mod geo;
mod mention;
pub mod names;
mod rules;
mod setup;
pub mod sim;
pub mod template;
mod util;
mod venn;
pub mod world_rules;

pub use effects::{
    apply, collect_debt, settle_debts_of, debt_for, delay_debt, raise_entropy, verb_of, BOLT_GIFT, DELAY_GROWTH,
    ENTROPY_PER_BOLT, ENTROPY_PER_GRANTED_BOLT, MAX_BOLTS, MAX_HEALTH, MOD_MAX_STACKS,
    PRICE_DEBT_DUE, REVIVE_FRACTION, TEMPO_MAX, TEMPO_MAX_BEATS, TEMPO_MIN,
};
pub use mention::{
    inheritable_rows, materialize, resolve_mention, tokens, Resolution, EXISTING_MIN,
    PLACE_ONLY_SCORE,
};
pub use rules::{
    arena_close_beats, arena_progress, arena_radius_at, capture_rewind, setting_f64, ARENA_EASE, drift, due_debts, shrink_arena, spread, ARENA_END_FRACTION,
    DRIFT_WINDOW, FOLLOW_JOIN, FOLLOW_LEAVE, SPREAD_DECAY, SPREAD_HORIZON,
};
pub use setup::{find_city, setup, split_hands, SIBLING_FRACTION_CITY, SIBLING_KM_PLANET, CITIES_PER_REGION, CITY_KM, PLANET_END_RADIUS_KM, PLANET_START_MARGIN_KM};
pub use util::{demigod_of, exists, is_alive, place_of};
pub use venn::{headcount, members, overlap_graph};
pub use world_rules::{
    army_dest, army_proposals, armies, entropy_key, entropy_level, entropy_proposals, entropy_prose, influence,
    mod_proposals, ARMY_CLASH_KM, ARMY_STRIKE_KM, ENTROPY_LEVELS,
};

/// Data types that live in the model, re-exported for callers of this crate.
pub use fn7_model::{SetupConfig, SpreadHop};

#[cfg(test)]
mod tests;
