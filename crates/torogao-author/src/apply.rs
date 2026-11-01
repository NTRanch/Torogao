//! Applying admitted ops.
//!
//! `apply` runs only after `decide()` returned `Verdict::Apply`;
//! it asserts the facts the gate established instead of
//! re-checking them. `step` is the one entry point that pairs the
//! two, so no caller can apply an op the gate did not see.

use crate::decide::{Observed, TRIM_INDICES, Verdict, blend_of, completion_of, decide, trim_quad};
use crate::model::{
    ArtBind, Asset, AssetKind, Clip, Deformer, DeformerKind, Draw, Expression, ExpressionValue,
    ExpressionVisibility, Glue, Header, Keyform, Keyforms, Mesh, MeshForm, Parameter, Part,
    PoseEntry, Project, Provenance, Rect, Rotation, RotationForm, VariantSet, Warp, WarpForm,
    locate,
};
use crate::op::{ExpressionDefineInput, FormInput, IntakePsdInput, KeyformSetInput, Op};
use crate::refusal::Refusal;
use torogao_format::MODEL_FORMAT_TOKEN;

/// A file the store must hold after an op commits: a
/// project-relative path and its bytes (borrowed from the
/// observation, so large layers are not copied).
#[derive(Debug)]
pub struct StoredFile<'a> {
    /// Project-relative path (`assets/<sha256>.<ext>`).
    pub path: String,
    /// Content; its sha256 is the name's hash.
    pub bytes: &'a [u8],
}

/// The result of one admitted step.
#[derive(Debug)]
pub struct Stepped<'a> {
    /// The project after the op.
    pub project: Project,
    /// Files the op adds to the asset store.
    pub files: Vec<StoredFile<'a>>,
    /// Whether the op changed anything.
    pub verdict: Verdict,
}

/// decide() then apply: the only way an op reaches a project.
pub fn step<'a>(
    state: Option<Project>,
    op: &Op,
    observed: &'a Observed,
) -> Result<Stepped<'a>, Refusal> {
    let verdict = decide(state.as_ref(), op, observed)?;
    let mut files = Vec::new();
    let project = match (verdict, state) {
        (Verdict::NoOp, Some(project)) => project,
        (_, state) => apply(state, op, observed, &mut files)?,
    };
    Ok(Stepped {
        project,
        files,
        verdict,
    })
}

fn invariant(what: &str) -> ! {
    panic!("decide() admitted an op whose {what} does not exist")
}

fn part_mut<'p>(project: &'p mut Project, id: &str) -> &'p mut Part {
    match locate(&project.parts, id, |part| &part.id) {
        Ok(index) => &mut project.parts[index],
        Err(_) => invariant("part"),
    }
}

fn deformer_mut<'p>(project: &'p mut Project, id: &str) -> &'p mut Deformer {
    match locate(&project.deformers, id, |deformer| &deformer.id) {
        Ok(index) => &mut project.deformers[index],
        Err(_) => invariant("deformer"),
    }
}

/// Inserts or replaces a record in an id-sorted collection.
fn upsert<T>(items: &mut Vec<T>, item: T, key: impl Fn(&T) -> &String) {
    let id = key(&item).clone();
    match locate(items, &id, &key) {
        Ok(index) => items[index] = item,
        Err(index) => items.insert(index, item),
    }
}

fn remove<T>(items: &mut Vec<T>, id: &str, key: impl Fn(&T) -> &String) {
    if let Ok(index) = locate(items, id, key) {
        items.remove(index);
    }
}

fn new_part(id: &str) -> Part {
    Part {
        id: id.to_string(),
        parent: None,
        deformer: None,
        art: None,
        mesh: None,
        clip: None,
        draw: None,
        visible: true,
        completion: None,
        keyforms: None,
    }
}

