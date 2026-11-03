//! torogao-vrm: VRM 1.0 export for Torogao.
//!
//! Torogao publishes its own cast as VRM — the avatar
//! ecosystem's exchange format (a glTF 2.0 extension family,
//! `VRMC_vrm` 1.0). This crate owns the whole concern:
//!
//! - [`profile`]: `vrm-profile/1`, the per-character permission
//!   record. VRM's meta permission fields are restrictive by
//!   default in every consumer, so an export takes its meta from
//!   a profile in which every permission field is stated
//!   explicitly — no defaults, and a loud refusal naming any
//!   field left unset.
//! - [`export`]: GLB + profile → VRM 1.0. Humanoid mapping from
//!   torogao-humanoid/1, expression binds from the model's morph
//!   targets and the governed expression map, `VRMC_springBone`
//!   when a spring record is supplied. Geometry, skins, and
//!   buffers are byte-preserved, proven by census inside the
//!   export itself.
//! - [`validate`]: the read-back reader. An export is only as
//!   good as its re-parse: humanoid completeness, expression
//!   binds, spring well-formedness, meta equal to the profile.
//!
//! Placement (D-749): this is a standalone crate in the Torogao
//! set. It takes no dependency on lumen or any other crate
//! outside the workspace's serde stack — the Torogao↔lumen
//! boundary stays file formats. The VRM 1.0 specification
//! (vrm-c/vrm-specification) is the contract; UniVRM and VRM4U
//! are behavioural oracles, never code sources.

#![forbid(unsafe_code)]

pub mod bones;
pub mod error;
pub mod export;
pub mod expressions;
pub mod glb;
pub mod profile;
pub mod sha256;
pub mod spring;
pub mod validate;

pub use error::{VrmError, VrmResult};
pub use export::{ExportOptions, export_vrm};
pub use expressions::ExpressionMap;
pub use glb::{Census, census, parse_glb};
pub use profile::VrmProfile;
pub use spring::SpringSpec;
pub use validate::{ValidationReport, validate_vrm};

use glb::node_parents;
use spring::NodeIndex;

/// Prepare a spring record against the exact GLB it will be
/// exported with: parse the record, resolve every name against
/// the model's nodes, and check the chain rule. This is the
/// only supported way to build the [`ExportOptions::spring`]
/// input, so a spring spec can never be resolved against one
/// model and exported onto another.
pub fn prepare_spring(text: &str, glb_bytes: &[u8]) -> VrmResult<SpringSpec> {
    let glb = parse_glb(glb_bytes)?;
    let report = census(&glb);
    let parents = node_parents(&glb.json);
    let index = NodeIndex {
        names: &report.node_names,
        parents: &parents,
    };
    SpringSpec::load(text, &index)
}

#[cfg(test)]
mod tests {
    /// The skeleton's one verifiable fact: the workspace member
    /// builds under its contracted name.
    #[test]
    fn workspace_member_builds_under_its_contracted_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-vrm");
    }
}
