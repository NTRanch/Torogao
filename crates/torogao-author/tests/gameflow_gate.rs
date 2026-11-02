//! The Stage 2B gate (AUTH-G game-flow port, Owner rulings
//! C-1..C-11): the seven deferred flow verbs —
//! `game_save_schema_define`, `choice_define`, `ending_define`,
//! `gallery_entry_define`, `schedule_define`, `scene_bind_rig`,
//! `scene_exit_define` — live in the trunk game module.
//!
//! Ported from the AUTH-G convergence branch's
//! `tests/gameflow_gate.rs` (6 tests) and the save-schema
//! battery in its `tests/game_gate.rs`, adapted to trunk APIs:
//! records nest in the one `Game` section (C-1), choice
//! label/prompt references are `content.ron` sidecar keys
//! refused at decide time (C-7), and the verb census reads the
//! trunk's `GAME_MANIFEST` (the trunk's `verbs_doc()` is the
//! rig manifest only). Added for this stage: choice
//! consumption with the alter-ego veto, ending evaluation by
//! priority, the derived save-schema version with its
//! migration-chain load walk (C-4), the SceneDef growth
//! round-trip (C-8), and the plain-VN negative gate.

mod common;

use common::{fresh, init, log, project_sha, query, request, verb};
use serde_json::{Value, json};
use std::path::Path;
use torogao_author::game::GAME_MANIFEST;
use torogao_author::game::save::{load_game, save_game};
use torogao_author::game::simulate::{genesis, parse_log, run_ticks};
use torogao_author::game::state::StatKey;
use torogao_author::{Status, run_requests};

/// A 1x1 transparent PNG (signature + IHDR valid; the intake
/// checks the header, not the pixels).
const PNG_1X1: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 100, 248, 207, 80, 15, 0, 3,
    134, 1, 128, 90, 52, 125, 107, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// Asserts a verb answer is a refusal carrying `code`.
fn assert_refused(answer: &torogao_author::Answer, code: &str) {
    assert_eq!(answer.status, Status::Refused, "answer: {}", answer.doc);
    assert_eq!(answer.doc["code"], code, "answer: {}", answer.doc);
}

/// Asserts a verb answer was admitted.
fn assert_ok(answer: &torogao_author::Answer) {
    assert_eq!(answer.status, Status::Ok, "answer: {}", answer.doc);
}

/// The choice fixture's option list, in authored order. The
/// presented order at day 1 is [stay, sleep, leave]: stay and
/// sleep sort 0 (authored order breaks the tie), leave sorts
/// clock_day + 1 = 2; sneak is never visible.
fn choice_inputs() -> Value {
    json!({
        "id": "morning_choice",
        "scene": "bedroom",
        "prompt_line": "text_prompt",
        "options": [
            {
                "id": "stay", "label_ref": "text_stay", "origin": "player",
                "effect_set": "bond_up",
                "target": {"kind": "line", "id": "stay_line"},
            },
            {
                "id": "leave", "label_ref": "text_leave", "origin": "alter_ego",
                "sort": {
                    "kind": "add",
                    "left": {"kind": "clock", "field": "day"},
                    "right": {"kind": "lit", "value": 1},
                },
                "effect_set": "mark_left",
                "target": {"kind": "exit", "id": "to-hall"},
            },
            {
                "id": "sleep", "label_ref": "text_sleep", "origin": "player",
                "target": {"kind": "line", "id": "prompt_line"},
            },
            {
                "id": "sneak", "label_ref": "text_sneak", "origin": "player",
                "visible_if": "never",
                "target": {"kind": "line", "id": "sneak_line"},
            },
        ],
    })
}

