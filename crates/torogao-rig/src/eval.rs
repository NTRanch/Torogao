//! Fixed-step deterministic puppet evaluation.
//!
//! The evaluator owns a parsed [`Puppet`] plus working state. Each
//! frame: `begin_frame` resets parameter values to their defaults
//! and clears the working state; the driver sets values with
//! `set_param`; `end_frame(dt)` applies every parameter (the
//! production apply-all route: no value, including exactly (0, 0),
//! is skipped), updates transforms, steps physics when the record
//! has physics nodes, then resets the working state and applies
//! everything a second time with the physics outputs written back
//! as parameter values.
//!
//! All iteration is in file order over vectors, so evaluation is
//! byte-reproducible across fresh processes.

use std::f32::consts::PI;
use std::ops::{Add, Mul, Sub};

use crate::error::RigError;
use crate::math::{Mat4, Vec2};
use crate::model::{
    Binding, BindingKind, BindingValues, InterpolateMode, NodeKind, Param, PhysicsNode, Puppet,
    TransformOffset,
};

/// One deform contribution on a part: per-vertex offsets pushed by
/// one parameter during the current apply pass.
struct DeformEntry {
    param: usize,
    offsets: Vec<Vec2>,
}

/// Per-frame working state, reset at frame start and again before
/// the physics second pass.
struct Working {
    rel: Vec<TransformOffset>,
    zsort: Vec<f32>,
    opacity: Vec<f32>,
    deforms: Vec<Vec<DeformEntry>>,
}

impl Working {
    fn new(model: &Puppet) -> Working {
        Working {
            rel: model.nodes.iter().map(|n| n.offset).collect(),
            zsort: model.nodes.iter().map(|n| n.zsort).collect(),
            opacity: model
                .nodes
                .iter()
                .map(|n| n.drawable().map_or(1.0, |d| d.opacity))
                .collect(),
            deforms: (0..model.nodes.len()).map(|_| Vec::new()).collect(),
        }
    }

    fn reset(&mut self, model: &Puppet) {
        for (i, node) in model.nodes.iter().enumerate() {
            self.rel[i] = node.offset;
            self.zsort[i] = node.zsort;
            self.opacity[i] = node.drawable().map_or(1.0, |d| d.opacity);
            self.deforms[i].clear();
        }
    }
}

/// Simulation state for one physics node.
#[derive(Clone, Copy)]
enum PhysicsState {
    Rigid { bob: Vec2, theta: f32, omega: f32 },
    Spring { pos: Vec2, vel: Vec2 },
}

impl PhysicsState {
    fn bob(&self) -> Vec2 {
        match self {
            PhysicsState::Rigid { bob, .. } => *bob,
            PhysicsState::Spring { pos, .. } => *pos,
        }
    }
}

/// A puppet under evaluation.
pub struct Evaluator {
    model: Puppet,
    values: Vec<Vec2>,
    working: Working,
    absolute: Vec<Mat4>,
    zsort_abs: Vec<f32>,
    physics: Vec<Option<PhysicsState>>,
    has_physics: bool,
    draw_root: Vec<usize>,
    draw_children: Vec<(usize, Vec<usize>)>,
}

impl Evaluator {
    /// Create an evaluator at the rest evaluation (defaults applied,
    /// transforms and draw lists computed).
    pub fn new(model: Puppet) -> Evaluator {
        let mut physics = Vec::with_capacity(model.nodes.len());
        let mut has_physics = false;
        for node in &model.nodes {
            match &node.kind {
                NodeKind::Physics(phys) => {
                    has_physics = true;
                    physics.push(Some(if phys.spring {
                        PhysicsState::Spring {
                            pos: Vec2::ZERO,
                            vel: Vec2::ZERO,
                        }
                    } else {
                        PhysicsState::Rigid {
                            bob: Vec2::ZERO,
                            theta: 0.0,
                            omega: 0.0,
                        }
                    }));
                }
                _ => physics.push(None),
            }
        }
        let mut evaluator = Evaluator {
            values: model.params.iter().map(|p| p.defaults).collect(),
            working: Working::new(&model),
            absolute: vec![Mat4::IDENTITY; model.nodes.len()],
            zsort_abs: vec![0.0; model.nodes.len()],
            physics,
            has_physics,
            model,
            draw_root: Vec::new(),
            draw_children: Vec::new(),
        };
        evaluator.apply_all_params();
        evaluator.update_transforms();
        evaluator.update_draw_lists();
        evaluator
    }

