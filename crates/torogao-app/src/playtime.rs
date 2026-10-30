//! The Playroom's Playtime surface (AMENDMENT-PLAYMODE
//! §2.3, §3.4, §8, as amended by D-226): the Persona
//! Playroom's binding for the Playtime mode with its
//! Blossom setting.
//!
//! Two bindings live here:
//!
//! - [`RealEngineView`] — PM-1's composition step: T4.1's
//!   gear surfaces (`torogao-gear`'s `PlayroomGearPanel`,
//!   the Remove All act, the §3.2 dashboard) were built
//!   against the `ArousalEngineView` trait with a labelled
//!   stub behind it. This adapter puts T4.2's **real**
//!   engine behind the same trait: the dashboard reads
//!   the integrated §5.2 vector, per-device inputs are
//!   the engine's own last outputs, and the trait's
//!   latch/reset verbs drive the real engine through the
//!   host's session ops. One store — the adapter owns the
//!   [`ArousalHost`] the mode session steps.
//! - [`PlayroomPlaytime`] — the mode panel binding:
//!   playmate entry through "Choose your Playmate", the
//!   mode panel (stage readout bound to live state —
//!   reads the engine and the mode, never estimates —
//!   device controls, the Hold control, pressure/volume/
//!   wetness readouts), the REC skin (§8.1), take
//!   start/stop, and Remove All Gear & Reset State
//!   behaving per §6.3.
//!
//! The egui shell draws these lines in the Playroom
//! panel; the binding itself is presentation-free so
//! headless gates can assert the surface against the
//! engine.

use torogao_gear::engine_iface::{ArousalEngineView, ArousalPhase, ArousalStateVector};
use torogao_session::Refusal;
use torogao_session::arousal::{
    ArousalHost, ArousalOp, CharacterProfile, DeviceOutput, DeviceSpec, EngineOutput, Phase,
    TickInputs,
};
use torogao_session::playmode::{
    DEVICE_ITEM_ID, ModeOutput, ModeTickInput, PlaytimeDefinition, PlaytimeMode, TakeRecord,
};

/// Maps the engine's phase onto the gear interface's
/// phase (the same §5.5 lifecycle in both vocabularies).
fn map_phase(phase: Phase) -> ArousalPhase {
    match phase {
        Phase::Baseline => ArousalPhase::Baseline,
        Phase::Stimulated => ArousalPhase::Stimulated,
        Phase::Culminating => ArousalPhase::Culminating,
        Phase::Taper => ArousalPhase::Taper,
        Phase::Afterglow => ArousalPhase::Afterglow,
        Phase::Refractory => ArousalPhase::Refractory,
    }
}

/// The real arousal engine behind T4.1's
/// `ArousalEngineView` interface (PM-1's composition
/// step). Owns the session host — the one store the
/// Playroom surface and the mode session both read.
#[derive(Debug)]
pub struct RealEngineView {
    host: ArousalHost,
    character: String,
    /// Specs applied through this view (the floor
    /// census + the latch verb's device list).
    applied: Vec<DeviceSpec>,
    /// Per-device rows from the most recent tick.
    last_devices: Vec<DeviceOutput>,
}

impl RealEngineView {
    /// A view over a fresh host, with the character's
    /// engine created under `profile`.
    pub fn new(character: &str, profile: CharacterProfile) -> RealEngineView {
        let mut host = ArousalHost::new();
        host.engine(character, profile);
        RealEngineView {
            host,
            character: character.to_string(),
            applied: Vec::new(),
            last_devices: Vec::new(),
        }
    }

    /// The underlying host (the §5.10 op log lives here).
    pub fn host(&self) -> &ArousalHost {
        &self.host
    }

    /// Mutable host access for the mode session (it
    /// steps the same store).
    pub fn host_mut(&mut self) -> &mut ArousalHost {
        &mut self.host
    }

    /// The character this view is bound to.
    pub fn character(&self) -> &str {
        &self.character
    }

    /// Applies an item through the host's session ops
    /// (§5.10) and remembers the spec for the view's
    /// censuses.
    pub fn apply_item(&mut self, spec: DeviceSpec) -> Result<(), Refusal> {
        self.host.apply_op(
            &self.character,
            &ArousalOp::ApplyItem { item: spec.clone() },
        )?;
        self.applied.push(spec);
        Ok(())
    }

