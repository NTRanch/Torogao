//! The A3 motion authoring verbs (BRIEF-AUTHORING, motion
//! group; landed by the G-10 completion): `motion_create`,
//! `curve_set`, and `curve_from_samples`, through the same
//! `decide()` → apply → op-log path as every other family.
//!
//! A curve's keyframe rows are the clip data the engine's
//! animation lanes evaluate (torogao-engine's `AnimationLane`
//! shape: keyframes ascending by tick). `curve_from_samples`
//! stores measured samples verbatim as keyframes — sample
//! ticks reproduce exactly — and cites their source in the
//! curve's provenance. `motion_bake` is not here: baking is
//! the engine's per-tick evaluation, descoped from this
//! surface by recorded ruling (`op::DESCOPED`).

use serde::Deserialize;

use crate::decide::{Verdict, check_id};
use crate::limits;
use crate::model::{Motion, MotionCurve, MotionKeyframe, Project};
use crate::op::Op;
use crate::refusal::{Refusal, RefusalClass, refuse};

// ---- inputs ------------------------------------------------------

/// Inputs of `motion_create`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct MotionCreateInput {
    /// Stable id.
    pub id: String,
    /// Clip length in ticks.
    pub length_ticks: u32,
    /// Whether the clip loops.
    #[serde(default)]
    pub looped: bool,
}

/// One keyframe row in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeyframeInput {
    /// The tick (0-based, inside the clip's length).
    pub tick: u32,
    /// The parameter's value at that tick.
    pub value: f64,
}

/// Inputs of `curve_set`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurveSetInput {
    /// The motion holding the curve.
    pub motion: String,
    /// The driven parameter.
    pub parameter: String,
    /// Keyframe rows, strictly ascending by tick.
    pub keyframes: Vec<KeyframeInput>,
}

/// Inputs of `curve_from_samples`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurveFromSamplesInput {
    /// The motion holding the curve.
    pub motion: String,
    /// The driven parameter.
    pub parameter: String,
    /// Measured samples, strictly ascending by tick; they
    /// become the curve's keyframes verbatim.
    pub samples: Vec<KeyframeInput>,
    /// The cited source of the samples (the motion's
    /// provenance record).
    pub source: String,
}

// ---- record builders ----------------------------------------------

fn keyframe_record(inputs: &KeyframeInput) -> MotionKeyframe {
    MotionKeyframe {
        tick: inputs.tick,
        value: inputs.value,
    }
}

fn keyframe_inputs(record: &MotionKeyframe) -> KeyframeInput {
    KeyframeInput {
        tick: record.tick,
        value: record.value,
    }
}

/// The canonical curve record for these rows.
pub fn curve_record(
    parameter: &str,
    rows: &[KeyframeInput],
    source: Option<String>,
) -> MotionCurve {
    MotionCurve {
        parameter: parameter.to_string(),
        keyframes: rows.iter().map(keyframe_record).collect(),
        source,
    }
}

/// The inputs that rebuild this curve (validation sweep):
/// sampled curves rebuild through `curve_from_samples` so
/// their citation survives.
pub enum CurveRebuild {
    /// An authored curve (`curve_set`).
    Set(CurveSetInput),
    /// A sampled curve (`curve_from_samples`).
    FromSamples(CurveFromSamplesInput),
}

/// The rebuild op inputs for one curve of one motion.
pub fn curve_rebuild(motion: &str, curve: &MotionCurve) -> CurveRebuild {
    let rows: Vec<KeyframeInput> = curve.keyframes.iter().map(keyframe_inputs).collect();
    match &curve.source {
        Some(source) => CurveRebuild::FromSamples(CurveFromSamplesInput {
            motion: motion.to_string(),
            parameter: curve.parameter.clone(),
            samples: rows,
            source: source.clone(),
        }),
        None => CurveRebuild::Set(CurveSetInput {
            motion: motion.to_string(),
            parameter: curve.parameter.clone(),
            keyframes: rows,
        }),
    }
}

/// The canonical motion record for these inputs (no curves;
/// curves arrive by their own verbs).
pub fn motion_record(inputs: &MotionCreateInput) -> Motion {
    Motion {
        id: inputs.id.clone(),
        length_ticks: inputs.length_ticks,
        looped: inputs.looped,
        curves: Vec::new(),
    }
}

/// The inputs that rebuild this motion's header (sweep).
pub fn motion_inputs(record: &Motion) -> MotionCreateInput {
    MotionCreateInput {
        id: record.id.clone(),
        length_ticks: record.length_ticks,
        looped: record.looped,
    }
}

// ---- decide --------------------------------------------------------

