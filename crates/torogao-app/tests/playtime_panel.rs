//! PM-1 scope items 6–8 at the app surface: the gear
//! panel binds the REAL arousal engine (the composition
//! step — the T4.1 surfaces were written against a
//! labelled stub), and the Playtime panel drives the
//! mode end to end with readouts bound to live state.

use torogao_app::playtime::{PlayroomPlaytime, RealEngineView};
use torogao_gear::examples::{clamp_pair_document, vibrator_document};
use torogao_gear::playtime_pack::starter_pack_documents;
use torogao_gear::surfaces::PlayroomGearPanel;
use torogao_gear::worn::OpEvent;
use torogao_session::CharacterProfile;
use torogao_session::arousal::{DeviceSpec, TickInputs, WaveKind, WaveSpec};
use torogao_session::playmode::{ModeTickInput, Stage};

fn vibe_spec(id: &str, site: &str, amplitude: u32, floor: u32) -> DeviceSpec {
    DeviceSpec {
        item_id: id.to_string(),
        site: site.to_string(),
        wave: Some(WaveSpec {
            kind: WaveKind::Constant,
            amplitude_permille: amplitude,
            frequency_millihz: 0,
            duty_permille: 0,
            pattern: Vec::new(),
            pattern_step_ticks: 1,
            phase_ticks: 0,
        }),
        contact_quality: 1000,
        arousal_floor: floor,
    }
}

fn idle_inputs() -> TickInputs {
    TickInputs {
        touch: None,
        suction_peak: false,
        spray_rate: None,
        face_contact: false,
    }
}

#[test]
fn gear_panel_binds_the_real_engine() {
    let mut view = RealEngineView::new("prima", CharacterProfile::default());
    view.apply_item(vibe_spec("vibe.a", "pelvis", 1000, 0))
        .unwrap();
    // Drive the real engine: the dashboard must read the
    // integrated state, not the stub's baseline.
    for t in 0..3600u64 {
        let mut inputs = idle_inputs();
        if (t / 600) % 2 == 0 {
            inputs.touch = Some(("pelvis".to_string(), 500));
        }
        view.step(&inputs).unwrap();
    }
    let library = vec![clamp_pair_document(), vibrator_document(1000)];
    let mut panel = PlayroomGearPanel::with_engine(library, Box::new(view));
    let board = panel.dashboard();
    assert!(
        board.state.arousal > 0 && board.state.dose > 0,
        "the real engine integrated input: A {} D {}",
        board.state.arousal,
        board.state.dose
    );
    assert_eq!(
        board.per_device_effective_input,
        vec![("vibe.a".to_string(), 1000)],
        "per-device rows are the engine's own outputs"
    );

    // The Remove All act drives the real engine through
    // the trait: stimulation latched zero, then the
    // engine reset — §3.4 order, with no mirror step
    // (nothing is mirrored in this build).
    let events = panel.remove_all_and_reset();
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            OpEvent::StimulationLatchedZero { .. } => "latch",
            OpEvent::MirrorStopIssued { .. } => "mirror-stop",
            OpEvent::EngineReset { .. } => "reset",
            OpEvent::RemoveAllCompleted { .. } => "completed",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["latch", "reset", "completed"]);
    let board = panel.dashboard();
    assert_eq!(
        board.state.arousal, 0,
        "the real engine is back at baseline"
    );
    assert_eq!(
        board.state.phase,
        torogao_gear::engine_iface::ArousalPhase::Baseline
    );
    assert!(
        !board.state.culmination_latch,
        "the reset cleared the latch (the episode counter itself \
         advances on re-arm under the engine's law, not on reset)"
    );
}

#[test]
fn playtime_panel_drives_the_mode_with_live_readouts() {
    let mut panel = PlayroomPlaytime::new("prima").unwrap();
    // Setup: the panel states its stage before entry.
    assert!(
        panel
            .panel_lines()
            .iter()
            .any(|l| l.contains("stage Setup")),
        "setup readout"
    );
    panel.enter_mode().unwrap();
    panel.set_rec_skin(true).unwrap();
    panel.start_take().unwrap();
    panel.set_amplitude(1000).unwrap();
    let mut input = ModeTickInput::default();
    let mut saw_building = false;
    for t in 0..3600u64 {
        if t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        } else {
            input.touch = None;
        }
        let out = panel.step(&input).unwrap();
        saw_building |= out.stage == Stage::Building;
    }
    assert!(saw_building, "the mode built");
    // The panel's readouts agree with the mode's own
    // state — bound, not re-derived.
    let lines = panel.panel_lines();
    let mode = panel.mode();
    assert!(
        lines
            .iter()
            .any(|l| l.contains(&format!("stage {}", mode.stage().name()))),
        "stage line is the mode's stage: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains(&format!("pressure {}", mode.pressure_permille()))),
        "pressure line is the mode's pressure: {lines:?}"
    );
    // The REC skin shows while its toggle is on.
    assert!(
        lines.iter().any(|l| l.contains("REC")),
        "the REC skin renders"
    );
    let take = panel.stop_take().unwrap();
    assert!(take.ticks() > 0 && take.trajectory_hash != 0);
}

#[test]
fn starter_pack_stages_through_the_panel() {
    // The starter pack's documents are ordinary gear:
    // they apply through the gear panel's worn set on
    // the same surface the mode uses.
    let view = RealEngineView::new("prima", CharacterProfile::default());
    let mut panel = PlayroomGearPanel::with_engine(starter_pack_documents(800), Box::new(view));
    assert!(panel.apply("fixture.blossom-seat"));
    assert!(panel.apply("device.blossom-mount"));
    assert!(panel.apply("apparel.sample-underwear"));
    let ids = panel.worn().ids();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().any(|i| i == "fixture.blossom-seat"));
}
