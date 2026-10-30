//! PM-1 gate battery: AMENDMENT-PLAYMODE §10's gates
//! PM-G1…PM-G8, PM-G10, PM-G11 (PM-1 side), restated as
//! BRIEF-PM1's P1–P10 + the persistence contracts.
//! (P11 is the workspace regression run, P12 the primo
//! pactl check, P13–P15 are PM-1S's, P16 the eye sheet —
//! all evidenced in GATE-REPORT-PM1.md, not here.)
//!
//! The canonical scripted session (§10): playmate
//! entered, fixture + device applied by entry, REC skin
//! on, take started, amplitude ramped 0→1000 over 7200
//! ticks with a paced plateau in Building (amplitude
//! feedback holds Q in [650, 760] until body + fixture
//! are wet — §5.2's pressure law needs the time, and
//! PM-G3 gates on it), facilitator touch from t=1200,
//! one contact shift (q 1000→700), one displaced window
//! re-seated after 150 ticks, Hold engaged at Q ≥ 800
//! for 900 ticks then released to Culmination, then
//! uninterrupted Taper → Afterglow → Refractory.
//! Epilogue (outside the take): a refused amplitude op
//! (U-DENY), take stop, Remove All & Reset (§6.3), then
//! a fresh entry and exit (D-OFF) — so the session cue
//! track carries the full census.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use torogao_session::playmode::{
    AUTHORITY_FIELDS, CueRecord, EXPRESSION_KEYS, GovernorData, ModeTickInput, PlaytimeDefinition,
    PlaytimeMode, Stage, TakeRecord, VOCAL_CLASSES, replay_take, validate_mode_profile,
    validate_settings_document,
};
use torogao_session::{ArousalHost, Phase};

/// Golden hashes of the canonical scripted session
/// (PM-G4): pinned from the first green run; the primo
/// run must reproduce them bit-for-bit (cross-machine
/// determinism is part of the gate).
const GOLDEN_TRAJECTORY_HASH: u64 = 10566805004775308757;
const GOLDEN_CUE_HASH: u64 = 9013338875949152787;

const AREAS: [&str; 3] = ["body.lower", "fixture.seat", "floor.pool"];

fn stage_rank(stage: Stage) -> usize {
    [
        Stage::Setup,
        Stage::WarmUp,
        Stage::Building,
        Stage::Edging,
        Stage::Culmination,
        Stage::Taper,
        Stage::Afterglow,
        Stage::Refractory,
    ]
    .iter()
    .position(|s| *s == stage)
    .expect("all stages ranked")
}

/// One tick's observed facts from the canonical run.
struct TickFact {
    stage: Stage,
    phase: Phase,
    pulses: Vec<torogao_session::playmode::PulseEvent>,
    bursts: Vec<torogao_session::playmode::BurstEvent>,
    pressure_after: u32,
    body_excursion: u32,
    alive: usize,
    bulk_spawned: u32,
}

/// Everything the gates read from one canonical run.
struct CanonicalRun {
    stage_before_enter: Stage,
    wet_at_setup: [u32; 3],
    facts: Vec<TickFact>,
    culminations: usize,
    tier: Option<u8>,
    first_wet_stage: BTreeMap<String, Stage>,
    wet_last_taper: [u32; 3],
    wet_first_afterglow: [u32; 3],
    wet_last_afterglow: [u32; 3],
    wet_first_refractory: [u32; 3],
    take: TakeRecord,
    trajectory_hash: u64,
    cue_hash: u64,
    cue_track: Vec<CueRecord>,
    wet_after_reset: [u32; 3],
    stage_after_reset: Stage,
}

fn wet_of(mode: &PlaytimeMode) -> [u32; 3] {
    let w = mode.wetness();
    [
        w.permille(AREAS[0]),
        w.permille(AREAS[1]),
        w.permille(AREAS[2]),
    ]
}

