//! THE EARTH SIMULATOR's pure core (state schema 13; docs/EARTH_SIM.md): a server-side, deterministic,
//! low-resolution voxel simulation of what the god powers do to the planet. It knows nothing of the match: the
//! rules (`fn7-plugin::earth`) feed it inputs and read back what it did.
//!
//! - [`grid`]: the regions-level hex grid of the whole globe and its derived base terrain.
//! - [`sim`]: the field (sparse dirty cells over the base), the phenomena and one beat's step.
//! - [`codec`]: how cells are packed in the state and in keyframes.

pub mod codec;
pub mod grid;
pub mod sim;
