//! torogao-gen — Torogao's image-generation core (Stage 1
//! inference proof, D-867): SDXL inference over a converted
//! single-file checkpoint, with a counter-based seeded RNG,
//! the ComfyUI-conventional sampler (DPM++ 2M, Karras
//! ladder), and the SDXL added conditioning that upstream
//! candle 0.11.1 does not expose (see the `sd` module for
//! the vendored UNet and why it is vendored).
//!
//! Authoring-side surface only, like torogao-gencond: no
//! serving, no queue, no callers yet — those are later
//! commissions gated on the Stage 1 parity pack.

#![deny(unsafe_code)]
#![deny(missing_docs)]

pub mod clip_vision;
pub mod conditioning;
pub mod controlnet;
pub mod error;
pub mod ipadapter;
pub mod limits;
pub mod pipeline;
pub mod rng;
pub mod sampler;
pub mod schedule;
/// Vendored candle-transformers stable-diffusion modules
/// (see the module's own header for provenance and the one
/// patch). Upstream code keeps upstream's documentation
/// state, so missing_docs is allowed inside this module
/// only; every torogao-gen module carries the crate deny.
#[allow(missing_docs)]
pub mod sd;
pub mod text;

pub use conditioning::{CnType, ConditioningSpec, ControlSpec, IpSpec};
pub use error::{GenError, Result};
pub use pipeline::{ConditioningAssets, GenParams, GeneratedImage, SdxlModel};
