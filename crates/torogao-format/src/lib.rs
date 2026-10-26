//! Torogao's format layer.
//!
//! This crate will own the INP1 container and the vendor-extension
//! records: parsing untrusted files with bounded work, and
//! byte-stable serialization (the same record always serializes to
//! the same bytes, so exports can be compared byte for byte).
//!
//! Port phase: RP1 brings the container reader and writer; RP2
//! completes the record set the engine consumes. Until then this
//! crate is a documented skeleton with no public surface — callers
//! must not grow APIs here ahead of the port phases that own them.

#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    /// The skeleton's one verifiable fact: the workspace member
    /// builds under its contracted name. The implementation that
    /// replaces this wiring proof arrives with the port phase that
    /// owns this crate.
    #[test]
    fn workspace_member_builds_under_its_contracted_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-format");
    }
}
