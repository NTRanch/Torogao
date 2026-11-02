//! The convergence gate: the six AUTH-G flow verbs the Phase 2
//! trunk deferred — `choice_define`, `ending_define`,
//! `gallery_entry_define`, `schedule_define`, `scene_bind_rig`,
//! `scene_exit_define` — author through decide() → apply →
//! op log on this branch's G1 core, validate clean, and replay
//! byte-identically ×3; the checks this branch can honestly run
//! (schedule coverage, ending-priority uniqueness, the
//! unconditional-option rule, binding targets, gallery
//! thumbnails) refuse with their classes; and the game census
//! reads 11 of 22 live with the remaining eleven named.

mod common;

use common::{fresh, init, log, project_sha, query, request, verb};
use serde_json::{Value, json};
use std::path::Path;
use torogao_author::{Status, run_requests, verbs_doc};

fn choice() -> Value {
    json!({
        "id": "door-choice",
        "scene": "room",
        "prompt_line": "door-prompt",
        "options": [
            {
                "id": "stay",
                "label_ref": "choice-stay",
                "origin": "player",
                "target": {"kind": "line", "id": "stay-line"},
            },
            {
                "id": "leave",
                "label_ref": "choice-leave",
                "visible_if": "door-unlocked",
                "origin": "alter_ego",
                "sort": {
                    "kind": "arith",
                    "op": "add",
                    "left": {"kind": "clock_day"},
                    "right": {"kind": "lit", "value": 1},
                },
                "effect_set": "leave-effects",
                "target": {"kind": "exit", "id": "to-hall"},
            },
            {
                "id": "sleep",
                "label_ref": "choice-sleep",
                "enabled_if": "not-tired",
                "origin": "player",
                "target": {"kind": "ending_check"},
            },
        ],
    })
}

/// Authors the gameflow fixture: a clock, a rig expression and
/// a motion for the binding to resolve against, then one record
/// per flow family.
fn build_flow(root: &Path) {
    let requests = vec![
        init("flow-gate"),
        request(
            "clock_define",
            json!({"slots": ["morning", "afternoon", "evening", "night"]}),
        ),
        request(
            "param_define",
            json!({"id": "angle-x", "class": "angle", "min": -30.0, "default": 0.0, "max": 30.0}),
        ),
        request(
            "expression_define",
            json!({
                "id": "smile",
                "blend": "additive",
                "values": [{"parameter": "angle-x", "value": 1.0}],
            }),
        ),
        request(
            "motion_create",
            json!({"id": "idle", "length_ticks": 60, "looped": true}),
        ),
        request(
            "schedule_define",
            json!({
                "character": "noor",
                "slots": [
                    {"slot": "morning", "scene": "kitchen"},
                    {"slot": "afternoon", "scene": "studio"},
                    {"slot": "evening", "scene": "room"},
                    {"slot": "night", "scene": "room"},
                ],
                "overrides": [{"condition": "invited-out", "scene": "cafe"}],
            }),
        ),
        request(
            "scene_bind_rig",
            json!({
                "scene": "room",
                "participant": "noor",
                "rig_model": "noor-rig",
                "entry_expression": "smile",
                "entry_motion": "idle",
            }),
        ),
        request(
            "scene_exit_define",
            json!({
                "scene": "room",
                "id": "to-hall",
                "target": "hall",
                "condition": "door-unlocked",
            }),
        ),
        request("choice_define", choice()),
        request(
            "ending_define",
            json!({
                "id": "good-end",
                "condition": "bond-high",
                "priority": 10,
                "epilogue_scene": "epilogue-good",
            }),
        ),
        request(
            "ending_define",
            json!({
                "id": "quiet-end",
                "condition": "days-spent",
                "priority": 20,
                "epilogue_scene": "epilogue-quiet",
            }),
        ),
        request(
            "gallery_entry_define",
            json!({
                "id": "first-meeting",
                "category": "scenes",
                "unlock": {"kind": "condition", "id": "met-noor"},
                "replay_scene": "room",
                "replay_line": "intro-1",
            }),
        ),
        request(
            "gallery_entry_define",
            json!({
                "id": "confession",
                "category": "scenes",
                "unlock": {"kind": "effect"},
                "replay_scene": "roof",
                "replay_line": "confess-1",
            }),
        ),
    ];
    let answer = run_requests(root, root, &requests);
    assert_eq!(answer.status, Status::Ok, "flow fixture: {}", answer.doc);
}

