//! The actlab gate (D-943, Tool 1): validation and adversarial
//! tests for the act-record validator and the Crescendo
//! simulator, run against the checked-in data exports in
//! `data/actlab/` and the act fixtures in
//! `tests/fixtures/actlab/`.
//!
//! Validation half: records built from the §§6a–6c mechanisms
//! (gear sets, corruption, Crescendo) pass. Adversarial half:
//! each malformed or out-of-canon record is refused with the
//! correct typed violation.

use serde_json::{Value, json};
use std::path::PathBuf;
use torogao_author::actlab::{
    ActViolation, ParameterUniverse, RigSnapshot, crescendo_events, parse_act_record,
    parse_parameter_universe, parse_rig_snapshot, simulate_crescendo, validate_act_record,
};
use torogao_author::{Status, run_verb};

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).expect("read fixture")
}

fn data_text(name: &str) -> String {
    read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data/actlab")
            .join(name),
    )
}

fn fixture_text(name: &str) -> String {
    read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/actlab")
            .join(name),
    )
}

fn universe() -> ParameterUniverse {
    parse_parameter_universe(&data_text("universe.ron")).expect("universe parses")
}

fn rig_named(name: &str) -> RigSnapshot {
    parse_rig_snapshot(&data_text(name)).expect("rig snapshot parses")
}

fn all_rigs() -> Vec<RigSnapshot> {
    vec![
        rig_named("rig-prima.ron"),
        rig_named("rig-yui.ron"),
        rig_named("rig-alma.ron"),
    ]
}

fn codes(violations: &[ActViolation]) -> Vec<&str> {
    violations.iter().map(ActViolation::code).collect()
}

/// Builds one act record's text from raw field fragments, so an
/// adversarial case differs from a valid one in exactly the
/// fragment it names.
fn act_text(
    slug: &str,
    rigs: &str,
    set_name: &str,
    set_value: &str,
    parts: &str,
    bindings: &str,
) -> String {
    format!(
        "ActRecord(\n    slug: \"{slug}\",\n    rigs: [{rigs}],\n    \
         set_parameter: SetParameter(name: \"{set_name}\", value: {set_value}),\n    \
         parts: [{parts}],\n    state_bindings: [{bindings}],\n    \
         source: \"D-943 adversarial fixture\",\n)\n"
    )
}

