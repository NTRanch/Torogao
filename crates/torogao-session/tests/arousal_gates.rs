//! AMENDMENT-T4 §8.2/§8.3/§8.4 gates for the arousal
//! engine (`arousal-engine/1`, D-211 constants), plus
//! §5.6 re-arm, §5.8 stop/reset, §5.9 persistence, and
//! §5.11 determinism. Fixtures run the default profile,
//! full contact (q = 1000), constant waveform, device
//! applied at tick 0 — the §8.3 fixture conditions.

use torogao_session::arousal::*;
use torogao_session::{ArousalEngine, ArousalHost, ArousalOp};

const NONE: TickInputs = TickInputs {
    touch: None,
    suction_peak: false,
    spray_rate: None,
    face_contact: false,
};

fn clamp_pair() -> DeviceSpec {
    DeviceSpec {
        item_id: "clamp-pair".to_string(),
        site: "nipple".to_string(),
        wave: None,
        contact_quality: 1000,
        arousal_floor: 320,
    }
}

fn vibrator(amplitude: u32) -> DeviceSpec {
    DeviceSpec {
        item_id: "vibe".to_string(),
        site: "perineum".to_string(),
        wave: Some(WaveSpec::constant(amplitude)),
        contact_quality: 1000,
        arousal_floor: 0,
    }
}

/// A scripted run's observations.
#[derive(Default)]
struct Observed {
    culminations: Vec<(u64, u8, u32)>, // (tick, tier, p)
    phase_seq: Vec<Phase>,
    max_body_amp: u32,
    max_eye_roll: u32,
    max_posture: u32,
    max_breath: u32,
    max_eye_cross: u32,
    max_secondary: u32,
    max_tremble: u32,
    records: Vec<String>,
}

impl Observed {
    fn absorb(&mut self, engine: &ArousalEngine, out: &EngineOutput) {
        for e in &out.events {
            match e {
                ArousalEvent::Culmination {
                    tier, p_permille, ..
                } => self.culminations.push((engine.tick(), *tier, *p_permille)),
                ArousalEvent::PhaseChanged { to, .. } => self.phase_seq.push(*to),
                _ => {}
            }
        }
        if let Some(p) = &out.effects.performance {
            self.max_body_amp = self.max_body_amp.max(p.body_amplitude);
            self.max_eye_roll = self.max_eye_roll.max(p.eye_roll);
            self.max_posture = self.max_posture.max(p.posture);
            self.max_breath = self.max_breath.max(p.breath_scale);
            self.max_eye_cross = self.max_eye_cross.max(p.eye_cross);
            self.max_secondary = self.max_secondary.max(p.secondary_scale);
            self.max_tremble = self.max_tremble.max(out.effects.tremble_milli_px);
        }
        // Parameter sanity (§8.3): every driven value in range.
        let v = engine.vector();
        assert!(v.arousal <= 1000 && v.pleasure_charge <= 1500);
        assert!((500..=1500).contains(&v.sensitivity));
        assert!(out.effective_input <= 1000);
        assert!(out.effects.blush_permille <= 1000);
        assert!(out.effects.sweat_permille <= 1000);
        self.records.push(engine.canonical_record(&out.events));
    }
}

fn run_engine(engine: &mut ArousalEngine, ticks: u64) -> Observed {
    let mut obs = Observed {
        phase_seq: vec![Phase::Baseline],
        ..Default::default()
    };
    for _ in 0..ticks {
        let out = engine.step(&NONE);
        obs.absorb(engine, &out);
    }
    obs
}

// ------------------------------------------------------------------ §8.2