fn apply<'a>(
    state: Option<Project>,
    op: &Op,
    observed: &'a Observed,
    files: &mut Vec<StoredFile<'a>>,
) -> Result<Project, Refusal> {
    if let Op::ProjectInit(inputs) = op {
        return Ok(Project {
            header: Header {
                name: inputs.name.clone(),
                model_format: MODEL_FORMAT_TOKEN.to_string(),
                stamp: inputs.stamp.clone(),
                consent: inputs.consent.clone(),
            },
            ..Project::default()
        });
    }
    let Some(mut project) = state else {
        invariant("project")
    };
    match op {
        Op::ProjectInit(_) => unreachable!("handled above"),
        Op::ProjectImport(inputs) => {
            return crate::import::apply_import(project, inputs, observed, files);
        }
        Op::IntakeImage(inputs) => {
            let Observed::Image(image) = observed else {
                invariant("image observation")
            };
            let file = format!("assets/{}.png", image.sha256);
            files.push(StoredFile {
                path: file.clone(),
                bytes: &image.bytes,
            });
            let asset = Asset {
                id: inputs.id.clone(),
                kind: AssetKind::Png,
                file,
                sha256: image.sha256.clone(),
                width: image.width,
                height: image.height,
                content: None,
                canvas_origin: None,
                provenance: Provenance {
                    license: inputs.license.clone(),
                    origin: inputs.origin.clone(),
                    source_name: image.source_name.clone(),
                    source_sha256: image.sha256.clone(),
                    layer: None,
                },
            };
            upsert(&mut project.assets, asset, |a| &a.id);
        }
        Op::IntakePsd(inputs) => apply_psd(&mut project, inputs, observed, files),
        _ => apply_edit(&mut project, op)?,
    }
    Ok(project)
}

fn apply_psd<'a>(
    project: &mut Project,
    inputs: &IntakePsdInput,
    observed: &'a Observed,
    files: &mut Vec<StoredFile<'a>>,
) {
    let Observed::Psd(psd) = observed else {
        invariant("PSD observation")
    };
    files.push(StoredFile {
        path: format!("assets/{}.psd", psd.sha256),
        bytes: &psd.bytes,
    });
    let part_of = |path: &str| {
        inputs
            .layers
            .iter()
            .find(|decl| decl.layer == path)
            .map(|decl| decl.part.clone())
    };
    for layer in &psd.layers {
        let Some(decl) = inputs.layers.iter().find(|decl| decl.layer == layer.path) else {
            invariant("layer declaration")
        };
        let mut part = new_part(&decl.part);
        part.parent = layer.group.as_deref().and_then(part_of);
        part.completion = decl.completion.map(completion_of);
        part.clip = decl.clip_base.clone().map(|source| Clip {
            source,
            inverted: false,
        });
        if let Some(image) = &layer.image {
            let file = format!("assets/{}.png", image.sha256);
            files.push(StoredFile {
                path: file.clone(),
                bytes: &image.png,
            });
            let full = Rect {
                x: 0,
                y: 0,
                width: image.width,
                height: image.height,
            };
            upsert(
                &mut project.assets,
                Asset {
                    id: decl.part.clone(),
                    kind: AssetKind::PsdLayer,
                    file,
                    sha256: image.sha256.clone(),
                    width: image.width,
                    height: image.height,
                    content: Some(full),
                    canvas_origin: Some(image.canvas_origin),
                    provenance: Provenance {
                        license: inputs.license.clone(),
                        origin: inputs.origin.clone(),
                        source_name: psd.source_name.clone(),
                        source_sha256: psd.sha256.clone(),
                        layer: Some(layer.path.clone()),
                    },
                },
                |a| &a.id,
            );
            part.art = Some(ArtBind {
                asset: decl.part.clone(),
                region: full,
            });
        }
        upsert(&mut project.parts, part, |p| &p.id);
    }
    apply_variant_sets(project, inputs);
}

/// Variant sets in declaration order; the first member of each set
/// is visible by default, the rest hidden.
fn apply_variant_sets(project: &mut Project, inputs: &IntakePsdInput) {
    let mut sets: Vec<VariantSet> = Vec::new();
    for decl in &inputs.layers {
        let Some(set_id) = &decl.variant_set else {
            continue;
        };
        match sets.iter_mut().find(|set| &set.id == set_id) {
            Some(set) => {
                set.variants.push(decl.part.clone());
                part_mut(project, &decl.part).visible = false;
            }
            None => sets.push(VariantSet {
                id: set_id.clone(),
                variants: vec![decl.part.clone()],
            }),
        }
    }
    for set in sets {
        upsert(&mut project.variant_sets, set, |s| &s.id);
    }
}