fn peaks_bindings(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| {
            format!("StateBinding(parameter: \"ParamCrescendoStage\", peaks_passed: {value})")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn validate_text(text: &str) -> Vec<ActViolation> {
    let record = parse_act_record(text).expect("record parses");
    validate_act_record(&record, &universe(), &all_rigs())
}

// ---- Validation half ----------------------------------------------

#[test]
fn shipped_snapshots_parse_with_their_provenance() {
    let universe = universe();
    assert_eq!(universe.set_parameters.len(), 5);
    assert_eq!(universe.state_parameters.len(), 2);
    assert_eq!(universe.provenance.record_hashes.len(), 6);
    let prima = rig_named("rig-prima.ron");
    let yui = rig_named("rig-yui.ron");
    let alma = rig_named("rig-alma.ron");
    assert_eq!(prima.parameters.len(), 46);
    assert_eq!(yui.parameters.len(), 45);
    assert_eq!(alma.parameters.len(), 52);
    assert_eq!(prima.parts.len(), 36);
    assert_eq!(yui.parts.len(), 35);
    assert_eq!(alma.parts.len(), 53);
    for rig in [&prima, &yui, &alma] {
        assert_eq!(rig.provenance.record_hashes.len(), 2);
        // The governed state parameters sit on every rig with
        // the universe's ranges: the snapshots agree.
        for spec in &universe.state_parameters {
            let row = rig.parameter(&spec.name).expect("state parameter on rig");
            assert_eq!((row.min, row.max), (spec.min, spec.max), "{}", spec.name);
        }
    }
    // The known divergence is data: Yui has no ParamHairSway3,
    // and only Alma carries ParamHucowSet.
    assert!(yui.parameter("ParamHairSway3").is_none());
    assert!(prima.parameter("ParamHucowSet").is_none());
    assert!(alma.parameter("ParamHucowSet").is_some());
}

#[test]
fn gear_set_act_on_one_rig_validates() {
    let text = fixture_text("record-rope-prima.ron");
    assert_eq!(validate_text(&text), Vec::new());
}

#[test]
fn two_rig_record_validates() {
    let text = fixture_text("record-blindfold-two-rig.ron");
    assert_eq!(validate_text(&text), Vec::new());
}

#[test]
fn crescendo_record_with_holds_validates_and_traces() {
    let text = fixture_text("record-crescendo-yui.ron");
    let record = parse_act_record(&text).expect("record parses");
    assert_eq!(
        validate_act_record(&record, &universe(), &all_rigs()),
        Vec::new()
    );
    let events = crescendo_events(&record);
    assert_eq!(events, vec![0, 1, 1, 2, 3, 4]);
    assert_eq!(simulate_crescendo(&events), Ok(vec![0, 1, 1, 2, 3, 4]));
}

#[test]
fn hucow_act_on_alma_validates() {
    let text = fixture_text("record-hucow-alma.ron");
    assert_eq!(validate_text(&text), Vec::new());
}

#[test]
fn gag_set_act_with_no_bindings_validates() {
    let text = act_text(
        "gag-set-yui",
        "\"yui\"",
        "ParamGagSet",
        "1.0",
        "\"GagSet\"",
        "",
    );
    assert_eq!(validate_text(&text), Vec::new());
}

#[test]
fn verb_reports_a_valid_record() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let inputs = json!({
        "record": fixture_text("record-crescendo-yui.ron"),
        "universe": data_text("universe.ron"),
        "rigs": [
            data_text("rig-prima.ron"),
            data_text("rig-yui.ron"),
            data_text("rig-alma.ron"),
        ],
    });
    let resolve = |name: &str| -> Result<PathBuf, torogao_author::Refusal> {
        Err(torogao_author::Refusal::new(
            torogao_author::RefusalClass::UnknownTarget,
            format!("no project named {name}"),
        ))
    };
    let answer = run_verb(&root, &root, "act_validate", &inputs, &resolve);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    assert_eq!(answer.doc["result"], json!("valid"));
    assert_eq!(answer.doc["slug"], json!("crescendo-rope-yui"));
    assert_eq!(answer.doc["violations"], json!([]));
    assert_eq!(answer.doc["crescendo_trace"], json!([0, 1, 1, 2, 3, 4]));
}

// ---- Adversarial half ----------------------------------------------

#[test]
fn unknown_set_parameter_is_refused() {
    let text = act_text("nope-set-prima", "\"prima\"", "ParamNopeSet", "1.0", "", "");
    assert_eq!(
        codes(&validate_text(&text)),
        vec!["ACTLAB-UNKNOWN-SET-PARAMETER"]
    );
}

#[test]
fn out_of_range_set_value_is_refused() {
    let text = act_text(
        "rope-set-prima",
        "\"prima\"",
        "ParamRopeSet",
        "1.5",
        "\"RopeSet\"",
        "",
    );
    let violations = validate_text(&text);
    assert_eq!(codes(&violations), vec!["ACTLAB-SET-VALUE-OUT-OF-RANGE"]);
    match &violations[0] {
        ActViolation::SetValueOutOfRange {
            rig_id,
            value,
            min,
            max,
            ..
        } => {
            assert_eq!(rig_id, "prima");
            assert_eq!((*value, *min, *max), (1.5, 0.0, 1.0));
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn nonexistent_part_variant_is_refused() {
    let text = act_text(
        "rope-set-prima",
        "\"prima\"",
        "ParamRopeSet",
        "1.0",
        "\"RopeSet\", \"NopeSet\"",
        "",
    );
    let violations = validate_text(&text);
    assert_eq!(codes(&violations), vec!["ACTLAB-UNKNOWN-PART"]);
    match &violations[0] {
        ActViolation::UnknownPartVariant { part, rig_id } => {
            assert_eq!(part, "NopeSet");
            assert_eq!(rig_id, "prima");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn dangling_state_binding_is_refused() {
    let text = act_text(
        "rope-set-prima",
        "\"prima\"",
        "ParamRopeSet",
        "1.0",
        "\"RopeSet\"",
        "StateBinding(parameter: \"ParamNopeState\", value: 0.5)",
    );
    assert_eq!(
        codes(&validate_text(&text)),
        vec!["ACTLAB-UNKNOWN-STATE-PARAMETER"]
    );
}

#[test]
fn crescendo_decrease_is_refused() {
    let text = act_text(
        "crescendo-yui",
        "\"yui\"",
        "ParamRopeSet",
        "1.0",
        "\"RopeSet\"",
        &peaks_bindings(&["0", "1", "0"]),
    );
    let violations = validate_text(&text);
    assert_eq!(codes(&violations), vec!["ACTLAB-CRESCENDO-DECREASE"]);
    match &violations[0] {
        ActViolation::CrescendoDecrease { index, from, to } => {
            assert_eq!((*index, *from, *to), (2, 1, 0));
        }
        other => panic!("wrong variant: {other:?}"),
    }
    let failure = simulate_crescendo(&[0, 1, 0]).expect_err("direct simulation refuses");
    assert_eq!(failure.index, 2);
}

#[test]
fn crescendo_skip_jump_is_refused() {
    let text = act_text(
        "crescendo-yui",
        "\"yui\"",
        "ParamRopeSet",
        "1.0",
        "\"RopeSet\"",
        &peaks_bindings(&["0", "2"]),
    );
    let violations = validate_text(&text);
    assert_eq!(codes(&violations), vec!["ACTLAB-CRESCENDO-SKIP"]);
    match &violations[0] {
        ActViolation::CrescendoSkip { index, from, to } => {
            assert_eq!((*index, *from, *to), (1, 0, 2));
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn peaks_passed_five_is_refused() {
    let text = act_text(
        "crescendo-yui",
        "\"yui\"",
        "ParamRopeSet",
        "1.0",
        "\"RopeSet\"",
        &peaks_bindings(&["5"]),
    );
    assert_eq!(
        codes(&validate_text(&text)),
        vec!["ACTLAB-CRESCENDO-OUT-OF-RANGE"]
    );
    assert!(simulate_crescendo(&[5]).is_err());
    assert!(simulate_crescendo(&[-1]).is_err());
}

#[test]
fn malformed_slug_is_refused() {
    let text = act_text(
        "Rope Set!",
        "\"prima\"",
        "ParamRopeSet",
        "1.0",
        "\"RopeSet\"",
        "",
    );
    let violations = validate_text(&text);
    assert_eq!(codes(&violations), vec!["ACTLAB-SLUG"]);
}

#[test]
fn non_integer_peaks_passed_is_refused_at_parse() {
    let text = act_text(
        "crescendo-yui",
        "\"yui\"",
        "ParamRopeSet",
        "1.0",
        "\"RopeSet\"",
        &peaks_bindings(&["1.5"]),
    );
    let err = parse_act_record(&text).expect_err("a float peaks_passed refuses");
    assert_eq!(err.code(), "ACTLAB-NONINTEGER-PEAKS");
    // Through the verb the same record is a verdict, not a crash:
    // result "unparseable" with the typed failure listed.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let inputs = json!({
        "record": text,
        "universe": data_text("universe.ron"),
        "rigs": [data_text("rig-yui.ron")],
    });
    let resolve = |name: &str| -> Result<PathBuf, torogao_author::Refusal> {
        Err(torogao_author::Refusal::new(
            torogao_author::RefusalClass::UnknownTarget,
            format!("no project named {name}"),
        ))
    };
    let answer = run_verb(&root, &root, "act_validate", &inputs, &resolve);
    assert_eq!(answer.status, Status::Findings, "{}", answer.doc);
    assert_eq!(answer.doc["result"], json!("unparseable"));
    assert_eq!(
        answer.doc["violations"][0]["code"],
        json!("ACTLAB-NONINTEGER-PEAKS")
    );
}

#[test]
fn per_rig_divergence_is_refused() {
    // ParamHucowSet is governed (the S6a precedent) but exists
    // only on Alma: the same act that validates on Alma refuses
    // on Prima, by name.
    let text = act_text(
        "hucow-set-prima",
        "\"prima\"",
        "ParamHucowSet",
        "1.0",
        "",
        "",
    );
    let violations = validate_text(&text);
    assert_eq!(codes(&violations), vec!["ACTLAB-SET-PARAMETER-NOT-ON-RIG"]);
    match &violations[0] {
        ActViolation::SetParameterNotOnRig { parameter, rig_id } => {
            assert_eq!(parameter, "ParamHucowSet");
            assert_eq!(rig_id, "prima");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn out_of_range_state_value_is_refused() {
    let text = act_text(
        "rope-set-prima",
        "\"prima\"",
        "ParamRopeSet",
        "1.0",
        "\"RopeSet\"",
        "StateBinding(parameter: \"ParamCorruptionState\", value: 1.5)",
    );
    assert_eq!(
        codes(&validate_text(&text)),
        vec!["ACTLAB-STATE-VALUE-OUT-OF-RANGE"]
    );
}

#[test]
fn unknown_target_rig_is_refused() {
    let text = act_text("rope-set-nope", "\"nope\"", "ParamRopeSet", "1.0", "", "");
    let violations = validate_text(&text);
    assert!(
        codes(&violations).contains(&"ACTLAB-UNKNOWN-RIG"),
        "{violations:?}"
    );
}

#[test]
fn verdicts_are_deterministic() {
    let text = act_text(
        "Bad Slug",
        "\"prima\", \"prima\"",
        "ParamNopeSet",
        "1.0",
        "\"NopeSet\"",
        &peaks_bindings(&["0", "2"]),
    );
    let first = validate_text(&text);
    let second = validate_text(&text);
    assert_eq!(first, second);
    let first_json: Vec<Value> = first.iter().map(ActViolation::to_json).collect();
    let second_json: Vec<Value> = second.iter().map(ActViolation::to_json).collect();
    assert_eq!(first_json, second_json);
    // The fixed check order: slug, rigs, set, parts, crescendo.
    assert_eq!(
        codes(&first),
        vec![
            "ACTLAB-SLUG",
            "ACTLAB-DUPLICATE-RIG",
            "ACTLAB-UNKNOWN-SET-PARAMETER",
            "ACTLAB-UNKNOWN-PART",
            "ACTLAB-CRESCENDO-SKIP",
        ]
    );
}
