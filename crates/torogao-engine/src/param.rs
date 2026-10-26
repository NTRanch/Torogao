//! Parameters and their bindings — the evaluation core,
//! ported operation for operation from the reference:
//! list-based input combinators (additive sum / weighted
//! average over the push count), axis mapping, binding
//! interpolation (nearest / linear / cubic; stepped and
//! bezier hit an unreachable assert in the reference and
//! yield zero here), and the apply step that pushes values
//! and deformations into nodes.

use crate::math::{clamp_f32, Vec2};
use crate::node::NodeArena;
use serde_json::Value;
use torogao_format::NodeKind;
use torogao_format::value::{jget, jnum, jstr};

/// A deformation: one 2D offset per vertex.
pub type Deformation = Vec<Vec2>;

/// How pushed inputs combine with the parameter value
/// (the reference `ParamMergeMode`, flexible spellings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    /// Additive (the default).
    Additive,
    /// Weighted average into the multiplicative combinator.
    Weighted,
    /// Multiplicative-named mode: pushes land in the same
    /// combinator with weight 1, and the combinator resolves
    /// as an average — the reference behavior.
    Multiplicative,
    /// Force the value directly.
    Forced,
    /// Resolve through the parameter's own merge mode.
    Passthrough,
}

impl MergeMode {
    /// Parses the payload's flexible mode spelling.
    pub fn parse(text: &str) -> Option<MergeMode> {
        match text {
            "Additive" | "additive" => Some(MergeMode::Additive),
            "Weighted" | "weighted" => Some(MergeMode::Weighted),
            "Multiplicative" | "multiplicative" => Some(MergeMode::Multiplicative),
            "Forced" | "forced" => Some(MergeMode::Forced),
            "Passthrough" | "passthrough" => Some(MergeMode::Passthrough),
            _ => None,
        }
    }
}

/// How a binding interpolates between keypoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolateMode {
    /// Nearest keypoint (switches at offset >= 0.5).
    Nearest,
    /// Linear.
    Linear,
    /// Stepped (unreachable assert in the reference: yields zero).
    Stepped,
    /// Cubic (Catmull-Rom).
    Cubic,
    /// Bezier (unreachable assert in the reference: yields zero).
    Bezier,
}

impl InterpolateMode {
    /// Parses the payload's flexible mode spelling.
    pub fn parse(text: &str) -> Option<InterpolateMode> {
        match text {
            "Nearest" | "nearest" => Some(InterpolateMode::Nearest),
            "Linear" | "linear" => Some(InterpolateMode::Linear),
            "Stepped" | "stepped" => Some(InterpolateMode::Stepped),
            "Cubic" | "cubic" => Some(InterpolateMode::Cubic),
            "Bezier" | "bezier" => Some(InterpolateMode::Bezier),
            _ => None,
        }
    }
}

/// One push combinator (the reference `Combinator`): a list
/// of pushed values with weights. `csum` sums the values;
/// `avg` returns the weight-multiplied sum divided by the
/// push COUNT (not the weight sum) — the reference quirk
/// that makes "multiplicative" pushes resolve as a mean.
#[derive(Debug, Clone, Default)]
pub struct Combinator {
    values: Vec<Vec2>,
    weights: Vec<f32>,
    count: usize,
}

impl Combinator {
    /// Drops all pushes (the storage is retained, as in the
    /// reference).
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// Appends a full-vector push.
    pub fn add(&mut self, value: Vec2, weight: f32) {
        if self.count >= self.values.len() {
            self.values.resize(self.count + 8, Vec2::ZERO);
            self.weights.resize(self.count + 8, 0.0);
        }
        self.values[self.count] = value;
        self.weights[self.count] = weight;
        self.count += 1;
    }

    /// Appends a single-axis push; the other axis carries
    /// 1.0 (the reference's axis form, for any axis value).
    pub fn add_axis(&mut self, axis: i32, value: f32, weight: f32) {
        let vector = Vec2::new(
            if axis == 0 { value } else { 1.0 },
            if axis == 1 { value } else { 1.0 },
        );
        self.add(vector, weight);
    }

