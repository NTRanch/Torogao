//! The decide() gate: one pure function of (project, op,
//! observation) → Apply | NoOp | Refusal.
//!
//! Every bound, kind, reference, provenance, and conflict check
//! for a mutating op lives here and nowhere else; `apply` assumes
//! an admitted op and asserts rather than re-checks. Deny is
//! final: no op input, flag, or face can skip this gate (there is
//! no override parameter anywhere in the crate).
//!
//! Upsert law (§Item 3): definition verbs (`*_create`,
//! `*_define`, intake) are no-ops when re-applied with identical
//! defining inputs and `TG-AUTH-CONFLICT` when the inputs differ.
//! Setter verbs (`*_set`, `*_bind`) replace the value they set.

use crate::import::ModelObservation;
use crate::intake::{ImageObservation, PsdObservation, source_is_readonly};
use crate::limits::{
    ASSETS_MAX, COORD_MAGNITUDE_MAX, DEFORMERS_MAX, DRAW_BAND_MAX, DRAW_INDEX_MAX,
    EXPRESSION_ROWS_MAX, EXPRESSIONS_MAX, GLUE_PAIRS_MAX, GLUES_MAX, ID_BYTES_MAX,
    INDICES_PER_MESH_MAX, KEYFORMS_PER_OBJECT_MAX, KEYS_PER_AXIS_MAX, PARAMETERS_MAX, PARTS_MAX,
    PROVENANCE_TEXT_BYTES_MAX, TREE_DEPTH_MAX, TRIANGLE_AREA2_MIN, VARIANT_SETS_MAX,
    VERTICES_PER_MESH_MAX,
};
use crate::model::{
    Blend, CONSENT_SCHEMA, Completion, Consent, Deformer, DeformerKind, Keyforms, Parameter, Part,
    Point, Project, Stamp,
};
use crate::op::{
    BlendInput, ExpressionDefineInput, FormInput, GlueInput, InitInput, IntakeImageInput,
    IntakePsdInput, KeyformSetInput, MeshSetInput, Op, ParamDefineInput,
};
use crate::refusal::{Refusal, RefusalClass, refuse};
use std::path::Path;

/// What prepare observed for ops that read sources.
#[derive(Clone, Debug, Default)]
pub enum Observed {
    /// The op reads no source.
    #[default]
    Nothing,
    /// A PNG source (`intake_image`).
    Image(ImageObservation),
    /// A PSD source (`intake_psd`).
    Psd(PsdObservation),
    /// A rig-model source (`project_import`).
    Model(ModelObservation),
}

/// An admitted op's effect class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// The op changes the project.
    Apply,
    /// The op is admitted and changes nothing (identical
    /// re-definition, or a setter writing the current value).
    NoOp,
}

/// Lanes the header stamp may name (the shipped lane packs).
pub const LANES: [&str; 2] = ["adult", "all-ages"];

/// THE gate. Pure: reads only its arguments.
pub fn decide(state: Option<&Project>, op: &Op, observed: &Observed) -> Result<Verdict, Refusal> {
    if let Op::ProjectInit(inputs) = op {
        return decide_init(state, inputs);
    }
    let Some(project) = state else {
        return refuse(
            RefusalClass::UnknownTarget,
            "no project exists here; run project_init first",
        );
    };
    match op {
        Op::ProjectInit(_) => unreachable!("handled above"),
        Op::ProjectImport(_) => crate::import::decide_import(project, op, observed),
        Op::IntakeImage(inputs) => decide_intake_image(project, inputs, observed),
        Op::IntakePsd(inputs) => decide_intake_psd(project, inputs, observed),
        Op::PartCreate(inputs) => decide_node_create(project, &inputs.id, None),
        Op::PartParentSet(inputs) => {
            decide_part_parent(project, &inputs.part, inputs.parent.as_deref())
        }
        Op::PartRemove(inputs) => decide_part_remove(project, &inputs.part),
        Op::PartVisibilitySet(inputs) => {
            let part = part(project, &inputs.part)?;
            Ok(verdict(part.visible != inputs.visible))
        }
        Op::PartArtBind(inputs) => decide_art_bind(project, inputs),
        Op::PartClipSet(inputs) => decide_clip(
            project,
            &inputs.part,
            inputs.source.as_deref(),
            inputs.inverted,
        ),
        Op::PartDrawOrderSet(inputs) => decide_draw(project, inputs),
        Op::MeshSet(inputs) => decide_mesh_set(project, inputs),
        Op::MeshTrim(inputs) => decide_mesh_trim(project, &inputs.part),
        _ => decide_rig(project, op),
    }
}

