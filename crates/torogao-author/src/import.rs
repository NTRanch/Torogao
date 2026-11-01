//! `project_import`: a native rig-model file (`torogao-rig-model`,
//! read through `torogao-format`'s one validator and its RP1-D1
//! token site) becomes project content.
//!
//! Import is not a second way in: the model is translated into a
//! sequence of ordinary ops (intake_image per texture,
//! param_define, part_create, ...) and every one passes `decide()`
//! through `step`. Model constructs this slice cannot represent
//! are refused by name, never dropped: combined parameters
//! (AUTH-D3), physics groups (slice A2), non-normal blend modes,
//! opacity other than 1, more than one clipping mask per mesh,
//! draw orders outside 0..=9999, and warp rest grids that are not
//! uniform over their bounds. The model's free-text `generator`
//! field is tool metadata and is not carried.

use crate::apply::{StoredFile, step};
use crate::decide::{Observed, Verdict};
use crate::intake::{ImageObservation, base_name, observe_png, read_bounded, resolve_source};
use crate::limits::IMAGE_SOURCE_BYTES_MAX;
use crate::model::Project;
use crate::op::{
    ArtBindInput, ClipInput, Coordinate, DeformerParentInput, DrawInput, FormInput, GridInput,
    IdInput, ImportInput, IntakeImageInput, KeyformSetInput, MeshSetInput, Op, ParamDefineInput,
    PartParentInput, RectInput, RotationCreateInput, TextureSource, VisibilityInput,
    WarpCreateInput,
};
use crate::refusal::{Refusal, RefusalClass, refuse};
use std::path::{Path, PathBuf};
use torogao_format::model::{BlendMode, Deformer, Lattice, Model, Parameter};

/// Largest deviation, in model units, between a warp's stored
/// rest points and the uniform grid over their bounds that import
/// still treats as uniform (sub-pixel: f32 storage rounding).
const WARP_REST_TOLERANCE: f64 = 1e-3;

/// What `project_import` observed.
#[derive(Clone, Debug)]
pub struct ModelObservation {
    /// Canonical model path.
    pub canonical_path: String,
    /// Model base name.
    pub source_name: String,
    /// sha256 of the model bytes.
    pub sha256: String,
    /// The model bytes (kept in the asset store for replay).
    pub bytes: Vec<u8>,
    /// The validated model.
    pub model: Model,
    /// One `Observed::Image` per model texture, in model order.
    pub textures: Vec<Observed>,
}

/// Live texture reading: paths resolve against the model's
/// directory and pass the same read-only check as every other
/// source.
pub fn read_texture_live(directory: &Path, source: &str) -> Result<(PathBuf, Vec<u8>), Refusal> {
    let path = resolve_source(source, directory)?;
    let png = read_bounded(&path, IMAGE_SOURCE_BYTES_MAX)?;
    Ok((path, png))
}

/// Parses and validates a model and observes its textures;
/// `read_texture(index, source)` supplies each texture's canonical
/// path and bytes (live from disk, or from the asset store on
/// replay).
pub fn observe_model(
    canonical: &Path,
    bytes: Vec<u8>,
    read_texture: impl Fn(usize, &str) -> Result<(PathBuf, Vec<u8>), Refusal>,
) -> Result<ModelObservation, Refusal> {
    let parsed = torogao_format::parse_model(&bytes).or_else(|err| {
        refuse(
            RefusalClass::Malformed,
            format!(
                "rig model refused by the format validator: {} {err}",
                err.code()
            ),
        )
    })?;
    let mut textures = Vec::with_capacity(parsed.model.textures.len());
    for (index, texture) in parsed.model.textures.iter().enumerate() {
        let (path, png) = read_texture(index, &texture.source)?;
        textures.push(Observed::Image(observe_png(&path, png)?));
    }
    Ok(ModelObservation {
        canonical_path: canonical.to_string_lossy().into_owned(),
        source_name: base_name(canonical),
        sha256: crate::intake::sha256_hex(&bytes),
        bytes,
        model: parsed.model,
        textures,
    })
}

/// The resolved texture list the op log records.
pub fn texture_sources(model: &ModelObservation) -> Vec<TextureSource> {
    model
        .textures
        .iter()
        .map(|observed| {
            let image = texture_image(observed);
            TextureSource {
                path: image.canonical_path.clone(),
                sha256: image.sha256.clone(),
            }
        })
        .collect()
}