/// Authors the flow fixture: rig anchors (an expression, a
/// motion, an intaken thumbnail), then the game core the flow
/// records hang off, then one record per flow family.
fn author_flow(root: &Path) {
    std::fs::write(root.join("thumb.png"), PNG_1X1).expect("write png");
    let requests = vec![
        init("flow-gate"),
        request(
            "param_define",
            json!({"id": "angle-x", "class": "angle", "min": -30.0, "default": 0.0, "max": 30.0}),
        ),
        request("part_create", json!({"id": "body"})),
        request(
            "expression_define",
            json!({
                "id": "smile", "blend": "additive",
                "values": [{"parameter": "angle-x", "value": 1.0}],
            }),
        ),
        request(
            "motion_create",
            json!({"id": "idle-sway", "length_ticks": 120, "looped": true}),
        ),
        request(
            "intake_image",
            json!({
                "id": "thumb", "path": "thumb.png",
                "license": "test fixture", "origin": "test fixture",
            }),
        ),
        request(
            "game_define",
            json!({
                "id": "flow-game", "start_scene": "bedroom",
                "alter_ego_veto": "veto_cost",
            }),
        ),
        request(
            "clock_define",
            json!({"slots": ["morning", "afternoon", "evening", "night"]}),
        ),
        request(
            "stat_define",
            json!({
                "id": "bond", "scope": "game", "kind": {"kind": "int"},
                "min": 0, "max": 10, "default": 0, "overflow": "clamp",
            }),
        ),
        request(
            "stat_define",
            json!({
                "id": "temptation", "scope": "game", "kind": {"kind": "int"},
                "min": 0, "max": 10, "default": 0, "overflow": "clamp",
            }),
        ),
        request(
            "flag_define",
            json!({"id": "left_room", "scope": "run", "default": false}),
        ),
        request(
            "condition_define",
            json!({
                "id": "bond_high",
                "expr": {
                    "kind": "cmp", "op": "ge",
                    "left": {"kind": "stat", "stat": "bond"},
                    "right": {"kind": "lit", "value": 5},
                },
            }),
        ),
        request(
            "condition_define",
            json!({
                "id": "bond_mid",
                "expr": {
                    "kind": "cmp", "op": "ge",
                    "left": {"kind": "stat", "stat": "bond"},
                    "right": {"kind": "lit", "value": 3},
                },
            }),
        ),
        request(
            "condition_define",
            json!({"id": "never", "expr": {"kind": "bool_lit", "value": false}}),
        ),
        request(
            "effect_set_define",
            json!({
                "id": "bond_up",
                "atoms": [{
                    "kind": "stat_add", "stat": "bond",
                    "amount": {"kind": "lit", "value": 1},
                }],
            }),
        ),
        request(
            "effect_set_define",
            json!({
                "id": "bond_five",
                "atoms": [{
                    "kind": "stat_set", "stat": "bond",
                    "value": {"kind": "lit", "value": 5},
                }],
            }),
        ),
        request(
            "effect_set_define",
            json!({
                "id": "veto_cost",
                "atoms": [{
                    "kind": "stat_add", "stat": "temptation",
                    "amount": {"kind": "lit", "value": 1},
                }],
            }),
        ),
        request(
            "effect_set_define",
            json!({"id": "mark_left", "atoms": [{"kind": "flag_set", "flag": "left_room"}]}),
        ),
        request(
            "character_define",
            json!({"id": "yui", "name_ref": "name_yui", "name_text": "Yui"}),
        ),
        request(
            "scene_define",
            json!({"id": "bedroom", "presentation": "vn", "participants": ["yui"]}),
        ),
        request("scene_define", json!({"id": "hall", "presentation": "vn"})),
        request(
            "dialogue_line_define",
            json!({
                "id": "bedroom_line", "scene": "bedroom", "ordinal": 0,
                "speaker": "narrator", "text_ref": "text_bedroom",
                "text": "Morning in the bedroom.",
            }),
        ),
        request(
            "dialogue_line_define",
            json!({
                "id": "prompt_line", "scene": "bedroom", "ordinal": 1,
                "speaker": "narrator", "text_ref": "text_prompt", "text": "What now?",
            }),
        ),
        request(
            "dialogue_line_define",
            json!({
                "id": "stay_line", "scene": "bedroom", "ordinal": 2,
                "speaker": "narrator", "text_ref": "text_stay", "text": "Stay a while.",
            }),
        ),
        request(
            "dialogue_line_define",
            json!({
                "id": "leave_line", "scene": "bedroom", "ordinal": 3,
                "speaker": "narrator", "text_ref": "text_leave", "text": "Slip out.",
            }),
        ),
        request(
            "dialogue_line_define",
            json!({
                "id": "sleep_line", "scene": "bedroom", "ordinal": 4,
                "speaker": "narrator", "text_ref": "text_sleep", "text": "Sleep.",
            }),
        ),
        request(
            "dialogue_line_define",
            json!({
                "id": "sneak_line", "scene": "bedroom", "ordinal": 5,
                "speaker": "narrator", "text_ref": "text_sneak", "text": "Sneak.",
            }),
        ),
        request(
            "dialogue_line_define",
            json!({
                "id": "hall_line", "scene": "hall", "ordinal": 0,
                "speaker": "narrator", "text_ref": "text_hall", "text": "The hall.",
            }),
        ),
        request(
            "trigger_define",
            json!({
                "id": "wake", "kind": {"kind": "on_event", "event": "wake"},
                "effect_set": "bond_five", "priority": 0,
                "repetition": "once", "cooldown_ticks": 0,
            }),
        ),
        request(
            "scene_exit_define",
            json!({"scene": "bedroom", "id": "to-hall", "target": "hall"}),
        ),
        request("choice_define", choice_inputs()),
        request(
            "ending_define",
            json!({
                "id": "good_end", "condition": "bond_high", "priority": 10,
                "epilogue_scene": "hall",
            }),
        ),
        request(
            "ending_define",
            json!({
                "id": "quiet_end", "condition": "bond_mid", "priority": 20,
                "epilogue_scene": "hall",
            }),
        ),
        request(
            "gallery_entry_define",
            json!({
                "id": "first_morning", "category": "scenes",
                "unlock": {"kind": "effect"},
                "replay_scene": "bedroom", "replay_line": "bedroom_line",
                "thumbnail": "thumb",
            }),
        ),
        request(
            "schedule_define",
            json!({
                "character": "yui",
                "slots": [
                    {"slot": "morning", "scene": "bedroom"},
                    {"slot": "afternoon", "scene": "bedroom"},
                    {"slot": "evening", "scene": "hall"},
                    {"slot": "night", "scene": "bedroom"},
                ],
            }),
        ),
        request(
            "scene_bind_rig",
            json!({
                "scene": "bedroom", "participant": "yui", "rig_model": "yui-rig",
                "entry_expression": "smile", "entry_motion": "idle-sway",
            }),
        ),
    ];
    let answer = run_requests(root, root, &requests);
    assert_eq!(answer.status, Status::Ok, "flow fixture: {}", answer.doc);
}