/// The deformer, parameter, keyform, and expression verbs.
fn decide_rig(project: &Project, op: &Op) -> Result<Verdict, Refusal> {
    match op {
        Op::DeformerRotationCreate(inputs) => {
            check_point(&inputs.origin, "origin")?;
            check_number(inputs.base_angle_degrees, "base_angle_degrees")?;
            let existing = project
                .deformer(&inputs.id)
                .map(|deformer| match &deformer.kind {
                    DeformerKind::Rotation(rotation) => {
                        rotation.origin == inputs.origin
                            && rotation.base_angle_degrees == inputs.base_angle_degrees
                    }
                    DeformerKind::Warp(_) => false,
                });
            decide_node_create(project, &inputs.id, Some(existing))
        }
        Op::DeformerWarpCreate(inputs) => {
            for side in [inputs.columns, inputs.rows] {
                if !(1..=32).contains(&side) {
                    return refuse(
                        RefusalClass::Range,
                        format!("warp grid side {side} is outside 1..=32"),
                    );
                }
            }
            let [x0, y0, x1, y1] = inputs.bounds;
            for value in inputs.bounds {
                check_number(value, "bounds")?;
            }
            if x0 >= x1 || y0 >= y1 {
                return refuse(
                    RefusalClass::Malformed,
                    "warp bounds need x0 < x1 and y0 < y1",
                );
            }
            let existing = project
                .deformer(&inputs.id)
                .map(|deformer| match &deformer.kind {
                    DeformerKind::Warp(warp) => {
                        warp.columns == inputs.columns
                            && warp.rows == inputs.rows
                            && warp.bounds == inputs.bounds
                    }
                    DeformerKind::Rotation(_) => false,
                });
            decide_node_create(project, &inputs.id, Some(existing))
        }
        Op::DeformerParentSet(inputs) => {
            decide_deformer_parent(project, &inputs.target, inputs.parent.as_deref())
        }
        Op::DeformerRemove(inputs) => decide_deformer_remove(project, &inputs.deformer),
        Op::DeformerGlueSet(inputs) => decide_glue(project, inputs),
        Op::ParamDefine(inputs) => decide_param_define(project, inputs),
        Op::ParamGridDefine(inputs) => decide_grid(project, &inputs.target, &inputs.axes),
        Op::ParamPoseDefaultSet(inputs) => {
            let parameter = parameter(project, &inputs.parameter)?;
            check_in_range(parameter, inputs.value, "pose value")?;
            Ok(verdict(project.neutral_value(parameter) != inputs.value))
        }
        Op::ParamRemove(inputs) => decide_param_remove(project, &inputs.parameter),
        Op::KeyformSet(inputs) => decide_keyform(project, inputs),
        Op::ExpressionDefine(inputs) => decide_expression(project, inputs),
        Op::ExpressionRemove(inputs) => {
            if project.expression(&inputs.id).is_none() {
                return unknown_target("expression", &inputs.id);
            }
            Ok(Verdict::Apply)
        }
        other => unreachable!("decide routes {} before decide_rig", other.verb()),
    }
}

fn verdict(changes: bool) -> Verdict {
    if changes {
        Verdict::Apply
    } else {
        Verdict::NoOp
    }
}

// ---- shared checks ------------------------------------------------

/// Checks id syntax: 1..=64 bytes of `[A-Za-z0-9_-]` (the model
/// format's id rule, so every authored id exports).
pub fn check_id(id: &str, what: &str) -> Result<(), Refusal> {
    let valid = !id.is_empty()
        && id.len() <= ID_BYTES_MAX
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if valid {
        return Ok(());
    }
    refuse(
        RefusalClass::Malformed,
        format!(
            "{what} '{}' is not a valid id ([A-Za-z0-9_-], 1..=64 bytes)",
            bounded(id)
        ),
    )
}

fn bounded(text: &str) -> &str {
    let mut cut = text.len().min(ID_BYTES_MAX * 2);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

/// A finite number within the model's coordinate magnitude.
pub fn check_number(value: f64, what: &str) -> Result<(), Refusal> {
    if !value.is_finite() {
        return refuse(RefusalClass::Malformed, format!("{what} is not finite"));
    }
    if value.abs() > f64::from(COORD_MAGNITUDE_MAX) {
        return refuse(
            RefusalClass::Range,
            format!("{what} {value} exceeds the magnitude bound {COORD_MAGNITUDE_MAX}"),
        );
    }
    Ok(())
}

fn check_point(point: &Point, what: &str) -> Result<(), Refusal> {
    check_number(point[0], what)?;
    check_number(point[1], what)
}

fn check_points(points: &[Point], what: &str) -> Result<(), Refusal> {
    points.iter().try_for_each(|point| check_point(point, what))
}

/// Free text: non-empty, bounded, no control characters.
fn check_text(text: &str, what: &str, class: RefusalClass) -> Result<(), Refusal> {
    if text.trim().is_empty() {
        return refuse(class, format!("{what} is missing or empty"));
    }
    if text.len() > PROVENANCE_TEXT_BYTES_MAX || text.chars().any(char::is_control) {
        return refuse(
            class,
            format!("{what} exceeds {PROVENANCE_TEXT_BYTES_MAX} bytes or has control characters"),
        );
    }
    Ok(())
}

fn check_in_range(parameter: &Parameter, value: f64, what: &str) -> Result<(), Refusal> {
    if !value.is_finite() {
        return refuse(RefusalClass::Malformed, format!("{what} is not finite"));
    }
    if value < parameter.min || value > parameter.max {
        return refuse(
            RefusalClass::Range,
            format!(
                "{what} {value} is outside parameter '{}' range [{}, {}]",
                parameter.id, parameter.min, parameter.max
            ),
        );
    }
    Ok(())
}

fn check_count(count: usize, max: usize, what: &str) -> Result<(), Refusal> {
    if count > max {
        return refuse(
            RefusalClass::Range,
            format!("{what} would number {count}; the bound is {max}"),
        );
    }
    Ok(())
}

fn unknown_target<T>(what: &str, id: &str) -> Result<T, Refusal> {
    refuse(
        RefusalClass::UnknownTarget,
        format!("{what} '{}' does not exist", bounded(id)),
    )
}

fn dangling<T>(what: &str, id: &str) -> Result<T, Refusal> {
    refuse(
        RefusalClass::Dangling,
        format!("{what} '{}' does not exist", bounded(id)),
    )
}

fn conflict<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::Conflict, message)
}

fn part<'a>(project: &'a Project, id: &str) -> Result<&'a Part, Refusal> {
    project
        .part(id)
        .map_or_else(|| unknown_target("part", id), Ok)
}

fn parameter<'a>(project: &'a Project, id: &str) -> Result<&'a Parameter, Refusal> {
    project
        .parameter(id)
        .map_or_else(|| unknown_target("parameter", id), Ok)
}

fn check_sha_pin(pinned: Option<&String>, observed: &str) -> Result<(), Refusal> {
    match pinned {
        Some(pin) if pin != observed => refuse(
            RefusalClass::Provenance,
            format!(
                "source sha256 {observed} does not match the pinned {}",
                bounded(pin)
            ),
        ),
        _ => Ok(()),
    }
}

fn check_source(canonical: &str, pinned: Option<&String>, sha256: &str) -> Result<(), Refusal> {
    if source_is_readonly(Path::new(canonical)) {
        return refuse(
            RefusalClass::ReadonlySource,
            "source lies in read-only evidence; studied material never enters a project",
        );
    }
    check_sha_pin(pinned, sha256)
}

