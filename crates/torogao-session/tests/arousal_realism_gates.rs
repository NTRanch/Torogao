//! D-237 realism gates for the arousal engine
//! (`arousal-engine/2`): the measured gaps of /1 against
//! the estate's Laplace3D/doujin standard, closed one by
//! one. Before/after numbers are the exact-law
//! simulation's (`t4-conflict-check/sim_realism.py`,
//! recorded in REALISM-OPT.md); these gates assert the
//! integer engine reproduces the /2 side of each pair.
//! The §8 fixtures themselves live in `arousal_gates.rs`
//! and pass unchanged.

use std::collections::BTreeMap;

use torogao_session::ArousalEngine;
use torogao_session::arousal::*;
use torogao_session::playmode::PlaytimeDefinition;

const NONE: TickInputs = TickInputs {
    touch: None,
    suction_peak: false,
    spray_rate: None,
    face_contact: false,
};

fn vibrator(amplitude: u32) -> DeviceSpec {
    DeviceSpec {
        item_id: "vibe".to_string(),
        site: "perineum".to_string(),
        wave: Some(WaveSpec::constant(amplitude)),
        contact_quality: 1000,
        arousal_floor: 0,
    }
}

fn engine_with(amplitude: u32) -> ArousalEngine {
    let mut engine = ArousalEngine::new(CharacterProfile::default());
    engine.apply_item(vibrator(amplitude));
    engine
}

/// Steps until the engine emits a culmination; returns
/// the tick it fired on. Panics past `max_ticks`.
fn step_until_fire(engine: &mut ArousalEngine, max_ticks: u64) -> u64 {
    for _ in 0..max_ticks {
        let out = engine.step(&NONE);
        if out
            .events
            .iter()
            .any(|e| matches!(e, ArousalEvent::Culmination { .. }))
        {
            return engine.tick();
        }
    }
    panic!("no culmination within {max_ticks} ticks");
}

// ---------------------------------------------------------------- R-G1

/// Edge dwell + fallback (LEAK2). Sim: dwell in
/// Q ∈ [800, 1000) on the low ramp 11.32 s (/1) →
/// 16.42 s (/2); idle fallback 900 → 800 takes 23.58 s
/// (/1) → 15.62 s (/2).
#[test]
fn r_g1_edge_dwell_and_fallback() {
    // Dwell on the low ramp.
    let mut engine = engine_with(250);
    let mut dwell_ticks = 0u64;
    loop {
        let out = engine.step(&NONE);
        let q = engine.vector().pleasure_charge;
        if (800..1000).contains(&q) {
            dwell_ticks += 1;
        }
        if out
            .events
            .iter()
            .any(|e| matches!(e, ArousalEvent::Culmination { .. }))
        {
            break;
        }
        assert!(engine.tick() < 200 * 60, "low ramp must fire");
    }
    assert!(
        dwell_ticks >= 840,
        "edge dwell {dwell_ticks} ticks (sim /2: 985, /1: 679)"
    );

    // Fallback: build to Q ≥ 900 at full input, cut the
    // device, time the fall below 800.
    let mut engine = engine_with(1000);
    while engine.vector().pleasure_charge < 900 {
        engine.step(&NONE);
    }
    engine.remove_item("vibe");
    let mut fall_ticks = 0u64;
    while engine.vector().pleasure_charge >= 800 {
        engine.step(&NONE);
        fall_ticks += 1;
        assert!(fall_ticks < 3600, "fallback must complete");
    }
    assert!(
        fall_ticks <= 1140,
        "fallback {fall_ticks} ticks (sim /2: 937, /1: 1415)"
    );
}

// ---------------------------------------------------------------- R-G2

