//! The body-cycle driver (Stage 2.1, the wave): her
//! driven body cycle at the corpus frequency, with the
//! REPLICA jitter law authored in the driver — a fresh
//! period drawn per cycle from a seeded stream, so the
//! spacing breathes ±20–40 ms instead of ticking like a
//! metronome — and the secondary body motion authored
//! one frame behind the primary (the JG-03 discipline:
//! secondary lag ≤ 17 ms, authored, then *measured*
//! through the evaluation by the lag instrument).
//!
//! The secondary rides ParamBodyAngleX, trailing the
//! ParamBodyAngleZ primary: a two-axis sway whose twist
//! is the one-frame lag. The hair channels cannot carry
//! an authored secondary on these rigs — measured on
//! the fixtures, ParamHairSway* are physics *outputs*:
//! base values written to them are discarded by the
//! evaluation, and their evaluated internals track the
//! body at ~0 ms with a gain of ~1e-4 (effectively no
//! authored secondary motion in the rig data). The
//! gate test measures those channels with the same
//! instrument and reports the finding; visible hair
//! secondaries are a rig-physics authoring matter for
//! the cast-rigs workstream, not something a driver
//! can write past the physics.
//!
//! Like the gaze driver, the cycle driver owns no scene
//! state: the frame hook writes its channels as base
//! values after the player's render and before the
//! evaluation, on the same fixed 16.7 ms steps the
//! whole companion runs on (the PROGRAM's presentation
//! step law). The repertoire animations do not lane the
//! body channels, so the driver composes under the
//! ambient performance.
//!
//! The performance is finite: an amplitude envelope
//! (attack, sustain, release) over a governed number of
//! steps, after which the driver goes quiet and the
//! channels return to rest.

use torogao_app::scene::Scene;

/// The cycle's corpus frequency, Hz (PROGRAM.md §2.1).
pub const CYCLE_FREQUENCY_HZ: f64 = 2.31;
/// Primary amplitude, degrees of ParamBodyAngleZ.
pub const CYCLE_AMPLITUDE_DEG: f32 = 6.0;
/// Per-cycle period jitter, standard deviation in
/// milliseconds — inside the REPLICA ±20–40 ms class.
/// The drawn period is clamped to ±2.5 σ.
pub const PERIOD_JITTER_SIGMA_MS: f64 = 20.0;
/// The driver's fixed seed (the stream is part of the
/// performance, like the gaze driver's).
const DRIVER_SEED: u64 = 0x7761_7665_5f74_776f;
/// Envelope attack, steps at 60 fps (1 s).
pub const ATTACK_STEPS: u64 = 60;
/// Envelope release, steps at 60 fps (1 s).
pub const RELEASE_STEPS: u64 = 60;
/// Secondary gain: the trailing body axis follows the
/// primary's normalized drive one frame behind, at
/// this fraction of the primary amplitude.
const SECONDARY_GAIN: f32 = 0.5;
/// Steps of history the secondary cascade reads.
const HISTORY_STEPS: usize = 2;

/// The cycle driver's state.
#[derive(Debug, Clone)]
pub struct CycleDriver {
    rng: u64,
    /// Steps since the performance started.
    step: u64,
    /// The performance's total length, steps.
    total_steps: u64,
    /// The current cycle's period, seconds.
    period_seconds: f64,
    /// Phase within the current cycle, radians.
    phase: f64,
    /// The normalized drive history (newest last), for
    /// the secondary cascade.
    history: [f32; HISTORY_STEPS],
}

impl CycleDriver {
    /// A driver armed for a performance of `total_steps`
    /// fixed steps (attack and release included).
    pub fn new(total_steps: u64) -> CycleDriver {
        let mut driver = CycleDriver {
            rng: DRIVER_SEED,
            step: 0,
            total_steps,
            period_seconds: 1.0 / CYCLE_FREQUENCY_HZ,
            phase: 0.0,
            history: [0.0; HISTORY_STEPS],
        };
        driver.period_seconds = driver.draw_period();
        driver
    }

    /// Whether the performance is still running.
    pub fn active(&self) -> bool {
        self.step < self.total_steps
    }

    /// The authored amplitude envelope at `step`,
    /// normalized 0..1: smoothstep attack, sustain,
    /// smoothstep release to zero at the end.
    pub fn envelope_at(&self, step: u64) -> f64 {
        if step >= self.total_steps {
            return 0.0;
        }
        let attack = smoothstep(step as f64 / ATTACK_STEPS as f64);
        let release_start = self.total_steps.saturating_sub(RELEASE_STEPS);
        let release = if step > release_start {
            1.0 - smoothstep((step - release_start) as f64 / RELEASE_STEPS as f64)
        } else {
            1.0
        };
        attack.min(release)
    }

    /// This step's normalized drive (−1..1, envelope
    /// applied): what the secondary cascade follows.
    pub fn drive(&self) -> f32 {
        (self.envelope_at(self.step) * self.phase.sin()) as f32
    }