    /// Advances the engine one tick, retaining the
    /// per-device rows the view reports.
    pub fn step(&mut self, inputs: &TickInputs) -> Result<EngineOutput, Refusal> {
        let out = self.host.step(&self.character, inputs)?;
        self.last_devices = out.devices.clone();
        Ok(out)
    }

    /// Notes a step taken by the mode session (which
    /// steps the same host) so the view's per-device
    /// rows stay current.
    pub fn note_step(&mut self, devices: &[DeviceOutput]) {
        self.last_devices = devices.to_vec();
    }

    /// The engine's current §5.2 vector, whole permille.
    pub fn vector(&self) -> Option<torogao_session::ArousalVector> {
        self.host.get(&self.character).map(|e| e.vector())
    }
}

impl ArousalEngineView for RealEngineView {
    fn state_vector(&self) -> ArousalStateVector {
        let Some(engine) = self.host.get(&self.character) else {
            return ArousalStateVector::baseline();
        };
        let v = engine.vector();
        ArousalStateVector {
            arousal: v.arousal,
            pleasure_charge: v.pleasure_charge,
            sensitivity: v.sensitivity,
            floor: v.floor,
            peak_input: v.peak_input,
            dose: v.dose,
            phase: map_phase(v.phase),
            culmination_latch: v.culmination_latch,
            episode_id: u64::from(v.episode),
            phase_ticks: v.phase_ticks,
            afterglow_level: v.afterglow_level,
        }
    }

    fn per_device_effective_input(&self) -> Vec<(String, u32)> {
        let mut rows: Vec<(String, u32)> = self
            .last_devices
            .iter()
            .map(|d| (d.item_id.clone(), d.effective_permille))
            .collect();
        rows.sort();
        rows
    }

    fn floor_contributions(&self) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = self
            .applied
            .iter()
            .filter(|spec| spec.arousal_floor > 0)
            .filter(|spec| {
                // A displaced item contributes no floor
                // (§5.4) — read from the last tick's rows.
                !self
                    .last_devices
                    .iter()
                    .find(|d| d.item_id == spec.item_id)
                    .is_some_and(|d| d.displaced)
            })
            .map(|spec| (spec.item_id.clone(), spec.arousal_floor))
            .collect();
        out.sort();
        out
    }

    fn mirror_active(&self) -> bool {
        // No mirror session exists in this build (T4.5
        // rides the HB-3 bridge); §3.4's step 2 issues
        // its stop only when this is true.
        false
    }

    fn latch_stimulation_zero(&mut self) {
        // §3.4 step 1, through the host's ops: every worn
        // vibratory device STOPs (its u is zero from the
        // next tick; the mirror generation bumps in the
        // calling act). Never refused for a worn device.
        let ids: Vec<String> = self
            .applied
            .iter()
            .filter(|s| s.is_vibratory())
            .map(|s| s.item_id.clone())
            .collect();
        for id in ids {
            let _ = self
                .host
                .apply_op(&self.character, &ArousalOp::StopDevice { item_id: id });
        }
    }

    fn reset_to_baseline(&mut self) {
        let _ = self.host.apply_op(&self.character, &ArousalOp::Reset);
    }
}

/// The Playroom Playtime panel binding: one
/// [`RealEngineView`] (the store) and one
/// [`PlaytimeMode`] (the mode state), for the playmate
/// chosen through "Choose your Playmate".
#[derive(Debug)]
pub struct PlayroomPlaytime {
    view: RealEngineView,
    mode: PlaytimeMode,
    /// Engine tick at which the running take started
    /// (the REC skin's honest clock).
    take_start_tick: Option<u64>,
    /// The most recently stopped take.
    last_take: Option<TakeRecord>,
    /// Completed takes this panel session.
    takes_completed: u64,
}

impl PlayroomPlaytime {
    /// A panel for `character`, not yet entered. Her
    /// engine is created under her mode profile.
    pub fn new(character: &str) -> Result<Self, Refusal> {
        let definition = PlaytimeDefinition::blossom();
        let profile = definition.character(character)?.profile.clone();
        Ok(PlayroomPlaytime {
            view: RealEngineView::new(character, profile),
            mode: PlaytimeMode::new(definition),
            take_start_tick: None,
            last_take: None,
            takes_completed: 0,
        })
    }