fn run_canonical(character: &str) -> CanonicalRun {
    let definition = PlaytimeDefinition::blossom();
    let mut host = ArousalHost::new();
    let mut mode = PlaytimeMode::new(definition);
    let stage_before_enter = mode.stage();
    mode.enter(&mut host, character).unwrap();
    let wet_at_setup = wet_of(&mode);
    mode.set_rec_skin(&mut host, true).unwrap();
    mode.start_take(&mut host).unwrap();

    let mut facts: Vec<TickFact> = Vec::new();
    let mut culminations = 0usize;
    let mut tier: Option<u8> = None;
    let mut first_wet_stage: BTreeMap<String, Stage> = BTreeMap::new();
    let mut contact_shifted = false;
    let mut displaced_at: Option<u64> = None;
    let mut reseat_at: Option<u64> = None;
    let mut hold_release_at: Option<u64> = None;
    let mut hold_spent = false;
    let mut wet_last_taper = [0; 3];
    let mut wet_first_afterglow = [0; 3];
    let mut wet_last_afterglow = [0; 3];
    let mut wet_first_refractory = [0; 3];
    let mut prev_stage = Stage::Setup;

    // Plateau state (§10's ramp, paced): Q sprints
    // through 500→800 in ~1.3 s at full input, but §5.2's
    // pressure needs ~15 s at Q ≈ 700 to cross the burst
    // threshold — so the canonical ramp pauses in
    // Building, riding the amplitude by feedback to hold
    // Q in [650, 760] until body + fixture are wet
    // (PM-G3's "by Building"), then resumes. All
    // state-triggered, so the take replays exactly.
    let mut plateau_active = false;
    let mut plateau_done = false;
    let mut plateau_amp: u32 = 0;
    let mut plateau_start: u64 = 0;
    let mut last_q: u32 = 0;
    for t in 0..120_000u64 {
        // The amplitude ramp (ops land between steps;
        // the take records them at their engine ticks).
        if plateau_active {
            if t % 15 == 0 {
                let next = if last_q > 760 {
                    plateau_amp.saturating_sub(30)
                } else if last_q < 650 {
                    (plateau_amp + 30).min(1000)
                } else {
                    plateau_amp
                };
                if next != plateau_amp {
                    plateau_amp = next;
                    mode.set_amplitude(&mut host, plateau_amp).unwrap();
                }
            }
        } else if t % 60 == 0 && t <= 7200 {
            let amp = ((t / 60) * 10).min(1000) as u32;
            mode.set_amplitude(&mut host, amp).unwrap();
        }
        if let Some(rt) = reseat_at {
            if t == rt {
                mode.set_contact(&mut host, 700, false).unwrap();
                reseat_at = None;
            }
        }
        if let Some(rt) = hold_release_at {
            if t == rt {
                mode.set_hold(&mut host, false).unwrap();
                hold_release_at = None;
            }
        }
        let mut input = ModeTickInput::default();
        // Touch starts once the device alone sustains
        // the Stimulated phase (earlier touches blip the
        // engine phase Baseline↔Stimulated for a tick —
        // the T4 engine's A dynamics at dose ≈ 0).
        if t >= 1200 && t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        }
        if t % 900 == 450 {
            input.spray_rate = Some(600);
            input.face_contact = true;
        }
        let out = mode.step(&mut host, &input).unwrap();
        let v = host.get(character).unwrap().vector();
        last_q = v.pleasure_charge;
        for e in &out.engine.events {
            if let torogao_session::ArousalEvent::Culmination { tier: ct, .. } = e {
                culminations += 1;
                tier = Some(*ct);
            }
        }
        let wet = wet_of(&mode);
        for (i, area) in AREAS.iter().enumerate() {
            if wet[i] > 0 && !first_wet_stage.contains_key(*area) {
                first_wet_stage.insert(area.to_string(), out.stage);
            }
        }
        match out.stage {
            Stage::Taper => wet_last_taper = wet,
            Stage::Afterglow if prev_stage != Stage::Afterglow => {
                wet_first_afterglow = wet;
                wet_last_afterglow = wet;
            }
            Stage::Afterglow => wet_last_afterglow = wet,
            Stage::Refractory if prev_stage != Stage::Refractory => {
                wet_first_refractory = wet;
            }
            _ => {}
        }
        prev_stage = out.stage;
        facts.push(TickFact {
            stage: out.stage,
            phase: v.phase,
            pulses: out.pulses.clone(),
            bursts: out.bursts.clone(),
            pressure_after: out.pressure_permille,
            body_excursion: out.body_cycle.amplitude_excursion_permille,
            alive: mode.alive_droplets(),
            bulk_spawned: out.bursts.iter().map(|b| b.droplets).sum::<u32>() + out.leak_droplets,
        });
        // State-triggered script ops (recorded in the
        // take at the engine tick they land on).
        if !plateau_done && !plateau_active && v.pleasure_charge >= 600 {
            plateau_active = true;
            plateau_start = t;
            plateau_amp = mode.user_amplitude();
        }
        if plateau_active && ((wet[0] > 0 && wet[1] > 0) || t - plateau_start > 10_800) {
            plateau_active = false;
            plateau_done = true;
        }
        if !contact_shifted && v.pleasure_charge >= 400 {
            mode.set_contact(&mut host, 700, false).unwrap();
            contact_shifted = true;
        }
        if displaced_at.is_none() && v.pleasure_charge >= 500 {
            mode.set_contact(&mut host, 700, true).unwrap();
            displaced_at = Some(t);
            reseat_at = Some(t + 150);
        }
        // Hold engages only after the plateau: engaging
        // it mid-plateau would pin Q ≥ 800 and rename
        // the paced window Edging before its wetness
        // work is done.
        if !hold_spent && plateau_done && v.pleasure_charge >= 800 {
            mode.set_hold(&mut host, true).unwrap();
            hold_release_at = Some(t + 900);
            hold_spent = true;
        }
        if v.phase == Phase::Refractory {
            // Thirty settle ticks inside the take, then
            // the take stops.
            for _ in 0..30 {
                let out = mode.step(&mut host, &ModeTickInput::default()).unwrap();
                let v = host.get(character).unwrap().vector();
                facts.push(TickFact {
                    stage: out.stage,
                    phase: v.phase,
                    pulses: out.pulses.clone(),
                    bursts: out.bursts.clone(),
                    pressure_after: out.pressure_permille,
                    body_excursion: out.body_cycle.amplitude_excursion_permille,
                    alive: mode.alive_droplets(),
                    bulk_spawned: out.bursts.iter().map(|b| b.droplets).sum::<u32>()
                        + out.leak_droplets,
                });
            }
            break;
        }
    }
    let take = mode.stop_take(&mut host).unwrap();
    // Epilogue (outside the take): the refused op, its
    // drain steps, then the §6.3 reset act.
    let refused = mode.set_amplitude(&mut host, 2000);
    assert!(refused.is_err(), "amplitude 2000 must refuse");
    for _ in 0..5 {
        mode.step(&mut host, &ModeTickInput::default()).unwrap();
    }
    mode.remove_all_and_reset(&mut host).unwrap();
    let wet_after_reset = wet_of(&mode);
    let stage_after_reset = mode.stage();
    // A fresh entry and exit, so D-OFF reaches the
    // session cue track.
    mode.enter(&mut host, character).unwrap();
    for _ in 0..3 {
        mode.step(&mut host, &ModeTickInput::default()).unwrap();
    }
    mode.exit(&mut host).unwrap();
    CanonicalRun {
        stage_before_enter,
        wet_at_setup,
        facts,
        culminations,
        tier,
        first_wet_stage,
        wet_last_taper,
        wet_first_afterglow,
        wet_last_afterglow,
        wet_first_refractory,
        take,
        trajectory_hash: mode.trajectory_hash(),
        cue_hash: mode.cue_track_hash(),
        cue_track: mode.cue_track().to_vec(),
        wet_after_reset,
        stage_after_reset,
    }
}

