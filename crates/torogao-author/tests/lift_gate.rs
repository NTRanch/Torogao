//! The LIFT layer's surface gates (feature `gen-conditioning`,
//! D-838): the scene record stores and round-trips, the
//! freeze verbs land artifacts + provenance into it, and the
//! custody gate refuses a regeneration proposal before
//! compilation. Without the feature this file compiles to an
//! empty binary; the refusal side is gated in
//! `gencond_architecture_gate.rs`.

#![cfg(feature = "gen-conditioning")]

mod common;

use common::{fresh, init, verb};
use serde_json::{Value, json};
use torogao_author::{Status, run_requests};

// The same tiny real PNGs the conditioning gate uses: a
// varied portrait, a varied pose map (custody tests compile
// for real when the targets are open).
const PORTRAIT_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0,
    0, 0, 75, 109, 41, 220, 0, 0, 0, 38, 73, 68, 65, 84, 120, 156, 99, 248, 31, 181, 224, 117, 212,
    130, 235, 81, 11, 14, 71, 45, 88, 31, 181, 96, 118, 212, 130, 246, 168, 5, 197, 81, 11, 24,
    134, 150, 4, 0, 211, 62, 108, 193, 144, 63, 25, 39, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
    130,
];
const PORTRAIT_SHA: &str = "2dfcd633a5c959314715295f9e860489c719be0f66c420ba67e003a4d3ff3cf1";
const GRADIENT_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0,
    0, 0, 75, 109, 41, 220, 0, 0, 0, 108, 73, 68, 65, 84, 120, 156, 13, 201, 65, 1, 0, 48, 8, 3,
    49, 148, 160, 164, 74, 170, 132, 231, 169, 64, 9, 74, 170, 104, 203, 55, 85, 69, 23, 42, 92,
    76, 177, 197, 21, 41, 170, 154, 110, 212, 184, 153, 102, 155, 107, 210, 63, 68, 11, 9, 139, 17,
    43, 78, 68, 63, 76, 27, 25, 155, 49, 107, 206, 196, 63, 134, 30, 52, 120, 152, 97, 135, 27, 50,
    63, 150, 94, 180, 120, 153, 101, 151, 91, 178, 63, 142, 62, 116, 248, 152, 99, 143, 59, 114,
    63, 66, 7, 5, 135, 9, 27, 46, 36, 60, 176, 44, 84, 129, 49, 200, 227, 255, 0, 0, 0, 0, 73, 69,
    78, 68, 174, 66, 96, 130,
];
const GRADIENT_SHA: &str = "98b911c80d1457886f697f885a88771f2b928aec026703620257e0e48a939da9";

fn hash(byte: &str) -> String {
    byte.repeat(32)
}

fn scene_inputs() -> Value {
    json!({
        "record": {
            "schema": 1,
            "id": "vivienne-keeper-scene",
            "recipe": {
                "conditioning_sha256": hash("11"),
                "graph_sha256": hash("22"),
                "seed": 20261301,
                "backend": "seam-c-worker",
                "execution_context": "primo/a6000; pinned",
                "rerender": "pinned_context_byte_identical",
            },
            "extensions": [
                {"kind": "puppet2d", "state": {"puppet2d": {"format": "lumen-rig-model", "format_version": 1, "model_sha256": hash("33")}}},
                {"kind": "armature3d", "state": "not_yet_populated"},
                {"kind": "sprite_cels", "state": "not_yet_populated"},
                {"kind": "audio_events", "state": "not_yet_populated"},
            ],
            "custody": {"entries": [
                {"node": "face/eyes", "state": {"frozen": {"by": "owner", "reason": "hand-tuned expression"}}},
                {"node": "hair", "state": "open"},
            ]},
            "provenance": [],
            "frozen_tracks": [],
            "palette_snaps": [],
        }
    })
}

