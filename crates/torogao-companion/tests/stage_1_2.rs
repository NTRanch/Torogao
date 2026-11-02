//! Stage 1.2 gate instruments (PROGRAM.md §3.3/§3.4),
//! fixture-gated like the Stage 1.1 suite: set
//! `TOROGAO_PHASE_F_FIXTURES` to the directory holding the
//! cast repertoire fixtures and these run; unset, the
//! fixture tests skip silently.
//!
//! - Persona/Nerd parity: the companion capability table
//!   matches the companion's live action surface exactly,
//!   and its state census holds (one declared state file).
//! - Cast: all three rigs selectable through the core.
//! - Gaze: the evaluated gaze direction tracks a scripted
//!   cursor path within 2° (read at each waypoint's
//!   settled tail, 60 fps sampling).
//! - Latency: a pointer click changes the evaluated frame
//!   within 2 frames of the event step.
//! - Seam: every cast rig's ambient loop seam ratio is
//!   within the 1.1 bound.
//! - Determinism: a fixed input script replays to the
//!   same evaluated trace three times.
//! - Quiet hours: the click reaction is suppressed and
//!   the ambient performance is never interrupted.

use std::collections::BTreeSet;
use std::path::PathBuf;

use torogao_companion::cast::CAST;
use torogao_companion::core::{CompanionCore, FACE_ANCHOR, VIEW_DISTANCE_HEIGHTS};
use torogao_companion::perf::{run_scripted_trace, run_trace, seam_report};

fn fixture_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("TOROGAO_PHASE_F_FIXTURES")?);
    dir.join("prima-repertoire.inx").is_file().then_some(dir)
}

fn fixture(file: &str) -> Option<PathBuf> {
    fixture_dir().map(|dir| dir.join(file))
}

/// The surface aspect the scripted runs present at (a
/// portrait window, like hers).
const SCRIPT_ASPECT: f64 = 0.5;

/// Inverts the core's pointer conversion: the surface
/// fractions a cursor must sit at for the driver to be
/// asked for this direction.
fn pointer_for_degrees(yaw: f64, pitch: f64) -> (f64, f64) {
    (
        FACE_ANCHOR.0 + yaw.to_radians().tan() * VIEW_DISTANCE_HEIGHTS / SCRIPT_ASPECT,
        FACE_ANCHOR.1 + pitch.to_radians().tan() * VIEW_DISTANCE_HEIGHTS,
    )
}

fn angular_error(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

#[test]
fn all_three_cast_rigs_are_selectable_and_gaze_complete() {
    let Some(dir) = fixture_dir() else {
        return;
    };
    let mut core = CompanionCore::load(
        &dir.join("prima-repertoire.inx"),
        None,
        None,
        "stage-1.2 cast",
    )
    .expect("load Prima");
    assert_eq!(core.cast_member().map(|member| member.id), Some("prima"));
    // The governed rigs' real gaze surface, measured on
    // the fixtures: head (ParamAngleX/Y) and blink
    // (ParamEyeLOpen/ROpen) channels are present on all
    // three; the ParamGaze eyeball channels are absent on
    // all three — their eyeball travel lives in the
    // authored roll/cross system. The gap report must say
    // exactly that, no more and no less.
    assert_eq!(
        core.gaze_gaps(),
        ["ParamGazeX", "ParamGazeY"],
        "Prima gaze gaps"
    );
    for member in CAST.iter().skip(1) {
        core.switch_character(member).expect("switch");
        assert_eq!(
            core.cast_member().map(|on_stage| on_stage.id),
            Some(member.id)
        );
        assert!(
            core.model_path().ends_with(member.fixture_file),
            "model path {} is not {}",
            core.model_path().display(),
            member.fixture_file
        );
        assert_eq!(
            core.gaze_gaps(),
            ["ParamGazeX", "ParamGazeY"],
            "{} gaze gaps",
            member.name
        );
        core.tick().expect("tick");
        let snapshot = core.play_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.animation.as_deref(),
            Some("Daydream"),
            "{} does not resume the ambient performance after a switch",
            member.name
        );
    }
    // And back to Prima, completing the roster round trip.
    core.switch_character(&CAST[0]).expect("switch back");
    assert_eq!(core.cast_member().map(|member| member.id), Some("prima"));
}