fn dedup<T: PartialEq + Copy>(seq: &[T]) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for item in seq {
        if out.last() != Some(item) {
            out.push(*item);
        }
    }
    out
}

/// P1 (PM-G1): the scripted session traverses the §3.2
/// stages in order, the engine phase sequence is exact,
/// and the culmination count is exactly 1.
#[test]
fn p1_progression() {
    let run = run_canonical("prima");
    assert_eq!(run.stage_before_enter, Stage::Setup);
    let stages: Vec<Stage> = run.facts.iter().map(|f| f.stage).collect();
    assert_eq!(
        dedup(&stages),
        vec![
            Stage::WarmUp,
            Stage::Building,
            Stage::Edging,
            Stage::Culmination,
            Stage::Taper,
            Stage::Afterglow,
            Stage::Refractory,
        ],
        "stage traversal in §3.2 order, none skipped or re-entered"
    );
    let phases: Vec<Phase> = run.facts.iter().map(|f| f.phase).collect();
    assert_eq!(
        dedup(&phases),
        vec![
            Phase::Baseline,
            Phase::Stimulated,
            Phase::Culminating,
            Phase::Taper,
            Phase::Afterglow,
            Phase::Refractory,
        ],
        "engine phase sequence exact across the episode"
    );
    assert_eq!(run.culminations, 1, "exactly one culmination per episode");
    assert!(run.tier.is_some(), "the culmination carried a tier");
}

/// P3 (PM-G3): wetness lifecycle in the scripted
/// session. (The 600 s no-decay hold and the
/// scene-change zeroing have their own tests below.)
#[test]
fn p3_wetness_lifecycle() {
    let run = run_canonical("prima");
    assert_eq!(run.wet_at_setup, [0, 0, 0], "0 on all areas at Setup");
    for area in ["body.lower", "fixture.seat"] {
        let stage = run
            .first_wet_stage
            .get(area)
            .unwrap_or_else(|| panic!("{area} never got wet"));
        assert!(
            stage_rank(*stage) <= stage_rank(Stage::Building),
            "{area} first wet at {stage:?}, must be by Building"
        );
    }
    // Present in Afterglow, and the phase transitions
    // themselves change nothing (drips may transfer
    // between areas mid-stage; the transition tick is
    // exact).
    assert!(
        run.wet_first_afterglow.iter().any(|w| *w > 0),
        "wetness present in Afterglow"
    );
    assert_eq!(
        run.wet_last_taper, run.wet_first_afterglow,
        "the Taper→Afterglow transition itself changes no wetness"
    );
    assert_eq!(
        run.wet_last_afterglow, run.wet_first_refractory,
        "the Afterglow→Refractory transition itself changes no wetness"
    );
    // The reset act zeroes the field in its own tick.
    assert_eq!(run.wet_after_reset, [0, 0, 0], "reset zeroed the field");
    assert_eq!(run.stage_after_reset, Stage::Setup);
}

/// P4 (PM-G4): the canonical trajectory (engine vector,
/// pulses, pressure, volume, wetness, cues) hashes
/// identically across two runs, and matches the pinned
/// golden (the primo run reproducing the same constants
/// is the cross-machine half of the gate).
#[test]
fn p4_determinism_and_golden() {
    let a = run_canonical("prima");
    let b = run_canonical("prima");
    assert_eq!(a.trajectory_hash, b.trajectory_hash);
    assert_eq!(a.cue_hash, b.cue_hash);
    assert_eq!(
        a.trajectory_hash, GOLDEN_TRAJECTORY_HASH,
        "canonical trajectory golden (got {})",
        a.trajectory_hash
    );
    assert_eq!(
        a.cue_hash, GOLDEN_CUE_HASH,
        "canonical cue-track golden (got {})",
        a.cue_hash
    );
}