    /// The sum of pushed values.
    pub fn csum(&self) -> Vec2 {
        let mut val = Vec2::ZERO;
        for i in 0..self.count {
            val += self.values[i];
        }
        val
    }

    /// The weighted sum divided by the push count; (1, 1)
    /// when nothing was pushed.
    pub fn avg(&self) -> Vec2 {
        if self.count == 0 {
            return Vec2::ONE;
        }
        let mut val = Vec2::ZERO;
        for i in 0..self.count {
            val += self.values[i] * self.weights[i];
        }
        val / (self.count as f32)
    }
}

/// A binding's keyform grid values.
#[derive(Debug, Clone)]
pub enum BindingValues {
    /// Scalar keyforms (value bindings).
    Scalar(Vec<Vec<f32>>),
    /// Vertex-offset keyforms (deformation bindings).
    Deform(Vec<Vec<Deformation>>),
}

/// One binding from a parameter to a node property or a
/// drawable's deformation stack.
#[derive(Debug, Clone)]
pub struct Binding {
    /// The node uuid the payload names.
    pub node_ref: u32,
    /// The resolved node index (set at finalize).
    pub node: Option<usize>,
    /// The payload's property name ("deform" = deformation).
    pub param_name: String,
    /// Keyform grids, [x keypoint][y keypoint].
    pub values: BindingValues,
    /// Which keyforms are set.
    pub is_set: Vec<Vec<bool>>,
    /// Interpolation mode.
    pub interpolate_mode: InterpolateMode,
}

impl Binding {
    /// Whether this is a deformation binding.
    pub fn is_deformation(&self) -> bool {
        matches!(self.values, BindingValues::Deform(_))
    }

    /// Counts the set keyforms.
    pub fn set_count(&self) -> usize {
        self.is_set.iter().flatten().filter(|s| **s).count()
    }
}

/// One puppet parameter.
#[derive(Debug, Clone)]
pub struct Parameter {
    /// Parameter uuid.
    pub uuid: u32,
    /// Display name (bindings and recipes address by name).
    pub name: String,
    /// Whether the parameter participates in updates.
    pub active: bool,
    /// The authored value.
    pub value: Vec2,
    /// The last computed internal value (value + pushes).
    pub last_internal: Vec2,
    /// Merge mode for passthrough pushes.
    pub merge_mode: MergeMode,
    /// Default value.
    pub defaults: Vec2,
    /// Whether this is a 2-axis parameter.
    pub is_vec2: bool,
    /// Value range minimum.
    pub min: Vec2,
    /// Value range maximum.
    pub max: Vec2,
    /// Axis keypoint positions per axis.
    pub axis_points: [Vec<f32>; 2],
    /// Bindings driven by this parameter.
    pub bindings: Vec<Binding>,
    iadd: Combinator,
    imul: Combinator,
}

/// Why a parameter record failed to decode (the engine's
/// load boundary turns this into a malformed-project
/// refusal, as the reference loader throws there).
#[derive(Debug, Clone)]
pub struct ParamDecodeError(pub String);

fn vec2_of(value: &Value) -> Vec2 {
    match value {
        Value::Array(items) => Vec2::new(
            items.first().map(num).unwrap_or(0.0),
            items.get(1).map(num).unwrap_or(0.0),
        ),
        _ => Vec2::ZERO,
    }
}

fn num(value: &Value) -> f32 {
    value.as_f64().unwrap_or(0.0) as f32
}

