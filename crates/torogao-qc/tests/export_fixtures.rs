//! B2 `export-check` fixtures: one planted loss per export,
//! each producing exactly its `QC_EXPORT_LOSS` kind and no
//! other — the planted-error positive control. Plus
//! adversarial manifests (malformed, duplicate, unknown
//! keys) failing closed with typed errors.

mod common;

use common::{codes, glb_bytes, tempdir, write_bytes, write_text};
use serde_json::{Value, json};
use torogao_qc::export_check::export_check;
use torogao_qc::report::Report;

/// Builds the two-mesh export the fixtures vary. `face_targets`
/// are the Face mesh's declared target names (its `targets`
/// arrays match in length); `face_tangent` / `body_uv` toggle
/// the planted attribute losses; `with_face` drops the whole
/// Face mesh.
fn export_glb(
    face_targets: &[&str],
    face_tangent: bool,
    body_uv: bool,
    with_face: bool,
) -> Vec<u8> {
    let mut face_attributes = json!({"POSITION": 0, "TEXCOORD_0": 1});
    if face_tangent {
        face_attributes["TANGENT"] = json!(3);
    }
    let face_targets_json: Vec<Value> = face_targets
        .iter()
        .map(|_| json!([{"POSITION": 0}]))
        .collect();
    let mut face = json!({
        "name": "Face",
        "primitives": [{
            "attributes": face_attributes,
            "indices": 2,
            "material": 0,
            "targets": face_targets_json,
        }],
    });
    if !face_targets.is_empty() {
        face["extras"] = json!({"targetNames": face_targets});
    }
    let mut body_attributes = json!({"POSITION": 0});
    if body_uv {
        body_attributes["TEXCOORD_0"] = json!(1);
    }
    let body = json!({
        "name": "Body",
        "primitives": [{
            "attributes": body_attributes,
            "indices": 2,
            "material": 1,
            "targets": [{"POSITION": 0}],
        }],
        "extras": {"targetNames": ["Wave"]},
    });
    let meshes: Vec<Value> = if with_face {
        vec![face, body]
    } else {
        vec![body]
    };
    let doc = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "Root", "mesh": 0}],
        "meshes": meshes,
        "materials": [
            {"name": "FaceSkin",
             "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}},
             "normalTexture": {"index": 1}},
            {"name": "Cloth",
             "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}},
        ],
        "textures": [{"source": 0}, {"source": 0}],
        "images": [{"mimeType": "image/png", "bufferView": 0}],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 4}],
        "buffers": [{"byteLength": 4}],
        "accessors": [
            {"componentType": 5126, "count": 4, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"componentType": 5126, "count": 4, "type": "VEC2"},
            {"componentType": 5123, "count": 6, "type": "SCALAR"},
            {"componentType": 5126, "count": 4, "type": "VEC4"},
        ],
    });
    glb_bytes(&doc, &[0, 0, 0, 0])
}

const MANIFEST: &str = r#"{
    "meshes": [
        {"name": "Face", "morph_targets": ["Blink", "Smile"],
         "uv_channels": 1, "require_tangents": true},
        {"name": "Body", "morph_target_count": 1}
    ]
}"#;

fn run(tag: &str, glb: Vec<u8>, manifest: &str) -> Report {
    let dir = tempdir(tag);
    let glb_path = write_bytes(&dir, "export.glb", &glb);
    let manifest_path = write_text(&dir, "expect.json", manifest);
    export_check(&glb_path, &manifest_path).expect("export check should succeed")
}