    /// The parsed record under evaluation.
    pub fn model(&self) -> &Puppet {
        &self.model
    }

    /// Start a frame: parameter values return to their defaults and
    /// the working state resets. Physics simulation state persists
    /// across frames by design.
    pub fn begin_frame(&mut self) {
        for (i, param) in self.model.params.iter().enumerate() {
            self.values[i] = param.defaults;
        }
        self.working.reset(&self.model);
    }

    /// Set a parameter value by name for the current frame.
    pub fn set_param(&mut self, name: &str, value: Vec2) -> Result<(), RigError> {
        let index = self
            .model
            .param_index(name)
            .ok_or_else(|| RigError::new("TRG-EVAL-001", format!("unknown parameter '{name}'")))?;
        self.values[index] = value;
        Ok(())
    }

    /// Set a parameter value by arena index.
    pub fn set_param_index(&mut self, index: usize, value: Vec2) {
        self.values[index] = value;
    }

    /// The current (driver-set or physics-written) value of a
    /// parameter by arena index.
    pub fn param_value(&self, index: usize) -> Vec2 {
        self.values[index]
    }

    /// Finish a frame: apply parameters, update transforms, step
    /// physics (dt > 0), reset, write physics outputs back, and
    /// apply again. `dt` is the frame step in seconds; the estate
    /// protocol uses a fixed 1/60.
    ///
    /// # Panics
    /// Panics when `dt` is negative; that is a driver contract
    /// violation, not record data.
    pub fn end_frame(&mut self, dt: f32) {
        assert!(dt >= 0.0, "end_frame dt must be non-negative, got {dt}");
        self.apply_all_params();
        self.update_transforms();
        if self.has_physics && dt > 0.0 {
            let outputs = self.step_physics(dt);
            self.working.reset(&self.model);
            for (param_index, value) in outputs {
                self.values[param_index] = value;
            }
            self.apply_all_params();
            self.update_transforms();
        }
        self.update_draw_lists();
    }

    /// The node's absolute matrix from the latest evaluation.
    pub fn absolute_matrix(&self, node: usize) -> Mat4 {
        self.absolute[node]
    }

    /// The node's accumulated z-sort from the latest evaluation.
    pub fn abs_zsort(&self, node: usize) -> f32 {
        self.zsort_abs[node]
    }

    /// The node's working opacity (base times opacity bindings).
    pub fn working_opacity(&self, node: usize) -> f32 {
        self.working.opacity[node]
    }

    /// The node's summed deform offsets for the current frame
    /// (zeros when the part carries no deform).
    pub fn combined_deform(&self, node: usize) -> Vec<Vec2> {
        let vertex_count = match &self.model.nodes[node].kind {
            NodeKind::Part(part) => part.mesh.verts.len(),
            _ => return Vec::new(),
        };
        let mut out = vec![Vec2::ZERO; vertex_count];
        for entry in &self.working.deforms[node] {
            for (i, offset) in entry.offsets.iter().enumerate() {
                out[i] = out[i].add(*offset);
            }
        }
        out
    }

    /// World-space vertices of a part: local verts plus combined
    /// deform, transformed by the absolute matrix. The authored mesh
    /// origin is not subtracted (the reference route's `offset`
    /// uniform is never set).
    pub fn world_vertices(&self, node: usize) -> Vec<Vec2> {
        let NodeKind::Part(part) = &self.model.nodes[node].kind else {
            return Vec::new();
        };
        let deform = self.combined_deform(node);
        let matrix = self.absolute[node];
        part.mesh
            .verts
            .iter()
            .enumerate()
            .map(|(i, v)| matrix.transform_point2(v.add(deform[i])))
            .collect()
    }

