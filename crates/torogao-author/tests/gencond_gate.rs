//! The conditioning family's surface gates (feature
//! `gen-conditioning`): the set verb stores the record, the
//! compile verb produces the graph deterministically, and
//! the compiler's typed refusals reach the surface intact.
//! Without the feature this file compiles to an empty binary;
//! the refusal side is gated in `gencond_architecture_gate.rs`.

#![cfg(feature = "gen-conditioning")]

mod common;

use common::{fresh, init, verb};
use serde_json::{Value, json};
use torogao_author::{Status, run_requests};

// Tiny real PNGs (8x8 RGB), embedded so the gate needs no
// fixture files: a varied portrait, a varied pose map, and a
// uniform (all-black) pose map — the false off.
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
const UNIFORM_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0,
    0, 0, 75, 109, 41, 220, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 96, 24, 30, 0, 0, 0, 200, 0,
    1, 173, 64, 118, 34, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const UNIFORM_SHA: &str = "4a0945534329d26ce523a2f096e203aae59a9fffaec71895599fa3ef42d23f22";

fn set_inputs(map_sha: &str) -> Value {
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
                "positive": [
                    {"slot": "identity", "text": "Vivienne, deep violet-black hair"},
                    {"slot": "scene", "text": "in a candlelit library"},
                ],
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
                            "sha256": map_sha,
                        },
                        "strength": 0.5,
                    }},
                },
            ],
        },
    })
}

fn compile_inputs(map_sha: &str) -> Value {
    let mut map_files = serde_json::Map::new();
    map_files.insert(map_sha.to_string(), json!("maps/pose.png"));
    json!({
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
    })
}

fn project_with_assets(name: &str, map_bytes: &[u8]) -> std::path::PathBuf {
    let root = fresh(name);
    let answer = run_requests(&root, &root, &[init("gencond-gate")]);
    assert_eq!(answer.status, Status::Ok, "init: {}", answer.doc);
    std::fs::create_dir_all(root.join("portraits")).expect("portraits dir");
    std::fs::create_dir_all(root.join("maps")).expect("maps dir");
    std::fs::write(root.join("portraits/vivienne_begin.png"), PORTRAIT_PNG).expect("portrait");
    std::fs::write(root.join("maps/pose.png"), map_bytes).expect("map");
    root
}

#[test]
fn set_stores_the_section_and_repeats_as_no_op() {
    let root = project_with_assets("set", GRADIENT_PNG);
    let answer = verb(&root, "conditioning_set", set_inputs(GRADIENT_SHA));
    assert_eq!(answer.status, Status::Ok, "set: {}", answer.doc);
    assert_eq!(answer.doc["result"], "applied");
    let text = std::fs::read_to_string(root.join("project.ron")).expect("project text");
    assert!(text.contains("Generation("), "section stored:\n{text}");
    assert!(text.contains("Conditioning("));
    assert!(text.contains("HeadShouldersSquare"));
    assert!(text.contains("identity-ref"));
    // The stored project still validates clean.
    let validated = verb(&root, "project_validate", json!({}));
    assert_eq!(validated.status, Status::Ok, "validate: {}", validated.doc);
    // Re-setting the identical record is a no-op.
    let again = verb(&root, "conditioning_set", set_inputs(GRADIENT_SHA));
    assert_eq!(again.status, Status::Ok);
    assert_eq!(again.doc["result"], "no-op");
}

#[test]
fn stored_section_round_trips_byte_identically() {
    let root = project_with_assets("round-trip", GRADIENT_PNG);
    verb(&root, "conditioning_set", set_inputs(GRADIENT_SHA));
    let loaded = torogao_author::store::load(&root)
        .expect("load")
        .expect("project exists");
    assert!(loaded.project.generation.is_some());
    assert_eq!(
        torogao_author::codec::emit_project(&loaded.project),
        loaded.text
    );
}

#[test]
fn compile_end_to_end_is_deterministic() {
    let root = project_with_assets("compile", GRADIENT_PNG);
    verb(&root, "conditioning_set", set_inputs(GRADIENT_SHA));
    let first = verb(&root, "conditioning_compile", compile_inputs(GRADIENT_SHA));
    assert_eq!(first.status, Status::Ok, "compile: {}", first.doc);
    assert_eq!(
        first.doc["report"]["graph_sha256"],
        first.doc["graph_sha256"]
    );
    let classes: Vec<&str> = first.doc["graph"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["class_type"].as_str().unwrap())
        .collect();
    assert!(classes.contains(&"KSampler"));
    assert!(classes.contains(&"IPAdapterAdvanced"));
    assert!(classes.contains(&"ControlNetApplyAdvanced"));
    assert_eq!(
        first.doc["report"]["normalisations"][0]["magnitude_mean"],
        11.25
    );
    let second = verb(&root, "conditioning_compile", compile_inputs(GRADIENT_SHA));
    assert_eq!(first.doc, second.doc, "same inputs, byte-identical answer");
}

#[test]
fn compile_refuses_a_uniform_map_with_the_gencond_code() {
    let root = project_with_assets("blank", UNIFORM_PNG);
    verb(&root, "conditioning_set", set_inputs(UNIFORM_SHA));
    let answer = verb(&root, "conditioning_compile", compile_inputs(UNIFORM_SHA));
    assert_eq!(answer.status, Status::Refused, "compile: {}", answer.doc);
    assert_eq!(answer.doc["code"], "TG-GENCOND-BLANK-MAP");
}

#[test]
fn set_refuses_a_reference_without_a_crop() {
    let root = project_with_assets("crop", GRADIENT_PNG);
    let mut inputs = set_inputs(GRADIENT_SHA);
    inputs["conditioning"]["sources"][0]["kind"]["reference_embed"]["reference"]
        .as_object_mut()
        .unwrap()
        .remove("crop");
    let answer = verb(&root, "conditioning_set", inputs);
    assert_eq!(answer.status, Status::Refused, "set: {}", answer.doc);
    assert_eq!(answer.doc["code"], "TG-AUTH-MALFORMED");
    assert!(
        answer.doc["message"].as_str().unwrap().contains("crop"),
        "refusal names the missing field: {}",
        answer.doc
    );
}

#[test]
fn tree_bridge_is_lossless() {
    let typed: torogao_gencond::Conditioning =
        serde_json::from_value(set_inputs(GRADIENT_SHA)["conditioning"].clone()).unwrap();
    let gencond_tree = typed.to_ron_tree();
    let author_tree = torogao_author::conditioning::gencond_tree_to_ron(&gencond_tree);
    let back = torogao_author::conditioning::ron_to_gencond_tree(&author_tree);
    assert_eq!(back, gencond_tree);
    let reparsed = torogao_gencond::Conditioning::from_ron_tree(back).unwrap();
    assert_eq!(reparsed, typed);
}