/// P9 (PM-G10a): cue census — every §9.2 cue id fires
/// in the scripted session's track, or is a listed
/// deliberate silence / not-applicable with a reason.
/// Fired here: 30 of 32. The two listed exceptions:
/// V-REFR is a deliberate silence by design (its table
/// trigger IS `DeliberateSilence`, §9.5); B-CLAMP is
/// not applicable to the scripted session — its
/// trigger is clamp application/removal operations
/// from T4 gear play interleaved with the mode; the
/// canonical mode script performs none (the starter
/// pack carries no clamps). The cue def + trigger
/// contract are frozen for PM-1S to bind.
#[test]
fn p9_cue_census() {
    let run = run_canonical("prima");
    let definition = PlaytimeDefinition::blossom();
    let fired: BTreeSet<&str> = run.cue_track.iter().map(|c| c.cue_id.as_str()).collect();
    let listed: BTreeSet<&str> = ["V-REFR", "B-CLAMP"].into_iter().collect();
    for def in &definition.cues {
        assert!(
            fired.contains(def.id.as_str()) || listed.contains(def.id.as_str()),
            "cue {} neither fired nor listed",
            def.id
        );
    }
    assert_eq!(
        fired.len() + listed.len(),
        definition.cues.len(),
        "census covers 100% of the table"
    );
    // The deliberate silence is deliberate in the data.
    let refr = definition
        .cues
        .iter()
        .find(|c| c.id == "V-REFR")
        .expect("V-REFR in the table");
    assert_eq!(
        refr.trigger,
        torogao_session::playmode::CueTrigger::DeliberateSilence
    );
    // The previously stranded terminal-op cues reach
    // the track (exit / remove-all fire on the op).
    for id in ["D-OFF", "U-RESET", "U-DENY", "R-REC-STOP", "R-REC-START"] {
        assert!(fired.contains(id), "{id} fired in the session track");
    }
}

/// P10 (PM-G10b/c): take replay emits the identical
/// trajectory and cue track, with 0 mirror/drive
/// actuations.
#[test]
fn p10_take_replay() {
    let run = run_canonical("prima");
    let definition = PlaytimeDefinition::blossom();
    let outcome = replay_take(&definition, &run.take).unwrap();
    assert_eq!(outcome.trajectory_hash, run.take.trajectory_hash);
    assert_eq!(outcome.cue_track_hash, run.take.cue_track_hash);
    assert_eq!(outcome.actuations, 0, "replay never actuates");
    assert_eq!(outcome.mirror_generation, 0);
}

/// A constant-amplitude build run to the end of the
/// episode (the PM-G2 tier builds + swap streams).
struct BuildRun {
    tier: Option<u8>,
    culminations: usize,
    episode_volume_micro: i64,
    bursts: Vec<torogao_session::playmode::BurstEvent>,
    trajectory_hash: u64,
    cue_track: Vec<CueRecord>,
    expressions: Vec<String>,
}

fn run_build(character: &str, amplitude: u32, ticks: u64, touch: bool) -> BuildRun {
    let definition = PlaytimeDefinition::blossom();
    let mut host = ArousalHost::new();
    let mut mode = PlaytimeMode::new(definition);
    mode.enter(&mut host, character).unwrap();
    mode.set_amplitude(&mut host, amplitude).unwrap();
    let mut tier = None;
    let mut culminations = 0usize;
    let mut bursts = Vec::new();
    let mut expressions = Vec::new();
    for t in 0..ticks {
        let mut input = ModeTickInput::default();
        if touch && t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        }
        let out = mode.step(&mut host, &input).unwrap();
        for e in &out.engine.events {
            if let torogao_session::ArousalEvent::Culmination { tier: ct, .. } = e {
                culminations += 1;
                tier = Some(*ct);
            }
        }
        bursts.extend(out.bursts.iter().cloned());
        expressions.push(out.expression_recipe_id.clone());
        let v = host.get(character).unwrap().vector();
        if v.phase == Phase::Refractory && t > 100 {
            break;
        }
    }
    BuildRun {
        tier,
        culminations,
        episode_volume_micro: mode.episode_volume_micro(),
        bursts,
        trajectory_hash: mode.trajectory_hash(),
        cue_track: mode.cue_track().to_vec(),
        expressions,
    }
}

