//! `torogao-spring`: deterministic spring and gesture physics for
//! torogao's Slint surfaces — the shared implementation behind
//! drag/settle motion in torogao-mobile, MoonDeep mobile, and the
//! rig editor.
//!
//! # Model
//!
//! A spring is parameterised the way Apple parameterises it in
//! *Designing Fluid Interfaces* (WWDC 2018): a **damping ratio**
//! (1.0 = critically damped, no overshoot; below 1.0 = overshoot)
//! and a **response** in seconds (how quickly the value reaches
//! the target — not a duration; settle time emerges from the
//! parameters). The physics mapping, at unit mass, is:
//!
//! - angular frequency `ω = 2π / response`
//! - stiffness `k = ω²`
//! - damping coefficient `c = 2 · damping_ratio · ω`
//!
//! Integration is semi-implicit Euler at a fixed timestep
//! ([`FIXED_STEP_SECONDS`]). The integrator never reads a clock:
//! the same parameters, start state, and step count produce a
//! bit-identical trajectory on a given platform, which is what
//! lets a gesture be replayed in a test and judged step by step.
//!
//! The crate is one-dimensional by design. For 2D motion, run two
//! springs — one per axis — as upstream prescribes: a single
//! spring on a 2D distance desynchronises when the axes carry
//! different velocities.
//!
//! # Provenance
//!
//! Adapted from the `apple-design` and `gestures` skills in
//! emilkowalski/skills at commit
//! `64eb3deb9a5fbfa041faf783e0bc3fd635872e9c` (MIT licence),
//! which distil Apple's WWDC fluid-interface material. The
//! parameter values, the velocity-handoff normalisation, the
//! momentum-projection function, and the rubber-band function are
//! upstream's; the fixed-step deterministic integrator, the
//! contract bounds, and this Rust API are the estate adaptation.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::error::Error;
use std::f64::consts::TAU;
use std::fmt;

/// Fixed integration timestep, in seconds (240 Hz). Callers decide
/// how many steps a frame consumes; the integrator itself is
/// clock-free, so trajectories depend only on step counts.
pub const FIXED_STEP_SECONDS: f64 = 1.0 / 240.0;

/// Maximum steps a single [`Spring::advance`] call may take
/// (2_400 steps = 10 s of simulated time). Bounds the work any
/// one call can do; longer runs chain calls.
pub const MAX_ADVANCE_STEPS: u32 = 2_400;

/// Largest accepted damping ratio. Ratios above 1.0 are
/// overdamped (no overshoot, sluggish settle); 8.0 is already
/// far past anything a UI surface can use.
pub const DAMPING_RATIO_MAX: f64 = 8.0;

/// Smallest accepted response, in seconds. Below this the fixed
/// 240 Hz step can no longer integrate the spring stably
/// (`ω · dt` approaches the semi-implicit Euler stability edge).
pub const RESPONSE_SECONDS_MIN: f64 = 0.05;

/// Largest accepted response, in seconds. A spring slower than
/// this is a timer, not a gesture.
pub const RESPONSE_SECONDS_MAX: f64 = 30.0;

/// Apple's normal scroll deceleration rate, used by
/// [`projected_offset`]. Per-millisecond exponential decay.
pub const DECELERATION_RATE_NORMAL: f64 = 0.998;

/// Snappier deceleration rate: projections land closer, flicks
/// feel shorter.
pub const DECELERATION_RATE_SNAPPY: f64 = 0.99;

/// Upstream's rubber-band constant: at the boundary, the element
/// follows the pointer at 55% and resistance grows from there.
pub const RUBBER_BAND_CONSTANT_DEFAULT: f64 = 0.55;

/// Default settle tolerance on position, in value units, for
/// [`Spring::is_settled`]. Callers animating large pixel ranges
/// should pass their own via [`Spring::is_settled_within`].
pub const SETTLE_POSITION_TOLERANCE: f64 = 0.1;

/// Default settle tolerance on velocity, in value units per
/// second, for [`Spring::is_settled`].
pub const SETTLE_VELOCITY_TOLERANCE: f64 = 1.0;

