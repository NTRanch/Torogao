//! The Prima → Alma two-body acceptance scene (D-847 staging, as
//! proven on the D-859 full rigs), reimplemented in the Torogao
//! lane: a 12 s timeline at 60 fps — approach, grasp, crouch, lift,
//! hold with a 26-degree turn, set-down, release, step-back.
//!
//! The figures are evaluated separately (their own evaluators,
//! physics live) and staged in a shared scene space at the
//! acceptance pack's offsets (Alma at dx 100, Prima approaching
//! from dx -90 and stepping back to -70). The keyframe poses are
//! choreographed pairs (the turn is the records' own ParamTurn
//! channel pair): Prima's hand anchors and Alma's waist anchors
//! coincide at the staged offset by authoring, and the contact
//! (C1) is MEASURED, never solved — hand centroids against the
//! STATIC waist band (Alma's rest Body AABB, 62–78% height band),
//! the D-859 acceptance instrument.
//!
//! Measurements (the estate's two-body criteria, TWOBODY-CRITERIA):
//! C1 slip per engaged step; C2 signed margin between the placed
//! meshes (2D signed triangle distance: positive separation,
//! negative penetration estimate, instrument defined in
//! `tri_signed_distance`); C4 Prima's root dip and Alma's foot
//! clearance against her rest ground line.

use std::ops::{Add, Mul, Sub};

use crate::Puppet;
use crate::error::RigError;
use crate::eval::Evaluator;
use crate::math::Vec2;
use crate::model::NodeKind;
use crate::render::Placement;

/// Fixed step rate of the scene protocol.
pub const SCENE_FPS: f32 = 60.0;
/// Total steps: 12 s at 60 fps.
pub const SCENE_STEPS: usize = 720;
/// Engaged window start (seconds).
pub const ENGAGE_START: f32 = 2.0;
/// Engaged window end (seconds).
pub const ENGAGE_END: f32 = 10.0;
/// Alma's home staging offset (the acceptance pack's engaged dx).
const ALMA_HOME: Vec2 = Vec2::new(100.0, 0.0);

/// One keyframe: time, Prima pose, Alma pose. Pose values are
/// parameter x-components by full parameter name; unlisted
/// parameters sit at their record defaults.
struct Keyframe {
    t: f32,
    prima: &'static [(&'static str, f32)],
    alma: &'static [(&'static str, f32)],
}

const GRASP_PRIMA: &[(&str, f32)] = &[
    ("ParamArmR", 0.9),
    ("ParamElbowR", 0.2),
    ("ParamArmL", 0.25),
    ("ParamElbowL", 0.5),
];
const CROUCH_PRIMA: &[(&str, f32)] = &[
    ("ParamArmR", 1.0),
    ("ParamElbowR", 0.25),
    ("ParamArmL", 0.3),
    ("ParamElbowL", 0.55),
    ("ParamCrouch", 0.8),
];
const LIFT_PRIMA: &[(&str, f32)] = &[
    ("ParamArmR", 1.0),
    ("ParamElbowR", 0.25),
    ("ParamArmL", 0.8),
    ("ParamElbowL", 0.6),
    ("ParamCrouch", 0.25),
];
const SETDOWN_PRIMA: &[(&str, f32)] = &[
    ("ParamArmR", 1.0),
    ("ParamElbowR", 0.25),
    ("ParamArmL", 0.3),
    ("ParamElbowL", 0.55),
    ("ParamCrouch", 0.6),
];
const CROUCH_ALMA: &[(&str, f32)] = &[("ParamKneeL", 0.15), ("ParamKneeR", 0.15)];
const LIFT_ALMA: &[(&str, f32)] = &[
    ("ParamRootY", 0.55),
    ("ParamKneeL", 0.7),
    ("ParamKneeR", 0.7),
    ("ParamArmL", -0.2),
    ("ParamArmR", -0.2),
];
const SETDOWN_ALMA: &[(&str, f32)] = &[
    ("ParamRootY", 0.05),
    ("ParamKneeL", 0.2),
    ("ParamKneeR", 0.2),
];
const TURN_PRIMA: &[(&str, f32)] = &[
    ("ParamArmR", 1.0),
    ("ParamElbowR", 0.25),
    ("ParamArmL", 0.8),
    ("ParamElbowL", 0.6),
    ("ParamCrouch", 0.25),
    ("ParamTurn", 0.9),
];
const TURN_ALMA: &[(&str, f32)] = &[
    ("ParamRootY", 0.55),
    ("ParamKneeL", 0.7),
    ("ParamKneeR", 0.7),
    ("ParamArmL", -0.2),
    ("ParamArmR", -0.2),
    ("ParamTurn", 0.9),
];

