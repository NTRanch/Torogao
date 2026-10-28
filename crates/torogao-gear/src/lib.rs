//! Torogao T4.1: user-defined gear — the `torogao-gear/1`
//! document model, the ordered §2.7 validator with its typed
//! refusals, the deterministic `.tgearpack` format, the
//! attachment/contact solve of §§4.2–4.5, the worn set with
//! its session op log, and the Creator + Playroom gear
//! surfaces (§3) as headless view-models.
//!
//! Scope boundary (AMENDMENT-T4 §8, T4.1 row): this crate is
//! the data-model half of the T4 first build. The arousal
//! engine's integration laws (§5) are T4.2's; this crate owns
//! only the §5.2 state-vector *interface* the surfaces bind
//! to, with a clearly labelled stub behind it
//! ([`engine_iface::StubEngine`]). Ache mirroring (§7) is
//! T4.5's; the one mirror rule that lives here is the refusal:
//! a file may declare mirror capability and nothing more
//! ([`authority`]).

#![forbid(unsafe_code)]

pub mod anchors;
pub mod authority;
pub mod document;
pub mod emit;
pub mod engine_iface;
pub mod examples;
pub mod extract;
pub mod pack;
pub mod payloads;
pub mod playtime_pack;
pub mod refusal;
pub mod solve;
pub mod surfaces;
pub mod validate;
pub mod worn;

pub use anchors::{BODY_MAP_V1, BodyAnchorDef, ContentStamps, RigTemplate};
pub use document::GearDocument;
pub use engine_iface::{ArousalEngineView, ArousalStateVector, StubEngine};
pub use pack::{GearPack, build_pack, install_pack, read_pack};
pub use refusal::GearRefusal;
pub use validate::{ValidationContext, validate_document, validate_pack};
pub use worn::{OpEvent, WornSet};

#[cfg(test)]
mod tests {
    /// The skeleton's one verifiable fact: the workspace
    /// member builds under its contracted name.
    #[test]
    fn workspace_member_builds_under_its_contracted_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-gear");
    }
}
