//! B3: the `roundtrip` verb — the reimport-verification
//! tier of the export checks (adapted 2026-11-26 from the
//! ellmos reimport-verification lifecycle, wave-2 Blender
//! review Record 3), built as B2's second tier, not a rival:
//! B2 reads the exported file; B3 asks what a consumer's
//! importer makes of it.
//!
//! Lifecycle: the source GLB's structural summary (node
//! inventory and hierarchy, local transforms, per-mesh
//! vertex/triangle counts, material and morph-target
//! inventories — see [`crate::glb::GlbModel::summarize`]) is
//! diffed against the same summary produced from the exported
//! GLB reimported into an empty scene by a pinned headless
//! Blender. The verb also runs B2's in-file audit, so one
//! report carries both tiers: findings are `QC_EXPORT_LOSS`
//! with `data.stage` = `in_file` (B2) or `on_reimport` (B3).
//!
//! Discipline (per the record): the Blender build is pinned
//! by the caller (`--blender` names the binary; the estate
//! rule is the official build — the Arch build segfaults on
//! IK-carrying blends), its version is recorded in the
//! report's `extra`, and the one numeric tolerance
//! (`--transform-tolerance`) is a required, recorded
//! parameter — there is no default. Counts are compared
//! exactly. The diff vocabulary extends B2's closed set with
//! structural terms (`node_dropped`, `node_added`,
//! `parent_changed`, `transform_drift`, `mesh_dropped`,
//! `mesh_added`, `vertex_count_changed`,
//! `triangle_count_changed`, `material_dropped`) — new
//! detail terms, never new codes.

use crate::error::QcError;
use crate::export_check::{KIND_SHAPE_KEY_DROPPED, STAGE_ON_REIMPORT, audit_export, load_manifest};
use crate::glb::GlbModel;
use crate::limits::{BLENDER_STDERR_MAX, SUMMARY_BYTES_MAX};
use crate::read_bounded;
use crate::report::{Finding, Report, Severity, round6};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Structural-diff loss terms (B3's extension of the
/// `QC_EXPORT_LOSS` detail vocabulary).
pub const KIND_NODE_DROPPED: &str = "node_dropped";
/// Structural-diff loss term: a node the importer added.
pub const KIND_NODE_ADDED: &str = "node_added";
/// Structural-diff loss term: a node's parent changed.
pub const KIND_PARENT_CHANGED: &str = "parent_changed";
/// Structural-diff loss term: a transform moved past the
/// named tolerance.
pub const KIND_TRANSFORM_DRIFT: &str = "transform_drift";
/// Structural-diff loss term: a node's mesh vanished.
pub const KIND_MESH_DROPPED: &str = "mesh_dropped";
/// Structural-diff loss term: a node gained a mesh.
pub const KIND_MESH_ADDED: &str = "mesh_added";
/// Structural-diff loss term: a mesh's vertex count changed.
pub const KIND_VERTEX_COUNT_CHANGED: &str = "vertex_count_changed";
/// Structural-diff loss term: a mesh's triangle count changed.
pub const KIND_TRIANGLE_COUNT_CHANGED: &str = "triangle_count_changed";
/// Structural-diff loss term: a material vanished from a mesh.
pub const KIND_MATERIAL_DROPPED: &str = "material_dropped";

/// The Blender runner script: imports one GLB into an empty
/// scene and writes the structural summary as JSON. Object
/// transforms are converted back to glTF's Y-up frame before
/// summarising (the importer maps glTF `(x, y, z)` to Blender
/// `(x, -z, y)`; node matrices satisfy `Mb = C * Mg * C^-1`,
/// so `Mg = C^-1 * Mb * C`).
const BLENDER_SCRIPT: &str = r#"
import bpy
import json
import mathutils
import sys


def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    glb_path, out_path = argv[0], argv[1]
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=glb_path)
    conv = mathutils.Matrix(
        ((1, 0, 0, 0), (0, 0, -1, 0), (0, 1, 0, 0), (0, 0, 0, 1))
    )
    conv_inv = conv.inverted()
    nodes = []
    for obj in bpy.data.objects:
        gltf_matrix = conv_inv @ obj.matrix_basis @ conv
        loc, quat, scale = gltf_matrix.decompose()
        mesh = None
        if obj.type == "MESH":
            data = obj.data
            data.calc_loop_triangles()
            mats = sorted({m.name for m in data.materials if m is not None})
            targets = []
            if data.shape_keys is not None:
                targets = [kb.name for kb in data.shape_keys.key_blocks[1:]]
            mesh = {
                "name": data.name,
                "vertices": len(data.vertices),
                "triangles": len(data.loop_triangles),
                "materials": mats,
                "targets": targets,
                "target_count": len(targets),
            }
        nodes.append(
            {
                "name": obj.name,
                "parent": obj.parent.name if obj.parent is not None else None,
                "translation": [loc.x, loc.y, loc.z],
                "rotation": [quat.x, quat.y, quat.z, quat.w],
                "scale": [scale.x, scale.y, scale.z],
                "mesh": mesh,
            }
        )
    with open(out_path, "w") as handle:
        json.dump({"nodes": nodes}, handle)


