//! B3 `roundtrip` fixtures: the structural summary diff
//! exercised as a pure function (planted differences, each
//! producing exactly its kind), plus one integration test
//! against the estate's pinned official Blender build —
//! skipped loudly when that build is absent, and the only
//! test here that spawns a process.

mod common;

use common::{codes, glb_bytes, tempdir, write_bytes, write_text};
use serde_json::{Value, json};
use std::path::PathBuf;
use torogao_qc::roundtrip::{RoundtripOptions, diff_summaries, roundtrip};

fn summary(nodes: Value) -> Value {
    json!({"nodes": nodes})
}

fn node(name: &str, parent: Option<&str>, mesh: Value) -> Value {
    json!({
        "name": name,
        "parent": parent,
        "translation": [0.0, 0.0, 0.0],
        "rotation": [0.0, 0.0, 0.0, 1.0],
        "scale": [1.0, 1.0, 1.0],
        "mesh": mesh,
    })
}

fn mesh(vertices: u64, triangles: u64, materials: &[&str], targets: &[&str]) -> Value {
    json!({
        "name": "Mesh",
        "vertices": vertices,
        "triangles": triangles,
        "materials": materials,
        "targets": targets,
        "target_count": targets.len(),
    })
}

fn kinds(findings: &[torogao_qc::report::Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|f| {
            assert_eq!(f.code, "QC_EXPORT_LOSS");
            assert_eq!(
                f.data.as_ref().and_then(|d| d.get("stage")),
                Some(&json!("on_reimport"))
            );
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
fn identical_summaries_diff_clean() {
    let source = summary(json!([
        node("Root", None, Value::Null),
        node("Quad", Some("Root"), mesh(4, 2, &["Red"], &["Blink"])),
    ]));
    let findings = diff_summaries(&source, &source.clone(), 0.001);
    assert_eq!(findings.len(), 0);
}

#[test]
fn dropped_node_is_exactly_node_dropped() {
    let source = summary(json!([
        node("Root", None, Value::Null),
        node("Arm", Some("Root"), Value::Null),
    ]));
    let reimport = summary(json!([node("Root", None, Value::Null)]));
    let findings = diff_summaries(&source, &reimport, 0.001);
    assert_eq!(kinds(&findings), ["node_dropped"]);
}

#[test]
fn added_node_is_exactly_node_added() {
    let source = summary(json!([node("Root", None, Value::Null)]));
    let reimport = summary(json!([
        node("Root", None, Value::Null),
        node("Extra", Some("Root"), Value::Null),
    ]));
    let findings = diff_summaries(&source, &reimport, 0.001);
    assert_eq!(kinds(&findings), ["node_added"]);
}

#[test]
fn parent_change_is_exactly_parent_changed() {
    let source = summary(json!([
        node("Root", None, Value::Null),
        node("Arm", Some("Root"), Value::Null),
    ]));
    let reimport = summary(json!([
        node("Root", None, Value::Null),
        node("Arm", None, Value::Null),
    ]));
    let findings = diff_summaries(&source, &reimport, 0.001);
    assert_eq!(kinds(&findings), ["parent_changed"]);
}

#[test]
fn transform_within_tolerance_passes_and_beyond_drifts() {
    let source = summary(json!([node("Root", None, Value::Null)]));
    let mut near = node("Root", None, Value::Null);
    near["translation"] = json!([0.0005, 0.0, 0.0]);
    let findings = diff_summaries(&source, &summary(json!([near])), 0.001);
    assert_eq!(findings.len(), 0);

    let mut far = node("Root", None, Value::Null);
    far["translation"] = json!([0.5, 0.0, 0.0]);
    let findings = diff_summaries(&source, &summary(json!([far])), 0.001);
    assert_eq!(kinds(&findings), ["transform_drift"]);
}

#[test]
fn vertex_count_change_is_exactly_flagged() {
    let source = summary(json!([node("Quad", None, mesh(4, 2, &[], &[]))]));
    let reimport = summary(json!([node("Quad", None, mesh(5, 2, &[], &[]))]));
    let findings = diff_summaries(&source, &reimport, 0.001);
    assert_eq!(kinds(&findings), ["vertex_count_changed"]);
}

#[test]
fn shape_key_lost_on_reimport_is_flagged_at_on_reimport() {
    let source = summary(json!([node("Quad", None, mesh(4, 2, &[], &["Blink"]))]));
    let reimport = summary(json!([node("Quad", None, mesh(4, 2, &[], &[]))]));
    let findings = diff_summaries(&source, &reimport, 0.001);
    assert_eq!(kinds(&findings), ["shape_key_dropped"]);
}

#[test]
fn material_lost_on_reimport_is_flagged() {
    let source = summary(json!([node("Quad", None, mesh(4, 2, &["Red"], &[]))]));
    let reimport = summary(json!([node("Quad", None, mesh(4, 2, &[], &[]))]));
    let findings = diff_summaries(&source, &reimport, 0.001);
    assert_eq!(kinds(&findings), ["material_dropped"]);
}

#[test]
fn missing_blender_is_a_typed_error() {
    let dir = tempdir("b3-noblender");
    let glb = write_bytes(&dir, "a.glb", &real_quad_glb());
    let manifest = write_text(
        &dir,
        "expect.json",
        r#"{"meshes": [{"name": "QuadMesh", "morph_targets": ["Blink"]}]}"#,
    );
    let options = RoundtripOptions {
        blender: PathBuf::from("/nonexistent/blender"),
        transform_tolerance: 0.001,
    };
    let err = roundtrip(&glb, &glb, &manifest, &options).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-BLENDER-RUN");
}

/// A quad GLB with a real binary layout (positions, morph
/// target positions, indices in three buffer views), a
/// translated root, and a Y-rotated mesh child — the
/// smallest scene that exercises transforms, counts,
/// materials, and shape keys through a real importer.
fn real_quad_glb() -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let positions: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
    for value in positions {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let target_positions: [f32; 12] = [0.0, 0.0, 0.1, 0.0, 0.0, 0.1, 0.0, 0.0, 0.1, 0.0, 0.0, 0.1];
    for value in target_positions {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    for value in indices {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let half_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
    let doc = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [
            {"name": "Root", "translation": [0.25, -0.5, 1.0], "children": [1]},
            {"name": "Quad", "mesh": 0, "translation": [0.0, 1.0, 0.0],
             "rotation": [0.0, half_sqrt2, 0.0, half_sqrt2]},
        ],
        "meshes": [{
            "name": "QuadMesh",
            "primitives": [{
                "attributes": {"POSITION": 0},
                "indices": 2,
                "material": 0,
                "targets": [{"POSITION": 1}],
            }],
            "extras": {"targetNames": ["Blink"]},
        }],
        "materials": [{
            "name": "Red",
            "pbrMetallicRoughness": {"baseColorFactor": [1.0, 0.0, 0.0, 1.0]},
        }],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3",
             "min": [0.0, 0.0, 0.1], "max": [0.0, 0.0, 0.1]},
            {"bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR"},
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 48},
            {"buffer": 0, "byteOffset": 48, "byteLength": 48},
            {"buffer": 0, "byteOffset": 96, "byteLength": 12},
        ],
        "buffers": [{"byteLength": 108}],
    });
    glb_bytes(&doc, &bin)
}