/// A rejected input to the physics core. Every public constructor
/// and pure function validates before it computes, so a bad
/// parameter surfaces as this typed error — never as a NaN
/// trajectory discovered on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhysicsError {
    /// An input was NaN or infinite.
    NonFiniteValue,
    /// Damping ratio was zero, negative, or above
    /// [`DAMPING_RATIO_MAX`].
    DampingRatioOutOfRange {
        /// The rejected damping ratio.
        damping_ratio: f64,
    },
    /// Response was outside
    /// \[[`RESPONSE_SECONDS_MIN`], [`RESPONSE_SECONDS_MAX`]\].
    ResponseOutOfRange {
        /// The rejected response, in seconds.
        response_seconds: f64,
    },
    /// Deceleration rate was outside the open interval (0, 1);
    /// at 1 the projection divides by zero, at 0 nothing moves.
    DecelerationRateOutOfRange {
        /// The rejected deceleration rate.
        deceleration_rate: f64,
    },
    /// Rubber-band dimension (the range the element moves in)
    /// was zero or negative.
    DimensionOutOfRange {
        /// The rejected dimension, in value units.
        dimension: f64,
    },
    /// Rubber-band constant was zero or negative.
    RubberBandConstantOutOfRange {
        /// The rejected constant.
        constant: f64,
    },
    /// Relative velocity was requested with target == current:
    /// the normalisation divides by the remaining distance.
    ZeroDistance,
    /// [`Spring::advance`] was asked for more than
    /// [`MAX_ADVANCE_STEPS`] steps in one call.
    StepBudgetExceeded {
        /// The rejected step count.
        steps: u32,
    },
}

impl fmt::Display for PhysicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteValue => write!(f, "value must be finite"),
            Self::DampingRatioOutOfRange { damping_ratio } => {
                write!(
                    f,
                    "damping ratio {damping_ratio} outside (0, {DAMPING_RATIO_MAX}]"
                )
            }
            Self::ResponseOutOfRange { response_seconds } => {
                write!(
                    f,
                    "response {response_seconds}s outside \
                     [{RESPONSE_SECONDS_MIN}, {RESPONSE_SECONDS_MAX}]s"
                )
            }
            Self::DecelerationRateOutOfRange { deceleration_rate } => {
                write!(f, "deceleration rate {deceleration_rate} outside (0, 1)")
            }
            Self::DimensionOutOfRange { dimension } => {
                write!(f, "rubber-band dimension {dimension} must be positive")
            }
            Self::RubberBandConstantOutOfRange { constant } => {
                write!(f, "rubber-band constant {constant} must be positive")
            }
            Self::ZeroDistance => {
                write!(f, "relative velocity undefined at zero distance to target")
            }
            Self::StepBudgetExceeded { steps } => {
                write!(
                    f,
                    "advance of {steps} steps exceeds budget {MAX_ADVANCE_STEPS}"
                )
            }
        }
    }
}

impl Error for PhysicsError {}

fn require_finite(value: f64) -> Result<f64, PhysicsError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(PhysicsError::NonFiniteValue)
    }
}

/// Spring parameters in Apple's parameterisation: damping ratio
/// and response. Construct via [`SpringParams::new`] (validated)
/// or the named presets, which carry the values Apple ships.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringParams {
    damping_ratio: f64,
    response_seconds: f64,
}

impl SpringParams {
    /// Creates validated parameters.
    ///
    /// # Errors
    ///
    /// [`PhysicsError::NonFiniteValue`] for NaN/infinite inputs,
    /// [`PhysicsError::DampingRatioOutOfRange`] for a ratio
    /// outside (0, [`DAMPING_RATIO_MAX`]],
    /// [`PhysicsError::ResponseOutOfRange`] for a response
    /// outside \[[`RESPONSE_SECONDS_MIN`], [`RESPONSE_SECONDS_MAX`]\].
    pub fn new(damping_ratio: f64, response_seconds: f64) -> Result<Self, PhysicsError> {
        require_finite(damping_ratio)?;
        require_finite(response_seconds)?;
        if damping_ratio <= 0.0 || damping_ratio > DAMPING_RATIO_MAX {
            return Err(PhysicsError::DampingRatioOutOfRange { damping_ratio });
        }
        if !(RESPONSE_SECONDS_MIN..=RESPONSE_SECONDS_MAX).contains(&response_seconds) {
            return Err(PhysicsError::ResponseOutOfRange { response_seconds });
        }
        Ok(Self {
            damping_ratio,
            response_seconds,
        })
    }

