//! B1 `asset`-verb fixtures: one planted defect per asset,
//! each producing exactly its contract code and no other —
//! the planted-error positive control. Plus adversarial
//! inputs: malformed rules, missing tolerances, unknown
//! keys, and non-GLB assets must fail closed with typed
//! errors, never panic.

mod common;

use common::{codes, glb_bytes, png_bytes, tempdir, write_bytes, write_text};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use torogao_qc::asset::check_assets;
use torogao_qc::report::Report;

/// A one-quad GLB: a "Skin" material with the given base
/// colour, a root node at the given translation, and an
/// optional embedded PNG texture.
fn quad_glb(factor: [f64; 4], root_translation: [f64; 3], png: Option<&[u8]>) -> Vec<u8> {
    let mut material = json!({
        "name": "Skin",
        "pbrMetallicRoughness": {"baseColorFactor": factor},
    });
    let mut doc = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "Root", "mesh": 0, "translation": root_translation}],
        "meshes": [{
            "name": "Quad",
            "primitives": [{
                "attributes": {"POSITION": 0, "TEXCOORD_0": 1},
                "indices": 2,
                "material": 0,
            }],
        }],
        "materials": [material.clone()],
        "accessors": [
            {"componentType": 5126, "count": 4, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"componentType": 5126, "count": 4, "type": "VEC2"},
            {"componentType": 5123, "count": 6, "type": "SCALAR"},
        ],
    });
    let bin: &[u8] = match png {
        Some(bytes) => {
            material["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": 0});
            doc["materials"] = json!([material]);
            doc["images"] = json!([{"mimeType": "image/png", "bufferView": 0}]);
            doc["textures"] = json!([{"source": 0}]);
            doc["bufferViews"] = json!([{"buffer": 0, "byteOffset": 0, "byteLength": bytes.len()}]);
            doc["buffers"] = json!([{"byteLength": bytes.len()}]);
            bytes
        }
        None => &[],
    };
    glb_bytes(&doc, bin)
}

fn run(rules: &str, assets: &[PathBuf], dir_tag: &str) -> (Report, PathBuf) {
    let dir = tempdir(dir_tag);
    let rules_path = write_text(&dir, "rules.json", rules);
    let report = check_assets(&rules_path, assets).expect("asset check should succeed");
    (report, rules_path)
}

fn write_asset(dir_tag: &str, name: &str, bytes: &[u8]) -> PathBuf {
    let dir = tempdir(dir_tag);
    write_bytes(&dir, name, bytes)
}

#[test]
fn clean_asset_passes_with_zero_findings() {
    let png = png_bytes(8, 8, [255, 0, 0]);
    let asset = write_asset(
        "b1-clean-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0], Some(&png)),
    );
    let rules = r#"{
        "palette": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        "palette_tolerance": 0.05,
        "budgets": {"file_bytes_max": 1000000, "texture_max_dimension": 512, "triangles_max": 100},
        "anchor": [0.0, 0.0, 0.0],
        "anchor_tolerance": 0.001,
        "size_envelope": {"min": [-0.5, -0.5, -0.5], "max": [1.5, 1.5, 0.5]}
    }"#;
    let dir = tempdir("b1-clean-rules");
    let rules_path = write_text(&dir, "rules.json", rules);
    let report = check_assets(&rules_path, &[asset]).expect("check runs");
    assert_eq!(codes(&report), Vec::<&str>::new());
    assert!(!report.failed());
    let json = report.to_json();
    assert_eq!(json["summary"]["pass"], json!(true));
    let expected_hash = format!("{:x}", Sha256::digest(rules.as_bytes()));
    assert_eq!(json["rules_sha256"], json!(expected_hash));
    assert_eq!(json["verb"], json!("asset"));
}