#[test]
fn f_clamp_fixture() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(clamp_pair());
    let mut band_at: Option<u64> = None;
    // 600 s hold (36 000 ticks): floor band, Q cap, no fire.
    for t in 1..=36_000u64 {
        let out = engine.step(&NONE);
        let v = engine.vector();
        if band_at.is_none() && (300..=340).contains(&v.arousal) {
            band_at = Some(t);
        }
        assert!(
            (300..=340).contains(&v.arousal),
            "tick {t}: A={} outside floor band",
            v.arousal
        );
        assert!(
            v.pleasure_charge <= 250,
            "tick {t}: Q={} over cap",
            v.pleasure_charge
        );
        assert!(
            !out.events
                .iter()
                .any(|e| matches!(e, ArousalEvent::Culmination { .. })),
            "tick {t}: culmination from presence alone"
        );
    }
    assert!(
        band_at.expect("band reached") <= 15 * 60,
        "band within 15 s"
    );
    assert_eq!(engine.culmination_count(), 0);
    // Removal: F = 0 on the removal tick; A <= 50 within 180 s.
    engine.remove_item("clamp-pair");
    assert_eq!(engine.vector().floor, 0, "floor zeroed on removal tick");
    let mut decayed_at = None;
    for t in 1..=180 * 60u64 {
        engine.step(&NONE);
        if engine.vector().arousal <= 50 {
            decayed_at = Some(t);
            break;
        }
    }
    assert!(decayed_at.expect("decays to <= 50") <= 180 * 60);
}

// ------------------------------------------------------------------ §8.3

#[test]
fn f_vibe_low_fixture() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(250));
    // Fire + full phase sequence + 120 s continued stimulation.
    let obs = run_engine(&mut engine, 320 * 60);
    assert_eq!(
        obs.culminations.len(),
        1,
        "exactly once: {:?}",
        obs.culminations
    );
    let (tick, tier, _p) = obs.culminations[0];
    assert!(tick <= 180 * 60, "fires <= 180 s (fired at {} ticks)", tick);
    assert_eq!(tier, 1, "recorded tier 1");
    assert!(
        obs.max_body_amp <= 350,
        "peak body amplitude <= 0.35 (got {})",
        obs.max_body_amp
    );
    assert_eq!(
        obs.phase_seq,
        vec![
            Phase::Baseline,
            Phase::Stimulated,
            Phase::Culminating,
            Phase::Taper,
            Phase::Afterglow,
            Phase::Refractory
        ],
        "exact phase sequence, no loop"
    );
}

#[test]
fn f_vibe_high_fixture() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(1000));
    let obs = run_engine(&mut engine, 320 * 60);
    assert_eq!(
        obs.culminations.len(),
        1,
        "exactly once: {:?}",
        obs.culminations
    );
    let (tick, tier, _p) = obs.culminations[0];
    assert!(tick <= 30 * 60, "fires <= 30 s (fired at {} ticks)", tick);
    assert_eq!(tier, 4, "recorded tier 4");
    assert!(
        obs.max_body_amp >= 900,
        "peak body amplitude >= 0.90 (got {})",
        obs.max_body_amp
    );
    assert_eq!(
        obs.phase_seq,
        vec![
            Phase::Baseline,
            Phase::Stimulated,
            Phase::Culminating,
            Phase::Taper,
            Phase::Afterglow,
            Phase::Refractory
        ]
    );
}

#[test]
fn f_vibe_scaling_ratio() {
    let mut low = ArousalEngine::new(CharacterProfile::default());
    low.apply_item(vibrator(250));
    let low_obs = run_engine(&mut low, 200 * 60);
    let mut high = ArousalEngine::new(CharacterProfile::default());
    high.apply_item(vibrator(1000));
    let high_obs = run_engine(&mut high, 200 * 60);
    let ratio = f64::from(high_obs.max_body_amp) / f64::from(low_obs.max_body_amp);
    assert!(ratio >= 3.0, "amplitude ratio >= 3.0 (got {ratio})");
}

#[test]
fn tier_monotonicity_four_builds() {
    let mut peaks: Vec<(u32, u32, u32, u32, u32, u32)> = Vec::new(); // body, posture, breath, roll, cross, secondary
    for (amp, want_tier) in [(250u32, 1u8), (500, 2), (700, 3), (1000, 4)] {
        let mut engine = ArousalEngine::new(CharacterProfile::default());
        engine.apply_item(vibrator(amp));
        let obs = run_engine(&mut engine, 200 * 60);
        assert_eq!(obs.culminations.len(), 1, "amp {amp}: one culmination");
        assert_eq!(
            obs.culminations[0].1, want_tier,
            "amp {amp}: tier {want_tier}"
        );
        peaks.push((
            obs.max_body_amp,
            obs.max_posture,
            obs.max_breath,
            obs.max_eye_roll,
            obs.max_eye_cross,
            obs.max_secondary,
        ));
    }
    for w in peaks.windows(2) {
        let (a, b) = (w[0], w[1]);
        assert!(
            b.0 > a.0 && b.3 > a.3,
            "body amplitude and eye roll strictly increase: {a:?} -> {b:?}"
        );
        assert!(
            b.1 >= a.1 && b.2 >= a.2 && b.4 >= a.4 && b.5 >= a.5,
            "every other channel non-decreasing: {a:?} -> {b:?}"
        );
    }
}

