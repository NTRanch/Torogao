//! torogao-scene: closed-form scene solves for self-contact
//! (D-1394 packet P08).
//!
//! Public contract, top down:
//!
//! 1. [`two_bone::solve`] places the middle joint (elbow) and the
//!    end joint (wrist) of a two-bone limb so the end reaches a
//!    target. One square root, no iteration: the same input gives
//!    the same bits on every run.
//! 2. [`anchor::SurfaceAnchor::resolve`] turns "this spot on this
//!    triangle, this far off the surface" into a point and an
//!    outward normal, so a hand target can ride a moving surface.
//!
//! Every function is a pure function of its arguments. Input that
//! cannot be solved (non-finite numbers, a zero-length bone, a
//! target on the root, a pole along the reach axis, a triangle
//! with no area) is rejected with a typed [`SceneError`]; nothing
//! is silently repaired except the two documented reach clamps.
//!
//! 3. [`curl::wrap`] lays a finger's segments on a curved surface
//!    (a circle in the finger's bend plane), one square root per
//!    segment, and gives the joint turns.
//! 4. [`squash::weight`] is the contact channel's drive of the
//!    dedicated, baked squash key (Owner ruling D-1415): press depth
//!    over baked depth, held to `[0, 1]`.

#![forbid(unsafe_code)]

pub mod anchor;
pub mod curl;
pub mod error;
pub mod follow;
pub mod instance;
pub mod squash;
pub mod two_bone;
pub mod vec3;

pub use error::SceneError;
pub use vec3::Vec3;

/// Lane-neutral shared schema; admission policy lives in [`instance`].
pub use lumen_scene_script as schema;