/// The jolt (SURGE2): after a 30 s build at 300 permille,
/// stepping to full input must charge at ≥ 1.35× the
/// un-surged law at the same (Q, S) over the first 0.5 s.
/// Sim ratio: 1.549 (/2) vs 1.0 (/1, by construction).
#[test]
fn r_g2_charge_surge_jolt() {
    let mut engine = engine_with(300);
    for _ in 0..30 * 60 {
        engine.step(&NONE);
    }
    let before = engine.vector();
    assert!(before.pleasure_charge < 1000, "the build must not fire");
    engine.set_wave("vibe", WaveSpec::constant(1000));
    let q0 = i64::from(engine.vector().pleasure_charge);
    for _ in 0..30 {
        engine.step(&NONE);
    }
    let q1 = i64::from(engine.vector().pleasure_charge);
    // Mean charge rate over the window, permille/s:
    // (q1 - q0) permille over 0.5 s.
    let measured = (q1 - q0) * 2;
    // The un-surged law at the pre-step state, permille/s:
    // 150 · (S/1000) − Q/τ_eff(Q).
    let law = 150 * i64::from(before.sensitivity) / 1000
        - i64::from(before.pleasure_charge) / leak_tau_s(before.pleasure_charge);
    assert!(law > 0, "law rate positive at the step state");
    assert!(
        measured * 100 >= law * 135,
        "jolt ratio: measured {measured}/s vs law {law}/s"
    );
}

// ---------------------------------------------------------------- R-G3

/// Sensitivity carry (CARRY2): a long build freezes a
/// carry ≥ 100; after rest, re-arm snaps S to ≥ 1000 and
/// the second build fires fast. A cold fast build
/// freezes almost nothing and earns no snap.
#[test]
fn r_g3_sensitivity_carry_rebound() {
    // Long build (low ramp).
    let mut engine = engine_with(250);
    step_until_fire(&mut engine, 200 * 60);
    let carry_at_fire = engine.vector().sensitivity_carry;
    assert!(
        carry_at_fire >= 100,
        "long build freezes a real carry (got {carry_at_fire})"
    );
    // Rest: device off, idle until Q < 150.
    engine.remove_item("vibe");
    let mut guard = 0u64;
    while engine.vector().pleasure_charge >= 150 {
        engine.step(&NONE);
        guard += 1;
        assert!(guard < 400_000, "Q must decay below 150");
    }
    // Restimulate; watch for the re-arm and the snap.
    engine.apply_item(vibrator(500));
    let mut s_at_rearm = None;
    let mut rearm_tick = 0u64;
    for _ in 0..120 * 60 {
        let out = engine.step(&NONE);
        if out
            .events
            .iter()
            .any(|e| matches!(e, ArousalEvent::Rearmed { .. }))
        {
            s_at_rearm = Some(engine.vector().sensitivity);
            rearm_tick = engine.tick();
            break;
        }
    }
    let s_at_rearm = s_at_rearm.expect("re-arm must complete");
    assert!(
        s_at_rearm >= 1000,
        "the rebound snaps S above baseline (got {s_at_rearm})"
    );
    let fire_tick = step_until_fire(&mut engine, 120 * 60);
    assert!(
        fire_tick - rearm_tick <= 1200,
        "second build fires fast ({} ticks; sim 892)",
        fire_tick - rearm_tick
    );

    // Cold control: a fast full-input build sensitizes
    // barely (S ≈ 1019 at firing) — carry < 25, no snap.
    let mut cold = engine_with(1000);
    step_until_fire(&mut cold, 30 * 60);
    assert!(
        cold.vector().sensitivity_carry < 25,
        "cold build freezes a sub-threshold carry"
    );
    cold.remove_item("vibe");
    let mut guard = 0u64;
    while cold.vector().pleasure_charge >= 150 {
        cold.step(&NONE);
        guard += 1;
        assert!(guard < 400_000, "Q must decay below 150");
    }
    cold.apply_item(vibrator(500));
    let mut cold_s = None;
    for _ in 0..120 * 60 {
        let out = cold.step(&NONE);
        if out
            .events
            .iter()
            .any(|e| matches!(e, ArousalEvent::Rearmed { .. }))
        {
            cold_s = Some(cold.vector().sensitivity);
            break;
        }
    }
    let cold_s = cold_s.expect("cold re-arm must complete");
    assert!(
        cold_s < 1000,
        "no snap without a carry (S at re-arm {cold_s})"
    );
}

// ---------------------------------------------------------------- R-G4

