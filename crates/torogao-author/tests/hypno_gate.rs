//! The Q1 hypnosis gate (D-634): the binding verbs author
//! through decide() → apply → op log, the authored records
//! land in `project.ron` and round-trip byte-identically,
//! validation stays clean, identical re-sets are no-ops, and
//! the refusal battery fires. The end-to-end example is the
//! subliminal-audio pattern: a voice row plus a five-layer
//! hypnosis track (voice drone, carrier, bed, encoded
//! affirmations, bell) for the pattern session, with its
//! visual loop and overlay hook bound beside it.

mod common;

use common::{fresh, init, log, project_sha, verb};
use serde_json::{Value, json};
use std::path::Path;
use torogao_author::Status;

fn track_inputs() -> Value {
    json!({
        "id": "vivienne-patterns",
        "binding": {
            "kind": "hypnosis_track",
            "session": "session.vivienne.patterns",
            "layers": [
                {"kind": "voice", "stem": "narration-drone", "looped": true},
                {"kind": "carrier", "stem": "carrier-binaural"},
                {"kind": "bed", "stem": "bed-noise"},
                {"kind": "subliminal", "stem": "affirmations-encoded",
                 "gain_offset_db": -3.0, "envelope": "env.subliminal-fade",
                 "editions": ["full"]},
                {"kind": "sfx", "stem": "bell", "editions": ["full", "lite"]}
            ]
        }
    })
}

fn build_hypno(root: &Path) {
    let answer = torogao_author::run_requests(root, root, &[init("hypno-gate")]);
    assert_eq!(answer.status, Status::Ok, "init: {}", answer.doc);
    let voice = verb(
        root,
        "audio_binding_set",
        json!({
            "id": "vivienne-narration",
            "binding": {
                "kind": "voice",
                "character": "vivienne",
                "delivery": {"identity": "vivienne", "record": "torori/vivienne/cond-1"}
            }
        }),
    );
    assert_eq!(voice.status, Status::Ok, "voice: {}", voice.doc);
    let track = verb(root, "audio_binding_set", track_inputs());
    assert_eq!(track.status, Status::Ok, "track: {}", track.doc);
    let visual_loop = verb(
        root,
        "visual_binding_set",
        json!({
            "id": "vivienne-spiral-loop",
            "binding": {
                "kind": "visual_loop",
                "asset": "spiral-loop",
                "session": "session.vivienne.patterns",
                "sync": {"mode": "count", "count": 12},
                "duration_seconds": 6.0,
                "frame_rate": 60
            }
        }),
    );
    assert_eq!(visual_loop.status, Status::Ok, "loop: {}", visual_loop.doc);
    let hook = verb(
        root,
        "visual_binding_set",
        json!({
            "id": "vivienne-spiral-hook",
            "binding": {
                "kind": "overlay_hook",
                "hook": "spiral",
                "session": "session.vivienne.patterns",
                "seed": 7,
                "rotation_rate": 0.5,
                "opacity_envelope": "env.phase-opacity"
            }
        }),
    );
    assert_eq!(hook.status, Status::Ok, "hook: {}", hook.doc);
}

#[test]
fn hypnosis_pattern_authors_end_to_end() {
    let root = fresh("hypno-e2e");
    build_hypno(&root);
    // The records are in the project file, in canonical form.
    let text = std::fs::read_to_string(root.join("project.ron")).expect("project file");
    for needle in [
        "HypnosisTrack",
        "HypnosisLayer",
        "affirmations-encoded",
        "VisualBinding",
        "OverlayHook",
        "vivienne-patterns",
    ] {
        assert!(text.contains(needle), "project.ron carries {needle}");
    }
    // The op log records all four binding ops after init.
    let entries = log(&root);
    let verbs: Vec<&str> = entries
        .iter()
        .filter_map(|entry| entry["verb"].as_str())
        .collect();
    assert_eq!(
        verbs,
        vec![
            "project_init",
            "audio_binding_set",
            "audio_binding_set",
            "visual_binding_set",
            "visual_binding_set"
        ]
    );
    // Validation stays clean over the authored rows.
    let validate = verb(&root, "project_validate", json!({}));
    assert_eq!(validate.status, Status::Ok, "validate: {}", validate.doc);
}

#[test]
fn identical_reset_is_a_noop_and_record_is_stable() {
    let root = fresh("hypno-noop");
    build_hypno(&root);
    let before = project_sha(&root);
    let again = verb(&root, "audio_binding_set", track_inputs());
    assert_eq!(again.status, Status::Ok, "reset: {}", again.doc);
    assert_eq!(project_sha(&root), before, "no-op leaves the file alone");
    // A changed record replaces the row.
    let mut changed = track_inputs();
    changed["binding"]["layers"][2]["gain_offset_db"] = json!(-6.0);
    let replaced = verb(&root, "audio_binding_set", changed);
    assert_eq!(replaced.status, Status::Ok, "replace: {}", replaced.doc);
    assert_ne!(project_sha(&root), before);
}

#[test]
fn refusal_battery() {
    let root = fresh("hypno-refusals");
    let answer = torogao_author::run_requests(&root, &root, &[init("hypno-refuse")]);
    assert_eq!(answer.status, Status::Ok);
    // Duplicate layer kind.
    let mut dup = track_inputs();
    dup["binding"]["layers"]
        .as_array_mut()
        .expect("layers array")
        .push(json!({"kind": "bed", "stem": "bed-2"}));
    let refused = verb(&root, "audio_binding_set", dup);
    assert_eq!(refused.status, Status::Refused, "dup: {}", refused.doc);
    // Empty stack.
    let mut empty = track_inputs();
    empty["binding"]["layers"] = json!([]);
    let refused = verb(&root, "audio_binding_set", empty);
    assert_eq!(refused.status, Status::Refused, "empty: {}", refused.doc);
    // Unknown layer kind.
    let mut bad = track_inputs();
    bad["binding"]["layers"][0]["kind"] = json!("ultrasonic");
    let refused = verb(&root, "audio_binding_set", bad);
    assert_eq!(refused.status, Status::Refused, "kind: {}", refused.doc);
    // Gain offset beyond the bound.
    let mut loud = track_inputs();
    loud["binding"]["layers"][0]["gain_offset_db"] = json!(60.0);
    let refused = verb(&root, "audio_binding_set", loud);
    assert_eq!(refused.status, Status::Refused, "gain: {}", refused.doc);
    // A loop that repeats zero times.
    let refused = verb(
        &root,
        "visual_binding_set",
        json!({
            "id": "zero-loop",
            "binding": {
                "kind": "visual_loop",
                "asset": "spiral-loop",
                "session": "session.vivienne.patterns",
                "sync": {"mode": "count", "count": 0},
                "duration_seconds": 6.0,
                "frame_rate": 60
            }
        }),
    );
    assert_eq!(refused.status, Status::Refused, "loop: {}", refused.doc);
    // Nothing refused was written.
    let text = std::fs::read_to_string(root.join("project.ron")).expect("project file");
    assert!(!text.contains("HypnosisTrack"), "refusals never land");
}