/// Header stamp and consent checks, shared with validation.
pub fn check_header(name: &str, stamp: &Stamp, consent: &Consent) -> Result<(), Refusal> {
    check_id(name, "project name")?;
    check_text(&stamp.basis, "stamp basis", RefusalClass::Stamp)?;
    check_text(&stamp.pack, "stamp pack", RefusalClass::Stamp)?;
    check_text(&stamp.rating, "stamp rating", RefusalClass::Stamp)?;
    if !LANES.contains(&stamp.lane.as_str()) {
        return refuse(
            RefusalClass::Stamp,
            format!(
                "stamp lane '{}' is not one of {LANES:?}",
                bounded(&stamp.lane)
            ),
        );
    }
    if consent.schema != CONSENT_SCHEMA {
        return refuse(
            RefusalClass::Stamp,
            format!("consent schema is not '{CONSENT_SCHEMA}'"),
        );
    }
    if !consent.adult || !consent.wants_selected_scenes {
        return refuse(
            RefusalClass::Stamp,
            "consent fields 'adult' and 'wants_selected_scenes' must both be true",
        );
    }
    Ok(())
}

// ---- project ------------------------------------------------------

fn decide_init(state: Option<&Project>, inputs: &InitInput) -> Result<Verdict, Refusal> {
    check_header(&inputs.name, &inputs.stamp, &inputs.consent)?;
    match state {
        None => Ok(Verdict::Apply),
        Some(project) => {
            let header = &project.header;
            if header.name == inputs.name
                && header.stamp == inputs.stamp
                && header.consent == inputs.consent
            {
                Ok(Verdict::NoOp)
            } else {
                conflict("a project with a different header already exists here")
            }
        }
    }
}

// ---- intake -------------------------------------------------------

fn decide_intake_image(
    project: &Project,
    inputs: &IntakeImageInput,
    observed: &Observed,
) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "asset id")?;
    check_text(&inputs.license, "license", RefusalClass::Provenance)?;
    check_text(&inputs.origin, "origin", RefusalClass::Provenance)?;
    let Observed::Image(image) = observed else {
        return refuse(
            RefusalClass::Malformed,
            "intake_image was not given its source",
        );
    };
    check_source(
        &image.canonical_path,
        inputs.source_sha256.as_ref(),
        &image.sha256,
    )?;
    match project.asset(&inputs.id) {
        Some(asset)
            if asset.sha256 == image.sha256
                && asset.provenance.license == inputs.license
                && asset.provenance.origin == inputs.origin =>
        {
            Ok(Verdict::NoOp)
        }
        Some(_) => conflict(format!(
            "asset '{}' already exists with different content",
            inputs.id
        )),
        None => {
            check_count(project.assets.len() + 1, ASSETS_MAX, "assets")?;
            Ok(Verdict::Apply)
        }
    }
}

fn decide_intake_psd(
    project: &Project,
    inputs: &IntakePsdInput,
    observed: &Observed,
) -> Result<Verdict, Refusal> {
    check_text(&inputs.license, "license", RefusalClass::Provenance)?;
    check_text(&inputs.origin, "origin", RefusalClass::Provenance)?;
    let Observed::Psd(psd) = observed else {
        return refuse(
            RefusalClass::Malformed,
            "intake_psd was not given its source",
        );
    };
    check_source(
        &psd.canonical_path,
        inputs.source_sha256.as_ref(),
        &psd.sha256,
    )?;
    check_layer_declarations(inputs, psd)?;
    check_variant_sets(project, inputs)?;
    let existing = inputs
        .layers
        .iter()
        .filter(|decl| project.part(&decl.part).is_some() || project.deformer(&decl.part).is_some())
        .count();
    if existing == 0 {
        check_count(
            project.parts.len() + inputs.layers.len(),
            PARTS_MAX,
            "parts",
        )?;
        let pixel_layers = psd
            .layers
            .iter()
            .filter(|layer| layer.image.is_some())
            .count();
        check_count(project.assets.len() + pixel_layers, ASSETS_MAX, "assets")?;
        return Ok(Verdict::Apply);
    }
    let identical = existing == inputs.layers.len()
        && psd.layers.iter().all(|layer| {
            let Some(decl) = inputs.layers.iter().find(|decl| decl.layer == layer.path) else {
                return false;
            };
            match &layer.image {
                Some(image) => project
                    .asset(&decl.part)
                    .is_some_and(|asset| asset.sha256 == image.sha256),
                None => project.part(&decl.part).is_some(),
            }
        });
    if identical {
        Ok(Verdict::NoOp)
    } else {
        conflict("intake_psd would redefine parts or assets that already exist differently")
    }
}

/// Every observed layer declared exactly once, every declaration
/// naming a real layer, completion and clip bases matching the
/// source (the separation standard made mechanical).
fn check_layer_declarations(inputs: &IntakePsdInput, psd: &PsdObservation) -> Result<(), Refusal> {
    let mut parts: Vec<&str> = Vec::with_capacity(inputs.layers.len());
    for decl in &inputs.layers {
        check_id(&decl.part, "part id")?;
        if inputs
            .layers
            .iter()
            .filter(|other| other.layer == decl.layer)
            .count()
            > 1
        {
            return refuse(
                RefusalClass::Malformed,
                format!("layer '{}' is declared twice", bounded(&decl.layer)),
            );
        }
        if parts.contains(&decl.part.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("part id '{}' is declared twice", decl.part),
            );
        }
        parts.push(&decl.part);
        let Some(layer) = psd.layers.iter().find(|layer| layer.path == decl.layer) else {
            return dangling("PSD layer", &decl.layer);
        };
        match (layer.is_group, decl.completion) {
            (true, Some(_)) => {
                return refuse(
                    RefusalClass::Malformed,
                    format!("group '{}' cannot declare completion painting", decl.layer),
                );
            }
            (false, None) => {
                return refuse(
                    RefusalClass::Provenance,
                    format!(
                        "layer '{}' does not declare its completion painting",
                        decl.layer
                    ),
                );
            }
            _ => {}
        }
        let expected_base = layer.clip_base.as_ref().and_then(|base| {
            inputs
                .layers
                .iter()
                .find(|other| &other.layer == base)
                .map(|other| other.part.as_str())
        });
        if decl.clip_base.as_deref() != expected_base {
            return refuse(
                RefusalClass::Provenance,
                format!(
                    "layer '{}' declares clip base {:?}; the source says {:?}",
                    decl.layer, decl.clip_base, expected_base
                ),
            );
        }
    }
    if let Some(layer) = psd
        .layers
        .iter()
        .find(|layer| !inputs.layers.iter().any(|decl| decl.layer == layer.path))
    {
        return refuse(
            RefusalClass::Provenance,
            format!(
                "PSD layer '{}' has no separation declaration",
                bounded(&layer.path)
            ),
        );
    }
    Ok(())
}