/// A minimal game project (init + game + clock + one scene)
/// for the save-schema tests.
fn author_save_base(root: &Path, name: &str) {
    let requests = vec![
        init(name),
        request(
            "game_define",
            json!({"id": "save-game", "start_scene": "s"}),
        ),
        request("clock_define", json!({"slots": ["morning", "night"]})),
        request("scene_define", json!({"id": "s", "presentation": "vn"})),
    ];
    let answer = run_requests(root, root, &requests);
    assert_eq!(answer.status, Status::Ok, "save base: {}", answer.doc);
}

/// The canonical v1 schema input: an exact cover of the nine
/// state fields across the two partitions.
fn schema_v1() -> Value {
    json!({
        "version": 1, "slots": 8, "slot_holds": "one_run",
        "slot_fields": ["clock", "scene", "stats"],
        "globals_fields": [
            "ending", "flags_globals", "flags_run", "gallery", "rng_state", "triggers",
        ],
        "hash_algorithm": "sha256/1",
        "policy": {"kind": "any_tick_boundary"},
        "migrations": [],
    })
}

#[test]
fn flow_records_author_validate_and_replay_three_times() {
    let root = fresh("flow-replay");
    author_flow(&root);

    let entries = log(&root);
    let verbs: Vec<&str> = entries
        .iter()
        .map(|entry| entry["verb"].as_str().expect("verb"))
        .collect();
    for name in [
        "choice_define",
        "ending_define",
        "gallery_entry_define",
        "schedule_define",
        "scene_bind_rig",
        "scene_exit_define",
    ] {
        assert!(verbs.contains(&name), "{name} missing: {verbs:?}");
    }

    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean", "{}", validated.doc);

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
    author_flow(&root);
    let sha_before = project_sha(&root);

    // Rebinding the identical binding is a no-op.
    let noop = verb(
        &root,
        "scene_bind_rig",
        json!({
            "scene": "bedroom", "participant": "yui", "rig_model": "yui-rig",
            "entry_expression": "smile", "entry_motion": "idle-sway",
        }),
    );
    assert_ok(&noop);
    assert_eq!(project_sha(&root), sha_before);

    // Rebinding to a different model APPLIES (set semantics,
    // never a conflict).
    let applied = verb(
        &root,
        "scene_bind_rig",
        json!({
            "scene": "bedroom", "participant": "yui", "rig_model": "yui-rig-v2",
            "entry_expression": "smile", "entry_motion": "idle-sway",
        }),
    );
    assert_ok(&applied);
    assert_ne!(project_sha(&root), sha_before);

    // The other flow families keep the upsert law: a differing
    // redefinition conflicts.
    let mut changed_choice = choice_inputs();
    changed_choice["options"][0]["effect_set"] = json!("bond_five");
    assert_refused(
        &verb(&root, "choice_define", changed_choice),
        "TG-AUTH-CONFLICT",
    );
    assert_refused(
        &verb(
            &root,
            "ending_define",
            json!({
                "id": "good_end", "condition": "bond_mid", "priority": 10,
                "epilogue_scene": "hall",
            }),
        ),
        "TG-AUTH-CONFLICT",
    );
    // An identical redefinition is a no-op.
    assert_ok(&verb(&root, "choice_define", choice_inputs()));
}