    /// Drawables outside any composite, in draw order (accumulated
    /// z-sort descending, stable for ties).
    pub fn draw_root(&self) -> &[usize] {
        &self.draw_root
    }

    /// Per-composite child draw lists, in draw order.
    pub fn draw_children(&self) -> &[(usize, Vec<usize>)] {
        &self.draw_children
    }

    fn apply_all_params(&mut self) {
        for index in 0..self.model.params.len() {
            apply_param(&self.model, &mut self.working, index, self.values[index]);
        }
    }

    fn update_transforms(&mut self) {
        for index in 0..self.model.nodes.len() {
            let node = &self.model.nodes[index];
            let relative = self.working.rel[index].to_matrix();
            let (base_matrix, base_zsort) = if index == 0 {
                (Mat4::IDENTITY, 0.0)
            } else if node.lock_to_root {
                (self.absolute[0], self.zsort_abs[0])
            } else {
                let parent = node.parent.expect("non-root node has a parent");
                (self.absolute[parent], self.zsort_abs[parent])
            };
            self.absolute[index] = base_matrix.mul_mat(relative);
            self.zsort_abs[index] = base_zsort + self.working.zsort[index];
        }
    }

    fn update_draw_lists(&mut self) {
        let mut root: Vec<usize> = Vec::new();
        let mut children: Vec<(usize, Vec<usize>)> = Vec::new();
        for (index, node) in self.model.nodes.iter().enumerate() {
            if !node.is_drawable() {
                continue;
            }
            match nearest_composite_ancestor(&self.model, index) {
                None => root.push(index),
                Some(comp) => {
                    if let Some((_, list)) = children.iter_mut().find(|(c, _)| *c == comp) {
                        list.push(index);
                    } else {
                        children.push((comp, vec![index]));
                    }
                }
            }
        }
        let zsort = &self.zsort_abs;
        root.sort_by(|a, b| zsort[*b].total_cmp(&zsort[*a]));
        for (_, list) in &mut children {
            list.sort_by(|a, b| zsort[*b].total_cmp(&zsort[*a]));
        }
        self.draw_root = root;
        self.draw_children = children;
    }

    /// Step every physics node once for this frame; returns the
    /// (parameter index, value) outputs in node file order.
    fn step_physics(&mut self, dt: f32) -> Vec<(usize, Vec2)> {
        let mut outputs = Vec::new();
        for index in 0..self.model.nodes.len() {
            let NodeKind::Physics(phys) = &self.model.nodes[index].kind else {
                continue;
            };
            let phys = *phys;
            let anchor = if phys.local_only {
                self.working.rel[index].translation.xy()
            } else {
                self.absolute[index].transform_point2(Vec2::ZERO)
            };
            let state = self.physics[index].as_mut().expect("physics state exists");
            // Substep structure mirrors the reference: steps of at
            // most 0.01 s, the remainder decremented by 0.01 per
            // step, dt clamped to 10 s. (The reference also advances
            // a per-node clock its eval functions ignore.)
            let mut remaining = dt.min(10.0);
            while remaining > 0.0 {
                let step = remaining.min(0.01);
                tick_physics(state, &phys, &self.model, anchor, step);
                remaining -= 0.01;
            }
            let value = physics_output(*state, &phys, anchor, self.absolute[index]);
            outputs.push((phys.param, value));
        }
        outputs
    }
}

/// The nearest strict ancestor that is a composite, if any.
fn nearest_composite_ancestor(model: &Puppet, mut index: usize) -> Option<usize> {
    while let Some(parent) = model.nodes[index].parent {
        if matches!(model.nodes[parent].kind, NodeKind::Composite(_)) {
            return Some(parent);
        }
        index = parent;
    }
    None
}

/// Clamp with a possibly inverted authored range (falls back to the
/// range minimum instead of panicking, as `f32::clamp` would).
fn clamp_range(value: f32, min: f32, max: f32) -> f32 {
    if min <= max {
        value.clamp(min, max)
    } else {
        min
    }
}

