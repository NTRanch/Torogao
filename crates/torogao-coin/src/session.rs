//! A play session over the coin dynamic: preparation actions
//! on a day clock, then timed swing attempts whose gauge is
//! the accumulated swing points scaled by the preparation
//! product.
//!
//! Measured composition (MainGameMode::run IL_20f3-2137): the
//! gauge receives accumulated points * (hp + 1) * (mp + 1) *
//! (sp + 1) * gauge_coefficient — a product, not a sum. The
//! attempt timer starts at timer_start and is decremented by
//! dt / timer_divisor per step (IL_20dd), failing at <= 0; a
//! full gauge (ratio 1 against the level's maximum) raises the
//! level by 1 (IL_21ad-21c0).
//!
//! Skeleton contract, stated plainly: within one step the
//! swing sample is fed first, success is checked, then the
//! timer is decremented and timeout is checked. The source's
//! intra-frame ordering is not established by the static
//! record; this order is the skeleton's own and is pinned by
//! tests.

use crate::dynamics::{CoinError, Dynamics, StatTriple};
use crate::swing::{SAMPLES_MAX, SwingAccumulator};

/// Bound on one step's dt, in seconds (a 1 s step at the
/// source's frame rates is already pathological input).
pub const DT_SECONDS_MAX: f64 = 1.0;

/// The result of one attempt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttemptOutcome {
    pub success: bool,
    /// Final gauge value (points after the preparation
    /// product), comparable against `Dynamics::gauge_maximum`.
    pub gauge: f64,
    /// Raw swing points banked during the attempt.
    pub points: f64,
    /// Seconds consumed inside the attempt window.
    pub elapsed_seconds: f64,
    /// The session's level after the attempt.
    pub level_after: usize,
    /// True when the session sits at the last level the
    /// measured table covers: a success there cannot raise
    /// the level, because the next maximum is UNKNOWN.
    pub at_measured_cap: bool,
}

/// Session state. Deterministic: identical input sequences
/// produce identical outcomes, bit for bit.
#[derive(Clone, Debug)]
pub struct Session {
    dynamics: Dynamics,
    prep: StatTriple,
    status_up: StatTriple,
    level: usize,
    clock_minute: u32,
}