fn kinds(report: &Report) -> Vec<String> {
    report
        .findings()
        .iter()
        .map(|f| {
            f.data
                .as_ref()
                .and_then(|d| d.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

#[test]
fn clean_export_passes_with_zero_findings() {
    let report = run(
        "b2-clean",
        export_glb(&["Blink", "Smile"], true, true, true),
        MANIFEST,
    );
    assert_eq!(codes(&report), Vec::<&str>::new());
    assert!(!report.failed());
    let json = report.to_json();
    assert_eq!(json["summary"]["pass"], json!(true));
    assert_eq!(json["meshes_in_file"], json!(2));
}

#[test]
fn dropped_morph_target_is_exactly_shape_key_dropped() {
    let report = run(
        "b2-dropped-target",
        export_glb(&["Blink"], true, true, true),
        MANIFEST,
    );
    assert_eq!(codes(&report), ["QC_EXPORT_LOSS"]);
    assert_eq!(kinds(&report), ["shape_key_dropped"]);
    let finding = &report.to_json()["findings"][0];
    assert_eq!(finding["data"]["target"], json!("Smile"));
    assert_eq!(finding["data"]["stage"], json!("in_file"));
    assert_eq!(finding["element"], json!("Face"));
    assert_eq!(finding["spec_ref"], json!("/meshes/0"));
}

#[test]
fn normal_map_without_tangents_is_exactly_tangents_missing() {
    let report = run(
        "b2-no-tangents",
        export_glb(&["Blink", "Smile"], false, true, true),
        MANIFEST,
    );
    assert_eq!(codes(&report), ["QC_EXPORT_LOSS"]);
    assert_eq!(kinds(&report), ["tangents_missing"]);
}

#[test]
fn textured_without_uv_is_exactly_uv_missing() {
    let report = run(
        "b2-no-uv",
        export_glb(&["Blink", "Smile"], true, false, true),
        MANIFEST,
    );
    assert_eq!(codes(&report), ["QC_EXPORT_LOSS"]);
    assert_eq!(kinds(&report), ["uv_missing"]);
    let finding = &report.to_json()["findings"][0];
    assert_eq!(finding["element"], json!("Body"));
}

#[test]
fn absent_manifest_mesh_loses_each_named_target() {
    let report = run(
        "b2-mesh-absent",
        export_glb(&["Blink", "Smile"], true, true, false),
        r#"{"meshes": [{"name": "Face", "morph_targets": ["Blink", "Smile"]}]}"#,
    );
    assert_eq!(codes(&report), ["QC_EXPORT_LOSS", "QC_EXPORT_LOSS"]);
    assert_eq!(kinds(&report), ["shape_key_dropped", "shape_key_dropped"]);
}

#[test]
fn count_only_expectation_catches_a_drop() {
    let report = run(
        "b2-count-drop",
        export_glb(&["Blink", "Smile"], true, true, true),
        r#"{"meshes": [{"name": "Body", "morph_target_count": 2}]}"#,
    );
    assert_eq!(codes(&report), ["QC_EXPORT_LOSS"]);
    assert_eq!(kinds(&report), ["shape_key_dropped"]);
    let finding = &report.to_json()["findings"][0];
    assert_eq!(finding["data"]["expected_count"], json!(2));
    assert_eq!(finding["data"]["actual_count"], json!(1));
}

#[test]
fn unnamed_targets_fall_back_to_counts() {
    // Face carries one target but no extras.targetNames; the
    // manifest names two. The count proves a loss even though
    // no name can be checked.
    let raw = json!({
        "asset": {"version": "2.0"},
        "meshes": [{
            "name": "Face",
            "primitives": [{
                "attributes": {"POSITION": 0},
                "targets": [{"POSITION": 0}],
            }],
        }],
        "accessors": [
            {"componentType": 5126, "count": 4, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
        ],
    });
    let bytes = glb_bytes(&raw, &[]);
    let report = run(
        "b2-unnamed-targets",
        bytes,
        r#"{"meshes": [{"name": "Face", "morph_targets": ["Blink", "Smile"]}]}"#,
    );
    assert_eq!(codes(&report), ["QC_EXPORT_LOSS"]);
    assert_eq!(kinds(&report), ["shape_key_dropped"]);
}

#[test]
fn report_is_byte_identical_across_runs() {
    let dir = tempdir("b2-determinism");
    let glb_path = write_bytes(
        &dir,
        "export.glb",
        &export_glb(&["Blink"], false, true, true),
    );
    let manifest_path = write_text(&dir, "expect.json", MANIFEST);
    let first = export_check(&glb_path, &manifest_path).expect("first run");
    let second = export_check(&glb_path, &manifest_path).expect("second run");
    assert_eq!(first.to_json_string(), second.to_json_string());
    assert_eq!(codes(&first), ["QC_EXPORT_LOSS", "QC_EXPORT_LOSS"]);
}

#[test]
fn malformed_manifest_is_a_typed_error() {
    let dir = tempdir("b2-badmanifest");
    let glb_path = write_bytes(
        &dir,
        "export.glb",
        &export_glb(&["Blink", "Smile"], true, true, true),
    );
    let manifest_path = write_text(&dir, "expect.json", "{nope");
    let err = export_check(&glb_path, &manifest_path).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-MANIFEST-PARSE");
}

#[test]
fn duplicate_manifest_mesh_is_a_shape_error() {
    let dir = tempdir("b2-dupmanifest");
    let glb_path = write_bytes(
        &dir,
        "export.glb",
        &export_glb(&["Blink", "Smile"], true, true, true),
    );
    let manifest_path = write_text(
        &dir,
        "expect.json",
        r#"{"meshes": [{"name": "Face"}, {"name": "Face"}]}"#,
    );
    let err = export_check(&glb_path, &manifest_path).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-MANIFEST-SHAPE");
}

#[test]
fn unknown_manifest_key_is_a_shape_error() {
    let dir = tempdir("b2-unknownmanifest");
    let glb_path = write_bytes(
        &dir,
        "export.glb",
        &export_glb(&["Blink", "Smile"], true, true, true),
    );
    let manifest_path = write_text(
        &dir,
        "expect.json",
        r#"{"meshes": [{"name": "Face", "morph_targetz": []}]}"#,
    );
    let err = export_check(&glb_path, &manifest_path).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-MANIFEST-SHAPE");
}