/// Expression hysteresis: an arousal trace oscillating
/// ±30 around the 800 threshold flips the raw band 114
/// times in the simulation's probe; the hysteresis
/// state machine must hold it to a handful.
#[test]
fn r_g4_expression_hysteresis() {
    let trace: Vec<u32> = (0..3600)
        .map(|t| (800.0 + 30.0 * (f64::from(t) / 10.0).sin()).round() as u32)
        .collect();
    let raw_changes = trace
        .windows(2)
        .filter(|w| raw_expression_band(w[0], 0) != raw_expression_band(w[1], 0))
        .count();
    assert!(raw_changes >= 50, "the probe trace really oscillates");
    let mut band = raw_expression_band(trace[0], 0);
    let mut held_changes = 0usize;
    for &a in &trace {
        let next = expression_band_next(band, a, 0);
        if next != band {
            held_changes += 1;
            band = next;
        }
    }
    assert!(
        held_changes <= 4,
        "hysteresis holds the band ({held_changes} changes vs {raw_changes} raw)"
    );
}

// ---------------------------------------------------------------- R-G5

/// Reaction density (the level + density law): onset
/// burst at the episode's first tick, the ≈ ⅓ steady
/// state in the building band, the culmination spike,
/// and the afterglow tail.
#[test]
fn r_g5_reaction_density_law() {
    let mut engine = engine_with(500);
    let mut entry_density = None;
    let mut steady: Vec<u32> = Vec::new();
    let mut prev_phase = Phase::Baseline;
    for _ in 0..90 * 60 {
        let out = engine.step(&NONE);
        let v = engine.vector();
        if prev_phase == Phase::Baseline && v.phase == Phase::Stimulated {
            entry_density = Some(out.effects.reaction_density_permille);
        }
        if v.phase == Phase::Stimulated && (550..800).contains(&v.arousal) {
            steady.push(out.effects.reaction_density_permille);
        }
        prev_phase = v.phase;
        if v.phase == Phase::Culminating {
            break;
        }
    }
    assert_eq!(entry_density, Some(900), "onset burst at episode entry");
    assert!(!steady.is_empty(), "the run visits the building band");
    let mean: f64 = steady.iter().map(|&d| f64::from(d)).sum::<f64>() / steady.len() as f64;
    assert!(
        (280.0..=340.0).contains(&mean),
        "steady-state density ≈ ⅓ (mean {mean:.1} over {} ticks)",
        steady.len()
    );

    // Culmination spike + afterglow tail on a full build.
    let mut engine = engine_with(1000);
    let mut culm_density = None;
    let mut afterglow: Vec<u32> = Vec::new();
    for _ in 0..400 * 60 {
        let out = engine.step(&NONE);
        let v = engine.vector();
        for e in &out.events {
            if matches!(e, ArousalEvent::Culmination { .. }) {
                culm_density = Some(out.effects.reaction_density_permille);
            }
        }
        if v.phase == Phase::Afterglow {
            afterglow.push(out.effects.reaction_density_permille);
        }
        if v.phase == Phase::Refractory {
            break;
        }
    }
    // Tier 4 culminating density is 650 + 80·4 = 970 —
    // above the 950 entry-tick floor, so the entry tick
    // reads the tier value (the spike is the tier law
    // itself at this tier).
    assert_eq!(culm_density, Some(970), "the culmination tick spikes");
    assert!(!afterglow.is_empty(), "the run reaches afterglow");
    assert!(
        afterglow.iter().all(|&d| d == 140),
        "afterglow keeps the low return tail"
    );
}

// ---------------------------------------------------------------- R-G6

/// Tremble jitter: during a Tier-4 culmination's
/// envelope plateau the tremble amplitude must vary
/// tick to tick inside ±12.5% of the tier value —
/// humanized, never mechanical, never out of band.
#[test]
fn r_g6_tremble_jitter() {
    let mut engine = engine_with(1000);
    step_until_fire(&mut engine, 30 * 60);
    let mut samples: Vec<u32> = Vec::new();
    for _ in 0..16 * 60 {
        let out = engine.step(&NONE);
        if let Some(p) = &out.effects.performance {
            if p.envelope_permille == 1000 {
                samples.push(out.effects.tremble_milli_px);
            }
        }
    }
    assert!(samples.len() > 300, "the plateau is sampled");
    assert!(
        samples.iter().all(|&s| (4375..=5625).contains(&s)),
        "jitter stays inside ±12.5% of 5000 milli-px"
    );
    let mut distinct = samples.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(
        distinct.len() >= 40,
        "the amplitude actually varies ({} distinct values)",
        distinct.len()
    );
}