fn set_inputs() -> Value {
    json!({
        "id": "vivienne-keeper",
        "base": {
            "checkpoint": "waiIllustriousSDXL_v170",
            "latent": {"width": 1024, "height": 1280, "batch_size": 1},
            "sampler": {"steps": 30, "cfg": 6.0, "name": "euler_ancestral", "scheduler": "normal", "denoise": 1.0},
            "seed": 20261301,
            "filename_prefix": "keeper",
        },
        "conditioning": {
            "schema": 1,
            "text": {
                "positive": [{"slot": "identity", "text": "Vivienne, deep violet-black hair"}],
                "negative": "photograph, blur",
            },
            "lora_stack": [],
            "sources": [
                {
                    "id": "identity-ref",
                    "enabled": true,
                    "kind": {"reference_embed": {
                        "reference": {"path": "portraits/vivienne_begin.png", "sha256": PORTRAIT_SHA, "crop": "head_shoulders_square"},
                        "adapter": "ip-adapter-plus_sdxl",
                        "vision_encoder": "clip-vit-h",
                        "weight": 0.35,
                        "weight_type": "linear",
                        "combine": "concat",
                        "normalisation": {"per_reference": {"scaling": "K+V w/ C penalty"}},
                    }},
                },
                {
                    "id": "pose",
                    "enabled": true,
                    "kind": {"spatial_map": {
                        "model": "controlnet-union-sdxl",
                        "control_type": "openpose",
                        "map": {
                            "origin": {"rig_projection": {"rig": "vivienne-rig", "pose": "standing", "camera": "front"}},
                            "sha256": GRADIENT_SHA,
                        },
                        "strength": 0.5,
                    }},
                },
            ],
        },
    })
}

fn compile_inputs(extra: Value) -> Value {
    let mut map_files = serde_json::Map::new();
    map_files.insert(GRADIENT_SHA.to_string(), json!("maps/pose.png"));
    let mut inputs = json!({
        "bindings": {
            "waiIllustriousSDXL_v170": {"file": "waiIllustriousSDXL_v170.safetensors"},
            "ip-adapter-plus_sdxl": {"file": "ip-adapter-plus_sdxl_vit-h.safetensors", "preset": "PLUS (high strength)"},
            "clip-vit-h": {"file": "CLIP-ViT-H-14.safetensors"},
            "controlnet-union-sdxl": {"file": "controlnet-union-sdxl-promax.safetensors"},
        },
        "measurements": [
            {"reference_sha256": PORTRAIT_SHA, "vision_encoder": "clip-vit-h", "token_count": 257, "magnitude_mean": 11.25, "magnitude_max": 24.5},
        ],
        "map_files": Value::Object(map_files),
    });
    if let Value::Object(extra) = extra {
        for (key, value) in extra {
            inputs[key] = value;
        }
    }
    inputs
}

fn project_with_scene(name: &str) -> std::path::PathBuf {
    let root = fresh(name);
    let answer = run_requests(&root, &root, &[init("lift-gate")]);
    assert_eq!(answer.status, Status::Ok, "init: {}", answer.doc);
    std::fs::create_dir_all(root.join("portraits")).expect("portraits dir");
    std::fs::create_dir_all(root.join("maps")).expect("maps dir");
    std::fs::write(root.join("portraits/vivienne_begin.png"), PORTRAIT_PNG).expect("portrait");
    std::fs::write(root.join("maps/pose.png"), GRADIENT_PNG).expect("map");
    let answer = verb(&root, "scene_record_set", scene_inputs());
    assert_eq!(answer.status, Status::Ok, "scene set: {}", answer.doc);
    root
}

fn stored_scene(root: &std::path::Path) -> torogao_gencond::SceneRecord {
    let loaded = torogao_author::store::load(root)
        .expect("load")
        .expect("project exists");
    let scene = &loaded.project.scenes[0];
    torogao_gencond::SceneRecord::from_ron_tree(torogao_author::conditioning::ron_to_gencond_tree(
        &scene.record,
    ))
    .expect("stored record parses")
}

fn sine_track_input(scene_id: &str, epsilon: Option<f64>) -> Value {
    let samples: Vec<Value> = (0..21)
        .map(|i| {
            let time = i as f64 * 0.1;
            json!({"time": time, "value": time.sin()})
        })
        .collect();
    let mut input = json!({
        "scene_id": scene_id,
        "track": {"parameter": "angle-x", "samples": samples},
    });
    if let Some(epsilon) = epsilon {
        input["epsilon"] = json!(epsilon);
    }
    input
}

// ---- validation -------------------------------------------------

#[test]
fn scene_set_stores_the_record_and_repeats_as_no_op() {
    let root = project_with_scene("set");
    let text = std::fs::read_to_string(root.join("project.ron")).expect("project text");
    assert!(text.contains("Scene("), "section stored:\n{text}");
    assert!(text.contains("SceneRecord("));
    assert!(text.contains("Puppet2dRef("));
    assert!(text.contains("NotYetPopulated"));
    let validated = verb(&root, "project_validate", json!({}));
    assert_eq!(validated.status, Status::Ok, "validate: {}", validated.doc);
    let again = verb(&root, "scene_record_set", scene_inputs());
    assert_eq!(again.status, Status::Ok);
    assert_eq!(again.doc["result"], "no-op");
}