// ------------------------------------------------------------------ §8.4

fn build_to_q(target: u32) -> (ArousalEngine, u32) {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(1000));
    while engine.vector().pleasure_charge < target {
        engine.step(&NONE);
        assert!(engine.tick() < 60 * 60, "build reaches Q{target}");
    }
    let q = engine.vector().pleasure_charge;
    (engine, q)
}

#[test]
fn dropout_removal_at_q500_and_q900() {
    for target in [500u32, 900] {
        let (mut engine, q0) = build_to_q(target);
        assert!(
            (target..target + 60).contains(&q0),
            "Q at removal ~{target} (got {q0})"
        );
        engine.remove_item("vibe");
        let mut events_after = 0usize;
        for _ in 0..600 {
            let out = engine.step(&NONE);
            events_after += out
                .events
                .iter()
                .filter(|e| matches!(e, ArousalEvent::Culmination { .. }))
                .count();
        }
        assert_eq!(events_after, 0, "no culmination after removal at Q{target}");
        assert_eq!(engine.culmination_count(), 0);
        // Q leaks per the (amended) §5.3 law within +/-5%:
        // Euler form of the implementation law over 10 s.
        let expected = f64::from(q0) * (1.0f64 - 1.0 / 12_000.0).powi(600);
        let got = f64::from(engine.vector().pleasure_charge);
        assert!(
            (got - expected).abs() / expected <= 0.05,
            "Q{target}: leak {got} vs law {expected:.1}"
        );
    }
}

#[test]
fn dropout_zero_drive_gaps() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(1000));
    let mut gap_events = 0usize;
    for _seg in 0..3 {
        for _ in 0..5 * 60 {
            engine.step(&NONE);
        }
        engine.stop_device("vibe");
        for _ in 0..2 * 60 {
            let out = engine.step(&NONE);
            gap_events += out
                .events
                .iter()
                .filter(|e| matches!(e, ArousalEvent::Culmination { .. }))
                .count();
            assert_eq!(
                out.drive
                    .iter()
                    .map(|d| d.strength_permille)
                    .max()
                    .unwrap_or(0),
                0,
                "zero drive during the gap"
            );
        }
        engine.start_device("vibe");
    }
    assert_eq!(gap_events, 0, "no events fire during zero-drive gaps");
}

#[test]
fn dropout_displacement_to_q0() {
    let (mut engine, _q) = build_to_q(400);
    engine.set_contact("vibe", 0, true);
    let q_before = engine.vector().pleasure_charge;
    for _ in 0..30 * 60 {
        let out = engine.step(&NONE);
        assert_eq!(
            out.effective_input, 0,
            "displaced device contributes nothing"
        );
    }
    assert_eq!(engine.culmination_count(), 0);
    assert!(
        engine.vector().pleasure_charge < q_before,
        "Q only leaks while displaced"
    );
}

#[test]
fn removal_mid_performance_remove_all() {
    let mut host = ArousalHost::new();
    host.engine("prima", CharacterProfile::default());
    host.apply_op(
        "prima",
        &ArousalOp::ApplyItem {
            item: vibrator(1000),
        },
    )
    .unwrap();
    // Step to the firing tick.
    loop {
        let out = host.step("prima", &NONE).unwrap();
        if out
            .events
            .iter()
            .any(|e| matches!(e, ArousalEvent::Culmination { .. }))
        {
            break;
        }
        assert!(host.get("prima").unwrap().tick() < 30 * 60);
    }
    assert_eq!(host.get("prima").unwrap().phase(), Phase::Culminating);
    // Remove-all during Culminating: ends the performance
    // in the same act; phase is Baseline on the next tick;
    // the emitted count stays exactly 1.
    host.apply_op("prima", &ArousalOp::RemoveAllAndReset)
        .unwrap();
    assert!(host.get("prima").unwrap().worn_ids().is_empty());
    host.step("prima", &NONE).unwrap();
    let engine = host.get("prima").unwrap();
    assert_eq!(engine.phase(), Phase::Baseline, "Baseline on the next tick");
    assert_eq!(
        engine.culmination_count(),
        1,
        "it fired; it is not un-fired"
    );
    assert!(
        host.log()
            .iter()
            .any(|e| e.op["op"] == "remove_all_and_reset"),
        "the exit is in the op log as a completed act"
    );
}