fn check_variant_sets(project: &Project, inputs: &IntakePsdInput) -> Result<(), Refusal> {
    let mut sets: Vec<&str> = Vec::new();
    for decl in &inputs.layers {
        let Some(set) = decl.variant_set.as_deref() else {
            continue;
        };
        check_id(set, "variant set id")?;
        if !sets.contains(&set) {
            sets.push(set);
        }
    }
    for set in &sets {
        let members = inputs
            .layers
            .iter()
            .filter(|decl| decl.variant_set.as_deref() == Some(*set))
            .count();
        if members < 2 {
            return refuse(
                RefusalClass::Malformed,
                format!("variant set '{set}' needs at least two layers"),
            );
        }
        if project
            .variant_sets
            .iter()
            .any(|existing| existing.id == *set)
            && !inputs
                .layers
                .iter()
                .any(|decl| project.part(&decl.part).is_some())
        {
            return conflict(format!("variant set '{set}' already exists"));
        }
    }
    check_count(
        project.variant_sets.len() + sets.len(),
        VARIANT_SETS_MAX,
        "variant sets",
    )
}

// ---- parts --------------------------------------------------------

/// Node creation (parts and deformers share one id space).
/// `existing_match` is None for parts (identity is the id alone),
/// or Some(whether an existing deformer matches the inputs).
fn decide_node_create(
    project: &Project,
    id: &str,
    existing_match: Option<Option<bool>>,
) -> Result<Verdict, Refusal> {
    check_id(id, "id")?;
    let is_part = project.part(id).is_some();
    let is_deformer = project.deformer(id).is_some();
    match existing_match {
        None if is_part => return Ok(Verdict::NoOp),
        None if is_deformer => return conflict(format!("id '{id}' is already a deformer")),
        Some(Some(true)) => return Ok(Verdict::NoOp),
        Some(Some(false)) => {
            return conflict(format!(
                "deformer '{id}' already exists with different inputs"
            ));
        }
        Some(None) if is_part => return conflict(format!("id '{id}' is already a part")),
        _ => {}
    }
    match existing_match {
        None => check_count(project.parts.len() + 1, PARTS_MAX, "parts")?,
        Some(_) => check_count(project.deformers.len() + 1, DEFORMERS_MAX, "deformers")?,
    }
    Ok(Verdict::Apply)
}

/// Walks a parent chain from `start`, bounded by the collection
/// size; true when it reaches `target`.
fn chain_reaches(
    start: Option<&str>,
    target: &str,
    bound: usize,
    parent_of: impl Fn(&str) -> Option<String>,
) -> (bool, usize) {
    let mut current = start.map(str::to_string);
    let mut depth = 0;
    while let Some(id) = current {
        if id == target {
            return (true, depth);
        }
        depth += 1;
        if depth > bound {
            // A pre-existing cycle that does not pass through
            // `target`; decide never admits one, so this is a
            // hand-edited file. Report it as reaching.
            return (true, depth);
        }
        current = parent_of(&id);
    }
    (false, depth)
}

fn decide_part_parent(
    project: &Project,
    id: &str,
    parent: Option<&str>,
) -> Result<Verdict, Refusal> {
    let current = part(project, id)?;
    if let Some(parent_id) = parent {
        if project.part(parent_id).is_none() {
            return dangling("parent part", parent_id);
        }
        let (reaches, depth) = chain_reaches(Some(parent_id), id, project.parts.len(), |node| {
            project.part(node).and_then(|p| p.parent.clone())
        });
        if reaches {
            return conflict(format!(
                "parenting '{id}' under '{parent_id}' would create a cycle"
            ));
        }
        if depth + 1 > TREE_DEPTH_MAX {
            return refuse(
                RefusalClass::Range,
                format!("part tree depth would exceed {TREE_DEPTH_MAX}"),
            );
        }
    }
    Ok(verdict(current.parent.as_deref() != parent))
}

fn decide_part_remove(project: &Project, id: &str) -> Result<Verdict, Refusal> {
    part(project, id)?;
    let referenced_by = project
        .parts
        .iter()
        .find(|other| {
            other.parent.as_deref() == Some(id)
                || other.clip.as_ref().is_some_and(|clip| clip.source == id)
        })
        .map(|other| format!("part '{}'", other.id))
        .or_else(|| {
            project
                .glues
                .iter()
                .find(|glue| glue.a == id || glue.b == id)
                .map(|glue| format!("glue '{}'", glue.id))
        })
        .or_else(|| {
            project
                .expressions
                .iter()
                .find(|expression| expression.visibility.iter().any(|row| row.part == id))
                .map(|expression| format!("expression '{}'", expression.id))
        })
        .or_else(|| {
            project
                .variant_sets
                .iter()
                .find(|set| set.variants.iter().any(|member| member == id))
                .map(|set| format!("variant set '{}'", set.id))
        });
    match referenced_by {
        Some(by) => conflict(format!("part '{id}' is still referenced by {by}")),
        None => Ok(Verdict::Apply),
    }
}

