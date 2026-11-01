//! `project_validate`: the authored-project analogue of
//! `rig-check`.
//!
//! Four checks, each reported as typed findings:
//! 1. Round trip: the file must re-emit byte-for-byte
//!    (`TG-AUTH-NONCANONICAL`).
//! 2. The decide() sweep: the project is rebuilt from nothing by
//!    synthesizing the op for every stored fact and passing each
//!    through `step`; the first refusal is a finding, and a rebuilt
//!    project that differs from the stored one (rows out of
//!    canonical order, a pose entry equal to its default) is
//!    `TG-AUTH-NONCANONICAL`. Validation therefore has no rules of
//!    its own: whatever the gate would refuse, validation reports.
//! 3. Lattice completeness: every keyed target carries a form at
//!    every key intersection, with at least two keys per axis
//!    (`TG-AUTH-INCOMPLETE`).
//! 4. Asset integrity: every asset file exists under its content
//!    hash (`TG-AUTH-DANGLING` / `TG-AUTH-PROVENANCE`).

use crate::apply::step;
use crate::codec::emit_project;
use crate::decide::{Observed, check_id};
use crate::intake::sha256_hex;
use crate::limits::{ASSETS_MAX, IMAGE_SIDE_MAX, PROVENANCE_TEXT_BYTES_MAX};
use crate::model::{Asset, DeformerKind, Keyforms, Project};
use crate::op::{
    ArtBindInput, BlendInput, ClipInput, Coordinate, DeformerParentInput, DrawInput,
    ExpressionDefineInput, FormInput, GlueInput, GridInput, IdInput, InitInput, KeyformSetInput,
    MeshSetInput, Op, ParamDefineInput, PartParentInput, PoseInput, RectInput, RotationCreateInput,
    VisibilityInput, VisibilityRow, WarpCreateInput,
};
use crate::refusal::{Refusal, RefusalClass};
use std::path::Path;
use torogao_format::MODEL_FORMAT_TOKEN;

/// Validates a loaded project; `text` is the file as read and
/// `root` the project directory (for asset integrity).
pub fn validate(project: &Project, text: &str, root: &Path) -> Vec<Refusal> {
    let mut findings = Vec::new();
    if emit_project(project) != text {
        findings.push(Refusal::new(
            RefusalClass::NonCanonical,
            "project.ron does not re-emit byte-for-byte; the next op re-canonicalizes it",
        ));
    }
    if project.header.model_format != MODEL_FORMAT_TOKEN {
        findings.push(Refusal::new(
            RefusalClass::UnknownKind,
            format!(
                "header model_format '{}' is not '{MODEL_FORMAT_TOKEN}'",
                project.header.model_format
            ),
        ));
    }
    match sweep(project) {
        Err(refusal) => findings.push(refusal),
        Ok(rebuilt) if rebuilt != *project => findings.push(Refusal::new(
            RefusalClass::NonCanonical,
            "stored records differ from what their ops produce (row order or redundant pose)",
        )),
        Ok(_) => {}
    }
    findings.extend(lattice_findings(project));
    findings.extend(asset_findings(project, root));
    findings
}

fn run(state: Project, op: Op) -> Result<Project, Refusal> {
    let observed = Observed::Nothing;
    step(Some(state), &op, &observed).map(|stepped| stepped.project)
}