// ------------------------------------------------------------------ §5.6 re-arm

#[test]
fn rearm_completes_after_refractory_and_q_decay() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(1000));
    while engine.culmination_count() == 0 {
        engine.step(&NONE);
    }
    engine.remove_item("vibe");
    // Idle: refractory expires, then Q must fall below 150
    // under the amended 200 s leak (reachable, measured).
    let mut refractory_expired_at = None;
    let mut q_below_at = None;
    for _ in 0..3600 * 60 {
        engine.step(&NONE);
        if refractory_expired_at.is_none()
            && engine.phase() == Phase::Refractory
            && engine.vector().phase_ticks >= 45 * 60
        {
            refractory_expired_at = Some(engine.tick());
        }
        if engine.vector().pleasure_charge < 150 {
            q_below_at = Some(engine.tick());
            break;
        }
    }
    let expired = refractory_expired_at.expect("refractory expires");
    let below = q_below_at.expect("Q < 150 is reachable under the 200 s leak");
    assert!(
        below - expired <= 300 * 60,
        "re-arm lag after refractory expiry is finite ({} ticks)",
        below - expired
    );
    // Fresh stimulation: the latch clears and a second
    // culmination fires in episode 2.
    engine.apply_item(vibrator(1000));
    let mut rearmed = false;
    for _ in 0..30 * 60 {
        let out = engine.step(&NONE);
        if out
            .events
            .iter()
            .any(|e| matches!(e, ArousalEvent::Rearmed { episode: 2 }))
        {
            rearmed = true;
            break;
        }
    }
    assert!(rearmed, "re-arm event fires on fresh stimulation");
    while engine.culmination_count() < 2 {
        engine.step(&NONE);
        assert!(
            engine.tick() < 3600 * 60 + 3600,
            "second culmination follows re-arm"
        );
    }
    assert_eq!(engine.vector().episode, 2);
}

// ------------------------------------------------------------------ §5.8

#[test]
fn stop_device_zero_from_next_tick() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(1000));
    for _ in 0..2 * 60 {
        engine.step(&NONE);
    }
    let q_before = engine.vector().pleasure_charge;
    let generation_before = engine.mirror_generation();
    engine.stop_device("vibe");
    assert_eq!(
        engine.mirror_generation(),
        generation_before + 1,
        "stop bumps the generation"
    );
    let out = engine.step(&NONE);
    assert!(
        out.drive.iter().all(|d| d.strength_permille == 0),
        "u is zero from the next tick"
    );
    for _ in 0..60 {
        engine.step(&NONE);
    }
    assert!(
        engine.vector().pleasure_charge <= q_before,
        "charge only leaks after stop"
    );
}

#[test]
fn reset_semantics() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(clamp_pair());
    engine.apply_item(vibrator(700));
    for _ in 0..10 * 60 {
        engine.step(&NONE);
    }
    engine.reset();
    let v = engine.vector();
    assert_eq!(v.arousal, 0);
    assert_eq!(v.pleasure_charge, 0);
    assert_eq!(v.dose, 0);
    assert_eq!(v.sensitivity, 1000);
    assert_eq!(v.phase, Phase::Baseline);
    assert!(!v.culmination_latch);
    // Gear is not engine state: the worn set and its floor survive.
    assert_eq!(v.floor, 320);
    engine.step(&NONE);
    assert_eq!(
        engine.vector().arousal,
        320,
        "floor law re-pins A on the next tick"
    );
}

// ------------------------------------------------------------------ §5.9