/// Apply one parameter (already-resolved current value) to the
/// working state.
fn apply_param(model: &Puppet, working: &mut Working, param_index: usize, raw_value: Vec2) {
    let param = &model.params[param_index];
    let value = Vec2::new(
        clamp_range(raw_value.x, param.min.x, param.max.x),
        clamp_range(raw_value.y, param.min.y, param.max.y),
    );
    for binding in &param.bindings {
        apply_binding(working, param, param_index, binding, value);
    }
}

/// Apply one binding at a (clamped) parameter value.
fn apply_binding(
    working: &mut Working,
    param: &Param,
    param_index: usize,
    binding: &Binding,
    value: Vec2,
) {
    let node = binding.node;
    match binding.kind {
        BindingKind::ZSort => {
            working.zsort[node] += scalar_at(param, binding, value);
        }
        BindingKind::Translate => {
            let v = vector_at(param, binding, value);
            working.rel[node].translation.x += v.x;
            working.rel[node].translation.y += v.y;
        }
        BindingKind::TranslateX => {
            working.rel[node].translation.x += scalar_at(param, binding, value);
        }
        BindingKind::TranslateY => {
            working.rel[node].translation.y += scalar_at(param, binding, value);
        }
        BindingKind::Scale => {
            let v = vector_at(param, binding, value);
            let scale = working.rel[node].scale;
            working.rel[node].scale = scale.mul_vec(v);
        }
        BindingKind::ScaleX => {
            working.rel[node].scale.x *= scalar_at(param, binding, value);
        }
        BindingKind::ScaleY => {
            working.rel[node].scale.y *= scalar_at(param, binding, value);
        }
        BindingKind::RotateX => {
            working.rel[node].rotation.x += scalar_at(param, binding, value);
        }
        BindingKind::RotateY => {
            working.rel[node].rotation.y += scalar_at(param, binding, value);
        }
        BindingKind::RotateZ => {
            working.rel[node].rotation.z += scalar_at(param, binding, value);
        }
        BindingKind::Opacity => {
            working.opacity[node] *= scalar_at(param, binding, value);
        }
        BindingKind::Deform => {
            let offsets = deform_at(param, binding, value);
            // Multiple bindings from one parameter onto one node sum
            // (deform is additive by design); the reference route's
            // deform stack would reject the duplicate source, a case
            // the corpus does not contain.
            if let Some(entry) = working.deforms[node]
                .iter_mut()
                .find(|e| e.param == param_index)
            {
                for (i, offset) in offsets.iter().enumerate() {
                    entry.offsets[i] = entry.offsets[i].add(*offset);
                }
            } else {
                working.deforms[node].push(DeformEntry {
                    param: param_index,
                    offsets,
                });
            }
        }
    }
}

/// The four grid corners and their weights for a parameter value:
/// bilinear for Linear, a single corner for Nearest.
struct GridSample {
    corners: [(usize, usize); 4],
    weights: [f32; 4],
}

fn grid_sample(param: &Param, mode: InterpolateMode, value: Vec2) -> GridSample {
    let normed = Vec2::new(
        normed_component(value.x, param.min.x, param.max.x),
        normed_component(value.y, param.min.y, param.max.y),
    );
    let (x_beg, x_end, tx) = axis_sample(&param.axis_points[0], normed.x, mode);
    let (y_beg, y_end, ty) = axis_sample(&param.axis_points[1], normed.y, mode);
    match mode {
        InterpolateMode::Linear => GridSample {
            corners: [
                (x_beg, y_beg),
                (x_end, y_beg),
                (x_beg, y_end),
                (x_end, y_end),
            ],
            weights: [
                (1.0 - tx) * (1.0 - ty),
                tx * (1.0 - ty),
                (1.0 - tx) * ty,
                tx * ty,
            ],
        },
        InterpolateMode::Nearest => GridSample {
            corners: [
                (x_beg, y_beg),
                (x_beg, y_beg),
                (x_beg, y_beg),
                (x_beg, y_beg),
            ],
            weights: [1.0, 0.0, 0.0, 0.0],
        },
    }
}

