//! The Addition G gaze driver: gaze-follow, idle
//! saccades, and blink discipline as one governed driver
//! inside the companion's evaluation (Stage 1.2).
//!
//! The driver owns no scene state. Each fixed step it
//! solves a gaze direction (yaw/pitch, degrees) from its
//! target — the pointer when one is present (attention),
//! a deterministic idle schedule when none is — and the
//! frame hook writes the solution into the puppet through
//! the parameter combinators, so it composes with the
//! playing performance instead of fighting its lanes:
//!
//! - `ParamGazeX` / `ParamGazeY` (never laned): the
//!   eyeball share, written as the base value.
//! - `ParamAngleX` / `ParamAngleY`: the head share, an
//!   Additive push on top of the authored angle.
//! - `ParamEyeLOpen` / `ParamEyeROpen`: the blink, a
//!   Multiplicative push (1 = open, 0 = shut) over the
//!   standing aperture.
//!
//! Decomposition law (declared, and the same law the
//! gate instrument reads back): the head takes
//! `HEAD_SHARE` of the direction, clamped to
//! `HEAD_MAX_DEG`; the eyeballs take the remainder,
//! clamped to `EYE_TRAVEL_DEG` (the ParamGaze range
//! −1..1 maps to ±`EYE_TRAVEL_DEG`). Evaluated gaze =
//! head + eye, by construction. The governed cast rigs
//! carry no ParamGaze channels (their eyeball travel
//! lives in the authored roll/cross system, Stage 2.2's
//! domain), so on them the decomposition degrades to
//! head gaze: the head takes the whole direction and
//! the channel resolution reports the gap at load.
//!
//! Determinism: every "random" choice (idle saccade
//! timing and targets, blink cadence) comes from a
//! SplitMix64 stream with a fixed seed, advanced only by
//! fixed steps — a fixed input script replays to the
//! same driver state, bit for bit.

use torogao_app::scene::Scene;
use torogao_engine::MergeMode;

/// The driver's fixed seed (the stream is part of the
/// performance: change it and every idle pattern moves).
const DRIVER_SEED: u64 = 0x6761_7a65_5f76_6f77;
/// Eyeball travel the ParamGaze range −1..1 maps to,
/// degrees.
pub const EYE_TRAVEL_DEG: f64 = 12.0;
/// The head's share of a gaze direction.
const HEAD_SHARE: f64 = 0.55;
/// Head yaw clamp, degrees (inside the rigs' ±30 range).
const HEAD_MAX_YAW_DEG: f64 = 18.0;
/// Head pitch clamp, degrees.
const HEAD_MAX_PITCH_DEG: f64 = 10.0;
/// Total gaze clamps, degrees (the rigs' angle range).
const YAW_MAX_DEG: f64 = 30.0;
const PITCH_MAX_DEG: f64 = 22.0;
/// Rest direction (yaw, pitch), degrees: level, a touch
/// down — her neutral regard.
const REST_DEG: (f64, f64) = (0.0, -2.0);
/// Distance that triggers a saccade instead of pursuit,
/// degrees.
const SACCADE_THRESHOLD_DEG: f64 = 2.0;
/// Saccade duration law: base + per-degree, seconds,
/// clamped to the stated band.
const SACCADE_BASE_SECONDS: f64 = 0.10;
const SACCADE_PER_DEGREE_SECONDS: f64 = 0.006;
const SACCADE_MAX_SECONDS: f64 = 0.30;
/// Smooth-pursuit rate per step for sub-threshold drift.
const PURSUIT_RATE: f64 = 0.35;
/// Idle saccade interval band, steps at 60 fps.
const IDLE_INTERVAL_STEPS: (u64, u64) = (84, 192);
/// Idle saccade target spread around rest, degrees.
const IDLE_SPREAD_DEG: (f64, f64) = (8.0, 5.0);
/// Blink interval band, steps at 60 fps.
const BLINK_INTERVAL_STEPS: (u64, u64) = (132, 276);
/// Blink length, steps (0.15 s at 60 fps).
const BLINK_STEPS: u64 = 9;
/// Quiet-mode salience: gaze targets blend this far
/// toward rest, cadences double (PROGRAM.md §3.3).
const QUIET_SALIENCE: f64 = 0.35;

/// One saccade in flight.
#[derive(Debug, Clone, Copy)]
struct Saccade {
    from: (f64, f64),
    to: (f64, f64),
    step: u64,
    steps: u64,
}