#[test]
fn gaze_tracks_the_scripted_cursor_path_within_two_degrees() {
    let Some(model) = fixture("prima-repertoire.inx") else {
        return;
    };
    // Waypoints reachable on her portrait surface (the
    // pointer conversion's own physics bounds yaw to
    // about ±10.5° at the surface edges at this aspect).
    let waypoints: [(f64, f64); 4] = [(9.0, 6.0), (-9.0, -4.0), (6.0, 16.0), (0.0, -2.0)];
    const HOLD_STEPS: usize = 90;
    let mut pointer: Vec<Option<(f64, f64)>> = Vec::new();
    for waypoint in waypoints {
        let position = Some(pointer_for_degrees(waypoint.0, waypoint.1));
        pointer.extend(std::iter::repeat_n(position, HOLD_STEPS));
    }
    let run = run_scripted_trace(
        &model,
        pointer.len(),
        &pointer,
        &BTreeSet::new(),
        SCRIPT_ASPECT,
    )
    .expect("scripted run");
    // The reading rule: at each waypoint's settled tail
    // (the last 30 of 90 samples — the saccade has landed
    // and pursuit has closed), the evaluated direction is
    // within the gate's 2° of the cursor direction.
    let mut worst = 0.0_f64;
    for (index, waypoint) in waypoints.iter().enumerate() {
        let tail_start = (index + 1) * HOLD_STEPS - 30;
        for (offset, gaze) in run.gaze[tail_start..(index + 1) * HOLD_STEPS]
            .iter()
            .enumerate()
        {
            let error = angular_error(*gaze, *waypoint);
            worst = worst.max(error);
            assert!(
                error <= 2.0,
                "waypoint {waypoint:?} step {}: gaze {gaze:?} is {error:.3} deg off",
                tail_start + offset
            );
        }
    }
    println!("gaze gate: worst settled error {worst:.4} deg over 4 waypoints");
}

#[test]
fn a_click_changes_the_frame_within_two_frames() {
    let Some(model) = fixture("prima-repertoire.inx") else {
        return;
    };
    const CLICK_STEP: usize = 200;
    const FRAMES: usize = 400;
    let pointer = vec![Some(pointer_for_degrees(2.0, 2.0)); FRAMES];
    let calm = run_scripted_trace(&model, FRAMES, &pointer, &BTreeSet::new(), SCRIPT_ASPECT)
        .expect("calm run");
    let clicked = run_scripted_trace(
        &model,
        FRAMES,
        &pointer,
        &BTreeSet::from([CLICK_STEP]),
        SCRIPT_ASPECT,
    )
    .expect("clicked run");
    let first_change = (0..FRAMES)
        .find(|&step| calm.step_hashes[step] != clicked.step_hashes[step])
        .expect("the click must change the evaluated trace");
    assert!(
        first_change >= CLICK_STEP && first_change - CLICK_STEP <= 2,
        "click at {CLICK_STEP} first changed frame {first_change} (latency {} frames)",
        first_change - CLICK_STEP
    );
    println!(
        "latency gate: click at {CLICK_STEP}, first changed frame {first_change} ({} frames)",
        first_change - CLICK_STEP
    );
}

#[test]
fn every_cast_rigs_ambient_seam_is_within_the_bound() {
    let Some(dir) = fixture_dir() else {
        return;
    };
    for member in CAST {
        let run = run_trace(&dir.join(member.fixture_file), 600).expect("trace run");
        assert!(
            !run.seam_steps.is_empty(),
            "{}: no loop wrap in 600 steps",
            member.name
        );
        let seam = seam_report(&run);
        println!(
            "seam gate: {} ratio {:.3} (median delta {:.6})",
            member.name, seam.seam_ratio_max, seam.median_delta
        );
        assert!(
            seam.seam_ratio_max <= 1.1,
            "{}: seam ratio {} exceeds 1.1",
            member.name,
            seam.seam_ratio_max
        );
    }
}