/// Rebuilds the project through the gate.
pub fn sweep(project: &Project) -> Result<Project, Refusal> {
    let header = &project.header;
    let init = Op::ProjectInit(InitInput {
        name: header.name.clone(),
        stamp: header.stamp.clone(),
        consent: header.consent.clone(),
    });
    let mut rebuilt = step(None, &init, &Observed::Nothing)?.project;
    rebuilt.header.model_format = header.model_format.clone();
    for asset in &project.assets {
        check_asset(asset)?;
    }
    if project.assets.len() > ASSETS_MAX {
        return Err(Refusal::new(RefusalClass::Range, "too many assets"));
    }
    rebuilt.assets = project.assets.clone();
    for op in structure_ops(project) {
        rebuilt = run(rebuilt, op)?;
    }
    for part in &project.parts {
        let Ok(index) = rebuilt.parts.binary_search_by(|p| p.id.cmp(&part.id)) else {
            unreachable!("the sweep created every part");
        };
        rebuilt.parts[index].completion = part.completion;
    }
    for op in content_ops(project) {
        rebuilt = run(rebuilt, op)?;
    }
    for set in &project.variant_sets {
        check_id(&set.id, "variant set id")?;
        let all_parts = set.variants.iter().all(|id| project.part(id).is_some());
        if set.variants.len() < 2 || !all_parts {
            return Err(Refusal::new(
                RefusalClass::Dangling,
                format!("variant set '{}' needs two or more existing parts", set.id),
            ));
        }
    }
    rebuilt.variant_sets = project.variant_sets.clone();
    Ok(rebuilt)
}

