//! The G-10 author-completion gate (VERSION-ASSESSMENT §8
//! G-10): the nine verbs the completion implements — A2 physics
//! records, A3 motion and audio records, and `export_manifest`
//! — author through decide() → apply → op log, validate clean,
//! and replay byte-identically ×3; the refusal battery fires
//! per family; the eight descoped verbs refuse by name with
//! their ruling; and the verb census reads 39 implemented +
//! 8 descoped of 47.

mod common;

use common::{fresh, init, log, project_sha, query, request, verb};
use serde_json::{Value, json};
use std::path::Path;
use torogao_author::{Status, run_requests, verbs_doc};

fn physics_group() -> Value {
    json!({
        "id": "hair-physics",
        "gravity": [0.0, -9.8],
        "wind": [0.5, 0.0],
        "turbulence_amplitude": 0.25,
        "inputs": [{"parameter": "angle-x", "scale_degrees": 30.0}],
        "strands": [{
            "id": "front-strand",
            "anchor": {"kind": "mesh", "id": "hair", "offset": [0.0, 0.0]},
            "particles": [
                {"length": 12.0, "radius": 3.0, "mobility": 0.9, "delay": 0.1, "damping": 0.05},
                {"length": 10.0, "radius": 2.0, "mobility": 0.8, "delay": 0.2, "damping": 0.05},
            ],
            "output": {"parameter": "hair-sway", "scale": 1.5, "offset": 0.0},
        }],
    })
}

/// Authors the G-10 fixture: two parameters, a part, an
/// expression, then one record per implemented family.
fn build_rigx(root: &Path) {
    let requests = vec![
        init("rigx-gate"),
        request(
            "param_define",
            json!({"id": "angle-x", "class": "angle", "min": -30.0, "default": 0.0, "max": 30.0}),
        ),
        request(
            "param_define",
            json!({"id": "hair-sway", "class": "physics", "min": -45.0, "default": 0.0, "max": 45.0}),
        ),
        request("part_create", json!({"id": "hair"})),
        request(
            "expression_define",
            json!({
                "id": "smile",
                "blend": "additive",
                "values": [{"parameter": "hair-sway", "value": 1.0}],
            }),
        ),
        request("physics_group_define", physics_group()),
        request(
            "motion_create",
            json!({"id": "idle-sway", "length_ticks": 120, "looped": true}),
        ),
        request(
            "curve_set",
            json!({
                "motion": "idle-sway",
                "parameter": "hair-sway",
                "keyframes": [{"tick": 0, "value": 0.0}, {"tick": 60, "value": 20.0}],
            }),
        ),
        request(
            "audio_track_define",
            json!({
                "id": "theme",
                "markers": [{"id": "verse", "tick": 0}, {"id": "chorus", "tick": 480}],
            }),
        ),
        request(
            "audio_bind_set",
            json!({
                "track": "theme",
                "marker": "chorus",
                "target": {"kind": "parameter_offset", "parameter": "hair-sway", "offset": 5.0},
                "sync_offset_ticks": -2,
                "peak_tick": 6,
                "trail_ticks": 24,
            }),
        ),
    ];
    let answer = run_requests(root, root, &requests);
    assert_eq!(answer.status, Status::Ok, "rigx fixture: {}", answer.doc);
}

/// Asserts a verb answer is a refusal carrying `code`.
fn assert_refused(answer: &torogao_author::Answer, code: &str) {
    assert_eq!(answer.status, Status::Refused, "answer: {}", answer.doc);
    assert_eq!(answer.doc["code"], code, "answer: {}", answer.doc);
}