/// Decide one motion op against the project.
pub fn decide_motion(project: &Project, op: &Op) -> Result<Verdict, Refusal> {
    match op {
        Op::MotionCreate(inputs) => decide_create(project, inputs),
        Op::CurveSet(inputs) => decide_curve(
            project,
            &inputs.motion,
            &inputs.parameter,
            &inputs.keyframes,
            None,
        ),
        Op::CurveFromSamples(inputs) => {
            if inputs.source.is_empty()
                || inputs.source.len() > limits::PROVENANCE_TEXT_BYTES_MAX
                || inputs.source.chars().any(char::is_control)
            {
                return refuse(
                    RefusalClass::Malformed,
                    "curve_from_samples needs a usable source citation: samples without provenance are not evidence",
                );
            }
            decide_curve(
                project,
                &inputs.motion,
                &inputs.parameter,
                &inputs.samples,
                Some(inputs.source.clone()),
            )
        }
        other => unreachable!("decide_motion called with {}", other.verb()),
    }
}

fn decide_create(project: &Project, inputs: &MotionCreateInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "id")?;
    if inputs.length_ticks == 0 || inputs.length_ticks > limits::TICKS_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "motion length {} ticks is outside 1..={}",
                inputs.length_ticks,
                limits::TICKS_MAX
            ),
        );
    }
    match project.motion(&inputs.id) {
        Some(existing)
            if existing.length_ticks == inputs.length_ticks && existing.looped == inputs.looped =>
        {
            Ok(Verdict::NoOp)
        }
        Some(_) => refuse(
            RefusalClass::Conflict,
            format!(
                "motion '{}' already exists with a different length or loop flag",
                inputs.id
            ),
        ),
        None => {
            if project.motions.len() + 1 > limits::MOTIONS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "project already holds {} motions, the bound",
                        limits::MOTIONS_MAX
                    ),
                );
            }
            Ok(Verdict::Apply)
        }
    }
}

fn decide_curve(
    project: &Project,
    motion_id: &str,
    parameter_id: &str,
    rows: &[KeyframeInput],
    source: Option<String>,
) -> Result<Verdict, Refusal> {
    check_id(parameter_id, "parameter")?;
    let Some(motion) = project.motion(motion_id) else {
        return refuse(
            RefusalClass::UnknownTarget,
            format!("motion '{motion_id}' does not exist"),
        );
    };
    let Some(parameter) = project.parameter(parameter_id) else {
        return refuse(
            RefusalClass::Dangling,
            format!("parameter '{parameter_id}' is not defined"),
        );
    };
    if rows.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            "a curve needs at least one keyframe row",
        );
    }
    if rows.len() > limits::KEYFRAMES_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "curve declares {} keyframes, over the bound of {}",
                rows.len(),
                limits::KEYFRAMES_MAX
            ),
        );
    }
    let mut previous: Option<u32> = None;
    for row in rows {
        if let Some(before) = previous
            && row.tick <= before
        {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "keyframe ticks must strictly ascend (tick {} follows {before})",
                    row.tick
                ),
            );
        }
        previous = Some(row.tick);
        if row.tick >= motion.length_ticks {
            return refuse(
                RefusalClass::Range,
                format!(
                    "keyframe tick {} is outside the motion's length {}",
                    row.tick, motion.length_ticks
                ),
            );
        }
        if !row.value.is_finite() {
            return refuse(RefusalClass::Malformed, "keyframe value is not finite");
        }
        if row.value < parameter.min || row.value > parameter.max {
            return refuse(
                RefusalClass::Range,
                format!(
                    "keyframe value {} lies outside parameter '{}' range {}..={}",
                    row.value, parameter_id, parameter.min, parameter.max
                ),
            );
        }
    }
    let record = curve_record(parameter_id, rows, source);
    match motion
        .curves
        .iter()
        .find(|curve| curve.parameter == parameter_id)
    {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        _ => Ok(Verdict::Apply),
    }
}

// ---- apply -----------------------------------------------------------

/// Apply one decided motion op to the project.
pub fn apply_motion(project: &mut Project, op: &Op) {
    match op {
        Op::MotionCreate(inputs) => {
            let record = motion_record(inputs);
            let at = project.motions.partition_point(|m| m.id < record.id);
            if project.motions.get(at).map(|m| &m.id) == Some(&record.id) {
                // A create that decides Apply on an existing id
                // cannot occur (decide conflicts); keep the
                // curves if it ever does.
                let curves = std::mem::take(&mut project.motions[at].curves);
                project.motions[at] = Motion { curves, ..record };
            } else {
                project.motions.insert(at, record);
            }
        }
        Op::CurveSet(inputs) => {
            set_curve(
                project,
                &inputs.motion,
                curve_record(&inputs.parameter, &inputs.keyframes, None),
            );
        }
        Op::CurveFromSamples(inputs) => {
            set_curve(
                project,
                &inputs.motion,
                curve_record(
                    &inputs.parameter,
                    &inputs.samples,
                    Some(inputs.source.clone()),
                ),
            );
        }
        other => unreachable!("apply_motion called with {}", other.verb()),
    }
}

fn set_curve(project: &mut Project, motion_id: &str, record: MotionCurve) {
    let Some(motion) = project.motions.iter_mut().find(|m| m.id == motion_id) else {
        return;
    };
    let at = motion
        .curves
        .partition_point(|curve| curve.parameter < record.parameter);
    if motion.curves.get(at).map(|c| &c.parameter) == Some(&record.parameter) {
        motion.curves[at] = record;
    } else {
        motion.curves.insert(at, record);
    }
}