    /// "Choose your Playmate" (§2.3): switches the panel
    /// to another playmate. If the mode is entered, the
    /// current composition exits first (its wetness ends
    /// with its scene, §6.3); the new playmate's engine
    /// is created under her own profile.
    pub fn choose_playmate(&mut self, character: &str) -> Result<(), Refusal> {
        if self.mode.entered() {
            self.mode.exit(self.view.host_mut())?;
        }
        let profile = self.mode.definition().character(character)?.profile.clone();
        self.view = RealEngineView::new(character, profile);
        self.take_start_tick = None;
        Ok(())
    }

    /// The active playmate.
    pub fn playmate(&self) -> &str {
        self.view.character()
    }

    /// Enters the mode for the chosen playmate (§2.3).
    pub fn enter_mode(&mut self) -> Result<(), Refusal> {
        let character = self.view.character().to_string();
        self.mode.enter(self.view.host_mut(), &character)?;
        Ok(())
    }

    /// Exits the mode (session end for the composition).
    pub fn exit_mode(&mut self) -> Result<(), Refusal> {
        self.mode.exit(self.view.host_mut())?;
        self.take_start_tick = None;
        Ok(())
    }

    /// The Hold control (§3.3).
    pub fn set_hold(&mut self, on: bool) -> Result<(), Refusal> {
        self.mode.set_hold(self.view.host_mut(), on)?;
        Ok(())
    }

    /// The device amplitude control (§3.2).
    pub fn set_amplitude(&mut self, permille: u32) -> Result<(), Refusal> {
        self.mode.set_amplitude(self.view.host_mut(), permille)?;
        Ok(())
    }

    /// The physics layer's contact feed (§4.3/§4.5).
    pub fn set_contact(&mut self, contact_quality: u32, displaced: bool) -> Result<(), Refusal> {
        self.mode
            .set_contact(self.view.host_mut(), contact_quality, displaced)?;
        Ok(())
    }

    /// The REC skin toggle (§8.1): a view change only.
    pub fn set_rec_skin(&mut self, on: bool) -> Result<(), Refusal> {
        self.mode.set_rec_skin(self.view.host_mut(), on)?;
        Ok(())
    }

    /// Take start (§8.3).
    pub fn start_take(&mut self) -> Result<(), Refusal> {
        self.mode.start_take(self.view.host_mut())?;
        self.take_start_tick = self.view.host().get(self.playmate()).map(|e| e.tick());
        Ok(())
    }

    /// Take stop: the finished take record.
    pub fn stop_take(&mut self) -> Result<TakeRecord, Refusal> {
        let take = self.mode.stop_take(self.view.host_mut())?;
        self.take_start_tick = None;
        self.takes_completed += 1;
        self.last_take = Some(take.clone());
        Ok(take)
    }

    /// The most recently stopped take.
    pub fn last_take(&self) -> Option<&TakeRecord> {
        self.last_take.as_ref()
    }

    /// Remove All Gear & Reset State (§3.4 + §6.3): the
    /// standing control, behaving per the mode's law —
    /// engine reset and wetness zeroed in one act.
    pub fn remove_all_and_reset(&mut self) -> Result<(), Refusal> {
        self.mode.remove_all_and_reset(self.view.host_mut())?;
        self.take_start_tick = None;
        Ok(())
    }

    /// Advances the mode one fixed step.
    pub fn step(&mut self, input: &ModeTickInput) -> Result<ModeOutput, Refusal> {
        let out = self.mode.step(self.view.host_mut(), input)?;
        self.view.note_step(&out.engine.devices);
        Ok(out)
    }

    /// The mode session (read access for gates).
    pub fn mode(&self) -> &PlaytimeMode {
        &self.mode
    }

    /// The engine view (the gear-panel composition
    /// seam).
    pub fn view(&self) -> &RealEngineView {
        &self.view
    }

    /// Mutable engine view (gates bind the gear panel to
    /// the same store through this).
    pub fn view_mut(&mut self) -> &mut RealEngineView {
        &mut self.view
    }