#[test]
fn a_fixed_input_script_replays_identically_three_times() {
    let Some(model) = fixture("prima-repertoire.inx") else {
        return;
    };
    let mut pointer: Vec<Option<(f64, f64)>> = Vec::new();
    pointer.extend(std::iter::repeat_n(
        Some(pointer_for_degrees(-6.0, 3.0)),
        150,
    ));
    pointer.extend(std::iter::repeat_n(None, 150));
    pointer.extend(std::iter::repeat_n(
        Some(pointer_for_degrees(5.0, 10.0)),
        150,
    ));
    let clicks = BTreeSet::from([320_usize]);
    let runs: Vec<_> = (0..3)
        .map(|_| {
            run_scripted_trace(&model, pointer.len(), &pointer, &clicks, SCRIPT_ASPECT)
                .expect("scripted run")
        })
        .collect();
    assert_eq!(runs[0].trace_hash, runs[1].trace_hash);
    assert_eq!(runs[1].trace_hash, runs[2].trace_hash);
    assert_eq!(runs[0].gaze, runs[1].gaze, "gaze traces must also agree");
}

#[test]
fn quiet_mode_suppresses_the_click_reaction() {
    let Some(model) = fixture("prima-repertoire.inx") else {
        return;
    };
    let mut core = CompanionCore::load(&model, None, None, "stage-1.2 quiet").expect("load");
    for _ in 0..30 {
        core.tick().expect("tick");
    }
    core.set_quiet(true);
    assert!(
        !core.click_reaction().expect("quiet click"),
        "quiet mode must suppress the reaction"
    );
    for _ in 0..30 {
        core.tick().expect("tick");
    }
    assert_eq!(
        core.play_snapshot().expect("snapshot").animation.as_deref(),
        Some("Daydream"),
        "the ambient performance must run uninterrupted in quiet mode"
    );
    core.set_quiet(false);
    assert!(core.click_reaction().expect("loud click"));
    core.tick().expect("tick");
    assert_eq!(
        core.play_snapshot().expect("snapshot").animation.as_deref(),
        Some("Greeting"),
        "with quiet off, the click greets again"
    );
}

#[test]
fn the_capability_table_matches_the_companions_live_surface() {
    use torogao_app::persona::{COMPANION_CAPABILITIES, CompanionHousing};
    use torogao_companion::menu::MenuAction;

    // The surface, enumerated by hand from the live code:
    // every MenuAction variant except the menu's own
    // dismiss chrome maps to one capability id (this match
    // is exhaustive — a new variant fails compilation here
    // until it is homed), and the pointer/driver acts of
    // the presentations and core complete the set.
    let mut surface: BTreeSet<&str> = BTreeSet::new();
    for action in [
        MenuAction::PlayGreeting,
        MenuAction::PlayDaydream,
        MenuAction::SwitchCharacter(0),
        MenuAction::SendToOutput("x".to_string()),
        MenuAction::ToggleQuiet,
        MenuAction::ToggleClickThrough,
        MenuAction::OpenTorogao,
        MenuAction::Quit,
        MenuAction::Close,
    ] {
        let id = match action {
            MenuAction::PlayGreeting => Some("play-greeting"),
            MenuAction::PlayDaydream => Some("play-daydream"),
            MenuAction::SwitchCharacter(_) => Some("switch-character"),
            MenuAction::SendToOutput(_) => Some("send-to-display"),
            MenuAction::ToggleQuiet => Some("quiet-hours"),
            MenuAction::ToggleClickThrough => Some("toggle-click-through"),
            MenuAction::OpenTorogao => Some("open-torogao"),
            MenuAction::Quit => Some("quit"),
            MenuAction::Close => None, // dismiss chrome, not a capability
        };
        if let Some(id) = id {
            surface.insert(id);
        }
    }
    // Pointer acts (presentations) and governed drivers
    // (core), named in the table's own terms.
    for id in [
        "click-reaction",
        "drag-move",
        "gaze-follow",
        "idle-saccades-blink",
        "invoke-area",
    ] {
        surface.insert(id);
    }
    let table: BTreeSet<&str> = COMPANION_CAPABILITIES.iter().map(|cap| cap.id).collect();
    assert_eq!(
        surface, table,
        "the companion's live surface and the audit table must be the same set"
    );
    // And every registry-housed row names play_control —
    // the one tool both local and remote play calls land
    // in.
    for capability in COMPANION_CAPABILITIES {
        if let CompanionHousing::Registry(tool) = capability.housing {
            assert_eq!(tool, "play_control", "{} homes elsewhere", capability.id);
        }
    }
}

