//! Stage 2.3 gate instruments (PROGRAM.md §2.3, the
//! duo), fixture-gated like the earlier stages.
//!
//! Prima and Yui share one clock through the authored
//! turn-taking script (see perf::run_duo_trace). The
//! gates:
//!
//! - occlusion census over the full take: 0 draw-order
//!   pops, 0 clipping events;
//! - partner reaction onset within ±1 frame (16.7 ms)
//!   of the authored offset, both directions;
//! - partner gaze settled within 2° of the partner's
//!   face during the other's deep hold;
//! - the Stage 2.2 per-face reads for BOTH faces
//!   simultaneously: the performer's post-reframe hold
//!   reads (visible iris ~0.01 in the plate frame, the
//!   2.2 state carried from the D-511 eyelid re-frame)
//!   while the watcher's face reads at rest — the
//!   absolute iris/sclera bands stay OWNER-PENDING on
//!   the FR-08 renderer-read route;
//! - determinism: the full take hash-equal across
//!   three runs.

use std::path::PathBuf;

use torogao_companion::core::VIEW_DISTANCE_HEIGHTS;
use torogao_companion::perf::{DUO_A_REACT, DUO_B_REACT, run_duo_trace};

fn fixture_file(file: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("TOROGAO_PHASE_F_FIXTURES")?;
    let path = PathBuf::from(dir).join(file);
    path.is_file().then_some(path)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if values.is_empty() {
        return 0.0;
    }
    values[values.len() / 2]
}

/// The yaw (degrees) aiming `own` at `target` through
/// the core's view geometry.
fn target_yaw(own: (f64, f64), target: (f64, f64), canvas_height: f64) -> f64 {
    ((target.0 - own.0) / (VIEW_DISTANCE_HEIGHTS * canvas_height))
        .atan()
        .to_degrees()
}

/// The reaction onset: the first step at or after the
/// authored trigger whose yaw has moved at least 0.30°
/// toward the target from its value at the trigger.
/// The gaze driver's saccades ease with a smoothstep
/// (gaze.rs), so a fractional-travel crossing lags the
/// onset by the easing (a 25% crossing reads ~6 steps
/// late on a 23° saccade); the onset is the motion's
/// start — the planning tick itself moves nothing, so
/// a healthy reaction reads exactly trigger + 1.
fn reaction_onset(yaw: &[f64], trigger: usize, target: f64) -> Option<usize> {
    let baseline = yaw[trigger];
    (trigger..yaw.len()).find(|&step| {
        let moved = yaw[step] - baseline;
        if target >= baseline {
            moved >= 0.30
        } else {
            moved <= -0.30
        }
    })
}