const KEYFRAMES: &[Keyframe] = &[
    Keyframe {
        t: 0.0,
        prima: &[],
        alma: &[],
    },
    Keyframe {
        t: 2.0,
        prima: GRASP_PRIMA,
        alma: &[],
    },
    Keyframe {
        t: 3.0,
        prima: CROUCH_PRIMA,
        alma: CROUCH_ALMA,
    },
    Keyframe {
        t: 4.0,
        prima: LIFT_PRIMA,
        alma: LIFT_ALMA,
    },
    Keyframe {
        t: 6.5,
        prima: LIFT_PRIMA,
        alma: LIFT_ALMA,
    },
    Keyframe {
        t: 8.0,
        prima: TURN_PRIMA,
        alma: TURN_ALMA,
    },
    Keyframe {
        t: 9.5,
        prima: SETDOWN_PRIMA,
        alma: SETDOWN_ALMA,
    },
    Keyframe {
        t: 10.5,
        prima: SETDOWN_PRIMA,
        alma: SETDOWN_ALMA,
    },
    Keyframe {
        t: 11.0,
        prima: &[],
        alma: &[],
    },
    Keyframe {
        t: 12.0,
        prima: &[],
        alma: &[],
    },
];

/// One step's measurements.
#[derive(Debug, Clone)]
pub struct StepRecord {
    /// Step index (0..720).
    pub step: usize,
    /// Time in seconds.
    pub t: f32,
    /// Whether the contact window is engaged at this step.
    pub engaged: bool,
    /// C1 residual at Prima's left hand (model units).
    pub slip_left: f32,
    /// C1 residual at Prima's right hand (model units).
    pub slip_right: f32,
    /// C1 acceptance instrument: distance from Prima's left hand
    /// centroid to Alma's placed waist band rectangle (0 inside).
    pub band_left: f32,
    /// Same for the right hand.
    pub band_right: f32,
    /// C2 signed margin between the placed meshes (model units).
    pub c2_margin: f32,
    /// The part pair (Prima's, Alma's) achieving the C2 margin.
    pub c2_pair: (String, String),
    /// Prima's root (SkirtFull centroid) scene y.
    pub prima_root_y: f32,
    /// Alma's foot clearance over her rest ground line (model
    /// units; positive is airborne in the y-down stage).
    pub alma_clearance: f32,
}

/// Aggregated scene verdicts.
#[derive(Debug, Clone, Copy)]
pub struct SceneSummary {
    /// Prima's rest height in model units.
    pub prima_height: f32,
    /// Alma's rest height in model units.
    pub alma_height: f32,
    /// C1 per-step band distance percentiles over the engaged
    /// window for the GRIPPING hand (Prima's right hand — the
    /// D-859 acceptance instrument: hand centroid to the static
    /// waist band rectangle, 0 inside).
    pub c1_p50: f32,
    /// C1 p95.
    pub c1_p95: f32,
    /// C1 max.
    pub c1_max: f32,
    /// C1 stricter anchor instrument: p95 gripping-hand distance
    /// to the band's right edge midpoint over the engaged window.
    pub c1_anchor_p95: f32,
    /// C1 anchor instrument max.
    pub c1_anchor_max: f32,
    /// The support (left) hand's band distance p95: the authored
    /// lift is one-handed, so this hand never reaches the band;
    /// reported for completeness, not part of the C1 verdict.
    pub c1_support_p95: f32,
    /// Support hand band distance max.
    pub c1_support_max: f32,
    /// Fraction of engaged steps with gripping-hand band
    /// distance <= 0.5.
    pub contact_fraction: f32,
    /// C2 minimum over the engaged window.
    pub c2_engaged_min: f32,
    /// C2 minimum over the whole scene.
    pub c2_scene_min: f32,
    /// Step of the whole-scene C2 minimum.
    pub c2_scene_min_step: usize,
    /// C4: Prima's largest root dip in the crouch window, units.
    pub c4_dip_units: f32,
    /// C4 dip as a percentage of Prima's height.
    pub c4_dip_pct_height: f32,
    /// C4: Prima's root y at hold (t = 8) minus rest, units.
    pub c4_hold_offset: f32,
    /// Alma's clearance at hold (step 300, t = 5).
    pub clearance_hold: f32,
    /// Alma's minimum clearance across the lift/hold window
    /// (steps 240..600).
    pub clearance_lift_min: f32,
}

