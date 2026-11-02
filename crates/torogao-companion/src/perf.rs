//! The performance instruments the Stage 1.1 gates read:
//! the evaluated-parameter trace of a fixed Daydream run
//! through the companion core, its determinism hash, the
//! loop-seam ratio (the Phase D `measure_motion.py` law,
//! re-implemented over the same parameter domain), and the
//! motion-on-every-frame fraction.
//!
//! The trace hash is a chain: FNV-1a over the per-step
//! evaluated state hashes in order, so two runs agree only
//! if every step agreed — a stronger statement than
//! comparing final hashes, and the one Gate G6 makes.

use std::path::Path;

use crate::core::{AMBIENT_ANIMATION, CompanionCore};
use crate::error::{CompanionError, CompanionResult};

/// FNV-1a offset basis (64-bit), the estate's chain hash.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime (64-bit).
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One fixed trace run through the companion core.
#[derive(Debug, Clone)]
pub struct TraceRun {
    /// Per-step evaluated state hashes, in order.
    pub step_hashes: Vec<u64>,
    /// Per-step parameter vectors `(x, y)` in payload
    /// order (the measure_motion domain).
    pub params: Vec<Vec<[f32; 2]>>,
    /// The chain hash over `step_hashes`.
    pub trace_hash: u64,
    /// How many of the steps changed the evaluated state
    /// versus the previous step.
    pub moving_steps: usize,
    /// Step indices at which the playhead wrapped (the
    /// loop seam steps): the step whose delta closes the
    /// loop.
    pub seam_steps: Vec<usize>,
}

/// One chain step: fold one step hash into the running
/// chain state. `chain_hash` over a slice is exactly the
/// fold of this over the slice from the offset basis, so
/// the live presentation (which cannot keep every hash)
/// and the instrumented trace (which can) produce the
/// same number for the same performance.
pub fn chain_step(state: u64, hash: u64) -> u64 {
    let mut state = state;
    for byte in hash.to_le_bytes() {
        state ^= u64::from(byte);
        state = state.wrapping_mul(FNV_PRIME);
    }
    state
}

/// Chains per-step hashes into one trace hash.
pub fn chain_hash(step_hashes: &[u64]) -> u64 {
    step_hashes
        .iter()
        .fold(FNV_OFFSET_BASIS, |state, &hash| chain_step(state, hash))
}

/// Runs `frames` fixed steps of the ambient performance
/// through a fresh companion core (no session server: the
/// trace is the presented performance's own evaluation,
/// and serving must not perturb it).
pub fn run_trace(model_path: &Path, frames: usize) -> CompanionResult<TraceRun> {
    let mut core = CompanionCore::load(model_path, None, None, "torogao-companion trace")?;
    let mut step_hashes = Vec::with_capacity(frames);
    let mut params: Vec<Vec<[f32; 2]>> = Vec::with_capacity(frames);
    let mut seam_steps = Vec::new();
    let mut previous_hframe: Option<f32> = None;
    let mut moving_steps = 0_usize;
    let mut previous_hash: Option<u64> = None;
    for _ in 0..frames {
        let hash = core.tick()?;
        if let Some(previous) = previous_hash {
            if previous != hash {
                moving_steps += 1;
            }
        }
        previous_hash = Some(hash);
        step_hashes.push(hash);
        let scene = core.scene()?;
        params.push(
            scene
                .puppet
                .params
                .iter()
                .map(|param| [param.last_internal.x, param.last_internal.y])
                .collect(),
        );
        let snapshot = core.play_snapshot()?;
        if snapshot.animation.as_deref() == Some(AMBIENT_ANIMATION) {
            if let Some(previous) = previous_hframe {
                if snapshot.hframe < previous - 0.5 {
                    // The playhead wrapped: this step's
                    // delta (params[len-1] vs params[len-2])
                    // is a seam delta.
                    seam_steps.push(step_hashes.len() - 1);
                }
            }
            previous_hframe = Some(snapshot.hframe);
        }
    }
    Ok(TraceRun {
        trace_hash: chain_hash(&step_hashes),
        step_hashes,
        params,
        moving_steps,
        seam_steps,
    })
}