    /// The REC skin's lines (§8.1), rendered from live
    /// take state: corner brackets; the breathing REC
    /// dot; a battery glyph that is an honest readout of
    /// take state (bars fill as the take runs — not a
    /// prop); centre focus brackets on the
    /// device/emitter region; ISO 300 and a **running**
    /// timecode — the take's real clock (the reference's
    /// frozen timecode is the skin's one deliberate,
    /// flagged deviation).
    pub fn rec_skin_lines(&self) -> Vec<String> {
        let tick = self
            .view
            .host()
            .get(self.playmate())
            .map_or(0, |e| e.tick());
        // The dot breathes at small amplitude: a 2 s
        // triangle cycle over the session tick.
        let phase = tick % 120;
        let level = if phase < 60 { phase } else { 120 - phase };
        let dot = if level >= 30 {
            "●"
        } else if level >= 15 {
            "◐"
        } else {
            "○"
        };
        // Battery: honest take state — recording fills
        // one bar per 30 s of take, six bars full at
        // 3 min; standby shows the empty glyph.
        let (battery, tc_ticks) = match self.take_start_tick {
            Some(start) => {
                let elapsed = tick.saturating_sub(start);
                let bars = (1 + elapsed / 1800).min(6) as usize;
                (
                    format!("[{}{}]", "|".repeat(bars), " ".repeat(6 - bars)),
                    elapsed,
                )
            }
            None => ("[      ]".to_string(), tick),
        };
        let frames = tc_ticks % 60;
        let seconds = (tc_ticks / 60) % 60;
        let minutes = (tc_ticks / 3600) % 60;
        let hours = tc_ticks / 216_000;
        vec![
            "+--------------------------------------------------------------+".to_string(),
            format!("  {dot} REC                                                {battery}"),
            format!("              [ + ]  focus {DEVICE_ITEM_ID} / body.perineum"),
            format!("  ISO 300   TC {hours:02}:{minutes:02}:{seconds:02}:{frames:02}"),
            "+--------------------------------------------------------------+".to_string(),
        ]
    }

    /// The mode panel's lines (Persona register): every
    /// readout is read from live engine/mode state at
    /// call time — the stage from the mode's overlay,
    /// the vector from the engine, the readouts from the
    /// mode's extensions. Nothing is estimated.
    pub fn panel_lines(&self) -> Vec<String> {
        let definition = self.mode.definition();
        let mut lines = vec![format!(
            "Playtime — {} setting · playmate {}",
            definition.setting_id,
            self.playmate()
        )];
        if !self.mode.entered() {
            lines.push("stage Setup — choose your playmate, then enter the mode".to_string());
            return lines;
        }
        let v = self
            .view
            .vector()
            .unwrap_or(torogao_session::ArousalVector {
                arousal: 0,
                pleasure_charge: 0,
                sensitivity: 1000,
                floor: 0,
                peak_input: 0,
                dose: 0,
                phase: Phase::Baseline,
                culmination_latch: false,
                episode: 1,
                phase_ticks: 0,
                afterglow_level: 150,
            });
        lines.push(format!(
            "stage {} (engine phase {})",
            self.mode.stage().name(),
            v.phase.token()
        ));
        lines.push(format!(
            "A {}  Q {}  S {}  F {}  P {}  D {}",
            v.arousal, v.pleasure_charge, v.sensitivity, v.floor, v.peak_input, v.dose
        ));
        lines.push(format!(
            "device {} amplitude {} (user) · hold {}",
            DEVICE_ITEM_ID,
            self.mode.user_amplitude(),
            if self.mode.hold_engaged() {
                "on"
            } else {
                "off"
            }
        ));
        if self.mode.hold_engaged() {
            let i_hold = definition
                .governor
                .i_hold_permille(v.pleasure_charge, v.sensitivity);
            lines.push(format!(
                "governor I_hold {} at Q {} S {}",
                i_hold, v.pleasure_charge, v.sensitivity
            ));
        }
        let wet = self.mode.wetness();
        lines.push(format!(
            "pressure {}  episode volume {} units  wetness body {} fixture {} floor {}",
            self.mode.pressure_permille(),
            self.mode.episode_volume_micro() / 1_000_000,
            wet.permille("body.lower"),
            wet.permille("fixture.seat"),
            wet.permille("floor.pool"),
        ));
        lines.push(format!(
            "take {} · rec skin {}",
            if self.mode.take_active() {
                "recording"
            } else {
                "idle"
            },
            if self.mode.rec_on() { "on" } else { "off" }
        ));
        if self.mode.rec_on() {
            lines.extend(self.rec_skin_lines());
        }
        lines
    }
}
