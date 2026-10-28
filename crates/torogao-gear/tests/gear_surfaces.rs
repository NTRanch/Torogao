//! Gate (§3): the Creator gear editor and Playroom gear
//! panel as headless view-models — verbatim refusal panels,
//! deterministic preview readouts, worn-set operations, the
//! dashboard bound to the §5.2 interface (stub behind it,
//! labelled), and the Remove All standing control: label,
//! availability under every state, and its §3.4 act order.

use torogao_gear::anchors::RigTemplate;
use torogao_gear::document::StateVariant;
use torogao_gear::emit::document_text;
use torogao_gear::engine_iface::{ArousalPhase, StubEngine};
use torogao_gear::examples::{
    clamp_pair_document, vibrator_document, worked_example_materials, worked_example_payloads,
};
use torogao_gear::refusal::TG_GEAR_SCHEMA;
use torogao_gear::surfaces::{CreatorGearEditor, PlayroomGearPanel, REMOVE_ALL_LABEL};
use torogao_gear::worn::{OpEvent, WornSet};

fn editor_for(text: String) -> CreatorGearEditor {
    CreatorGearEditor::new(
        "estate/clamp-pair.gear.ron",
        text,
        RigTemplate::prima(),
        worked_example_payloads(),
        worked_example_materials(),
    )
}

#[test]
fn editor_validates_and_renders_refusals_verbatim() {
    let editor = editor_for(document_text(&clamp_pair_document()));
    assert!(editor.validate().is_ok());
    assert_eq!(editor.refusal_panel(), None);

    let bad = document_text(&clamp_pair_document()).replace("torogao-gear/1", "torogao-gear/9");
    let editor = editor_for(bad);
    let panel = editor.refusal_panel().expect("refusal panel renders");
    assert!(panel.contains(TG_GEAR_SCHEMA), "panel: {panel}");
    assert!(
        panel.contains("estate/clamp-pair.gear.ron"),
        "panel: {panel}"
    );
    assert!(panel.contains("format"), "panel: {panel}");
}

#[test]
fn editor_preview_is_a_real_solve_with_variant_stepping() {
    let mut editor = editor_for(document_text(&clamp_pair_document()));
    let report = editor.preview(600).expect("preview solves");
    assert_eq!(report.anchors.len(), 2);
    assert!(
        report.max_residual_px <= 2.0,
        "residual {}",
        report.max_residual_px
    );
    assert!(report.max_lag_steps <= 1);
    let first_variant = report.anchors[0].variant.clone();
    editor.step_variant();
    let report = editor.preview(600).expect("preview solves again");
    assert_ne!(report.anchors[0].variant, first_variant);
}

#[test]
fn panel_apply_remove_and_floor_max_rule() {
    let mut panel = PlayroomGearPanel::new(vec![clamp_pair_document(), vibrator_document(250)]);
    assert_eq!(panel.library_ids().len(), 2);
    assert!(panel.apply("estate.clamp-pair"));
    let dash = panel.dashboard();
    assert_eq!(dash.gear_floor_permille, 320);
    assert_eq!(
        dash.gear_floor_contributions,
        vec![("estate.clamp-pair".to_string(), 320)]
    );
    // Removing one side unseats the pair: its floor is gone
    // (floors never sum and a partial item contributes none).
    assert!(panel.remove_side("estate.clamp-pair", "body.nipple.left"));
    assert_eq!(panel.dashboard().gear_floor_permille, 0);
    assert!(panel.remove("estate.clamp-pair"));
    assert!(panel.worn().ids().is_empty());
}

#[test]
fn dashboard_reads_engine_state_through_the_interface() {
    let mut stub = StubEngine::new();
    let mut state = stub_state(&stub);
    state.arousal = 777;
    state.phase = ArousalPhase::Stimulated;
    stub.set_state(state);
    let panel = PlayroomGearPanel::with_engine(vec![clamp_pair_document()], Box::new(stub));
    let dash = panel.dashboard();
    assert_eq!(dash.state.arousal, 777);
    assert_eq!(dash.state.phase, ArousalPhase::Stimulated);
    let rendered = dash.render();
    assert!(rendered.contains("arousal 777"), "render: {rendered}");
}