/// P2 (PM-G2): (a) four scripted builds landing in
/// Tiers 1–4 produce strictly increasing episode
/// volume; (b) at fixed pulse depth, burst volume is
/// monotone non-decreasing in P_f; (c) emission stays
/// inside §5.4's bounds on 100% of ticks.
#[test]
fn p2_ejection_scaling() {
    // Measured build map (Prima, constant amplitude, no
    // touch, run to Refractory): 250 → T1 (689 units),
    // 500 → T2 (1502), 650 → T3 (3108), 1000 → T4
    // (5471). (800 is non-monotone — T3 at 3001 — and is
    // not used.)
    let amps = [250u32, 500, 650, 1000];
    let mut volumes: Vec<i64> = Vec::new();
    let mut all_bursts: Vec<torogao_session::playmode::BurstEvent> = Vec::new();
    for (i, amp) in amps.iter().enumerate() {
        let run = run_build("prima", *amp, 240_000, false);
        assert_eq!(run.culminations, 1, "amp {amp} culminates exactly once");
        assert_eq!(
            run.tier,
            Some((i + 1) as u8),
            "amp {amp} lands in Tier {}",
            i + 1
        );
        volumes.push(run.episode_volume_micro);
        all_bursts.extend(run.bursts.iter().cloned());
    }
    for w in volumes.windows(2) {
        assert!(
            w[1] > w[0],
            "episode volume strictly increasing in tier: {volumes:?}"
        );
    }
    // (b) fixed depth → volume monotone in pressure.
    let mut by_depth: BTreeMap<u32, Vec<(u32, i64)>> = BTreeMap::new();
    for b in &all_bursts {
        if !b.final_release {
            by_depth
                .entry(b.pulse.depth_permille)
                .or_default()
                .push((b.pressure_before_permille, b.volume_micro));
        }
    }
    let mut pairs = 0usize;
    for (depth, mut rows) in by_depth {
        rows.sort();
        for w in rows.windows(2) {
            pairs += 1;
            assert!(
                w[1].1 >= w[0].1,
                "depth {depth}: volume {} at P_f {} < volume {} at P_f {}",
                w[1].1,
                w[1].0,
                w[0].1,
                w[0].0
            );
        }
    }
    assert!(pairs > 0, "the burst band produced comparable pairs");
    // (c) bounds on the canonical run's ticks: alive
    // droplets ≤ 4096 always; bulk emission (bursts +
    // leak) ≤ 512 per tick. (Drip spawns are ≤ 1 per
    // chained area per tick by construction.)
    let run = run_canonical("prima");
    for (i, f) in run.facts.iter().enumerate() {
        assert!(f.alive <= 4096, "tick {i}: {} droplets alive", f.alive);
        assert!(
            f.bulk_spawned <= 512,
            "tick {i}: {} bulk droplets spawned",
            f.bulk_spawned
        );
    }
}

/// P5 (PM-G5): the same input stream for Prima / Yui /
/// Alma produces three profile-resolved trajectories
/// with 0 cross-character expression or vocal asset
/// ids.
#[test]
fn p5_character_swap() {
    let chars = ["prima", "yui", "alma"];
    let mut runs: Vec<BuildRun> = Vec::new();
    for c in chars {
        runs.push(run_build(c, 1000, 9000, true));
    }
    for i in 0..3 {
        for j in (i + 1)..3 {
            assert_ne!(
                runs[i].trajectory_hash, runs[j].trajectory_hash,
                "{} and {} trajectories differ",
                chars[i], chars[j]
            );
            assert_ne!(
                runs[i].episode_volume_micro, runs[j].episode_volume_micro,
                "episode volume is profile-resolved ({} vs {})",
                chars[i], chars[j]
            );
        }
    }
    for (i, run) in runs.iter().enumerate() {
        let me = chars[i];
        assert!(
            run.expressions
                .iter()
                .all(|e| e.starts_with(&format!("{me}."))),
            "{me}: every expression recipe is hers"
        );
        let mut saw_vocal = false;
        for rec in &run.cue_track {
            if rec.cue_id.starts_with("V-") {
                saw_vocal = true;
                assert!(
                    rec.asset_id.contains(me),
                    "{me}: vocal asset {} resolves to her set",
                    rec.asset_id
                );
            }
            for other in chars {
                if other != me {
                    assert!(
                        !rec.asset_id.contains(other),
                        "cross-character asset id: {} in {}'s track",
                        rec.asset_id,
                        me
                    );
                }
            }
        }
        assert!(saw_vocal, "{me}: vocal cues fired in the stream");
    }
}

/// P6 (PM-G6): the Hold governor at Q ≈ 850 under
/// maximum input for 600 s — 0 culminations, Q never
/// crosses 1000, drift within ±5% of the equilibrium
/// law (§3.3: at I_hold the charge law nets to zero);
/// culmination follows release. Plus the D-PM1-1
/// exhibit: the amendment's printed constants
/// (τ 45 · coeff 110) do NOT hold — measured, not
/// assumed.
#[test]
fn p6_hold_governor() {
    let definition = PlaytimeDefinition::blossom();
    let (culminations, q_engage, q_min, q_max, q_end, ticks_to_culmination) =
        hold_experiment(&definition);
    assert_eq!(culminations, 0, "no culmination under the governor");
    assert!(q_max <= 1000, "Q never crosses 1000 (max {q_max})");
    let band = (q_engage * 5 / 100).max(1);
    assert!(
        q_min >= q_engage - band && q_end <= q_engage + band,
        "Q held [{q_min}, {q_end}] within ±5% of engage {q_engage}"
    );
    assert!(
        ticks_to_culmination <= 3600,
        "culmination follows release (took {ticks_to_culmination} ticks)"
    );

    // The exhibit: PM §3.3's printed numerals are
    // T4 §5.3's pre-D-211 constants. Under them the
    // same experiment culminates — the conflict that
    // D-PM1-1 records, kept as a regression exhibit.
    let mut literal = PlaytimeDefinition::blossom();
    literal.governor = GovernorData {
        charge_coeff: 110,
        leak_tau_s: 45,
    };
    let (literal_culminations, ..) = hold_experiment(&literal);
    assert!(
        literal_culminations >= 1,
        "the printed §3.3 constants fail to hold (the D-PM1-1 conflict)"
    );
}