    /// Move / reposition (e.g. picture-in-picture): damping 1.0,
    /// response 0.4 s. Critically damped — the default starting
    /// point for UI motion that no gesture threw.
    pub fn move_reposition() -> Self {
        Self {
            damping_ratio: 1.0,
            response_seconds: 0.4,
        }
    }

    /// Rotation: damping 0.8, response 0.4 s.
    pub fn rotation() -> Self {
        Self {
            damping_ratio: 0.8,
            response_seconds: 0.4,
        }
    }

    /// Drawer / bottom sheet: damping 0.8, response 0.3 s. The
    /// slight overshoot is earned by the drag that precedes it.
    pub fn drawer_sheet() -> Self {
        Self {
            damping_ratio: 0.8,
            response_seconds: 0.3,
        }
    }

    /// The damping ratio: 1.0 is critically damped (no
    /// overshoot); below 1.0 the spring overshoots and settles.
    pub fn damping_ratio(&self) -> f64 {
        self.damping_ratio
    }

    /// The response, in seconds: how quickly the value reaches
    /// the target. Not a duration — settle time emerges.
    pub fn response_seconds(&self) -> f64 {
        self.response_seconds
    }

    /// Angular frequency `ω = 2π / response`, in radians/second.
    pub fn angular_frequency(&self) -> f64 {
        TAU / self.response_seconds
    }

    /// Stiffness `k = ω²` at unit mass, in units/second².
    pub fn stiffness(&self) -> f64 {
        let omega = self.angular_frequency();
        omega * omega
    }

    /// Damping coefficient `c = 2 · damping_ratio · ω` at unit
    /// mass, in units/second.
    pub fn damping_coefficient(&self) -> f64 {
        2.0 * self.damping_ratio * self.angular_frequency()
    }
}

/// A one-dimensional spring: position and velocity integrated
/// toward a target at the fixed timestep. Retargeting preserves
/// velocity, which is what makes the motion interruptible — a
/// user can grab a moving element and redirect it without a
/// velocity discontinuity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    params: SpringParams,
    position: f64,
    velocity: f64,
    target: f64,
}

impl Spring {
    /// Creates a spring at rest at `position`, aimed at `target`.
    ///
    /// # Errors
    ///
    /// [`PhysicsError::NonFiniteValue`] for NaN/infinite inputs.
    pub fn new(params: SpringParams, position: f64, target: f64) -> Result<Self, PhysicsError> {
        require_finite(position)?;
        require_finite(target)?;
        Ok(Self {
            params,
            position,
            velocity: 0.0,
            target,
        })
    }

    /// Creates a spring seeded with a gesture's release velocity
    /// — the velocity handoff. The animation continues at exactly
    /// the speed the finger was moving, so there is no visible
    /// seam between dragging and animating.
    ///
    /// # Errors
    ///
    /// [`PhysicsError::NonFiniteValue`] for NaN/infinite inputs.
    pub fn from_gesture(
        params: SpringParams,
        position: f64,
        target: f64,
        release_velocity: f64,
    ) -> Result<Self, PhysicsError> {
        require_finite(position)?;
        require_finite(target)?;
        require_finite(release_velocity)?;
        Ok(Self {
            params,
            position,
            velocity: release_velocity,
            target,
        })
    }

    /// The parameters this spring integrates under.
    pub fn params(&self) -> SpringParams {
        self.params
    }

    /// Current position, in value units.
    pub fn position(&self) -> f64 {
        self.position
    }

    /// Current velocity, in value units per second.
    pub fn velocity(&self) -> f64 {
        self.velocity
    }

    /// The value the spring is settling toward.
    pub fn target(&self) -> f64 {
        self.target
    }

    /// Retargets the spring, preserving position and velocity.
    /// This is the interrupt operation: call it when the user
    /// grabs a moving element or the destination changes
    /// mid-flight.
    ///
    /// # Errors
    ///
    /// [`PhysicsError::NonFiniteValue`] for a NaN/infinite
    /// target; on error the spring is left unchanged.
    pub fn set_target(&mut self, target: f64) -> Result<(), PhysicsError> {
        require_finite(target)?;
        self.target = target;
        Ok(())
    }