fn stub_state(stub: &StubEngine) -> torogao_gear::ArousalStateVector {
    use torogao_gear::engine_iface::ArousalEngineView;
    stub.state_vector()
}

#[test]
fn displacement_and_reseat_drive_the_floor() {
    let mut worn = WornSet::new();
    worn.apply(clamp_pair_document(), 10);
    assert_eq!(worn.arousal_floor_permille(), 320);
    assert!(worn.mark_displaced(
        "estate.clamp-pair",
        "body.nipple.left",
        "drift-threshold",
        20
    ));
    // Latched: a second mark does not re-fire.
    assert!(!worn.mark_displaced(
        "estate.clamp-pair",
        "body.nipple.left",
        "drift-threshold",
        21
    ));
    assert_eq!(worn.arousal_floor_permille(), 0);
    let entry = worn.get("estate.clamp-pair").expect("entry");
    assert_eq!(entry.attachments[0].variant, StateVariant::Displaced);
    assert!(worn.reseat("estate.clamp-pair", 30));
    assert_eq!(worn.arousal_floor_permille(), 320);
    let displaced_events = worn
        .log()
        .iter()
        .filter(|e| matches!(e, OpEvent::ItemDisplaced { .. }))
        .count();
    assert_eq!(displaced_events, 1, "displacement event fires once");
}

#[test]
fn remove_all_act_order_and_ungated_availability() {
    assert_eq!(REMOVE_ALL_LABEL, "Remove All Gear & Reset State");
    let mut stub = StubEngine::new();
    let mut state = stub_state(&stub);
    state.arousal = 900;
    state.floor = 320;
    stub.set_state(state);
    let mut panel = PlayroomGearPanel::with_engine(vec![clamp_pair_document()], Box::new(stub));
    // Available with nothing worn, available mid-episode.
    assert!(panel.remove_all_available());
    panel.apply("estate.clamp-pair");
    panel.set_tick(5000);
    assert!(panel.remove_all_available());
    let events = panel.remove_all_and_reset();
    let kinds: Vec<String> = events.iter().map(|e| e.canonical()).collect();
    assert_eq!(kinds.len(), 3, "{kinds:?}");
    assert!(kinds[0].contains("stimulation-latched-zero"), "{kinds:?}");
    assert!(kinds[1].contains("engine-reset"), "{kinds:?}");
    assert!(kinds[2].contains("remove-all-completed"), "{kinds:?}");
    // The item removals (§3.4 step 3) sit in the session op
    // log between the latch and the reset.
    let log_so_far = panel.worn().log_lines();
    let latch_at = log_so_far
        .iter()
        .position(|l| l.contains("stimulation-latched-zero"));
    let removed_at = log_so_far
        .iter()
        .position(|l| l.contains(" removed estate.clamp-pair"));
    let reset_at = log_so_far.iter().position(|l| l.contains("engine-reset"));
    assert!(
        latch_at < removed_at && removed_at < reset_at,
        "act order in op log: {log_so_far:?}"
    );
    // No mirror session: no stop event issued.
    assert!(
        !kinds.iter().any(|k| k.contains("mirror-stop")),
        "{kinds:?}"
    );
    assert!(panel.worn().ids().is_empty());
    let dash = panel.dashboard();
    assert_eq!(dash.state, torogao_gear::ArousalStateVector::baseline());
    assert_eq!(dash.gear_floor_permille, 0);
    // The session op log carries the completed act.
    let log: Vec<String> = panel.worn().log_lines();
    assert!(
        log.last()
            .is_some_and(|l| l.contains("remove-all-completed")),
        "{log:?}"
    );
    // And it remains available afterwards.
    assert!(panel.remove_all_available());
}
