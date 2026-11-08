//! The swing: the coin dynamic's one verb, as a pure
//! accumulator over pointer-x samples.
//!
//! Measured rule (goenScr::Update, IL_0013-00ac): points bank
//! when the sign of the pointer's relative x displacement
//! crosses the stored direction (the direction comparison is
//! against 0, IL_0052-0057). The banked increment is the
//! square of the maximum displacement since the previous
//! crossing, times the swing gain (1.5); a stroke whose
//! maximum displacement reached the bonus threshold (8) is
//! multiplied by the bonus (1.15) before banking.
//!
//! The one interpretive step in this skeleton: "relative"
//! displacement is read as displacement from the attempt's
//! starting pointer position (the first sample), so a stroke
//! is one excursion to one side of the start point, and a
//! crossing is the pointer passing back through it. That
//! reading is pinned by the tests in `tests/`; if a later
//! runtime-calibration rung refutes it, the change is local
//! to this module.

use crate::dynamics::{CoinError, Dynamics};

/// Bound on samples in one attempt stream. At 240 samples per
/// second this is over an hour of pointer input — far past the
/// 20-second attempt window; the bound exists so a caller
/// cannot hand the accumulator unbounded work.
pub const SAMPLES_MAX: usize = 1_000_000;

/// Accumulates swing points from a stream of pointer-x
/// positions (in the source's relative world-x units).
#[derive(Clone, Debug)]
pub struct SwingAccumulator {
    origin_x: f64,
    max_displacement: f64,
    direction: i8,
    points: f64,
    anchored: bool,
}

impl SwingAccumulator {
    pub fn new() -> SwingAccumulator {
        SwingAccumulator {
            origin_x: 0.0,
            max_displacement: 0.0,
            direction: 0,
            points: 0.0,
            anchored: false,
        }
    }

    /// Total points banked so far.
    pub fn points(&self) -> f64 {
        self.points
    }

    /// Reset to the attempt-start state (the source resets the
    /// accumulator to 0 at attempt setup; measured).
    pub fn reset(&mut self) {
        *self = SwingAccumulator::new();
    }

    /// Feed one pointer-x sample. Returns the points banked by
    /// this sample (non-zero only on the sample that crosses
    /// back through the origin). Non-finite samples are a
    /// typed refusal: the caller's input contract is finite
    /// pointer positions.
    pub fn feed(&mut self, x: f64, d: &Dynamics) -> Result<f64, CoinError> {
        if !x.is_finite() {
            return Err(CoinError::InvalidStep {
                reason: "pointer sample must be finite",
            });
        }
        if !self.anchored {
            self.origin_x = x;
            self.anchored = true;
            return Ok(0.0);
        }
        let displacement = x - self.origin_x;
        let sign: i8 = if displacement > 0.0 {
            1
        } else if displacement < 0.0 {
            -1
        } else {
            0
        };
        if sign == 0 {
            return Ok(0.0);
        }
        if self.direction == 0 {
            self.direction = sign;
            self.max_displacement = displacement.abs();
            return Ok(0.0);
        }
        if sign == self.direction {
            if displacement.abs() > self.max_displacement {
                self.max_displacement = displacement.abs();
            }
            return Ok(0.0);
        }
        // Direction crossing: bank the completed stroke.
        let max_displacement = self.max_displacement;
        let mut increment = max_displacement * max_displacement * d.swing_gain();
        if max_displacement >= d.bonus_threshold() {
            increment *= d.bonus_multiplier();
        }
        self.points += increment;
        self.direction = sign;
        self.max_displacement = displacement.abs();
        Ok(increment)
    }
}

impl Default for SwingAccumulator {
    fn default() -> SwingAccumulator {
        SwingAccumulator::new()
    }
}

/// Bank a whole sample stream at once. Bounded by
/// `SAMPLES_MAX`; the per-sample rule is `SwingAccumulator`.
pub fn accumulate(samples: &[f64], d: &Dynamics) -> Result<f64, CoinError> {
    if samples.len() > SAMPLES_MAX {
        return Err(CoinError::InvalidStep {
            reason: "sample stream exceeds SAMPLES_MAX",
        });
    }
    let mut acc = SwingAccumulator::new();
    for &x in samples {
        acc.feed(x, d)?;
    }
    Ok(acc.points())
}