main()
"#;

/// Options for a round-trip run. Both fields are required by
/// the CLI: there is no default Blender and no default
/// tolerance.
#[derive(Debug, Clone, PartialEq)]
pub struct RoundtripOptions {
    /// The pinned Blender binary (estate rule: the official
    /// build).
    pub blender: PathBuf,
    /// Transform tolerance: translations compare by Euclidean
    /// distance, scales by max component delta, rotations by
    /// quaternion angle in radians.
    pub transform_tolerance: f64,
}

/// Runs the round-trip: B2's audit of the exported file plus
/// the reimport structural diff, in one report.
pub fn roundtrip(
    source_path: &Path,
    export_path: &Path,
    manifest_path: &Path,
    options: &RoundtripOptions,
) -> Result<Report, QcError> {
    let manifest = load_manifest(manifest_path)?;
    let source = GlbModel::load(source_path)?;
    let export = GlbModel::load(export_path)?;
    let blender_version = blender_version(&options.blender)?;
    let reimport_summary = reimport_summary(&options.blender, export_path)?;
    let scene = manifest_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "roundtrip".to_string());
    let mut report = Report::new("roundtrip", scene);
    report
        .extra
        .push(("manifest_sha256", json!(manifest.sha256)));
    report
        .extra
        .push(("blender_version", json!(blender_version)));
    report.extra.push((
        "transform_tolerance",
        json!(round6(options.transform_tolerance)),
    ));
    report
        .extra
        .push(("source_file", json!(file_label(source_path))));
    report
        .extra
        .push(("export_file", json!(file_label(export_path))));
    for finding in audit_export(&export, &manifest) {
        report.push(finding);
    }
    for finding in diff_summaries(
        &source.summarize(),
        &reimport_summary,
        options.transform_tolerance,
    ) {
        report.push(finding);
    }
    report.sort_findings();
    Ok(report)
}

/// A report-safe file label: the file name only.
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The Blender binary's version, from its own `--version`
/// banner ("Blender 5.3.0" -> "5.3.0").
fn blender_version(blender: &Path) -> Result<String, QcError> {
    let output = Command::new(blender)
        .arg("--version")
        .output()
        .map_err(|err| {
            QcError::new(
                "QC-E-BLENDER-RUN",
                format!("cannot run {}: {err}", blender.display()),
            )
        })?;
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next().unwrap_or("");
    let version = first.strip_prefix("Blender ").unwrap_or(first).trim();
    if version.is_empty() {
        return Err(QcError::new(
            "QC-E-BLENDER-RUN",
            format!(
                "{} --version produced no recognisable banner",
                blender.display()
            ),
        ));
    }
    Ok(version.to_string())
}

