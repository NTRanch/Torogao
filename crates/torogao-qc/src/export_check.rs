//! B2: the `export-check` verb — an export read-back audit
//! (adapted 2026-11-26 from the BlenderLens export read-back
//! pattern, wave-2 Blender review Record 2). The exported GLB
//! is re-opened and its feature inventory compared against
//! what the source declared, in a pinned expectation manifest
//! derived from the source side at export time. The check does
//! not trust the exporter's success report: it reads the
//! artefact the consumer will actually receive.
//!
//! Manifest shape:
//!
//! ```json
//! {
//!   "meshes": [
//!     {"name": "Face", "morph_targets": ["Blink", "Smile"],
//!      "uv_channels": 1, "require_tangents": true},
//!     {"name": "Body", "morph_target_count": 3}
//!   ]
//! }
//! ```
//!
//! One code, `QC_EXPORT_LOSS`, with the loss named in the
//! finding's `data.kind` from a closed vocabulary —
//! `shape_key_dropped`, `uv_missing`, `tangents_missing` —
//! and in the first token of its `detail`. Findings carry
//! `data.stage = "in_file"`; the round-trip tier (B3) adds
//! its own under `on_reimport` in the same report shape.
//!
//! Two check families run:
//!
//! - Manifest-driven: each named mesh's declared morph
//!   targets, UV channels, and tangent requirement against
//!   the file. A manifest mesh absent from the file loses
//!   everything it declared.
//! - Intrinsic: any mesh primitive whose material binds a
//!   texture must carry a UV channel for it, and any whose
//!   material binds a normal map must carry tangents —
//!   whether or not the manifest names the mesh.

use crate::error::QcError;
use crate::glb::{GlbModel, MeshInfo};
use crate::limits::MANIFEST_BYTES_MAX;
use crate::read_bounded;
use crate::report::{Finding, Report, Severity};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

/// The closed loss vocabulary of `QC_EXPORT_LOSS`.
pub const KIND_SHAPE_KEY_DROPPED: &str = "shape_key_dropped";
/// The closed loss vocabulary of `QC_EXPORT_LOSS`.
pub const KIND_UV_MISSING: &str = "uv_missing";
/// The closed loss vocabulary of `QC_EXPORT_LOSS`.
pub const KIND_TANGENTS_MISSING: &str = "tangents_missing";

/// The stage tag B2's findings carry.
pub const STAGE_IN_FILE: &str = "in_file";
/// The stage tag B3's findings carry.
pub const STAGE_ON_REIMPORT: &str = "on_reimport";

/// One mesh's expectations, from the manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Expectation {
    /// The mesh name to match in the exported file.
    pub name: String,
    /// Expected morph-target names (empty when undeclared).
    pub morph_targets: Vec<String>,
    /// Expected morph-target count, when declared by count.
    pub morph_target_count: Option<usize>,
    /// Expected UV channel count, when declared.
    pub uv_channels: Option<usize>,
    /// Whether tangents are required even where no normal
    /// map forces them.
    pub require_tangents: bool,
}

/// A validated expectation manifest plus its content hash.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// Per-mesh expectations, in document order.
    pub meshes: Vec<Expectation>,
    /// SHA-256 of the manifest file's bytes, lowercase hex.
    pub sha256: String,
}

