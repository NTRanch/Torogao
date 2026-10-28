//! Torogao's evaluation engine: a faithful port of the
//! reference puppet runtime — payload loading with its
//! strictness and its quirks, parameter/binding evaluation,
//! automations, and the fixed-step simple-physics drivers
//! whose series the physics preview records. All arithmetic
//! is f32 (f64 only at the clock and output boundaries),
//! with evaluation order preserved from the reference so
//! series agree bit for bit where the arithmetic allows.

#![forbid(unsafe_code)]

pub mod animation;
pub mod automation;
pub mod math;
pub mod node;
pub mod param;
pub mod physics;
pub mod puppet;
pub mod transform;

pub use animation::{Animation, AnimationLane, AnimationPlayback, AnimationPlayer, Keyframe};
pub use automation::Automation;
pub use math::{Mat3, Mat4, Vec2, Vec3, Vec4};
pub use node::{MeshData, Node, NodeArena, SimplePhysicsState};
pub use param::{Binding, MergeMode, Parameter};
pub use puppet::{Clock, Puppet, PuppetMeta, TextureSlotInfo};
pub use transform::Transform;

#[cfg(test)]
mod tests {
    /// The skeleton's one verifiable fact: the workspace member
    /// builds under its contracted name.
    #[test]
    fn workspace_member_builds_under_its_contracted_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-engine");
    }
}
