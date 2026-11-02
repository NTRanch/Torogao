//! The A2 physics authoring verbs (BRIEF-AUTHORING, physics
//! group; landed by the G-10 completion): `physics_group_define`,
//! `physics_group_remove`, and `physics_forces_set`, through the
//! same `decide()` → apply → op-log path as every other family.
//!
//! The authored records mirror the physics model torogao-format
//! carries (see `model`'s A2 section), so a group exports
//! without translation. `physics_bake` is not here: a bake is
//! the engine's fixed-step evaluation, descoped from this
//! surface by recorded ruling (`op::DESCOPED`).

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::decide::{Verdict, check_id, check_number};
use crate::limits;
use crate::model::{
    AnchorKind, PhysicsAnchor, PhysicsGroup, PhysicsInput, PhysicsOutput, PhysicsParticle,
    PhysicsStrand, Point, Project,
};
use crate::op::Op;
use crate::refusal::{Refusal, RefusalClass, refuse};

// ---- inputs ------------------------------------------------------

/// One physics input row in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsInputInput {
    /// The driving parameter.
    pub parameter: String,
    /// Degrees of rotation at the parameter's maximum.
    pub scale_degrees: f64,
}

/// A strand anchor in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsAnchorInput {
    /// The collection the anchor names: `deformer` or `mesh`.
    pub kind: String,
    /// The deformer or part id.
    pub id: String,
    /// Offset from the anchor's origin.
    pub offset: Point,
}

/// One particle row in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsParticleInput {
    /// Segment length.
    pub length: f64,
    /// Collision radius.
    pub radius: f64,
    /// Mobility (response to forces).
    pub mobility: f64,
    /// Delay (response lag).
    pub delay: f64,
    /// Damping (velocity loss per step).
    pub damping: f64,
}

/// A strand output in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsOutputInput {
    /// The written parameter.
    pub parameter: String,
    /// Degrees-to-value scale.
    pub scale: f64,
    /// Value offset.
    pub offset: f64,
}

/// One strand in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsStrandInput {
    /// Stable id (unique within its group).
    pub id: String,
    /// What the strand's root is pinned to.
    pub anchor: PhysicsAnchorInput,
    /// The strand's particles, root to tip.
    pub particles: Vec<PhysicsParticleInput>,
    /// The parameter the strand's tip angle writes, when any.
    #[serde(default)]
    pub output: Option<PhysicsOutputInput>,
}

/// Inputs of `physics_group_define`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsGroupDefineInput {
    /// Stable id.
    pub id: String,
    /// Gravity vector (default: zero).
    #[serde(default)]
    pub gravity: Option<Point>,
    /// Wind vector (default: zero).
    #[serde(default)]
    pub wind: Option<Point>,
    /// Turbulence amplitude (default: zero).
    #[serde(default)]
    pub turbulence_amplitude: Option<f64>,
    /// Parameter inputs.
    #[serde(default)]
    pub inputs: Vec<PhysicsInputInput>,
    /// Particle strands.
    pub strands: Vec<PhysicsStrandInput>,
}

/// Inputs of `physics_forces_set`: the fields present are the
/// fields set.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsForcesSetInput {
    /// The group to set forces on.
    pub group: String,
    /// Gravity vector.
    #[serde(default)]
    pub gravity: Option<Point>,
    /// Wind vector.
    #[serde(default)]
    pub wind: Option<Point>,
    /// Turbulence amplitude.
    #[serde(default)]
    pub turbulence_amplitude: Option<f64>,
}

// ---- record builders ----------------------------------------------

fn input_record(inputs: &PhysicsInputInput) -> PhysicsInput {
    PhysicsInput {
        parameter: inputs.parameter.clone(),
        scale_degrees: inputs.scale_degrees,
    }
}

fn input_inputs(record: &PhysicsInput) -> PhysicsInputInput {
    PhysicsInputInput {
        parameter: record.parameter.clone(),
        scale_degrees: record.scale_degrees,
    }
}

