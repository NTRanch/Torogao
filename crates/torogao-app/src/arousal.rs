//! Playroom arousal dashboard (AMENDMENT-T4 §3.2, T4.2):
//! the panel's binding to the real arousal engine in
//! `torogao-session`. The dashboard owns an [`ArousalHost`]
//! for the Playroom character and the worked-example gear
//! of the brief's §7 walkthrough — a clamp pair holding
//! the 320 floor and one vibratory device — and renders
//! its readouts from live engine state: the §5.2 vector,
//! the per-device drive rows, the floor, the phase, and
//! the last culmination. Nothing here simulates: every
//! number on the panel is the engine's own state, and the
//! controls are the §5.10 session ops (apply / remove /
//! set wave / stop / remove-all), which the host validates
//! and logs.
//!
//! The egui shell draws these lines in the Playroom panel;
//! the binding itself is presentation-free so headless
//! gates can assert the dashboard against the engine.

use torogao_session::Refusal;
use torogao_session::arousal::{
    ArousalEvent, ArousalHost, ArousalOp, CharacterProfile, DeviceOutput, DeviceSpec, EngineOutput,
    Phase, TickInputs, WaveSpec,
};

/// The Playroom character the dashboard drives.
pub const PLAYROOM_CHARACTER: &str = "prima";

/// The worked-example clamp pair (brief §7): presence at
/// the nipple site holding the 320 arousal floor.
pub fn worked_example_clamps() -> DeviceSpec {
    DeviceSpec {
        item_id: "clamp-pair".to_string(),
        site: "nipple".to_string(),
        wave: None,
        contact_quality: 1000,
        arousal_floor: 320,
    }
}

/// The worked-example vibratory device (brief §7).
pub fn worked_example_vibrator(amplitude_permille: u32) -> DeviceSpec {
    DeviceSpec {
        item_id: "vibe".to_string(),
        site: "perineum".to_string(),
        wave: Some(WaveSpec::constant(amplitude_permille)),
        contact_quality: 1000,
        arousal_floor: 0,
    }
}

/// The last culmination the panel reports (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastCulmination {
    /// Tick the culmination fired on.
    pub tick: u64,
    /// Recorded tier (1..=4).
    pub tier: u8,
    /// The determining P, permille.
    pub p_permille: u32,
    /// 1-based episode number.
    pub episode: u32,
}

/// The Playroom arousal dashboard: one host, one
/// character, the worked-example gear on the controls.
#[derive(Debug)]
pub struct PlayroomArousal {
    host: ArousalHost,
    last_culmination: Option<LastCulmination>,
    /// Per-device rows from the most recent tick — the
    /// panel's per-device readouts (§3.2).
    last_devices: Vec<DeviceOutput>,
}

impl Default for PlayroomArousal {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayroomArousal {
    /// A dashboard for the Playroom character with the
    /// default §5.2 profile.
    pub fn new() -> Self {
        let mut host = ArousalHost::new();
        host.engine(PLAYROOM_CHARACTER, CharacterProfile::default());
        PlayroomArousal {
            host,
            last_culmination: None,
            last_devices: Vec::new(),
        }
    }

    /// The underlying host (the §5.10 op log lives here).
    pub fn host(&self) -> &ArousalHost {
        &self.host
    }

    /// Apply the worked-example clamp pair (§5.10 op).
    pub fn apply_clamps(&mut self) -> Result<(), Refusal> {
        self.host
            .apply_op(
                PLAYROOM_CHARACTER,
                &ArousalOp::ApplyItem {
                    item: worked_example_clamps(),
                },
            )
            .map(|_| ())
    }

    /// Apply the worked-example vibrator at an amplitude
    /// (§5.10 op).
    pub fn apply_vibrator(&mut self, amplitude_permille: u32) -> Result<(), Refusal> {
        self.host
            .apply_op(
                PLAYROOM_CHARACTER,
                &ArousalOp::ApplyItem {
                    item: worked_example_vibrator(amplitude_permille),
                },
            )
            .map(|_| ())
    }

