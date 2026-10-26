//! Torogao's graphical application.
//!
//! This crate will own the editor's two presentations over one
//! functional skeleton: the Persona-style stage (the default user
//! interface) and the traditional toolbar/panel presentation
//! ("Nerd mode", a toggle, default off). Every capability is
//! reachable from both presentations; the presentation is a
//! preference, never a capability tier.
//!
//! Port phases: RP4 brings the panel presentation; RP5 brings the
//! Persona-style stage. Until then this crate is a documented
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
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-app");
    }
}