fn anchor_record(inputs: &PhysicsAnchorInput) -> PhysicsAnchor {
    PhysicsAnchor {
        kind: match inputs.kind.as_str() {
            "mesh" => AnchorKind::Mesh,
            _ => AnchorKind::Deformer,
        },
        id: inputs.id.clone(),
        offset: inputs.offset,
    }
}

fn anchor_inputs(record: &PhysicsAnchor) -> PhysicsAnchorInput {
    PhysicsAnchorInput {
        kind: match record.kind {
            AnchorKind::Deformer => "deformer".to_string(),
            AnchorKind::Mesh => "mesh".to_string(),
        },
        id: record.id.clone(),
        offset: record.offset,
    }
}

fn particle_record(inputs: &PhysicsParticleInput) -> PhysicsParticle {
    PhysicsParticle {
        length: inputs.length,
        radius: inputs.radius,
        mobility: inputs.mobility,
        delay: inputs.delay,
        damping: inputs.damping,
    }
}

fn particle_inputs(record: &PhysicsParticle) -> PhysicsParticleInput {
    PhysicsParticleInput {
        length: record.length,
        radius: record.radius,
        mobility: record.mobility,
        delay: record.delay,
        damping: record.damping,
    }
}

fn output_record(inputs: &PhysicsOutputInput) -> PhysicsOutput {
    PhysicsOutput {
        parameter: inputs.parameter.clone(),
        scale: inputs.scale,
        offset: inputs.offset,
    }
}

fn output_inputs(record: &PhysicsOutput) -> PhysicsOutputInput {
    PhysicsOutputInput {
        parameter: record.parameter.clone(),
        scale: record.scale,
        offset: record.offset,
    }
}

fn strand_record(inputs: &PhysicsStrandInput) -> PhysicsStrand {
    PhysicsStrand {
        id: inputs.id.clone(),
        anchor: anchor_record(&inputs.anchor),
        particles: inputs.particles.iter().map(particle_record).collect(),
        output: inputs.output.as_ref().map(output_record),
    }
}

fn strand_inputs(record: &PhysicsStrand) -> PhysicsStrandInput {
    PhysicsStrandInput {
        id: record.id.clone(),
        anchor: anchor_inputs(&record.anchor),
        particles: record.particles.iter().map(particle_inputs).collect(),
        output: record.output.as_ref().map(output_inputs),
    }
}

/// The canonical group record for these inputs (inputs sorted
/// by parameter, strands sorted by id; strand particle order is
/// authored order — root to tip is semantic).
pub fn group_record(inputs: &PhysicsGroupDefineInput) -> PhysicsGroup {
    let mut rows: Vec<PhysicsInput> = inputs.inputs.iter().map(input_record).collect();
    rows.sort_by(|a, b| a.parameter.cmp(&b.parameter));
    let mut strands: Vec<PhysicsStrand> = inputs.strands.iter().map(strand_record).collect();
    strands.sort_by(|a, b| a.id.cmp(&b.id));
    PhysicsGroup {
        id: inputs.id.clone(),
        gravity: inputs.gravity.unwrap_or([0.0, 0.0]),
        wind: inputs.wind.unwrap_or([0.0, 0.0]),
        turbulence_amplitude: inputs.turbulence_amplitude.unwrap_or(0.0),
        inputs: rows,
        strands,
    }
}

/// The inputs that rebuild this group record (validation sweep).
pub fn group_inputs(record: &PhysicsGroup) -> PhysicsGroupDefineInput {
    PhysicsGroupDefineInput {
        id: record.id.clone(),
        gravity: Some(record.gravity),
        wind: Some(record.wind),
        turbulence_amplitude: Some(record.turbulence_amplitude),
        inputs: record.inputs.iter().map(input_inputs).collect(),
        strands: record.strands.iter().map(strand_inputs).collect(),
    }
}