fn texture_image(observed: &Observed) -> &ImageObservation {
    match observed {
        Observed::Image(image) => image,
        _ => unreachable!("observe_model stores only image observations"),
    }
}

fn is_empty(project: &Project) -> bool {
    project.assets.is_empty()
        && project.deformers.is_empty()
        && project.expressions.is_empty()
        && project.glues.is_empty()
        && project.parameters.is_empty()
        && project.parts.is_empty()
        && project.pose.is_empty()
        && project.variant_sets.is_empty()
}

/// The import gate: an empty project, admissible sources, and a
/// model whose every op passes `decide()` in sequence.
pub fn decide_import(project: &Project, op: &Op, observed: &Observed) -> Result<Verdict, Refusal> {
    let Op::ProjectImport(inputs) = op else {
        unreachable!("decide routes only project_import here")
    };
    let Observed::Model(model) = observed else {
        return refuse(
            RefusalClass::Malformed,
            "project_import was not given its source",
        );
    };
    if !is_empty(project) {
        return refuse(
            RefusalClass::Conflict,
            "project_import needs a freshly initialized, empty project",
        );
    }
    if crate::intake::source_is_readonly(Path::new(&model.canonical_path)) {
        return refuse(
            RefusalClass::ReadonlySource,
            "model lies in read-only evidence; studied material never enters a project",
        );
    }
    if let Some(pin) = &inputs.source_sha256
        && *pin != model.sha256
    {
        return refuse(
            RefusalClass::Provenance,
            "model sha256 does not match the pinned value",
        );
    }
    if let Some(pinned) = &inputs.textures
        && *pinned != texture_sources(model)
    {
        return refuse(
            RefusalClass::Provenance,
            "model textures do not match the pinned paths and hashes",
        );
    }
    run_import(project.clone(), inputs, model, &mut Vec::new())?;
    Ok(Verdict::Apply)
}

/// Applies an admitted import; `inputs` supplies the license and
/// origin the textures' provenance records carry.
pub fn apply_import<'a>(
    project: Project,
    inputs: &ImportInput,
    observed: &'a Observed,
    files: &mut Vec<StoredFile<'a>>,
) -> Result<Project, Refusal> {
    let Observed::Model(model) = observed else {
        unreachable!("decide admitted project_import without its source")
    };
    files.push(StoredFile {
        path: format!("assets/{}.model.json", model.sha256),
        bytes: &model.bytes,
    });
    run_import(project, inputs, model, files)
}

/// Runs one synthesized op through the gate, prefixing refusals.
fn run_op<'a>(
    project: Project,
    op: Op,
    observed: &'a Observed,
    files: &mut Vec<StoredFile<'a>>,
) -> Result<Project, Refusal> {
    match step(Some(project), &op, observed) {
        Ok(stepped) => {
            files.extend(stepped.files);
            Ok(stepped.project)
        }
        Err(refusal) => Err(Refusal::new(
            refusal.class,
            format!("import ({}): {}", op.verb(), refusal.message),
        )),
    }
}

fn unsupported<T>(what: String) -> Result<T, Refusal> {
    refuse(RefusalClass::UnknownKind, format!("import: {what}"))
}

fn run_import<'a>(
    mut project: Project,
    inputs: &ImportInput,
    model: &'a ModelObservation,
    files: &mut Vec<StoredFile<'a>>,
) -> Result<Project, Refusal> {
    let source = &model.model;
    if !source.physics.is_empty() {
        return unsupported("physics groups are authored in slice A2".to_string());
    }
    for (texture, observed) in source.textures.iter().zip(&model.textures) {
        let image = texture_image(observed);
        let op = Op::IntakeImage(IntakeImageInput {
            id: texture.id.clone(),
            path: image.canonical_path.clone(),
            license: inputs.license.clone(),
            origin: inputs.origin.clone(),
            source_sha256: Some(image.sha256.clone()),
        });
        project = run_op(project, op, observed, files)?;
    }
    for op in structure_ops(source)? {
        project = run_op(project, op, &NOTHING, files)?;
    }
    for op in mesh_ops(source, model)? {
        project = run_op(project, op, &NOTHING, files)?;
    }
    for op in keyform_ops(source)? {
        project = run_op(project, op, &NOTHING, files)?;
    }
    Ok(project)
}

/// The observation of ops that read no source.
static NOTHING: Observed = Observed::Nothing;