    /// Advances exactly one fixed step ([`FIXED_STEP_SECONDS`])
    /// of semi-implicit Euler integration.
    pub fn step(&mut self) {
        let stiffness = self.params.stiffness();
        let damping = self.params.damping_coefficient();
        let acceleration = stiffness * (self.target - self.position) - damping * self.velocity;
        self.velocity += acceleration * FIXED_STEP_SECONDS;
        self.position += self.velocity * FIXED_STEP_SECONDS;
    }

    /// Advances `steps` fixed steps.
    ///
    /// # Errors
    ///
    /// [`PhysicsError::StepBudgetExceeded`] if `steps` exceeds
    /// [`MAX_ADVANCE_STEPS`]; on error the spring is unchanged.
    pub fn advance(&mut self, steps: u32) -> Result<(), PhysicsError> {
        if steps > MAX_ADVANCE_STEPS {
            return Err(PhysicsError::StepBudgetExceeded { steps });
        }
        for _ in 0..steps {
            self.step();
        }
        Ok(())
    }

    /// Whether the spring has settled within the default
    /// tolerances ([`SETTLE_POSITION_TOLERANCE`],
    /// [`SETTLE_VELOCITY_TOLERANCE`]).
    pub fn is_settled(&self) -> bool {
        self.is_settled_within(SETTLE_POSITION_TOLERANCE, SETTLE_VELOCITY_TOLERANCE)
    }

    /// Whether the spring has settled within caller tolerances:
    /// position within `position_tolerance` of the target and
    /// speed at most `velocity_tolerance`. Tolerances carry the
    /// caller's units and must be non-negative.
    pub fn is_settled_within(&self, position_tolerance: f64, velocity_tolerance: f64) -> bool {
        (self.target - self.position).abs() <= position_tolerance
            && self.velocity.abs() <= velocity_tolerance
    }

    /// Snaps exactly onto the target and zeroes velocity. Call
    /// once [`Spring::is_settled`] holds, so the resting value is
    /// exact instead of asymptotically close.
    pub fn snap_to_target(&mut self) {
        self.position = self.target;
        self.velocity = 0.0;
    }
}

/// Upstream's velocity-handoff normalisation: some spring APIs
/// want velocity relative to the remaining distance,
/// `gesture_velocity / (target − current)`.
///
/// # Errors
///
/// [`PhysicsError::ZeroDistance`] when `target == current` (the
/// normalisation would divide by zero);
/// [`PhysicsError::NonFiniteValue`] for NaN/infinite inputs.
pub fn relative_velocity(
    gesture_velocity: f64,
    target: f64,
    current: f64,
) -> Result<f64, PhysicsError> {
    require_finite(gesture_velocity)?;
    require_finite(target)?;
    require_finite(current)?;
    let distance = target - current;
    if distance == 0.0 {
        return Err(PhysicsError::ZeroDistance);
    }
    Ok(gesture_velocity / distance)
}

/// The distance a released gesture travels while decelerating:
/// Apple's projection, `(velocity / 1000) · d / (1 − d)`, with
/// `d` the per-millisecond deceleration rate
/// ([`DECELERATION_RATE_NORMAL`] ≈ 0.998). The sign follows the
/// velocity: flick left, project left. Note this is deliberately
/// *not* the textbook `v² / (2·decel)` — it is the exponential
/// form Apple ships.
///
/// # Errors
///
/// [`PhysicsError::DecelerationRateOutOfRange`] for a rate
/// outside (0, 1); [`PhysicsError::NonFiniteValue`] for
/// NaN/infinite inputs.
pub fn projected_offset(velocity: f64, deceleration_rate: f64) -> Result<f64, PhysicsError> {
    require_finite(velocity)?;
    require_finite(deceleration_rate)?;
    if deceleration_rate <= 0.0 || deceleration_rate >= 1.0 {
        return Err(PhysicsError::DecelerationRateOutOfRange { deceleration_rate });
    }
    Ok((velocity / 1000.0) * deceleration_rate / (1.0 - deceleration_rate))
}