/// Normalize a value into [0, 1] across [min, max]; a degenerate
/// range normalizes to 0.
fn normed_component(value: f32, min: f32, max: f32) -> f32 {
    if max == min {
        0.0
    } else {
        (value - min) / (max - min)
    }
}

/// Interval selection on one axis: returns (beg, end, t) for Linear
/// or (pick, pick, 0) for Nearest.
fn axis_sample(points: &[f32], value: f32, mode: InterpolateMode) -> (usize, usize, f32) {
    if points.len() <= 1 {
        return (0, 0, 0.0);
    }
    let (beg, end) = axis_interval(points, value);
    match mode {
        InterpolateMode::Linear => {
            let lo = points[beg];
            let hi = points[end];
            let t = if hi == lo {
                0.0
            } else {
                (value.clamp(lo, hi) - lo) / (hi - lo)
            };
            (beg, end, t)
        }
        InterpolateMode::Nearest => {
            let pick = if value - points[beg] <= points[end] - value {
                beg
            } else {
                end
            };
            (pick, pick, 0.0)
        }
    }
}

/// The axis interval containing `value`: exact hits at the last
/// point use the final interval; values below the first point use
/// the first interval.
fn axis_interval(points: &[f32], value: f32) -> (usize, usize) {
    let last = points.len() - 1;
    match points.binary_search_by(|p| p.total_cmp(&value)) {
        Ok(i) if i >= last => (last - 1, last),
        Ok(i) => (i, i + 1),
        Err(0) => (0, 1),
        Err(i) if i >= points.len() => (last - 1, last),
        Err(i) => (i - 1, i),
    }
}

/// Interpolated scalar cell value.
fn scalar_at(param: &Param, binding: &Binding, value: Vec2) -> f32 {
    let BindingValues::Scalars(grid) = &binding.values else {
        return 0.0;
    };
    let sample = grid_sample(param, binding.interpolate, value);
    let mut out = 0.0f32;
    for (k, (cx, cy)) in sample.corners.iter().enumerate() {
        out += grid[*cx][*cy] * sample.weights[k];
    }
    out
}

/// Interpolated vector cell value.
fn vector_at(param: &Param, binding: &Binding, value: Vec2) -> Vec2 {
    let BindingValues::Vectors(grid) = &binding.values else {
        return Vec2::ZERO;
    };
    let sample = grid_sample(param, binding.interpolate, value);
    let mut out = Vec2::ZERO;
    for (k, (cx, cy)) in sample.corners.iter().enumerate() {
        out = out.add(grid[*cx][*cy].mul(sample.weights[k]));
    }
    out
}

/// Interpolated per-vertex deform offsets: the four corner cells
/// accumulate weighted, per vertex.
fn deform_at(param: &Param, binding: &Binding, value: Vec2) -> Vec<Vec2> {
    let BindingValues::Deforms(grid) = &binding.values else {
        return Vec::new();
    };
    let sample = grid_sample(param, binding.interpolate, value);
    let vertex_count = grid[sample.corners[0].0][sample.corners[0].1].len();
    let mut out = vec![Vec2::ZERO; vertex_count];
    for (k, (cx, cy)) in sample.corners.iter().enumerate() {
        let weight = sample.weights[k];
        if weight == 0.0 {
            continue;
        }
        for (i, offset) in grid[*cx][*cy].iter().enumerate() {
            out[i] = out[i].add(offset.mul(weight));
        }
    }
    out
}