// ---------------------------------------------------------------- R-G7

/// The lead channel across a full progression: exactly
/// the escalation sequence — breath → blush → body →
/// voice → eyes → body → breath — with lead changes
/// only at band/phase transitions.
#[test]
fn r_g7_lead_channel_sequence() {
    // 450, not the 250 low ramp: the near band (voice
    // lead) needs A ≥ 800, and the 250 ramp's A target
    // (≈ 683) never reaches it before firing — measured.
    let mut engine = engine_with(450);
    let mut leads: Vec<&'static str> = Vec::new();
    for _ in 0..400 * 60 {
        let out = engine.step(&NONE);
        if leads.last() != Some(&out.effects.lead_channel) {
            leads.push(out.effects.lead_channel);
        }
        if engine.vector().phase == Phase::Refractory {
            break;
        }
    }
    assert_eq!(
        leads,
        ["breath", "blush", "body", "voice", "eyes", "body", "breath"],
        "the lead changes are the escalation"
    );
}

// ---------------------------------------------------------------- R-G8

/// The near band's two registers: pleading while she
/// builds under the ceiling, strained once Q ≥ 900.
#[test]
fn r_g8_near_register_split() {
    let mut engine = engine_with(450);
    let mut saw_pleading = false;
    for _ in 0..120 * 60 {
        let out = engine.step(&NONE);
        if out.effects.expression_key == "near" {
            assert_eq!(out.effects.expression_register, "pleading");
            assert!(engine.vector().pleasure_charge < 900);
            saw_pleading = true;
            break;
        }
    }
    assert!(saw_pleading, "the build reaches the near band");
    engine.set_wave("vibe", WaveSpec::constant(1000));
    let mut saw_strained = false;
    for _ in 0..30 * 60 {
        let out = engine.step(&NONE);
        // The register is the quantity under test: it is
        // resolved from the near band directly, while
        // §6.5's key may legitimately read fluid-contact
        // through this window (saliva landings at A > 600
        // mask the key, measured — they do not mask the
        // register).
        if engine.vector().pleasure_charge >= 900 && out.effects.expression_register == "strained" {
            saw_strained = true;
            break;
        }
        if out
            .events
            .iter()
            .any(|e| matches!(e, ArousalEvent::Culmination { .. }))
        {
            break;
        }
    }
    assert!(saw_strained, "at the ceiling the register strains");
}

// ---------------------------------------------------------------- R-G9

/// The Hold contract at the engine level (PM-G6's
/// shape, re-derived under /2): feeding the engine
/// exactly the governor's equilibrium input at Q ≈ 850
/// holds Q in the edge band for 36,000 ticks with zero
/// culminations. The governor data is the Blossom
/// definition's — the same derivation PM-1 ships.
#[test]
fn r_g9_hold_equilibrium_under_v2() {
    let governor = PlaytimeDefinition::blossom().governor;
    let mut engine = engine_with(1000);
    while engine.vector().pleasure_charge < 850 {
        engine.step(&NONE);
        assert!(engine.tick() < 30 * 60, "the build reaches the edge");
    }
    let mut culminations = 0usize;
    let (mut q_min, mut q_max) = (1000u32, 0u32);
    for t in 0..36_000u64 {
        let v = engine.vector();
        let hold = governor.i_hold_permille(v.pleasure_charge, v.sensitivity);
        engine.set_wave("vibe", WaveSpec::constant(hold));
        let out = engine.step(&NONE);
        culminations += out
            .events
            .iter()
            .filter(|e| matches!(e, ArousalEvent::Culmination { .. }))
            .count();
        if t >= 600 {
            let q = engine.vector().pleasure_charge;
            q_min = q_min.min(q);
            q_max = q_max.max(q);
        }
    }
    assert_eq!(culminations, 0, "the governor holds: no culmination");
    assert!(
        q_min >= 780 && q_max <= 1000,
        "Q held in the edge band [{q_min}, {q_max}]"
    );
}