// ---- decide --------------------------------------------------------

/// Decide one physics op against the project.
pub fn decide_physics(project: &Project, op: &Op) -> Result<Verdict, Refusal> {
    match op {
        Op::PhysicsGroupDefine(inputs) => decide_group_define(project, inputs),
        Op::PhysicsGroupRemove(inputs) => {
            if project.physics_group(&inputs.id).is_none() {
                return refuse(
                    RefusalClass::UnknownTarget,
                    format!("physics group '{}' does not exist", inputs.id),
                );
            }
            Ok(Verdict::Apply)
        }
        Op::PhysicsForcesSet(inputs) => decide_forces_set(project, inputs),
        other => unreachable!("decide_physics called with {}", other.verb()),
    }
}

fn check_point(point: Point, what: &str) -> Result<(), Refusal> {
    check_number(point[0], what)?;
    check_number(point[1], what)
}

fn decide_group_define(
    project: &Project,
    inputs: &PhysicsGroupDefineInput,
) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "id")?;
    if let Some(gravity) = inputs.gravity {
        check_point(gravity, "gravity")?;
    }
    if let Some(wind) = inputs.wind {
        check_point(wind, "wind")?;
    }
    if let Some(amplitude) = inputs.turbulence_amplitude {
        check_number(amplitude, "turbulence_amplitude")?;
    }
    if inputs.inputs.len() > limits::PHYSICS_INPUTS_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "group declares {} inputs, over the bound of {}",
                inputs.inputs.len(),
                limits::PHYSICS_INPUTS_MAX
            ),
        );
    }
    let mut seen = BTreeSet::new();
    for row in &inputs.inputs {
        check_id(&row.parameter, "parameter")?;
        check_number(row.scale_degrees, "scale_degrees")?;
        if !seen.insert(row.parameter.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("group names input parameter '{}' twice", row.parameter),
            );
        }
        if project.parameter(&row.parameter).is_none() {
            return refuse(
                RefusalClass::Dangling,
                format!("input parameter '{}' is not defined", row.parameter),
            );
        }
    }
    if inputs.strands.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            "a physics group needs at least one strand: a group with no strands simulates nothing",
        );
    }
    if inputs.strands.len() > limits::PHYSICS_STRANDS_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "group declares {} strands, over the bound of {}",
                inputs.strands.len(),
                limits::PHYSICS_STRANDS_MAX
            ),
        );
    }
    let mut seen = BTreeSet::new();
    for strand in &inputs.strands {
        check_id(&strand.id, "strand")?;
        if !seen.insert(strand.id.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("group names strand '{}' twice", strand.id),
            );
        }
        decide_strand(project, strand)?;
    }
    let record = group_record(inputs);
    match project.physics_group(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => refuse(
            RefusalClass::Conflict,
            format!(
                "physics group '{}' already exists with different inputs",
                inputs.id
            ),
        ),
        None => {
            if project.physics_groups.len() + 1 > limits::PHYSICS_GROUPS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "project already holds {} physics groups, the bound",
                        limits::PHYSICS_GROUPS_MAX
                    ),
                );
            }
            Ok(Verdict::Apply)
        }
    }
}

