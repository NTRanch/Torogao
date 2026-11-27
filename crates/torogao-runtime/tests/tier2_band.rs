//! Tier 2 band gates (runtime plan Rung R2): the digest is a
//! within-architecture identity (same view, same digest,
//! bit-for-bit), and the band instrument measures numeric
//! distance — a perturbed gauge is measured, not merely
//! flagged, and a perturbation beyond the recorded band fails
//! the band check while a last-bit wiggle passes it.

use torogao_runtime::present::{GaugeView, MeterView, PresentationState};
use torogao_runtime::{RECORDED_BAND, compare_presentations, presentation_digest};

fn view(fraction: f64) -> PresentationState {
    PresentationState {
        tick: 3,
        headline: "room".to_string(),
        detail: None,
        meters: vec![MeterView {
            id: "day".to_string(),
            value: 2,
        }],
        gauges: vec![GaugeView {
            id: "day_progress".to_string(),
            fraction,
        }],
    }
}

#[test]
fn digest_is_bit_exact_for_identical_views() {
    assert_eq!(
        presentation_digest(&view(0.375)),
        presentation_digest(&view(0.375))
    );
    assert_ne!(
        presentation_digest(&view(0.375)),
        presentation_digest(&view(0.5)),
        "a different gauge must digest differently"
    );
}

#[test]
fn identical_runs_measure_zero_distance() {
    let run = vec![view(0.125), view(0.375), view(0.625)];
    let report = compare_presentations(&run, &run);
    assert_eq!(report.gauges_compared, 3);
    assert_eq!(report.digest_mismatches, 0);
    assert_eq!(report.max_abs_diff, 0.0);
    assert_eq!(report.max_rel_diff, 0.0);
    assert!(report.within(RECORDED_BAND));
}

#[test]
fn band_measures_a_perturbed_gauge() {
    let reference = vec![view(0.5)];
    // A last-bit-scale wiggle: measured, inside the band.
    let wiggle = vec![view(0.5 + 1e-9)];
    let report = compare_presentations(&reference, &wiggle);
    assert_eq!(report.digest_mismatches, 1, "the digest sees the wiggle");
    assert!(report.max_abs_diff > 0.0);
    assert!(
        report.within(RECORDED_BAND),
        "a 1e-9 wiggle is what the band exists to absorb"
    );
    // A real perturbation: measured at its true size, outside
    // the band — this is the twin gate's negative-control
    // shape, at unit scale.
    let perturbed = vec![view(0.5 + 1.25e-3)];
    let report = compare_presentations(&reference, &perturbed);
    assert_eq!(report.digest_mismatches, 1);
    assert!((report.max_abs_diff - 1.25e-3).abs() < 1e-12);
    assert!(
        !report.within(RECORDED_BAND),
        "a 1.25e-3 perturbation must fail the recorded band"
    );
}