// ---------------------------------------------------------------- R-G10
//
// D-238 profile distinctness. The cast profiles below are
// PROPOSED tuning under the ratified D-223 directions
// (Prima slowest/highest threshold, Yui fastest/
// overshoots, Alma slowest-start/highest peaks);
// AMENDMENT-T4 §9 item 3 reserves the values' ruling to
// the Owner. The gate's claim is narrower than the
// values: ONE identical stimulus script must produce
// measurably different trajectories per profile, in the
// D-223 directions.

fn weights(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
    pairs.iter().map(|(s, w)| (s.to_string(), *w)).collect()
}

fn prima_profile() -> CharacterProfile {
    CharacterProfile {
        site_weights: weights(&[("clitoris", 850), ("g_spot", 900), ("nipples", 700)]),
        default_site_weight: 800,
        activity_weights: weights(&[
            ("power_exchange", 300),
            ("servitude", 200),
            ("hypnosis", 150),
            ("outfit", 250),
        ]),
        weak_spot: Some(WeakSpot {
            site: "nipples".to_string(),
            threshold_dose: 20_000,
            mode: SwitchMode::Engage,
        }),
        submission_classes: vec!["power_exchange".to_string(), "servitude".to_string()],
        subspace_threshold_permille: 800,
        episode_trend_permille: 80,
        refractory_depth_permille: 1150,
        refractory_ticks: 68 * 60,
        afterglow_ticks: 45 * 60,
        carry_scale_permille: 1200,
        ..CharacterProfile::default()
    }
}

fn yui_profile() -> CharacterProfile {
    CharacterProfile {
        site_weights: weights(&[("clitoris", 1000), ("g_spot", 950), ("nipples", 900)]),
        default_site_weight: 950,
        activity_weights: weights(&[
            ("power_exchange", 500),
            ("hypnosis", 700),
            ("outfit", 600),
            ("servitude", 300),
        ]),
        weak_spot: Some(WeakSpot {
            site: "clitoris".to_string(),
            threshold_dose: 3_000,
            mode: SwitchMode::Engage,
        }),
        submission_classes: vec!["power_exchange".to_string(), "hypnosis".to_string()],
        subspace_threshold_permille: 600,
        episode_trend_permille: 20,
        refractory_depth_permille: 700,
        refractory_ticks: 27 * 60,
        afterglow_ticks: 18 * 60,
        carry_scale_permille: 1300,
        ..CharacterProfile::default()
    }
}

fn alma_profile() -> CharacterProfile {
    CharacterProfile {
        site_weights: weights(&[("clitoris", 800), ("g_spot", 1000), ("nipples", 850)]),
        default_site_weight: 850,
        activity_weights: weights(&[
            ("power_exchange", 900),
            ("servitude", 800),
            ("hypnosis", 500),
            ("outfit", 300),
        ]),
        weak_spot: Some(WeakSpot {
            site: "clitoris".to_string(),
            threshold_dose: 3_500,
            mode: SwitchMode::Blank,
        }),
        submission_classes: vec!["power_exchange".to_string(), "servitude".to_string()],
        subspace_threshold_permille: 450,
        episode_trend_permille: 150,
        refractory_depth_permille: 1000,
        carry_scale_permille: 1400,
        lactation_active: true,
        ..CharacterProfile::default()
    }
}

fn site_vibrator(site: &str, amplitude: u32) -> DeviceSpec {
    DeviceSpec {
        item_id: "vibe".to_string(),
        site: site.to_string(),
        wave: Some(WaveSpec::constant(amplitude)),
        contact_quality: 1000,
        arousal_floor: 0,
    }
}

/// One run of the identical script: vibrator 650 at
/// `clitoris` + the power_exchange activity at 600,
/// two episodes with a full rest between.
struct ScriptOutcome {
    ttc1: u64,
    switch1: SwitchState,
    switch_tick1: Option<u64>,
    subspace1: bool,
    p1: u32,
    arc1: u32,
    lactation_coupled: bool,
    s_at_rearm: u32,
    ttc2: u64,
    p2: u32,
    arc2: u32,
}