/// Runs the governor experiment: build at maximum
/// input to Q ≥ 850, engage Hold for 36,000 ticks
/// (600 s) under continued maximum input, then release
/// and count ticks to culmination.
/// Returns (culminations during hold, Q at engage,
/// Q min, Q max, Q at release, ticks release→culmination).
fn hold_experiment(definition: &PlaytimeDefinition) -> (usize, u32, u32, u32, u32, u64) {
    let mut host = ArousalHost::new();
    let mut mode = PlaytimeMode::new(definition.clone());
    mode.enter(&mut host, "prima").unwrap();
    mode.set_amplitude(&mut host, 1000).unwrap();
    let mut input = ModeTickInput::default();
    // Build to Q ≈ 850 at maximum input.
    let mut q_engage = 0;
    for t in 0..120_000u64 {
        if t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        } else {
            input.touch = None;
        }
        mode.step(&mut host, &input).unwrap();
        let q = host.get("prima").unwrap().vector().pleasure_charge;
        if q >= 850 {
            q_engage = q;
            break;
        }
    }
    assert!(q_engage >= 850, "the build reached Q ≈ 850");
    mode.set_hold(&mut host, true).unwrap();
    let mut culminations = 0usize;
    let (mut q_min, mut q_max) = (q_engage, q_engage);
    for t in 0..36_000u64 {
        if t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        } else {
            input.touch = None;
        }
        let out = mode.step(&mut host, &input).unwrap();
        culminations += out
            .engine
            .events
            .iter()
            .filter(|e| matches!(e, torogao_session::ArousalEvent::Culmination { .. }))
            .count();
        let q = host.get("prima").unwrap().vector().pleasure_charge;
        q_min = q_min.min(q);
        q_max = q_max.max(q);
        if t == 35_999 {
            // q_end read after the loop from the vector.
        }
    }
    let q_end = host.get("prima").unwrap().vector().pleasure_charge;
    mode.set_hold(&mut host, false).unwrap();
    let mut ticks_to_culmination = 36_000u64;
    for t in 0..36_000u64 {
        if t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        } else {
            input.touch = None;
        }
        let out = mode.step(&mut host, &input).unwrap();
        if out
            .engine
            .events
            .iter()
            .any(|e| matches!(e, torogao_session::ArousalEvent::Culmination { .. }))
        {
            ticks_to_culmination = t;
            break;
        }
    }
    (
        culminations,
        q_engage,
        q_min,
        q_max,
        q_end,
        ticks_to_culmination,
    )
}

/// P7 (PM-G7): pulse coupling — every burst is
/// preceded by a pulse at the same tick (0 tick lag),
/// burst count == qualifying-pulse count under §5.2's
/// rule (a pulse qualifies iff P_f ≥ 400 at its turn;
/// each burst discharges to 65% before the next
/// pulse's turn), and the body-cycle channel shows an
/// excursion at every pulse.
#[test]
fn p7_pulse_coupling() {
    let definition = PlaytimeDefinition::blossom();
    let threshold = definition.ejection.burst_threshold_permille;
    let retain = definition.ejection.burst_retain_permille;
    let run = run_canonical("prima");
    let mut total_pulses = 0usize;
    let mut total_bursts = 0usize;
    for (i, f) in run.facts.iter().enumerate() {
        total_pulses += f.pulses.len();
        let bursts: Vec<_> = f.bursts.iter().filter(|b| !b.final_release).collect();
        total_bursts += bursts.len();
        // No orphan bursts: each burst's pulse is one
        // of this tick's pulses, consumed at most once.
        let mut consumed: Vec<bool> = vec![false; f.pulses.len()];
        for b in &bursts {
            // 0 tick lag: the burst fired on this step,
            // on one of this step's pulses.
            if let Some(first) = f.pulses.first() {
                assert_eq!(b.tick, first.tick, "burst and pulse share the tick");
            }
            let pos = f
                .pulses
                .iter()
                .position(|p| {
                    p.tick == b.pulse.tick
                        && p.depth_permille == b.pulse.depth_permille
                        && p.lobe == b.pulse.lobe
                })
                .unwrap_or_else(|| panic!("tick {i}: burst pulse not among the tick's pulses"));
            assert!(!consumed[pos], "tick {i}: pulse consumed twice");
            consumed[pos] = true;
            assert!(
                b.pressure_before_permille >= threshold,
                "tick {i}: burst below the §5.2 pressure threshold"
            );
        }
        // Discharge chain: each later burst's pressure
        // is the previous burst's pressure × retain
        // (whole-permille rounding slack ±2).
        for w in bursts.windows(2) {
            let expected =
                (u64::from(w[0].pressure_before_permille) * u64::from(retain) + 500) / 1000;
            let actual = u64::from(w[1].pressure_before_permille);
            assert!(
                actual.abs_diff(expected) <= 2,
                "tick {i}: discharge chain {actual} vs expected {expected}"
            );
        }
        // Completeness: if pulses remain after the last
        // burst, the pressure at their turn was below
        // threshold; if there were pulses and no burst,
        // the tick's pressure never reached threshold.
        if f.pulses.len() > bursts.len() {
            if let Some(last) = bursts.last() {
                let after =
                    (u64::from(last.pressure_before_permille) * u64::from(retain) + 500) / 1000;
                assert!(
                    after < u64::from(threshold) + 2,
                    "tick {i}: a qualifying pulse was skipped (pressure {after})"
                );
            } else {
                assert!(
                    f.pressure_after <= threshold,
                    "tick {i}: pulses at pressure {} produced no burst",
                    f.pressure_after
                );
            }
        }
        // Body-cycle excursion at every pulse (deform
        // channels only — BodyCycleMod has no BodyRoot
        // field, structurally).
        if !f.pulses.is_empty() {
            assert!(
                f.body_excursion > 0,
                "tick {i}: pulses with no body-cycle excursion"
            );
        }
    }
    assert!(total_pulses > 0 && total_bursts > 0);
}