#[test]
fn rigx_records_author_validate_and_replay_three_times() {
    let root = fresh("rigx-replay");
    build_rigx(&root);

    let entries = log(&root);
    let verbs: Vec<&str> = entries
        .iter()
        .map(|entry| entry["verb"].as_str().expect("verb"))
        .collect();
    assert!(verbs.contains(&"physics_group_define"), "{verbs:?}");
    assert!(verbs.contains(&"motion_create"), "{verbs:?}");
    assert!(verbs.contains(&"curve_set"), "{verbs:?}");
    assert!(verbs.contains(&"audio_track_define"), "{verbs:?}");
    assert!(verbs.contains(&"audio_bind_set"), "{verbs:?}");

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
fn forces_set_and_group_remove() {
    let root = fresh("rigx-forces");
    build_rigx(&root);
    let sha_before = project_sha(&root);

    // Setting the recorded forces is a no-op: bytes unchanged.
    let noop = verb(
        &root,
        "physics_forces_set",
        json!({"group": "hair-physics", "gravity": [0.0, -9.8]}),
    );
    assert_eq!(noop.status, Status::Ok, "{}", noop.doc);
    assert_eq!(project_sha(&root), sha_before);

    // Changing the wind applies and revalidates.
    let applied = verb(
        &root,
        "physics_forces_set",
        json!({"group": "hair-physics", "wind": [2.0, 0.0]}),
    );
    assert_eq!(applied.status, Status::Ok, "{}", applied.doc);
    assert_ne!(project_sha(&root), sha_before);
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);

    // Identical redefinition of the group now conflicts (its
    // forces changed), and removal works exactly once.
    assert_refused(
        &verb(&root, "physics_group_define", physics_group()),
        "TG-AUTH-CONFLICT",
    );
    let removed = verb(&root, "physics_group_remove", json!({"id": "hair-physics"}));
    assert_eq!(removed.status, Status::Ok, "{}", removed.doc);
    assert_refused(
        &verb(&root, "physics_group_remove", json!({"id": "hair-physics"})),
        "TG-AUTH-UNKNOWN-TARGET",
    );
}

#[test]
fn sampled_curves_carry_their_citation() {
    let root = fresh("rigx-samples");
    build_rigx(&root);

    let sampled = verb(
        &root,
        "curve_from_samples",
        json!({
            "motion": "idle-sway",
            "parameter": "angle-x",
            "samples": [{"tick": 0, "value": -10.0}, {"tick": 30, "value": 12.5}],
            "source": "scene study take 4, measured 2026-10-30",
        }),
    );
    assert_eq!(sampled.status, Status::Ok, "{}", sampled.doc);
    let bytes = std::fs::read_to_string(root.join("project.ron")).expect("project bytes");
    assert!(
        bytes.contains("scene study take 4, measured 2026-10-30"),
        "the citation is in the canonical record"
    );
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    let replayed = query(&root, "project_replay");
    assert_eq!(replayed.status, Status::Ok, "{}", replayed.doc);
    assert_eq!(replayed.doc["sha256"], project_sha(&root).as_str());

    // Samples without a citation are not evidence.
    assert_refused(
        &verb(
            &root,
            "curve_from_samples",
            json!({
                "motion": "idle-sway",
                "parameter": "angle-x",
                "samples": [{"tick": 0, "value": 0.0}],
                "source": "",
            }),
        ),
        "TG-AUTH-MALFORMED",
    );
}

#[test]
fn export_manifest_reports_constructs_and_lane() {
    let root = fresh("rigx-manifest");
    build_rigx(&root);

    let answer = verb(&root, "export_manifest", json!({}));
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let doc = &answer.doc;
    assert_eq!(doc["name"], "rigx-gate");
    assert_eq!(doc["evaluation"], "rust-lane-only");
    let constructs: Vec<&str> = doc["required_constructs"]
        .as_array()
        .expect("constructs")
        .iter()
        .map(|item| item.as_str().expect("construct"))
        .collect();
    for want in [
        "audio_bindings",
        "audio_tracks",
        "expressions",
        "motions",
        "parameters",
        "parts",
        "physics_groups",
    ] {
        assert!(constructs.contains(&want), "{want} in {constructs:?}");
    }
    assert_eq!(doc["counts"]["physics_groups"], 1);
    assert_eq!(doc["counts"]["motions"], 1);
    assert_eq!(doc["stamp"]["lane"], "all-ages");
    assert_eq!(doc["consent"]["schema"], "torogao-consent/1");

    // A bare project uses nothing the pre-port tooling lacks.
    let bare = fresh("rigx-manifest-bare");
    let answer = run_requests(&bare, &bare, &[init("rigx-bare")]);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let answer = verb(&bare, "export_manifest", json!({}));
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    assert_eq!(answer.doc["evaluation"], "d-evaluable");
    assert_eq!(
        answer.doc["required_constructs"]
            .as_array()
            .expect("constructs")
            .len(),
        0
    );
}