fn decide_art_bind(
    project: &Project,
    inputs: &crate::op::ArtBindInput,
) -> Result<Verdict, Refusal> {
    let current = part(project, &inputs.part)?;
    let Some(asset) = project.asset(&inputs.asset) else {
        return dangling("asset", &inputs.asset);
    };
    let region = inputs.region;
    if region.width == 0 || region.height == 0 {
        return refuse(
            RefusalClass::Malformed,
            "art region needs a non-zero width and height",
        );
    }
    let fits = region
        .x
        .checked_add(region.width)
        .is_some_and(|end| end <= asset.width)
        && region
            .y
            .checked_add(region.height)
            .is_some_and(|end| end <= asset.height);
    if !fits {
        return refuse(
            RefusalClass::Range,
            format!(
                "art region lies outside asset '{}' ({}x{})",
                asset.id, asset.width, asset.height
            ),
        );
    }
    let unchanged = current.art.as_ref().is_some_and(|art| {
        art.asset == inputs.asset
            && (
                art.region.x,
                art.region.y,
                art.region.width,
                art.region.height,
            ) == (region.x, region.y, region.width, region.height)
    });
    Ok(verdict(!unchanged))
}

fn decide_clip(
    project: &Project,
    id: &str,
    source: Option<&str>,
    inverted: bool,
) -> Result<Verdict, Refusal> {
    let current = part(project, id)?;
    if let Some(source_id) = source {
        if project.part(source_id).is_none() {
            return dangling("clip source part", source_id);
        }
        let (reaches, _) = chain_reaches(Some(source_id), id, project.parts.len(), |node| {
            project
                .part(node)
                .and_then(|p| p.clip.as_ref().map(|clip| clip.source.clone()))
        });
        if reaches {
            return conflict(format!(
                "clipping '{id}' to '{source_id}' would create a mask cycle"
            ));
        }
    }
    let unchanged = match (&current.clip, source) {
        (None, None) => true,
        (Some(clip), Some(source_id)) => clip.source == source_id && clip.inverted == inverted,
        _ => false,
    };
    Ok(verdict(!unchanged))
}

fn decide_draw(project: &Project, inputs: &crate::op::DrawInput) -> Result<Verdict, Refusal> {
    let current = part(project, &inputs.part)?;
    if inputs.band > DRAW_BAND_MAX || inputs.index > DRAW_INDEX_MAX {
        return refuse(
            RefusalClass::Range,
            format!("draw band/index must be within 0..={DRAW_BAND_MAX} / 0..={DRAW_INDEX_MAX}"),
        );
    }
    if let Some(group) = &inputs.group {
        check_id(group, "render group")?;
    }
    let unchanged = current.draw.as_ref().is_some_and(|draw| {
        draw.band == inputs.band && draw.index == inputs.index && draw.group == inputs.group
    });
    Ok(verdict(!unchanged))
}

/// Mesh law: shape, bounds, index range, no degenerate triangles,
/// and no keyform or glue left pointing at vertices that would no
/// longer exist.
pub fn check_mesh(vertices: &[Point], uvs: &[Point], indices: &[u32]) -> Result<(), Refusal> {
    if vertices.len() < 3 {
        return refuse(RefusalClass::Malformed, "a mesh needs at least 3 vertices");
    }
    check_count(vertices.len(), VERTICES_PER_MESH_MAX, "mesh vertices")?;
    check_count(indices.len(), INDICES_PER_MESH_MAX, "mesh indices")?;
    if uvs.len() != vertices.len() {
        return refuse(
            RefusalClass::Malformed,
            "a mesh needs exactly one UV per vertex",
        );
    }
    if indices.len() < 3 || indices.len() % 3 != 0 {
        return refuse(
            RefusalClass::Malformed,
            "mesh indices must be whole triangles",
        );
    }
    check_points(vertices, "vertex")?;
    for uv in uvs {
        if !uv[0].is_finite() || !uv[1].is_finite() {
            return refuse(RefusalClass::Malformed, "a UV is not finite");
        }
    }
    for (triangle, corners) in indices.chunks_exact(3).enumerate() {
        let mut points = [[0.0; 2]; 3];
        for (slot, index) in points.iter_mut().zip(corners) {
            match vertices.get(*index as usize) {
                Some(point) => *slot = *point,
                None => {
                    return refuse(
                        RefusalClass::Range,
                        format!("triangle {triangle} index {index} is out of range"),
                    );
                }
            }
        }
        let [a, b, c] = points;
        let area2 = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs();
        let repeated =
            corners[0] == corners[1] || corners[1] == corners[2] || corners[0] == corners[2];
        if repeated || area2 < TRIANGLE_AREA2_MIN {
            return refuse(
                RefusalClass::Malformed,
                format!("triangle {triangle} is degenerate"),
            );
        }
    }
    Ok(())
}

fn decide_mesh_set(project: &Project, inputs: &MeshSetInput) -> Result<Verdict, Refusal> {
    let current = part(project, &inputs.part)?;
    check_mesh(&inputs.vertices, &inputs.uvs, &inputs.indices)?;
    check_vertex_users(project, current, inputs.vertices.len())?;
    let unchanged = current.mesh.as_ref().is_some_and(|mesh| {
        mesh.vertices == inputs.vertices && mesh.uvs == inputs.uvs && mesh.indices == inputs.indices
    });
    Ok(verdict(!unchanged))
}

/// Keyforms and glues bound to a part's vertices must survive a
/// geometry change.
fn check_vertex_users(project: &Project, part: &Part, vertex_count: usize) -> Result<(), Refusal> {
    if let Some(keyforms) = &part.keyforms
        && keyforms
            .forms
            .iter()
            .any(|form| form.form.vertices.len() != vertex_count)
    {
        return conflict(format!(
            "part '{}' has keyforms for a different vertex count; remove the part to re-mesh",
            part.id
        ));
    }
    for glue in &project.glues {
        let side = if glue.a == part.id {
            Some(0)
        } else if glue.b == part.id {
            Some(1)
        } else {
            None
        };
        if let Some(side) = side
            && glue
                .pairs
                .iter()
                .any(|pair| pair[side] as usize >= vertex_count)
        {
            return conflict(format!(
                "glue '{}' references vertices the new mesh lacks",
                glue.id
            ));
        }
    }
    Ok(())
}