/// Reimports the exported GLB with the pinned Blender and
/// returns its structural summary.
fn reimport_summary(blender: &Path, export_path: &Path) -> Result<Value, QcError> {
    let dir = std::env::temp_dir().join(format!("torogao-qc-roundtrip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|err| {
        QcError::new(
            "QC-E-BLENDER-RUN",
            format!("cannot create runner temp dir {}: {err}", dir.display()),
        )
    })?;
    let script_path = dir.join("summarize.py");
    let out_path = dir.join("summary.json");
    let result = run_blender(blender, &script_path, &out_path, export_path);
    let summary = result.and_then(|()| {
        let bytes = read_bounded(&out_path, SUMMARY_BYTES_MAX, "SUMMARY")?;
        serde_json::from_slice(&bytes).map_err(|err| {
            QcError::new(
                "QC-E-BLENDER-OUTPUT",
                format!("reimport summary is not valid JSON: {err}"),
            )
        })
    });
    let _ = std::fs::remove_dir_all(&dir);
    summary
}

fn run_blender(
    blender: &Path,
    script_path: &Path,
    out_path: &Path,
    export_path: &Path,
) -> Result<(), QcError> {
    std::fs::write(script_path, BLENDER_SCRIPT).map_err(|err| {
        QcError::new(
            "QC-E-BLENDER-RUN",
            format!("cannot write runner script: {err}"),
        )
    })?;
    let output = Command::new(blender)
        .args(["--background", "--factory-startup", "-noaudio", "--python"])
        .arg(script_path)
        .arg("--")
        .arg(export_path)
        .arg(out_path)
        .output()
        .map_err(|err| {
            QcError::new(
                "QC-E-BLENDER-RUN",
                format!("cannot run {}: {err}", blender.display()),
            )
        })?;
    if !output.status.success() || !out_path.exists() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr
            .chars()
            .rev()
            .take(BLENDER_STDERR_MAX)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(QcError::new(
            "QC-E-BLENDER-RUN",
            format!(
                "Blender reimport failed (status {}); stderr tail: {}",
                output.status, tail
            ),
        ));
    }
    Ok(())
}

/// One node of a structural summary.
#[derive(Debug, Clone, PartialEq)]
struct SummaryNode {
    name: String,
    parent: Option<String>,
    translation: [f64; 3],
    rotation: [f64; 4],
    scale: [f64; 3],
    mesh: Option<SummaryMesh>,
}

/// One mesh of a structural summary.
#[derive(Debug, Clone, PartialEq)]
struct SummaryMesh {
    vertices: u64,
    triangles: u64,
    materials: BTreeSet<String>,
    targets: Vec<String>,
    target_count: u64,
}

fn parse_summary(summary: &Value) -> Vec<SummaryNode> {
    let nodes = summary
        .get("nodes")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    nodes
        .iter()
        .map(|node| SummaryNode {
            name: node
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            parent: node
                .get("parent")
                .and_then(Value::as_str)
                .map(str::to_string),
            translation: summary_vec3(node, "translation", [0.0, 0.0, 0.0]),
            rotation: summary_vec4(node, "rotation"),
            scale: summary_vec3(node, "scale", [1.0, 1.0, 1.0]),
            mesh: node.get("mesh").and_then(|mesh| {
                if mesh.is_null() {
                    return None;
                }
                Some(SummaryMesh {
                    vertices: mesh.get("vertices").and_then(Value::as_u64).unwrap_or(0),
                    triangles: mesh.get("triangles").and_then(Value::as_u64).unwrap_or(0),
                    materials: mesh
                        .get("materials")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                    targets: mesh
                        .get("targets")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                    target_count: mesh
                        .get("target_count")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                })
            }),
        })
        .collect()
}

fn summary_vec3(node: &Value, key: &str, default: [f64; 3]) -> [f64; 3] {
    node.get(key)
        .and_then(crate::glb::vec3_of)
        .unwrap_or(default)
}