impl Session {
    /// A fresh session at level 0 with default saved gains
    /// (status_up = 1 per stat; measured defaults) and the
    /// day clock at `day_start_minute`.
    pub fn new(dynamics: Dynamics, day_start_minute: u32) -> Result<Session, CoinError> {
        if day_start_minute > crate::dynamics::MINUTE_OF_DAY_MAX {
            return Err(CoinError::Invalid {
                field: "day_start_minute",
                reason: "outside 0..=1439",
            });
        }
        Ok(Session {
            dynamics,
            prep: StatTriple::ZERO,
            status_up: StatTriple::ONE,
            level: 0,
            clock_minute: day_start_minute,
        })
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn prep(&self) -> StatTriple {
        self.prep
    }

    pub fn clock_minute(&self) -> u32 {
        self.clock_minute
    }

    /// Apply one preparation action by switch index: the base
    /// tuple scaled by prep_scale and the per-stat saved gain,
    /// floored at 0 per stat (statusUIScr::minCut, measured),
    /// and the day clock advanced by the action's minutes.
    /// Returns the minutes spent.
    pub fn apply_prep_action(&mut self, index: usize) -> Result<u32, CoinError> {
        let action = self.dynamics.prep_action(index)?;
        let s = self.dynamics.prep_scale();
        self.prep.hp = (self.prep.hp + action.delta.hp * s * self.status_up.hp).max(0.0);
        self.prep.mp = (self.prep.mp + action.delta.mp * s * self.status_up.mp).max(0.0);
        self.prep.sp = (self.prep.sp + action.delta.sp * s * self.status_up.sp).max(0.0);
        self.clock_minute = self.clock_minute.saturating_add(action.minutes);
        Ok(action.minutes)
    }

    /// The message-driven stat path (UICtl::showMessage):
    /// each raw value is divided by message_divisor, scaled by
    /// message_scale, and added, floored at 0. Raw values are
    /// external input: non-finite values are refused.
    pub fn apply_message_stats(&mut self, raw: StatTriple) -> Result<(), CoinError> {
        if ![raw.hp, raw.mp, raw.sp].iter().all(|v| v.is_finite()) {
            return Err(CoinError::InvalidStep {
                reason: "message stats must be finite",
            });
        }
        let div = self.dynamics.message_divisor();
        let scale = self.dynamics.message_scale();
        self.prep.hp = (self.prep.hp + raw.hp / div * scale).max(0.0);
        self.prep.mp = (self.prep.mp + raw.mp / div * scale).max(0.0);
        self.prep.sp = (self.prep.sp + raw.sp / div * scale).max(0.0);
        Ok(())
    }

    /// The preparation product the gauge applies to banked
    /// swing points: (hp + 1) * (mp + 1) * (sp + 1) *
    /// gauge_coefficient (measured; offsets +1 per stat).
    pub fn gauge_multiplier(&self) -> f64 {
        (self.prep.hp + 1.0)
            * (self.prep.mp + 1.0)
            * (self.prep.sp + 1.0)
            * self.dynamics.gauge_coefficient()
    }

    /// Run one timed attempt over a pointer-x sample stream
    /// with a fixed step `dt_seconds`. On success the level
    /// rises by 1, except at the measured table's cap, where
    /// the success is reported with `at_measured_cap` set and
    /// the level holds — inventing a sixth maximum would be
    /// fabrication, not adaptation.
    pub fn run_attempt(
        &mut self,
        samples: &[f64],
        dt_seconds: f64,
    ) -> Result<AttemptOutcome, CoinError> {
        self.attempt(samples, dt_seconds, None)
    }

    /// `run_attempt` with a per-step trajectory trace: after
    /// each executed sample step, one record is appended to
    /// `trace` — the accumulator's banked points and the gauge
    /// they make (points x the attempt's preparation
    /// multiplier, constant within an attempt), each as an
    /// f64 bit pattern. The traced and untraced runs are the
    /// same loop, so their outcomes agree by construction;
    /// the trace is the canonical record the cross-platform
    /// trajectory hash (`trajectory` module) digests.
    pub fn run_attempt_traced(
        &mut self,
        samples: &[f64],
        dt_seconds: f64,
        trace: &mut Vec<(u64, u64)>,
    ) -> Result<AttemptOutcome, CoinError> {
        self.attempt(samples, dt_seconds, Some(trace))
    }

    fn attempt(
        &mut self,
        samples: &[f64],
        dt_seconds: f64,
        mut trace: Option<&mut Vec<(u64, u64)>>,
    ) -> Result<AttemptOutcome, CoinError> {
        if !dt_seconds.is_finite() || dt_seconds <= 0.0 || dt_seconds > DT_SECONDS_MAX {
            return Err(CoinError::InvalidStep {
                reason: "dt outside (0, 1] seconds",
            });
        }
        if samples.len() > SAMPLES_MAX {
            return Err(CoinError::InvalidStep {
                reason: "sample stream exceeds SAMPLES_MAX",
            });
        }
        let target = self.dynamics.gauge_maximum(self.level)?;
        let mut swing = SwingAccumulator::new();
        let mut timer = self.dynamics.timer_start();
        let divisor = self.dynamics.timer_divisor();
        let mut elapsed = 0.0_f64;
        let mut success = false;
        for &x in samples {
            swing.feed(x, &self.dynamics)?;
            if let Some(trace) = &mut trace {
                let points = swing.points();
                trace.push((points.to_bits(), (points * self.gauge_multiplier()).to_bits()));
            }
            elapsed += dt_seconds;
            if swing.points() * self.gauge_multiplier() >= target {
                success = true;
                break;
            }
            timer -= dt_seconds / divisor;
            if timer <= 0.0 {
                break;
            }
        }
        let points = swing.points();
        let gauge = points * self.gauge_multiplier();
        let at_cap = self.level + 1 >= self.dynamics.measured_level_count();
        if success && !at_cap {
            self.level += 1;
        }
        Ok(AttemptOutcome {
            success,
            gauge,
            points,
            elapsed_seconds: elapsed,
            level_after: self.level,
            at_measured_cap: at_cap,
        })
    }

    /// Saving is possible only at the end of the day: the
    /// clock at or past the leaving minute (18:00, measured
    /// day frame).
    pub fn can_save(&self) -> bool {
        self.clock_minute >= self.dynamics.leaving_minute()
    }
}