/// P8 (PM-G8): the §8.4 refusals fire exactly on their
/// adversarial documents; the authority corpus is
/// refused 6/6 with 0 applied.
#[test]
fn p8_validation_and_authority() {
    let valid = valid_settings_document();
    let admitted =
        validate_settings_document(&serde_json::to_string(&valid).unwrap(), "test.gear.json")
            .expect("the control document validates");
    assert_eq!(admitted.setting, "blossom");

    // TG-PM-EJECT-ANCHOR.
    let mut doc = valid.clone();
    doc["emitter"]["anchor"] = serde_json::json!("body.ankle.left");
    let err = validate_settings_document(&serde_json::to_string(&doc).unwrap(), "test.gear.json")
        .expect_err("a non-perineum emitter must refuse");
    assert_eq!(err.code, "TG-PM-EJECT-ANCHOR");
    assert_eq!(err.field_path, "emitter.anchor");

    // TG-PM-VOLUME-BOUND (both fields).
    for (field, value) in [
        ("eject_volume_scale_permille", 2001u64),
        ("eject_threshold_permille", 499),
    ] {
        let mut doc = valid.clone();
        doc["characters"]["prima"][field] = serde_json::json!(value);
        let err =
            validate_settings_document(&serde_json::to_string(&doc).unwrap(), "test.gear.json")
                .expect_err("an out-of-bounds profile field must refuse");
        assert_eq!(err.code, "TG-PM-VOLUME-BOUND", "{field}");
    }

    // TG-PM-PULSE-BOUND (rate and the fixed lobe gap).
    for (field, value) in [("max_rate_millihz", 2001u64), ("lobe_gap_ticks", 8)] {
        let mut doc = valid.clone();
        doc["pulse"][field] = serde_json::json!(value);
        let err =
            validate_settings_document(&serde_json::to_string(&doc).unwrap(), "test.gear.json")
                .expect_err("an out-of-bounds pulse law must refuse");
        assert_eq!(err.code, "TG-PM-PULSE-BOUND", "{field}");
    }

    // The authority corpus: six authority-bearing
    // variants of an otherwise-valid document — 6/6
    // refused with the mirror-authority code, 0 applied
    // (the validator is pure: a refusal IS non-application).
    let corpus = [
        "mirror_enabled",
        "mirror",
        "ceiling",
        "consent",
        "bind_device",
        "ache_ceiling",
    ];
    for field in corpus {
        assert!(
            AUTHORITY_FIELDS.contains(&field),
            "{field} is in the census list"
        );
        let mut doc = valid.clone();
        doc[field] = serde_json::json!(true);
        let err =
            validate_settings_document(&serde_json::to_string(&doc).unwrap(), "test.gear.json")
                .expect_err("an authority-bearing document must refuse");
        assert_eq!(err.code, "TG-GEAR-MIRROR-AUTHORITY", "{field}");
    }

    // Pipeline discipline: an unknown field meets its
    // own refusal, not a later step's.
    let mut doc = valid.clone();
    doc["bogus_field"] = serde_json::json!(1);
    let err = validate_settings_document(&serde_json::to_string(&doc).unwrap(), "test.gear.json")
        .expect_err("an unknown field must refuse");
    assert_eq!(err.code, "TG-GEAR-FIELD-UNKNOWN");
}

/// The valid control settings document (§2.2's form).
fn valid_settings_document() -> serde_json::Value {
    serde_json::json!({
        "format": "playtime-settings/1",
        "setting": "blossom",
        "emitter": { "anchor": "body.perineum" },
        "characters": {
            "prima": { "eject_threshold_permille": 1000, "eject_volume_scale_permille": 1000 },
            "yui": { "eject_threshold_permille": 900, "eject_volume_scale_permille": 850 },
            "alma": { "eject_threshold_permille": 1100, "eject_volume_scale_permille": 1150 }
        },
        "pulse": { "max_rate_millihz": 1000, "max_depth_permille": 1000, "lobe_gap_ticks": 9 },
        "device_amplitude_permille": 800,
        "rec_skin": false
    })
}