#[test]
fn off_palette_material_is_exactly_palette() {
    let asset = write_asset(
        "b1-palette-asset",
        "hero.glb",
        &quad_glb([0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 0.0], None),
    );
    let (report, _) = run(
        r#"{"palette": [[1.0, 0.0, 0.0]], "palette_tolerance": 0.05}"#,
        &[asset],
        "b1-palette-rules",
    );
    assert_eq!(codes(&report), ["QC_ASSET_PALETTE"]);
    assert!(report.failed());
    let finding = &report.to_json()["findings"][0];
    assert_eq!(finding["scene"], json!("hero.glb"));
    assert_eq!(finding["element"], json!("Skin"));
    assert_eq!(finding["spec_ref"], json!("/palette"));
}

#[test]
fn over_budget_texture_is_exactly_budget() {
    let png = png_bytes(64, 64, [255, 0, 0]);
    let asset = write_asset(
        "b1-texbudget-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0], Some(&png)),
    );
    let (report, _) = run(
        r#"{"budgets": {"texture_max_dimension": 32}}"#,
        &[asset],
        "b1-texbudget-rules",
    );
    assert_eq!(codes(&report), ["QC_ASSET_BUDGET"]);
    let finding = &report.to_json()["findings"][0];
    assert_eq!(finding["spec_ref"], json!("/budgets/texture_max_dimension"));
}

#[test]
fn over_budget_triangles_is_exactly_budget() {
    let asset = write_asset(
        "b1-tribudget-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None),
    );
    let (report, _) = run(
        r#"{"budgets": {"triangles_max": 1}}"#,
        &[asset],
        "b1-tribudget-rules",
    );
    assert_eq!(codes(&report), ["QC_ASSET_BUDGET"]);
}

#[test]
fn over_file_bytes_budget_is_exactly_budget() {
    let asset = write_asset(
        "b1-filebudget-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None),
    );
    let (report, _) = run(
        r#"{"budgets": {"file_bytes_max": 10}}"#,
        &[asset],
        "b1-filebudget-rules",
    );
    assert_eq!(codes(&report), ["QC_ASSET_BUDGET"]);
}

#[test]
fn off_origin_root_is_exactly_origin() {
    let asset = write_asset(
        "b1-anchor-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [2.0, 0.0, 0.0], None),
    );
    let (report, _) = run(
        r#"{"anchor": [0.0, 0.0, 0.0], "anchor_tolerance": 0.001}"#,
        &[asset],
        "b1-anchor-rules",
    );
    assert_eq!(codes(&report), ["QC_ASSET_ORIGIN"]);
    let finding = &report.to_json()["findings"][0];
    assert_eq!(finding["spec_ref"], json!("/anchor"));
    assert_eq!(finding["element"], json!("Root"));
}

#[test]
fn envelope_violation_is_exactly_origin() {
    let asset = write_asset(
        "b1-envelope-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None),
    );
    let (report, _) = run(
        r#"{"size_envelope": {"min": [-1.0, -1.0, -1.0], "max": [0.5, 2.0, 2.0]}}"#,
        &[asset],
        "b1-envelope-rules",
    );
    assert_eq!(codes(&report), ["QC_ASSET_ORIGIN"]);
    let finding = &report.to_json()["findings"][0];
    assert_eq!(finding["spec_ref"], json!("/size_envelope"));
}

#[test]
fn palette_from_textures_flags_dominant_colour() {
    let png = png_bytes(16, 16, [0, 0, 255]);
    let asset = write_asset(
        "b1-texpalette-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0], Some(&png)),
    );
    let (report, _) = run(
        r#"{"palette": [[1.0, 0.0, 0.0]], "palette_tolerance": 0.05, "palette_from_textures": true}"#,
        &[asset],
        "b1-texpalette-rules",
    );
    assert_eq!(codes(&report), ["QC_ASSET_PALETTE"]);
    let json = report.to_json();
    assert_eq!(json["findings"][0]["element"], json!("texture 0"));
    assert_eq!(json["textures_sampled"], json!(1));
    assert_eq!(json["textures_undecodable"], json!(0));
}