#[test]
fn persistence_roundtrip_mid_stimulated() {
    let dir = std::env::temp_dir().join(format!("tg-aro-persist-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prima.arousal.json");

    let script = |engine: &mut ArousalEngine, ticks: u64, records: &mut Vec<String>| {
        for _ in 0..ticks {
            let out = engine.step(&NONE);
            records.push(engine.canonical_record(&out.events));
        }
    };
    // Uninterrupted: 5000 + 5000 ticks (mid-Stimulated:
    // amplitude 150 builds Q without firing).
    let mut a = ArousalEngine::new(CharacterProfile::default());
    a.apply_item(clamp_pair());
    a.apply_item(vibrator(150));
    let mut a_records = Vec::new();
    script(&mut a, 5000, &mut a_records);
    assert_eq!(a.phase(), Phase::Stimulated);
    a.save_to(&path).unwrap();
    let a_at_save = a.vector();
    script(&mut a, 5000, &mut a_records);

    // Interrupted: same 5000, save, restore, continue.
    let mut b = ArousalEngine::new(CharacterProfile::default());
    b.apply_item(clamp_pair());
    b.apply_item(vibrator(150));
    let mut b_records = Vec::new();
    script(&mut b, 5000, &mut b_records);
    b.save_to(&path).unwrap();
    let mut restored = ArousalEngine::load_from(&path).unwrap();
    assert!(
        b.internal_eq(&restored),
        "restored state is field-for-field equal"
    );
    assert_eq!(
        restored.vector(),
        a_at_save,
        "restored vector equals the pre-kill vector"
    );
    assert_eq!(
        restored.mirror_generation(),
        0,
        "no mirror state exists to restore"
    );
    script(&mut restored, 5000, &mut b_records);
    assert_eq!(
        a_records, b_records,
        "resumed trajectory equals the uninterrupted run from the restore tick"
    );
    // The persisted record carries no mirror enablement field at all.
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("mirror"),
        "§5.9: mirror state is not persisted"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------------ §5.11

/// The scripted 36 000-tick mixed stream (apply/remove,
/// ramps, dropouts, touch) of T4-G2.2, driven through the
/// host so the op log is exercised too.
fn mixed_stream_records() -> (Vec<String>, u64) {
    let mut host = ArousalHost::new();
    host.engine("prima", CharacterProfile::default());
    let mut records = Vec::new();
    let push = |host: &mut ArousalHost, records: &mut Vec<String>, inputs: &TickInputs| {
        let out = host.step("prima", inputs).unwrap();
        let engine = host.get("prima").unwrap();
        records.push(engine.canonical_record(&out.events));
    };
    host.apply_op("prima", &ArousalOp::ApplyItem { item: clamp_pair() })
        .unwrap();
    host.apply_op(
        "prima",
        &ArousalOp::ApplyItem {
            item: vibrator(700),
        },
    )
    .unwrap();
    for t in 0..36_000u64 {
        match t {
            5_000 => {
                host.apply_op(
                    "prima",
                    &ArousalOp::SetWave {
                        item_id: "vibe".to_string(),
                        wave: WaveSpec::constant(250),
                    },
                )
                .unwrap();
            }
            10_000 => {
                host.apply_op(
                    "prima",
                    &ArousalOp::StopDevice {
                        item_id: "vibe".to_string(),
                    },
                )
                .unwrap();
            }
            11_200 => {
                host.apply_op(
                    "prima",
                    &ArousalOp::StartDevice {
                        item_id: "vibe".to_string(),
                    },
                )
                .unwrap();
            }
            20_000 => {
                host.apply_op(
                    "prima",
                    &ArousalOp::RemoveItem {
                        item_id: "vibe".to_string(),
                    },
                )
                .unwrap();
            }
            25_000 => {
                host.apply_op(
                    "prima",
                    &ArousalOp::ApplyItem {
                        item: vibrator(1000),
                    },
                )
                .unwrap();
            }
            _ => {}
        }
        let inputs = if (1000..1030).contains(&(t % 5000)) {
            TickInputs {
                touch: Some(("perineum".to_string(), 500)),
                ..NONE.clone()
            }
        } else {
            NONE.clone()
        };
        push(&mut host, &mut records, &inputs);
    }
    let bytes: Vec<u8> = records.join("\n").bytes().collect();
    (records, torogao_session::arousal::fnv1a_64(&bytes))
}

#[test]
fn trajectory_determinism_36000_mixed() {
    let (records_a, hash_a) = mixed_stream_records();
    let (records_b, hash_b) = mixed_stream_records();
    assert_eq!(
        records_a, records_b,
        "canonical trajectories byte-identical across runs"
    );
    assert_eq!(hash_a, hash_b);
    // Golden: pinned from the first green run; the primo
    // mirror run must reproduce this exact value (the
    // resonance D.3 pattern — cross-machine equality is
    // the gate, not a hope).
    assert_eq!(hash_a, GOLDEN_TRAJECTORY_HASH, "trajectory hash golden");
}

const GOLDEN_TRAJECTORY_HASH: u64 = 17_404_604_780_309_621_038;

// ------------------------------------------------------------------ §6 effects

#[test]
fn afterglow_settles_at_floor_or_residue() {
    // With clamps: afterglow holds at the floor (320 +/- 50).
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(clamp_pair());
    engine.apply_item(vibrator(1000));
    while engine.culmination_count() == 0 {
        engine.step(&NONE);
    }
    for _ in 0..(16 + 12 + 30) * 60 {
        engine.step(&NONE);
    }
    let a = engine.vector().arousal;
    assert!(
        (270..=370).contains(&a),
        "clamped afterglow settles at the floor (A={a})"
    );
    // Without gear: the 150 residue.
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(1000));
    while engine.culmination_count() == 0 {
        engine.step(&NONE);
    }
    for _ in 0..(16 + 12 + 30) * 60 {
        engine.step(&NONE);
    }
    let a = engine.vector().arousal;
    assert!(
        (100..=200).contains(&a),
        "bare afterglow settles at the residue (A={a})"
    );
}

#[test]
fn effects_bands_and_sweat() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(250));
    let mut keys: Vec<&'static str> = Vec::new();
    let mut peak_expression = None;
    let mut sweat_max = 0;
    for _ in 0..200 * 60 {
        let out = engine.step(&NONE);
        if keys.last() != Some(&out.effects.expression_key) {
            keys.push(out.effects.expression_key);
        }
        if engine.phase() == Phase::Culminating {
            peak_expression = Some(out.effects.expression_key);
        }
        sweat_max = sweat_max.max(out.effects.sweat_permille);
    }
    assert_eq!(keys.first(), Some(&"ambient"));
    assert!(keys.contains(&"aroused"), "bands progress: {keys:?}");
    assert!(keys.contains(&"building"), "bands progress: {keys:?}");
    assert_eq!(
        peak_expression,
        Some("peak"),
        "Culminating selects the peak name"
    );
    assert!(sweat_max > 0, "sweat accumulates under sustained arousal");
}

