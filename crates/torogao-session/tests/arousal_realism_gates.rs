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
// D-238 profile distinctness, re-mapped onto the
// D-273 locked kink sets (Owner ruling, 2026-10-28):
// each profile's trigger map names the classes and
// sites of her locked set, under the D-223 directions
// the D-244 ratification measured (Prima slowest/
// highest threshold, Yui fastest/overshoots, Alma
// slowest-start/highest peaks). The gate's claim is
// narrower than the values: ONE identical stimulus
// script must produce measurably different
// trajectories per profile, in the D-223 directions.
// R-G13 below extends the same claim to all six.

fn weights(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
    pairs.iter().map(|(s, w)| (s.to_string(), *w)).collect()
}

fn prima_profile() -> CharacterProfile {
    // D-273 locked set: corruption · erotic hypnosis ·
    // TPE · overstimulation · multi penetration ·
    // stockades/spreader bars. Hypnotic suggestion is
    // her dominant activity and, with power exchange
    // (the TPE container), feeds her arc — corruption
    // is the arc term itself. The restraint fixture
    // and multi-penetration classes carry the gear and
    // contact halves; the pelvis site carries the
    // contact weight. Overstimulation lives in the
    // pattern fields: the trio's highest carry and a
    // refractory depth of 1150, whose sensitivity
    // target (747) is the shallowest dip of the three
    // — she endures past culmination. Canon: she has
    // NO weak spot — no single dominant zone.
    CharacterProfile {
        site_weights: weights(&[
            ("clitoris", 850),
            ("g_spot", 900),
            ("nipples", 700),
            ("pelvis", 950),
        ]),
        default_site_weight: 800,
        activity_weights: weights(&[
            ("hypnotic_suggestion", 950),
            ("power_exchange", 400),
            ("restraint_fixture", 550),
            ("multi_penetration", 500),
        ]),
        submission_classes: vec![
            "hypnotic_suggestion".to_string(),
            "power_exchange".to_string(),
        ],
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
    // D-273 locked set: shibari · suspension · facials ·
    // edging/orgasm denial · begging · biting. Rope
    // bondage and suspension are her dominant classes;
    // begging is her vocal class; biting lands as
    // contact at the neck. Facials ride the §6.4 seam:
    // the face site at full weight and a raised fluid-
    // contact input (the default profile's is zero).
    // Edging is her near-threshold shape — the fastest
    // build of the trio, the shallowest refractory, the
    // ratified ENGAGE dose: she reaches the edge first
    // and can be held there. The ENGAGE bonus (+350)
    // and arc growth are the D-244 ratified constants.
    CharacterProfile {
        site_weights: weights(&[
            ("clitoris", 1000),
            ("g_spot", 950),
            ("nipples", 900),
            ("neck", 850),
            ("face", 1000),
        ]),
        default_site_weight: 950,
        activity_weights: weights(&[
            ("rope_bondage", 950),
            ("suspension", 900),
            ("begging", 650),
            ("power_exchange", 500),
        ]),
        weak_spot: Some(WeakSpot {
            site: "clitoris".to_string(),
            threshold_dose: 3_000,
            mode: SwitchMode::Engage,
        }),
        submission_classes: vec!["power_exchange".to_string(), "rope_bondage".to_string()],
        subspace_threshold_permille: 600,
        episode_trend_permille: 20,
        refractory_depth_permille: 700,
        refractory_ticks: 27 * 60,
        afterglow_ticks: 18 * 60,
        carry_scale_permille: 1300,
        fluid_contact_input_permille: 650,
        ..CharacterProfile::default()
    }
}

fn alma_profile() -> CharacterProfile {
    // D-273 locked set: hucow/lactation · maid/domestic
    // service (incl. paizuri) · breeding · breast
    // growth · nipple play · medical play. Lactation
    // is active and the breast/nipple sites are
    // maximal — the paizuri contact lives at the
    // breast site. Domestic servitude is the maid
    // service; breeding is a submission class, so it
    // feeds her arc directly: the fertility state and
    // breast growth are arc-expressed body states, and
    // the arc's data shape is the profile's highest
    // episode trend (+150) and carry (1400) — her
    // peaks grow across episodes. Medical play is the
    // clinical-examination class. Her BLANK weak spot
    // stays the trio's only sub-space entry.
    CharacterProfile {
        site_weights: weights(&[
            ("clitoris", 800),
            ("g_spot", 1000),
            ("nipples", 1000),
            ("breast", 950),
        ]),
        default_site_weight: 850,
        activity_weights: weights(&[
            ("domestic_servitude", 950),
            ("power_exchange", 900),
            ("breeding", 800),
            ("medical_play", 700),
        ]),
        weak_spot: Some(WeakSpot {
            site: "clitoris".to_string(),
            // 3_500 in the first D-238 tuning; the
            // D-273 union script (R-G13) is a denser
            // stimulus than the R-G10 script and fired
            // her at tick 387, before a 3_500 dose could
            // accrue (~427) — the switch was unreachable
            // under it. 2_800 keeps the flip ahead of
            // culmination under both scripts.
            threshold_dose: 2_800,
            mode: SwitchMode::Blank,
        }),
        submission_classes: vec![
            "power_exchange".to_string(),
            "domestic_servitude".to_string(),
            "breeding".to_string(),
        ],
        subspace_threshold_permille: 250,
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
    // (Alma, Yui), not for Prima (240 effective < 250
    // under this script — her submission channel here
    // is power exchange alone; her dominant channel,
    // hypnotic suggestion, is not in this script).
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

// ---------------------------------------------------------------- R-G13
//
// D-273: the troupe profiles (Sloane, Lux, Piper) join
// the cast, and the D-238 distinctness claim extends
// to all six: ONE identical stimulus script — the
// union script below, every cast member's classes and
// sites present at once — must produce six measurably
// different trajectories. Each profile answers only
// through its own trigger map (the default activity
// weight is zero), so the union script is a stress
// test of the maps, not a portrait of any one scene.

fn sloane_profile() -> CharacterProfile {
    // D-273 locked set: tantric/yoni stimulation · sex
    // toys (machines, gags, exotic) · collared/leashed ·
    // slavery/slave training · appraisal · chastity
    // device. D-267 dynamic: the trigger map is led by
    // ownership signals — slave training dominant, the
    // collar/leash class beside it, the machine's
    // steady input, tantric as the slow long-build
    // class. Her weak spot is being appraised: the
    // format's dose channel is site-only, so appraisal
    // is expressed through the collar site (the neck —
    // where appraisal happens, in trained postures,
    // collared) with the appraisal class also weighted
    // in her map. The collar flips her ENGAGE. Sub-
    // space (COLLARED) gates on the two ownership
    // classes. The pattern is denial-kept: the deepest
    // refractory depth in the cast, the longest
    // refractory, a negative episode trend and a thin
    // carry — culmination is rare, singular, and
    // episode 2 is weaker than episode 1. The chastity
    // device's between-scenes input has no data
    // expression in the format (filed gap, D-273).
    CharacterProfile {
        site_weights: weights(&[
            ("neck", 1000),
            ("clitoris", 350),
            ("g_spot", 400),
            ("nipples", 500),
            ("mouth", 450),
        ]),
        default_site_weight: 400,
        activity_weights: weights(&[
            ("slave_training", 950),
            ("collar_leash", 900),
            ("machine_stimulation", 850),
            ("appraisal", 800),
            ("tantric", 700),
        ]),
        weak_spot: Some(WeakSpot {
            site: "neck".to_string(),
            threshold_dose: 900,
            mode: SwitchMode::Engage,
        }),
        submission_classes: vec!["slave_training".to_string(), "collar_leash".to_string()],
        subspace_threshold_permille: 250,
        episode_trend_permille: -100,
        refractory_depth_permille: 1500,
        refractory_ticks: 240 * 60,
        afterglow_ticks: 120 * 60,
        carry_scale_permille: 800,
        ..CharacterProfile::default()
    }
}

fn lux_profile() -> CharacterProfile {
    // D-273 locked set: exhibitionism · voyeurism/
    // being recorded · free use · praise kink · body
    // writing · deep throating/swallowing. D-262
    // dynamic: she is gaze-coupled — the audience and
    // recording classes dominate her map, praise is
    // high, body writing (inscription) is its own
    // class, and the mouth is her maximal site. Her
    // weak spot is praise inscribed: the dose channel
    // is site-only, so it is expressed at the chest,
    // the inscription site, flipping her ENGAGE (she
    // goes LIVE). Sub-space (AVAILABLE) gates on the
    // free-use class alone. The pattern is take-locked
    // consistency: a flat episode trend (take after
    // take lands the same), the §5.3 refractory depth,
    // and a long refractory — a take ends with a CUT,
    // not a flow into the next. The watcher-attention
    // leak (her charge decays when the gaze leaves)
    // is not an engine mechanic (filed gap, D-273).
    CharacterProfile {
        site_weights: weights(&[
            ("mouth", 1000),
            ("chest", 900),
            ("clitoris", 700),
            ("g_spot", 650),
            ("nipples", 750),
            ("face", 800),
        ]),
        default_site_weight: 650,
        activity_weights: weights(&[
            ("audience_present", 950),
            ("recording", 900),
            ("praise", 850),
            ("free_use", 850),
            ("body_writing", 800),
        ]),
        weak_spot: Some(WeakSpot {
            site: "chest".to_string(),
            threshold_dose: 550,
            mode: SwitchMode::Engage,
        }),
        submission_classes: vec!["free_use".to_string()],
        subspace_threshold_permille: 150,
        episode_trend_permille: 0,
        refractory_depth_permille: 1000,
        refractory_ticks: 150 * 60,
        afterglow_ticks: 30 * 60,
        carry_scale_permille: 900,
        ..CharacterProfile::default()
    }
}

fn piper_profile() -> CharacterProfile {
    // D-273 locked set: protocol · punishment · sensory
    // deprivation · orgasm control · anal · following
    // commands/JOI. D-261 dynamic: the discipline class
    // leads — protocol dominant, following commands
    // beside it (the two gate her PENITENT sub-space),
    // punishment high. The anal site is maximal and is
    // her weak spot: sustained input there flips her
    // ENGAGE (PENITENT's gain posture). Sensory
    // deprivation is a weighted class; the deprivation
    // state-multiplier on her remaining channels is
    // not an engine mechanic (filed gap, D-273). The
    // pattern is permission-gated as far as data can
    // say it: a deep refractory, a long refractory, a
    // negative trend and the cast's thinnest carry —
    // nothing about her builds twice in a row. The
    // permission flag itself is an engine gap, filed
    // in D-273, not built here.
    CharacterProfile {
        site_weights: weights(&[
            ("anus", 1000),
            ("clitoris", 500),
            ("g_spot", 550),
            ("nipples", 450),
        ]),
        default_site_weight: 450,
        activity_weights: weights(&[
            ("protocol", 950),
            ("following_commands", 900),
            ("punishment", 850),
            ("sensory_deprivation", 700),
        ]),
        weak_spot: Some(WeakSpot {
            site: "anus".to_string(),
            threshold_dose: 2_400,
            mode: SwitchMode::Engage,
        }),
        submission_classes: vec!["protocol".to_string(), "following_commands".to_string()],
        subspace_threshold_permille: 250,
        episode_trend_permille: -50,
        refractory_depth_permille: 1400,
        refractory_ticks: 180 * 60,
        afterglow_ticks: 60 * 60,
        carry_scale_permille: 700,
        ..CharacterProfile::default()
    }
}

/// The union script's activity half: every cast
/// member's classes at once, at scene intensities.
/// The intensities are deliberately moderate: at full
/// strength the combination law (strongest leads, the
/// rest add at 0.35) saturates every profile's input
/// at once and the maps stop differentiating — the
/// first D-273 run fired all six at the same tick
/// with the same P1, which is the measurement that
/// set these values.
const SIX_ACTIVITIES: &[(&str, u32)] = &[
    ("power_exchange", 300),
    ("hypnotic_suggestion", 300),
    ("domestic_servitude", 300),
    ("slave_training", 300),
    ("protocol", 300),
    ("following_commands", 300),
    ("audience_present", 300),
    ("praise", 300),
    ("rope_bondage", 300),
    ("collar_leash", 220),
    ("machine_stimulation", 220),
    ("appraisal", 220),
    ("free_use", 220),
    ("recording", 220),
    ("suspension", 200),
    ("begging", 200),
    ("breeding", 200),
    ("medical_play", 200),
    ("punishment", 200),
    ("body_writing", 200),
    ("restraint_fixture", 200),
    ("multi_penetration", 200),
    ("tantric", 160),
    ("sensory_deprivation", 160),
];

fn device_at(item_id: &str, site: &str, amplitude: u32) -> DeviceSpec {
    DeviceSpec {
        item_id: item_id.to_string(),
        site: site.to_string(),
        wave: Some(WaveSpec::constant(amplitude)),
        contact_quality: 1000,
        arousal_floor: 0,
    }
}

fn apply_six_script(engine: &mut ArousalEngine) {
    engine.apply_item(device_at("vibe", "clitoris", 600));
    engine.apply_item(device_at("plug", "anus", 300));
    for (class, intensity) in SIX_ACTIVITIES {
        engine.set_activity(class, *intensity);
    }
}

/// The union script's touch half: collar/appraisal
/// contact at the neck and inscription contact at the
/// chest, alternating by tick — identical for every
/// profile.
fn six_touch(engine: &ArousalEngine) -> TickInputs {
    if engine.tick() % 2 == 0 {
        TickInputs {
            touch: Some(("neck".to_string(), 260)),
            ..NONE.clone()
        }
    } else {
        TickInputs {
            touch: Some(("chest".to_string(), 240)),
            ..NONE.clone()
        }
    }
}

fn phase_code(phase: Phase) -> u64 {
    match phase {
        Phase::Baseline => 0,
        Phase::Stimulated => 1,
        Phase::Culminating => 2,
        Phase::Taper => 3,
        Phase::Afterglow => 4,
        Phase::Refractory => 5,
    }
}

fn switch_code(state: SwitchState) -> u64 {
    match state {
        SwitchState::None => 0,
        SwitchState::Engage => 1,
        SwitchState::Blank => 2,
    }
}

/// FNV-1a fold of one tick's vector into the run's
/// trajectory hash.
fn fold_tick(hash: &mut u64, engine: &ArousalEngine) {
    let v = engine.vector();
    for value in [
        u64::from(v.pleasure_charge),
        u64::from(v.arousal),
        u64::from(v.sensitivity),
        u64::from(v.dose),
        phase_code(v.phase),
        switch_code(v.switch_state),
        u64::from(v.subspace),
        u64::from(v.arc_affinity),
    ] {
        *hash = (*hash ^ value).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

struct SixOutcome {
    ttc1: Option<u64>,
    switch1: SwitchState,
    switch_tick1: Option<u64>,
    subspace_tick1: Option<u64>,
    p1: Option<u32>,
    arc1: u32,
    lactation_coupled: bool,
    s_at_rearm: Option<u32>,
    ttc2: Option<u64>,
    p2: Option<u32>,
    hash: u64,
}

/// One run of the union script: two episodes with a
/// full rest between, the R-G10 shape widened. The
/// episode-2 wait is harness plumbing, uniform for
/// all six: the script is re-applied only once the
/// refractory has fully elapsed with inputs off —
/// two profiles' refractory durations exceed the
/// window in which a no-input rest leaves Q < 150
/// while the phase clock is still running, and with
/// input applied during that wait Q saturates and
/// re-arm becomes unreachable (filed gap, D-273).
fn run_six_script(profile: CharacterProfile) -> SixOutcome {
    let mut engine = ArousalEngine::new(profile);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    // Episode 1.
    apply_six_script(&mut engine);
    let start = engine.tick();
    let mut switch_tick1 = None;
    let mut subspace_tick1 = None;
    let mut lactation_coupled = false;
    let (mut p1, mut fire1) = (None, None);
    for _ in 0..600 * 60 {
        let input = six_touch(&engine);
        let out = engine.step(&input);
        fold_tick(&mut hash, &engine);
        let v = engine.vector();
        if switch_tick1.is_none() && v.switch_state != SwitchState::None {
            switch_tick1 = Some(engine.tick() - start);
        }
        if subspace_tick1.is_none() && v.subspace {
            subspace_tick1 = Some(engine.tick() - start);
        }
        lactation_coupled |= out.effects.lactation_coupled;
        if let Some(&ArousalEvent::Culmination { p_permille, .. }) = out
            .events
            .iter()
            .find(|e| matches!(e, ArousalEvent::Culmination { .. }))
        {
            p1 = Some(p_permille);
            fire1 = Some(engine.tick() - start);
            break;
        }
    }
    let switch1 = engine.vector().switch_state;
    let arc1 = engine.vector().arc_affinity;
    let (mut s_at_rearm, mut ttc2, mut p2) = (None, None, None);
    if fire1.is_some() {
        // Rest: everything off until Q < 150.
        engine.remove_item("vibe");
        engine.remove_item("plug");
        engine.clear_activities();
        let mut guard = 0u64;
        while engine.vector().pleasure_charge >= 150 {
            engine.step(&NONE);
            fold_tick(&mut hash, &engine);
            guard += 1;
            assert!(guard < 900 * 60, "rest completes");
        }
        // Wait out the refractory with inputs off (see
        // the fn comment), then re-apply the script.
        let mut guard = 0u64;
        while !(engine.vector().phase == Phase::Refractory
            && engine.vector().phase_ticks >= 600 * 60)
        {
            engine.step(&NONE);
            fold_tick(&mut hash, &engine);
            guard += 1;
            assert!(guard < 1800 * 60, "the refractory elapses");
        }
        apply_six_script(&mut engine);
        let mut rearm_tick = None;
        for _ in 0..600 * 60 {
            let input = six_touch(&engine);
            let out = engine.step(&input);
            fold_tick(&mut hash, &engine);
            if rearm_tick.is_none()
                && out
                    .events
                    .iter()
                    .any(|e| matches!(e, ArousalEvent::Rearmed { .. }))
            {
                rearm_tick = Some(engine.tick());
                s_at_rearm = Some(engine.vector().sensitivity);
            }
            if let Some(&ArousalEvent::Culmination { p_permille, .. }) = out
                .events
                .iter()
                .find(|e| matches!(e, ArousalEvent::Culmination { .. }))
            {
                p2 = Some(p_permille);
                ttc2 = rearm_tick.map(|r| engine.tick() - r);
                break;
            }
        }
    }
    SixOutcome {
        ttc1: fire1,
        switch1,
        switch_tick1,
        subspace_tick1,
        p1,
        arc1,
        lactation_coupled,
        s_at_rearm,
        ttc2,
        p2,
        hash,
    }
}

#[test]
fn r_g13_six_profile_distinctness() {
    let outcomes = [
        ("Prima", run_six_script(prima_profile())),
        ("Yui", run_six_script(yui_profile())),
        ("Alma", run_six_script(alma_profile())),
        ("Sloane", run_six_script(sloane_profile())),
        ("Lux", run_six_script(lux_profile())),
        ("Piper", run_six_script(piper_profile())),
    ];
    for (name, o) in &outcomes {
        println!(
            "{name}: ttc1 {:?}, switch {:?} at {:?}, subspace at {:?}, P1 {:?}, arc1 {}, \
             lactation {}, S@rearm {:?}, ttc2 {:?}, P2 {:?}, hash {:016x}",
            o.ttc1,
            o.switch1,
            o.switch_tick1,
            o.subspace_tick1,
            o.p1,
            o.arc1,
            o.lactation_coupled,
            o.s_at_rearm,
            o.ttc2,
            o.p2,
            o.hash
        );
    }
    // Every profile culminates under the union script.
    for (name, o) in &outcomes {
        assert!(o.ttc1.is_some(), "{name} culminates in episode 1");
        assert!(o.ttc2.is_some(), "{name} culminates in episode 2");
    }
    // The D-238 claim at six: pairwise-distinct
    // trajectories (hash) and pairwise-distinct
    // outcome tuples.
    let mut hashes: Vec<u64> = outcomes.iter().map(|(_, o)| o.hash).collect();
    hashes.sort_unstable();
    hashes.dedup();
    assert_eq!(hashes.len(), 6, "six distinct trajectory hashes");
    for (i, (name_a, a)) in outcomes.iter().enumerate() {
        for (name_b, b) in outcomes.iter().skip(i + 1) {
            assert!(
                (
                    a.ttc1,
                    a.switch1,
                    a.switch_tick1,
                    a.subspace_tick1,
                    a.p1,
                    a.arc1,
                    a.ttc2,
                    a.p2
                ) != (
                    b.ttc1,
                    b.switch1,
                    b.switch_tick1,
                    b.subspace_tick1,
                    b.p1,
                    b.arc1,
                    b.ttc2,
                    b.p2
                ),
                "{name_a} and {name_b} outcome tuples differ"
            );
        }
    }
    let get = |name: &str| &outcomes.iter().find(|(n, _)| *n == name).unwrap().1;
    // Switch canon: Prima has no weak spot and never
    // flips; the other five flip ENGAGE except Alma,
    // whose BLANK is hers alone.
    assert_eq!(get("Prima").switch1, SwitchState::None);
    assert_eq!(get("Yui").switch1, SwitchState::Engage);
    assert_eq!(get("Alma").switch1, SwitchState::Blank);
    assert_eq!(get("Sloane").switch1, SwitchState::Engage);
    assert_eq!(get("Lux").switch1, SwitchState::Engage);
    assert_eq!(get("Piper").switch1, SwitchState::Engage);
    // Sub-space canon: Alma (service), Sloane
    // (COLLARED), Lux (AVAILABLE) and Piper (PENITENT)
    // enter; Prima and Yui do not.
    for name in ["Alma", "Sloane", "Lux", "Piper"] {
        assert!(
            get(name).subspace_tick1.is_some(),
            "{name} enters sub-space"
        );
    }
    for name in ["Prima", "Yui"] {
        assert!(
            get(name).subspace_tick1.is_none(),
            "{name} does not enter sub-space"
        );
    }
    // The culmination-coupled side channel fires only
    // where canon carries it (Alma).
    assert!(get("Alma").lactation_coupled);
    for name in ["Prima", "Yui", "Sloane", "Lux", "Piper"] {
        assert!(!get(name).lactation_coupled, "{name} has no side channel");
    }
}