/// The gaze driver's state (advanced one fixed step at a
/// time by the companion core).
#[derive(Debug, Clone)]
pub struct GazeDriver {
    rng: u64,
    /// Whether the rig on stage carries the ParamGaze
    /// eyeball channels. The governed cast rigs do not —
    /// their eyeball travel lives in the authored
    /// roll/cross system — so on them the head takes the
    /// whole direction (head gaze) and the eyeball share
    /// is zero. Set from the channel resolution at every
    /// scene install.
    eye_channels: bool,
    /// Current evaluated direction (yaw, pitch), degrees.
    current: (f64, f64),
    /// The pointer's direction when present (attention).
    pointer_target: Option<(f64, f64)>,
    /// The idle schedule's standing target.
    idle_target: (f64, f64),
    steps_to_idle_saccade: u64,
    saccade: Option<Saccade>,
    steps_to_blink: u64,
    blink_step: Option<u64>,
    quiet: bool,
}

impl Default for GazeDriver {
    fn default() -> GazeDriver {
        GazeDriver::new()
    }
}

impl GazeDriver {
    /// A driver at rest, schedules armed from the seed.
    pub fn new() -> GazeDriver {
        let mut driver = GazeDriver {
            rng: DRIVER_SEED,
            eye_channels: false,
            current: REST_DEG,
            pointer_target: None,
            idle_target: REST_DEG,
            steps_to_idle_saccade: 0,
            saccade: None,
            steps_to_blink: 0,
            blink_step: None,
            quiet: false,
        };
        driver.steps_to_idle_saccade = driver.band(IDLE_INTERVAL_STEPS);
        driver.steps_to_blink = driver.band(BLINK_INTERVAL_STEPS);
        driver
    }

    /// Sets (or clears) the pointer's gaze direction, in
    /// degrees — the core converts surface positions to
    /// directions; the driver works in angles only.
    pub fn set_pointer_target(&mut self, target: Option<(f64, f64)>) {
        self.pointer_target = target.map(|(yaw, pitch)| {
            (
                yaw.clamp(-YAW_MAX_DEG, YAW_MAX_DEG),
                pitch.clamp(-PITCH_MAX_DEG, PITCH_MAX_DEG),
            )
        });
    }

    /// Sets quiet mode (focus discipline: reduced
    /// salience, slower cadence, reactions suppressed by
    /// the core).
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// Whether quiet mode is on.
    pub fn quiet(&self) -> bool {
        self.quiet
    }

    /// The current evaluated gaze direction (yaw, pitch)
    /// in degrees — the gate instrument's read.
    pub fn gaze_degrees(&self) -> (f64, f64) {
        self.current
    }

    /// The current blink factor (1 = fully open).
    pub fn blink_factor(&self) -> f64 {
        match self.blink_step {
            None => 1.0,
            Some(step) => {
                let phase = step as f64 / BLINK_STEPS as f64;
                1.0 - smoothstep(0.0, 0.35, phase) * (1.0 - smoothstep(0.55, 1.0, phase))
            }
        }
    }

    /// Records whether the rig on stage carries the
    /// ParamGaze eyeball channels (the core calls this at
    /// every scene install, from the channel resolution).
    pub fn set_eye_channels(&mut self, present: bool) {
        self.eye_channels = present;
    }

    /// The head share of the current direction, degrees.
    /// With eyeball channels the head takes `HEAD_SHARE`
    /// (clamped); without them the head takes the whole
    /// direction — the direction itself is already
    /// clamped to the rigs' angle range at intake.
    pub fn head_degrees(&self) -> (f64, f64) {
        if !self.eye_channels {
            return self.current;
        }
        (
            (self.current.0 * HEAD_SHARE).clamp(-HEAD_MAX_YAW_DEG, HEAD_MAX_YAW_DEG),
            (self.current.1 * HEAD_SHARE).clamp(-HEAD_MAX_PITCH_DEG, HEAD_MAX_PITCH_DEG),
        )
    }

    /// The eyeball share, normalized to the ParamGaze
    /// range −1..1 (zero when the rig has no eyeball
    /// channels: the head carries the direction alone).
    pub fn eye_normalized(&self) -> (f64, f64) {
        if !self.eye_channels {
            return (0.0, 0.0);
        }
        let head = self.head_degrees();
        (
            ((self.current.0 - head.0) / EYE_TRAVEL_DEG).clamp(-1.0, 1.0),
            ((self.current.1 - head.1) / EYE_TRAVEL_DEG).clamp(-1.0, 1.0),
        )
    }

    /// Advances the driver one fixed step.
    pub fn step(&mut self) {
        let desired = self.desired_target();
        self.follow(desired);
        self.schedule_idle();
        self.schedule_blink();
    }

    /// The direction the driver is steering toward this
    /// step: the pointer's while one is present, else the
    /// idle schedule's; quiet mode blends toward rest.
    fn desired_target(&self) -> (f64, f64) {
        let target = self.pointer_target.unwrap_or(self.idle_target);
        if self.quiet {
            (
                REST_DEG.0 + (target.0 - REST_DEG.0) * QUIET_SALIENCE,
                REST_DEG.1 + (target.1 - REST_DEG.1) * QUIET_SALIENCE,
            )
        } else {
            target
        }
    }