// ------------------------------------------------------------------ host validation (adversarial)

#[test]
fn host_op_validation() {
    let mut host = ArousalHost::new();
    host.engine("prima", CharacterProfile::default());
    // Floor above the class bound.
    let mut bad = clamp_pair();
    bad.arousal_floor = 601;
    let err = host
        .apply_op("prima", &ArousalOp::ApplyItem { item: bad })
        .unwrap_err();
    assert_eq!(err.code, "TG-ARO-BOUND");
    // Waveform amplitude above bound.
    let mut bad = vibrator(1000);
    bad.wave = Some(WaveSpec::constant(1001));
    let err = host
        .apply_op("prima", &ArousalOp::ApplyItem { item: bad })
        .unwrap_err();
    assert_eq!(err.code, "TG-ARO-BOUND");
    // Pattern beyond the bounded length.
    let mut bad = vibrator(500);
    bad.wave = Some(WaveSpec {
        kind: WaveKind::Pattern,
        amplitude_permille: 500,
        frequency_millihz: 0,
        duty_permille: 1000,
        pattern: vec![500; 65],
        pattern_step_ticks: 10,
        phase_ticks: 0,
    });
    let err = host
        .apply_op("prima", &ArousalOp::ApplyItem { item: bad })
        .unwrap_err();
    assert_eq!(err.code, "TG-ARO-WAVEFORM");
    // Duplicate apply.
    host.apply_op("prima", &ArousalOp::ApplyItem { item: clamp_pair() })
        .unwrap();
    let err = host
        .apply_op("prima", &ArousalOp::ApplyItem { item: clamp_pair() })
        .unwrap_err();
    assert_eq!(err.code, "TG-ARO-CONFLICT");
    // Unknown item operations.
    let err = host
        .apply_op(
            "prima",
            &ArousalOp::RemoveItem {
                item_id: "nope".to_string(),
            },
        )
        .unwrap_err();
    assert_eq!(err.code, "TG-ARO-UNKNOWN-ITEM");
    let err = host
        .apply_op(
            "prima",
            &ArousalOp::SetWave {
                item_id: "nope".to_string(),
                wave: WaveSpec::constant(100),
            },
        )
        .unwrap_err();
    assert_eq!(err.code, "TG-ARO-UNKNOWN-ITEM");
    // Unknown character step.
    let err = host.step("nobody", &NONE).unwrap_err();
    assert_eq!(err.code, "TG-ARO-UNKNOWN-CHARACTER");
    // Refused ops are not logged; admitted ones are.
    assert_eq!(host.log().len(), 1);
}