/// The estate's pinned official Blender build, when present.
fn official_blender() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path =
        PathBuf::from(home).join(".cache/azoth/blender-official/blender-5.3.0-linux-x64/blender");
    path.exists().then_some(path)
}

#[test]
fn real_blender_roundtrip_is_clean() {
    let Some(blender) = official_blender() else {
        eprintln!(
            "SKIP: official Blender build not present at \
             ~/.cache/azoth/blender-official/blender-5.3.0-linux-x64/blender"
        );
        return;
    };
    let dir = tempdir("b3-real-roundtrip");
    let glb = write_bytes(&dir, "scene.glb", &real_quad_glb());
    let manifest = write_text(
        &dir,
        "expect.json",
        r#"{"meshes": [{"name": "QuadMesh", "morph_targets": ["Blink"]}]}"#,
    );
    let options = RoundtripOptions {
        blender,
        transform_tolerance: 0.01,
    };
    let report = roundtrip(&glb, &glb, &manifest, &options).expect("roundtrip runs");
    assert_eq!(
        codes(&report),
        Vec::<&str>::new(),
        "findings: {}",
        report.to_json_string()
    );
    let json = report.to_json();
    assert_eq!(json["blender_version"], json!("5.3.0"));
    assert_eq!(json["verb"], json!("roundtrip"));
}