/// Builds the expression record an `expression_define` produces
/// (rows sorted by id). Shared with decide's identity check.
pub fn expression_record(inputs: &ExpressionDefineInput) -> Expression {
    let mut values: Vec<ExpressionValue> = inputs
        .values
        .iter()
        .map(|row| ExpressionValue {
            parameter: row.parameter.clone(),
            value: row.value,
        })
        .collect();
    values.sort_by(|a, b| a.parameter.cmp(&b.parameter));
    let mut visibility: Vec<ExpressionVisibility> = inputs
        .visibility
        .iter()
        .map(|row| ExpressionVisibility {
            part: row.part.clone(),
            visible: row.visible,
        })
        .collect();
    visibility.sort_by(|a, b| a.part.cmp(&b.part));
    Expression {
        id: inputs.id.clone(),
        blend: blend_of(inputs.blend),
        values,
        visibility,
    }
}

fn apply_edit(project: &mut Project, op: &Op) -> Result<(), Refusal> {
    match op {
        Op::PartCreate(inputs) => upsert(&mut project.parts, new_part(&inputs.id), |p| &p.id),
        Op::PartParentSet(inputs) => part_mut(project, &inputs.part).parent = inputs.parent.clone(),
        Op::PartRemove(inputs) => remove(&mut project.parts, &inputs.part, |p| &p.id),
        Op::PartVisibilitySet(inputs) => part_mut(project, &inputs.part).visible = inputs.visible,
        Op::PartArtBind(inputs) => {
            let region = Rect {
                x: inputs.region.x,
                y: inputs.region.y,
                width: inputs.region.width,
                height: inputs.region.height,
            };
            part_mut(project, &inputs.part).art = Some(ArtBind {
                asset: inputs.asset.clone(),
                region,
            });
        }
        Op::PartClipSet(inputs) => {
            part_mut(project, &inputs.part).clip = inputs.source.clone().map(|source| Clip {
                source,
                inverted: inputs.inverted,
            });
        }
        Op::PartDrawOrderSet(inputs) => {
            part_mut(project, &inputs.part).draw = Some(Draw {
                band: inputs.band,
                index: inputs.index,
                group: inputs.group.clone(),
            });
        }
        Op::MeshSet(inputs) => {
            part_mut(project, &inputs.part).mesh = Some(Mesh {
                vertices: inputs.vertices.clone(),
                uvs: inputs.uvs.clone(),
                indices: inputs.indices.clone(),
            });
        }
        Op::MeshTrim(inputs) => {
            let Some(part) = project.part(&inputs.part) else {
                invariant("part")
            };
            let (vertices, uvs) = trim_quad(project, part)?;
            part_mut(project, &inputs.part).mesh = Some(Mesh {
                vertices,
                uvs,
                indices: TRIM_INDICES.to_vec(),
            });
        }
        _ => apply_rig(project, op),
    }
    Ok(())
}

fn apply_rig(project: &mut Project, op: &Op) {
    match op {
        Op::DeformerRotationCreate(inputs) => {
            let deformer = Deformer {
                id: inputs.id.clone(),
                parent: None,
                kind: DeformerKind::Rotation(Rotation {
                    origin: inputs.origin,
                    base_angle_degrees: inputs.base_angle_degrees,
                    keyforms: None,
                }),
            };
            upsert(&mut project.deformers, deformer, |d| &d.id);
        }
        Op::DeformerWarpCreate(inputs) => {
            let deformer = Deformer {
                id: inputs.id.clone(),
                parent: None,
                kind: DeformerKind::Warp(Warp {
                    columns: inputs.columns,
                    rows: inputs.rows,
                    bounds: inputs.bounds,
                    keyforms: None,
                }),
            };
            upsert(&mut project.deformers, deformer, |d| &d.id);
        }
        Op::DeformerParentSet(inputs) => {
            if project.part(&inputs.target).is_some() {
                part_mut(project, &inputs.target).deformer = inputs.parent.clone();
            } else {
                deformer_mut(project, &inputs.target).parent = inputs.parent.clone();
            }
        }
        Op::DeformerRemove(inputs) => remove(&mut project.deformers, &inputs.deformer, |d| &d.id),
        Op::DeformerGlueSet(inputs) => {
            let glue = Glue {
                id: inputs.id.clone(),
                a: inputs.a.clone(),
                b: inputs.b.clone(),
                pairs: inputs.pairs.clone(),
                weight: inputs.weight,
            };
            upsert(&mut project.glues, glue, |g| &g.id);
        }
        Op::ParamDefine(inputs) => {
            let parameter = Parameter {
                id: inputs.id.clone(),
                class: inputs.class.clone(),
                min: inputs.min,
                default: inputs.default,
                max: inputs.max,
            };
            upsert(&mut project.parameters, parameter, |p| &p.id);
        }
        Op::ParamGridDefine(inputs) => apply_grid(project, &inputs.target, &inputs.axes),
        Op::ParamPoseDefaultSet(inputs) => apply_pose(project, &inputs.parameter, inputs.value),
        Op::ParamRemove(inputs) => remove(&mut project.parameters, &inputs.parameter, |p| &p.id),
        Op::KeyformSet(inputs) => apply_keyform(project, inputs),
        Op::ExpressionDefine(inputs) => {
            upsert(&mut project.expressions, expression_record(inputs), |e| {
                &e.id
            });
        }
        Op::ExpressionRemove(inputs) => remove(&mut project.expressions, &inputs.id, |e| &e.id),
        other => unreachable!("apply routes {} before apply_rig", other.verb()),
    }
}