/// Asserts a verb answer is a refusal carrying `code`.
fn assert_refused(answer: &torogao_author::Answer, code: &str) {
    assert_eq!(answer.status, Status::Refused, "answer: {}", answer.doc);
    assert_eq!(answer.doc["code"], code, "answer: {}", answer.doc);
}

#[test]
fn flow_records_author_validate_and_replay_three_times() {
    let root = fresh("flow-replay");
    build_flow(&root);

    let entries = log(&root);
    let verbs: Vec<&str> = entries
        .iter()
        .map(|entry| entry["verb"].as_str().expect("verb"))
        .collect();
    for want in [
        "choice_define",
        "ending_define",
        "gallery_entry_define",
        "schedule_define",
        "scene_bind_rig",
        "scene_exit_define",
    ] {
        assert!(verbs.contains(&want), "{want} in {verbs:?}");
    }

    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean");

    let on_disk = project_sha(&root);
    for run in 1..=3 {
        let replayed = query(&root, "project_replay");
        assert_eq!(replayed.status, Status::Ok, "run {run}: {}", replayed.doc);
        assert_eq!(replayed.doc["sha256"], on_disk.as_str(), "run {run}");
    }
}

#[test]
fn rebinding_is_a_set_not_a_conflict() {
    let root = fresh("flow-rebind");
    build_flow(&root);
    let sha_before = project_sha(&root);

    // The identical binding is a no-op.
    let noop = verb(
        &root,
        "scene_bind_rig",
        json!({
            "scene": "room",
            "participant": "noor",
            "rig_model": "noor-rig",
            "entry_expression": "smile",
            "entry_motion": "idle",
        }),
    );
    assert_eq!(noop.status, Status::Ok, "{}", noop.doc);
    assert_eq!(project_sha(&root), sha_before);

    // A different rig model rebinds: art iterates without
    // re-authoring the scene.
    let rebound = verb(
        &root,
        "scene_bind_rig",
        json!({
            "scene": "room",
            "participant": "noor",
            "rig_model": "noor-rig-v2",
            "entry_expression": "smile",
            "entry_motion": "idle",
        }),
    );
    assert_eq!(rebound.status, Status::Ok, "{}", rebound.doc);
    assert_ne!(project_sha(&root), sha_before);
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);

    // The define-verbs keep upsert discipline: a differing
    // redefinition conflicts.
    let mut changed = choice();
    changed["prompt_line"] = json!("other-prompt");
    assert_refused(&verb(&root, "choice_define", changed), "TG-AUTH-CONFLICT");
    assert_refused(
        &verb(
            &root,
            "ending_define",
            json!({
                "id": "good-end",
                "condition": "bond-high",
                "priority": 30,
                "epilogue_scene": "epilogue-good",
            }),
        ),
        "TG-AUTH-CONFLICT",
    );
}

#[test]
fn schedule_coverage_battery() {
    // No clock: coverage cannot be checked at all.
    let root = fresh("flow-schedule-noclock");
    let answer = run_requests(&root, &root, &[init("flow-noclock")]);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    assert_refused(
        &verb(
            &root,
            "schedule_define",
            json!({
                "character": "noor",
                "slots": [{"slot": "morning", "scene": "kitchen"}],
            }),
        ),
        "TG-GAME-GRAPH",
    );

    let root = fresh("flow-schedule");
    build_flow(&root);
    // A missing clock slot is a graph defect.
    assert_refused(
        &verb(
            &root,
            "schedule_define",
            json!({
                "character": "mia",
                "slots": [
                    {"slot": "morning", "scene": "kitchen"},
                    {"slot": "afternoon", "scene": "studio"},
                    {"slot": "evening", "scene": "room"},
                ],
            }),
        ),
        "TG-GAME-GRAPH",
    );
    // A slot the clock does not declare is a graph defect.
    assert_refused(
        &verb(
            &root,
            "schedule_define",
            json!({
                "character": "mia",
                "slots": [
                    {"slot": "morning", "scene": "kitchen"},
                    {"slot": "afternoon", "scene": "studio"},
                    {"slot": "evening", "scene": "room"},
                    {"slot": "night", "scene": "room"},
                    {"slot": "dawn", "scene": "room"},
                ],
            }),
        ),
        "TG-GAME-GRAPH",
    );
    // A slot named twice is malformed.
    assert_refused(
        &verb(
            &root,
            "schedule_define",
            json!({
                "character": "mia",
                "slots": [
                    {"slot": "morning", "scene": "kitchen"},
                    {"slot": "morning", "scene": "studio"},
                    {"slot": "afternoon", "scene": "studio"},
                    {"slot": "evening", "scene": "room"},
                    {"slot": "night", "scene": "room"},
                ],
            }),
        ),
        "TG-AUTH-MALFORMED",
    );
}