#[test]
fn schedule_coverage_is_a_graph_invariant() {
    // No clock at all: the slot set does not exist.
    let root = fresh("flow-schedule-noclock");
    let requests = vec![
        init("flow-schedule-noclock"),
        request("game_define", json!({"id": "g", "start_scene": "s"})),
        request("scene_define", json!({"id": "s", "presentation": "vn"})),
        request(
            "character_define",
            json!({"id": "yui", "name_ref": "name_yui", "name_text": "Yui"}),
        ),
    ];
    let answer = run_requests(&root, &root, &requests);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    assert_refused(
        &verb(
            &root,
            "schedule_define",
            json!({
                "character": "yui",
                "slots": [{"slot": "morning", "scene": "s"}],
            }),
        ),
        "TG-GAME-GRAPH",
    );

    let root = fresh("flow-schedule");
    author_flow(&root);
    // A missing clock slot.
    assert_refused(
        &verb(
            &root,
            "schedule_define",
            json!({
                "character": "yui",
                "slots": [
                    {"slot": "morning", "scene": "bedroom"},
                    {"slot": "afternoon", "scene": "bedroom"},
                    {"slot": "evening", "scene": "hall"},
                ],
            }),
        ),
        "TG-GAME-GRAPH",
    );
    // An unknown clock slot.
    assert_refused(
        &verb(
            &root,
            "schedule_define",
            json!({
                "character": "yui",
                "slots": [
                    {"slot": "morning", "scene": "bedroom"},
                    {"slot": "afternoon", "scene": "bedroom"},
                    {"slot": "evening", "scene": "hall"},
                    {"slot": "night", "scene": "bedroom"},
                    {"slot": "brunch", "scene": "bedroom"},
                ],
            }),
        ),
        "TG-GAME-GRAPH",
    );
    // A duplicate slot row.
    assert_refused(
        &verb(
            &root,
            "schedule_define",
            json!({
                "character": "yui",
                "slots": [
                    {"slot": "morning", "scene": "bedroom"},
                    {"slot": "morning", "scene": "hall"},
                    {"slot": "afternoon", "scene": "bedroom"},
                    {"slot": "evening", "scene": "hall"},
                    {"slot": "night", "scene": "bedroom"},
                ],
            }),
        ),
        "TG-AUTH-MALFORMED",
    );
}

#[test]
fn choice_and_ending_graphs_are_checked() {
    let root = fresh("flow-choice-battery");
    author_flow(&root);
    // A choice needs at least one option.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({"id": "empty", "scene": "bedroom", "options": []}),
        ),
        "TG-AUTH-MALFORMED",
    );
    // At least one option must be unconditional.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({
                "id": "gated", "scene": "bedroom",
                "options": [{
                    "id": "only", "label_ref": "text_stay", "origin": "player",
                    "visible_if": "never",
                    "target": {"kind": "line", "id": "stay_line"},
                }],
            }),
        ),
        "TG-GAME-GRAPH",
    );
    // The origin must be a known tag.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({
                "id": "bad_origin", "scene": "bedroom",
                "options": [{
                    "id": "only", "label_ref": "text_stay", "origin": "nobody",
                    "target": {"kind": "line", "id": "stay_line"},
                }],
            }),
        ),
        "TG-AUTH-UNKNOWN-KIND",
    );
    // A boolean sort expression is a type error.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({
                "id": "bool_sort", "scene": "bedroom",
                "options": [{
                    "id": "only", "label_ref": "text_stay", "origin": "player",
                    "sort": {"kind": "bool_lit", "value": true},
                    "target": {"kind": "line", "id": "stay_line"},
                }],
            }),
        ),
        "TG-GAME-EXPR",
    );
    // A sort naming an unknown stat is an expression error.
    assert_refused(
        &verb(
            &root,
            "choice_define",
            json!({
                "id": "bad_sort", "scene": "bedroom",
                "options": [{
                    "id": "only", "label_ref": "text_stay", "origin": "player",
                    "sort": {"kind": "stat", "stat": "nope"},
                    "target": {"kind": "line", "id": "stay_line"},
                }],
            }),
        ),
        "TG-GAME-EXPR",
    );
    // Ending priorities are unique (priority 10 is taken).
    assert_refused(
        &verb(
            &root,
            "ending_define",
            json!({
                "id": "third_end", "condition": "bond_mid", "priority": 10,
                "epilogue_scene": "hall",
            }),
        ),
        "TG-GAME-GRAPH",
    );
}

#[test]
fn binding_and_gallery_references_resolve_at_define_time() {
    let root = fresh("flow-dangling");
    author_flow(&root);
    // An unknown entry expression.
    assert_refused(
        &verb(
            &root,
            "scene_bind_rig",
            json!({
                "scene": "bedroom", "participant": "yui", "rig_model": "yui-rig",
                "entry_expression": "missing",
            }),
        ),
        "TG-AUTH-DANGLING",
    );
    // An unknown entry motion.
    assert_refused(
        &verb(
            &root,
            "scene_bind_rig",
            json!({
                "scene": "bedroom", "participant": "yui", "rig_model": "yui-rig",
                "entry_expression": "smile", "entry_motion": "missing",
            }),
        ),
        "TG-AUTH-DANGLING",
    );
    // A participant who is not in the scene's participants.
    assert_refused(
        &verb(
            &root,
            "scene_bind_rig",
            json!({
                "scene": "hall", "participant": "yui", "rig_model": "yui-rig",
                "entry_expression": "smile",
            }),
        ),
        "TG-GAME-GRAPH",
    );
    // An unknown thumbnail asset.
    assert_refused(
        &verb(
            &root,
            "gallery_entry_define",
            json!({
                "id": "bad_thumb", "category": "scenes",
                "unlock": {"kind": "effect"},
                "replay_scene": "bedroom", "replay_line": "bedroom_line",
                "thumbnail": "missing",
            }),
        ),
        "TG-AUTH-DANGLING",
    );
}