fn check_text(text: &str, what: &str) -> Result<(), Refusal> {
    if text.trim().is_empty() || text.len() > PROVENANCE_TEXT_BYTES_MAX {
        return Err(Refusal::new(
            RefusalClass::Provenance,
            format!("asset {what} is missing or over the bound"),
        ));
    }
    Ok(())
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The facts an intake op establishes for one asset record.
fn check_asset(asset: &Asset) -> Result<(), Refusal> {
    check_id(&asset.id, "asset id")?;
    let provenance = &asset.provenance;
    check_text(&provenance.license, "license")?;
    check_text(&provenance.origin, "origin")?;
    check_text(&provenance.source_name, "source name")?;
    if !is_sha256(&asset.sha256) || !is_sha256(&provenance.source_sha256) {
        return Err(Refusal::new(
            RefusalClass::Provenance,
            format!("asset '{}' carries a malformed sha256", asset.id),
        ));
    }
    if asset.file != format!("assets/{}.png", asset.sha256) {
        return Err(Refusal::new(
            RefusalClass::Malformed,
            format!("asset '{}' file is not its content-hashed name", asset.id),
        ));
    }
    let sides_ok =
        (1..=IMAGE_SIDE_MAX).contains(&asset.width) && (1..=IMAGE_SIDE_MAX).contains(&asset.height);
    let content_ok = asset.content.is_none_or(|rect| {
        rect.width > 0
            && rect.height > 0
            && rect
                .x
                .checked_add(rect.width)
                .is_some_and(|end| end <= asset.width)
            && rect
                .y
                .checked_add(rect.height)
                .is_some_and(|end| end <= asset.height)
    });
    if !sides_ok || !content_ok {
        return Err(Refusal::new(
            RefusalClass::Range,
            format!(
                "asset '{}' dimensions or content bounds are out of range",
                asset.id
            ),
        ));
    }
    Ok(())
}

/// Parameters, nodes, and the two trees.
fn structure_ops(project: &Project) -> Vec<Op> {
    let mut ops = Vec::new();
    for parameter in &project.parameters {
        ops.push(Op::ParamDefine(ParamDefineInput {
            id: parameter.id.clone(),
            class: parameter.class.clone(),
            min: parameter.min,
            default: parameter.default,
            max: parameter.max,
        }));
    }
    for part in &project.parts {
        ops.push(Op::PartCreate(IdInput {
            id: part.id.clone(),
        }));
    }
    for deformer in &project.deformers {
        ops.push(match &deformer.kind {
            DeformerKind::Rotation(rotation) => Op::DeformerRotationCreate(RotationCreateInput {
                id: deformer.id.clone(),
                origin: rotation.origin,
                base_angle_degrees: rotation.base_angle_degrees,
            }),
            DeformerKind::Warp(warp) => Op::DeformerWarpCreate(WarpCreateInput {
                id: deformer.id.clone(),
                columns: warp.columns,
                rows: warp.rows,
                bounds: warp.bounds,
            }),
        });
    }
    for part in project.parts.iter().filter(|p| p.parent.is_some()) {
        ops.push(Op::PartParentSet(PartParentInput {
            part: part.id.clone(),
            parent: part.parent.clone(),
        }));
    }
    for deformer in project.deformers.iter().filter(|d| d.parent.is_some()) {
        ops.push(Op::DeformerParentSet(DeformerParentInput {
            target: deformer.id.clone(),
            parent: deformer.parent.clone(),
        }));
    }
    ops
}

/// Per-part content, keyforms, glues, pose, and expressions.
fn content_ops(project: &Project) -> Vec<Op> {
    let mut ops = Vec::new();
    for part in &project.parts {
        let id = || part.id.clone();
        if part.deformer.is_some() {
            ops.push(Op::DeformerParentSet(DeformerParentInput {
                target: id(),
                parent: part.deformer.clone(),
            }));
        }
        if let Some(art) = &part.art {
            let region = art.region;
            ops.push(Op::PartArtBind(ArtBindInput {
                part: id(),
                asset: art.asset.clone(),
                region: RectInput {
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height,
                },
            }));
        }
        if let Some(mesh) = &part.mesh {
            ops.push(Op::MeshSet(MeshSetInput {
                part: id(),
                vertices: mesh.vertices.clone(),
                uvs: mesh.uvs.clone(),
                indices: mesh.indices.clone(),
            }));
        }
        if let Some(draw) = &part.draw {
            ops.push(Op::PartDrawOrderSet(DrawInput {
                part: id(),
                band: draw.band,
                index: draw.index,
                group: draw.group.clone(),
            }));
        }
        if !part.visible {
            ops.push(Op::PartVisibilitySet(VisibilityInput {
                part: id(),
                visible: false,
            }));
        }
    }
    for part in project.parts.iter().filter(|p| p.clip.is_some()) {
        if let Some(clip) = &part.clip {
            ops.push(Op::PartClipSet(ClipInput {
                part: part.id.clone(),
                source: Some(clip.source.clone()),
                inverted: clip.inverted,
            }));
        }
    }
    keyform_ops(project, &mut ops);
    for glue in &project.glues {
        ops.push(Op::DeformerGlueSet(GlueInput {
            id: glue.id.clone(),
            a: glue.a.clone(),
            b: glue.b.clone(),
            pairs: glue.pairs.clone(),
            weight: glue.weight,
        }));
    }
    for entry in &project.pose {
        ops.push(Op::ParamPoseDefaultSet(PoseInput {
            parameter: entry.parameter.clone(),
            value: entry.value,
        }));
    }
    for expression in &project.expressions {
        ops.push(Op::ExpressionDefine(ExpressionDefineInput {
            id: expression.id.clone(),
            blend: match expression.blend {
                crate::model::Blend::Additive => BlendInput::Additive,
                crate::model::Blend::Override => BlendInput::Override,
            },
            values: expression
                .values
                .iter()
                .map(|row| Coordinate {
                    parameter: row.parameter.clone(),
                    value: row.value,
                })
                .collect(),
            visibility: expression
                .visibility
                .iter()
                .map(|row| VisibilityRow {
                    part: row.part.clone(),
                    visible: row.visible,
                })
                .collect(),
        }));
    }
    ops
}

fn lattice_ops<F>(
    target: &str,
    keyforms: &Keyforms<F>,
    ops: &mut Vec<Op>,
    payload: impl Fn(&F) -> FormInput,
) {
    if let [first, second] = &keyforms.axes[..] {
        ops.push(Op::ParamGridDefine(GridInput {
            target: target.to_string(),
            axes: [first.clone(), second.clone()],
        }));
    }
    for form in &keyforms.forms {
        ops.push(Op::KeyformSet(KeyformSetInput {
            target: target.to_string(),
            at: keyforms
                .axes
                .iter()
                .zip(&form.at)
                .map(|(parameter, value)| Coordinate {
                    parameter: parameter.clone(),
                    value: *value,
                })
                .collect(),
            form: payload(&form.form),
        }));
    }
}

fn keyform_ops(project: &Project, ops: &mut Vec<Op>) {
    for part in &project.parts {
        if let Some(keyforms) = &part.keyforms {
            lattice_ops(&part.id, keyforms, ops, |form| FormInput::Mesh {
                vertices: form.vertices.clone(),
            });
        }
    }
    for deformer in &project.deformers {
        match &deformer.kind {
            DeformerKind::Rotation(rotation) => {
                if let Some(keyforms) = &rotation.keyforms {
                    lattice_ops(&deformer.id, keyforms, ops, |form| FormInput::Rotation {
                        angle_degrees: form.angle_degrees,
                        scale: form.scale,
                    });
                }
            }
            DeformerKind::Warp(warp) => {
                if let Some(keyforms) = &warp.keyforms {
                    lattice_ops(&deformer.id, keyforms, ops, |form| FormInput::Warp {
                        positions: form.positions.clone(),
                    });
                }
            }
        }
    }
}

/// The distinct keys per axis of a lattice, ascending.
pub fn lattice_keys<F>(keyforms: &Keyforms<F>) -> Vec<Vec<f64>> {
    (0..keyforms.axes.len())
        .map(|axis| {
            let mut keys: Vec<f64> = keyforms
                .forms
                .iter()
                .filter_map(|f| f.at.get(axis).copied())
                .collect();
            keys.sort_by(f64::total_cmp);
            keys.dedup();
            keys
        })
        .collect()
}

fn lattice_finding<F>(owner: &str, keyforms: &Keyforms<F>) -> Option<Refusal> {
    let keys = lattice_keys(keyforms);
    let expected: usize = keys.iter().map(Vec::len).product();
    let thin_axis = keys.iter().any(|axis| axis.len() < 2);
    if thin_axis || keyforms.forms.len() != expected {
        return Some(Refusal::new(
            RefusalClass::Incomplete,
            format!(
                "'{owner}' keyforms on {:?}: {} of {expected} intersections authored, \
                 key counts {:?} (each axis needs >= 2)",
                keyforms.axes,
                keyforms.forms.len(),
                keys.iter().map(Vec::len).collect::<Vec<_>>()
            ),
        ));
    }
    None
}

fn lattice_findings(project: &Project) -> Vec<Refusal> {
    let mut findings = Vec::new();
    for part in &project.parts {
        if let Some(finding) = part
            .keyforms
            .as_ref()
            .and_then(|k| lattice_finding(&part.id, k))
        {
            findings.push(finding);
        }
    }
    for deformer in &project.deformers {
        let finding = match &deformer.kind {
            DeformerKind::Rotation(r) => r
                .keyforms
                .as_ref()
                .and_then(|k| lattice_finding(&deformer.id, k)),
            DeformerKind::Warp(w) => w
                .keyforms
                .as_ref()
                .and_then(|k| lattice_finding(&deformer.id, k)),
        };
        findings.extend(finding);
    }
    findings
}

fn asset_findings(project: &Project, root: &Path) -> Vec<Refusal> {
    let mut findings = Vec::new();
    for asset in &project.assets {
        let path = root.join(&asset.file);
        match crate::intake::read_bounded(&path, crate::limits::IMAGE_SOURCE_BYTES_MAX) {
            Err(_) => findings.push(Refusal::new(
                RefusalClass::Dangling,
                format!("asset '{}' file '{}' is missing", asset.id, asset.file),
            )),
            Ok(bytes) if sha256_hex(&bytes) != asset.sha256 => findings.push(Refusal::new(
                RefusalClass::Provenance,
                format!(
                    "asset '{}' file does not hash to its recorded sha256",
                    asset.id
                ),
            )),
            Ok(_) => {}
        }
    }
    findings
}