/// Parameters, part groups, deformers, and their parenting.
fn structure_ops(model: &Model) -> Result<Vec<Op>, Refusal> {
    let mut ops = Vec::new();
    for parameter in &model.parameters {
        let Parameter::Scalar(scalar) = parameter else {
            return unsupported(format!(
                "combined parameter '{}' is conditional on AUTH-D3 (ParameterParameterBinding)",
                parameter.id()
            ));
        };
        ops.push(Op::ParamDefine(ParamDefineInput {
            id: scalar.id.clone(),
            class: "imported".to_string(),
            min: f64::from(scalar.min),
            default: f64::from(scalar.default),
            max: f64::from(scalar.max),
        }));
    }
    for part in &model.parts {
        if part.opacity != 1.0 {
            return unsupported(format!(
                "part '{}' opacity {} is not 1",
                part.id, part.opacity
            ));
        }
        ops.push(Op::PartCreate(IdInput {
            id: part.id.clone(),
        }));
    }
    for deformer in &model.deformers {
        ops.push(deformer_create(deformer)?);
    }
    for part in model.parts.iter().filter(|part| part.parent.is_some()) {
        ops.push(Op::PartParentSet(PartParentInput {
            part: part.id.clone(),
            parent: part.parent.clone(),
        }));
    }
    for deformer in model.deformers.iter().filter(|d| d.parent().is_some()) {
        ops.push(Op::DeformerParentSet(DeformerParentInput {
            target: deformer.id().to_string(),
            parent: deformer.parent().map(str::to_string),
        }));
    }
    Ok(ops)
}

fn deformer_create(deformer: &Deformer) -> Result<Op, Refusal> {
    match deformer {
        Deformer::Rotation(rotation) => Ok(Op::DeformerRotationCreate(RotationCreateInput {
            id: rotation.id.clone(),
            origin: [f64::from(rotation.origin[0]), f64::from(rotation.origin[1])],
            base_angle_degrees: f64::from(rotation.base_angle_degrees),
        })),
        Deformer::Warp(warp) => {
            let rest = &warp.rest_positions;
            let (Some(first), Some(last)) = (rest.first(), rest.last()) else {
                return unsupported(format!("warp '{}' has no rest grid", warp.id));
            };
            let input = WarpCreateInput {
                id: warp.id.clone(),
                columns: warp.columns,
                rows: warp.rows,
                bounds: [
                    f64::from(first[0]),
                    f64::from(first[1]),
                    f64::from(last[0]),
                    f64::from(last[1]),
                ],
            };
            let uniform = crate::model::Warp {
                columns: input.columns,
                rows: input.rows,
                bounds: input.bounds,
                keyforms: None,
            }
            .rest_positions();
            let matches = uniform.len() == rest.len()
                && uniform.iter().zip(rest).all(|(want, have)| {
                    (want[0] - f64::from(have[0])).abs() <= WARP_REST_TOLERANCE
                        && (want[1] - f64::from(have[1])).abs() <= WARP_REST_TOLERANCE
                });
            if !matches {
                return unsupported(format!(
                    "warp '{}' rest grid is not uniform over its bounds",
                    warp.id
                ));
            }
            Ok(Op::DeformerWarpCreate(input))
        }
    }
}

fn points(values: &[[f32; 2]]) -> Vec<[f64; 2]> {
    values
        .iter()
        .map(|point| [f64::from(point[0]), f64::from(point[1])])
        .collect()
}