impl Parameter {
    /// Decodes a parameter from its raw payload record.
    /// Mirrors the reference `deserializeFromFghj`: uuid and
    /// name are required; the rest are forgiving. Bindings
    /// with an empty `param_name` are skipped, and binding
    /// grid dimensions must match the axis point counts —
    /// the reference enforces that at binding decode, so a
    /// mismatch fails the load.
    pub fn from_payload(raw: &Value) -> Result<Parameter, ParamDecodeError> {
        let uuid = match jget(raw, "uuid") {
            Value::Number(number) => number.as_u64().unwrap_or(u32::MAX as u64) as u32,
            _ => {
                return Err(ParamDecodeError(
                    "parameter record carries no numeric uuid".to_string(),
                ))
            }
        };
        let name = match jget(raw, "name") {
            Value::String(text) => text.clone(),
            _ => {
                return Err(ParamDecodeError(
                    "parameter record carries no string name".to_string(),
                ))
            }
        };
        let is_vec2 = matches!(jget(raw, "is_vec2"), Value::Bool(true));
        let min = vec2_of(jget(raw, "min"));
        let max = vec2_of(jget(raw, "max"));
        let defaults = vec2_of(jget(raw, "defaults"));
        let mut axis_points: [Vec<f32>; 2] = [Vec::new(), Vec::new()];
        if let Value::Array(axes) = jget(raw, "axis_points") {
            for (axis, points) in axes.iter().enumerate().take(2) {
                if let Value::Array(points) = points {
                    axis_points[axis] = points.iter().map(num).collect();
                }
            }
        }
        let merge_mode = match jget(raw, "merge_mode") {
            Value::String(text) => MergeMode::parse(text).unwrap_or(MergeMode::Additive),
            _ => MergeMode::Additive,
        };
        let mut bindings = Vec::new();
        if let Value::Array(items) = jget(raw, "bindings") {
            for item in items {
                let param_name = jstr(item, "param_name");
                if param_name.is_empty() {
                    continue;
                }
                bindings.push(binding_from_payload(item, param_name, &axis_points)?);
            }
        }
        Ok(Parameter {
            uuid,
            name,
            active: true,
            value: defaults,
            last_internal: Vec2::ZERO,
            merge_mode,
            defaults,
            is_vec2,
            min,
            max,
            axis_points,
            bindings,
            iadd: Combinator::default(),
            imul: Combinator::default(),
        })
    }

    /// Maps a value through the parameter's range into
    /// [0, 1] per component (the reference `mapValue`).
    pub fn map_value(&self, value: Vec2) -> Vec2 {
        Vec2::new(
            clamp_f32((value.x - self.min.x) / (self.max.x - self.min.x), 0.0, 1.0),
            clamp_f32((value.y - self.min.y) / (self.max.y - self.min.y), 0.0, 1.0),
        )
    }

    /// Finds the keypoint segment + offset for the mapped
    /// value (the reference `findOffset`; axis 1 is only
    /// visited for vec2 parameters).
    pub fn find_offset(&self, mapped: Vec2) -> (Vec2, [usize; 2]) {
        let mut offset = Vec2::ZERO;
        let mut index = [0usize; 2];
        for axis in 0..2 {
            if axis == 1 && !self.is_vec2 {
                break;
            }
            let val = if axis == 0 { mapped.x } else { mapped.y };
            let pos = &self.axis_points[axis];
            if pos.len() >= 2 {
                for i in 0..pos.len() - 1 {
                    if pos[i + 1] > val || i == pos.len() - 2 {
                        index[axis] = i;
                        let o = (val - pos[i]) / (pos[i + 1] - pos[i]);
                        if axis == 0 {
                            offset.x = o;
                        } else {
                            offset.y = o;
                        }
                        break;
                    }
                }
            }
        }
        (offset, index)
    }

    /// Pushes an offset (the reference `pushIOffset`;
    /// passthrough resolves through this parameter's merge
    /// mode).
    pub fn push_offset(&mut self, value: Vec2, mode: MergeMode, weight: f32) {
        let mode = if mode == MergeMode::Passthrough {
            self.merge_mode
        } else {
            mode
        };
        match mode {
            MergeMode::Forced => self.value = value,
            MergeMode::Additive => self.iadd.add(value, 1.0),
            MergeMode::Multiplicative => self.imul.add(value, 1.0),
            MergeMode::Weighted => self.imul.add(value, weight),
            MergeMode::Passthrough => {}
        }
    }

