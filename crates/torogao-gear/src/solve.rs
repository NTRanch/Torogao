//! The attachment solve and contact model (§§4.2–4.5).
//!
//! Each attachment is a critically damped spring toward its
//! body anchor, integrated in 4 bounded substeps per fixed
//! step, followed by a hard projection onto the anchor. The
//! DRIFT BOUND the gate measures is the post-solve residual on
//! the 1024 canvas: **≤ 2.0 px on every step**, with motion
//! lag **≤ 1 fixed step**. Because the rendered position is
//! the projected position, both hold by the construction §4.2
//! specifies — the report says so plainly rather than dressing
//! the result up as an empirical surprise. The pre-projection
//! spring error is the diagnostic that drives displacement
//! (§4.5): sustained error past the item's threshold for 30
//! consecutive steps latches `displaced`, exactly once, as an
//! op-log event; the solve never silently re-seats gear.

use crate::document::{Attachment, StateVariant};

/// The drift bound: post-solve residual, pixels at the 1024
/// convention (§4.2).
pub const DRIFT_BOUND_PX: f64 = 2.0;
/// The lag bound: fixed steps at 60 Hz (§4.2).
pub const LAG_BOUND_STEPS: u32 = 1;
/// Fixed-step rate of the T4 model (§4.1).
pub const STEPS_PER_SECOND: u32 = 60;
/// Solver substeps per fixed step (§4.2: bounded 4 iterations).
pub const SOLVE_ITERATIONS: u32 = 4;
/// Consecutive over-threshold steps before displacement
/// latches (§4.5: 30 steps = 0.5 s).
pub const DISPLACEMENT_STEPS: u32 = 30;

/// Named anchor-motion tracks the preview and gates run over.
/// `Greeting` and `Daydream` are the repertoire's motion
/// classes (§2.9 step 3); `StressJump` is an authored
/// adversarial track: a step displacement of the anchor that
/// exists to drive the §4.5 displacement latch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionTrack {
    /// Slow idle drift.
    Daydream,
    /// Gentle greeting sway.
    Greeting,
    /// A 160 px anchor step at tick 300, held to tick 360.
    StressJump,
}

/// The anchor position of one site under a motion track at a
/// tick. Pure and deterministic: the same tick always yields
/// the same position.
pub fn anchor_position(
    track: MotionTrack,
    base_x_px: f64,
    base_y_px: f64,
    tick: u64,
) -> (f64, f64) {
    let t = tick as f64;
    match track {
        MotionTrack::Daydream => (
            base_x_px + 10.0 * (t * std::f64::consts::TAU / 600.0).sin(),
            base_y_px + 8.0 * (t * std::f64::consts::TAU / 480.0).cos(),
        ),
        MotionTrack::Greeting => (
            base_x_px + 6.0 * (t * std::f64::consts::TAU / 240.0).sin(),
            base_y_px + 4.0 * (t * std::f64::consts::TAU / 180.0 + 1.0).sin(),
        ),
        MotionTrack::StressJump => {
            if (300..360).contains(&tick) {
                (base_x_px + 160.0, base_y_px)
            } else {
                (base_x_px, base_y_px)
            }
        }
    }
}

/// The state of one attachment's solver.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentSolver {
    /// Spring-integrated (pre-projection) position.
    pub spring_x_px: f64,
    /// Spring-integrated (pre-projection) position.
    pub spring_y_px: f64,
    spring_vx: f64,
    spring_vy: f64,
    /// Consecutive over-threshold steps (§4.5 counter).
    pub over_threshold_steps: u32,
    /// Whether the displacement latch has fired.
    pub displaced: bool,
}

/// The outcome of one solver step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepOutcome {
    /// Post-projection position x (the rendered position).
    pub x_px: f64,
    /// Post-projection position y.
    pub y_px: f64,
    /// Post-solve residual distance to the anchor, px.
    pub residual_px: f64,
    /// Pre-projection spring error, px (diagnostic, §4.2).
    pub pre_projection_error_px: f64,
    /// Whether this step latched displacement (§4.5).
    pub displaced_event: bool,
}

/// Spring constant mapping (§4.2): stiffness permille maps
/// monotonically onto the spring constant. Solver data,
/// versioned with the format (`torogao-gear/1`).
pub fn spring_constant(stiffness_permille: i64) -> f64 {
    30.0 + 570.0 * (stiffness_permille.clamp(0, 1000) as f64 / 1000.0)
}

impl AttachmentSolver {
    /// A solver starting at rest on the anchor.
    pub fn new(x_px: f64, y_px: f64) -> Self {
        Self {
            spring_x_px: x_px,
            spring_y_px: y_px,
            spring_vx: 0.0,
            spring_vy: 0.0,
            over_threshold_steps: 0,
            displaced: false,
        }
    }

    /// Advances one fixed step toward the anchor target,
    /// using the attachment's contact radius as the
    /// displacement threshold. Callers with the document's
    /// physics threshold use [`step_with_threshold`].
    pub fn step(
        &mut self,
        target_x_px: f64,
        target_y_px: f64,
        attachment: &Attachment,
    ) -> StepOutcome {
        step_with_threshold(
            self,
            target_x_px,
            target_y_px,
            attachment,
            attachment.contact_radius_px as f64,
        )
    }
}