    /// Change the vibrator's intensity (§5.10 op).
    pub fn set_vibrator_amplitude(&mut self, amplitude_permille: u32) -> Result<(), Refusal> {
        self.host
            .apply_op(
                PLAYROOM_CHARACTER,
                &ArousalOp::SetWave {
                    item_id: "vibe".to_string(),
                    wave: WaveSpec::constant(amplitude_permille),
                },
            )
            .map(|_| ())
    }

    /// The STOP control (§5.8): the vibrator's drive is
    /// zero from the next tick. Never refused for a worn
    /// device; unknown devices are a §5.10 refusal.
    pub fn stop_vibrator(&mut self) -> Result<(), Refusal> {
        self.host
            .apply_op(
                PLAYROOM_CHARACTER,
                &ArousalOp::StopDevice {
                    item_id: "vibe".to_string(),
                },
            )
            .map(|_| ())
    }

    /// Remove-all + engine reset (§3.4 / §5.10): the
    /// unconditional safety exit.
    pub fn remove_all(&mut self) -> Result<(), Refusal> {
        self.host
            .apply_op(PLAYROOM_CHARACTER, &ArousalOp::RemoveAllAndReset)
            .map(|_| ())
    }

    /// Advance the engine one tick with the Playroom's
    /// per-tick inputs, tracking the last culmination for
    /// the panel.
    pub fn step(&mut self, inputs: &TickInputs) -> Result<EngineOutput, Refusal> {
        let out = self.host.step(PLAYROOM_CHARACTER, inputs)?;
        self.last_devices = out.devices.clone();
        let tick = self.host.get(PLAYROOM_CHARACTER).map_or(0, |e| e.tick());
        for event in &out.events {
            if let ArousalEvent::Culmination {
                tier,
                p_permille,
                episode,
            } = event
            {
                self.last_culmination = Some(LastCulmination {
                    tick,
                    tier: *tier,
                    p_permille: *p_permille,
                    episode: *episode,
                });
            }
        }
        Ok(out)
    }

    /// The last culmination, if one has fired this session.
    pub fn last_culmination(&self) -> Option<LastCulmination> {
        self.last_culmination
    }

    /// The current phase, for the panel's phase readout.
    pub fn phase(&self) -> Phase {
        self.host
            .get(PLAYROOM_CHARACTER)
            .map_or(Phase::Baseline, |e| e.phase())
    }

    /// The §3.2 dashboard readouts, rendered from live
    /// engine state: vector, per-device drive rows,
    /// floor, phase, last culmination. The egui panel
    /// draws exactly these lines; headless gates assert
    /// them against the engine.
    pub fn dashboard_lines(&self) -> Vec<String> {
        let Some(engine) = self.host.get(PLAYROOM_CHARACTER) else {
            return vec!["arousal: no engine".to_string()];
        };
        let v = engine.vector();
        let mut lines = vec![
            format!(
                "A {}  Q {}  S {}  F {}  P {}  D {}",
                v.arousal, v.pleasure_charge, v.sensitivity, v.floor, v.peak_input, v.dose
            ),
            format!(
                "phase {}  episode {}  latch {}",
                v.phase.token(),
                v.episode,
                u8::from(v.culmination_latch)
            ),
        ];
        for out in &self.last_devices {
            lines.push(format!(
                "device {} u {} q {} effective {} on {}",
                out.item_id,
                out.u_permille,
                out.contact_quality,
                out.effective_permille,
                u8::from(out.on)
            ));
        }
        match self.last_culmination {
            Some(last) => lines.push(format!(
                "last culmination tick {} tier {} P {} episode {}",
                last.tick, last.tier, last.p_permille, last.episode
            )),
            None => lines.push("last culmination none".to_string()),
        }
        lines
    }
}