fn summary_vec4(node: &Value, key: &str) -> [f64; 4] {
    crate::glb::vec4_of(node.get(key).unwrap_or(&Value::Null)).unwrap_or([0.0, 0.0, 0.0, 1.0])
}

/// Diffs two structural summaries. Nodes match by name
/// (duplicates pair in document order). Every difference is
/// a `QC_EXPORT_LOSS` finding at stage `on_reimport`.
pub fn diff_summaries(source: &Value, reimport: &Value, tolerance: f64) -> Vec<Finding> {
    let source_nodes = parse_summary(source);
    let reimport_nodes = parse_summary(reimport);
    let mut findings = Vec::new();
    let mut used: Vec<bool> = vec![false; reimport_nodes.len()];
    for source_node in &source_nodes {
        let partner = reimport_nodes
            .iter()
            .enumerate()
            .find(|(index, candidate)| !used[*index] && candidate.name == source_node.name)
            .map(|(index, candidate)| {
                used[index] = true;
                candidate
            });
        let Some(partner) = partner else {
            findings.push(reimport_loss(
                KIND_NODE_DROPPED,
                &source_node.name,
                json!({}),
                format!(
                    "{KIND_NODE_DROPPED}: node {:?} is absent from the reimported scene",
                    source_node.name
                ),
            ));
            continue;
        };
        if partner.parent != source_node.parent {
            findings.push(reimport_loss(
                KIND_PARENT_CHANGED,
                &source_node.name,
                json!({
                    "source_parent": source_node.parent,
                    "reimport_parent": partner.parent,
                }),
                format!(
                    "{KIND_PARENT_CHANGED}: node {:?} has parent {:?} in the source but {:?} after reimport",
                    source_node.name, source_node.parent, partner.parent
                ),
            ));
        }
        let drift = transform_drift(source_node, partner, tolerance);
        if !drift.is_empty() {
            findings.push(reimport_loss(
                KIND_TRANSFORM_DRIFT,
                &source_node.name,
                json!({"tolerance": round6(tolerance)}),
                format!(
                    "{KIND_TRANSFORM_DRIFT}: node {:?} transform moved past the tolerance {}: {}",
                    source_node.name,
                    round6(tolerance),
                    drift.join("; ")
                ),
            ));
        }
        diff_meshes(source_node, partner, &mut findings);
    }
    for (index, node) in reimport_nodes.iter().enumerate() {
        if !used[index] {
            findings.push(reimport_loss(
                KIND_NODE_ADDED,
                &node.name,
                json!({}),
                format!(
                    "{KIND_NODE_ADDED}: node {:?} appears after reimport but is not in the source",
                    node.name
                ),
            ));
        }
    }
    findings
}

