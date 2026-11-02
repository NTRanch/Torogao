//! The roll performance (Stage 2.2): the authored
//! `ParamEyeRoll` curve — anticipation, rise, hold,
//! return — with the ganged face driven from the one
//! driver, and the afterglow state it settles into.
//!
//! One driver, ganged (the cast-rigs law): R itself is
//! written to ParamEyeRoll; ParamEyeCross follows it by
//! the smoothstep class the template declares; the lids
//! and iris answer through the rigs' own keyforms. The
//! driver writes through the frame hook like the other
//! governed drivers, composing with the ambient
//! performance and with the gaze driver's blink (which
//! multiplies the eye-open channels the roll leaves
//! alone during the performance).
//!
//! After the return completes, the performance ends in
//! the afterglow state: the eye-open channels are held
//! at `AFTERGLOW_APERTURE` (0.80, not the 1.0 baseline)
//! until another performance takes the stage — the
//! settled, post-roll regard the PROGRAM names as this
//! stage's closing behaviour.

use torogao_app::scene::Scene;

/// The authored R-curve keys: (step at 60 fps, R). The
/// shape is the §4 record's: anticipation to 0.30 over
/// 3.5 s, a shoulder to 0.50, the rise to 1.00 in
/// 1.9 s, the hold at ≥ 0.85 for 6.4 s, and the 2.0 s
/// return to 0.10.
pub const ROLL_KEYS: [(u64, f32); 6] = [
    (0, 0.15),
    (210, 0.30),
    (252, 0.50),
    (366, 1.00),
    (750, 0.85),
    (870, 0.10),
];
/// The performance's length, steps (the last key).
pub const ROLL_STEPS: u64 = 870;
/// The afterglow aperture: where the eye-open channels
/// rest after the performance (the gate's ≈0.80).
pub const AFTERGLOW_APERTURE: f32 = 0.80;

/// The roll driver's state.
#[derive(Debug, Clone)]
pub struct RollDriver {
    /// Steps since the performance started.
    step: u64,
}

impl Default for RollDriver {
    fn default() -> RollDriver {
        RollDriver::new()
    }
}

impl RollDriver {
    /// A driver at the performance's first step.
    pub fn new() -> RollDriver {
        RollDriver { step: 0 }
    }

    /// Whether the R-curve is still playing (after it,
    /// the driver holds the afterglow).
    pub fn performing(&self) -> bool {
        self.step < ROLL_STEPS
    }

    /// The authored R at the driver's current step:
    /// piecewise-linear over `ROLL_KEYS`, clamped flat
    /// outside the key span.
    pub fn roll_at(step: u64) -> f32 {
        if step <= ROLL_KEYS[0].0 {
            return ROLL_KEYS[0].1;
        }
        for pair in ROLL_KEYS.windows(2) {
            let (start_step, start_value) = pair[0];
            let (end_step, end_value) = pair[1];
            if step <= end_step {
                let span = (end_step - start_step) as f32;
                let offset = (step - start_step) as f32 / span;
                return start_value + (end_value - start_value) * offset;
            }
        }
        ROLL_KEYS[ROLL_KEYS.len() - 1].1
    }

    /// The current R.
    pub fn roll(&self) -> f32 {
        Self::roll_at(self.step)
    }

    /// The crossing value for a roll of `r`: the
    /// template's smoothstep class — nothing below the
    /// anticipation's end, full at the peak.
    pub fn cross_at(r: f32) -> f32 {
        let t = ((r - 0.2) / 0.6).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Advances the driver one fixed step.
    pub fn step(&mut self) {
        self.step += 1;
    }
}

/// The parameter channels the roll driver writes,
/// resolved once against a scene.
#[derive(Debug, Clone, Copy, Default)]
pub struct RollChannels {
    /// ParamEyeCross (the ganged crossing).
    pub eye_cross: Option<usize>,
    /// ParamEyeLOpen (afterglow, left).
    pub eye_l_open: Option<usize>,
    /// ParamEyeROpen (afterglow, right).
    pub eye_r_open: Option<usize>,
    /// ParamEyeRoll (the driver itself).
    pub eye_roll: Option<usize>,
}

impl RollChannels {
    /// Resolves the channels against a scene's puppet.
    pub fn resolve(scene: &Scene) -> RollChannels {
        let index = |name: &str| scene.puppet.param_index(name);
        RollChannels {
            eye_cross: index("ParamEyeCross"),
            eye_l_open: index("ParamEyeLOpen"),
            eye_r_open: index("ParamEyeROpen"),
            eye_roll: index("ParamEyeRoll"),
        }
    }

    /// The channel names that did not resolve.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        for (name, channel) in [
            ("ParamEyeCross", self.eye_cross),
            ("ParamEyeLOpen", self.eye_l_open),
            ("ParamEyeROpen", self.eye_r_open),
            ("ParamEyeRoll", self.eye_roll),
        ] {
            if channel.is_none() {
                missing.push(name);
            }
        }
        missing
    }
}

/// Writes one step of the driver into the scene (the
/// frame hook's roll body): R and its ganged crossing
/// while performing; the afterglow aperture on the
/// eye-open channels once the performance completes.
pub fn apply(driver: &RollDriver, channels: &RollChannels, scene: &mut Scene) {
    if let Some(index) = channels.eye_roll {
        scene.puppet.params[index].value.x = driver.roll();
    }
    if let Some(index) = channels.eye_cross {
        scene.puppet.params[index].value.x = RollDriver::cross_at(driver.roll());
    }
    if !driver.performing() {
        for index in [channels.eye_l_open, channels.eye_r_open]
            .into_iter()
            .flatten()
        {
            scene.puppet.params[index].value.x = AFTERGLOW_APERTURE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_hits_its_keys() {
        for (step, value) in ROLL_KEYS {
            assert!(
                (RollDriver::roll_at(step) - value).abs() < 1e-6,
                "key ({step}, {value})"
            );
        }
        // Anticipation midpoint and hold flatness.
        assert!((RollDriver::roll_at(105) - 0.225).abs() < 1e-6);
        assert!(RollDriver::roll_at(500) >= 0.85);
        assert!((RollDriver::roll_at(870) - 0.10).abs() < 1e-6);
        assert!((RollDriver::roll_at(2000) - 0.10).abs() < 1e-6);
    }

    #[test]
    fn the_crossing_follows_its_class() {
        assert_eq!(RollDriver::cross_at(0.15), 0.0);
        assert_eq!(RollDriver::cross_at(1.0), 1.0);
        assert!((RollDriver::cross_at(0.5) - 0.5).abs() < 1e-6);
        let mid = RollDriver::cross_at(0.65);
        assert!(mid > 0.5 && mid < 1.0);
    }

    #[test]
    fn the_performance_ends_into_the_afterglow() {
        let mut driver = RollDriver::new();
        for _ in 0..ROLL_STEPS {
            assert!(driver.performing());
            driver.step();
        }
        assert!(!driver.performing());
    }
}