/// Loads and validates an expectation manifest. Parse
/// failures are `QC-E-MANIFEST-PARSE`; wrong shapes, unknown
/// keys, and duplicate mesh names are `QC-E-MANIFEST-SHAPE`.
pub fn load_manifest(path: &Path) -> Result<Manifest, QcError> {
    let bytes = read_bounded(path, MANIFEST_BYTES_MAX, "MANIFEST")?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let doc: Value = serde_json::from_slice(&bytes).map_err(|err| {
        QcError::new(
            "QC-E-MANIFEST-PARSE",
            format!("manifest is not valid JSON: {err}"),
        )
    })?;
    let root = doc
        .as_object()
        .ok_or_else(|| QcError::new("QC-E-MANIFEST-SHAPE", "manifest must be a JSON object"))?;
    for key in root.keys() {
        if key != "meshes" {
            return Err(shape(format!("unknown manifest key {key:?}")));
        }
    }
    let entries = root
        .get("meshes")
        .and_then(Value::as_array)
        .ok_or_else(|| shape("manifest.meshes must be an array".to_string()))?;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut meshes = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let entry = entry
            .as_object()
            .ok_or_else(|| shape(format!("manifest.meshes[{index}] must be an object")))?;
        for key in entry.keys() {
            if !matches!(
                key.as_str(),
                "name"
                    | "morph_targets"
                    | "morph_target_count"
                    | "uv_channels"
                    | "require_tangents"
            ) {
                return Err(shape(format!(
                    "unknown manifest.meshes[{index}] key {key:?}"
                )));
            }
        }
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| shape(format!("manifest.meshes[{index}].name must be a string")))?
            .to_string();
        if !seen.insert(name.clone()) {
            return Err(shape(format!(
                "manifest names mesh {name:?} twice; expectations must be unambiguous"
            )));
        }
        let morph_targets = match entry.get("morph_targets") {
            Some(value) => {
                let names = value.as_array().ok_or_else(|| {
                    shape(format!(
                        "manifest.meshes[{index}].morph_targets must be an array of strings"
                    ))
                })?;
                let mut parsed = Vec::new();
                for name in names {
                    parsed.push(
                        name.as_str()
                            .ok_or_else(|| {
                                shape(format!(
                                    "manifest.meshes[{index}].morph_targets must be strings"
                                ))
                            })?
                            .to_string(),
                    );
                }
                parsed
            }
            None => Vec::new(),
        };
        let morph_target_count = match entry.get("morph_target_count") {
            Some(value) => Some(count_value(value, index, "morph_target_count")?),
            None => None,
        };
        let uv_channels = match entry.get("uv_channels") {
            Some(value) => Some(count_value(value, index, "uv_channels")?),
            None => None,
        };
        let require_tangents = match entry.get("require_tangents") {
            Some(value) => value.as_bool().ok_or_else(|| {
                shape(format!(
                    "manifest.meshes[{index}].require_tangents must be a boolean"
                ))
            })?,
            None => false,
        };
        meshes.push(Expectation {
            name,
            morph_targets,
            morph_target_count,
            uv_channels,
            require_tangents,
        });
    }
    Ok(Manifest { meshes, sha256 })
}

fn count_value(value: &Value, index: usize, field: &str) -> Result<usize, QcError> {
    value.as_u64().map(|n| n as usize).ok_or_else(|| {
        shape(format!(
            "manifest.meshes[{index}].{field} must be a non-negative integer"
        ))
    })
}

fn shape(message: String) -> QcError {
    QcError::new("QC-E-MANIFEST-SHAPE", message)
}

/// Audits one exported GLB against a manifest and returns the
/// report.
pub fn export_check(glb_path: &Path, manifest_path: &Path) -> Result<Report, QcError> {
    let manifest = load_manifest(manifest_path)?;
    let model = GlbModel::load(glb_path)?;
    let scene = manifest_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export-check".to_string());
    let mut report = Report::new("export-check", scene);
    report
        .extra
        .push(("manifest_sha256", json!(manifest.sha256)));
    report
        .extra
        .push(("meshes_in_file", json!(model.meshes().len())));
    report
        .extra
        .push(("meshes_in_manifest", json!(manifest.meshes.len())));
    for finding in audit_export(&model, &manifest) {
        report.push(finding);
    }
    report.sort_findings();
    Ok(report)
}