/// The quad `mesh_trim` produces: (vertices, uvs). Shared with
/// apply so the geometry is computed in one place.
pub fn trim_quad(project: &Project, part: &Part) -> Result<(Vec<Point>, Vec<Point>), Refusal> {
    let Some(art) = &part.art else {
        return refuse(
            RefusalClass::Dangling,
            format!("part '{}' has no art binding to trim to", part.id),
        );
    };
    let Some(asset) = project.asset(&art.asset) else {
        return dangling("asset", &art.asset);
    };
    let Some(content) = asset.content else {
        return refuse(
            RefusalClass::UnknownKind,
            format!(
                "asset '{}' (kind png) carries no decoded content bounds; PNG pixel decode is \
                 not in this slice",
                asset.id
            ),
        );
    };
    let region = art.region;
    let x0 = region.x.max(content.x);
    let y0 = region.y.max(content.y);
    let x1 = (region.x + region.width).min(content.x + content.width);
    let y1 = (region.y + region.height).min(content.y + content.height);
    if x0 >= x1 || y0 >= y1 {
        return refuse(
            RefusalClass::Range,
            format!("part '{}' art region holds no painted content", part.id),
        );
    }
    let [ox, oy] = asset.canvas_origin.unwrap_or([0, 0]);
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    let vertices = corners
        .iter()
        .map(|(x, y)| [(ox + i64::from(*x)) as f64, (oy + i64::from(*y)) as f64])
        .collect();
    let uvs = corners
        .iter()
        .map(|(x, y)| {
            [
                f64::from(*x) / f64::from(asset.width),
                f64::from(*y) / f64::from(asset.height),
            ]
        })
        .collect();
    Ok((vertices, uvs))
}

/// The triangle list of a trimmed quad.
pub const TRIM_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];

fn decide_mesh_trim(project: &Project, id: &str) -> Result<Verdict, Refusal> {
    let current = part(project, id)?;
    let (vertices, uvs) = trim_quad(project, current)?;
    check_mesh(&vertices, &uvs, &TRIM_INDICES)?;
    check_vertex_users(project, current, vertices.len())?;
    let unchanged = current.mesh.as_ref().is_some_and(|mesh| {
        mesh.vertices == vertices && mesh.uvs == uvs && mesh.indices == TRIM_INDICES
    });
    Ok(verdict(!unchanged))
}

// ---- deformers ----------------------------------------------------

fn decide_deformer_parent(
    project: &Project,
    target: &str,
    parent: Option<&str>,
) -> Result<Verdict, Refusal> {
    if let Some(parent_id) = parent
        && project.deformer(parent_id).is_none()
    {
        return dangling("parent deformer", parent_id);
    }
    if let Some(part) = project.part(target) {
        return Ok(verdict(part.deformer.as_deref() != parent));
    }
    let Some(deformer) = project.deformer(target) else {
        return unknown_target("part or deformer", target);
    };
    if let Some(parent_id) = parent {
        let (reaches, depth) =
            chain_reaches(Some(parent_id), target, project.deformers.len(), |node| {
                project.deformer(node).and_then(|d| d.parent.clone())
            });
        if reaches {
            return conflict(format!(
                "parenting deformer '{target}' under '{parent_id}' would create a cycle"
            ));
        }
        if depth + 1 > TREE_DEPTH_MAX {
            return refuse(
                RefusalClass::Range,
                format!("deformer chain depth would exceed {TREE_DEPTH_MAX}"),
            );
        }
    }
    Ok(verdict(deformer.parent.as_deref() != parent))
}

fn decide_deformer_remove(project: &Project, id: &str) -> Result<Verdict, Refusal> {
    if project.deformer(id).is_none() {
        return unknown_target("deformer", id);
    }
    if let Some(child) = project
        .deformers
        .iter()
        .find(|d| d.parent.as_deref() == Some(id))
    {
        return conflict(format!(
            "deformer '{id}' still parents deformer '{}'",
            child.id
        ));
    }
    if let Some(part) = project
        .parts
        .iter()
        .find(|p| p.deformer.as_deref() == Some(id))
    {
        return conflict(format!("deformer '{id}' still deforms part '{}'", part.id));
    }
    Ok(Verdict::Apply)
}

fn decide_glue(project: &Project, inputs: &GlueInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "glue id")?;
    if inputs.a == inputs.b {
        return refuse(RefusalClass::Malformed, "a glue joins two different parts");
    }
    let mut counts = [0usize; 2];
    for (slot, id) in [&inputs.a, &inputs.b].into_iter().enumerate() {
        let Some(part) = project.part(id) else {
            return dangling("glued part", id);
        };
        let Some(mesh) = &part.mesh else {
            return refuse(
                RefusalClass::Dangling,
                format!("glued part '{id}' has no mesh"),
            );
        };
        counts[slot] = mesh.vertices.len();
    }
    if inputs.pairs.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            "a glue needs at least one vertex pair",
        );
    }
    check_count(inputs.pairs.len(), GLUE_PAIRS_MAX, "glue pairs")?;
    for pair in &inputs.pairs {
        if pair[0] as usize >= counts[0] || pair[1] as usize >= counts[1] {
            return refuse(
                RefusalClass::Range,
                format!("glue pair ({}, {}) is outside the meshes", pair[0], pair[1]),
            );
        }
    }
    if !inputs.weight.is_finite() || !(0.0..=1.0).contains(&inputs.weight) {
        return refuse(RefusalClass::Range, "glue weight must be within 0..=1");
    }
    match project.glue(&inputs.id) {
        Some(glue) => Ok(verdict(
            !(glue.a == inputs.a
                && glue.b == inputs.b
                && glue.pairs == inputs.pairs
                && glue.weight == inputs.weight),
        )),
        None => {
            check_count(project.glues.len() + 1, GLUES_MAX, "glues")?;
            Ok(Verdict::Apply)
        }
    }
}

// ---- parameters ---------------------------------------------------