/// The over-tolerance transform components of a matched node
/// pair, as human phrases (empty when within tolerance).
fn transform_drift(source: &SummaryNode, partner: &SummaryNode, tolerance: f64) -> Vec<String> {
    let mut drift = Vec::new();
    let delta = [
        source.translation[0] - partner.translation[0],
        source.translation[1] - partner.translation[1],
        source.translation[2] - partner.translation[2],
    ];
    let translation_delta =
        (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    if translation_delta > tolerance {
        drift.push(format!("translation moved {}", round6(translation_delta)));
    }
    let scale_delta = (0..3)
        .map(|axis| (source.scale[axis] - partner.scale[axis]).abs())
        .fold(0.0, f64::max);
    if scale_delta > tolerance {
        drift.push(format!("scale moved {}", round6(scale_delta)));
    }
    let dot = (0..4)
        .map(|axis| source.rotation[axis] * partner.rotation[axis])
        .sum::<f64>()
        .abs()
        .min(1.0);
    let angle = 2.0 * dot.acos();
    if angle > tolerance {
        drift.push(format!("rotation moved {} rad", round6(angle)));
    }
    drift
}

/// Mesh-level differences of a matched node pair.
fn diff_meshes(source: &SummaryNode, partner: &SummaryNode, findings: &mut Vec<Finding>) {
    let name = &source.name;
    match (&source.mesh, &partner.mesh) {
        (None, None) => {}
        (Some(_), None) => {
            findings.push(reimport_loss(
                KIND_MESH_DROPPED,
                name,
                json!({}),
                format!(
                    "{KIND_MESH_DROPPED}: node {name:?} carries a mesh in the source but none after reimport"
                ),
            ));
        }
        (None, Some(_)) => {
            findings.push(reimport_loss(
                KIND_MESH_ADDED,
                name,
                json!({}),
                format!(
                    "{KIND_MESH_ADDED}: node {name:?} carries no mesh in the source but gained one after reimport"
                ),
            ));
        }
        (Some(source_mesh), Some(partner_mesh)) => {
            if source_mesh.vertices != partner_mesh.vertices {
                findings.push(reimport_loss(
                    KIND_VERTEX_COUNT_CHANGED,
                    name,
                    json!({
                        "source": source_mesh.vertices,
                        "reimport": partner_mesh.vertices,
                    }),
                    format!(
                        "{KIND_VERTEX_COUNT_CHANGED}: mesh on node {name:?} has {} vertices in the source but {} after reimport",
                        source_mesh.vertices, partner_mesh.vertices
                    ),
                ));
            }
            if source_mesh.triangles != partner_mesh.triangles {
                findings.push(reimport_loss(
                    KIND_TRIANGLE_COUNT_CHANGED,
                    name,
                    json!({
                        "source": source_mesh.triangles,
                        "reimport": partner_mesh.triangles,
                    }),
                    format!(
                        "{KIND_TRIANGLE_COUNT_CHANGED}: mesh on node {name:?} has {} triangles in the source but {} after reimport",
                        source_mesh.triangles, partner_mesh.triangles
                    ),
                ));
            }
            for material in source_mesh.materials.difference(&partner_mesh.materials) {
                findings.push(reimport_loss(
                    KIND_MATERIAL_DROPPED,
                    name,
                    json!({"material": material}),
                    format!(
                        "{KIND_MATERIAL_DROPPED}: mesh on node {name:?} loses material {material:?} after reimport"
                    ),
                ));
            }
            if !source_mesh.targets.is_empty() {
                for target in &source_mesh.targets {
                    if !partner_mesh.targets.contains(target) {
                        findings.push(reimport_loss(
                            KIND_SHAPE_KEY_DROPPED,
                            name,
                            json!({"target": target}),
                            format!(
                                "{KIND_SHAPE_KEY_DROPPED}: mesh on node {name:?} loses morph target {target:?} after reimport"
                            ),
                        ));
                    }
                }
            } else if source_mesh.target_count != partner_mesh.target_count {
                findings.push(reimport_loss(
                    KIND_SHAPE_KEY_DROPPED,
                    name,
                    json!({
                        "expected_count": source_mesh.target_count,
                        "actual_count": partner_mesh.target_count,
                    }),
                    format!(
                        "{KIND_SHAPE_KEY_DROPPED}: mesh on node {name:?} has {} morph targets in the source but {} after reimport (targets are unnamed on at least one side)",
                        source_mesh.target_count, partner_mesh.target_count
                    ),
                ));
            }
        }
    }
}

/// Builds one `QC_EXPORT_LOSS` finding at stage
/// `on_reimport`.
fn reimport_loss(
    kind: &'static str,
    node_name: &str,
    extra_data: Value,
    detail: String,
) -> Finding {
    let mut data = json!({"kind": kind, "stage": STAGE_ON_REIMPORT, "node": node_name});
    if let (Some(target), Some(extra)) = (data.as_object_mut(), extra_data.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    Finding {
        code: "QC_EXPORT_LOSS",
        severity: Severity::Fail,
        element: Some(node_name.to_string()),
        frame: None,
        detail,
        spec_ref: "/roundtrip".to_string(),
        data: Some(data),
        scene_override: None,
    }
}