#[test]
fn stored_scene_round_trips_byte_identically() {
    let root = project_with_scene("round-trip");
    let loaded = torogao_author::store::load(&root)
        .expect("load")
        .expect("project exists");
    assert_eq!(loaded.project.scenes.len(), 1);
    assert_eq!(
        torogao_author::codec::emit_project(&loaded.project),
        loaded.text
    );
}

#[test]
fn freeze_track_lands_artifact_and_provenance_then_no_ops() {
    let root = project_with_scene("freeze-track");
    let answer = verb(
        &root,
        "freeze_track",
        sine_track_input("vivienne-keeper-scene", Some(0.01)),
    );
    assert_eq!(answer.status, Status::Ok, "freeze: {}", answer.doc);
    assert_eq!(answer.doc["result"], "applied");
    let scene = stored_scene(&root);
    assert_eq!(scene.frozen_tracks.len(), 1);
    assert_eq!(scene.frozen_tracks[0].parameter, "angle-x");
    assert!(scene.frozen_tracks[0].max_deviation <= 0.01);
    assert_eq!(scene.provenance.len(), 1);
    assert_eq!(
        scene.provenance[0].op,
        torogao_gencond::FreezeOp::FreezeTrack
    );
    assert_eq!(scene.provenance[0].epsilon, Some(0.01));
    assert_eq!(
        scene.provenance[0].epsilon_origin,
        Some(torogao_gencond::EpsilonOrigin::Caller)
    );
    // The identical freeze repeats as a no-op: no duplicate
    // provenance, no churn.
    let again = verb(
        &root,
        "freeze_track",
        sine_track_input("vivienne-keeper-scene", Some(0.01)),
    );
    assert_eq!(again.doc["result"], "no-op");
    assert_eq!(stored_scene(&root).provenance.len(), 1);
    // A tighter contract is a new freeze: applies, and the
    // provenance history grows.
    let tighter = verb(
        &root,
        "freeze_track",
        sine_track_input("vivienne-keeper-scene", Some(0.001)),
    );
    assert_eq!(tighter.doc["result"], "applied");
    let scene = stored_scene(&root);
    assert_eq!(scene.provenance.len(), 2);
    assert!(scene.frozen_tracks[0].max_deviation <= 0.001);
}

#[test]
fn freeze_track_without_epsilon_derives_it_from_the_track() {
    let root = project_with_scene("freeze-noise");
    let answer = verb(
        &root,
        "freeze_track",
        sine_track_input("vivienne-keeper-scene", None),
    );
    assert_eq!(answer.status, Status::Ok, "freeze: {}", answer.doc);
    let scene = stored_scene(&root);
    assert_eq!(
        scene.provenance[0].epsilon_origin,
        Some(torogao_gencond::EpsilonOrigin::TrackNoise)
    );
}

#[test]
fn freeze_palette_lands_the_snap() {
    let root = project_with_scene("freeze-palette");
    let answer = verb(
        &root,
        "freeze_palette",
        json!({
            "scene_id": "vivienne-keeper-scene",
            "colors": ["#a03c51", "#010101"],
            "palette": ["#000000", "#a03c50", "#ffffff"],
            "tolerance": 2,
        }),
    );
    assert_eq!(answer.status, Status::Ok, "snap: {}", answer.doc);
    let scene = stored_scene(&root);
    assert_eq!(scene.palette_snaps.len(), 1);
    assert_eq!(scene.palette_snaps[0].pairs.len(), 2);
    assert_eq!(scene.provenance.len(), 1);
    assert_eq!(scene.provenance[0].tolerance, Some(2));
}

#[test]
fn freeze_surface_is_byte_deterministic_across_projects() {
    let first = project_with_scene("bake-a");
    let second = project_with_scene("bake-b");
    for root in [&first, &second] {
        verb(
            root,
            "freeze_track",
            sine_track_input("vivienne-keeper-scene", Some(0.01)),
        );
        verb(
            root,
            "freeze_palette",
            json!({
                "scene_id": "vivienne-keeper-scene",
                "colors": ["#a03c51"],
                "palette": ["#a03c50"],
                "tolerance": 2,
            }),
        );
    }
    let a = std::fs::read(first.join("project.ron")).expect("a bytes");
    let b = std::fs::read(second.join("project.ron")).expect("b bytes");
    assert_eq!(
        a, b,
        "double-bake: same verb sequence, byte-identical projects"
    );
}

