//! Headless proof harness for the Stage 2.2 culmination
//! plug-in: drives the REAL arousal engine (the landed
//! T4 build) with F-VIBE-class input to Tier-3 and
//! Tier-4 culminations for Prima (the default
//! character profile) and asserts the emitted curve set
//! against the performance package — durations from the
//! engine's §5.7 table, face order and segment timing
//! from `culmination.json`, eye peaks inside the
//! Stage 2.2 bands, a finite non-looping envelope, and
//! remove-all cancelling within one tick.
//!
//! Package location: `TOROGAO_CULMINATION_JSON` when
//! set, else the estate package beside the workspace.
//! When the package is absent the harness skips (the
//! tree's trace-determinism convention): the plug-in's
//! law is covered by the module's own tests either way.
//!
//! Feed pattern: each engine step's events are observed
//! first, then the player steps against the engine's
//! phase after that step. The engine counts a phase's
//! ticks from the step that enters it (the firing step
//! is the performance's first Culminating tick), so
//! this pattern emits exactly the §5.7 tick counts.

use std::path::PathBuf;

use torogao_companion::culmination::{
    CulminationPackage, CulminationPlayer, ExpressionMap, PerformanceTick, PlaybackOutcome,
    Segment, TICKS_PER_SECOND,
};
use torogao_session::{ArousalEngine, CharacterProfile, DeviceSpec, Phase, TickInputs, WaveSpec};

const NONE: TickInputs = TickInputs {
    touch: None,
    suction_peak: false,
    spray_rate: None,
    face_contact: false,
};

fn package_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TOROGAO_CULMINATION_JSON") {
        let path = PathBuf::from(path);
        return path.exists().then_some(path);
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../culmination-performance-20261029/culmination.json");
    path.exists().then_some(path)
}

fn load_package() -> Option<CulminationPackage> {
    let path = package_path()?;
    Some(
        CulminationPackage::load_file(&path)
            .unwrap_or_else(|err| panic!("the shipped package must load: {err}")),
    )
}

/// A constant vibrator at `amplitude` on the perineum
/// site — the F-VIBE-class drive (the arousal gate
/// suite's device construction and fixture conditions:
/// default profile, full contact, applied at tick 0).
fn vibrator(amplitude: u32) -> DeviceSpec {
    DeviceSpec {
        item_id: "vibe".to_string(),
        site: "perineum".to_string(),
        wave: Some(WaveSpec::constant(amplitude)),
        contact_quality: 1000,
        arousal_floor: 0,
    }
}

/// One engine + player run: drive with the device until
/// the performance has played out on the engine's own
/// phases, capturing every emitted tick.
fn run_performance(amplitude: u32, want_tier: u8) -> (Vec<PerformanceTick>, PlaybackOutcome) {
    let package = load_package().expect("package present");
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(amplitude));
    let mut player = CulminationPlayer::with_package(package, ExpressionMap::prima());
    let mut ticks: Vec<PerformanceTick> = Vec::new();
    let mut fired = false;
    // Bounded generously: F-VIBE fires inside 30 s and
    // the longest performance is 28 s.
    for _ in 0..6000 {
        let out = engine.step(&NONE);
        player.observe(&out.events).expect("events feed the player");
        if player.is_playing() {
            fired = true;
            assert_eq!(player.active_tier(), Some(want_tier));
        }
        if fired {
            match player.step(engine.phase()).expect("playback steps") {
                Some(tick) => ticks.push(tick),
                None => break,
            }
        }
    }
    assert!(fired, "the engine culminates under F-VIBE-class input");
    (ticks, player.outcome())
}

/// Segment tick counts in playback order.
fn segment_counts(ticks: &[PerformanceTick]) -> Vec<(Segment, u64)> {
    let mut counts: Vec<(Segment, u64)> = Vec::new();
    for tick in ticks {
        match counts.last_mut() {
            Some((segment, count)) if *segment == tick.face.segment => *count += 1,
            _ => counts.push((tick.face.segment, 1)),
        }
    }
    counts
}