#[test]
fn census_reads_39_implemented_and_8_descoped() {
    let doc = verbs_doc();
    assert_eq!(doc["manifest_count"], 47);
    assert_eq!(doc["implemented_count"], 39);
    assert_eq!(doc["descoped_count"], 8);
    assert_eq!(
        doc["deferred"].as_array().expect("deferred").len(),
        0,
        "no manifest verb is left merely deferred: {doc}"
    );
}

#[test]
fn descoped_verbs_refuse_with_their_ruling() {
    let root = fresh("rigx-descoped");
    let answer = run_requests(&root, &root, &[init("rigx-descoped")]);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    for name in [
        "export_bundle",
        "export_native",
        "motion_bake",
        "param_link_define",
        "physics_bake",
        "render_range_sweep",
        "render_still",
        "render_take",
    ] {
        let refused = verb(&root, name, json!({}));
        assert_refused(&refused, "TG-AUTH-UNKNOWN-KIND");
        assert!(
            refused.doc["message"]
                .as_str()
                .expect("message")
                .contains("descoped"),
            "{name}: {}",
            refused.doc
        );
    }
}

#[test]
fn physics_refusal_battery() {
    let root = fresh("rigx-physics-refusals");
    build_rigx(&root);

    // An anchor naming nothing.
    let mut dangling_anchor = physics_group();
    dangling_anchor["id"] = json!("other");
    dangling_anchor["strands"][0]["anchor"]["id"] = json!("no-such-part");
    assert_refused(
        &verb(&root, "physics_group_define", dangling_anchor),
        "TG-AUTH-DANGLING",
    );
    // An input parameter that is not defined.
    let mut dangling_param = physics_group();
    dangling_param["id"] = json!("other");
    dangling_param["inputs"] = json!([{"parameter": "nope", "scale_degrees": 1.0}]);
    assert_refused(
        &verb(&root, "physics_group_define", dangling_param),
        "TG-AUTH-DANGLING",
    );
    // A group with no strands simulates nothing.
    let mut empty = physics_group();
    empty["id"] = json!("other");
    empty["strands"] = json!([]);
    assert_refused(
        &verb(&root, "physics_group_define", empty),
        "TG-AUTH-MALFORMED",
    );
    // Forces on a group that does not exist; forces naming
    // nothing to set.
    assert_refused(
        &verb(
            &root,
            "physics_forces_set",
            json!({"group": "nope", "wind": [1.0, 0.0]}),
        ),
        "TG-AUTH-UNKNOWN-TARGET",
    );
    assert_refused(
        &verb(
            &root,
            "physics_forces_set",
            json!({"group": "hair-physics"}),
        ),
        "TG-AUTH-MALFORMED",
    );
}