/// Meshes become drawable parts under their model part.
fn mesh_ops(model: &Model, observed: &ModelObservation) -> Result<Vec<Op>, Refusal> {
    let mut ops = Vec::new();
    for mesh in &model.meshes {
        if mesh.blend_mode != BlendMode::Normal {
            return unsupported(format!("mesh '{}' blend mode is not normal", mesh.id));
        }
        if mesh.opacity != 1.0 {
            return unsupported(format!(
                "mesh '{}' opacity {} is not 1",
                mesh.id, mesh.opacity
            ));
        }
        if mesh.clipping_mask_ids.len() > 1 {
            return unsupported(format!(
                "mesh '{}' has more than one clipping mask",
                mesh.id
            ));
        }
        if !(0..=9999).contains(&mesh.draw_order) {
            return unsupported(format!(
                "mesh '{}' draw order {} is outside the band range 0..=9999",
                mesh.id, mesh.draw_order
            ));
        }
        let Some(texture_index) = model.textures.iter().position(|t| t.id == mesh.texture) else {
            unreachable!("the format validator resolved every mesh texture")
        };
        let image = texture_image(&observed.textures[texture_index]);
        let id = mesh.id.clone();
        ops.push(Op::PartCreate(IdInput { id: id.clone() }));
        ops.push(Op::PartParentSet(PartParentInput {
            part: id.clone(),
            parent: Some(mesh.part.clone()),
        }));
        if mesh.deformer.is_some() {
            ops.push(Op::DeformerParentSet(DeformerParentInput {
                target: id.clone(),
                parent: mesh.deformer.clone(),
            }));
        }
        ops.push(Op::PartArtBind(ArtBindInput {
            part: id.clone(),
            asset: mesh.texture.clone(),
            region: RectInput {
                x: 0,
                y: 0,
                width: image.width,
                height: image.height,
            },
        }));
        ops.push(Op::MeshSet(MeshSetInput {
            part: id.clone(),
            vertices: points(&mesh.vertices),
            uvs: points(&mesh.uvs),
            indices: mesh.indices.clone(),
        }));
        // The range check above makes the draw order 0..=9999.
        let order = mesh.draw_order as u32;
        ops.push(Op::PartDrawOrderSet(DrawInput {
            part: id.clone(),
            band: order / 100,
            index: order % 100,
            group: None,
        }));
        if !mesh.rendered {
            ops.push(Op::PartVisibilitySet(VisibilityInput {
                part: id,
                visible: false,
            }));
        }
    }
    for mesh in model
        .meshes
        .iter()
        .filter(|mesh| !mesh.clipping_mask_ids.is_empty())
    {
        ops.push(Op::PartClipSet(ClipInput {
            part: mesh.id.clone(),
            source: mesh.clipping_mask_ids.first().cloned(),
            inverted: false,
        }));
    }
    Ok(ops)
}

/// Lattices become a grid declaration (two axes) plus one
/// keyform_set per form, coordinates resolved from key indices.
fn keyform_ops(model: &Model) -> Result<Vec<Op>, Refusal> {
    let mut ops = Vec::new();
    for deformer in &model.deformers {
        match deformer {
            Deformer::Rotation(rotation) => {
                if let Some(lattice) = &rotation.keyforms {
                    lattice_ops(&rotation.id, lattice, &mut ops, |form| {
                        FormInput::Rotation {
                            angle_degrees: f64::from(form.angle_degrees),
                            scale: [f64::from(form.scale[0]), f64::from(form.scale[1])],
                        }
                    });
                }
            }
            Deformer::Warp(warp) => {
                if let Some(lattice) = &warp.keyforms {
                    lattice_ops(&warp.id, lattice, &mut ops, |form| FormInput::Warp {
                        positions: points(&form.positions),
                    });
                }
            }
        }
    }
    for mesh in &model.meshes {
        if let Some(lattice) = &mesh.keyforms {
            lattice_ops(&mesh.id, lattice, &mut ops, |form| FormInput::Mesh {
                vertices: points(&form.vertices),
            });
        }
    }
    Ok(ops)
}

/// A form type that carries its lattice coordinate.
trait At {
    fn at(&self) -> &[u32];
}

impl At for torogao_format::model::RotationForm {
    fn at(&self) -> &[u32] {
        &self.at
    }
}

impl At for torogao_format::model::WarpForm {
    fn at(&self) -> &[u32] {
        &self.at
    }
}

impl At for torogao_format::model::MeshForm {
    fn at(&self) -> &[u32] {
        &self.at
    }
}

fn lattice_ops<F: At>(
    target: &str,
    lattice: &Lattice<F>,
    ops: &mut Vec<Op>,
    payload: impl Fn(&F) -> FormInput,
) {
    if let [first, second] = &lattice.axes[..] {
        ops.push(Op::ParamGridDefine(GridInput {
            target: target.to_string(),
            axes: [first.clone(), second.clone()],
        }));
    }
    for form in &lattice.forms {
        // The format validator proved every `at` index in range.
        let at = form
            .at()
            .iter()
            .enumerate()
            .map(|(axis, key)| Coordinate {
                parameter: lattice.axes[axis].clone(),
                value: f64::from(lattice.keys[axis][*key as usize]),
            })
            .collect();
        ops.push(Op::KeyformSet(KeyformSetInput {
            target: target.to_string(),
            at,
            form: payload(form),
        }));
    }
}