/// The B2 audit itself: every `QC_EXPORT_LOSS` finding the
/// exported file earns against the manifest, at stage
/// `in_file`. Shared with the round-trip tier, which runs the
/// same audit as its first half.
pub fn audit_export(model: &GlbModel, manifest: &Manifest) -> Vec<Finding> {
    let mut findings = Vec::new();
    let meshes = model.meshes();
    let materials = model.materials();
    let mut matched: BTreeSet<usize> = BTreeSet::new();
    for (manifest_index, expectation) in manifest.meshes.iter().enumerate() {
        let spec_ref = format!("/meshes/{manifest_index}");
        let found = meshes
            .iter()
            .enumerate()
            .find(|(_, mesh)| mesh.name.as_deref() == Some(expectation.name.as_str()));
        match found {
            Some((mesh_index, mesh)) => {
                matched.insert(mesh_index);
                check_morph_targets(mesh, mesh_index, expectation, &spec_ref, &mut findings);
                check_primitives(
                    mesh,
                    mesh_index,
                    expectation,
                    &materials,
                    &spec_ref,
                    &mut findings,
                );
            }
            None => {
                // The mesh itself is absent: everything it
                // declared is lost with it.
                for target in &expectation.morph_targets {
                    findings.push(loss(
                        KIND_SHAPE_KEY_DROPPED,
                        &expectation.name,
                        &spec_ref,
                        json!({"target": target}),
                        format!(
                            "{KIND_SHAPE_KEY_DROPPED}: mesh {:?} is absent from the exported file; morph target {target:?} is lost with it",
                            expectation.name,
                        ),
                    ));
                }
                if expectation.morph_targets.is_empty()
                    && expectation
                        .morph_target_count
                        .is_some_and(|count| count > 0)
                {
                    let count = expectation.morph_target_count.unwrap_or(0);
                    findings.push(loss(
                        KIND_SHAPE_KEY_DROPPED,
                        &expectation.name,
                        &spec_ref,
                        json!({"expected_count": count, "actual_count": 0}),
                        format!(
                            "{KIND_SHAPE_KEY_DROPPED}: mesh {:?} is absent from the exported file; its {count} morph targets are lost with it",
                            expectation.name,
                        ),
                    ));
                }
                if expectation.uv_channels.is_some_and(|channels| channels > 0) {
                    findings.push(loss(
                        KIND_UV_MISSING,
                        &expectation.name,
                        &spec_ref,
                        json!({}),
                        format!(
                            "{KIND_UV_MISSING}: mesh {:?} is absent from the exported file; its declared UV channels are lost with it",
                            expectation.name,
                        ),
                    ));
                }
                if expectation.require_tangents {
                    findings.push(loss(
                        KIND_TANGENTS_MISSING,
                        &expectation.name,
                        &spec_ref,
                        json!({}),
                        format!(
                            "{KIND_TANGENTS_MISSING}: mesh {:?} is absent from the exported file; its declared tangents are lost with it",
                            expectation.name,
                        ),
                    ));
                }
            }
        }
    }
    // Intrinsic checks for meshes the manifest does not name.
    for (mesh_index, mesh) in meshes.iter().enumerate() {
        if matched.contains(&mesh_index) {
            continue;
        }
        let spec_ref = format!("/glb/meshes/{mesh_index}");
        check_primitives(
            mesh,
            mesh_index,
            &INTRINSIC,
            &materials,
            &spec_ref,
            &mut findings,
        );
    }
    findings
}

/// The empty expectation intrinsic checks run under: no
/// declared targets or channels, only what the file's own
/// materials force.
const INTRINSIC: Expectation = Expectation {
    name: String::new(),
    morph_targets: Vec::new(),
    morph_target_count: None,
    uv_channels: None,
    require_tangents: false,
};