    /// Pushes an offset on one axis (the reference
    /// `pushIOffsetAxis`).
    pub fn push_offset_axis(&mut self, axis: i32, value: f32, mode: MergeMode, weight: f32) {
        let mode = if mode == MergeMode::Passthrough {
            self.merge_mode
        } else {
            mode
        };
        match mode {
            MergeMode::Forced => {
                if axis == 0 {
                    self.value.x = value;
                } else {
                    self.value.y = value;
                }
            }
            MergeMode::Additive => self.iadd.add_axis(axis, value, 1.0),
            MergeMode::Multiplicative => self.imul.add_axis(axis, value, 1.0),
            MergeMode::Weighted => self.imul.add_axis(axis, value, weight),
            MergeMode::Passthrough => {}
        }
    }

    /// The head of the reference `Parameter.update`:
    /// recomputes the internal value. Inactive parameters
    /// return false and keep their combinators — the
    /// reference returns before clearing them.
    pub fn update_head(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.last_internal = (self.value + self.iadd.csum()) * self.imul.avg();
        true
    }

    /// Clears the combinators (the reference end of update).
    pub fn update_tail(&mut self) {
        self.iadd.clear();
        self.imul.clear();
    }
}

fn binding_from_payload(
    raw: &Value,
    param_name: &str,
    axis_points: &[Vec<f32>; 2],
) -> Result<Binding, ParamDecodeError> {
    let node_ref = jnum(raw, "node", 0.0) as u32;
    let is_deform = param_name == "deform";
    let grid_shape = |value: &Value| -> Vec<usize> {
        match value {
            Value::Array(rows) => rows
                .iter()
                .map(|row| match row {
                    Value::Array(cells) => cells.len(),
                    _ => 0,
                })
                .collect(),
            _ => Vec::new(),
        }
    };
    let values_raw = jget(raw, "values");
    let shape = grid_shape(values_raw);
    let expect = [axis_points[0].len(), axis_points[1].len()];
    if shape.len() != expect[0] || shape.iter().any(|&cols| cols != expect[1]) {
        return Err(ParamDecodeError(format!(
            "binding '{param_name}' grid {}x{} does not match axis point counts {}x{}",
            shape.len(),
            shape.first().copied().unwrap_or(0),
            expect[0],
            expect[1]
        )));
    }
    let values = if is_deform {
        let grid = match values_raw {
            Value::Array(rows) => rows
                .iter()
                .map(|row| match row {
                    Value::Array(cells) => cells
                        .iter()
                        .map(|cell| match cell {
                            Value::Array(points) => points
                                .iter()
                                .map(|point| vec2_of(point))
                                .collect::<Deformation>(),
                            _ => Vec::new(),
                        })
                        .collect(),
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        };
        BindingValues::Deform(grid)
    } else {
        let grid = match values_raw {
            Value::Array(rows) => rows
                .iter()
                .map(|row| match row {
                    Value::Array(cells) => cells.iter().map(num).collect(),
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        };
        BindingValues::Scalar(grid)
    };
    let is_set = match jget(raw, "isSet") {
        Value::Array(rows) => rows
            .iter()
            .map(|row| match row {
                Value::Array(cells) => cells
                    .iter()
                    .map(|cell| matches!(cell, Value::Bool(true)))
                    .collect(),
                _ => Vec::new(),
            })
            .collect(),
        _ => shape.iter().map(|&cols| vec![false; cols]).collect(),
    };
    let interpolate_mode = match jget(raw, "interpolate_mode") {
        Value::String(text) => InterpolateMode::parse(text).unwrap_or(InterpolateMode::Linear),
        _ => InterpolateMode::Linear,
    };
    Ok(Binding {
        node_ref,
        node: None,
        param_name: param_name.to_string(),
        values,
        is_set,
        interpolate_mode,
    })
}

/// The reference Catmull-Rom cubic on scalars.
fn cubic(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    0.5 * (2.0 * p1
        + (-p0 + p3) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - 3.0 * p3) * t * t
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
}

/// The reference Catmull-Rom cubic on deformations
/// (componentwise, via the deformation arithmetic).
fn cubic_deform(
    p0: &Deformation,
    p1: &Deformation,
    p2: &Deformation,
    p3: &Deformation,
    t: f32,
) -> Deformation {
    let n = p1.len().max(p0.len()).max(p2.len()).max(p3.len());
    (0..n)
        .map(|i| {
            let at = |d: &Deformation| d.get(i).copied().unwrap_or(Vec2::ZERO);
            Vec2::new(
                cubic(at(p0).x, at(p1).x, at(p2).x, at(p3).x, t),
                cubic(at(p0).y, at(p1).y, at(p2).y, at(p3).y, t),
            )
        })
        .collect()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

impl Binding {
    /// Interpolates a scalar keyform at (index, offset).
    /// Grid reads are clamped to the grid the loader
    /// enforced — identical to the reference for every grid
    /// that satisfies the load-time dimension contract.
    pub fn interpolate_f32(&self, index: [usize; 2], offset: Vec2, param: &Parameter) -> f32 {
        let BindingValues::Scalar(values) = &self.values else {
            return 0.0;
        };
        let get = |x: usize, y: usize| -> f32 {
            values.get(x).and_then(|row| row.get(y)).copied().unwrap_or(0.0)
        };
        match self.interpolate_mode {
            InterpolateMode::Nearest => {
                let px = index[0] + usize::from(offset.x >= 0.5);
                if param.is_vec2 {
                    let py = index[1] + usize::from(offset.y >= 0.5);
                    get(px, py)
                } else {
                    get(px, 0)
                }
            }
            InterpolateMode::Linear => {
                if param.is_vec2 {
                    let p00 = get(index[0], index[1]);
                    let p01 = get(index[0], index[1] + 1);
                    let p10 = get(index[0] + 1, index[1]);
                    let p11 = get(index[0] + 1, index[1] + 1);
                    let p0 = lerp(p00, p01, offset.y);
                    let p1 = lerp(p10, p11, offset.y);
                    lerp(p0, p1, offset.x)
                } else {
                    let p0 = get(index[0], 0);
                    let p1 = get(index[0] + 1, 0);
                    lerp(p0, p1, offset.x)
                }
            }
            InterpolateMode::Cubic => {
                let xlen = values.len().saturating_sub(1);
                if param.is_vec2 {
                    let ylen = values.first().map(|row| row.len().saturating_sub(1)).unwrap_or(0);
                    let mut out = [0.0f32; 4];
                    for (row, slot) in out.iter_mut().enumerate() {
                        let yp = (index[1] as i64 + row as i64 - 1).clamp(0, ylen as i64) as usize;
                        *slot = cubic(
                            get(index[0].saturating_sub(1), yp),
                            get(index[0], yp),
                            get(index[0] + 1, yp),
                            get((index[0] + 2).min(xlen), yp),
                            offset.x,
                        );
                    }
                    cubic(out[0], out[1], out[2], out[3], offset.y)
                } else {
                    cubic(
                        get(index[0].saturating_sub(1), 0),
                        get(index[0], 0),
                        get(index[0] + 1, 0),
                        get((index[0] + 2).min(xlen), 0),
                        offset.x,
                    )
                }
            }
            InterpolateMode::Stepped | InterpolateMode::Bezier => 0.0,
        }
    }

    /// Interpolates a deformation keyform at (index, offset).
    pub fn interpolate_deform(
        &self,
        index: [usize; 2],
        offset: Vec2,
        param: &Parameter,
    ) -> Deformation {
        let BindingValues::Deform(values) = &self.values else {
            return Vec::new();
        };
        let empty = Deformation::new();
        let get = |x: usize, y: usize| -> &Deformation {
            values.get(x).and_then(|row| row.get(y)).unwrap_or(&empty)
        };
        match self.interpolate_mode {
            InterpolateMode::Nearest => {
                let px = index[0] + usize::from(offset.x >= 0.5);
                if param.is_vec2 {
                    let py = index[1] + usize::from(offset.y >= 0.5);
                    get(px, py).clone()
                } else {
                    get(px, 0).clone()
                }
            }
            InterpolateMode::Linear => {
                if param.is_vec2 {
                    let p0 = deform_lerp(get(index[0], index[1]), get(index[0], index[1] + 1), offset.y);
                    let p1 = deform_lerp(
                        get(index[0] + 1, index[1]),
                        get(index[0] + 1, index[1] + 1),
                        offset.y,
                    );
                    deform_lerp(&p0, &p1, offset.x)
                } else {
                    deform_lerp(get(index[0], 0), get(index[0] + 1, 0), offset.x)
                }
            }
            InterpolateMode::Cubic => {
                let xlen = values.len().saturating_sub(1);
                if param.is_vec2 {
                    let ylen = values.first().map(|row| row.len().saturating_sub(1)).unwrap_or(0);
                    let mut out: Vec<Deformation> = Vec::with_capacity(4);
                    for row in 0..4 {
                        let yp = (index[1] as i64 + row as i64 - 1).clamp(0, ylen as i64) as usize;
                        out.push(cubic_deform(
                            get(index[0].saturating_sub(1), yp),
                            get(index[0], yp),
                            get(index[0] + 1, yp),
                            get((index[0] + 2).min(xlen), yp),
                            offset.x,
                        ));
                    }
                    cubic_deform(&out[0], &out[1], &out[2], &out[3], offset.y)
                } else {
                    cubic_deform(
                        get(index[0].saturating_sub(1), 0),
                        get(index[0], 0),
                        get(index[0] + 1, 0),
                        get((index[0] + 2).min(xlen), 0),
                        offset.x,
                    )
                }
            }
            InterpolateMode::Stepped | InterpolateMode::Bezier => Vec::new(),
        }
    }
}

fn deform_lerp(a: &Deformation, b: &Deformation, t: f32) -> Deformation {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| {
            let av = a.get(i).copied().unwrap_or(Vec2::ZERO);
            let bv = b.get(i).copied().unwrap_or(Vec2::ZERO);
            av.lerp(bv, t)
        })
        .collect()
}

/// Applies a binding's interpolated value into the node
/// arena: deformation bindings push into the drawable's
/// deformation (a length mismatch against the vertex count
/// is the reference's enforce — reported as an error);
/// value bindings call the node's `set_value`.
pub fn apply_binding(
    nodes: &mut NodeArena,
    binding: &Binding,
    param: &Parameter,
) -> Result<(), String> {
    let Some(node_idx) = binding.node else {
        return Ok(());
    };
    let mapped = param.map_value(param.last_internal);
    let (offset, index) = param.find_offset(mapped);
    if binding.is_deformation() {
        let deform = binding.interpolate_deform(index, offset, param);
        let node = &mut nodes.nodes[node_idx];
        let vertex_count = node.mesh.as_ref().map(|m| m.vertices.len()).unwrap_or(0);
        if deform.len() != vertex_count {
            return Err(format!(
                "binding deformation length {} does not match vertex count {}",
                deform.len(),
                vertex_count
            ));
        }
        for (i, delta) in deform.iter().enumerate() {
            node.deformation[i] += *delta;
        }
        return Ok(());
    }
    let value = binding.interpolate_f32(index, offset, param);
    nodes.set_value(node_idx, &binding.param_name, value);
    Ok(())
}

/// Whether a node provides a named value property (the
/// reference `hasParam` chain).
pub fn node_has_param(nodes: &NodeArena, idx: usize, key: &str) -> bool {
    match key {
        "zSort" | "transform.t.x" | "transform.t.y" | "transform.t.z" | "transform.r.x"
        | "transform.r.y" | "transform.r.z" | "transform.s.x" | "transform.s.y" => return true,
        _ => {}
    }
    match nodes.nodes[idx].kind {
        NodeKind::Part | NodeKind::AnimatedPart => matches!(
            key,
            "alphaThreshold"
                | "opacity"
                | "tint.r"
                | "tint.g"
                | "tint.b"
                | "screenTint.r"
                | "screenTint.g"
                | "screenTint.b"
                | "emissionStrength"
        ),
        NodeKind::Composite => matches!(
            key,
            "opacity" | "tint.r" | "tint.g" | "tint.b" | "screenTint.r" | "screenTint.g"
                | "screenTint.b"
        ),
        NodeKind::SimplePhysics => matches!(
            key,
            "gravity" | "length" | "frequency" | "angleDamping" | "lengthDamping" | "outputScale.x"
                | "outputScale.y"
        ),
        _ => false,
    }
}

/// The reference `getDefaultValue` for a node property
/// (NaN when the property does not exist).
pub fn node_default_value(nodes: &NodeArena, idx: usize, key: &str) -> f32 {
    match key {
        "zSort" | "transform.t.x" | "transform.t.y" | "transform.t.z" | "transform.r.x"
        | "transform.r.y" | "transform.r.z" => return 0.0,
        "transform.s.x" | "transform.s.y" => return 1.0,
        _ => {}
    }
    match nodes.nodes[idx].kind {
        NodeKind::Part | NodeKind::AnimatedPart => match key {
            "alphaThreshold" => 0.0,
            "opacity" => 1.0,
            "tint.r" | "tint.g" | "tint.b" => 1.0,
            "screenTint.r" | "screenTint.g" | "screenTint.b" => 0.0,
            "emissionStrength" => 1.0,
            _ => f32::NAN,
        },
        NodeKind::Composite => match key {
            "opacity" | "tint.r" | "tint.g" | "tint.b" => 1.0,
            "screenTint.r" | "screenTint.g" | "screenTint.b" => 0.0,
            _ => f32::NAN,
        },
        NodeKind::SimplePhysics => match key {
            "gravity" | "frequency" | "angleDamping" | "lengthDamping" | "outputScale.x"
            | "outputScale.y" => 1.0,
            "length" => 0.0,
            _ => f32::NAN,
        },
        _ => f32::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::{Combinator, MergeMode, Parameter};
    use crate::math::Vec2;
    use serde_json::json;

    #[test]
    fn combinator_avg_divides_by_count_not_weight_sum() {
        let mut c = Combinator::default();
        c.add(Vec2::new(2.0, 4.0), 0.5);
        c.add(Vec2::new(4.0, 8.0), 0.5);
        // (2*0.5 + 4*0.5) / 2 = 1.5, (4*0.5 + 8*0.5) / 2 = 3
        assert_eq!(c.avg(), Vec2::new(1.5, 3.0));
        assert_eq!(c.csum(), Vec2::new(6.0, 12.0));
    }

    #[test]
    fn axis_push_carries_one_on_the_other_axis() {
        let mut c = Combinator::default();
        c.add_axis(0, 5.0, 1.0);
        assert_eq!(c.csum(), Vec2::new(5.0, 1.0));
    }

    #[test]
    fn param_decodes_with_defaults() {
        let raw = json!({
            "uuid": 7, "name": "P", "is_vec2": false,
            "min": [0, 0], "max": [1, 1], "defaults": [0.25, 0],
            "axis_points": [[0, 1], [0]], "bindings": []
        });
        let p = Parameter::from_payload(&raw).expect("decode");
        assert_eq!(p.merge_mode, MergeMode::Additive);
        assert_eq!(p.defaults, Vec2::new(0.25, 0.0));
        assert_eq!(p.axis_points[0], vec![0.0, 1.0]);
    }
}