fn decide_param_define(project: &Project, inputs: &ParamDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "parameter id")?;
    check_id(&inputs.class, "parameter class")?;
    for (value, what) in [
        (inputs.min, "min"),
        (inputs.default, "default"),
        (inputs.max, "max"),
    ] {
        check_number(value, what)?;
    }
    if inputs.min >= inputs.max {
        return refuse(RefusalClass::Malformed, "parameter min must be below max");
    }
    if inputs.default < inputs.min || inputs.default > inputs.max {
        return refuse(
            RefusalClass::Range,
            "parameter default lies outside [min, max]",
        );
    }
    match project.parameter(&inputs.id) {
        Some(existing) => {
            let same = existing.class == inputs.class
                && existing.min == inputs.min
                && existing.default == inputs.default
                && existing.max == inputs.max;
            if same {
                Ok(Verdict::NoOp)
            } else {
                conflict(format!(
                    "parameter '{}' already exists with different inputs",
                    inputs.id
                ))
            }
        }
        None => {
            check_count(project.parameters.len() + 1, PARAMETERS_MAX, "parameters")?;
            Ok(Verdict::Apply)
        }
    }
}

fn decide_param_remove(project: &Project, id: &str) -> Result<Verdict, Refusal> {
    parameter(project, id)?;
    let in_axes = |axes: Option<&[String]>| axes.is_some_and(|axes| axes.iter().any(|a| a == id));
    let user = project
        .parts
        .iter()
        .find(|part| in_axes(part.keyforms.as_ref().map(|k| &k.axes[..])))
        .map(|part| format!("part '{}' keyforms", part.id))
        .or_else(|| {
            project
                .deformers
                .iter()
                .find(|deformer| in_axes(deformer.axes()))
                .map(|deformer| format!("deformer '{}' keyforms", deformer.id))
        })
        .or_else(|| {
            project
                .expressions
                .iter()
                .find(|e| e.values.iter().any(|row| row.parameter == id))
                .map(|e| format!("expression '{}'", e.id))
        })
        .or_else(|| {
            project
                .pose
                .iter()
                .find(|entry| entry.parameter == id)
                .map(|_| "the neutral pose".to_string())
        });
    match user {
        Some(by) => conflict(format!("parameter '{id}' is still referenced by {by}")),
        None => Ok(Verdict::Apply),
    }
}

/// The current lattice axes of a keyform target, and whether the
/// target is a meshed part (Some(vertex count)) or a deformer.
fn target_axes<'a>(project: &'a Project, target: &str) -> Result<Option<&'a [String]>, Refusal> {
    if let Some(part) = project.part(target) {
        return Ok(part.keyforms.as_ref().map(|k| &k.axes[..]));
    }
    match project.deformer(target) {
        Some(deformer) => Ok(deformer.axes()),
        None => unknown_target("keyform target", target),
    }
}

fn decide_grid(project: &Project, target: &str, axes: &[String; 2]) -> Result<Verdict, Refusal> {
    let current = target_axes(project, target)?;
    if let Some(part) = project.part(target)
        && part.mesh.is_none()
    {
        return refuse(
            RefusalClass::Dangling,
            format!("part '{target}' has no mesh to key"),
        );
    }
    for axis in axes {
        if project.parameter(axis).is_none() {
            return dangling("grid axis parameter", axis);
        }
    }
    if axes[0] == axes[1] {
        return refuse(
            RefusalClass::Malformed,
            "a grid needs two different parameters",
        );
    }
    match current {
        None => Ok(Verdict::Apply),
        Some(existing) if existing == axes => Ok(Verdict::NoOp),
        Some(existing) => conflict(format!(
            "target '{target}' is already keyed on {existing:?}; a grid cannot replace it"
        )),
    }
}

/// The current lattice of a keyform target: axes and the sorted
/// coordinates of its forms.
fn lattice_of(project: &Project, target: &str) -> (Option<Vec<String>>, Vec<Vec<f64>>) {
    fn split<F>(keyforms: Option<&Keyforms<F>>) -> (Option<Vec<String>>, Vec<Vec<f64>>) {
        match keyforms {
            None => (None, Vec::new()),
            Some(k) => (
                Some(k.axes.clone()),
                k.forms.iter().map(|f| f.at.clone()).collect(),
            ),
        }
    }
    if let Some(part) = project.part(target) {
        return split(part.keyforms.as_ref());
    }
    match project.deformer(target).map(|d| &d.kind) {
        Some(DeformerKind::Rotation(rotation)) => split(rotation.keyforms.as_ref()),
        Some(DeformerKind::Warp(warp)) => split(warp.keyforms.as_ref()),
        None => (None, Vec::new()),
    }
}

fn decide_keyform(project: &Project, inputs: &KeyformSetInput) -> Result<Verdict, Refusal> {
    target_axes(project, &inputs.target)?;
    check_keyform_payload(project, &inputs.target, &inputs.form)?;
    if inputs.at.is_empty() || inputs.at.len() > 2 {
        return refuse(
            RefusalClass::Malformed,
            "a keyform coordinate has one or two axes",
        );
    }
    if inputs.at.len() == 2 && inputs.at[0].parameter == inputs.at[1].parameter {
        return refuse(RefusalClass::Malformed, "keyform axes must differ");
    }
    for coordinate in &inputs.at {
        let Some(param) = project.parameter(&coordinate.parameter) else {
            return dangling("keyform axis parameter", &coordinate.parameter);
        };
        check_in_range(param, coordinate.value, "keyform coordinate")?;
    }
    let (axes, coordinates) = lattice_of(project, &inputs.target);
    let requested: Vec<&str> = inputs.at.iter().map(|c| c.parameter.as_str()).collect();
    match &axes {
        None if requested.len() == 2 => {
            return conflict(format!(
                "target '{}' has no grid; declare one with param_grid_define first",
                inputs.target
            ));
        }
        Some(axes)
            if axes
                .iter()
                .map(String::as_str)
                .ne(requested.iter().copied()) =>
        {
            return conflict(format!(
                "target '{}' is keyed on {axes:?}, not {requested:?}",
                inputs.target
            ));
        }
        _ => {}
    }
    let at: Vec<f64> = inputs.at.iter().map(|c| c.value).collect();
    let exists = coordinates.contains(&at);
    if !exists {
        check_count(coordinates.len() + 1, KEYFORMS_PER_OBJECT_MAX, "keyforms")?;
        for axis in 0..at.len() {
            let mut keys: Vec<f64> = coordinates.iter().map(|c| c[axis]).collect();
            keys.push(at[axis]);
            keys.sort_by(f64::total_cmp);
            keys.dedup();
            check_count(keys.len(), KEYS_PER_AXIS_MAX, "keys on one axis")?;
        }
    }
    Ok(verdict(!exists || !keyform_unchanged(project, inputs, &at)))
}

