//! Quiet hours (PROGRAM.md §3.3), fixture-gated like the
//! trace determinism suite: set `TOROGAO_PHASE_F_FIXTURES`
//! to the directory holding `prima-repertoire.inx` and
//! these run; unset, they skip silently.
//!
//! The law under test: while quiet hours are on, the
//! reaction entry is a silent no-op (she stays in her
//! ambient performance); with them off, the same entry
//! plays the Greeting reaction.

use std::path::PathBuf;

use torogao_companion::core::{AMBIENT_ANIMATION, CLICK_ANIMATION, CompanionCore};

fn prima_fixture() -> Option<PathBuf> {
    let dir = std::env::var_os("TOROGAO_PHASE_F_FIXTURES")?;
    let path = PathBuf::from(dir).join("prima-repertoire.inx");
    path.is_file().then_some(path)
}

fn current_animation(core: &CompanionCore) -> Option<String> {
    core.play_snapshot()
        .expect("play snapshot")
        .animation
        .clone()
}

#[test]
fn quiet_hours_suppress_the_reaction_and_only_the_reaction() {
    let Some(model) = prima_fixture() else {
        return;
    };
    let mut core = CompanionCore::load(&model, None, None, "torogao-companion quiet-hours test")
        .expect("core loads");
    // At rest she is in the ambient performance.
    assert_eq!(current_animation(&core).as_deref(), Some(AMBIENT_ANIMATION));
    assert!(!core.quiet_hours(), "quiet hours default off");

    // Quiet on: the reaction entry fires nothing — no
    // error, no performance change.
    core.set_quiet_hours(true);
    core.play_reaction().expect("suppression is not a refusal");
    core.tick().expect("tick under quiet");
    assert_eq!(
        current_animation(&core).as_deref(),
        Some(AMBIENT_ANIMATION),
        "the reaction must not start under quiet hours"
    );

    // Quiet off: the same entry plays the reaction.
    core.set_quiet_hours(false);
    core.play_reaction().expect("reaction plays");
    assert_eq!(
        current_animation(&core).as_deref(),
        Some(CLICK_ANIMATION),
        "the reaction must start once quiet hours lift"
    );
}