#[test]
fn choice_content_references_refuse_at_decide_time() {
    // C-7: label/prompt keys are content.ron sidecar keys; a
    // choice naming prose nobody authored refuses at decide
    // time, not at runtime.
    let root = fresh("flow-content");
    author_flow(&root);
    let mut bad_label = choice_inputs();
    bad_label["id"] = json!("bad_label");
    bad_label["options"][0]["label_ref"] = json!("text_missing");
    assert_refused(&verb(&root, "choice_define", bad_label), "TG-AUTH-DANGLING");
    let mut bad_prompt = choice_inputs();
    bad_prompt["id"] = json!("bad_prompt");
    bad_prompt["prompt_line"] = json!("text_missing");
    assert_refused(
        &verb(&root, "choice_define", bad_prompt),
        "TG-AUTH-DANGLING",
    );
}

#[test]
fn game_manifest_counts_the_flow_verbs_live() {
    // The trunk census: GAME_MANIFEST directly (verbs_doc()
    // covers the rig manifest only — the documented
    // adaptation of the branch's census test).
    assert_eq!(GAME_MANIFEST.len(), 24);
    let implemented = GAME_MANIFEST.iter().filter(|spec| spec.implemented).count();
    assert_eq!(implemented, 24);
    for name in [
        "choice_define",
        "ending_define",
        "gallery_entry_define",
        "game_save_schema_define",
        "schedule_define",
        "scene_bind_rig",
        "scene_exit_define",
    ] {
        let spec = GAME_MANIFEST
            .iter()
            .find(|spec| spec.name == name)
            .expect("verb in manifest");
        assert!(spec.implemented, "{name} must be implemented");
    }
}

#[test]
fn save_schema_partition_and_chain_battery() {
    // Ported from the branch's game_gate.rs battery.
    let root = fresh("flow-schema");
    author_save_base(&root, "flow-schema");
    let define = |inputs: Value| verb(&root, "game_save_schema_define", inputs);

    // A field in both partitions.
    let mut both = schema_v1();
    both["globals_fields"] = json!([
        "clock",
        "ending",
        "flags_globals",
        "flags_run",
        "gallery",
        "rng_state",
        "triggers",
    ]);
    assert_refused(&define(both), "TG-GAME-SAVE-SCHEMA");
    // A field in neither partition.
    let mut neither = schema_v1();
    neither["globals_fields"] = json!([
        "flags_globals",
        "flags_run",
        "gallery",
        "rng_state",
        "triggers",
    ]);
    assert_refused(&define(neither), "TG-GAME-SAVE-SCHEMA");
    // An unknown field.
    let mut unknown = schema_v1();
    unknown["slot_fields"] = json!(["clock", "scene", "stats", "bogus"]);
    assert_refused(&define(unknown), "TG-GAME-SAVE-SCHEMA");
    // Slot counts outside 1..=64.
    let mut zero = schema_v1();
    zero["slots"] = json!(0);
    assert_refused(&define(zero), "TG-GAME-SAVE-SCHEMA");
    let mut many = schema_v1();
    many["slots"] = json!(65);
    assert_refused(&define(many), "TG-GAME-SAVE-SCHEMA");
    // An unversioned hash name is a schema refusal; a foreign
    // versioned name is an unknown algorithm.
    let mut unversioned = schema_v1();
    unversioned["hash_algorithm"] = json!("sha256");
    assert_refused(&define(unversioned), "TG-GAME-SAVE-SCHEMA");
    let mut foreign = schema_v1();
    foreign["hash_algorithm"] = json!("md5/1");
    assert_refused(&define(foreign), "TG-AUTH-UNKNOWN-KIND");
    // A gapped chain: version 3 whose only step starts at 1
    // ends at 1, not at version - 1.
    let mut gapped = schema_v1();
    gapped["version"] = json!(3);
    gapped["migrations"] = json!([{"from": 1, "name": "first"}]);
    assert_refused(&define(gapped), "TG-GAME-SAVE-SCHEMA");

    // The valid v1 schema authors; an identical redefinition
    // is a no-op (bytes unchanged).
    assert_ok(&define(schema_v1()));
    let sha_v1 = project_sha(&root);
    assert_ok(&define(schema_v1()));
    assert_eq!(project_sha(&root), sha_v1);
    // An orphaned v2: the version moved but the chain is
    // empty.
    let mut orphan = schema_v1();
    orphan["version"] = json!(2);
    orphan["slots"] = json!(12);
    assert_refused(&define(orphan.clone()), "TG-GAME-SAVE-SCHEMA");
    // A properly chained v2 authors.
    let mut chained = orphan;
    chained["migrations"] = json!([{"from": 1, "name": "widen-slots"}]);
    assert_ok(&define(chained));
    // The same version with different content does not
    // strictly increase.
    let mut same = schema_v1();
    same["version"] = json!(2);
    same["slots"] = json!(16);
    same["migrations"] = json!([{"from": 1, "name": "other"}]);
    assert_refused(&define(same), "TG-GAME-SAVE-SCHEMA");

    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean", "{}", validated.doc);
}