fn run_script(profile: CharacterProfile) -> ScriptOutcome {
    let mut engine = ArousalEngine::new(profile);
    engine.apply_item(site_vibrator("clitoris", 650));
    engine.set_activity("power_exchange", 600);
    let start = engine.tick();
    let mut switch_tick1 = None;
    let mut subspace1 = false;
    let mut lactation_coupled = false;
    let (mut p1, mut fire1) = (0, 0);
    for _ in 0..300 * 60 {
        let out = engine.step(&NONE);
        let v = engine.vector();
        if switch_tick1.is_none() && v.switch_state != SwitchState::None {
            switch_tick1 = Some(engine.tick());
        }
        subspace1 |= v.subspace;
        lactation_coupled |= out.effects.lactation_coupled;
        if let Some(&ArousalEvent::Culmination { p_permille, .. }) = out
            .events
            .iter()
            .find(|e| matches!(e, ArousalEvent::Culmination { .. }))
        {
            p1 = p_permille;
            fire1 = engine.tick();
            break;
        }
    }
    assert!(fire1 > 0, "episode 1 culminates");
    let switch1 = engine.vector().switch_state;
    let arc1 = engine.vector().arc_affinity;
    // Rest: everything off until Q < 150 (the phase may
    // still read Refractory — it only leaves Refractory
    // through a stimulated re-arm, which is episode 2's
    // first act).
    engine.remove_item("vibe");
    engine.clear_activities();
    let mut guard = 0u64;
    while engine.vector().pleasure_charge >= 150 {
        engine.step(&NONE);
        guard += 1;
        assert!(guard < 900 * 60, "rest completes");
    }
    // Episode 2: the identical script again.
    engine.apply_item(site_vibrator("clitoris", 650));
    engine.set_activity("power_exchange", 600);
    let mut rearm_tick = None;
    let mut s_at_rearm = 0;
    let (mut p2, mut fire2) = (0, 0);
    for _ in 0..300 * 60 {
        let out = engine.step(&NONE);
        if rearm_tick.is_none()
            && out
                .events
                .iter()
                .any(|e| matches!(e, ArousalEvent::Rearmed { .. }))
        {
            rearm_tick = Some(engine.tick());
            s_at_rearm = engine.vector().sensitivity;
        }
        if let Some(&ArousalEvent::Culmination { p_permille, .. }) = out
            .events
            .iter()
            .find(|e| matches!(e, ArousalEvent::Culmination { .. }))
        {
            p2 = p_permille;
            fire2 = engine.tick();
            break;
        }
    }
    assert!(fire2 > 0, "episode 2 culminates");
    let rearm_tick = rearm_tick.expect("episode 2 re-arms");
    ScriptOutcome {
        ttc1: fire1 - start,
        switch1,
        switch_tick1,
        subspace1,
        p1,
        arc1,
        lactation_coupled,
        s_at_rearm,
        ttc2: fire2 - rearm_tick,
        p2,
        arc2: engine.vector().arc_affinity,
    }
}