#[test]
fn the_duo_gate() {
    let (Some(model_a), Some(model_b)) = (
        fixture_file("prima-repertoire.inx"),
        fixture_file("yui-repertoire.inx"),
    ) else {
        return;
    };
    let run = run_duo_trace(&model_a, &model_b).expect("duo run");
    println!(
        "duo gate: layout offset {:.1}, divider {:.1}, faces A {:?} B {:?}",
        run.layout.offset_b, run.layout.divider, run.layout.face_a, run.layout.face_b
    );

    // ---- Occlusion census over the full take.
    println!(
        "duo gate: census pops {}, clipping {} over {} steps",
        run.pops,
        run.clipping,
        run.step_hashes.len()
    );
    assert_eq!(run.pops, 0, "draw-order pops across the take");
    assert_eq!(run.clipping, 0, "clipping events across the take");

    // ---- Both performances actually ran.
    let peak_a = run.roll_a.iter().fold(0.0_f64, |a, b| a.max(*b));
    let peak_b = run.roll_b.iter().fold(0.0_f64, |a, b| a.max(*b));
    println!("duo gate: roll peaks A {peak_a:.3} B {peak_b:.3}");
    assert!(peak_a >= 0.99 && peak_b >= 0.99);

    // ---- Reaction onsets, both directions.
    let target_b = target_yaw(run.layout.face_b, run.layout.face_a, run.canvas_height_b);
    let onset_b = reaction_onset(&run.yaw_b, DUO_B_REACT, target_b).expect("B reacts");
    println!(
        "duo gate: B reaction onset step {onset_b} (authored {DUO_B_REACT}), target yaw {target_b:.2} deg"
    );
    assert!(
        onset_b.abs_diff(DUO_B_REACT) <= 1,
        "B onset {onset_b} vs authored {DUO_B_REACT}"
    );
    let target_a = target_yaw(run.layout.face_a, run.layout.face_b, run.canvas_height_a);
    let onset_a = reaction_onset(&run.yaw_a, DUO_A_REACT, target_a).expect("A reacts");
    println!(
        "duo gate: A reaction onset step {onset_a} (authored {DUO_A_REACT}), target yaw {target_a:.2} deg"
    );
    assert!(
        onset_a.abs_diff(DUO_A_REACT) <= 1,
        "A onset {onset_a} vs authored {DUO_A_REACT}"
    );

    // ---- Settled partner gaze during each deep hold,
    // and both faces' reads in the same frames.
    for (label, roll, yaw, target, performer_is_a) in [
        (
            "A performs, B watches",
            &run.roll_a,
            &run.yaw_b,
            target_b,
            true,
        ),
        (
            "B performs, A watches",
            &run.roll_b,
            &run.yaw_a,
            target_a,
            false,
        ),
    ] {
        let hold_steps: Vec<usize> = (0..roll.len()).filter(|&step| roll[step] >= 0.97).collect();
        assert!(!hold_steps.is_empty(), "{label}: no deep-hold steps");
        let mut yaw_hold: Vec<f64> = hold_steps.iter().map(|&step| yaw[step]).collect();
        let settled = median(&mut yaw_hold);
        println!(
            "duo gate: {label}: watcher yaw settled {settled:.2} deg vs target {target:.2} deg"
        );
        assert!(
            (settled - target).abs() <= 2.0,
            "{label}: watcher gaze off by {} deg",
            (settled - target).abs()
        );
        let mut performer_iris: Vec<f64> = Vec::new();
        let mut performer_sclera: Vec<f64> = Vec::new();
        let mut watcher_iris: Vec<f64> = Vec::new();
        let mut watcher_aperture: Vec<f64> = Vec::new();
        for (step, read_a, read_b) in &run.face_reads {
            if roll[*step] < 0.97 {
                continue;
            }
            let (performer, watcher) = if performer_is_a {
                (read_a, read_b)
            } else {
                (read_b, read_a)
            };
            performer_iris.push(performer.left.iris_fraction);
            performer_sclera.push(performer.left.sclera_field);
            watcher_iris.push(watcher.left.iris_fraction);
            watcher_aperture.push(watcher.left.aperture);
        }
        assert!(!performer_iris.is_empty(), "{label}: no hold face reads");
        // Interquartile spread: the watcher's own blink
        // dips land in the read grid and are not
        // perturbation, so the steady-face measure is
        // the p25–p75 band of her reads.
        let watcher_iris_spread = {
            let mut sorted = watcher_iris.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let p = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
            p(0.75) - p(0.25)
        };
        let performer_iris_median = median(&mut performer_iris);
        let performer_sclera_median = median(&mut performer_sclera);
        let watcher_iris_median = median(&mut watcher_iris);
        let watcher_aperture_median = median(&mut watcher_aperture);
        println!(
            "duo gate: {label}: performer iris {performer_iris_median:.3} sclera {performer_sclera_median:.3} (post-reframe plate-frame read); watcher iris {watcher_iris_median:.3} aperture {watcher_aperture_median:.3}"
        );
        assert!(
            performer_iris_median < 0.02,
            "{label}: performer visible iris {performer_iris_median} (post-reframe read)"
        );
        assert!(
            performer_sclera_median > 0.95,
            "{label}: performer sclera {performer_sclera_median} (post-reframe read)"
        );
        // The watcher's face, read in the same frames:
        // she is turned ~23° toward the performer (the
        // AngleX keyforms narrow the visible iris and
        // open the lid line relative to her forward
        // face), so her bands are the turned-face bands,
        // re-measured under the D-511 re-frame (iris
        // 0.57 / 0.68; aperture 1.00 / 0.89 — the plate
        // read runs high on the turned face and Yui's
        // saturates at the 1.0 clamp, so aperture is
        // asserted as a floor; her forward-face rest
        // aperture is the on-table 0.85 of the 2.2 pure
        // reads): iris well above the performer's
        // rolled-away ~0.01, aperture open, and — the
        // simultaneity content — her reads steady
        // across the other's hold (interquartile spread,
        // blink dips excepted), unperturbed by the
        // performance.
        assert!(
            (0.45..=0.80).contains(&watcher_iris_median),
            "{label}: watcher iris {watcher_iris_median} is not a turned watching face"
        );
        assert!(
            watcher_aperture_median >= 0.85,
            "{label}: watcher aperture {watcher_aperture_median} is not a turned watching face"
        );
        assert!(
            watcher_iris_spread <= 0.10,
            "{label}: watcher iris spread {watcher_iris_spread} across the hold — her face is perturbed"
        );
    }
}

#[test]
fn the_duo_take_is_deterministic_across_three_runs() {
    let (Some(model_a), Some(model_b)) = (
        fixture_file("prima-repertoire.inx"),
        fixture_file("yui-repertoire.inx"),
    ) else {
        return;
    };
    let first = run_duo_trace(&model_a, &model_b).expect("first run");
    let second = run_duo_trace(&model_a, &model_b).expect("second run");
    let third = run_duo_trace(&model_a, &model_b).expect("third run");
    assert_eq!(first.step_hashes, second.step_hashes);
    assert_eq!(second.step_hashes, third.step_hashes);
    assert_eq!(first.trace_hash, third.trace_hash);
}