    /// Advances the driver one fixed step: phase accrues
    /// at the current cycle's period; at the wrap the
    /// next period is drawn (the REPLICA jitter).
    pub fn step(&mut self) {
        if !self.active() {
            return;
        }
        self.history.copy_within(1.., 0);
        self.history[HISTORY_STEPS - 1] = self.drive();
        self.phase += std::f64::consts::TAU * (1.0 / 60.0) / self.period_seconds;
        if self.phase >= std::f64::consts::TAU {
            self.phase -= std::f64::consts::TAU;
            self.period_seconds = self.draw_period();
        }
        self.step += 1;
    }

    /// Draws the next cycle's period: the corpus period
    /// jittered by a seeded normal (Box–Muller), clamped.
    fn draw_period(&mut self) -> f64 {
        let mean_ms = 1000.0 / CYCLE_FREQUENCY_HZ;
        let z = self.next_normal().clamp(-2.5, 2.5);
        (mean_ms + z * PERIOD_JITTER_SIGMA_MS) / 1000.0
    }

    /// A standard normal draw from the seeded stream
    /// (Box–Muller; the second variate is discarded so
    /// the stream stays a simple sequence of draws).
    fn next_normal(&mut self) -> f64 {
        let u1 = self.next_unit().max(1e-12);
        let u2 = self.next_unit();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    /// Draws a unit value in [0, 1) from the seeded stream.
    fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// SplitMix64: the driver's deterministic stream.
    fn next_u64(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// The parameter channels the cycle driver writes,
/// resolved once against a scene.
#[derive(Debug, Clone, Copy, Default)]
pub struct CycleChannels {
    /// ParamBodyAngleX (the secondary, trailing axis).
    pub body_x: Option<usize>,
    /// ParamBodyAngleZ (the primary sway).
    pub body_z: Option<usize>,
}

impl CycleChannels {
    /// Resolves the channels against a scene's puppet.
    pub fn resolve(scene: &Scene) -> CycleChannels {
        let index = |name: &str| scene.puppet.param_index(name);
        CycleChannels {
            body_x: index("ParamBodyAngleX"),
            body_z: index("ParamBodyAngleZ"),
        }
    }

    /// The channel names that did not resolve.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        for (name, channel) in [
            ("ParamBodyAngleX", self.body_x),
            ("ParamBodyAngleZ", self.body_z),
        ] {
            if channel.is_none() {
                missing.push(name);
            }
        }
        missing
    }
}

/// Writes one step of the driver into the scene (the
/// frame hook's cycle body): the primary as base
/// degrees, the secondary as the one-frame-lagged
/// cascade at its gain.
pub fn apply(driver: &CycleDriver, channels: &CycleChannels, scene: &mut Scene) {
    if let Some(index) = channels.body_z {
        scene.puppet.params[index].value.x = driver.drive() * CYCLE_AMPLITUDE_DEG;
    }
    // After `step`, history's newest slot holds the
    // drive the primary wrote one tick ago — the
    // one-frame lag (16.7 ms, inside the 17 ms bound).
    if let Some(index) = channels.body_x {
        scene.puppet.params[index].value.x =
            driver.history[HISTORY_STEPS - 1] * SECONDARY_GAIN * CYCLE_AMPLITUDE_DEG;
    }
}

/// The smoothstep easing on [0, 1].
fn smoothstep(value: f64) -> f64 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_attacks_sustains_and_releases() {
        let driver = CycleDriver::new(3600);
        assert_eq!(driver.envelope_at(0), 0.0);
        assert!((driver.envelope_at(ATTACK_STEPS) - 1.0).abs() < 1e-9);
        assert!((driver.envelope_at(1800) - 1.0).abs() < 1e-9);
        assert_eq!(driver.envelope_at(3600), 0.0);
        let near_end = driver.envelope_at(3599);
        assert!(near_end < 0.05, "release nearly complete: {near_end}");
    }

    #[test]
    fn drawn_periods_carry_the_replica_jitter() {
        let mut driver = CycleDriver::new(3600);
        let periods: Vec<f64> = (0..400).map(|_| driver.draw_period() * 1000.0).collect();
        let mean = periods.iter().sum::<f64>() / periods.len() as f64;
        let variance =
            periods.iter().map(|p| (p - mean) * (p - mean)).sum::<f64>() / periods.len() as f64;
        let sigma = variance.sqrt();
        assert!(
            (mean - 1000.0 / CYCLE_FREQUENCY_HZ).abs() < 2.0,
            "mean period {mean:.2} ms"
        );
        assert!(
            (10.0..=30.0).contains(&sigma),
            "period sigma {sigma:.2} ms outside the 10-30 ms band"
        );
    }

    #[test]
    fn the_driver_is_deterministic() {
        let run = || {
            let mut driver = CycleDriver::new(3600);
            let mut out = Vec::new();
            while driver.active() {
                driver.step();
                out.push(driver.drive());
            }
            out
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn the_secondary_history_lags_the_drive_by_one_frame() {
        let mut driver = CycleDriver::new(3600);
        for _ in 0..120 {
            // The core's order: step, then apply reads.
            // What step() pushes into the newest history
            // slot is exactly the drive the primary wrote
            // on the previous tick — a one-frame lag.
            let previous_drive = driver.drive();
            driver.step();
            assert_eq!(driver.history[HISTORY_STEPS - 1], previous_drive);
        }
    }
}
