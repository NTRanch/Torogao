//! torogao-rig: INP1 puppet evaluation for the Torogao lane.
//!
//! This crate imports `.inx` puppet records (the INP1 container: a
//! `TRNSRTS\0` magic, a big-endian JSON payload, a `TEX_SECT` texture
//! section, and an optional `EXT_SECT` vendor section), evaluates them
//! with a fixed-step deterministic parameter/physics pipeline, and
//! rasterizes frames on the CPU at measurement grade.
//!
//! It is an independent implementation written for Torogao (D-876).
//! The only things shared with the lumen rig stack are the INP1 format
//! itself and the estate's gate/measurement conventions (frame
//! recipes, sweep protocol, comparison metric); no lumen code or crate
//! is used or copied.
//!
//! Evaluation contract (the production route, D-869):
//! - Every parameter applies at every value, including exactly
//!   `(0, 0)`; there is no rest-value skip.
//! - Frame lifecycle: reset working state, the driver sets parameter
//!   values, then `end_frame(dt)` applies all parameters, updates
//!   transforms, steps physics (when the record has physics nodes and
//!   `dt > 0`), resets working state again, writes the physics outputs
//!   back as parameter values, and applies everything a second time.
//! - Parameter application order is the record's file order, and all
//!   iteration in this crate is over vectors in file order, so two
//!   runs of the same scene in fresh processes are byte-identical.
//!
//! Bounds: input size, node/param/texture counts, mesh sizes, and
//! recursion depth are all capped by named constants in the importer;
//! violations are typed errors, never panics or truncation.

#![forbid(unsafe_code)]

pub mod error;
pub mod eval;
pub mod inp;
pub mod math;
pub mod model;
pub mod render;
pub mod scene;
pub mod texture;

pub use error::RigError;
pub use eval::Evaluator;
pub use inp::load_inp;
pub use model::Puppet;
pub use render::{Canvas, Placement, render_frame, render_onto};