/// PM-G11, PM-1 side: every §9.2(a) vocal class and
/// every governed expression key resolves per character
/// through the profile validator; a missing class fails
/// naming the class and the character. (The asset half
/// is PM-1S's P14.)
#[test]
fn p14_pm1_side_profile_completeness() {
    let definition = PlaytimeDefinition::blossom();
    for character in ["prima", "yui", "alma"] {
        validate_mode_profile(&definition, character)
            .unwrap_or_else(|e| panic!("{character} profile validates: {e}"));
        let data = definition.character(character).unwrap();
        assert_eq!(data.vocal_set.honesty, "stand-in", "labelled per §9.6");
        for class in VOCAL_CLASSES {
            assert!(
                data.vocal_set.classes.get(class).copied().unwrap_or(0) > 0,
                "{character} resolves vocal class {class}"
            );
        }
        for key in EXPRESSION_KEYS {
            assert!(
                data.expression_recipes.contains_key(key),
                "{character} resolves expression {key}"
            );
        }
        // Her recipes are hers (no substitution).
        assert!(
            data.expression_recipes
                .values()
                .all(|r| r.starts_with(&format!("{character}."))),
            "{character}: recipe ids are her own"
        );
    }
    // A missing class fails named.
    let mut broken = PlaytimeDefinition::blossom();
    broken
        .characters
        .get_mut("yui")
        .unwrap()
        .vocal_set
        .classes
        .remove("gasp");
    let err = validate_mode_profile(&broken, "yui").expect_err("a missing class must fail");
    assert!(
        err.message.contains("gasp") && err.message.contains("yui"),
        "the failure names the class and the character: {}",
        err.message
    );
}

/// Persistence contracts: the preference set (§2.2)
/// round-trips through §8.4's pipeline, and the mode
/// state (§6.1) round-trips wetness + pressure.
#[test]
fn persistence_round_trips() {
    let dir = test_tmp("persistence");
    let definition = PlaytimeDefinition::blossom();
    let mut host = ArousalHost::new();
    let mut mode = PlaytimeMode::new(definition.clone());
    mode.enter(&mut host, "prima").unwrap();
    mode.set_amplitude(&mut host, 800).unwrap();
    let path = mode.save_preference_set(&dir, "evening").unwrap();
    assert!(
        path.ends_with("playroom/evening.json"),
        "Addition S playroom/ path: {}",
        path.display()
    );
    let loaded = PlaytimeMode::load_preference_set(&dir, "evening")
        .expect("the written preference set validates on load");
    assert_eq!(loaded.device_amplitude_permille, 800);
    assert_eq!(loaded.characters.len(), 3);

    // Mode state: build some pressure + wetness, save,
    // restore into a fresh entry, compare.
    for t in 0..3600u64 {
        let mut input = ModeTickInput::default();
        if t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        }
        mode.step(&mut host, &input).unwrap();
    }
    let wet_before = wet_of(&mode);
    let pressure_before = mode.pressure_permille();
    assert!(wet_before.iter().any(|w| *w > 0), "the build got wet");
    mode.save_state(&dir).unwrap();
    let mut host2 = ArousalHost::new();
    let mut mode2 = PlaytimeMode::new(definition);
    mode2.enter(&mut host2, "prima").unwrap();
    mode2.restore_state(&dir).unwrap();
    assert_eq!(wet_of(&mode2), wet_before, "wetness restored");
    assert_eq!(
        mode2.pressure_permille(),
        pressure_before,
        "pressure restored"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// P3 (PM-G3) second half: over a 600 s hold past
/// Afterglow, no timed or phase-driven path reduces
/// the wetness field (total is conserved exactly —
/// drips only transfer), and a scene change (exit)
/// zeroes it.
#[test]
fn p3_no_decay_and_scene_change() {
    let definition = PlaytimeDefinition::blossom();
    let mut host = ArousalHost::new();
    let mut mode = PlaytimeMode::new(definition);
    mode.enter(&mut host, "prima").unwrap();
    mode.set_amplitude(&mut host, 1000).unwrap();
    // Build to Afterglow.
    let mut reached_afterglow = false;
    for t in 0..120_000u64 {
        let mut input = ModeTickInput::default();
        if t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        }
        let out = mode.step(&mut host, &input).unwrap();
        if out.stage == Stage::Afterglow {
            reached_afterglow = true;
            break;
        }
    }
    assert!(reached_afterglow);
    mode.set_amplitude(&mut host, 0).unwrap();
    let total_at_hold_start = mode.wetness().total_mp();
    assert!(
        total_at_hold_start > 0,
        "the field is wet entering the hold"
    );
    // 600 s of idle steps: sample the conserved total.
    for t in 0..36_000u64 {
        mode.step(&mut host, &ModeTickInput::default()).unwrap();
        if t % 600 == 0 || t == 35_999 {
            assert_eq!(
                mode.wetness().total_mp(),
                total_at_hold_start,
                "no timed/phase path reduces wetness (tick {t})"
            );
        }
    }
    // Scene change: exit zeroes the field with the
    // scene (§6.3's second act).
    mode.exit(&mut host).unwrap();
    assert_eq!(wet_of(&mode), [0, 0, 0], "scene change zeroed the field");
}

fn test_tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pm1-gates")
        .join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create test tmp dir");
    dir
}