/// What a still-frame callback sees at one step.
pub struct SceneStepView<'a> {
    /// Step index.
    pub step: usize,
    /// Prima's evaluator at this step's evaluation.
    pub prima: &'a Evaluator,
    /// Alma's evaluator at this step's evaluation.
    pub alma: &'a Evaluator,
    /// Prima's scene placement.
    pub prima_placement: Placement,
    /// Alma's scene placement.
    pub alma_placement: Placement,
}

/// Rest-state references the summary is computed against, captured
/// before the first step is driven.
#[derive(Debug, Clone, Copy)]
pub struct SceneContext {
    /// Prima's rest height in model units.
    pub prima_height: f32,
    /// Alma's rest height in model units.
    pub alma_height: f32,
    /// Prima's SkirtFull centroid y at rest staging.
    pub prima_root_rest_y: f32,
}

/// Run the full scene. `on_step` fires after each step's
/// evaluation and measurement (the gate driver renders stills
/// there). Returns the per-step records and the rest context.
pub fn run_scene(
    prima: Puppet,
    alma: Puppet,
    on_step: &mut dyn FnMut(&SceneStepView<'_>),
) -> Result<(Vec<StepRecord>, SceneContext), RigError> {
    let mut prima_ev = Evaluator::new(prima);
    let mut alma_ev = Evaluator::new(alma);
    let context = SceneContext {
        prima_height: figure_height(&prima_ev),
        alma_height: figure_height(&alma_ev),
        prima_root_rest_y: part_centroid(&prima_ev, "SkirtFull")
            .ok_or_else(|| RigError::new("TRG-SCENE-001", "Prima has no SkirtFull part"))?
            .y,
    };
    let ground_y = rest_foot_bottom(&alma_ev);
    let rest_band = waist_band(&alma_ev)?;
    let mut records = Vec::with_capacity(SCENE_STEPS);
    for step in 0..SCENE_STEPS {
        let t = step as f32 / SCENE_FPS;
        drive_figure(&mut prima_ev, pose_at(t, true), step)?;
        drive_figure(&mut alma_ev, pose_at(t, false), step)?;
        let engaged = (ENGAGE_START..=ENGAGE_END).contains(&t);
        let hand_left = part_centroid(&prima_ev, "HandL")
            .ok_or_else(|| RigError::new("TRG-SCENE-001", "Prima has no HandL part"))?;
        let hand_right = part_centroid(&prima_ev, "HandR")
            .ok_or_else(|| RigError::new("TRG-SCENE-001", "Prima has no HandR part"))?;
        let prima_placement = Placement {
            rotation: 0.0,
            translation: Vec2::new(prima_offset_x(t), 0.0),
        };
        let alma_placement = Placement {
            rotation: 0.0,
            translation: ALMA_HOME,
        };
        let target_left = prima_placement.apply(hand_left);
        let target_right = prima_placement.apply(hand_right);
        // The acceptance band is static (rest geometry), staged
        // with Alma; the anchor points are its edge midpoints.
        let band = [
            rest_band[0] + ALMA_HOME.x,
            rest_band[1] + ALMA_HOME.y,
            rest_band[2] + ALMA_HOME.x,
            rest_band[3] + ALMA_HOME.y,
        ];
        let band_mid_y = (band[1] + band[3]) * 0.5;
        let waist_left = Vec2::new(band[0], band_mid_y);
        let waist_right = Vec2::new(band[2], band_mid_y);
        let slip_left = waist_left.distance(target_left);
        let slip_right = waist_right.distance(target_right);
        let band_left = point_rect_distance(target_left, band);
        let band_right = point_rect_distance(target_right, band);
        let (c2_margin, c2_pair) =
            scene_margin(&prima_ev, prima_placement, &alma_ev, alma_placement);
        let prima_root_y = part_centroid(&prima_ev, "SkirtFull")
            .map(|c| c.y)
            .unwrap_or(0.0);
        let foot_bottom = placed_foot_bottom(&alma_ev, alma_placement);
        let record = StepRecord {
            step,
            t,
            engaged,
            slip_left,
            slip_right,
            band_left,
            band_right,
            c2_margin,
            c2_pair,
            prima_root_y,
            alma_clearance: ground_y - foot_bottom,
        };
        records.push(record);
        on_step(&SceneStepView {
            step,
            prima: &prima_ev,
            alma: &alma_ev,
            prima_placement,
            alma_placement,
        });
    }
    Ok((records, context))
}

/// Aggregate the verdict table from per-step records.
pub fn summarize(records: &[StepRecord], context: &SceneContext) -> SceneSummary {
    let prima_height = context.prima_height;
    let alma_height = context.alma_height;
    let prima_root_rest = context.prima_root_rest_y;
    let mut engaged_slips: Vec<f32> = records
        .iter()
        .filter(|r| r.engaged)
        .map(|r| r.band_right)
        .collect();
    engaged_slips.sort_by(|a, b| a.total_cmp(b));
    let percentile_of = |sorted: &[f32], p: f32| -> f32 {
        if sorted.is_empty() {
            return 0.0;
        }
        let index = ((sorted.len() - 1) as f32 * p).round() as usize;
        sorted[index]
    };
    let percentile = |p: f32| percentile_of(&engaged_slips, p);
    let mut anchor_slips: Vec<f32> = records
        .iter()
        .filter(|r| r.engaged)
        .map(|r| r.slip_right)
        .collect();
    anchor_slips.sort_by(|a, b| a.total_cmp(b));
    let mut support_slips: Vec<f32> = records
        .iter()
        .filter(|r| r.engaged)
        .map(|r| r.band_left)
        .collect();
    support_slips.sort_by(|a, b| a.total_cmp(b));
    let contact = engaged_slips.iter().filter(|&&s| s <= 0.5).count();
    let c2_engaged_min = records
        .iter()
        .filter(|r| r.engaged)
        .map(|r| r.c2_margin)
        .fold(f32::MAX, f32::min);
    let (c2_scene_min, c2_scene_min_step) =
        records
            .iter()
            .map(|r| (r.c2_margin, r.step))
            .fold(
                (f32::MAX, 0usize),
                |acc, x| if x.0 < acc.0 { x } else { acc },
            );
    let dip = records
        .iter()
        .filter(|r| r.t >= 2.5 && r.t <= 4.5)
        .map(|r| r.prima_root_y - prima_root_rest)
        .fold(0.0f32, f32::max);
    let hold = records.get(480);
    let lift_window: Vec<&StepRecord> = records
        .iter()
        .filter(|r| r.step >= 240 && r.step < 600)
        .collect();
    SceneSummary {
        prima_height,
        alma_height,
        c1_p50: percentile(0.50),
        c1_p95: percentile(0.95),
        c1_max: engaged_slips.last().copied().unwrap_or(0.0),
        c1_anchor_p95: percentile_of(&anchor_slips, 0.95),
        c1_anchor_max: anchor_slips.last().copied().unwrap_or(0.0),
        c1_support_p95: percentile_of(&support_slips, 0.95),
        c1_support_max: support_slips.last().copied().unwrap_or(0.0),
        contact_fraction: if engaged_slips.is_empty() {
            0.0
        } else {
            contact as f32 / engaged_slips.len() as f32
        },
        c2_engaged_min,
        c2_scene_min,
        c2_scene_min_step,
        c4_dip_units: dip,
        c4_dip_pct_height: if prima_height > 0.0 {
            dip / prima_height * 100.0
        } else {
            0.0
        },
        c4_hold_offset: hold.map_or(0.0, |r| r.prima_root_y - prima_root_rest),
        clearance_hold: records.get(300).map_or(0.0, |r| r.alma_clearance),
        clearance_lift_min: lift_window
            .iter()
            .map(|r| r.alma_clearance)
            .fold(f32::MAX, f32::min),
    }
}

/// Pose (full param name → x value) at time t for one figure,
/// smoothstep-blended between keyframes.
fn pose_at(t: f32, prima: bool) -> Vec<(String, f32)> {
    let mut names: Vec<&str> = Vec::new();
    for key in KEYFRAMES {
        for (name, _) in if prima { key.prima } else { key.alma } {
            if !names.contains(name) {
                names.push(name);
            }
        }
    }
    let value_in = |key: &Keyframe, name: &str| -> f32 {
        let pose = if prima { key.prima } else { key.alma };
        pose.iter()
            .find(|(n, _)| *n == name)
            .map_or(0.0, |(_, v)| *v)
    };
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let value = if t <= KEYFRAMES[0].t {
            value_in(&KEYFRAMES[0], name)
        } else {
            let mut v = value_in(&KEYFRAMES[KEYFRAMES.len() - 1], name);
            for pair in KEYFRAMES.windows(2) {
                let (a, b) = (&pair[0], &pair[1]);
                if t >= a.t && t <= b.t {
                    let u = smoothstep((t - a.t) / (b.t - a.t));
                    v = value_in(a, name) + (value_in(b, name) - value_in(a, name)) * u;
                    break;
                }
            }
            v
        };
        out.push((name.to_owned(), value));
    }
    out
}

