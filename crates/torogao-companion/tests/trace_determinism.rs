//! Stage 1.1 gate instruments, fixture-gated like the
//! app crate's repertoire parity suite: set
//! `TOROGAO_PHASE_F_FIXTURES` to the directory holding
//! `prima-repertoire.inx` and these run; unset, they skip
//! silently (the sandbox has no fixtures; primo does).
//!
//! - Gate G6: the presented Daydream performance is
//!   deterministic — three 600-step traces through the
//!   companion core chain to the same hash.
//! - Gate G5 (motion side): she actually moves (the
//!   moving-step fraction is high) and the loop seam is a
//!   typical step (seam ratio within the bound).

use std::path::PathBuf;

use torogao_companion::perf::{run_trace, seam_report};

fn prima_fixture() -> Option<PathBuf> {
    let dir = std::env::var_os("TOROGAO_PHASE_F_FIXTURES")?;
    let path = PathBuf::from(dir).join("prima-repertoire.inx");
    path.is_file().then_some(path)
}

#[test]
fn daydream_trace_is_deterministic_over_600_frames() {
    let Some(model) = prima_fixture() else {
        return;
    };
    let runs: Vec<_> = (0..3)
        .map(|_| run_trace(&model, 600).expect("trace run"))
        .collect();
    assert_eq!(runs[0].trace_hash, runs[1].trace_hash);
    assert_eq!(runs[1].trace_hash, runs[2].trace_hash);
    assert_eq!(runs[0].step_hashes.len(), 600);
}

#[test]
fn daydream_moves_and_its_seam_is_a_typical_step() {
    let Some(model) = prima_fixture() else {
        return;
    };
    let run = run_trace(&model, 600).expect("trace run");
    // Motion on (nearly) every frame: the ambient loop is
    // a performance, not a statue — G5's moving fraction.
    let moving_fraction = run.moving_steps as f64 / (run.step_hashes.len() - 1) as f64;
    assert!(
        moving_fraction > 0.9,
        "moving fraction {moving_fraction} is too low for the ambient performance"
    );
    // The loop wraps inside 600 steps (Daydream is 180
    // steps), so seams exist to measure.
    assert!(
        !run.seam_steps.is_empty(),
        "no loop wrap in 600 steps of a 180-step loop"
    );
    let seam = seam_report(&run);
    assert!(
        seam.seam_ratio_max <= 1.1,
        "seam ratio {} exceeds the 1.1 bound (median delta {}, seam deltas {:?})",
        seam.seam_ratio_max,
        seam.median_delta,
        seam.seam_deltas
    );
}
