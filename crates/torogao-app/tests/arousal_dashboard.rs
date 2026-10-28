//! Playroom arousal dashboard gates (AMENDMENT-T4 §3.2,
//! T4.2): the dashboard binds to the real engine — every
//! readout equals engine state, the worked-example gear
//! behaves per the §8 fixtures through the panel's own
//! controls, and the safety exits act through the same
//! surface the panel exposes.

use torogao_app::arousal::{PLAYROOM_CHARACTER, PlayroomArousal};
use torogao_session::arousal::{Phase, TickInputs};

const NONE: TickInputs = TickInputs {
    touch: None,
    suction_peak: false,
    spray_rate: None,
    face_contact: false,
};

fn line(lines: &[String], prefix: &str) -> String {
    lines
        .iter()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting {prefix:?} in {lines:?}"))
        .clone()
}

#[test]
fn dashboard_readouts_equal_engine_state() {
    let mut dash = PlayroomArousal::new();
    dash.apply_clamps().unwrap();
    dash.step(&NONE).unwrap();
    let engine = dash.host().get(PLAYROOM_CHARACTER).unwrap();
    let v = engine.vector();
    assert_eq!(v.floor, 320);
    assert_eq!(
        v.arousal, 320,
        "the clamp floor holds A from the first tick"
    );
    let lines = dash.dashboard_lines();
    assert_eq!(
        line(&lines, "A "),
        format!(
            "A {}  Q {}  S {}  F {}  P {}  D {}",
            v.arousal, v.pleasure_charge, v.sensitivity, v.floor, v.peak_input, v.dose
        ),
        "the panel's first line is the engine's vector, verbatim"
    );
    assert_eq!(line(&lines, "last culmination"), "last culmination none");
}

#[test]
fn dashboard_drives_the_vibe_fixture_and_stop() {
    let mut dash = PlayroomArousal::new();
    dash.apply_vibrator(1000).unwrap();
    // §8.3 high ramp through the panel: fires <= 30 s, tier 4.
    let mut fired = None;
    for _ in 0..30 * 60 {
        dash.step(&NONE).unwrap();
        if let Some(last) = dash.last_culmination() {
            fired = Some(last);
            break;
        }
    }
    let last = fired.expect("culmination through the dashboard controls");
    assert_eq!(last.tier, 4);
    assert_eq!(last.episode, 1);
    let lines = dash.dashboard_lines();
    assert!(
        line(&lines, "last culmination").contains("tier 4"),
        "panel reports the recorded tier: {lines:?}"
    );
    // STOP through the panel: drive is zero next tick.
    dash.stop_vibrator().unwrap();
    let out = dash.step(&NONE).unwrap();
    assert!(
        out.drive.iter().all(|d| d.strength_permille == 0),
        "STOP zeroes the drive from the next tick"
    );
    let lines = dash.dashboard_lines();
    assert!(
        line(&lines, "device vibe").contains("u 0"),
        "panel shows the stopped device: {lines:?}"
    );
}

#[test]
fn dashboard_remove_all_is_the_unconditional_exit() {
    let mut dash = PlayroomArousal::new();
    dash.apply_clamps().unwrap();
    dash.apply_vibrator(700).unwrap();
    for _ in 0..5 * 60 {
        dash.step(&NONE).unwrap();
    }
    dash.remove_all().unwrap();
    dash.step(&NONE).unwrap();
    let engine = dash.host().get(PLAYROOM_CHARACTER).unwrap();
    assert!(engine.worn_ids().is_empty());
    assert_eq!(engine.phase(), Phase::Baseline);
    let lines = dash.dashboard_lines();
    assert!(
        line(&lines, "A ").contains("F 0"),
        "floor released: {lines:?}"
    );
    // The exit is in the host's op log as a completed act.
    assert!(
        dash.host()
            .log()
            .iter()
            .any(|e| e.op["op"] == "remove_all_and_reset")
    );
}