    /// Saccade / pursuit toward `desired`.
    fn follow(&mut self, desired: (f64, f64)) {
        if let Some(mut saccade) = self.saccade.take() {
            saccade.step += 1;
            if saccade.step >= saccade.steps {
                self.current = saccade.to;
            } else {
                let phase = smoothstep(0.0, 1.0, saccade.step as f64 / saccade.steps as f64);
                self.current = (
                    saccade.from.0 + (saccade.to.0 - saccade.from.0) * phase,
                    saccade.from.1 + (saccade.to.1 - saccade.from.1) * phase,
                );
                self.saccade = Some(saccade);
            }
            return;
        }
        let distance = angular_distance(self.current, desired);
        if distance > SACCADE_THRESHOLD_DEG {
            let seconds = (SACCADE_BASE_SECONDS + SACCADE_PER_DEGREE_SECONDS * distance)
                .min(SACCADE_MAX_SECONDS);
            let steps = (seconds * 60.0).round().max(1.0) as u64;
            self.saccade = Some(Saccade {
                from: self.current,
                to: desired,
                step: 0,
                steps,
            });
        } else {
            self.current = (
                self.current.0 + (desired.0 - self.current.0) * PURSUIT_RATE,
                self.current.1 + (desired.1 - self.current.1) * PURSUIT_RATE,
            );
        }
    }

    /// The idle schedule: on expiry, pick the next idle
    /// target around rest and re-arm (twice as slow when
    /// quiet). A pointer present suspends the schedule —
    /// attention owns the eyes.
    fn schedule_idle(&mut self) {
        if self.pointer_target.is_some() {
            return;
        }
        if self.steps_to_idle_saccade > 0 {
            self.steps_to_idle_saccade -= 1;
            return;
        }
        let yaw = REST_DEG.0 + (self.next_unit() * 2.0 - 1.0) * IDLE_SPREAD_DEG.0;
        let pitch = REST_DEG.1 + (self.next_unit() * 2.0 - 1.0) * IDLE_SPREAD_DEG.1;
        self.idle_target = (yaw, pitch);
        let mut interval = self.band(IDLE_INTERVAL_STEPS);
        if self.quiet {
            interval *= 2;
        }
        self.steps_to_idle_saccade = interval;
    }

    /// The blink schedule: on expiry, run one blink
    /// profile and re-arm (twice as slow when quiet).
    fn schedule_blink(&mut self) {
        if let Some(step) = self.blink_step {
            let next = step + 1;
            self.blink_step = if next >= BLINK_STEPS {
                None
            } else {
                Some(next)
            };
            return;
        }
        if self.steps_to_blink > 0 {
            self.steps_to_blink -= 1;
            return;
        }
        self.blink_step = Some(0);
        let mut interval = self.band(BLINK_INTERVAL_STEPS);
        if self.quiet {
            interval *= 2;
        }
        self.steps_to_blink = interval;
    }

    /// Draws an integer in `band` from the seeded stream.
    fn band(&mut self, band: (u64, u64)) -> u64 {
        band.0 + (self.next_u64() % (band.1 - band.0 + 1))
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

/// The parameter channels the driver writes, resolved
/// once against a scene. A channel the rig does not
/// carry is absent and the driver writes nothing there —
/// the resolution is reported, never papered over.
#[derive(Debug, Clone, Copy, Default)]
pub struct GazeChannels {
    /// ParamAngleX index (head yaw).
    pub angle_x: Option<usize>,
    /// ParamAngleY index (head pitch).
    pub angle_y: Option<usize>,
    /// ParamEyeLOpen index (blink, left).
    pub eye_l_open: Option<usize>,
    /// ParamEyeROpen index (blink, right).
    pub eye_r_open: Option<usize>,
    /// ParamGazeX index (eyeball yaw).
    pub gaze_x: Option<usize>,
    /// ParamGazeY index (eyeball pitch).
    pub gaze_y: Option<usize>,
}

impl GazeChannels {
    /// Resolves the channels against a scene's puppet.
    pub fn resolve(scene: &Scene) -> GazeChannels {
        let index = |name: &str| scene.puppet.param_index(name);
        GazeChannels {
            angle_x: index("ParamAngleX"),
            angle_y: index("ParamAngleY"),
            eye_l_open: index("ParamEyeLOpen"),
            eye_r_open: index("ParamEyeROpen"),
            gaze_x: index("ParamGazeX"),
            gaze_y: index("ParamGazeY"),
        }
    }

    /// The channel names that did not resolve, in
    /// declaration order — the load-time gap report.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        for (name, channel) in [
            ("ParamAngleX", self.angle_x),
            ("ParamAngleY", self.angle_y),
            ("ParamEyeLOpen", self.eye_l_open),
            ("ParamEyeROpen", self.eye_r_open),
            ("ParamGazeX", self.gaze_x),
            ("ParamGazeY", self.gaze_y),
        ] {
            if channel.is_none() {
                missing.push(name);
            }
        }
        missing
    }
}