#[test]
fn r_g10_profile_distinctness() {
    let prima = run_script(prima_profile());
    let yui = run_script(yui_profile());
    let alma = run_script(alma_profile());
    for (name, o) in [("Prima", &prima), ("Yui", &yui), ("Alma", &alma)] {
        println!(
            "{name}: ttc1 {} ticks, switch {:?} at {:?}, subspace {}, P1 {}, arc1 {}, \
             S@rearm {}, ttc2 {} ticks, P2 {}, arc2 {}, lactation {}",
            o.ttc1,
            o.switch1,
            o.switch_tick1,
            o.subspace1,
            o.p1,
            o.arc1,
            o.s_at_rearm,
            o.ttc2,
            o.p2,
            o.arc2,
            o.lactation_coupled
        );
    }
    // D-223 directions on the identical script.
    assert!(yui.ttc1 < alma.ttc1, "Yui builds fastest");
    assert!(alma.ttc1 < prima.ttc1, "Prima is the slowest build");
    assert!(
        prima.ttc1.abs_diff(alma.ttc1) * 20 > prima.ttc1.max(alma.ttc1),
        "Prima and Alma trajectories differ measurably"
    );
    // Switch behaviour: Yui flips ENGAGE at her weak
    // spot; Alma flips BLANK; Prima's weak spot is never
    // stimulated by this script, so she never flips.
    assert_eq!(yui.switch1, SwitchState::Engage);
    assert_eq!(alma.switch1, SwitchState::Blank);
    assert_eq!(prima.switch1, SwitchState::None);
    // Sub-space: only Alma's submission weights clear
    // her gate under this script.
    assert!(alma.subspace1, "Alma enters sub-space");
    assert!(!prima.subspace1, "Prima does not");
    assert!(!yui.subspace1, "Yui does not");
    // Arc: grows where submission input actually lands
    // (Alma, Yui), not for Prima (180 effective < 250).
    assert!(alma.arc1 > 0 && yui.arc1 > 0, "the arc grows with exposure");
    assert_eq!(prima.arc1, 0, "no qualifying exposure, no arc");
    assert!(
        alma.arc2 > alma.arc1,
        "the arc keeps growing across episodes"
    );
    // Episode-2 deltas differ per character (measured
    // on the first green run; the assertions pin the
    // signatures, not the story I expected): every
    // second build starts from refractory sensitivity,
    // so every ttc2 exceeds its ttc1 — but by
    // character-specific amounts; strength rises across
    // episodes for all three, most for Alma (trend
    // +150, the highest peaks of D-223); re-arm
    // sensitivity is a per-character signature — Yui's
    // shallow refractory (depth 700 → target 455) hits
    // the engine's S floor of 500, Alma sits at the
    // §5.3 value 650, Prima's deep refractory (depth
    // 1150) leaves her at 747.
    assert!(prima.ttc2 > prima.ttc1 && yui.ttc2 > yui.ttc1 && alma.ttc2 > alma.ttc1);
    assert!(
        prima.ttc2 != yui.ttc2 && yui.ttc2 != alma.ttc2 && prima.ttc2 != alma.ttc2,
        "episode-2 builds are pairwise distinct"
    );
    assert!(prima.p2 > prima.p1 && yui.p2 > yui.p1 && alma.p2 > alma.p1);
    assert!(
        alma.p2 > prima.p2 && alma.p2 > yui.p2,
        "Alma's peaks are the highest once she is moving"
    );
    assert!(
        yui.s_at_rearm < alma.s_at_rearm && alma.s_at_rearm < prima.s_at_rearm,
        "re-arm sensitivity orders shallow → deep: {} < {} < {}",
        yui.s_at_rearm,
        alma.s_at_rearm,
        prima.s_at_rearm
    );
    // The culmination-coupled side channel fires only
    // where canon carries it (Alma, D-040's ladder).
    assert!(alma.lactation_coupled, "Alma's side channel couples");
    assert!(!prima.lactation_coupled && !yui.lactation_coupled);
}

// ---------------------------------------------------------------- R-G11

/// The arc term across episodes: Alma's switch flips
/// earlier in episode 2 than episode 1 under the
/// identical script, because the grown arc discounts
/// the dose threshold.
#[test]
fn r_g11_arc_discounts_switch_threshold() {
    let mut engine = ArousalEngine::new(alma_profile());
    let flip_tick = |engine: &mut ArousalEngine| -> u64 {
        let start = engine.tick();
        loop {
            engine.step(&NONE);
            if engine.vector().switch_state != SwitchState::None {
                return engine.tick() - start;
            }
            assert!(engine.tick() - start < 120 * 60, "the switch flips");
        }
    };
    engine.apply_item(site_vibrator("clitoris", 650));
    engine.set_activity("power_exchange", 600);
    let flip1 = flip_tick(&mut engine);
    // Finish the episode, rest, re-arm.
    step_until_fire(&mut engine, 300 * 60);
    engine.remove_item("vibe");
    engine.clear_activities();
    let mut guard = 0u64;
    while engine.vector().pleasure_charge >= 150 {
        engine.step(&NONE);
        guard += 1;
        assert!(guard < 900 * 60, "rest completes");
    }
    engine.apply_item(site_vibrator("clitoris", 650));
    engine.set_activity("power_exchange", 600);
    // Step until re-armed, then time the second flip.
    let mut rearmed = false;
    for _ in 0..120 * 60 {
        let out = engine.step(&NONE);
        if out
            .events
            .iter()
            .any(|e| matches!(e, ArousalEvent::Rearmed { .. }))
        {
            rearmed = true;
            break;
        }
    }
    assert!(rearmed, "episode 2 re-arms");
    let flip2 = flip_tick(&mut engine);
    println!(
        "Alma switch flip: episode 1 at {flip1} ticks, episode 2 at {flip2} ticks \
         (arc after episode 1 discounts the dose threshold)"
    );
    assert!(
        flip2 < flip1,
        "the grown arc flips her switch sooner ({flip2} < {flip1} ticks)"
    );
}

