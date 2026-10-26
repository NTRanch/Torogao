//! Torogao's puppet engine.
//!
//! This crate will own the puppet model, parameter and deformer
//! evaluation on a fixed-step determinism clock, the physics
//! solver, and the renderer. Evaluation is deterministic: the same
//! model plus the same input track always produces the same
//! curves, byte for byte.
//!
//! Port phases: RP2 brings the model and evaluation; RP3 brings
//! physics and rendering. Until then this crate is a documented
//! skeleton with no public surface — callers must not grow APIs
//! here ahead of the port phases that own them.

#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    /// The skeleton's one verifiable fact: the workspace member
    /// builds under its contracted name. The implementation that
    /// replaces this wiring proof arrives with the port phase that
    /// owns this crate.
    #[test]
    fn workspace_member_builds_under_its_contracted_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-engine");
    }
}
