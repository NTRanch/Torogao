//! The embedded Prima repertoire fixture and the
//! determinism smoke's evaluation (BRIEF-A1 §Scope 5).
//!
//! The fixture bytes are the canonical Phase F corpus file
//! (see `fixtures/README.md` for provenance and hash),
//! embedded so the phone evaluates the *same bytes* the
//! desktop evaluates. The input stream is the estate's
//! play-parity protocol, the one `play_parity.rs` and
//! `repertoire_parity.rs` gate on: fresh evaluation → play
//! Greeting looping → 45 fixed steps → pause → seek to
//! frame 60 → 1 step. The trajectory hash chains the
//! evaluated-state hash after every step (FNV-1a over the
//! per-step hashes, in order), so two runs agree on the
//! chain only if every step agreed — a single divergent
//! tick anywhere in the stream breaks it (STOP-4's
//! instrument).

use torogao_app::player::PlayHost;
use torogao_app::scene::Scene;
use torogao_app::state_hash::evaluated_state_hash;
use torogao_app::{AppError, AppResult};

/// Prima's repertoire model, embedded byte-identically
/// from the canonical Phase F fixture.
pub const PRIMA_REPERTOIRE_BYTES: &[u8] = include_bytes!("../fixtures/prima-repertoire.inx");

/// The fixture's recorded SHA-256 (provenance:
/// `fixtures/README.md`). The trajectory test pins the
/// bytes behaviourally instead of re-hashing at runtime:
/// any byte change moves the trajectory hash.
pub const PRIMA_REPERTOIRE_SHA256: &str =
    "f0c3b8395d5e4ce9f31661705af386049dd6c84b2eefb5125f47e5eb551e656d";

/// The source label saves and logs use for the embedded
/// fixture (it is not a filesystem path; the Settings
/// store refuses saves naming any other model).
pub const EMBEDDED_MODEL_LABEL: &str = "torogao-android://prima-repertoire.inx";

/// The animation the determinism stream plays.
pub const STREAM_ANIMATION: &str = "Greeting";
/// Steps played before the pause (the play-parity
/// protocol's span).
pub const STREAM_PLAY_STEPS: u64 = 45;
/// The frame the protocol seeks to after the pause.
pub const STREAM_SEEK_FRAME: i32 = 60;

/// FNV-1a's offset basis (the state hashes' own constant).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a's prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One step's mix into the trajectory chain.
fn mix_step(chain: &mut u64, step_hash: u64) {
    for byte in step_hash.to_le_bytes() {
        *chain ^= u64::from(byte);
        *chain = chain.wrapping_mul(FNV_PRIME);
    }
}

/// The outcome of one protocol run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrajectoryReport {
    /// Total fixed steps stepped (play steps + the
    /// post-seek step).
    pub steps: u64,
    /// The trajectory chain hash over every step.
    pub trajectory_hash: u64,
    /// The evaluated-state hash after the final step (the
    /// play-parity protocol's own comparison value).
    pub final_hash: u64,
    /// Distinct evaluated states seen across the stream —
    /// a frozen rig collapses this to a handful and the
    /// repertoire gate's own floor is 20.
    pub distinct_states: usize,
}

/// Loads Prima's repertoire from raw INP bytes (the
/// embedded fixture on phone; the canonical file's bytes
/// on desktop — identical input, identical scene).
pub fn load_prima(bytes: &[u8]) -> AppResult<Scene> {
    let mut scene = Scene::load_bytes(bytes, EMBEDDED_MODEL_LABEL)?;
    scene.evaluate_fresh(&[])?;
    Ok(scene)
}

/// Evaluates the Greeting stream over `scene` with a fresh
/// play host, following the play-parity protocol exactly.
pub fn greeting_trajectory(scene: &mut Scene) -> AppResult<TrajectoryReport> {
    let mut play = PlayHost::new();
    play.play(scene, Some(STREAM_ANIMATION), true, None)?;
    let mut chain = FNV_OFFSET;
    let mut distinct = std::collections::BTreeSet::new();
    let mut steps = 0_u64;
    for _ in 0..STREAM_PLAY_STEPS {
        play.step(scene, 1)?;
        steps += 1;
        let hash = evaluated_state_hash(&mut scene.puppet);
        mix_step(&mut chain, hash);
        distinct.insert(hash);
    }
    play.pause();
    play.seek(scene, STREAM_SEEK_FRAME)?;
    play.step(scene, 1)?;
    steps += 1;
    let final_hash = evaluated_state_hash(&mut scene.puppet);
    mix_step(&mut chain, final_hash);
    distinct.insert(final_hash);
    Ok(TrajectoryReport {
        steps,
        trajectory_hash: chain,
        final_hash,
        distinct_states: distinct.len(),
    })
}

/// Runs the whole smoke over raw fixture bytes: load,
/// evaluate, report. A fixture without Greeting is a typed
/// refusal (`play` names the unknown animation), never a
/// panic.
pub fn run_greeting_stream(bytes: &[u8]) -> AppResult<TrajectoryReport> {
    let mut scene = load_prima(bytes)?;
    greeting_trajectory(&mut scene)
}

/// Typed refusal shape for a corrupt fixture: the loader's
/// own error, asserted here so the adversarial case has a
/// named home.
pub fn corrupt_fixture_refuses() -> AppError {
    match run_greeting_stream(b"not an inp container") {
        Ok(_) => AppError::Format("corrupt fixture unexpectedly evaluated".to_string()),
        Err(err) => err,
    }
}

#[cfg(test)]
mod tests {
    use super::{PRIMA_REPERTOIRE_BYTES, run_greeting_stream};

    /// The embedded fixture evaluates the full stream and
    /// the performance actually moves (the repertoire
    /// gate's floor: at least 20 distinct states).
    #[test]
    fn embedded_fixture_streams_and_moves() {
        let report = run_greeting_stream(PRIMA_REPERTOIRE_BYTES).expect("stream evaluates");
        assert_eq!(report.steps, 46);
        assert!(
            report.distinct_states >= 20,
            "only {} distinct states — the performance barely moves",
            report.distinct_states
        );
    }

    /// Garbage bytes are a typed refusal, not a panic.
    #[test]
    fn corrupt_fixture_is_a_typed_refusal() {
        assert!(run_greeting_stream(b"junk").is_err());
        assert!(run_greeting_stream(&[]).is_err());
    }

    /// The stream is deterministic on this machine across
    /// repeated runs (the cross-machine claim is the A-1
    /// gate's; this pins the local half).
    #[test]
    fn stream_repeats_identically() {
        let first = run_greeting_stream(PRIMA_REPERTOIRE_BYTES).expect("first run");
        let second = run_greeting_stream(PRIMA_REPERTOIRE_BYTES).expect("second run");
        assert_eq!(first, second);
    }
}