#[test]
fn derived_schema_version_binds_saves_and_walks_the_chain() {
    // C-4: the binding version is derived from the authored
    // schema when one exists.
    let root = fresh("flow-derived");
    author_save_base(&root, "flow-derived");
    let sha = project_sha(&root);
    let load_project = |root: &Path| {
        torogao_author::store::load(root)
            .expect("load runs")
            .expect("project exists")
            .project
    };

    // Pre-schema: the stored scalar (default 1) binds.
    let project = load_project(&root);
    let state = genesis(&project, 7).expect("genesis");
    let save_v1 = save_game(&project, &sha, &state, 7).expect("save at v1");
    assert!(save_v1.contains("schema_version: 1"), "{save_v1}");
    let loaded = load_game(&project, &sha, &save_v1).expect("v1 save loads");
    assert!(loaded.migrations_applied.is_empty());

    // Author schema v2 over the scalar default: the derived
    // version is now 2, new saves bind 2, and the v1 save
    // walks the authored chain by name.
    let mut chained = schema_v1();
    chained["version"] = json!(2);
    chained["migrations"] = json!([{"from": 1, "name": "widen-slots"}]);
    assert_ok(&verb(&root, "game_save_schema_define", chained));
    let project = load_project(&root);
    let save_v2 = save_game(&project, &sha, &state, 7).expect("save at v2");
    assert!(save_v2.contains("schema_version: 2"), "{save_v2}");
    let loaded = load_game(&project, &sha, &save_v1).expect("v1 save migrates");
    assert_eq!(loaded.migrations_applied, vec!["widen-slots".to_string()]);
    assert_eq!(loaded.state, state);

    // A save newer than the derived version refuses.
    let future = save_v1.replace("schema_version: 1", "schema_version: 9");
    assert_ne!(future, save_v1);
    let refused = load_game(&project, &sha, &future).expect_err("newer save refuses");
    assert_eq!(refused.class, torogao_author::RefusalClass::GameSaveSchema);

    // Advance to v3 with a chain segment starting at 2: the
    // v1 save can no longer reach the derived version (no
    // step from 1 in the current schema's chain).
    let mut v3 = schema_v1();
    v3["version"] = json!(3);
    v3["migrations"] = json!([{"from": 2, "name": "third"}]);
    assert_ok(&verb(&root, "game_save_schema_define", v3));
    let project = load_project(&root);
    let refused = load_game(&project, &sha, &save_v1).expect_err("incomplete chain refuses");
    assert_eq!(refused.class, torogao_author::RefusalClass::GameSaveSchema);
    // The v2 save still walks its one step.
    let loaded = load_game(&project, &sha, &save_v2).expect("v2 save migrates");
    assert_eq!(loaded.migrations_applied, vec!["third".to_string()]);
}