fn assert_performance(
    ticks: &[PerformanceTick],
    tier: u8,
    performance_seconds: u64,
    taper_seconds: u64,
    segment_frames: [u64; 4],
    eye_roll_peak: f64,
) {
    let total_ticks = (performance_seconds + taper_seconds) * TICKS_PER_SECOND;
    assert_eq!(
        ticks.len() as u64,
        total_ticks,
        "tier {tier}: emitted ticks == (performance + taper) x 60"
    );
    // Face path order + segment timing (the JSON's 60
    // fps segment frames).
    assert_eq!(
        segment_counts(ticks),
        vec![
            (Segment::Onset, segment_frames[0]),
            (Segment::PeakHold, segment_frames[1]),
            (Segment::Release, segment_frames[2]),
            (Segment::Taper, segment_frames[3]),
        ],
        "tier {tier}: face path order and segment frames"
    );
    // Face states: F-03 in onset, F-05 at the peak,
    // F-05 -> F-10 across the release, F-10 in taper.
    assert!(ticks.iter().all(|t| t.tier == tier));
    let onset = &ticks[0];
    assert_eq!(onset.face.canon_from, "F-03");
    let peak = ticks
        .iter()
        .find(|t| t.face.segment == Segment::PeakHold)
        .expect("a peak tick");
    assert_eq!(peak.face.canon_from, "F-05");
    assert_eq!(peak.face.rig_from, "F-05");
    let release = ticks
        .iter()
        .find(|t| t.face.segment == Segment::Release)
        .expect("a release tick");
    assert_eq!(release.face.canon_from, "F-05");
    assert_eq!(release.face.canon_to, "F-10");
    let taper_tick = ticks
        .iter()
        .find(|t| t.face.segment == Segment::Taper)
        .expect("a taper tick");
    assert_eq!(taper_tick.face.canon_from, "F-10");
    // Phases: the first performance_seconds x 60 ticks
    // ride Culminating, the rest Taper.
    let perf_ticks = (performance_seconds * TICKS_PER_SECOND) as usize;
    assert!(
        ticks[..perf_ticks]
            .iter()
            .all(|t| t.phase == Phase::Culminating),
        "tier {tier}: performance ticks ride Culminating"
    );
    assert!(
        ticks[perf_ticks..].iter().all(|t| t.phase == Phase::Taper),
        "tier {tier}: taper ticks ride Taper"
    );
    // The envelope: finite, starts at 0, peaks at 1,
    // settles to ~0 at the last tick, and never rises
    // after the plateau (non-looping, progresses once).
    assert!(ticks.iter().all(|t| t.master.is_finite()));
    assert_eq!(ticks[0].master, 0.0);
    let max_master = ticks.iter().map(|t| t.master).fold(0.0_f64, f64::max);
    assert!((max_master - 1.0).abs() < 1e-12, "master peaks at 1");
    // The settle value 0 is reached exactly at the
    // total duration — the phase boundary, one step
    // past the last emitted tick (the module tests
    // assert sample(total) == 0). The last emitted tick
    // sits on the taper decay law at total - 1/60, up
    // to the document's own 4-dp keyframe rounding
    // (the sampler honors the stored keyframes; the
    // observed deviation from the unrounded law is
    // ~1e-5).
    let last = ticks.last().expect("ticks");
    let expected_last = 0.35 * (-(last.t_seconds - performance_seconds as f64) / 12.0).exp();
    assert!(
        (last.master - expected_last).abs() < 1e-4,
        "last tick {} on the decay law ({expected_last})",
        last.master
    );
    let plateau_end = ticks
        .iter()
        .rposition(|t| (t.master - 1.0).abs() < 1e-12)
        .expect("a plateau");
    assert!(
        ticks[plateau_end..]
            .windows(2)
            .all(|pair| pair[1].master <= pair[0].master + 1e-12),
        "the envelope never rises after its plateau"
    );
    // Eye peaks: ParamEyeRoll reaches the tier peak on
    // the plateau; the peak-hold eye targets sit inside
    // the Stage 2.2 bands.
    let roll = |tick: &PerformanceTick| {
        tick.curves
            .iter()
            .find(|c| c.parameter == "ParamEyeRoll")
            .map(|c| c.value)
            .expect("ParamEyeRoll is emitted")
    };
    assert!(
        (roll(peak) - eye_roll_peak).abs() < 1e-9,
        "ParamEyeRoll peaks at the tier peak"
    );
    let target = peak.peak_eye_target.expect("peak-hold ticks carry targets");
    assert!(
        (0.20..=0.35).contains(&target.visible_iris_fraction),
        "visible iris {} inside 0.20..=0.35",
        target.visible_iris_fraction
    );
    assert!(
        (0.63..=0.73).contains(&target.sclera_field),
        "sclera field {} inside 0.63..=0.73",
        target.sclera_field
    );
    // The remaining channels are present on every tick.
    for tick in ticks {
        for name in [
            "ParamBodyAngleY",
            "ParamBreath",
            "ParamBreastY",
            "ParamEyeCross",
            "ParamHairSway1",
        ] {
            assert!(
                tick.curves.iter().any(|c| c.parameter == name),
                "{name} is emitted"
            );
        }
        assert!(tick.curves.iter().all(|c| c.value.is_finite()));
        assert_eq!(tick.body_cycle.period_band_seconds, [0.5, 0.8]);
        assert!(tick.tremble.rate_hz > 0.0);
    }
}