/// Set one figure's pose for this step and evaluate one fixed step.
/// The first frame of a run primes physics with a zero step so the
/// state matches a fresh evaluator's rest evaluation.
fn drive_figure(
    ev: &mut Evaluator,
    pose: Vec<(String, f32)>,
    _step: usize,
) -> Result<(), RigError> {
    ev.begin_frame();
    for (name, value) in &pose {
        ev.set_param(name, Vec2::new(*value, 0.0))?;
    }
    ev.end_frame(1.0 / SCENE_FPS);
    Ok(())
}

/// Prima's staging offset: approach from -90 (dx 190) to 0 across
/// [0, 2], and step back to -70 (dx 170) across [11, 12].
fn prima_offset_x(t: f32) -> f32 {
    if t < ENGAGE_START {
        -90.0 * (1.0 - smoothstep(t / ENGAGE_START))
    } else if t > 11.0 {
        -70.0 * smoothstep((t - 11.0) / 1.0)
    } else {
        0.0
    }
}

fn smoothstep(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// A part's vertex centroid in model space, if the part exists.
fn part_centroid(ev: &Evaluator, name: &str) -> Option<Vec2> {
    let index = ev.model().node_index(name)?;
    let verts = ev.world_vertices(index);
    if verts.is_empty() {
        return None;
    }
    let mut sum = Vec2::ZERO;
    for v in &verts {
        sum = sum.add(*v);
    }
    Some(sum.mul(1.0 / verts.len() as f32))
}

/// Alma's waist band rectangle in her local frame (the D-859
/// acceptance instrument): the REST Body part's AABB x-range
/// crossed with the 62–78% height band of its y-range. Static by
/// design — the acceptance staging anchors the band to the rest
/// geometry and the staging offset, not to the posed AABB (whose
/// band would ride up the chest when RootY stretches the AABB).
/// Returned as [x0, y0, x1, y1]; call on the rest evaluator.
fn waist_band(ev: &Evaluator) -> Result<[f32; 4], RigError> {
    let index = ev
        .model()
        .node_index("Body")
        .ok_or_else(|| RigError::new("TRG-SCENE-001", "Alma has no Body part"))?;
    let verts = ev.world_vertices(index);
    if verts.is_empty() {
        return Err(RigError::new(
            "TRG-SCENE-001",
            "Alma Body part has no vertices",
        ));
    }
    let (mut min, mut max) = (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
    for v in &verts {
        min = Vec2::new(min.x.min(v.x), min.y.min(v.y));
        max = Vec2::new(max.x.max(v.x), max.y.max(v.y));
    }
    let band_lo = min.y + 0.62 * (max.y - min.y);
    let band_hi = min.y + 0.78 * (max.y - min.y);
    Ok([min.x, band_lo, max.x, band_hi])
}

/// Distance from a point to an axis-aligned rectangle
/// ([x0, y0, x1, y1]); 0 when the point is inside.
fn point_rect_distance(p: Vec2, rect: [f32; 4]) -> f32 {
    let dx = (rect[0] - p.x).max(p.x - rect[2]).max(0.0);
    let dy = (rect[1] - p.y).max(p.y - rect[3]).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Full rest height of a figure (all parts' AABB height).
fn figure_height(ev: &Evaluator) -> f32 {
    let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
    for (index, node) in ev.model().nodes.iter().enumerate() {
        if matches!(node.kind, NodeKind::Part(_)) {
            for v in ev.world_vertices(index) {
                min_y = min_y.min(v.y);
                max_y = max_y.max(v.y);
            }
        }
    }
    if max_y > min_y { max_y - min_y } else { 0.0 }
}

/// Alma's foot bottom (largest y of FootL/FootR) at rest staging.
fn rest_foot_bottom(ev: &Evaluator) -> f32 {
    let mut bottom = f32::MIN;
    for name in ["FootL", "FootR"] {
        if let Some(index) = ev.model().node_index(name) {
            for v in ev.world_vertices(index) {
                bottom = bottom.max(v.y);
            }
        }
    }
    bottom + ALMA_HOME.y
}

/// Alma's foot bottom under a placement.
fn placed_foot_bottom(ev: &Evaluator, placement: Placement) -> f32 {
    let mut bottom = f32::MIN;
    for name in ["FootL", "FootR"] {
        if let Some(index) = ev.model().node_index(name) {
            for v in ev.world_vertices(index) {
                bottom = bottom.max(placement.apply(v).y);
            }
        }
    }
    bottom
}

/// Placed triangles of one figure: (name, AABB, triangles) per part.
fn placed_parts(ev: &Evaluator, placement: Placement) -> Vec<(String, [f32; 4], Vec<[Vec2; 3]>)> {
    let mut out = Vec::new();
    for (index, node) in ev.model().nodes.iter().enumerate() {
        let NodeKind::Part(part) = &node.kind else {
            continue;
        };
        let world = ev.world_vertices(index);
        let placed: Vec<Vec2> = world.iter().map(|&w| placement.apply(w)).collect();
        let mut aabb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for v in &placed {
            aabb[0] = aabb[0].min(v.x);
            aabb[1] = aabb[1].min(v.y);
            aabb[2] = aabb[2].max(v.x);
            aabb[3] = aabb[3].max(v.y);
        }
        let triangles = part
            .mesh
            .indices
            .chunks_exact(3)
            .map(|tri| {
                [
                    placed[tri[0] as usize],
                    placed[tri[1] as usize],
                    placed[tri[2] as usize],
                ]
            })
            .collect();
        out.push((node.name.clone(), aabb, triangles));
    }
    out
}

/// The C2 signed margin between the two placed figures, with
/// the part pair achieving it.
fn scene_margin(
    prima: &Evaluator,
    prima_placement: Placement,
    alma: &Evaluator,
    alma_placement: Placement,
) -> (f32, (String, String)) {
    let prima_parts = placed_parts(prima, prima_placement);
    let alma_parts = placed_parts(alma, alma_placement);
    let mut best_signed = f32::MAX;
    let mut best_unsigned = f32::MAX;
    let mut best_pair: (String, String) = (String::new(), String::new());
    for (name_a, aabb_a, tris_a) in &prima_parts {
        for (name_b, aabb_b, tris_b) in &alma_parts {
            let gap = aabb_gap(aabb_a, aabb_b);
            if gap > 0.0 && gap >= best_unsigned {
                continue;
            }
            for tri_a in tris_a {
                for tri_b in tris_b {
                    let d = tri_signed_distance(*tri_a, *tri_b);
                    if d >= 0.0 {
                        if d < best_unsigned && best_signed == f32::MAX {
                            best_pair = (name_a.clone(), name_b.clone());
                        }
                        best_unsigned = best_unsigned.min(d);
                    } else {
                        if d < best_signed {
                            best_pair = (name_a.clone(), name_b.clone());
                        }
                        best_signed = best_signed.min(d);
                        best_unsigned = 0.0;
                    }
                }
            }
        }
    }
    if best_signed != f32::MAX {
        (best_signed, best_pair)
    } else {
        (best_unsigned, best_pair)
    }
}

/// Gap between two AABBs ([min_x, min_y, max_x, max_y]); 0 when
/// they overlap.
fn aabb_gap(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let dx = (a[0] - b[2]).max(b[0] - a[2]).max(0.0);
    let dy = (a[1] - b[3]).max(b[1] - a[3]).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Signed 2D distance between two triangles: the minimum
/// segment-segment / vertex-triangle distance when disjoint;
/// when they overlap, the negative of the smallest distance from
/// an interior vertex of either triangle to the other's boundary
/// (0 when edges merely cross). This instrument is stated wherever
/// its numbers are reported.
fn tri_signed_distance(a: [Vec2; 3], b: [Vec2; 3]) -> f32 {
    let mut dmin = f32::MAX;
    for ea in edges(a) {
        for eb in edges(b) {
            dmin = dmin.min(seg_seg_distance(ea, eb));
        }
    }
    for v in a {
        dmin = dmin.min(point_tri_distance(v, b));
    }
    for v in b {
        dmin = dmin.min(point_tri_distance(v, a));
    }
    if dmin > 0.0 {
        return dmin;
    }
    if !triangles_overlap(a, b) {
        return 0.0;
    }
    let mut pen = f32::MAX;
    for v in a {
        if point_in_tri(v, b) {
            pen = pen.min(point_tri_boundary_distance(v, b));
        }
    }
    for v in b {
        if point_in_tri(v, a) {
            pen = pen.min(point_tri_boundary_distance(v, a));
        }
    }
    if pen == f32::MAX { 0.0 } else { -pen }
}

fn edges(t: [Vec2; 3]) -> [[Vec2; 2]; 3] {
    [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]]
}

fn triangles_overlap(a: [Vec2; 3], b: [Vec2; 3]) -> bool {
    for ea in edges(a) {
        for eb in edges(b) {
            if segments_intersect(ea, eb) {
                return true;
            }
        }
    }
    point_in_tri(a[0], b) || point_in_tri(b[0], a)
}

/// Point-in-triangle by consistent edge signs (boundary counts).
fn point_in_tri(p: Vec2, t: [Vec2; 3]) -> bool {
    let d0 = cross(t[1].sub(t[0]), p.sub(t[0]));
    let d1 = cross(t[2].sub(t[1]), p.sub(t[1]));
    let d2 = cross(t[0].sub(t[2]), p.sub(t[2]));
    let any_neg = d0 < 0.0 || d1 < 0.0 || d2 < 0.0;
    let any_pos = d0 > 0.0 || d1 > 0.0 || d2 > 0.0;
    !(any_neg && any_pos)
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Distance from a point to a triangle (0 inside).
fn point_tri_distance(p: Vec2, t: [Vec2; 3]) -> f32 {
    if point_in_tri(p, t) {
        return 0.0;
    }
    point_tri_boundary_distance(p, t)
}

/// Distance from a point to a triangle's boundary segments.
fn point_tri_boundary_distance(p: Vec2, t: [Vec2; 3]) -> f32 {
    edges(t)
        .iter()
        .map(|e| point_seg_distance(p, *e))
        .fold(f32::MAX, f32::min)
}

fn point_seg_distance(p: Vec2, seg: [Vec2; 2]) -> f32 {
    let ab = seg[1].sub(seg[0]);
    let len2 = ab.x * ab.x + ab.y * ab.y;
    let t = if len2 == 0.0 {
        0.0
    } else {
        ((p.x - seg[0].x) * ab.x + (p.y - seg[0].y) * ab.y) / len2
    }
    .clamp(0.0, 1.0);
    let closest = seg[0].add(ab.mul(t));
    p.distance(closest)
}

fn seg_seg_distance(a: [Vec2; 2], b: [Vec2; 2]) -> f32 {
    if segments_intersect(a, b) {
        return 0.0;
    }
    point_seg_distance(a[0], b)
        .min(point_seg_distance(a[1], b))
        .min(point_seg_distance(b[0], a))
        .min(point_seg_distance(b[1], a))
}

fn segments_intersect(a: [Vec2; 2], b: [Vec2; 2]) -> bool {
    let d1 = cross(b[1].sub(b[0]), a[0].sub(b[0]));
    let d2 = cross(b[1].sub(b[0]), a[1].sub(b[0]));
    let d3 = cross(a[1].sub(a[0]), b[0].sub(a[0]));
    let d4 = cross(a[1].sub(a[0]), b[1].sub(a[0]));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}