#[test]
fn wave_sampling_shapes() {
    // Pulse: 10 Hz, duty 500 -> period 6 ticks, 3 on.
    let pulse = WaveSpec {
        kind: WaveKind::Pulse,
        amplitude_permille: 800,
        frequency_millihz: 10_000,
        duty_permille: 500,
        pattern: Vec::new(),
        pattern_step_ticks: 1,
        phase_ticks: 0,
    };
    let samples: Vec<u32> = (0..6).map(|p| pulse.sample(p)).collect();
    assert_eq!(samples, vec![800, 800, 800, 0, 0, 0]);
    // Wave: bounded by amplitude, starts mid-scale.
    let wave = WaveSpec {
        kind: WaveKind::Wave,
        amplitude_permille: 600,
        frequency_millihz: 1000,
        duty_permille: 1000,
        pattern: Vec::new(),
        pattern_step_ticks: 1,
        phase_ticks: 0,
    };
    for p in 0..60 {
        assert!(wave.sample(p) <= 600);
    }
    assert!(
        (250..=350).contains(&wave.sample(0)),
        "wave starts near mid-scale"
    );
    // Pattern: steps hold for pattern_step_ticks.
    let pattern = WaveSpec {
        kind: WaveKind::Pattern,
        amplitude_permille: 1000,
        frequency_millihz: 0,
        duty_permille: 1000,
        pattern: vec![1000, 500],
        pattern_step_ticks: 10,
        phase_ticks: 0,
    };
    assert_eq!(pattern.sample(0), 1000);
    assert_eq!(pattern.sample(9), 1000);
    assert_eq!(pattern.sample(10), 500);
    assert_eq!(pattern.sample(20), 1000);
    // Ramp: rises within the period.
    let ramp = WaveSpec {
        kind: WaveKind::Ramp,
        amplitude_permille: 900,
        frequency_millihz: 1000,
        duty_permille: 1000,
        pattern: Vec::new(),
        pattern_step_ticks: 1,
        phase_ticks: 0,
    };
    assert_eq!(ramp.sample(0), 0);
    assert!(ramp.sample(59) <= 900 && ramp.sample(59) > 800);
    // Zero frequency cyclic kinds are silence, not a stop.
    let mut zero = pulse.clone();
    zero.frequency_millihz = 0;
    assert_eq!(zero.sample(3), 0);
}

#[test]
fn drive_signal_provenance() {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(400));
    let out = engine.step(&NONE);
    assert_eq!(out.drive.len(), 1);
    let d = &out.drive[0];
    assert_eq!(d.device_item_id, "vibe");
    assert_eq!(d.strength_permille, 400, "pre-contact drive strength");
    assert_eq!(d.wave_kind, WaveKind::Constant);
    assert_eq!(d.session_tick, 1);
    // Contact quality scales the engine's side only (§7.2).
    engine.set_contact("vibe", 500, false);
    let out = engine.step(&NONE);
    assert_eq!(
        out.drive[0].strength_permille, 400,
        "drive signal is pre-contact"
    );
    assert_eq!(
        out.devices[0].effective_permille, 200,
        "engine applies contact on its side"
    );
}