fn check_keyform_payload(project: &Project, target: &str, form: &FormInput) -> Result<(), Refusal> {
    match (
        project.part(target),
        project.deformer(target).map(|d| &d.kind),
        form,
    ) {
        (Some(part), _, FormInput::Mesh { vertices }) => {
            let Some(mesh) = &part.mesh else {
                return refuse(
                    RefusalClass::Dangling,
                    format!("part '{target}' has no mesh to key"),
                );
            };
            if vertices.len() != mesh.vertices.len() {
                return refuse(
                    RefusalClass::Malformed,
                    format!("mesh keyform needs {} vertices", mesh.vertices.len()),
                );
            }
            check_points(vertices, "keyform vertex")
        }
        (
            None,
            Some(DeformerKind::Rotation(_)),
            FormInput::Rotation {
                angle_degrees,
                scale,
            },
        ) => {
            check_number(*angle_degrees, "angle_degrees")?;
            check_point(scale, "scale")?;
            if scale[0] <= 0.0 || scale[1] <= 0.0 {
                return refuse(RefusalClass::Range, "rotation keyform scale must be > 0");
            }
            Ok(())
        }
        (None, Some(DeformerKind::Warp(warp)), FormInput::Warp { positions }) => {
            if positions.len() != warp.point_count() {
                return refuse(
                    RefusalClass::Malformed,
                    format!("warp keyform needs {} positions", warp.point_count()),
                );
            }
            check_points(positions, "keyform position")
        }
        _ => refuse(
            RefusalClass::Malformed,
            format!("keyform payload kind does not match target '{target}'"),
        ),
    }
}

fn keyform_unchanged(project: &Project, inputs: &KeyformSetInput, at: &[f64]) -> bool {
    let target = inputs.target.as_str();
    match &inputs.form {
        FormInput::Mesh { vertices } => project
            .part(target)
            .and_then(|part| part.keyforms.as_ref())
            .and_then(|k| k.forms.iter().find(|f| f.at == at))
            .is_some_and(|f| f.form.vertices == *vertices),
        FormInput::Rotation {
            angle_degrees,
            scale,
        } => match project.deformer(target).map(|d| &d.kind) {
            Some(DeformerKind::Rotation(rotation)) => rotation
                .keyforms
                .as_ref()
                .and_then(|k| k.forms.iter().find(|f| f.at == at))
                .is_some_and(|f| f.form.angle_degrees == *angle_degrees && f.form.scale == *scale),
            _ => false,
        },
        FormInput::Warp { positions } => match project.deformer(target).map(|d| &d.kind) {
            Some(DeformerKind::Warp(warp)) => warp
                .keyforms
                .as_ref()
                .and_then(|k| k.forms.iter().find(|f| f.at == at))
                .is_some_and(|f| f.form.positions == *positions),
            _ => false,
        },
    }
}

// ---- expressions --------------------------------------------------

fn decide_expression(
    project: &Project,
    inputs: &ExpressionDefineInput,
) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "expression id")?;
    let rows = inputs.values.len() + inputs.visibility.len();
    if rows == 0 {
        return refuse(
            RefusalClass::Malformed,
            "an expression needs at least one row",
        );
    }
    check_count(rows, EXPRESSION_ROWS_MAX, "expression rows")?;
    for (index, row) in inputs.values.iter().enumerate() {
        if inputs.values[..index]
            .iter()
            .any(|other| other.parameter == row.parameter)
        {
            return refuse(
                RefusalClass::Malformed,
                format!("expression row for '{}' appears twice", row.parameter),
            );
        }
        let Some(param) = project.parameter(&row.parameter) else {
            return dangling("expression parameter", &row.parameter);
        };
        match inputs.blend {
            BlendInput::Override => check_in_range(param, row.value, "override value")?,
            BlendInput::Additive => {
                let span = param.max - param.min;
                if !row.value.is_finite() || row.value.abs() > span {
                    return refuse(
                        RefusalClass::Range,
                        format!(
                            "additive delta for '{}' exceeds the range span {span}",
                            param.id
                        ),
                    );
                }
            }
        }
    }
    for (index, row) in inputs.visibility.iter().enumerate() {
        if inputs.visibility[..index]
            .iter()
            .any(|other| other.part == row.part)
        {
            return refuse(
                RefusalClass::Malformed,
                format!("visibility row for '{}' appears twice", row.part),
            );
        }
        if project.part(&row.part).is_none() {
            return dangling("expression part", &row.part);
        }
    }
    match project.expression(&inputs.id) {
        Some(existing) => {
            if *existing == crate::apply::expression_record(inputs) {
                Ok(Verdict::NoOp)
            } else {
                conflict(format!(
                    "expression '{}' already exists with different rows",
                    inputs.id
                ))
            }
        }
        None => {
            check_count(
                project.expressions.len() + 1,
                EXPRESSIONS_MAX,
                "expressions",
            )?;
            Ok(Verdict::Apply)
        }
    }
}

/// Maps the input blend to the model blend.
pub fn blend_of(input: BlendInput) -> Blend {
    match input {
        BlendInput::Additive => Blend::Additive,
        BlendInput::Override => Blend::Override,
    }
}

/// Maps the input completion to the model completion.
pub fn completion_of(input: crate::op::CompletionInput) -> Completion {
    match input {
        crate::op::CompletionInput::Painted => Completion::Painted,
        crate::op::CompletionInput::NotNeeded => Completion::NotNeeded,
    }
}

/// Deformer lookup for callers outside the gate.
pub fn deformer_kind_name(deformer: &Deformer) -> &'static str {
    deformer.kind_name()
}