/// Morph-target expectations for one present mesh.
fn check_morph_targets(
    mesh: &MeshInfo,
    mesh_index: usize,
    expectation: &Expectation,
    spec_ref: &str,
    findings: &mut Vec<Finding>,
) {
    let label = mesh_label(mesh, mesh_index);
    if !expectation.morph_targets.is_empty() {
        if !mesh.target_names.is_empty() {
            for target in &expectation.morph_targets {
                if !mesh.target_names.contains(target) {
                    findings.push(loss(
                        KIND_SHAPE_KEY_DROPPED,
                        &label,
                        spec_ref,
                        json!({"target": target}),
                        format!(
                            "{KIND_SHAPE_KEY_DROPPED}: mesh {label:?} is missing morph target {target:?} named in the expectation manifest"
                        ),
                    ));
                }
            }
        } else if mesh.target_count < expectation.morph_targets.len() {
            findings.push(loss(
                KIND_SHAPE_KEY_DROPPED,
                &label,
                spec_ref,
                json!({
                    "expected_count": expectation.morph_targets.len(),
                    "actual_count": mesh.target_count,
                }),
                format!(
                    "{KIND_SHAPE_KEY_DROPPED}: mesh {label:?} carries {} morph targets, fewer than the {} named in the expectation manifest, and names none of them (no extras.targetNames)",
                    mesh.target_count,
                    expectation.morph_targets.len(),
                ),
            ));
        }
    }
    if let Some(expected) = expectation.morph_target_count {
        if mesh.target_count < expected {
            findings.push(loss(
                KIND_SHAPE_KEY_DROPPED,
                &label,
                spec_ref,
                json!({"expected_count": expected, "actual_count": mesh.target_count}),
                format!(
                    "{KIND_SHAPE_KEY_DROPPED}: mesh {label:?} carries {} morph targets, fewer than the {expected} the expectation manifest declares",
                    mesh.target_count,
                ),
            ));
        }
    }
}

/// UV-channel and tangent expectations for one mesh's
/// primitives, manifest-driven and intrinsic combined: a
/// primitive needs as many UV channels as the manifest
/// declares, and at least one when its material binds any
/// texture; it needs tangents when its material binds a
/// normal map or the manifest requires them.
fn check_primitives(
    mesh: &MeshInfo,
    mesh_index: usize,
    expectation: &Expectation,
    materials: &[crate::glb::MaterialInfo],
    spec_ref: &str,
    findings: &mut Vec<Finding>,
) {
    let label = mesh_label(mesh, mesh_index);
    for (primitive_index, primitive) in mesh.primitives.iter().enumerate() {
        let material = primitive.material.and_then(|index| materials.get(index));
        let textured = material.is_some_and(|m| !m.textures.is_empty());
        let normal_mapped = material.is_some_and(|m| m.has_normal_texture);
        let required_uv = expectation
            .uv_channels
            .unwrap_or(0)
            .max(if textured { 1 } else { 0 });
        let actual_uv = (0..8)
            .take_while(|channel| {
                primitive
                    .attributes
                    .contains(&format!("TEXCOORD_{channel}"))
            })
            .count();
        if actual_uv < required_uv {
            findings.push(loss(
                KIND_UV_MISSING,
                &label,
                spec_ref,
                json!({"primitive": primitive_index, "required": required_uv, "actual": actual_uv}),
                format!(
                    "{KIND_UV_MISSING}: mesh {label:?} primitive {primitive_index} carries {actual_uv} UV channels, fewer than the {required_uv} its material and expectations require"
                ),
            ));
        }
        if (normal_mapped || expectation.require_tangents)
            && !primitive.attributes.contains("TANGENT")
        {
            findings.push(loss(
                KIND_TANGENTS_MISSING,
                &label,
                spec_ref,
                json!({"primitive": primitive_index}),
                format!(
                    "{KIND_TANGENTS_MISSING}: mesh {label:?} primitive {primitive_index} carries no TANGENT attribute, but its material binds a normal map or the manifest requires tangents"
                ),
            ));
        }
    }
}

/// Builds one `QC_EXPORT_LOSS` finding at stage `in_file`.
fn loss(
    kind: &'static str,
    mesh_label: &str,
    spec_ref: &str,
    extra_data: Value,
    detail: String,
) -> Finding {
    let mut data = json!({"kind": kind, "stage": STAGE_IN_FILE, "mesh": mesh_label});
    if let (Some(target), Some(extra)) = (data.as_object_mut(), extra_data.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    Finding {
        code: "QC_EXPORT_LOSS",
        severity: Severity::Fail,
        element: Some(mesh_label.to_string()),
        frame: None,
        detail,
        spec_ref: spec_ref.to_string(),
        data: Some(data),
        scene_override: None,
    }
}

/// A mesh's finding label: its name, or `#<index>` (its
/// document position) when unnamed.
fn mesh_label(mesh: &MeshInfo, mesh_index: usize) -> String {
    mesh.name
        .clone()
        .unwrap_or_else(|| format!("#{mesh_index}"))
}