/// The projected resting position of a released gesture:
/// `current + `[`projected_offset`]. Snap decisions (which sheet
/// detent, which page) are made against this point, not the
/// release point — that is what makes a flick feel like a throw.
///
/// # Errors
///
/// As [`projected_offset`], plus
/// [`PhysicsError::NonFiniteValue`] for a non-finite `current`.
pub fn projected_position(
    current: f64,
    velocity: f64,
    deceleration_rate: f64,
) -> Result<f64, PhysicsError> {
    require_finite(current)?;
    Ok(current + projected_offset(velocity, deceleration_rate)?)
}

/// The snap point nearest a projected position. Returns `None`
/// for an empty slice or a non-finite projection. Ties resolve
/// to the earliest point in the slice, so the result is
/// deterministic for a fixed input order. Non-finite snap
/// points are skipped.
pub fn nearest_snap_point(projected: f64, snap_points: &[f64]) -> Option<f64> {
    if !projected.is_finite() {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    for &point in snap_points {
        if !point.is_finite() {
            continue;
        }
        let distance = (point - projected).abs();
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((point, distance));
        }
    }
    best.map(|(point, _)| point)
}

/// Rubber-banding with the upstream default constant
/// ([`RUBBER_BAND_CONSTANT_DEFAULT`]): how far an element may be
/// dragged past a boundary, as a function of how far past it the
/// pointer has travelled. See [`rubber_band_with_constant`].
///
/// # Errors
///
/// As [`rubber_band_with_constant`].
pub fn rubber_band(overshoot: f64, dimension: f64) -> Result<f64, PhysicsError> {
    rubber_band_with_constant(overshoot, dimension, RUBBER_BAND_CONSTANT_DEFAULT)
}

