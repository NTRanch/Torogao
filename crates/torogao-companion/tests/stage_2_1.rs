//! Stage 2.1 gate instruments (PROGRAM.md §2.1, the
//! wave), fixture-gated like the earlier stages: set
//! `TOROGAO_PHASE_F_FIXTURES` and these run; unset, they
//! skip silently.
//!
//! One 3600-step (60 s) body-cycle performance through
//! the companion core on Prima carries all five numbers:
//!
//! - body cycle 2.31 Hz ± 0.05 (peak spacing over the
//!   sustain window of the evaluated primary);
//! - peak-spacing jitter σ within 10–30 ms;
//! - envelope RMSE ≤ 0.10 (peak-hold envelope of the
//!   evaluated primary vs the authored envelope,
//!   amplitude-normalized);
//! - secondary lag ≤ 17 ms by the regional-MAD
//!   cross-correlation instrument — measured on the
//!   evaluated primary/secondary series, and again on
//!   the body/hair regions' MAD series;
//! - near-zero-shift frames ≤ 9.1% (the body region's
//!   MAD series at the no-visible-change floor).

use std::path::PathBuf;

use torogao_companion::cycle::CYCLE_AMPLITUDE_DEG;
use torogao_companion::lag::{
    cross_correlation_lag, near_zero_fraction, peak_hold_envelope, peak_spacing,
    regional_mad_series, rmse,
};
use torogao_companion::perf::run_cycle_trace;

fn prima_fixture() -> Option<PathBuf> {
    let dir = std::env::var_os("TOROGAO_PHASE_F_FIXTURES")?;
    let path = PathBuf::from(dir).join("prima-repertoire.inx");
    path.is_file().then_some(path)
}

const FRAMES: usize = 3600;
/// The sustain window for peak statistics: after the
/// 1 s attack, before the 1 s release.
const SUSTAIN: (usize, usize) = (120, 3480);
/// The no-visible-change floor for one step of the body
/// region, in degrees of ParamBodyAngleZ: a hundredth of
/// a degree per frame moves nothing anyone can see.
const NEAR_ZERO_EPSILON: f64 = 0.01;

#[test]
fn the_wave_gate() {
    let Some(model) = prima_fixture() else {
        return;
    };
    let run = run_cycle_trace(&model, FRAMES).expect("cycle run");

    // Frequency + jitter from the evaluated primary.
    let peaks = peak_spacing(&run.primary, SUSTAIN);
    println!(
        "wave gate: {} peaks, mean period {:.4} s, frequency {:.4} Hz, jitter sigma {:.2} ms",
        peaks.peaks, peaks.mean_period_seconds, peaks.frequency_hz, peaks.jitter_sigma_ms
    );
    assert!(
        (peaks.frequency_hz - 2.31).abs() <= 0.05,
        "body cycle {} Hz outside 2.31 +/- 0.05",
        peaks.frequency_hz
    );
    assert!(
        (10.0..=30.0).contains(&peaks.jitter_sigma_ms),
        "peak-spacing jitter sigma {} ms outside 10-30",
        peaks.jitter_sigma_ms
    );

    // Envelope fidelity: peak-hold envelope of the
    // evaluated primary, amplitude-normalized, against
    // the authored envelope.
    let measured: Vec<f64> = peak_hold_envelope(&run.primary, 26)
        .iter()
        .map(|value| value / f64::from(CYCLE_AMPLITUDE_DEG))
        .collect();
    let envelope_rmse = rmse(&measured, &run.envelope);
    println!("wave gate: envelope RMSE {envelope_rmse:.4} (bound 0.10)");
    assert!(
        envelope_rmse <= 0.10,
        "envelope RMSE {envelope_rmse} exceeds 0.10"
    );

    // Secondary lag, direct series: the evaluated
    // trailing axis against the evaluated primary over
    // the sustain window.
    let primary: Vec<f64> = run.primary[SUSTAIN.0..SUSTAIN.1].to_vec();
    let secondary: Vec<f64> = run.secondary[SUSTAIN.0..SUSTAIN.1].to_vec();
    let lag = cross_correlation_lag(&primary, &secondary, 4);
    println!(
        "wave gate: secondary lag {:.2} steps = {:.2} ms (peak correlation {:.4})",
        lag.lag_steps, lag.lag_ms, lag.peak_correlation
    );
    assert!(
        lag.peak_correlation > 0.9,
        "secondary does not track the primary (correlation {})",
        lag.peak_correlation
    );
    assert!(
        lag.lag_ms <= 17.0 && lag.lag_ms > 8.0,
        "secondary lag {} ms: the authored frame is 16.7, the bound 17",
        lag.lag_ms
    );

    // Secondary lag, regional-MAD form (the instrument
    // the PROGRAM names): the body region's activity
    // series against the secondary region's.
    let body_mad = regional_mad_series(&run.params, &run.body_region);
    let secondary_mad = regional_mad_series(&run.params, &run.secondary_region);
    let mad_lag = cross_correlation_lag(
        &body_mad[SUSTAIN.0..SUSTAIN.1],
        &secondary_mad[SUSTAIN.0..SUSTAIN.1],
        4,
    );
    println!(
        "wave gate: regional-MAD lag {:.2} steps = {:.2} ms (peak correlation {:.4})",
        mad_lag.lag_steps, mad_lag.lag_ms, mad_lag.peak_correlation
    );
    assert!(
        mad_lag.lag_ms <= 17.0 && mad_lag.lag_ms > 0.0,
        "regional-MAD lag {} ms outside (0, 17]",
        mad_lag.lag_ms
    );

    // The rig-data finding, measured with the same
    // instrument: the hair channels are physics outputs
    // whose evaluated motion tracks the body at ~0 ms
    // with a gain of ~1e-4 — there is no visible authored
    // secondary in the rig data. Asserted as a finding
    // (amplitude below one hundredth of the channel's
    // range) so a future rig that raises the physics
    // gain flags itself for re-gating instead of passing
    // silently.
    let physics: Vec<f64> = run.physics_secondary[SUSTAIN.0..SUSTAIN.1].to_vec();
    let physics_lag = cross_correlation_lag(&primary, &physics, 4);
    let physics_peak = physics
        .iter()
        .map(|value| value.abs())
        .fold(0.0_f64, f64::max);
    println!(
        "wave gate: rig physics secondary (ParamHairSway1) lag {:.2} ms, correlation {:.4}, peak amplitude {:.5}",
        physics_lag.lag_ms, physics_lag.peak_correlation, physics_peak
    );
    assert!(
        physics_peak < 0.01,
        "the rig physics gain changed (peak {physics_peak}); re-gate the secondary design"
    );

    // Near-zero-shift frames over the whole performance.
    let fraction = near_zero_fraction(&body_mad, NEAR_ZERO_EPSILON);
    println!(
        "wave gate: near-zero-shift fraction {:.4} (bound 0.091)",
        fraction
    );
    assert!(
        fraction <= 0.091,
        "near-zero-shift fraction {fraction} exceeds 9.1%"
    );
}

#[test]
fn the_cycle_performance_is_deterministic() {
    let Some(model) = prima_fixture() else {
        return;
    };
    let first = run_cycle_trace(&model, 900).expect("first run");
    let second = run_cycle_trace(&model, 900).expect("second run");
    assert_eq!(first.trace_hash, second.trace_hash);
    assert_eq!(first.primary, second.primary);
}