// ---------------------------------------------------------------- R-G12

/// The D-237.1 regression gate (PM-G6's failure class):
/// a Hold equilibrium fed exactly as PM-1 feeds it —
/// the governor's input plus a capped touch ripple
/// every 45 ticks — must hold for 36,000 ticks, and
/// the surge must never fire on ripple. The
/// un-deadbanded surge culminated once in this shape.
#[test]
fn r_g12_surge_deadband_holds_the_governor() {
    // Direct probe: a one-tick full-strength touch is
    // ripple, however large; a sustained step is not.
    // (Warm-up: applying the device at tick 0 is itself
    // a deliberate step and commits a surge — 1500
    // ticks lets it decay to zero before the probe.)
    let mut probe = engine_with(300);
    for _ in 0..1500 {
        probe.step(&NONE);
    }
    assert_eq!(probe.vector().charge_surge, 0, "warm-up surge decayed");
    let touch_tick = TickInputs {
        touch: Some(("perineum".to_string(), 1000)),
        ..NONE.clone()
    };
    probe.step(&touch_tick);
    for _ in 0..30 {
        probe.step(&NONE);
    }
    assert_eq!(
        probe.vector().charge_surge,
        0,
        "a one-tick touch commits no surge"
    );
    probe.set_wave("vibe", WaveSpec::constant(1000));
    for _ in 0..30 {
        probe.step(&NONE);
    }
    assert!(
        probe.vector().charge_surge > 300,
        "a sustained step commits the surge (got {})",
        probe.vector().charge_surge
    );

    // The governor shape, with ripple.
    let governor = PlaytimeDefinition::blossom().governor;
    let mut engine = engine_with(1000);
    while engine.vector().pleasure_charge < 850 {
        engine.step(&NONE);
        assert!(engine.tick() < 30 * 60, "the build reaches the edge");
    }
    let mut culminations = 0usize;
    let (mut q_min, mut q_max) = (1000u32, 0u32);
    // The build's own surge is legitimately present at
    // hold start; the anti-pumping claim is that ripple
    // never raises it again — it only decays, to zero.
    let surge_at_hold_start = engine.vector().charge_surge;
    for t in 0..36_000u64 {
        let v = engine.vector();
        let hold = governor.i_hold_permille(v.pleasure_charge, v.sensitivity);
        engine.set_wave("vibe", WaveSpec::constant(hold));
        let input = if t % 45 == 0 {
            TickInputs {
                touch: Some(("perineum".to_string(), hold)),
                ..NONE.clone()
            }
        } else {
            NONE.clone()
        };
        let out = engine.step(&input);
        culminations += out
            .events
            .iter()
            .filter(|e| matches!(e, ArousalEvent::Culmination { .. }))
            .count();
        assert!(
            engine.vector().charge_surge <= surge_at_hold_start,
            "ripple never re-pumps the surge (tick {t})"
        );
        if t >= 1200 {
            assert_eq!(
                engine.vector().charge_surge,
                0,
                "the build's surge has fully decayed (tick {t})"
            );
        }
        if t >= 600 {
            let q = engine.vector().pleasure_charge;
            q_min = q_min.min(q);
            q_max = q_max.max(q);
        }
    }
    assert_eq!(culminations, 0, "the governor holds under ripple");
    assert!(
        q_min >= 760 && q_max <= 1000,
        "Q held in the edge band [{q_min}, {q_max}]"
    );
}