#[test]
fn choice_and_ending_battery() {
    let root = fresh("flow-choice");
    build_flow(&root);

    // No options at all.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({"id": "c2", "scene": "room", "options": []}),
        ),
        "TG-AUTH-MALFORMED",
    );
    // Every option gated: the choice can render empty.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({
                "id": "c2",
                "scene": "room",
                "options": [{
                    "id": "only",
                    "label_ref": "lbl",
                    "visible_if": "some-condition",
                    "origin": "player",
                    "target": {"kind": "ending_check"},
                }],
            }),
        ),
        "TG-GAME-GRAPH",
    );
    // An unknown origin tag.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({
                "id": "c2",
                "scene": "room",
                "options": [{
                    "id": "only",
                    "label_ref": "lbl",
                    "origin": "narrator",
                    "target": {"kind": "ending_check"},
                }],
            }),
        ),
        "TG-AUTH-UNKNOWN-KIND",
    );
    // A boolean sort expression is not an integer sort.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({
                "id": "c2",
                "scene": "room",
                "options": [{
                    "id": "only",
                    "label_ref": "lbl",
                    "origin": "player",
                    "sort": {"kind": "bool_lit", "value": true},
                    "target": {"kind": "ending_check"},
                }],
            }),
        ),
        "TG-GAME-EXPR",
    );
    // A sort referencing an undefined stat cannot resolve on
    // this branch (no G2.2 definitions exist yet).
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({
                "id": "c2",
                "scene": "room",
                "options": [{
                    "id": "only",
                    "label_ref": "lbl",
                    "origin": "player",
                    "sort": {"kind": "stat", "id": "bond"},
                    "target": {"kind": "ending_check"},
                }],
            }),
        ),
        "TG-GAME-EXPR",
    );
    // An ending claiming an existing priority: the runtime
    // must never coin-flip an ending.
    assert_refused(
        &verb(
            &root,
            "ending_define",
            json!({
                "id": "third-end",
                "condition": "anything",
                "priority": 10,
                "epilogue_scene": "epilogue-third",
            }),
        ),
        "TG-GAME-GRAPH",
    );
}

#[test]
fn binding_and_gallery_battery() {
    let root = fresh("flow-bind");
    build_flow(&root);

    // An entry expression that is not a rig expression.
    assert_refused(
        &verb(
            &root,
            "scene_bind_rig",
            json!({
                "scene": "room",
                "participant": "mia",
                "rig_model": "mia-rig",
                "entry_expression": "nope",
            }),
        ),
        "TG-AUTH-DANGLING",
    );
    // An entry motion that is not a defined clip.
    assert_refused(
        &verb(
            &root,
            "scene_bind_rig",
            json!({
                "scene": "room",
                "participant": "mia",
                "rig_model": "mia-rig",
                "entry_expression": "smile",
                "entry_motion": "nope",
            }),
        ),
        "TG-AUTH-DANGLING",
    );
    // A thumbnail that is not an intaken asset.
    assert_refused(
        &verb(
            &root,
            "gallery_entry_define",
            json!({
                "id": "thumb-test",
                "category": "scenes",
                "unlock": {"kind": "effect"},
                "replay_scene": "room",
                "replay_line": "intro-1",
                "thumbnail": "no-such-asset",
            }),
        ),
        "TG-AUTH-DANGLING",
    );
}

#[test]
fn game_census_reads_11_of_22_with_the_remainder_named() {
    let doc = verbs_doc();
    assert_eq!(doc["game_manifest_count"], 22);
    assert_eq!(doc["game_implemented_count"], 11);
    let mut deferred: Vec<&str> = doc["game_deferred"]
        .as_array()
        .expect("deferred")
        .iter()
        .map(|row| row["verb"].as_str().expect("verb"))
        .collect();
    deferred.sort_unstable();
    assert_eq!(
        deferred,
        [
            "character_define",
            "condition_define",
            "dialogue_line_define",
            "effect_set_define",
            "flag_define",
            "scene_define",
            "stat_define",
            "stat_tier_define",
            "table_define",
            "trigger_define",
            "value_define",
        ],
        "the eleven still-deferred game verbs, by name"
    );
    // The rig census is untouched by the flow work.
    assert_eq!(doc["implemented_count"], 39);
    assert_eq!(doc["descoped_count"], 8);
}
