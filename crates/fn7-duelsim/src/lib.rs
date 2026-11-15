//! # fn7-duelsim: the duel's physics, baked on the server
//!
//! The browser lab (`duel_lab/`) gave the duel real physics: objects with mass from their parts and materials, a
//! baked deterministic rigid-body simulation, fighters with mass from their skin's material, feet, hands and strikes
//! placed on real surfaces. This crate is that engine in plain Rust, run **once, on the server**, so every client
//! plays back the same baked result: the same on every screen, exact on rewind and resume, cheap for the client.
//!
//! - [`materials`]: the materials table, skins, figures (`data/materials.json`).
//! - [`shapes`], [`props`]: primitives and what is derived from a scene: mass, centre of mass, inertia, support
//!   polygon, tipping angle, surfaces and affordances.
//! - [`physics`]: the rigid-body engine. [`paths`]: the parkour paths resolved on real surfaces.
//! - [`bake`]: the incremental bake, one panel at a time. [`wire`]: the quantised keyframe encoding.
//! - [`seam`]: [`panel_sim`], the one function the duel rules call, and the JSON shapes.
//!
//! **Determinism.** A fixed 120 Hz step, one seeded RNG (its state is one u32), no clock, no hash maps, and `f64`
//! arithmetic restricted to `+ - * /` and `sqrt` in a fixed order with our own `sin` / `cos` / `atan2` / `cbrt`
//! ([`math`]): the same inputs give the same bytes on every 64-bit target. See docs/DUEL_SIM.md.

pub mod bake;
pub mod materials;
pub mod math;
pub mod paths;
pub mod physics;
pub mod props;
pub mod seam;
pub mod shapes;
pub mod wire;

pub use bake::{FighterSpec, PanelBake, Sim};
pub use seam::{bake, fighters_of, panel_sim, present_sim, scene_sim, seed32, with_sims, DuelBake, PanelSim};