/// Writes one step of the driver into the scene through
/// the parameter combinators (the frame hook's body).
/// Runs after the player's frame render and before the
/// engine evaluation, per `PlayHost::step_hooked`.
pub fn apply(driver: &GazeDriver, channels: &GazeChannels, scene: &mut Scene) {
    let (eye_x, eye_y) = driver.eye_normalized();
    if let Some(index) = channels.gaze_x {
        scene.puppet.params[index].value.x = eye_x as f32;
    }
    if let Some(index) = channels.gaze_y {
        scene.puppet.params[index].value.x = eye_y as f32;
    }
    let (head_yaw, head_pitch) = driver.head_degrees();
    if let Some(index) = channels.angle_x {
        scene.puppet.params[index].push_offset_axis(0, head_yaw as f32, MergeMode::Additive, 1.0);
    }
    if let Some(index) = channels.angle_y {
        scene.puppet.params[index].push_offset_axis(0, head_pitch as f32, MergeMode::Additive, 1.0);
    }
    let blink = driver.blink_factor() as f32;
    if blink < 1.0 {
        for index in [channels.eye_l_open, channels.eye_r_open]
            .into_iter()
            .flatten()
        {
            scene.puppet.params[index].push_offset_axis(0, blink, MergeMode::Multiplicative, 1.0);
        }
    }
}

/// Angular distance between two directions, degrees.
fn angular_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

/// The smoothstep easing (the saccade profile and the
/// blink edges share it).
fn smoothstep(edge0: f64, edge1: f64, value: f64) -> f64 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_far_target_is_reached_by_saccade_and_held() {
        let mut driver = GazeDriver::new();
        driver.set_pointer_target(Some((20.0, 6.0)));
        for _ in 0..30 {
            driver.step();
        }
        let (yaw, pitch) = driver.gaze_degrees();
        assert!((yaw - 20.0).abs() < 2.0, "yaw {yaw}");
        assert!((pitch - 6.0).abs() < 2.0, "pitch {pitch}");
    }

    #[test]
    fn decomposition_never_exceeds_the_travels() {
        let mut driver = GazeDriver::new();
        driver.set_eye_channels(true);
        driver.set_pointer_target(Some((30.0, 22.0)));
        for _ in 0..40 {
            driver.step();
        }
        let (head_yaw, head_pitch) = driver.head_degrees();
        assert!(head_yaw.abs() <= HEAD_MAX_YAW_DEG);
        assert!(head_pitch.abs() <= HEAD_MAX_PITCH_DEG);
        let (eye_x, eye_y) = driver.eye_normalized();
        assert!(eye_x.abs() <= 1.0 && eye_y.abs() <= 1.0);
    }

    #[test]
    fn the_driver_is_deterministic_under_a_fixed_script() {
        let run = || {
            let mut driver = GazeDriver::new();
            let mut trace = Vec::new();
            for step in 0..600 {
                if step == 100 {
                    driver.set_pointer_target(Some((-14.0, 4.0)));
                }
                if step == 300 {
                    driver.set_pointer_target(None);
                }
                driver.step();
                trace.push(driver.gaze_degrees());
            }
            trace
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn blinks_close_and_reopen_on_the_seeded_cadence() {
        let mut driver = GazeDriver::new();
        let mut saw_closure = false;
        for _ in 0..1200 {
            driver.step();
            let factor = driver.blink_factor();
            assert!((0.0..=1.0).contains(&factor));
            if factor < 0.05 {
                saw_closure = true;
            }
        }
        assert!(saw_closure, "a blink must close within 20 s");
    }

    #[test]
    fn without_eyeball_channels_the_head_takes_the_whole_direction() {
        let mut driver = GazeDriver::new();
        driver.set_pointer_target(Some((20.0, 6.0)));
        for _ in 0..30 {
            driver.step();
        }
        let (yaw, pitch) = driver.gaze_degrees();
        assert_eq!(driver.head_degrees(), (yaw, pitch));
        assert_eq!(driver.eye_normalized(), (0.0, 0.0));
    }

    #[test]
    fn quiet_mode_pulls_the_target_toward_rest() {
        let mut loud = GazeDriver::new();
        loud.set_pointer_target(Some((24.0, 0.0)));
        let mut quiet = GazeDriver::new();
        quiet.set_quiet(true);
        quiet.set_pointer_target(Some((24.0, 0.0)));
        for _ in 0..60 {
            loud.step();
            quiet.step();
        }
        assert!(quiet.gaze_degrees().0 < loud.gaze_degrees().0);
        assert!(quiet.gaze_degrees().0 > 5.0, "still follows, at salience");
    }
}
