//! Torogao's native 3D renderer — rung 1 (D-1216, amended by
//! D-1217 to a Vulkan-native substrate).
//!
//! Layering, per the backend-boundary guidance: everything
//! above [`backend`] is API-agnostic — GLB decoding, the sparse
//! morph packing, deterministic pose evaluation (reusing
//! `torogao-motion-clip`), the CPU morph+LBS reference, the
//! procedural arc scene, and the plate-comparison statistics.
//! The [`backend`] module defines the narrow packet surface
//! those layers talk through; the Vulkan implementation (ash
//! handles, memory, synchronisation) lives below it in
//! `backend::vulkan`, so a second backend could slot in as a
//! swap rather than a rewrite. Rung 1 ships the Vulkan backend
//! only.
//!
//! Determinism contract (gap analysis R6): no wall-clock input
//! anywhere in the render path; identical evaluated state must
//! produce a bit-identical frame on a fixed machine/adapter,
//! witnessed by per-frame SHA-256 of the readback pixels.

#[forbid(unsafe_code)]
pub mod accessor;
#[forbid(unsafe_code)]
pub mod arc;
// The backend module holds the boundary (safe) and its
// Vulkan implementation (the crate's one unsafe island:
// ash calls and mapped-memory writes, each with its
// safety obligations stated in place).
pub mod backend;
#[forbid(unsafe_code)]
pub mod cast;
#[forbid(unsafe_code)]
pub mod compare;
#[forbid(unsafe_code)]
pub mod error;
#[forbid(unsafe_code)]
pub mod math;
#[forbid(unsafe_code)]
pub mod model;
#[forbid(unsafe_code)]
pub mod morph;
#[forbid(unsafe_code)]
pub mod pose;
#[forbid(unsafe_code)]
pub mod reference;
#[forbid(unsafe_code)]
pub mod setscene;
#[forbid(unsafe_code)]
pub mod splat;
#[forbid(unsafe_code)]
pub mod splat_gpu;
#[forbid(unsafe_code)]
pub mod splat_light;
#[forbid(unsafe_code)]
pub mod splat_rotate;
#[forbid(unsafe_code)]
pub mod splat_spec;
#[forbid(unsafe_code)]
pub mod toon;

pub use error::RenderError;