#[test]
fn choice_consumption_applies_effects_veto_and_targets() {
    let root = fresh("flow-consume");
    author_flow(&root);
    let project = torogao_author::store::load(&root)
        .expect("load runs")
        .expect("project exists")
        .project;
    let game_stat = |state: &torogao_author::game::state::GameState, stat: &str| {
        state
            .stats
            .get(&StatKey {
                character: None,
                stat: stat.to_string(),
            })
            .copied()
            .unwrap_or(-1)
    };
    let run = |entries: Vec<Value>| {
        let inputs: Vec<torogao_author::game::op::LogEntryInput> =
            serde_json::from_value(json!(entries)).expect("log inputs parse");
        let log = parse_log(&inputs, 4).expect("log parses");
        let mut state = genesis(&project, 7).expect("genesis");
        let mut events = Vec::new();
        let mut chain = Vec::new();
        run_ticks(&project, &mut state, &log, 4, &mut events, &mut chain)
            .map(|()| (state, events, chain))
    };

    // Picking "stay" (player origin) while the alter-ego
    // option "leave" was offered: the option's set applies,
    // then the veto set as its own transaction.
    let (state, events, _) = run(vec![json!({
        "tick": 1, "kind": "choice", "id": "morning_choice", "option": "stay",
    })])
    .expect("stay run");
    assert_eq!(game_stat(&state, "bond"), 1, "events: {events:?}");
    assert_eq!(game_stat(&state, "temptation"), 1, "events: {events:?}");
    assert_eq!(state.line.as_deref(), Some("stay_line"));
    assert!(
        events
            .iter()
            .any(|event| event.contains("presented [stay, sleep, leave]")),
        "events: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event.contains("the alter-ego veto applied effect set 'veto_cost'")),
        "events: {events:?}"
    );

    // Picking the alter-ego option itself: no veto cost, its
    // own set applies, and the exit target transitions.
    let (state, events, _) = run(vec![json!({
        "tick": 1, "kind": "choice", "id": "morning_choice", "option": "leave",
    })])
    .expect("leave run");
    assert_eq!(game_stat(&state, "temptation"), 0, "events: {events:?}");
    assert!(state.flags_run.get("left_room").copied().unwrap_or(false));
    assert_eq!(state.current_scene.as_deref(), Some("hall"));
    assert!(
        !events.iter().any(|event| event.contains("veto")),
        "events: {events:?}"
    );

    // Through the verb: the presented order lands in the run
    // trace and the run outcome reports no ending.
    let answer = verb(
        &root,
        "game_simulate",
        json!({
            "run_seed": 7, "ticks": 1,
            "choice_log": [{
                "tick": 1, "kind": "choice", "id": "morning_choice", "option": "stay",
            }],
        }),
    );
    assert_ok(&answer);
    assert_eq!(answer.doc["ending"], Value::Null);
    assert_eq!(answer.doc["final"]["line"], "stay_line");
    let events = answer.doc["events"].as_array().expect("events");
    assert!(
        events.iter().any(|event| event
            .as_str()
            .is_some_and(|e| e.contains("presented [stay, sleep, leave]"))),
        "events: {}",
        answer.doc
    );
}

#[test]
fn choice_inputs_refuse_when_the_log_diverges() {
    let root = fresh("flow-choice-refusals");
    author_flow(&root);
    let simulate = |log: Value| {
        verb(
            &root,
            "game_simulate",
            json!({"run_seed": 7, "ticks": 2, "choice_log": log}),
        )
    };
    // An unknown choice.
    assert_refused(
        &simulate(json!([{"tick": 1, "kind": "choice", "id": "nope", "option": "stay"}])),
        "TG-GAME-GRAPH",
    );
    // An unknown option.
    assert_refused(
        &simulate(json!([{
            "tick": 1, "kind": "choice", "id": "morning_choice", "option": "nope",
        }])),
        "TG-GAME-GRAPH",
    );
    // A non-offered option (sneak is gated invisible).
    assert_refused(
        &simulate(json!([{
            "tick": 1, "kind": "choice", "id": "morning_choice", "option": "sneak",
        }])),
        "TG-GAME-GRAPH",
    );
    // A choice input with no option at all.
    assert_refused(
        &simulate(json!([{"tick": 1, "kind": "choice", "id": "morning_choice"}])),
        "TG-AUTH-MALFORMED",
    );
    // The choice belongs to the bedroom; after leaving, the
    // run is in the hall and the same choice no longer
    // resolves there.
    assert_refused(
        &simulate(json!([
            {"tick": 1, "kind": "choice", "id": "morning_choice", "option": "leave"},
            {"tick": 2, "kind": "choice", "id": "morning_choice", "option": "stay"},
        ])),
        "TG-GAME-GRAPH",
    );
}

#[test]
fn endings_evaluate_in_priority_order_and_stop_the_run() {
    let root = fresh("flow-endings");
    author_flow(&root);
    // The wake event sets bond to 5: both endings' conditions
    // hold (5 >= 5 and 5 >= 3), and the lower priority number
    // (good_end, 10) wins over quiet_end (20).
    let answer = verb(
        &root,
        "game_simulate",
        json!({
            "run_seed": 7, "ticks": 5,
            "choice_log": [{"tick": 1, "kind": "event", "id": "wake"}],
        }),
    );
    assert_ok(&answer);
    assert_eq!(answer.doc["ending"], "good_end", "{}", answer.doc);
    let chain = answer.doc["chain"].as_array().expect("chain");
    assert_eq!(chain.len(), 1, "the run stops at the ending tick");
    let events = answer.doc["events"].as_array().expect("events");
    assert!(
        events.iter().any(|event| {
            event
                .as_str()
                .is_some_and(|e| e.contains("ending 'good_end' reached"))
        }),
        "events: {}",
        answer.doc
    );

    // No event: no ending, the full run elapses.
    let answer = verb(
        &root,
        "game_simulate",
        json!({"run_seed": 7, "ticks": 2, "choice_log": []}),
    );
    assert_ok(&answer);
    assert_eq!(answer.doc["ending"], Value::Null);
    assert_eq!(answer.doc["chain"].as_array().expect("chain").len(), 2);
}