/// The result of running one attachment over a motion track.
#[derive(Clone, Debug, PartialEq)]
pub struct SolveTrace {
    /// Steps simulated.
    pub ticks: u64,
    /// Maximum post-solve residual over the run, px.
    pub max_residual_px: f64,
    /// Steps whose residual exceeded the drift bound.
    pub ticks_over_bound: u64,
    /// Maximum pre-projection spring error, px.
    pub max_pre_projection_error_px: f64,
    /// Measured lag in fixed steps (crossing-based; the
    /// projected position tracks the anchor, so a correct
    /// solve measures 0).
    pub lag_steps: u32,
    /// Ticks at which displacement latched (§4.5).
    pub displacement_ticks: Vec<u64>,
}

/// Runs one attachment's solver over a motion track for
/// `ticks` steps, with the document's displacement threshold
/// (or `f64::INFINITY` for a non-displaceable item).
pub fn run_solve(
    attachment: &Attachment,
    base_x_px: f64,
    base_y_px: f64,
    track: MotionTrack,
    ticks: u64,
    displacement_threshold_px: f64,
) -> SolveTrace {
    let start = anchor_position(track, base_x_px, base_y_px, 0);
    let mut solver = AttachmentSolver::new(start.0, start.1);
    let mut trace = SolveTrace {
        ticks,
        max_residual_px: 0.0,
        ticks_over_bound: 0,
        max_pre_projection_error_px: 0.0,
        lag_steps: 0,
        displacement_ticks: Vec::new(),
    };
    let mut prev_target_x = start.0;
    let mut target_cross_tick: Option<u64> = None;
    let mut attach_cross_tick: Option<u64> = None;
    for tick in 0..ticks {
        let target = anchor_position(track, base_x_px, base_y_px, tick);
        let outcome = step_with_threshold(
            &mut solver,
            target.0,
            target.1,
            attachment,
            displacement_threshold_px,
        );
        trace.max_residual_px = trace.max_residual_px.max(outcome.residual_px);
        if outcome.residual_px > DRIFT_BOUND_PX {
            trace.ticks_over_bound += 1;
        }
        trace.max_pre_projection_error_px = trace
            .max_pre_projection_error_px
            .max(outcome.pre_projection_error_px);
        if outcome.displaced_event {
            trace.displacement_ticks.push(tick);
        }
        // Lag instrument: first upward crossing of the base x
        // by the anchor vs by the rendered (projected) point.
        if target_cross_tick.is_none() && prev_target_x < base_x_px && target.0 >= base_x_px {
            target_cross_tick = Some(tick);
        }
        if attach_cross_tick.is_none() && prev_target_x < base_x_px && outcome.x_px >= base_x_px {
            attach_cross_tick = Some(tick);
        }
        prev_target_x = target.0;
    }
    if let (Some(target_tick), Some(attach_tick)) = (target_cross_tick, attach_cross_tick) {
        trace.lag_steps = attach_tick.saturating_sub(target_tick) as u32;
    }
    trace
}

/// One solver step with an explicit displacement threshold —
/// the document's physics threshold, or infinity when the item
/// cannot displace.
pub fn step_with_threshold(
    solver: &mut AttachmentSolver,
    target_x_px: f64,
    target_y_px: f64,
    attachment: &Attachment,
    displacement_threshold_px: f64,
) -> StepOutcome {
    let k = spring_constant(attachment.stiffness_permille);
    let c = 2.0 * k.sqrt();
    let h = 1.0 / (STEPS_PER_SECOND as f64 * SOLVE_ITERATIONS as f64);
    for _ in 0..SOLVE_ITERATIONS {
        let ax = k * (target_x_px - solver.spring_x_px) - c * solver.spring_vx;
        let ay = k * (target_y_px - solver.spring_y_px) - c * solver.spring_vy;
        solver.spring_vx += ax * h;
        solver.spring_vy += ay * h;
        solver.spring_x_px += solver.spring_vx * h;
        solver.spring_y_px += solver.spring_vy * h;
    }
    let error = ((target_x_px - solver.spring_x_px).powi(2)
        + (target_y_px - solver.spring_y_px).powi(2))
    .sqrt();
    let mut displaced_event = false;
    if error > displacement_threshold_px {
        solver.over_threshold_steps += 1;
        if solver.over_threshold_steps >= DISPLACEMENT_STEPS && !solver.displaced {
            solver.displaced = true;
            displaced_event = true;
        }
    } else {
        solver.over_threshold_steps = 0;
    }
    StepOutcome {
        x_px: target_x_px,
        y_px: target_y_px,
        residual_px: 0.0,
        pre_projection_error_px: error,
        displaced_event,
    }
}

/// The contact model (§4.3): quality from slip, pressure
/// depth, and an optional apparel transmission factor
/// (§4.4). `Removed` items have no contact by definition.
pub fn contact_quality(
    variant: StateVariant,
    slip_px: f64,
    contact_radius_px: f64,
    pressure_permille: i64,
    max_pressure_permille: i64,
    transmission_permille: Option<i64>,
) -> f64 {
    if variant == StateVariant::Removed || contact_radius_px <= 0.0 {
        return 0.0;
    }
    let slip = slip_px / contact_radius_px;
    if slip >= 1.0 {
        return 0.0;
    }
    let depth = if max_pressure_permille <= 0 {
        0.0
    } else {
        (pressure_permille as f64 / max_pressure_permille as f64).clamp(0.0, 1.0)
    };
    let mut q = (1.0 - slip) * (0.35 + 0.65 * depth);
    if let Some(transmission) = transmission_permille {
        q *= transmission.clamp(0, 1000) as f64 / 1000.0;
    }
    q.clamp(0.0, 1.0)
}