#[test]
fn the_state_census_holds_one_declared_file_and_no_new_writers() {
    use torogao_app::persona::COMPANION_STATE_FILE;

    // The declared state file is the file the state
    // module actually writes.
    let path = torogao_companion::state::default_state_path().expect("state path");
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some(COMPANION_STATE_FILE)
    );
    // The census: file writes in the crate live only in
    // the state module (the state file itself) and in the
    // two presentations + main (user-named evidence
    // outputs and the save_state calls). A new writer
    // anywhere else fails here until it is declared.
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut direct_writers: BTreeSet<String> = BTreeSet::new();
    let mut any_write: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(&src).expect("read src dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read source");
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        if text.contains("std::fs::write") {
            direct_writers.insert(file.clone());
        }
        if text.contains("std::fs::write") || text.contains("save_state(") {
            any_write.insert(file);
        }
    }
    assert_eq!(
        direct_writers,
        BTreeSet::from([
            "fallback.rs".to_string(),
            "state.rs".to_string(),
            "wayland.rs".to_string()
        ]),
        "direct fs writes (evidence outputs + the state writer) live in exactly these files"
    );
    assert_eq!(
        any_write,
        BTreeSet::from([
            "fallback.rs".to_string(),
            "main.rs".to_string(),
            "state.rs".to_string(),
            "wayland.rs".to_string()
        ]),
        "no other module may write files"
    );
}

#[test]
fn a_slice_era_state_file_parses_with_quiet_off() {
    // A state file written before the quiet_hours field
    // existed must load with quiet off (the field is
    // additive with a default).
    let legacy = r#"{
        "version": 1,
        "anchor": "bottom-right",
        "margin_side": 24,
        "margin_edge": 0,
        "display_height": 480
    }"#;
    let state = torogao_companion::state::CompanionState::from_json(legacy).expect("legacy parse");
    assert!(!state.quiet_hours);
}

#[test]
fn the_gaze_push_lands_in_the_evaluation_and_blinks_close_the_eyes() {
    let Some(model) = fixture("prima-repertoire.inx") else {
        return;
    };
    // Plumbing, head channel: two step-identical runs —
    // one holding the pointer at rest, one at (9°, 6°) —
    // differ only in the driver's push, so the evaluated
    // ParamAngleX internal difference at the settled tail
    // is the head gaze that actually landed. On the cast
    // rigs (no eyeball channels) the head carries the
    // full 9°.
    const HOLD: usize = 90;
    let rest = vec![Some(pointer_for_degrees(0.0, -2.0)); HOLD];
    let attending = vec![Some(pointer_for_degrees(9.0, 6.0)); HOLD];
    let rest_run =
        run_scripted_trace(&model, HOLD, &rest, &BTreeSet::new(), SCRIPT_ASPECT).expect("rest");
    let gaze_run = run_scripted_trace(&model, HOLD, &attending, &BTreeSet::new(), SCRIPT_ASPECT)
        .expect("attending");
    let mut worst = 0.0_f64;
    for step in HOLD - 30..HOLD {
        let landed = f64::from(gaze_run.angle_x_internal[step] - rest_run.angle_x_internal[step]);
        worst = worst.max((landed - 9.0).abs());
        assert!(
            (landed - 9.0).abs() <= 0.75,
            "step {step}: landed head gaze {landed:.3} deg, expected 9 +/- 0.75"
        );
    }
    println!("plumbing gate: head push landed within {worst:.3} deg of 9 across the tail");

    // Plumbing, blink channel: over 1200 unattended
    // steps the evaluated ParamEyeLOpen internal must
    // dip to near-shut at least once (the blink profile
    // multiplies the standing aperture in evaluation).
    let mut core = CompanionCore::load(&model, None, None, "stage-1.2 blink").expect("load");
    let eye_l = core
        .scene()
        .expect("scene")
        .puppet
        .param_index("ParamEyeLOpen")
        .expect("eye channel");
    let mut minimum = f32::MAX;
    for _ in 0..1200 {
        core.tick().expect("tick");
        let value = core.scene().expect("scene").puppet.params[eye_l]
            .last_internal
            .x;
        minimum = minimum.min(value);
    }
    println!("plumbing gate: ParamEyeLOpen internal minimum over 1200 steps: {minimum:.3}");
    assert!(
        minimum < 0.2,
        "no blink closed the eye in 1200 steps (min internal {minimum})"
    );
}
