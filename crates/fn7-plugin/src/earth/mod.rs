//! THE EARTH SIMULATOR (state schema 13, room setting `earth_sim_enabled`; docs/EARTH_SIM.md): a server-side,
//! deterministic, low-resolution voxel simulation of what the god powers do to the planet.
//!
//! - [`grid`]: the regions-level hex grid of the whole globe and its derived base terrain.
//! - [`sim`]: the field (sparse dirty cells over the base), the phenomena and one beat's step. Pure.
//! - [`codec`]: how cells are packed in the state and in keyframes.
//! - [`rules`]: the simulator inside a match: casts feed it, it steps each beat, and what it did goes back through
//!   the rules (damage, crowds, settlements, roads, shrines, encounters, the DM's memory, public events).
//! - [`view`]: what clients get (`earth.delta`, `earth.phenomena`, `game.earth`, `game.earth_at`), finer zoom levels
//!   derived from the coarse truth, and the ASCII map.

pub mod codec;
pub mod grid;
pub mod rules;
pub mod sim;
pub mod view;

#[cfg(test)]
mod rules_tests;
#[cfg(test)]
pub(crate) mod tests;