#[test]
fn custody_gate_passes_proposals_touching_only_open_nodes() {
    let root = project_with_scene("custody-open");
    verb(&root, "conditioning_set", set_inputs());
    let answer = verb(
        &root,
        "conditioning_compile",
        compile_inputs(
            json!({"scene_id": "vivienne-keeper-scene", "targets": ["hair", "wardrobe/hem"]}),
        ),
    );
    assert_eq!(answer.status, Status::Ok, "compile: {}", answer.doc);
    // And a proposal naming no scene compiles as Stage 1 did.
    let plain = verb(&root, "conditioning_compile", compile_inputs(json!({})));
    assert_eq!(plain.status, Status::Ok, "plain compile: {}", plain.doc);
    assert_eq!(answer.doc["graph_sha256"], plain.doc["graph_sha256"]);
}

// ---- adversarial ------------------------------------------------

#[test]
fn custody_gate_refuses_frozen_targets_before_compile() {
    let root = project_with_scene("custody-frozen");
    verb(&root, "conditioning_set", set_inputs());
    let answer = verb(
        &root,
        "conditioning_compile",
        compile_inputs(
            json!({"scene_id": "vivienne-keeper-scene", "targets": ["face/eyes", "hair"]}),
        ),
    );
    assert_eq!(answer.status, Status::Refused, "compile: {}", answer.doc);
    assert_eq!(answer.doc["code"], "TG-GENCOND-CUSTODY-FROZEN");
    assert!(
        answer.doc["message"]
            .as_str()
            .unwrap()
            .contains("face/eyes"),
        "the refusal names the frozen node: {}",
        answer.doc
    );
}

#[test]
fn custody_gate_refuses_a_missing_scene_by_name() {
    let root = project_with_scene("custody-missing");
    verb(&root, "conditioning_set", set_inputs());
    let answer = verb(
        &root,
        "conditioning_compile",
        compile_inputs(json!({"scene_id": "no-such-scene", "targets": []})),
    );
    assert_eq!(answer.status, Status::Refused);
    assert_eq!(answer.doc["code"], "TG-AUTH-UNKNOWN-TARGET");
}

#[test]
fn freeze_into_a_missing_scene_refuses() {
    let root = project_with_scene("freeze-missing");
    let answer = verb(
        &root,
        "freeze_track",
        sine_track_input("no-such-scene", Some(0.01)),
    );
    assert_eq!(answer.status, Status::Refused, "freeze: {}", answer.doc);
    assert_eq!(answer.doc["code"], "TG-AUTH-UNKNOWN-TARGET");
}

#[test]
fn freeze_palette_beyond_tolerance_refuses_with_the_gencond_code_named() {
    let root = project_with_scene("palette-refusal");
    let answer = verb(
        &root,
        "freeze_palette",
        json!({
            "scene_id": "vivienne-keeper-scene",
            "colors": ["#a03c50"],
            "palette": ["#000000", "#ffffff"],
            "tolerance": 2,
        }),
    );
    assert_eq!(answer.status, Status::Refused, "snap: {}", answer.doc);
    assert_eq!(answer.doc["code"], "TG-AUTH-MALFORMED");
    assert!(
        answer.doc["message"]
            .as_str()
            .unwrap()
            .contains("TG-GENCOND-PALETTE-UNMATCHED"),
        "the gencond code is named: {}",
        answer.doc
    );
    // Nothing landed.
    assert!(stored_scene(&root).palette_snaps.is_empty());
}

#[test]
fn unknown_medium_kind_in_a_stored_record_refuses_at_validate() {
    let root = project_with_scene("unknown-kind");
    let path = root.join("project.ron");
    let text = std::fs::read_to_string(&path).expect("project text");
    let poisoned = text.replacen("Puppet2d", "Hologram", 1);
    assert_ne!(poisoned, text, "the kind ident is in the file to poison");
    std::fs::write(&path, poisoned).expect("rewrite project");
    let answer = verb(&root, "project_validate", json!({}));
    assert_eq!(answer.status, Status::Findings, "validate: {}", answer.doc);
    assert!(
        answer.doc["findings"]
            .to_string()
            .contains("TG-GENCOND-UNKNOWN-KIND"),
        "the finding names the gencond code: {}",
        answer.doc
    );
}

#[test]
fn scene_set_refuses_a_record_whose_recipe_claims_unpinned_byte_identity() {
    let root = fresh("honesty");
    let answer = run_requests(&root, &root, &[init("lift-gate")]);
    assert_eq!(answer.status, Status::Ok, "init: {}", answer.doc);
    let mut inputs = scene_inputs();
    inputs["record"]["recipe"]["execution_context"] = json!("");
    let answer = verb(&root, "scene_record_set", inputs);
    assert_eq!(answer.status, Status::Refused, "set: {}", answer.doc);
    assert_eq!(answer.doc["code"], "TG-AUTH-MALFORMED");
}