/// Advance one physics node by one substep (at most 0.01 s by the
/// caller's loop, with the final substep taking the remainder).
fn tick_physics(
    state: &mut PhysicsState,
    phys: &PhysicsNode,
    model: &Puppet,
    anchor: Vec2,
    step: f32,
) {
    match state {
        PhysicsState::Rigid { bob, theta, omega } => {
            let d_bob = bob.sub(anchor);
            *theta = (-d_bob.x).atan2(d_bob.y);
            let g = phys.gravity * model.pixels_per_meter * model.gravity;
            let r = phys.length;
            let crit_damp = 2.0 * (g / r).sqrt();
            let vars = rk4([*theta, *omega], step, |v| {
                [
                    v[1],
                    -(g / r) * v[0].sin() - v[1] * phys.angle_damping * crit_damp,
                ]
            });
            *theta = vars[0];
            *omega = vars[1];
            *bob = anchor.add(Vec2::new(-theta.sin(), theta.cos()).mul(r));
        }
        PhysicsState::Spring { pos, vel } => {
            let vars = rk4([pos.x, pos.y, vel.x, vel.y], step, |v| {
                spring_derivatives(v, phys, model, anchor)
            });
            *pos = Vec2::new(vars[0], vars[1]);
            *vel = Vec2::new(vars[2], vars[3]);
        }
    }
}

/// Spring-pendulum derivatives for state [pos.x, pos.y, vel.x,
/// vel.y], returning [vel.x, vel.y, acc.x, acc.y]. The damping
/// back-rotation deliberately reproduces the reference formula
/// verbatim, including its use of the un-damped rotated velocity in
/// the x component.
fn spring_derivatives(
    vars: [f32; 4],
    phys: &PhysicsNode,
    model: &Puppet,
    anchor: Vec2,
) -> [f32; 4] {
    let bob_pos = Vec2::new(vars[0], vars[1]);
    let bob_vel = Vec2::new(vars[2], vars[3]);
    let spring_ksqrt = phys.frequency * 2.0 * PI;
    let spring_k = spring_ksqrt * spring_ksqrt;
    let g = phys.gravity * model.pixels_per_meter * model.gravity;
    let rest_length = phys.length - g / spring_k;
    let off_pos = bob_pos.sub(anchor);
    let off_norm = off_pos.normalize();
    let crit_damp_angle = 2.0 * (g / phys.length).sqrt();
    let crit_damp_length = 2.0 * spring_ksqrt;
    let dist = anchor.distance(bob_pos).abs();
    let force = Vec2::new(0.0, g).sub(off_norm.mul((dist - rest_length) * spring_k));
    let d_bob_rot = Vec2::new(
        bob_vel.x * off_norm.y + bob_vel.y * off_norm.x,
        bob_vel.y * off_norm.y - bob_vel.x * off_norm.x,
    );
    let dd_bob_rot = Vec2::new(
        -d_bob_rot.x * phys.angle_damping * crit_damp_angle,
        -d_bob_rot.y * phys.length_damping * crit_damp_length,
    );
    let dd_bob_damping = Vec2::new(
        dd_bob_rot.x * off_norm.y - d_bob_rot.y * off_norm.x,
        dd_bob_rot.y * off_norm.y + d_bob_rot.x * off_norm.x,
    );
    let acc = force.add(dd_bob_damping);
    [bob_vel.x, bob_vel.y, acc.x, acc.y]
}

/// Map a finished physics state onto its parameter value.
fn physics_output(state: PhysicsState, phys: &PhysicsNode, anchor: Vec2, absolute: Mat4) -> Vec2 {
    let bob = state.bob();
    let local_pos = if phys.local_only {
        bob
    } else {
        let inverse = absolute.inverse().unwrap_or(Mat4::IDENTITY);
        let out = inverse.transform_vec4([bob.x, bob.y, 0.0, 1.0]);
        Vec2::new(out[0], out[1])
    };
    let local_angle = local_pos.normalize();
    let relative_length = bob.distance(anchor) / phys.length;
    let value = match phys.map_mode {
        crate::model::PhysicsMapMode::AngleLength => {
            Vec2::new((-local_angle.x).atan2(local_angle.y) / PI, relative_length)
        }
        crate::model::PhysicsMapMode::LengthAngle => {
            Vec2::new(relative_length, (-local_angle.x).atan2(local_angle.y) / PI)
        }
        crate::model::PhysicsMapMode::Xy => {
            let base = local_angle.mul(relative_length).sub(Vec2::new(0.0, 1.0));
            Vec2::new(base.x, -base.y)
        }
        crate::model::PhysicsMapMode::Yx => {
            let base = local_angle.mul(relative_length).sub(Vec2::new(0.0, 1.0));
            let xy = Vec2::new(base.x, -base.y);
            Vec2::new(xy.y, xy.x)
        }
    };
    value.mul_vec(phys.output_scale)
}