/// The pose holds only values that differ from the declared
/// default; setting the default removes the entry.
fn apply_pose(project: &mut Project, parameter: &str, value: f64) {
    let Some(declared) = project.parameter(parameter).map(|p| p.default) else {
        invariant("parameter")
    };
    if value == declared {
        remove(&mut project.pose, parameter, |e| &e.parameter);
    } else {
        let entry = PoseEntry {
            parameter: parameter.to_string(),
            value,
        };
        upsert(&mut project.pose, entry, |e| &e.parameter);
    }
}

fn apply_grid(project: &mut Project, target: &str, axes: &[String; 2]) {
    let axes = axes.to_vec();
    if project.part(target).is_some() {
        part_mut(project, target).keyforms = Some(Keyforms {
            axes,
            forms: Vec::new(),
        });
        return;
    }
    match &mut deformer_mut(project, target).kind {
        DeformerKind::Rotation(rotation) => {
            rotation.keyforms = Some(Keyforms {
                axes,
                forms: Vec::new(),
            });
        }
        DeformerKind::Warp(warp) => {
            warp.keyforms = Some(Keyforms {
                axes,
                forms: Vec::new(),
            })
        }
    }
}

/// Inserts or replaces the form at `at`, keeping forms sorted by
/// coordinate (lexicographic, total order on floats).
fn set_form<F>(slot: &mut Option<Keyforms<F>>, axes: Vec<String>, at: Vec<f64>, form: F) {
    let keyforms = slot.get_or_insert_with(|| Keyforms {
        axes,
        forms: Vec::new(),
    });
    let position = keyforms
        .forms
        .binary_search_by(|existing| compare_at(&existing.at, &at));
    match position {
        Ok(index) => keyforms.forms[index].form = form,
        Err(index) => keyforms.forms.insert(index, Keyform { at, form }),
    }
}

/// Lexicographic coordinate order.
pub fn compare_at(left: &[f64], right: &[f64]) -> std::cmp::Ordering {
    for (a, b) in left.iter().zip(right) {
        let ordering = a.total_cmp(b);
        if ordering.is_ne() {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn apply_keyform(project: &mut Project, inputs: &KeyformSetInput) {
    let axes: Vec<String> = inputs.at.iter().map(|c| c.parameter.clone()).collect();
    let at: Vec<f64> = inputs.at.iter().map(|c| c.value).collect();
    match &inputs.form {
        FormInput::Mesh { vertices } => {
            let part = part_mut(project, &inputs.target);
            let form = MeshForm {
                vertices: vertices.clone(),
            };
            set_form(&mut part.keyforms, axes, at, form);
        }
        FormInput::Rotation {
            angle_degrees,
            scale,
        } => {
            let DeformerKind::Rotation(rotation) = &mut deformer_mut(project, &inputs.target).kind
            else {
                invariant("rotation deformer")
            };
            let form = RotationForm {
                angle_degrees: *angle_degrees,
                scale: *scale,
            };
            set_form(&mut rotation.keyforms, axes, at, form);
        }
        FormInput::Warp { positions } => {
            let DeformerKind::Warp(warp) = &mut deformer_mut(project, &inputs.target).kind else {
                invariant("warp deformer")
            };
            let form = WarpForm {
                positions: positions.clone(),
            };
            set_form(&mut warp.keyforms, axes, at, form);
        }
    }
}