/// The mean absolute per-step parameter delta between two
/// parameter vectors (the MAD of measure_motion.py: mean
/// over parameters of the L1 delta across the [x, y] pair).
pub fn param_delta(a: &[[f32; 2]], b: &[[f32; 2]]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| f64::from((x[0] - y[0]).abs() + (x[1] - y[1]).abs()))
        .sum();
    sum / a.len() as f64
}

/// The loop-seam report over one trace, measure_motion's
/// law: the seam ratio is the seam step's delta over the
/// median per-step delta (1.0 = the seam is a typical step;
/// Gate G5's bound is 1.1).
#[derive(Debug, Clone)]
pub struct SeamReport {
    /// Median per-step delta.
    pub median_delta: f64,
    /// The seam deltas, one per wrap.
    pub seam_deltas: Vec<f64>,
    /// Worst seam ratio (max seam delta / median).
    pub seam_ratio_max: f64,
}

/// Computes the seam report for a trace.
pub fn seam_report(trace: &TraceRun) -> SeamReport {
    let mut deltas: Vec<f64> = Vec::with_capacity(trace.params.len().saturating_sub(1));
    for pair in trace.params.windows(2) {
        deltas.push(param_delta(&pair[0], &pair[1]));
    }
    let median_delta = median(&mut deltas.clone());
    let seam_deltas: Vec<f64> = trace
        .seam_steps
        .iter()
        .filter(|&&step| step >= 1 && step < trace.params.len())
        .map(|&step| param_delta(&trace.params[step - 1], &trace.params[step]))
        .collect();
    let seam_ratio_max = if median_delta > 0.0 {
        seam_deltas.iter().copied().fold(0.0_f64, f64::max) / median_delta
    } else {
        0.0
    };
    SeamReport {
        median_delta,
        seam_deltas,
        seam_ratio_max,
    }
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

// ---------------------------------------------------------------------------
// Stage 1.2 instruments: the scripted-input trace. A fixed
// script of pointer positions (surface fractions) and click
// steps drives the core headlessly; the run records the
// per-step evaluated hash, the gaze driver's evaluated
// direction, and the playing animation — the three series
// the 1.2 gates read (gaze accuracy over a scripted cursor
// path, pointer-event → first-changed-frame latency, and
// fixed-script determinism ×3).
// ---------------------------------------------------------------------------

/// One scripted-input run through the companion core.
#[derive(Debug, Clone)]
pub struct ScriptRun {
    /// Per-step evaluated state hashes, in order.
    pub step_hashes: Vec<u64>,
    /// The chain hash over `step_hashes`.
    pub trace_hash: u64,
    /// Per-step evaluated gaze direction (yaw, pitch),
    /// degrees, read after the step.
    pub gaze: Vec<(f64, f64)>,
    /// Per-step evaluated `ParamAngleX` internal value
    /// (degrees): the head channel as it landed in the
    /// evaluation, performance sway included — the
    /// plumbing instrument differences two runs to
    /// isolate the driver's contribution.
    pub angle_x_internal: Vec<f32>,
    /// Per-step playing animation name, when one is
    /// loaded.
    pub animations: Vec<Option<String>>,
}

/// Runs `frames` fixed steps under a fixed input script:
/// at step `i` the pointer is set to `pointer[i]` (surface
/// fractions; the slice repeats its last entry when the
/// script is shorter than the run) at the given surface
/// aspect, and a click reaction fires before the step
/// when `clicks` contains `i`. The script is data, so a
/// gate can replay it verbatim.
pub fn run_scripted_trace(
    model_path: &Path,
    frames: usize,
    pointer: &[Option<(f64, f64)>],
    clicks: &std::collections::BTreeSet<usize>,
    surface_aspect: f64,
) -> CompanionResult<ScriptRun> {
    let mut core = CompanionCore::load(model_path, None, None, "torogao-companion script")?;
    let angle_x = core.scene()?.puppet.param_index("ParamAngleX");
    let mut step_hashes = Vec::with_capacity(frames);
    let mut gaze = Vec::with_capacity(frames);
    let mut angle_x_internal = Vec::with_capacity(frames);
    let mut animations = Vec::with_capacity(frames);
    for step in 0..frames {
        let position = if pointer.is_empty() {
            None
        } else {
            pointer[step.min(pointer.len() - 1)]
        };
        core.set_pointer(position, surface_aspect);
        if clicks.contains(&step) {
            core.click_reaction()?;
        }
        step_hashes.push(core.tick()?);
        gaze.push(core.gaze_degrees());
        angle_x_internal.push(match angle_x {
            Some(index) => core.scene()?.puppet.params[index].last_internal.x,
            None => 0.0,
        });
        animations.push(core.play_snapshot()?.animation);
    }
    Ok(ScriptRun {
        trace_hash: chain_hash(&step_hashes),
        step_hashes,
        gaze,
        angle_x_internal,
        animations,
    })
}

// ---------------------------------------------------------------------------
// Stage 2.1 instrument: the body-cycle performance trace.
// A CycleDriver performance runs through the core under
// the ambient Daydream; the run records the evaluated
// parameter vectors (for the regional-MAD series), the
// primary and secondary channels' evaluated internals,
// and the authored envelope — the four series the 2.1
// gates read.
// ---------------------------------------------------------------------------

/// One body-cycle performance run through the companion
/// core.
#[derive(Debug, Clone)]
pub struct CycleRun {
    /// The chain hash over the per-step evaluated hashes.
    pub trace_hash: u64,
    /// Per-step parameter vectors `(x, y)` in payload
    /// order (the regional-MAD domain).
    pub params: Vec<Vec<[f32; 2]>>,
    /// Per-step evaluated ParamBodyAngleZ internal
    /// (degrees): the primary as it landed.
    pub primary: Vec<f64>,
    /// Per-step evaluated ParamBodyAngleX internal
    /// (degrees): the authored secondary as it landed.
    pub secondary: Vec<f64>,
    /// Per-step evaluated ParamHairSway1 internal: the
    /// rig's physics secondary, recorded so the gate can
    /// report what the rig data itself does.
    pub physics_secondary: Vec<f64>,
    /// Per-step authored envelope (normalized).
    pub envelope: Vec<f64>,
    /// The body region's parameter indices (regional MAD).
    pub body_region: Vec<usize>,
    /// The secondary region's parameter indices
    /// (regional MAD).
    pub secondary_region: Vec<usize>,
    /// The hair region's parameter indices (regional MAD).
    pub hair_region: Vec<usize>,
}

/// Runs a body-cycle performance of `frames` fixed steps
/// on the given rig through the companion core.
pub fn run_cycle_trace(model_path: &Path, frames: usize) -> CompanionResult<CycleRun> {
    let mut core = CompanionCore::load(model_path, None, None, "torogao-companion cycle")?;
    let (body_x, body_z, hair_1, body_region, secondary_region, hair_region) = {
        let scene = core.scene()?;
        let body_x = scene.puppet.param_index("ParamBodyAngleX");
        let body_z = scene.puppet.param_index("ParamBodyAngleZ");
        let hair_1 = scene.puppet.param_index("ParamHairSway1");
        let hair_2 = scene.puppet.param_index("ParamHairSway2");
        (
            body_x,
            body_z,
            hair_1,
            body_z.into_iter().collect(),
            body_x.into_iter().collect(),
            hair_1.into_iter().chain(hair_2).collect(),
        )
    };
    core.start_cycle(frames as u64);
    let mut step_hashes = Vec::with_capacity(frames);
    let mut params = Vec::with_capacity(frames);
    let mut primary = Vec::with_capacity(frames);
    let mut secondary = Vec::with_capacity(frames);
    let mut physics_secondary = Vec::with_capacity(frames);
    let mut envelope = Vec::with_capacity(frames);
    for step in 0..frames {
        envelope.push(core.cycle_envelope_at(step as u64).unwrap_or(0.0));
        step_hashes.push(core.tick()?);
        let scene = core.scene()?;
        params.push(
            scene
                .puppet
                .params
                .iter()
                .map(|param| [param.last_internal.x, param.last_internal.y])
                .collect(),
        );
        let read = |index: Option<usize>| match index {
            Some(index) => f64::from(scene.puppet.params[index].last_internal.x),
            None => 0.0,
        };
        primary.push(read(body_z));
        secondary.push(read(body_x));
        physics_secondary.push(read(hair_1));
    }
    Ok(CycleRun {
        trace_hash: chain_hash(&step_hashes),
        params,
        primary,
        secondary,
        physics_secondary,
        envelope,
        body_region,
        secondary_region,
        hair_region,
    })
}

// ---------------------------------------------------------------------------
// Stage 2.2 instrument: the roll performance trace. The
// authored R-curve performance runs through the core
// (the gaze driver live throughout: saccades and blink
// discipline compose with the roll); the run records
// the evaluated driver channels per step and the face
// geometry reads every `FACE_READ_EVERY` steps — the
// series the 2.2 gates read.
// ---------------------------------------------------------------------------

/// Face reads are taken every this many steps.
pub const FACE_READ_EVERY: usize = 5;

/// One roll performance run through the companion core.
#[derive(Debug, Clone)]
pub struct RollRun {
    /// The chain hash over the per-step evaluated hashes.
    pub trace_hash: u64,
    /// Per-step evaluated state hashes.
    pub step_hashes: Vec<u64>,
    /// Per-step evaluated ParamEyeRoll internal.
    pub roll: Vec<f64>,
    /// Per-step evaluated ParamEyeCross internal.
    pub cross: Vec<f64>,
    /// Per-step evaluated ParamEyeLOpen internal.
    pub eye_open: Vec<f64>,
    /// Face reads: (step, read, eye-open at that step).
    pub face_reads: Vec<(usize, crate::face::FaceRead, f64)>,
}

/// Runs the roll performance plus an afterglow
/// observation window of `afterglow_steps` through the
/// companion core on the given rig.
pub fn run_roll_trace(model_path: &Path, afterglow_steps: usize) -> CompanionResult<RollRun> {
    let mut core = CompanionCore::load(model_path, None, None, "torogao-companion roll")?;
    let (roll_index, cross_index, open_index) = {
        let scene = core.scene()?;
        (
            scene.puppet.param_index("ParamEyeRoll"),
            scene.puppet.param_index("ParamEyeCross"),
            scene.puppet.param_index("ParamEyeLOpen"),
        )
    };
    core.start_roll();
    let frames = crate::roll::ROLL_STEPS as usize + afterglow_steps;
    let mut step_hashes = Vec::with_capacity(frames);
    let mut roll = Vec::with_capacity(frames);
    let mut cross = Vec::with_capacity(frames);
    let mut eye_open = Vec::with_capacity(frames);
    let mut face_reads = Vec::new();
    for step in 0..frames {
        step_hashes.push(core.tick()?);
        {
            let scene = core.scene()?;
            let read = |index: Option<usize>| match index {
                Some(index) => f64::from(scene.puppet.params[index].last_internal.x),
                None => 0.0,
            };
            roll.push(read(roll_index));
            cross.push(read(cross_index));
            eye_open.push(read(open_index));
        }
        if step % FACE_READ_EVERY == 0 {
            let scene = core.scene_mut()?;
            let frame = torogao_app::render::prepare_frame(scene)
                .map_err(|err| CompanionError::model(err.to_string()))?;
            if let Some(read) = crate::face::read_face(&frame) {
                face_reads.push((step, read, eye_open[step]));
            }
        }
    }
    Ok(RollRun {
        trace_hash: chain_hash(&step_hashes),
        step_hashes,
        roll,
        cross,
        eye_open,
        face_reads,
    })
}

// ---------------------------------------------------------------------------
// Stage 2.3 instrument: the duo take. Prima (A) and Yui
// (B) share one clock through the authored turn-taking
// script:
//
// - steps [0, 60): settle, both gaze forward;
// - step 60: A starts her roll; at step 90 B reacts —
//   her gaze turns to A (the authored reaction offset);
// - step 990: B starts her roll and her gaze returns
//   forward (she performs); at step 1020 A reacts —
//   her gaze turns to B;
// - the take ends at step 1980 (B's afterglow tail).
//
// Every step composes the stage frame and runs the
// census; face reads land every 5 steps; the per-step
// combined hash chains both partners' evaluated states.
// ---------------------------------------------------------------------------

/// Settle steps before the first performance.
pub const DUO_SETTLE_STEPS: usize = 60;
/// The authored reaction offset: the watcher turns to
/// the performer this many steps after her roll starts.
pub const DUO_REACTION_OFFSET: usize = 30;
/// Rest steps between the two performances.
pub const DUO_GAP_STEPS: usize = 60;
/// Afterglow observation steps after the last roll.
pub const DUO_TAIL_STEPS: usize = 120;

/// The step A starts her roll.
pub const DUO_A_ROLL_START: usize = DUO_SETTLE_STEPS;
/// The step B reacts to A.
pub const DUO_B_REACT: usize = DUO_A_ROLL_START + DUO_REACTION_OFFSET;
/// The step B starts her roll.
pub const DUO_B_ROLL_START: usize =
    DUO_A_ROLL_START + crate::roll::ROLL_STEPS as usize + DUO_GAP_STEPS;
/// The step A reacts to B.
pub const DUO_A_REACT: usize = DUO_B_ROLL_START + DUO_REACTION_OFFSET;
/// The full take length.
pub const DUO_TOTAL_STEPS: usize =
    DUO_B_ROLL_START + crate::roll::ROLL_STEPS as usize + DUO_TAIL_STEPS;

/// The surface aspect both partners convert gaze with
/// in the duo instrument (the companion portrait).
pub const DUO_ASPECT: f64 = 0.5;

/// One duo take.
#[derive(Debug, Clone)]
pub struct DuoRun {
    /// The chain hash over the per-step combined hashes.
    pub trace_hash: u64,
    /// Per-step combined hashes.
    pub step_hashes: Vec<u64>,
    /// Total draw-order pops across the take.
    pub pops: usize,
    /// Total clipping events across the take.
    pub clipping: usize,
    /// Per-step evaluated ParamEyeRoll, partner A.
    pub roll_a: Vec<f64>,
    /// Per-step evaluated ParamEyeRoll, partner B.
    pub roll_b: Vec<f64>,
    /// Per-step evaluated ParamAngleX, partner A.
    pub yaw_a: Vec<f64>,
    /// Per-step evaluated ParamAngleX, partner B.
    pub yaw_b: Vec<f64>,
    /// Face reads every 5 steps: (step, A, B).
    pub face_reads: Vec<(usize, crate::face::FaceRead, crate::face::FaceRead)>,
    /// The stage layout the take composed under.
    pub layout: crate::duo::StageLayout,
    /// Partner A's canvas height (world units).
    pub canvas_height_a: f64,
    /// Partner B's canvas height (world units).
    pub canvas_height_b: f64,
}

/// Runs the duo take on the two given rigs.
pub fn run_duo_trace(model_a: &Path, model_b: &Path) -> CompanionResult<DuoRun> {
    let mut duo = crate::duo::DuoCore::load(model_a, model_b, "torogao-companion duo")?;
    let (layout, canvas_height_a, canvas_height_b) = {
        let frame_a = torogao_app::render::prepare_frame(duo.a.scene_mut()?)
            .map_err(|err| CompanionError::model(err.to_string()))?;
        let frame_b = torogao_app::render::prepare_frame(duo.b.scene_mut()?)
            .map_err(|err| CompanionError::model(err.to_string()))?;
        (
            crate::duo::layout(&frame_a, &frame_b),
            f64::from(frame_a.canvas.height),
            f64::from(frame_b.canvas.height),
        )
    };
    let (roll_a_index, yaw_a_index) = {
        let scene = duo.a.scene()?;
        (
            scene.puppet.param_index("ParamEyeRoll"),
            scene.puppet.param_index("ParamAngleX"),
        )
    };
    let (roll_b_index, yaw_b_index) = {
        let scene = duo.b.scene()?;
        (
            scene.puppet.param_index("ParamEyeRoll"),
            scene.puppet.param_index("ParamAngleX"),
        )
    };
    let read = |scene: &torogao_app::scene::Scene, index: Option<usize>| match index {
        Some(index) => f64::from(scene.puppet.params[index].last_internal.x),
        None => 0.0,
    };
    let mut step_hashes = Vec::with_capacity(DUO_TOTAL_STEPS);
    let mut roll_a = Vec::with_capacity(DUO_TOTAL_STEPS);
    let mut roll_b = Vec::with_capacity(DUO_TOTAL_STEPS);
    let mut yaw_a = Vec::with_capacity(DUO_TOTAL_STEPS);
    let mut yaw_b = Vec::with_capacity(DUO_TOTAL_STEPS);
    let mut face_reads = Vec::new();
    let mut pops = 0;
    let mut clipping = 0;
    let mut previous: Option<crate::duo::ComposedFrame> = None;
    let mut pointer_a: Option<(f64, f64)> = None;
    let mut pointer_b: Option<(f64, f64)> = None;
    for step in 0..DUO_TOTAL_STEPS {
        match step {
            DUO_A_ROLL_START => duo.a.start_roll(),
            DUO_B_REACT => {
                pointer_b = Some(crate::duo::partner_pointer(
                    layout.face_b,
                    layout.face_a,
                    canvas_height_b,
                    DUO_ASPECT,
                ));
            }
            DUO_B_ROLL_START => {
                duo.b.start_roll();
                pointer_b = None;
            }
            DUO_A_REACT => {
                pointer_a = Some(crate::duo::partner_pointer(
                    layout.face_a,
                    layout.face_b,
                    canvas_height_a,
                    DUO_ASPECT,
                ));
            }
            _ => {}
        }
        duo.a.set_pointer(pointer_a, DUO_ASPECT);
        duo.b.set_pointer(pointer_b, DUO_ASPECT);
        step_hashes.push(duo.tick()?);
        roll_a.push(read(duo.a.scene()?, roll_a_index));
        yaw_a.push(read(duo.a.scene()?, yaw_a_index));
        roll_b.push(read(duo.b.scene()?, roll_b_index));
        yaw_b.push(read(duo.b.scene()?, yaw_b_index));
        let frame_a = torogao_app::render::prepare_frame(duo.a.scene_mut()?)
            .map_err(|err| CompanionError::model(err.to_string()))?;
        let frame_b = torogao_app::render::prepare_frame(duo.b.scene_mut()?)
            .map_err(|err| CompanionError::model(err.to_string()))?;
        let composed = crate::duo::compose(&frame_a, &frame_b, &layout);
        let (frame_pops, frame_clipping) = crate::duo::census(previous.as_ref(), &composed);
        pops += frame_pops;
        clipping += frame_clipping;
        previous = Some(composed);
        if step % FACE_READ_EVERY == 0 {
            if let (Some(read_a), Some(read_b)) = (
                crate::face::read_face(&frame_a),
                crate::face::read_face(&frame_b),
            ) {
                face_reads.push((step, read_a, read_b));
            }
        }
    }
    Ok(DuoRun {
        trace_hash: chain_hash(&step_hashes),
        step_hashes,
        pops,
        clipping,
        roll_a,
        roll_b,
        yaw_a,
        yaw_b,
        face_reads,
        layout,
        canvas_height_a,
        canvas_height_b,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_hash_is_order_sensitive_and_deterministic() {
        let a = chain_hash(&[1, 2, 3]);
        assert_eq!(a, chain_hash(&[1, 2, 3]));
        assert_ne!(a, chain_hash(&[3, 2, 1]));
        assert_eq!(chain_hash(&[]), FNV_OFFSET_BASIS);
    }

    #[test]
    fn param_delta_is_l1_mean() {
        let a = vec![[0.0, 0.0], [2.0, 0.0]];
        let b = vec![[1.0, 0.0], [2.0, 3.0]];
        assert!((param_delta(&a, &b) - 2.0).abs() < 1e-12);
        assert_eq!(param_delta(&a, &a), 0.0);
    }

    #[test]
    fn median_handles_even_and_odd_counts() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&mut []), 0.0);
    }
}