/// Classic RK4 over an f32 state vector, with the reference
/// integrator's failure behavior: if any combined component is
/// non-finite, the whole step reverts to the pre-step state.
fn rk4<const N: usize>(curs: [f32; N], h: f32, deriv: impl Fn([f32; N]) -> [f32; N]) -> [f32; N] {
    let k1 = deriv(curs);
    let stage2 = std::array::from_fn(|i| curs[i] + h * k1[i] / 2.0);
    let k2 = deriv(stage2);
    let stage3 = std::array::from_fn(|i| curs[i] + h * k2[i] / 2.0);
    let k3 = deriv(stage3);
    let stage4 = std::array::from_fn(|i| curs[i] + h * k3[i]);
    let k4 = deriv(stage4);
    let mut out = [0.0f32; N];
    for i in 0..N {
        out[i] = curs[i] + h * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]) / 6.0;
        if !out[i].is_finite() {
            return curs;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Binding, BindingKind, BindingValues, InterpolateMode, Param};

    fn scalar_param(axis: Vec<f32>, grid: Vec<Vec<f32>>) -> Param {
        Param {
            uuid: 1,
            name: "P".to_owned(),
            is_vec2: false,
            min: Vec2::new(0.0, 0.0),
            max: Vec2::new(1.0, 0.0),
            defaults: Vec2::ZERO,
            axis_points: [axis, vec![0.0]],
            bindings: vec![Binding {
                node: 0,
                kind: BindingKind::ZSort,
                interpolate: InterpolateMode::Linear,
                values: BindingValues::Scalars(grid),
                is_set: Vec::new(),
            }],
        }
    }

    #[test]
    fn axis_interval_exact_last_point_uses_final_interval() {
        let points = [0.0, 0.5, 1.0];
        assert_eq!(axis_interval(&points, 1.0), (1, 2));
        assert_eq!(axis_interval(&points, 0.5), (1, 2));
        assert_eq!(axis_interval(&points, 0.0), (0, 1));
        assert_eq!(axis_interval(&points, -0.25), (0, 1));
        assert_eq!(axis_interval(&points, 1.25), (1, 2));
        assert_eq!(axis_interval(&points, 0.25), (0, 1));
    }

    #[test]
    fn apply_all_at_zero_still_moves_the_node() {
        // The estate ruling (D-869): a parameter at exactly (0, 0)
        // applies like any other value. A param whose (0, 0) grid
        // cell is non-zero must move its target at the default.
        let param = scalar_param(vec![0.0, 1.0], vec![vec![3.0], vec![7.0]]);
        let model = Puppet {
            name: None,
            version: String::new(),
            pixels_per_meter: 100.0,
            gravity: 1.0,
            nodes: vec![crate::model::Node {
                uuid: 1,
                name: "root".to_owned(),
                enabled: true,
                zsort: 10.0,
                offset: TransformOffset::default(),
                lock_to_root: false,
                parent: None,
                children: Vec::new(),
                kind: NodeKind::Group,
            }],
            params: vec![param],
            textures: Vec::new(),
            extensions: Vec::new(),
            dropped_texture_slots: 0,
            dropped_unsupported_bindings: 0,
            skipped_bindings: 0,
            tga_rle_line_crossings: 0,
        };
        let mut ev = Evaluator::new(model);
        assert!((ev.abs_zsort(0) - 13.0).abs() < 1e-6);
        ev.begin_frame();
        ev.set_param_index(0, Vec2::new(1.0, 0.0));
        ev.end_frame(0.0);
        assert!((ev.abs_zsort(0) - 17.0).abs() < 1e-6);
    }

    #[test]
    fn rk4_reverts_wholly_on_non_finite() {
        let out = rk4([1.0f32, 2.0], 0.5, |_| [f32::INFINITY, 0.0]);
        assert_eq!(out, [1.0, 2.0]);
    }
}