#[test]
fn report_is_byte_identical_across_runs() {
    let asset = write_asset(
        "b1-determinism-asset",
        "hero.glb",
        &quad_glb([0.0, 0.0, 1.0, 1.0], [2.0, 0.0, 0.0], None),
    );
    let dir = tempdir("b1-determinism-rules");
    let rules_path = write_text(
        &dir,
        "rules.json",
        r#"{"palette": [[1.0, 0.0, 0.0]], "palette_tolerance": 0.05,
            "anchor": [0.0, 0.0, 0.0], "anchor_tolerance": 0.001}"#,
    );
    let assets = vec![asset];
    let first = check_assets(&rules_path, &assets).expect("first run");
    let second = check_assets(&rules_path, &assets).expect("second run");
    assert_eq!(first.to_json_string(), second.to_json_string());
    assert_eq!(codes(&first), ["QC_ASSET_ORIGIN", "QC_ASSET_PALETTE"]);
}

#[test]
fn malformed_rules_are_a_typed_error() {
    let asset = write_asset(
        "b1-badrules-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0; 3], None),
    );
    let dir = tempdir("b1-badrules-rules");
    let rules_path = write_text(&dir, "rules.json", "{not json");
    let err = check_assets(&rules_path, &[asset]).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-RULES-PARSE");
}

#[test]
fn palette_without_tolerance_is_a_shape_error() {
    let asset = write_asset(
        "b1-notol-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0; 3], None),
    );
    let dir = tempdir("b1-notol-rules");
    let rules_path = write_text(&dir, "rules.json", r#"{"palette": [[1.0, 0.0, 0.0]]}"#);
    let err = check_assets(&rules_path, &[asset]).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-RULES-SHAPE");
}

#[test]
fn unknown_rules_key_is_a_shape_error() {
    let asset = write_asset(
        "b1-unknown-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0; 3], None),
    );
    let dir = tempdir("b1-unknown-rules");
    let rules_path = write_text(
        &dir,
        "rules.json",
        r#"{"palette": [[1.0, 0.0, 0.0]], "palette_tolerance": 0.05, "pallete": []}"#,
    );
    let err = check_assets(&rules_path, &[asset]).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-RULES-SHAPE");
}

#[test]
fn out_of_range_palette_entry_is_a_shape_error() {
    let asset = write_asset(
        "b1-range-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0; 3], None),
    );
    let dir = tempdir("b1-range-rules");
    let rules_path = write_text(
        &dir,
        "rules.json",
        r#"{"palette": [[2.0, 0.0, 0.0]], "palette_tolerance": 0.05}"#,
    );
    let err = check_assets(&rules_path, &[asset]).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-RULES-SHAPE");
}

#[test]
fn non_glb_asset_is_a_typed_error() {
    let dir = tempdir("b1-notglb");
    let asset = write_bytes(&dir, "hero.glb", b"this is not a glb at all");
    let rules_path = write_text(&dir, "rules.json", r#"{"budgets": {"triangles_max": 10}}"#);
    let err = check_assets(&rules_path, &[asset]).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-ASSET-PARSE");
}

/// The rules document the module docs promise, parsed as a
/// value (keeps the doc example honest if the shape moves).
#[test]
fn documented_rules_shape_is_accepted() {
    let doc: Value = json!({
        "palette": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        "palette_tolerance": 0.05,
        "palette_from_textures": false,
        "budgets": {"file_bytes_max": 1000000, "texture_max_dimension": 512, "triangles_max": 1000},
        "anchor": [0.0, 0.0, 0.0],
        "anchor_tolerance": 0.001,
        "size_envelope": {"min": [-1, -1, -1], "max": [1, 1, 1]},
    });
    let asset = write_asset(
        "b1-docshape-asset",
        "hero.glb",
        &quad_glb([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None),
    );
    let dir = tempdir("b1-docshape-rules");
    let rules_path = write_text(&dir, "rules.json", &doc.to_string());
    let report = check_assets(&rules_path, &[asset]).expect("documented shape parses");
    // The quad's AABB reaches (1, 1, 0), inside the envelope;
    // its 2 triangles are under budget; the file is small.
    assert_eq!(codes(&report), Vec::<&str>::new());
}