#[test]
fn motion_refusal_battery() {
    let root = fresh("rigx-motion-refusals");
    build_rigx(&root);

    assert_refused(
        &verb(
            &root,
            "motion_create",
            json!({"id": "bad", "length_ticks": 0}),
        ),
        "TG-AUTH-RANGE",
    );
    assert_refused(
        &verb(
            &root,
            "motion_create",
            json!({"id": "idle-sway", "length_ticks": 60, "looped": true}),
        ),
        "TG-AUTH-CONFLICT",
    );
    // A curve on a motion that does not exist.
    assert_refused(
        &verb(
            &root,
            "curve_set",
            json!({
                "motion": "nope",
                "parameter": "hair-sway",
                "keyframes": [{"tick": 0, "value": 0.0}],
            }),
        ),
        "TG-AUTH-UNKNOWN-TARGET",
    );
    // A curve driving an undefined parameter.
    assert_refused(
        &verb(
            &root,
            "curve_set",
            json!({
                "motion": "idle-sway",
                "parameter": "nope",
                "keyframes": [{"tick": 0, "value": 0.0}],
            }),
        ),
        "TG-AUTH-DANGLING",
    );
    // Ticks that do not strictly ascend.
    assert_refused(
        &verb(
            &root,
            "curve_set",
            json!({
                "motion": "idle-sway",
                "parameter": "angle-x",
                "keyframes": [{"tick": 10, "value": 0.0}, {"tick": 10, "value": 1.0}],
            }),
        ),
        "TG-AUTH-MALFORMED",
    );
    // A tick outside the clip's length.
    assert_refused(
        &verb(
            &root,
            "curve_set",
            json!({
                "motion": "idle-sway",
                "parameter": "angle-x",
                "keyframes": [{"tick": 120, "value": 0.0}],
            }),
        ),
        "TG-AUTH-RANGE",
    );
    // A value outside the parameter's declared range.
    assert_refused(
        &verb(
            &root,
            "curve_set",
            json!({
                "motion": "idle-sway",
                "parameter": "angle-x",
                "keyframes": [{"tick": 0, "value": 31.0}],
            }),
        ),
        "TG-AUTH-RANGE",
    );
}

#[test]
fn audio_refusal_battery() {
    let root = fresh("rigx-audio-refusals");
    build_rigx(&root);

    // A track with no markers; a track naming a marker twice.
    assert_refused(
        &verb(
            &root,
            "audio_track_define",
            json!({"id": "t", "markers": []}),
        ),
        "TG-AUTH-MALFORMED",
    );
    assert_refused(
        &verb(
            &root,
            "audio_track_define",
            json!({
                "id": "t",
                "markers": [{"id": "m", "tick": 0}, {"id": "m", "tick": 9}],
            }),
        ),
        "TG-AUTH-MALFORMED",
    );
    // A binding on a track that does not exist.
    assert_refused(
        &verb(
            &root,
            "audio_bind_set",
            json!({
                "track": "nope",
                "marker": "chorus",
                "target": {"kind": "parameter_offset", "parameter": "hair-sway", "offset": 1.0},
                "peak_tick": 1,
                "trail_ticks": 1,
            }),
        ),
        "TG-AUTH-UNKNOWN-TARGET",
    );
    // A binding on a marker the track does not carry.
    assert_refused(
        &verb(
            &root,
            "audio_bind_set",
            json!({
                "track": "theme",
                "marker": "bridge",
                "target": {"kind": "parameter_offset", "parameter": "hair-sway", "offset": 1.0},
                "peak_tick": 1,
                "trail_ticks": 1,
            }),
        ),
        "TG-AUTH-DANGLING",
    );
    // A binding driving an undefined expression.
    assert_refused(
        &verb(
            &root,
            "audio_bind_set",
            json!({
                "track": "theme",
                "marker": "verse",
                "target": {"kind": "expression_trigger", "expression": "nope"},
                "peak_tick": 1,
                "trail_ticks": 1,
            }),
        ),
        "TG-AUTH-DANGLING",
    );
    // The expression-trigger form binds cleanly when it exists.
    let bound = verb(
        &root,
        "audio_bind_set",
        json!({
            "track": "theme",
            "marker": "verse",
            "target": {"kind": "expression_trigger", "expression": "smile"},
            "peak_tick": 3,
            "trail_ticks": 12,
        }),
    );
    assert_eq!(bound.status, Status::Ok, "{}", bound.doc);
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
}