#[test]
fn tier3_performance_matches_the_package() {
    if load_package().is_none() {
        eprintln!("culmination package absent; harness skipped");
        return;
    }
    // F-VIBE at 700 permille drives Tier 3 (the gate
    // suite's calibration): 11 s + 9 s, segments
    // 198/264/198/540 frames, eye-roll peak 0.85.
    let (ticks, outcome) = run_performance(700, 3);
    assert_eq!(outcome, PlaybackOutcome::Finished);
    assert_performance(&ticks, 3, 11, 9, [198, 264, 198, 540], 0.85);
}

#[test]
fn tier4_performance_matches_the_package() {
    if load_package().is_none() {
        eprintln!("culmination package absent; harness skipped");
        return;
    }
    // F-VIBE high (1000 permille) drives Tier 4:
    // 16 s + 12 s, segments 288/384/288/720 frames,
    // eye-roll peak 1.00.
    let (ticks, outcome) = run_performance(1000, 4);
    assert_eq!(outcome, PlaybackOutcome::Finished);
    assert_performance(&ticks, 4, 16, 12, [288, 384, 288, 720], 1.0);
}

/// Drives a fresh engine + player to a playing Tier-4
/// performance and plays `ticks` ticks into it.
fn playing_tier4(ticks: u64) -> (ArousalEngine, CulminationPlayer) {
    let package = load_package().expect("package present");
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(1000));
    let mut player = CulminationPlayer::with_package(package, ExpressionMap::prima());
    let mut played = 0u64;
    for _ in 0..3600 {
        let out = engine.step(&NONE);
        player.observe(&out.events).expect("starts");
        if player.is_playing() {
            let tick = player
                .step(engine.phase())
                .expect("plays")
                .expect("a tick while playing");
            assert_eq!(tick.phase, Phase::Culminating);
            played += 1;
            if played >= ticks {
                break;
            }
        }
    }
    assert!(player.is_playing(), "the performance is playing");
    (engine, player)
}

#[test]
fn remove_all_mid_performance_cancels_within_one_tick() {
    if load_package().is_none() {
        eprintln!("culmination package absent; harness skipped");
        return;
    }
    // Direct seam: the §3.4 remove-all broadcast.
    let (mut engine, mut player) = playing_tier4(120);
    engine.remove_all_and_reset();
    player.notify_remove_all();
    assert!(!player.is_playing(), "cancelled in the same tick");
    assert_eq!(player.outcome(), PlaybackOutcome::Cancelled);
    assert!(
        player.step(engine.phase()).expect("no tick").is_none(),
        "no curve is emitted after remove-all"
    );
    // Phase keying alone (the shell never calls the
    // seam): the §5.8 reset lands in Baseline and the
    // next step cancels.
    let (mut engine, mut player) = playing_tier4(120);
    engine.remove_all_and_reset();
    assert!(
        player.step(engine.phase()).expect("no tick").is_none(),
        "the phase keying cancels without the seam call"
    );
    assert_eq!(player.outcome(), PlaybackOutcome::Cancelled);
}