#[test]
fn scene_growth_round_trips_and_old_scenes_keep_their_bytes() {
    // A scene authored without the C-8 fields serializes
    // exactly as before the growth.
    let root = fresh("flow-scene-old");
    author_save_base(&root, "flow-scene-old");
    let bytes = std::fs::read_to_string(root.join("project.ron")).expect("project bytes");
    assert!(bytes.contains("Scene("), "{bytes}");
    for absent in [
        "participants",
        "backdrop",
        "entry_condition",
        "on_enter",
        "on_exit",
    ] {
        assert!(!bytes.contains(absent), "old bytes carry {absent}: {bytes}");
    }

    // A scene authored with the growth fields validates and
    // replays byte-identically.
    let root = fresh("flow-scene-new");
    let requests = vec![
        init("flow-scene-new"),
        request("game_define", json!({"id": "g", "start_scene": "s"})),
        request("clock_define", json!({"slots": ["morning"]})),
        request(
            "stat_define",
            json!({
                "id": "bond", "scope": "game", "kind": {"kind": "int"},
                "min": 0, "max": 10, "default": 0, "overflow": "clamp",
            }),
        ),
        request(
            "condition_define",
            json!({"id": "always", "expr": {"kind": "bool_lit", "value": true}}),
        ),
        request(
            "effect_set_define",
            json!({
                "id": "ping",
                "atoms": [{
                    "kind": "stat_add", "stat": "bond",
                    "amount": {"kind": "lit", "value": 1},
                }],
            }),
        ),
        request(
            "character_define",
            json!({"id": "yui", "name_ref": "name_yui", "name_text": "Yui"}),
        ),
        request(
            "scene_define",
            json!({
                "id": "s", "presentation": "vn", "participants": ["yui"],
                "backdrop": "bg_room", "entry_condition": "always",
                "on_enter": "ping", "on_exit": "ping",
            }),
        ),
    ];
    let answer = run_requests(&root, &root, &requests);
    assert_eq!(answer.status, Status::Ok, "grown scene: {}", answer.doc);
    let bytes = std::fs::read_to_string(root.join("project.ron")).expect("project bytes");
    assert!(bytes.contains("participants"), "{bytes}");
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean", "{}", validated.doc);
    let replayed = query(&root, "project_replay");
    assert_eq!(replayed.status, Status::Ok, "{}", replayed.doc);
    assert_eq!(replayed.doc["sha256"], project_sha(&root).as_str());
}

#[test]
fn plain_vn_projects_gain_no_flow_sections() {
    // The negative gate: a pre-port shape of project (game,
    // scenes, a character, a two-line chain) authors,
    // validates, simulates, and replays exactly as before —
    // no flow section names appear in its bytes.
    let root = fresh("flow-plain-vn");
    let requests = vec![
        init("flow-plain-vn"),
        request("game_define", json!({"id": "vn", "start_scene": "room"})),
        request("clock_define", json!({"slots": ["morning", "night"]})),
        request(
            "character_define",
            json!({"id": "amy", "name_ref": "name_amy", "name_text": "Amy"}),
        ),
        request("scene_define", json!({"id": "room", "presentation": "vn"})),
        request(
            "dialogue_line_define",
            json!({
                "id": "l1", "scene": "room", "ordinal": 0, "speaker": "amy",
                "text_ref": "t1", "text": "Hello.", "next": "l2",
            }),
        ),
        request(
            "dialogue_line_define",
            json!({
                "id": "l2", "scene": "room", "ordinal": 1, "speaker": "amy",
                "text_ref": "t2", "text": "World.",
            }),
        ),
    ];
    let answer = run_requests(&root, &root, &requests);
    assert_eq!(answer.status, Status::Ok, "plain vn: {}", answer.doc);
    let bytes = std::fs::read_to_string(root.join("project.ron")).expect("project bytes");
    for absent in [
        "alter_ego_veto",
        "backdrop",
        "choices",
        "endings",
        "gallery_entries",
        "participants",
        "save_schema:",
        "schedules",
        "scene_bindings",
        "scene_exits",
        "GameChoice",
        "SaveSchema",
    ] {
        assert!(!bytes.contains(absent), "plain VN bytes carry {absent}");
    }
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean", "{}", validated.doc);
    let simulated = verb(
        &root,
        "game_simulate",
        json!({
            "run_seed": 3, "ticks": 1,
            "choice_log": [{"tick": 1, "kind": "advance", "id": ""}],
        }),
    );
    assert_ok(&simulated);
    assert_eq!(simulated.doc["final"]["line"], "l2");
    assert_eq!(simulated.doc["ending"], Value::Null);
    let on_disk = project_sha(&root);
    for run in 1..=3 {
        let replayed = query(&root, "project_replay");
        assert_eq!(replayed.status, Status::Ok, "run {run}: {}", replayed.doc);
        assert_eq!(replayed.doc["sha256"], on_disk.as_str(), "run {run}");
    }
}