/// Rubber-banding: progressive resistance past a boundary,
/// `(overshoot · dimension · c) / (dimension + c · |overshoot|)`.
/// Continuous at the boundary (zero overshoot maps to zero),
/// odd-symmetric (resistance mirrors on both edges), and bounded
/// by `dimension` no matter how far the pointer travels — real
/// things slow before they stop; they never stop dead.
///
/// `dimension` is the size of the range the element moves in
/// (e.g. the drawer's travel), in the same units as `overshoot`.
///
/// # Errors
///
/// [`PhysicsError::DimensionOutOfRange`] for a non-positive
/// dimension, [`PhysicsError::RubberBandConstantOutOfRange`] for
/// a non-positive constant, [`PhysicsError::NonFiniteValue`] for
/// NaN/infinite inputs.
pub fn rubber_band_with_constant(
    overshoot: f64,
    dimension: f64,
    constant: f64,
) -> Result<f64, PhysicsError> {
    require_finite(overshoot)?;
    require_finite(dimension)?;
    require_finite(constant)?;
    if dimension <= 0.0 {
        return Err(PhysicsError::DimensionOutOfRange { dimension });
    }
    if constant <= 0.0 {
        return Err(PhysicsError::RubberBandConstantOutOfRange { constant });
    }
    Ok((overshoot * dimension * constant) / (dimension + constant * overshoot.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The adversarial cases in this module are the positive
    // controls for the crate's gates (estate verifier clause):
    // a build that stops refusing bad parameters, stops carrying
    // velocity, or stops being deterministic fails here by name.

    fn approx_eq(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0)
    }

    fn trajectory_hash(spring: &mut Spring, steps: u32) -> u64 {
        // FNV-1a over the bit patterns of position and velocity
        // after every step: any change to the integrator's
        // arithmetic changes the hash.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for _ in 0..steps {
            spring.step();
            for value in [spring.position(), spring.velocity()] {
                for byte in value.to_bits().to_le_bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        hash
    }

    // ---- Validation: the physics does what upstream says ----

    #[test]
    fn params_physics_mapping_matches_apple_parameterisation() {
        let params = SpringParams::new(1.0, 0.4).unwrap();
        // ω = 2π / 0.4, k = ω², c = 2 · 1.0 · ω (unit mass).
        assert!(approx_eq(
            params.angular_frequency(),
            15.707_963_267_948_966
        ));
        assert!(approx_eq(params.stiffness(), 246.740_110_027_233_95));
        assert!(approx_eq(
            params.damping_coefficient(),
            31.415_926_535_897_93
        ));
    }

    #[test]
    fn presets_carry_upstream_values() {
        let mv = SpringParams::move_reposition();
        assert_eq!((mv.damping_ratio(), mv.response_seconds()), (1.0, 0.4));
        let rot = SpringParams::rotation();
        assert_eq!((rot.damping_ratio(), rot.response_seconds()), (0.8, 0.4));
        let drawer = SpringParams::drawer_sheet();
        assert_eq!(
            (drawer.damping_ratio(), drawer.response_seconds()),
            (0.8, 0.3)
        );
    }

    #[test]
    fn critically_damped_settles_without_overshoot() {
        let mut spring = Spring::new(SpringParams::move_reposition(), 0.0, 100.0).unwrap();
        let mut max_position = f64::MIN;
        let mut steps = 0;
        while !spring.is_settled() && steps < MAX_ADVANCE_STEPS {
            spring.step();
            steps += 1;
            max_position = max_position.max(spring.position());
        }
        assert!(spring.is_settled(), "not settled after {steps} steps");
        // Critically damped: approaches from below; 1e-6 of slack
        // for the discrete integrator, never a visible overshoot.
        assert!(max_position <= 100.0 + 1e-6, "overshot to {max_position}");
        spring.snap_to_target();
        assert_eq!(spring.position(), 100.0);
        assert_eq!(spring.velocity(), 0.0);
    }

    #[test]
    fn underdamped_overshoots_then_settles() {
        let mut spring = Spring::new(SpringParams::drawer_sheet(), 0.0, 100.0).unwrap();
        let mut max_position = f64::MIN;
        let mut steps = 0;
        while !spring.is_settled() && steps < MAX_ADVANCE_STEPS {
            spring.step();
            steps += 1;
            max_position = max_position.max(spring.position());
        }
        assert!(spring.is_settled(), "not settled after {steps} steps");
        // ζ = 0.8 overshoots by ~1.5% of the distance travelled.
        assert!(max_position > 100.5, "no overshoot, max {max_position}");
    }

    #[test]
    fn velocity_handoff_moves_away_when_released_away() {
        // Released moving away from the target: the first step
        // must still travel away — velocity is carried, not cut.
        let mut spring =
            Spring::from_gesture(SpringParams::move_reposition(), 0.0, 100.0, -200.0).unwrap();
        spring.step();
        assert!(spring.position() < 0.0, "moved to {}", spring.position());
    }

    #[test]
    fn velocity_handoff_toward_target_is_ahead_of_rest_start() {
        let params = SpringParams::drawer_sheet();
        let mut thrown = Spring::from_gesture(params, 0.0, 260.0, 320.0).unwrap();
        let mut resting = Spring::new(params, 0.0, 260.0).unwrap();
        thrown.advance(24).unwrap();
        resting.advance(24).unwrap();
        assert!(thrown.position() > resting.position());
    }

    #[test]
    fn retarget_preserves_velocity_and_position() {
        let mut spring =
            Spring::from_gesture(SpringParams::drawer_sheet(), 0.0, 260.0, 320.0).unwrap();
        spring.advance(24).unwrap();
        let (position, velocity) = (spring.position(), spring.velocity());
        spring.set_target(40.0).unwrap();
        assert_eq!(spring.position(), position);
        assert_eq!(spring.velocity(), velocity);
        assert_eq!(spring.target(), 40.0);
    }

    #[test]
    fn relative_velocity_matches_upstream_example() {
        // Upstream's worked example: at 50, target 150, finger at
        // 50 units/s → 50 / (150 − 50) = 0.5.
        assert_eq!(relative_velocity(50.0, 150.0, 50.0).unwrap(), 0.5);
        assert_eq!(relative_velocity(-50.0, 150.0, 50.0).unwrap(), -0.5);
    }

    #[test]
    fn projected_offset_matches_upstream_formula() {
        // (1000 / 1000) · 0.998 / (1 − 0.998) = 499.
        assert!(approx_eq(
            projected_offset(1000.0, DECELERATION_RATE_NORMAL).unwrap(),
            499.0
        ));
        // Sign follows velocity, at the snappy rate:
        // (−500 / 1000) · 0.99 / 0.01 = −49.5.
        assert!(approx_eq(
            projected_offset(-500.0, DECELERATION_RATE_SNAPPY).unwrap(),
            -49.5
        ));
        assert_eq!(
            projected_offset(0.0, DECELERATION_RATE_NORMAL).unwrap(),
            0.0
        );
    }

    #[test]
    fn projected_position_adds_offset() {
        let projected = projected_position(20.0, 1000.0, DECELERATION_RATE_NORMAL).unwrap();
        assert!(approx_eq(projected, 519.0));
    }

    #[test]
    fn nearest_snap_point_picks_closest() {
        let snaps = [0.0, 300.0, 600.0];
        assert_eq!(nearest_snap_point(519.0, &snaps), Some(600.0));
        assert_eq!(nearest_snap_point(-40.0, &snaps), Some(0.0));
        // Tie resolves to the earliest point: deterministic.
        assert_eq!(nearest_snap_point(150.0, &snaps), Some(0.0));
        assert_eq!(nearest_snap_point(10.0, &[]), None);
        assert_eq!(nearest_snap_point(f64::NAN, &snaps), None);
    }

    #[test]
    fn rubber_band_zero_overshoot_is_zero() {
        assert_eq!(rubber_band(0.0, 300.0).unwrap(), 0.0);
    }

    #[test]
    fn rubber_band_matches_upstream_formula() {
        // (100 · 300 · 0.55) / (300 + 0.55 · 100) = 16500 / 355.
        assert!(approx_eq(
            rubber_band(100.0, 300.0).unwrap(),
            46.478_873_239_436_62
        ));
    }

    #[test]
    fn rubber_band_is_odd_symmetric() {
        let positive = rubber_band(137.0, 300.0).unwrap();
        let negative = rubber_band(-137.0, 300.0).unwrap();
        assert_eq!(negative, -positive);
    }

    #[test]
    fn rubber_band_bounded_by_dimension() {
        let banded = rubber_band(1.0e9, 300.0).unwrap();
        assert!(banded < 300.0, "reached {banded}");
        assert!(banded > 299.9, "only reached {banded}");
    }

    #[test]
    fn determinism_trajectory_hash_is_stable() {
        // Golden hash for the fixed scenario below, recorded on
        // primo (x86_64, pinned toolchain). Any change to the
        // integrator's arithmetic breaks this test by design.
        const GOLDEN_TRAJECTORY_HASH: u64 = 0xd5d8_5e2e_56fe_5c56;
        let scenario =
            || Spring::from_gesture(SpringParams::drawer_sheet(), 0.0, 260.0, 320.0).unwrap();
        let mut first = scenario();
        let mut second = scenario();
        let first_hash = trajectory_hash(&mut first, 480);
        let second_hash = trajectory_hash(&mut second, 480);
        assert_eq!(first_hash, second_hash);
        assert_eq!(
            first_hash, GOLDEN_TRAJECTORY_HASH,
            "trajectory hash drifted"
        );
    }

    #[test]
    fn advance_zero_steps_is_noop() {
        let mut spring =
            Spring::from_gesture(SpringParams::drawer_sheet(), 3.0, 260.0, 320.0).unwrap();
        spring.advance(0).unwrap();
        assert_eq!(spring.position(), 3.0);
        assert_eq!(spring.velocity(), 320.0);
    }

    // ---- Adversarial: the contract refuses what it must ----

    #[test]
    fn params_reject_zero_and_negative_damping_ratio() {
        assert_eq!(
            SpringParams::new(0.0, 0.4),
            Err(PhysicsError::DampingRatioOutOfRange { damping_ratio: 0.0 })
        );
        assert!(matches!(
            SpringParams::new(-0.5, 0.4),
            Err(PhysicsError::DampingRatioOutOfRange { .. })
        ));
        assert!(matches!(
            SpringParams::new(9.0, 0.4),
            Err(PhysicsError::DampingRatioOutOfRange { .. })
        ));
    }

    #[test]
    fn params_reject_out_of_range_response() {
        for response in [0.0, -1.0, 0.01, 60.0] {
            assert!(
                matches!(
                    SpringParams::new(1.0, response),
                    Err(PhysicsError::ResponseOutOfRange { .. })
                ),
                "response {response} accepted"
            );
        }
    }

    #[test]
    fn params_reject_non_finite() {
        assert_eq!(
            SpringParams::new(f64::NAN, 0.4),
            Err(PhysicsError::NonFiniteValue)
        );
        assert_eq!(
            SpringParams::new(1.0, f64::INFINITY),
            Err(PhysicsError::NonFiniteValue)
        );
    }

    #[test]
    fn spring_rejects_non_finite_state() {
        let params = SpringParams::move_reposition();
        assert_eq!(
            Spring::new(params, f64::NAN, 0.0),
            Err(PhysicsError::NonFiniteValue)
        );
        assert_eq!(
            Spring::from_gesture(params, 0.0, 0.0, f64::INFINITY),
            Err(PhysicsError::NonFiniteValue)
        );
        // A rejected retarget leaves the spring untouched
        // (validate → commit, never half-applied).
        let mut spring = Spring::new(params, 10.0, 100.0).unwrap();
        assert_eq!(
            spring.set_target(f64::NAN),
            Err(PhysicsError::NonFiniteValue)
        );
        assert_eq!(spring.target(), 100.0);
    }

    #[test]
    fn relative_velocity_rejects_zero_distance() {
        assert_eq!(
            relative_velocity(50.0, 100.0, 100.0),
            Err(PhysicsError::ZeroDistance)
        );
    }

    #[test]
    fn projection_rejects_rate_outside_open_unit_interval() {
        for rate in [0.0, 1.0, 1.5, -0.5] {
            assert!(
                matches!(
                    projected_offset(100.0, rate),
                    Err(PhysicsError::DecelerationRateOutOfRange { .. })
                ),
                "rate {rate} accepted"
            );
        }
        assert_eq!(
            projected_offset(100.0, f64::NAN),
            Err(PhysicsError::NonFiniteValue)
        );
    }

    #[test]
    fn rubber_band_rejects_bad_dimension_and_constant() {
        assert!(matches!(
            rubber_band(10.0, 0.0),
            Err(PhysicsError::DimensionOutOfRange { .. })
        ));
        assert!(matches!(
            rubber_band(10.0, -300.0),
            Err(PhysicsError::DimensionOutOfRange { .. })
        ));
        assert!(matches!(
            rubber_band_with_constant(10.0, 300.0, 0.0),
            Err(PhysicsError::RubberBandConstantOutOfRange { .. })
        ));
        assert!(matches!(
            rubber_band_with_constant(10.0, 300.0, -0.55),
            Err(PhysicsError::RubberBandConstantOutOfRange { .. })
        ));
        assert_eq!(
            rubber_band(f64::NAN, 300.0),
            Err(PhysicsError::NonFiniteValue)
        );
    }

    #[test]
    fn rubber_band_continuity_at_boundary() {
        // Approaching the boundary from either side, the banded
        // value approaches zero smoothly — no jump at the seam
        // where tracking hands over to resistance.
        for epsilon in [1.0e-9, 1.0e-6, 1.0e-3] {
            let inside = rubber_band(epsilon, 300.0).unwrap();
            let outside = rubber_band(-epsilon, 300.0).unwrap();
            assert!(inside.abs() <= epsilon, "jump at +{epsilon}: {inside}");
            assert!(outside.abs() <= epsilon, "jump at −{epsilon}: {outside}");
            assert_eq!(outside, -inside);
        }
        // And resistance is monotone: further out, further banded.
        let near = rubber_band(10.0, 300.0).unwrap();
        let far = rubber_band(100.0, 300.0).unwrap();
        assert!(far > near);
    }

    #[test]
    fn advance_rejects_over_budget_and_leaves_state_unchanged() {
        let mut spring = Spring::new(SpringParams::move_reposition(), 5.0, 100.0).unwrap();
        assert_eq!(
            spring.advance(MAX_ADVANCE_STEPS + 1),
            Err(PhysicsError::StepBudgetExceeded {
                steps: MAX_ADVANCE_STEPS + 1
            })
        );
        assert_eq!(spring.position(), 5.0);
        assert_eq!(spring.velocity(), 0.0);
    }

    #[test]
    fn huge_release_velocity_stays_finite() {
        let mut spring =
            Spring::from_gesture(SpringParams::drawer_sheet(), 0.0, 260.0, 1.0e6).unwrap();
        for _ in 0..480 {
            spring.step();
            assert!(spring.position().is_finite());
            assert!(spring.velocity().is_finite());
        }
    }
}