fn decide_strand(project: &Project, strand: &PhysicsStrandInput) -> Result<(), Refusal> {
    check_id(&strand.anchor.id, "anchor")?;
    check_point(strand.anchor.offset, "anchor offset")?;
    match strand.anchor.kind.as_str() {
        "deformer" => {
            if project.deformer(&strand.anchor.id).is_none() {
                return refuse(
                    RefusalClass::Dangling,
                    format!("anchor deformer '{}' is not defined", strand.anchor.id),
                );
            }
        }
        "mesh" => {
            if project.part(&strand.anchor.id).is_none() {
                return refuse(
                    RefusalClass::Dangling,
                    format!("anchor part '{}' is not defined", strand.anchor.id),
                );
            }
        }
        other => {
            return refuse(
                RefusalClass::UnknownKind,
                format!("anchor kind '{other}' is not one the family defines"),
            );
        }
    }
    if strand.particles.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            format!("strand '{}' needs at least one particle", strand.id),
        );
    }
    if strand.particles.len() > limits::PHYSICS_PARTICLES_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "strand '{}' declares {} particles, over the bound of {}",
                strand.id,
                strand.particles.len(),
                limits::PHYSICS_PARTICLES_MAX
            ),
        );
    }
    for particle in &strand.particles {
        for (value, what) in [
            (particle.length, "particle length"),
            (particle.radius, "particle radius"),
            (particle.mobility, "particle mobility"),
            (particle.delay, "particle delay"),
            (particle.damping, "particle damping"),
        ] {
            check_number(value, what)?;
        }
        if particle.length <= 0.0 || particle.radius < 0.0 {
            return refuse(
                RefusalClass::Range,
                "a particle's length must be positive and its radius non-negative",
            );
        }
    }
    if let Some(output) = &strand.output {
        check_id(&output.parameter, "parameter")?;
        check_number(output.scale, "output scale")?;
        check_number(output.offset, "output offset")?;
        if project.parameter(&output.parameter).is_none() {
            return refuse(
                RefusalClass::Dangling,
                format!("output parameter '{}' is not defined", output.parameter),
            );
        }
    }
    Ok(())
}

fn decide_forces_set(
    project: &Project,
    inputs: &PhysicsForcesSetInput,
) -> Result<Verdict, Refusal> {
    let Some(group) = project.physics_group(&inputs.group) else {
        return refuse(
            RefusalClass::UnknownTarget,
            format!("physics group '{}' does not exist", inputs.group),
        );
    };
    if inputs.gravity.is_none() && inputs.wind.is_none() && inputs.turbulence_amplitude.is_none() {
        return refuse(
            RefusalClass::Malformed,
            "physics_forces_set names no force to set",
        );
    }
    if let Some(gravity) = inputs.gravity {
        check_point(gravity, "gravity")?;
    }
    if let Some(wind) = inputs.wind {
        check_point(wind, "wind")?;
    }
    if let Some(amplitude) = inputs.turbulence_amplitude {
        check_number(amplitude, "turbulence_amplitude")?;
    }
    let changes = inputs.gravity.is_some_and(|value| value != group.gravity)
        || inputs.wind.is_some_and(|value| value != group.wind)
        || inputs
            .turbulence_amplitude
            .is_some_and(|value| value != group.turbulence_amplitude);
    Ok(if changes {
        Verdict::Apply
    } else {
        Verdict::NoOp
    })
}

// ---- apply -----------------------------------------------------------

/// Apply one decided physics op to the project.
pub fn apply_physics(project: &mut Project, op: &Op) {
    match op {
        Op::PhysicsGroupDefine(inputs) => {
            let record = group_record(inputs);
            let at = project
                .physics_groups
                .partition_point(|group| group.id < record.id);
            if project.physics_groups.get(at).map(|g| &g.id) == Some(&record.id) {
                project.physics_groups[at] = record;
            } else {
                project.physics_groups.insert(at, record);
            }
        }
        Op::PhysicsGroupRemove(inputs) => {
            project.physics_groups.retain(|group| group.id != inputs.id);
        }
        Op::PhysicsForcesSet(inputs) => {
            let Some(group) = project
                .physics_groups
                .iter_mut()
                .find(|group| group.id == inputs.group)
            else {
                return;
            };
            if let Some(gravity) = inputs.gravity {
                group.gravity = gravity;
            }
            if let Some(wind) = inputs.wind {
                group.wind = wind;
            }
            if let Some(amplitude) = inputs.turbulence_amplitude {
                group.turbulence_amplitude = amplitude;
            }
        }
        other => unreachable!("apply_physics called with {}", other.verb()),
    }
}
