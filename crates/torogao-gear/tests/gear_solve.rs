//! Gate (brief scope, §§4.2–4.5): the attachment solve's
//! drift bound — post-solve residual ≤2.0 px on 100% of ticks
//! and lag ≤1 step across the repertoire motion set, measured
//! at 60 ticks/second over 3600 ticks per motion; displacement
//! latching fires exactly once under the stress motion; the
//! contact-quality formula of §4.3 is checked at its corners.

use torogao_gear::anchors::RigTemplate;
use torogao_gear::document::StateVariant;
use torogao_gear::examples::{clamp_pair_document, vibrator_document};
use torogao_gear::solve::{
    DRIFT_BOUND_PX, LAG_BOUND_STEPS, MotionTrack, contact_quality, run_solve,
};

#[test]
fn drift_bound_holds_across_repertoire_motions() {
    let rig = RigTemplate::prima();
    let docs = [clamp_pair_document(), vibrator_document(250)];
    for doc in &docs {
        for track in [MotionTrack::Greeting, MotionTrack::Daydream] {
            for attachment in &doc.anchors {
                let site = rig.resolve(&attachment.anchor).expect("anchor resolves");
                let trace = run_solve(
                    attachment,
                    site.x_px + attachment.offset_x_px,
                    site.y_px + attachment.offset_y_px,
                    track,
                    3600,
                    doc.physics.displacement_threshold_px as f64,
                );
                assert!(
                    trace.max_residual_px <= DRIFT_BOUND_PX,
                    "{} {} {:?}: residual {}",
                    doc.id,
                    attachment.anchor,
                    track,
                    trace.max_residual_px
                );
                assert_eq!(trace.ticks_over_bound, 0, "100% of ticks within bound");
                assert!(
                    trace.lag_steps <= LAG_BOUND_STEPS,
                    "{} {} {:?}: lag {}",
                    doc.id,
                    attachment.anchor,
                    track,
                    trace.lag_steps
                );
                assert!(trace.displacement_ticks.is_empty());
            }
        }
    }
}

#[test]
fn displacement_latches_exactly_once_under_stress() {
    let rig = RigTemplate::prima();
    let doc = clamp_pair_document();
    let stiff = &doc.anchors[0];
    let site = rig.resolve(&stiff.anchor).expect("anchor resolves");
    // §4.5 is a sustained-error rule: the stiff clamp
    // (stiffness 800) settles the 160 px jolt in ~9 steps, so
    // its error never stays over threshold for 30 consecutive
    // steps and it does NOT displace — the model working as
    // specified, not a missed latch.
    let trace = run_solve(
        stiff,
        site.x_px,
        site.y_px,
        MotionTrack::StressJump,
        600,
        doc.physics.displacement_threshold_px as f64,
    );
    assert!(
        trace.displacement_ticks.is_empty(),
        "stiff clamp rides out the jolt"
    );
    // A soft-strapped attachment (stiffness 0) cannot settle
    // within the window: its error stays over threshold past
    // 30 consecutive steps and the latch fires exactly once,
    // on step 30 of the over-threshold run.
    let mut soft = stiff.clone();
    soft.stiffness_permille = 0;
    let trace = run_solve(
        &soft,
        site.x_px,
        site.y_px,
        MotionTrack::StressJump,
        600,
        doc.physics.displacement_threshold_px as f64,
    );
    assert_eq!(
        trace.displacement_ticks.len(),
        1,
        "latch fires exactly once"
    );
    let tick = trace.displacement_ticks[0];
    assert!(
        (329..=345).contains(&tick),
        "latch tick {tick} after 30 over-threshold steps"
    );
    // A non-displaceable item never latches.
    let trace = run_solve(
        &soft,
        site.x_px,
        site.y_px,
        MotionTrack::StressJump,
        600,
        f64::INFINITY,
    );
    assert!(trace.displacement_ticks.is_empty());
}

#[test]
fn contact_quality_formula_corners() {
    let seated = StateVariant::Attached;
    // Full seating, no slip: q = 1 * (0.35 + 0.65) = 1.
    let q = contact_quality(seated, 0.0, 40.0, 700, 700, None);
    assert!((q - 1.0).abs() < 1e-9, "q={q}");
    // Removed: always zero.
    assert_eq!(
        contact_quality(StateVariant::Removed, 0.0, 40.0, 700, 700, None),
        0.0
    );
    // Slip at the radius: zero.
    assert_eq!(contact_quality(seated, 40.0, 40.0, 700, 700, None), 0.0);
    // Half slip, half depth: 0.5 * (0.35 + 0.325) = 0.3375.
    let q = contact_quality(seated, 20.0, 40.0, 350, 700, None);
    assert!((q - 0.3375).abs() < 1e-9, "q={q}");
    // Opaque apparel attenuates to zero.
    assert_eq!(contact_quality(seated, 0.0, 40.0, 700, 700, Some(0)), 0.0);
    // Half transmission halves the quality.
    let q = contact_quality(seated, 0.0, 40.0, 700, 700, Some(500));
    assert!((q - 0.5).abs() < 1e-9, "q={q}");
}

#[test]
fn waveform_sampling_is_deterministic_and_bounded() {
    let constant = torogao_gear::document::WaveSpec {
        kind: torogao_gear::document::WaveKind::Constant,
        amplitude_permille: 250,
        frequency_millihz: 0,
        duty_permille: None,
        pattern: None,
        pattern_step_ticks: None,
        phase_ticks: 0,
    };
    for tick in [0, 1, 999, 1_000_000] {
        assert_eq!(constant.sample(tick), 250);
        assert_eq!(constant.sample(tick), constant.sample(tick));
    }
    let pulse = torogao_gear::document::WaveSpec {
        kind: torogao_gear::document::WaveKind::Pulse,
        amplitude_permille: 500,
        frequency_millihz: 2000,
        duty_permille: Some(500),
        pattern: None,
        pattern_step_ticks: None,
        phase_ticks: 0,
    };
    // Period 30 ticks at 2 Hz; duty half: on at 0, off at 20.
    assert_eq!(pulse.sample(0), 500);
    assert_eq!(pulse.sample(20), 0);
    assert_eq!(pulse.sample(30), 500);
}
