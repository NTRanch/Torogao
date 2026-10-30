//! The rig exporter (rigexport.d): INP -> lumen-rig model
//! conversion. Every source construct is MAPPED,
//! APPROXIMATED, or DROPPED-WITH-REASON in the conversion
//! report; the exporter mechanically accounts for every
//! parameter binding, so silent loss is impossible by
//! construction.
//!
//! Orientation: INP space is y-down, lumen model space is
//! y-up. All baked positions flip y; rotation angles flip
//! sign. The model document is a lumen-rig model — the
//! format is lumen-rig-core's SPEC — so it carries lumen's
//! format token (`lumen-rig-model`): the one-token law
//! (Pax ruling 2026-10-29, superseding RP1-D1 for this
//! bridge). Torogao's native token
//! (`torogao_format::MODEL_FORMAT_TOKEN`) governs
//! torogao-authored model files, not bridge exports; the
//! reference exporter writes the lumen token too, so the
//! two exporters agree on this field.

use crate::extdoc::ExtView;
use crate::handlers::{ToolLoad, join_path, stamp_doc};
use crate::registry::ToolResult;
use crate::util::{f64_of_f32, fmt_g};
use crate::walk::{Walk, grid_dims, is_part, slugify, suffixed_name};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use torogao_engine::param::BindingValues;
use torogao_engine::{Node, Puppet, Vec2};

/// The format token the exported model document carries:
/// `lumen-rig-model`. This module is the INP -> lumen
/// bridge and the format is lumen-rig-core's SPEC, so the
/// export conforms to lumen's token (the one-token law);
/// torogao's native token stays with torogao-authored
/// model files. The motion documents this exporter emits
/// likewise carry lumen's `lumen-rig-motion` token.
pub const LUMEN_MODEL_FORMAT_TOKEN: &str = "lumen-rig-model";

/// The smallest scale component a lumen rotation keyform
/// may carry. SPEC section 3.4 requires sx, sy > 0
/// (LRF-305), while INP encodes a part's "feature off"
/// state as scale 0; non-positive keyform components
/// export at this floor instead (see
/// `keyform_scale_component`).
const KEYFORM_SCALE_FLOOR: f32 = 1e-6;

/// One rotation keyform scale component as lumen may
/// carry it: positive components pass through unchanged;
/// a non-positive component (INP's "feature off" states)
/// exports at [`KEYFORM_SCALE_FLOOR`], because lumen
/// requires sx, sy > 0 (SPEC section 3.4, LRF-305).
fn keyform_scale_component(value: f32) -> f32 {
    if value <= 0.0 {
        KEYFORM_SCALE_FLOOR
    } else {
        value
    }
}

/// How an export can fail: the conversion itself, or the
/// file writes after it.
pub enum ExportError {
    /// The exporter refused or failed mid-conversion.
    Build(String),
    /// Writing an export file failed.
    Io(String),
}

/// One conversion report entry.
#[derive(Clone)]
pub struct ConvEntry {
    /// "mapped" | "approximated" | "dropped".
    pub verdict: &'static str,
    /// What the source construct is.
    pub construct: String,
    /// How it was handled, or why it was dropped.
    pub detail: String,
}

impl ConvEntry {
    fn to_json(&self) -> Value {
        serde_json::json!({
            "construct": self.construct,
            "detail": self.detail,
            "verdict": self.verdict,
        })
    }
}

/// The conversion outcome: model document plus entries.
pub struct ExportResult {
    /// The rig model document.
    pub model: Value,
    /// The motion asset documents (SPEC section 4), as
    /// (file name suffix, document) pairs — a separate
    /// asset kind from the model, written beside it.
    pub motions: Vec<(String, Value)>,
    /// The conversion entries, in note order.
    pub entries: Vec<ConvEntry>,
}

impl ExportResult {
    fn count(&self, verdict: &str) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.verdict == verdict)
            .count()
    }
}

/// Radians → degrees quotient at extended precision:
/// the product `value * 180.0f` rounds to a float first,
/// the division by π then runs at extended precision and
/// is NOT rounded back to a float here — the reference
/// keeps the quotient at `real` precision into the angle
/// arithmetic, and only the stored angle rounds. (The
/// product-first rounding is verified against the
/// reference export; keeping the product exact, or
/// dividing at f32 precision, lands one ulp off.)
fn degrees_quotient(value: f32) -> f64 {
    f64::from(value * 180.0f32) / std::f64::consts::PI
}

/// The deformer's base angle: -(z * 180 / π), the
/// reference's `-(rotation.z * 180.0f / PI)` float —
/// this one IS a float variable, so it rounds here.
fn base_degrees(rotation_z: f32) -> f32 {
    -(degrees_quotient(rotation_z) as f32)
}

/// One `angle -= value * 180.0f / PI` step: the
/// subtraction runs at extended precision against the
/// unrounded quotient and the result rounds back into
/// the float angle.
fn angle_minus_degrees(angle: f32, radians: f32) -> f32 {
    (f64::from(angle) - degrees_quotient(radians)) as f32
}

fn jv(value: f32) -> Value {
    f64_of_f32(value)
}

fn jv2(x: f32, y: f32) -> Value {
    Value::Array(vec![f64_of_f32(x), f64_of_f32(y)])
}

fn jv2f(point: [f32; 2]) -> Value {
    jv2(point[0], point[1])
}

/// One lattice axis: a scalar parameter channel + keys.
struct AxisSrc {
    param: usize,
    axis: usize,
    scalar_id: Option<String>,
    keys: Vec<f32>,
}

fn strictly_increasing(values: &[f32]) -> bool {
    values.windows(2).all(|pair| pair[1] > pair[0])
}

/// A binding reference: (parameter index, binding index).
type BindingRef = (usize, usize);

/// The exporter state.
pub struct RigExporter<'a> {
    puppet: &'a Puppet,
    walk: &'a mut Walk,
    slot_count: usize,
    entries: Vec<ConvEntry>,
    consumed: HashSet<BindingRef>,
    param_x_id: HashMap<usize, String>,
    param_y_id: HashMap<usize, String>,
    param_dropped: HashSet<usize>,
    draw_rank: HashMap<u32, usize>,
    anchor_point: HashMap<u32, [f32; 2]>,
    param_array: Value,
    motions: Vec<(String, Value)>,
    physics_outputs: HashSet<String>,
}

impl<'a> RigExporter<'a> {
    /// Runs the conversion in the reference's order.
    pub fn run(
        puppet: &'a Puppet,
        walk: &'a mut Walk,
        slot_count: usize,
    ) -> Result<ExportResult, String> {
        let mut exporter = RigExporter {
            puppet,
            walk,
            slot_count,
            entries: Vec::new(),
            consumed: HashSet::new(),
            param_x_id: HashMap::new(),
            param_y_id: HashMap::new(),
            param_dropped: HashSet::new(),
            draw_rank: HashMap::new(),
            anchor_point: HashMap::new(),
            param_array: Value::Null,
            motions: Vec::new(),
            physics_outputs: HashSet::new(),
        };
        let model_name = if puppet.meta.name.is_empty() {
            "model".to_string()
        } else {
            slugify(&puppet.meta.name, 48)
        };
        exporter.emit_parameters();
        exporter.assign_draw_ranks();
        let deformers = exporter.emit_deformers();
        let parts = exporter.emit_parts();
        let textures = exporter.emit_textures();
        let meshes = exporter.emit_meshes();
        let physics = exporter.emit_physics();
        exporter.emit_motions(&model_name);
        exporter.account_bindings();
        exporter.account_whole_constructs();
        let model = serde_json::json!({
            "deformers": deformers,
            "format": LUMEN_MODEL_FORMAT_TOKEN,
            "format_version": 1,
            "generator": "torogao-cli 0.1.0",
            "id": model_name,
            "meshes": meshes,
            "parameters": exporter.param_array.clone(),
            "parts": parts,
            "physics": physics,
            "textures": textures,
        });
        Ok(ExportResult {
            model,
            motions: exporter.motions,
            entries: exporter.entries,
        })
    }

    fn note(&mut self, verdict: &'static str, construct: String, detail: String) {
        self.entries.push(ConvEntry {
            verdict,
            construct,
            detail,
        });
    }

    fn node_of(&self, walk_idx: usize) -> &'a Node {
        &self.puppet.nodes.nodes[self.walk.nodes[walk_idx].node]
    }

    // ------------------------------------------------ parameters
    fn emit_parameters(&mut self) {
        let mut arr = Vec::new();
        let mut used_ids: HashSet<String> = HashSet::new();
        for (param_idx, param) in self.puppet.params.iter().enumerate() {
            let base = format!("{}_{}", slugify(&param.name, 44), param.uuid);
            let degenerate = param.min.x >= param.max.x
                || param.defaults.x < param.min.x
                || param.defaults.x > param.max.x
                || (param.is_vec2
                    && (param.min.y >= param.max.y
                        || param.defaults.y < param.min.y
                        || param.defaults.y > param.max.y));
            if degenerate {
                self.param_dropped.insert(param_idx);
                self.note(
                    "dropped",
                    format!("parameter '{}'", param.name),
                    "parameter range is degenerate or its default lies outside it; lumen parameters require min < max with the default inside (LRF-302/303)"
                        .to_string(),
                );
                continue;
            }
            let mut xid = format!("{base}_x");
            if used_ids.contains(&xid) {
                xid = format!("{base}_x2");
            }
            used_ids.insert(xid.clone());
            self.param_x_id.insert(param_idx, xid.clone());
            arr.push(serde_json::json!({
                "default": jv(param.defaults.x),
                "id": xid,
                "kind": "scalar",
                "max": jv(param.max.x),
                "min": jv(param.min.x),
            }));
            if param.is_vec2 {
                let mut yid = format!("{base}_y");
                if used_ids.contains(&yid) {
                    yid = format!("{base}_y2");
                }
                used_ids.insert(yid.clone());
                self.param_y_id.insert(param_idx, yid.clone());
                arr.push(serde_json::json!({
                    "default": jv(param.defaults.y),
                    "id": yid,
                    "kind": "scalar",
                    "max": jv(param.max.y),
                    "min": jv(param.min.y),
                }));
            }
            if !matches!(
                param.merge_mode,
                torogao_engine::MergeMode::Additive | torogao_engine::MergeMode::Passthrough
            ) {
                self.note(
                    "approximated",
                    format!("parameter '{}'", param.name),
                    format!(
                        "merge mode '{}' is a runtime composition rule in INP; lumen v1 has no equivalent (combined parameters are explicit terms), so the parameter exports as plain scalar channel(s)",
                        merge_mode_name(param.merge_mode)
                    ),
                );
            }
        }
        self.param_array = Value::Array(arr);
    }

    // ------------------------------------------------ draw order
    fn assign_draw_ranks(&mut self) {
        let mut rank = 0usize;
        let root_parts: Vec<usize> = self.puppet.root_parts.clone();
        for root_part in root_parts {
            let Some(walk_idx) = self.walk.index_of_node(self.puppet, root_part) else {
                continue;
            };
            let kind = self.node_of(walk_idx).kind;
            if kind == torogao_format::NodeKind::Composite {
                self.assign_subtree_parts(walk_idx, &mut rank);
            } else if is_part(kind) {
                let uuid = self.node_of(walk_idx).uuid;
                self.draw_rank.insert(uuid, rank);
                rank += 1;
            }
        }
        for walk_idx in 0..self.walk.nodes.len() {
            let node = self.node_of(walk_idx);
            if is_part(node.kind) && !self.draw_rank.contains_key(&node.uuid) {
                self.draw_rank.insert(node.uuid, rank);
                rank += 1;
            }
        }
    }

    fn assign_subtree_parts(&mut self, walk_idx: usize, rank: &mut usize) {
        let node = self.node_of(walk_idx);
        if is_part(node.kind) {
            self.draw_rank.insert(node.uuid, *rank);
            *rank += 1;
        }
        let children = self.walk.nodes[walk_idx].children.clone();
        for child in children {
            self.assign_subtree_parts(child, rank);
        }
    }

    // ------------------------------------------------ lattices
    fn collect_axes(&self, bindings: &[BindingRef]) -> Result<Vec<AxisSrc>, String> {
        let mut axes: Vec<AxisSrc> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for &(param_idx, _) in bindings {
            let param = &self.puppet.params[param_idx];
            let channels: &[usize] = if param.is_vec2 { &[0, 1] } else { &[0] };
            for &channel in channels {
                let key = format!("{}/{channel}", param.uuid);
                if !seen.insert(key) {
                    continue;
                }
                let scalar_id = if channel == 0 {
                    self.param_x_id.get(&param_idx).cloned()
                } else {
                    self.param_y_id.get(&param_idx).cloned()
                };
                axes.push(AxisSrc {
                    param: param_idx,
                    axis: channel,
                    scalar_id,
                    keys: param.axis_points[channel].clone(),
                });
            }
        }
        if axes.is_empty() {
            return Err("no usable parameter axes".to_string());
        }
        if axes.len() > 2 {
            return Err(format!(
                "bindings span {} parameter axes; a lumen lattice has at most 2",
                axes.len()
            ));
        }
        for axis in &axes {
            let Some(scalar_id) = &axis.scalar_id else {
                return Err("an axis parameter was dropped during parameter conversion".to_string());
            };
            if axis.keys.len() < 2 || axis.keys.len() > 32 {
                return Err(format!(
                    "axis '{}' has {} keypoints; a lumen lattice axis needs 2..=32",
                    scalar_id,
                    axis.keys.len()
                ));
            }
            if !strictly_increasing(&axis.keys) {
                return Err(format!(
                    "axis '{}' keypoints are not strictly increasing",
                    scalar_id
                ));
            }
            let param = &self.puppet.params[axis.param];
            let (lo, hi) = if axis.axis == 0 {
                (param.min.x, param.max.x)
            } else {
                (param.min.y, param.max.y)
            };
            for key in &axis.keys {
                if *key < lo || *key > hi {
                    return Err(format!(
                        "axis '{}' keypoint {} lies outside the parameter range",
                        scalar_id,
                        fmt_g(*key as f64)
                    ));
                }
            }
        }
        Ok(axes)
    }

    fn binding_indices(
        &self,
        binding: BindingRef,
        axes: &[AxisSrc],
        i: usize,
        j: usize,
    ) -> (usize, usize) {
        let (mut bx, mut by) = (0usize, 0usize);
        for (ai, axis) in axes.iter().enumerate() {
            let idx = if ai == 0 { i } else { j };
            if axis.param == binding.0 {
                if axis.axis == 0 {
                    bx = idx;
                } else {
                    by = idx;
                }
            }
        }
        (bx, by)
    }

    // ------------------------------------------------ binding access
    fn value_at(&self, binding: BindingRef, bx: usize, by: usize) -> f32 {
        let binding = &self.puppet.params[binding.0].bindings[binding.1];
        match &binding.values {
            BindingValues::Scalar(grid) => grid
                .get(bx)
                .and_then(|row| row.get(by))
                .copied()
                .unwrap_or(0.0),
            BindingValues::Deform(_) => 0.0,
        }
    }

    fn deform_offset_at(
        &self,
        binding: BindingRef,
        bx: usize,
        by: usize,
        vertex_index: usize,
    ) -> Vec2 {
        let binding = &self.puppet.params[binding.0].bindings[binding.1];
        match &binding.values {
            BindingValues::Deform(grid) => grid
                .get(bx)
                .and_then(|row| row.get(by))
                .and_then(|cell| cell.get(vertex_index))
                .copied()
                .unwrap_or(Vec2::new(0.0, 0.0)),
            BindingValues::Scalar(_) => Vec2::new(0.0, 0.0),
        }
    }

    fn channel_bindings(&self, node_idx: usize, prop: &str) -> Vec<BindingRef> {
        let mut out = Vec::new();
        for (param_idx, param) in self.puppet.params.iter().enumerate() {
            if self.param_dropped.contains(&param_idx) {
                continue;
            }
            for (binding_idx, binding) in param.bindings.iter().enumerate() {
                if binding.node == Some(node_idx)
                    && binding.param_name == prop
                    && matches!(binding.values, BindingValues::Scalar(_))
                {
                    out.push((param_idx, binding_idx));
                }
            }
        }
        out
    }

    fn deform_bindings_of(&self, node_idx: usize) -> Vec<BindingRef> {
        let mut out = Vec::new();
        for (param_idx, param) in self.puppet.params.iter().enumerate() {
            if self.param_dropped.contains(&param_idx) {
                continue;
            }
            for (binding_idx, binding) in param.bindings.iter().enumerate() {
                if binding.node == Some(node_idx)
                    && matches!(binding.values, BindingValues::Deform(_))
                {
                    out.push((param_idx, binding_idx));
                }
            }
        }
        out
    }

    fn binding_construct(&self, binding: BindingRef) -> String {
        let param = &self.puppet.params[binding.0];
        let binding = &param.bindings[binding.1];
        let target = match binding.node {
            Some(node_idx) => self
                .walk
                .index_of_node(self.puppet, node_idx)
                .map(|walk_idx| self.walk.path_of(self.puppet, walk_idx))
                .unwrap_or_else(|| "?".to_string()),
            None => "?".to_string(),
        };
        format!(
            "binding '{}' -> '{}'.{}",
            param.name, target, binding.param_name
        )
    }

    fn lattice_json(&self, axes: &[AxisSrc], forms: Vec<Value>) -> Value {
        let axis_ids: Vec<Value> = axes
            .iter()
            .map(|axis| Value::String(axis.scalar_id.clone().unwrap_or_default()))
            .collect();
        let keys: Vec<Value> = axes
            .iter()
            .map(|axis| Value::Array(axis.keys.iter().map(|key| jv(*key)).collect()))
            .collect();
        serde_json::json!({
            "axes": axis_ids,
            "forms": forms,
            "keys": keys,
        })
    }
}

/// The D enum member name of a merge mode (report text).
fn merge_mode_name(mode: torogao_engine::MergeMode) -> &'static str {
    match mode {
        torogao_engine::MergeMode::Additive => "Additive",
        torogao_engine::MergeMode::Weighted => "Weighted",
        torogao_engine::MergeMode::Multiplicative => "Multiplicative",
        torogao_engine::MergeMode::Forced => "Forced",
        torogao_engine::MergeMode::Passthrough => "Passthrough",
    }
}

/// The physics model member name.
pub fn physics_model_name(spring: bool) -> &'static str {
    if spring { "SpringPendulum" } else { "Pendulum" }
}

/// The parameter map mode member name.
pub fn map_mode_name(map_mode: u8) -> &'static str {
    match map_mode {
        1 => "XY",
        2 => "LengthAngle",
        3 => "YX",
        _ => "AngleLength",
    }
}

/// Whether a node renders: itself and every ancestor
/// enabled (the reference's recursive renderEnabled).
pub fn render_enabled(puppet: &Puppet, node_idx: usize) -> bool {
    let node = &puppet.nodes.nodes[node_idx];
    if !node.enabled {
        return false;
    }
    match node.parent {
        Some(parent) => render_enabled(puppet, parent),
        None => true,
    }
}

/// The blend-mode slug for a part, noting the AddGlow
/// approximation and unknown-mode drops as the reference
/// does. Returns the lumen blend_mode value.
pub fn blend_mode_for(
    mode_name: &str,
    path: &str,
    note: &mut dyn FnMut(&'static str, String, String),
) -> &'static str {
    match mode_name {
        "Normal" => "normal",
        "Multiply" => "multiply",
        "Screen" => "screen",
        "Overlay" => "overlay",
        "Darken" => "darken",
        "Lighten" => "lighten",
        "ColorDodge" => "color_dodge",
        "ColorBurn" => "color_burn",
        "HardLight" => "hard_light",
        "SoftLight" => "soft_light",
        "Difference" => "difference",
        "Exclusion" => "exclusion",
        "LinearDodge" => "linear_dodge",
        "AddGlow" => {
            note(
                "approximated",
                format!("Part '{path}' blend mode"),
                "AddGlow exports as linear_dodge; the glow falloff is not carried".to_string(),
            );
            "linear_dodge"
        }
        other => {
            note(
                "dropped",
                format!("Part '{path}' blend mode"),
                format!(
                    "blend mode {other} has no lumen v1 equivalent; the mesh exports as normal"
                ),
            );
            "normal"
        }
    }
}

// The deformer / part / texture / mesh / physics emitters
// and the final accounting live in the second half of
// this module (impl block below).
impl<'a> RigExporter<'a> {
    // ------------------------------------------------ deformers
    fn emit_deformers(&mut self) -> Value {
        let mut arr = Vec::new();
        for walk_idx in 0..self.walk.nodes.len() {
            if let Some(rot) = self.build_rotation(walk_idx) {
                arr.push(rot);
            }
        }
        for walk_idx in 0..self.walk.nodes.len() {
            if self.walk.nodes[walk_idx].warp_id.is_none() {
                continue;
            }
            if let Some(warp) = self.build_warp(walk_idx) {
                arr.push(warp);
            }
        }
        // MeshGroups without a grid cannot be lumen warps.
        for walk_idx in 0..self.walk.nodes.len() {
            let node = self.node_of(walk_idx);
            if node.kind != torogao_format::NodeKind::MeshGroup {
                continue;
            }
            if self.walk.nodes[walk_idx].warp_id.is_some() {
                continue;
            }
            self.note(
                "dropped",
                format!(
                    "MeshGroup '{}'",
                    self.walk.path_of(self.puppet, walk_idx)
                ),
                "the MeshGroup's mesh is not a regular grid; its barycentric triangle warp has no lumen v1 equivalent (lumen warps are grids)"
                    .to_string(),
            );
        }
        Value::Array(arr)
    }

    fn build_rotation(&mut self, walk_idx: usize) -> Option<Value> {
        let node_idx = self.walk.nodes[walk_idx].node;
        let rz = self.channel_bindings(node_idx, "transform.r.z");
        let sx = self.channel_bindings(node_idx, "transform.s.x");
        let sy = self.channel_bindings(node_idx, "transform.s.y");
        if rz.is_empty() && sx.is_empty() && sy.is_empty() {
            return None;
        }
        let all: Vec<BindingRef> = rz
            .iter()
            .chain(sx.iter())
            .chain(sy.iter())
            .copied()
            .collect();
        let axes = match self.collect_axes(&all) {
            Ok(axes) => axes,
            Err(reason) => {
                for binding in &all {
                    self.consumed.insert(*binding);
                    let construct = self.binding_construct(*binding);
                    self.note(
                        "dropped",
                        construct,
                        format!("rotation binding cannot form a lumen lattice: {reason}"),
                    );
                }
                return None;
            }
        };
        let node = self.node_of(walk_idx);
        let rot_id = suffixed_name("rot_", &node.name, node.uuid, 44);
        self.walk.nodes[walk_idx].rot_id = Some(rot_id.clone());
        let world_pos = self.walk.world_pos(walk_idx);
        self.anchor_point.insert(node.uuid, world_pos);

        let base_rz_deg = base_degrees(node.local.rotation.z);
        let base_sx = node.local.scale.x;
        let base_sy = node.local.scale.y;

        let nx = axes[0].keys.len();
        let ny = if axes.len() > 1 {
            axes[1].keys.len()
        } else {
            1
        };
        let mut forms = Vec::new();
        let mut floored_components = 0usize;
        for i in 0..nx {
            for j in 0..ny {
                let mut angle = base_rz_deg;
                let (mut scale_x, mut scale_y) = (base_sx, base_sy);
                for binding in &rz {
                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                    angle = angle_minus_degrees(angle, self.value_at(*binding, bx, by));
                }
                for binding in &sx {
                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                    scale_x *= self.value_at(*binding, bx, by);
                }
                for binding in &sy {
                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                    scale_y *= self.value_at(*binding, bx, by);
                }
                let floored_x = keyform_scale_component(scale_x);
                if floored_x != scale_x {
                    floored_components += 1;
                }
                scale_x = floored_x;
                let floored_y = keyform_scale_component(scale_y);
                if floored_y != scale_y {
                    floored_components += 1;
                }
                scale_y = floored_y;
                let mut at = vec![Value::from(i as u64)];
                if axes.len() > 1 {
                    at.push(Value::from(j as u64));
                }
                forms.push(serde_json::json!({
                    "angle_degrees": jv(angle),
                    "at": at,
                    "scale": jv2(scale_x, scale_y),
                }));
            }
        }
        for binding in &all {
            self.consumed.insert(*binding);
        }
        self.note(
            "approximated",
            format!(
                "rotation of node '{}'",
                self.walk.path_of(self.puppet, walk_idx)
            ),
            "node transform rotation/scale bindings become a lumen rotation deformer; angles are absolute world-space degrees (sign-flipped for the y-up space) and rest vertices are baked with the full rest transform, so runtime evaluation semantics differ from INP's local-transform composition"
                .to_string(),
        );
        if floored_components > 0 {
            self.note(
                "approximated",
                format!(
                    "rotation of node '{}' keyform scale",
                    self.walk.path_of(self.puppet, walk_idx)
                ),
                format!(
                    "{floored_components} keyform scale component(s) are non-positive in INP (its 'feature off' states); lumen requires sx, sy > 0 (SPEC section 3.4, LRF-305), so those components export at the 1e-6 floor"
                ),
            );
        }

        let mut deformer = serde_json::json!({
            "base_angle_degrees": jv(base_rz_deg),
            "id": rot_id,
            "kind": "rotation",
            "origin": jv2f(world_pos),
        });
        if let Some(parent) = self
            .walk
            .nearest_deformer_ancestor(walk_idx)
            .and_then(|ancestor| self.walk.deformer_id_of(ancestor))
        {
            deformer["parent"] = Value::String(parent.to_string());
        }
        deformer["keyforms"] = self.lattice_json(&axes, forms);
        Some(deformer)
    }

    fn build_warp(&mut self, walk_idx: usize) -> Option<Value> {
        let node_idx = self.walk.nodes[walk_idx].node;
        let node = self.node_of(walk_idx);
        let (vertex_count, dims) = match &node.mesh {
            Some(mesh) => (mesh.vertices.len(), grid_dims(&mesh.vertices)),
            None => (0, None),
        };
        let (cols, rows) = match dims {
            Some((cols, rows)) if cols <= 32 && rows <= 32 => (cols, rows),
            _ => {
                self.note(
                    "dropped",
                    format!(
                        "MeshGroup '{}'",
                        self.walk.path_of(self.puppet, walk_idx)
                    ),
                    format!(
                        "mesh vertices ({vertex_count}) do not form a regular grid within the lumen warp limits (1..=32 cells per side); the deformer and its bindings were dropped"
                    ),
                );
                for binding in self.deform_bindings_of(node_idx) {
                    self.consumed.insert(binding);
                }
                self.walk.nodes[walk_idx].warp_id = None;
                return None;
            }
        };
        let world_pos = self.walk.world_pos(walk_idx);
        self.anchor_point.insert(node.uuid, world_pos);

        let mesh = node.mesh.as_ref().expect("grid mesh exists");
        let rest: Vec<Value> = mesh
            .vertices
            .iter()
            .map(|vertex| jv2f(self.walk.to_world(walk_idx, *vertex, mesh.origin)))
            .collect();

        let defs = self.deform_bindings_of(node_idx);
        let mut keyforms = Value::Null;
        if !defs.is_empty() {
            match self.collect_axes(&defs) {
                Err(reason) => {
                    for binding in &defs {
                        self.consumed.insert(*binding);
                        let construct = self.binding_construct(*binding);
                        self.note(
                            "dropped",
                            construct,
                            format!("warp deformation cannot form a lumen lattice: {reason}"),
                        );
                    }
                }
                Ok(axes) => {
                    let nx = axes[0].keys.len();
                    let ny = if axes.len() > 1 {
                        axes[1].keys.len()
                    } else {
                        1
                    };
                    let mut forms = Vec::new();
                    for i in 0..nx {
                        for j in 0..ny {
                            let mut positions = Vec::new();
                            for (vi, vertex) in mesh.vertices.iter().enumerate() {
                                let mut point = self.walk.to_world(walk_idx, *vertex, mesh.origin);
                                for binding in &defs {
                                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                                    let offset = self.deform_offset_at(*binding, bx, by, vi);
                                    let delta = self.walk.delta_to_world(walk_idx, offset);
                                    point[0] += delta[0];
                                    point[1] += delta[1];
                                }
                                positions.push(jv2f(point));
                            }
                            let mut at = vec![Value::from(i as u64)];
                            if axes.len() > 1 {
                                at.push(Value::from(j as u64));
                            }
                            forms.push(serde_json::json!({
                                "at": at,
                                "positions": positions,
                            }));
                        }
                    }
                    for binding in &defs {
                        self.consumed.insert(*binding);
                    }
                    keyforms = self.lattice_json(&axes, forms);
                }
            }
        }
        self.note(
            "mapped",
            format!(
                "MeshGroup '{}'",
                self.walk.path_of(self.puppet, walk_idx)
            ),
            "grid MeshGroup becomes a lumen warp deformer; children deform through the deformer chain (INP applies the same mesh barycentrically — grid case only)"
                .to_string(),
        );

        let warp_id = self.walk.nodes[walk_idx]
            .warp_id
            .clone()
            .expect("warp id present");
        let mut deformer = serde_json::json!({
            "columns": cols,
            "id": warp_id,
            "kind": "warp",
            "rest_positions": rest,
            "rows": rows,
        });
        let parent = if let Some(rot_id) = &self.walk.nodes[walk_idx].rot_id {
            Some(rot_id.clone())
        } else {
            self.walk
                .nearest_deformer_ancestor(walk_idx)
                .and_then(|ancestor| self.walk.deformer_id_of(ancestor))
                .map(str::to_string)
        };
        if let Some(parent) = parent {
            deformer["parent"] = Value::String(parent);
        }
        if !keyforms.is_null() {
            deformer["keyforms"] = keyforms;
        }
        Some(deformer)
    }
}

impl<'a> RigExporter<'a> {
    // ------------------------------------------------ parts
    fn emit_parts(&mut self) -> Value {
        let mut arr = Vec::new();
        for walk_idx in 0..self.walk.nodes.len() {
            let node = self.node_of(walk_idx);
            let mut part = serde_json::json!({
                "id": self.walk.nodes[walk_idx].part_id,
            });
            if let Some(parent) = self.walk.nodes[walk_idx].parent {
                part["parent"] = Value::String(self.walk.nodes[parent].part_id.clone());
            }
            if let Some(lattice) = self.build_part_translation(walk_idx) {
                part["translation_keyforms"] = lattice;
            }
            arr.push(part);
            let world_pos = self.walk.world_pos(walk_idx);
            self.anchor_point.insert(node.uuid, world_pos);
            if node.lock_to_root {
                self.note(
                    "approximated",
                    format!(
                        "node '{}'",
                        self.walk.path_of(self.puppet, walk_idx)
                    ),
                    "lockToRoot is baked into the rest transform; the lock semantics are not carried"
                        .to_string(),
                );
            }
            if node.kind == torogao_format::NodeKind::Composite {
                self.note(
                    "approximated",
                    format!(
                        "Composite '{}'",
                        self.walk.path_of(self.puppet, walk_idx)
                    ),
                    format!(
                        "composite group is flattened: children reparent to the composite's parent part, its opacity is folded into descendant mesh opacity, and its blend mode ({}) is dropped (render-to-texture groups have no lumen v1 equivalent)",
                        node.blend_mode
                    ),
                );
            }
        }
        Value::Array(arr)
    }

    /// BIND-1: a node's `transform.t.x` / `transform.t.y`
    /// bindings become a `translation_keyforms` lattice on
    /// the node's part (lumen SPEC section 3.2, the BIND-1
    /// amendment). Each lattice point's payload is the
    /// bindings' summed local delta carried into model
    /// space at rest orientation — the same
    /// `delta_to_world` conversion the mesh-deformation
    /// exporter applies to deform offsets. Returns the
    /// lattice document, or None when the node has no
    /// translation bindings (or they cannot form a lattice,
    /// in which case they are noted dropped and consumed,
    /// exactly as failed rotation lattices are).
    fn build_part_translation(&mut self, walk_idx: usize) -> Option<Value> {
        let node_idx = self.walk.nodes[walk_idx].node;
        let tx = self.channel_bindings(node_idx, "transform.t.x");
        let ty = self.channel_bindings(node_idx, "transform.t.y");
        if tx.is_empty() && ty.is_empty() {
            return None;
        }
        let all: Vec<BindingRef> = tx.iter().chain(ty.iter()).copied().collect();
        let axes = match self.collect_axes(&all) {
            Ok(axes) => axes,
            Err(reason) => {
                for binding in &all {
                    self.consumed.insert(*binding);
                    let construct = self.binding_construct(*binding);
                    self.note(
                        "dropped",
                        construct,
                        format!("translation binding cannot form a lumen lattice: {reason}"),
                    );
                }
                return None;
            }
        };
        let nx = axes[0].keys.len();
        let ny = if axes.len() > 1 {
            axes[1].keys.len()
        } else {
            1
        };
        let mut forms = Vec::new();
        for i in 0..nx {
            for j in 0..ny {
                let (mut dx, mut dy) = (0.0f32, 0.0f32);
                for binding in &tx {
                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                    dx += self.value_at(*binding, bx, by);
                }
                for binding in &ty {
                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                    dy += self.value_at(*binding, bx, by);
                }
                let delta = self.walk.delta_to_world(walk_idx, Vec2::new(dx, dy));
                let mut at = vec![Value::from(i as u64)];
                if axes.len() > 1 {
                    at.push(Value::from(j as u64));
                }
                forms.push(serde_json::json!({
                    "at": at,
                    "translation": jv2(delta[0], delta[1]),
                }));
            }
        }
        for binding in &all {
            self.consumed.insert(*binding);
        }
        self.note(
            "approximated",
            format!(
                "translation of node '{}'",
                self.walk.path_of(self.puppet, walk_idx)
            ),
            "node transform translation bindings become a BIND-1 translation_keyforms lattice on the part; values are model-space translations converted at rest orientation (delta_to_world), so the INP local-frame behaviour — the delta rotating with an ancestor's animated rotation, and descendant deformer pivots moving with the node — is not carried (lumen SPEC section 3.2 names both)"
                .to_string(),
        );
        Some(self.lattice_json(&axes, forms))
    }

    /// Product of Composite ancestors' opacity for a node.
    fn composite_opacity_factor(&self, walk_idx: usize) -> f32 {
        let mut factor = 1.0f32;
        let mut cursor = self.walk.nodes[walk_idx].parent;
        while let Some(parent) = cursor {
            let node = self.node_of(parent);
            if node.kind == torogao_format::NodeKind::Composite {
                factor *= node.opacity;
            }
            cursor = self.walk.nodes[parent].parent;
        }
        factor
    }

    // ------------------------------------------------ textures
    fn emit_textures(&mut self) -> Value {
        let mut arr = Vec::new();
        for (index, slot) in self.puppet.texture_slots.iter().enumerate() {
            let ext = match slot.format.as_str() {
                "png" => "png",
                "tga" => "tga",
                "bc7" => "bc7",
                _ => "bin",
            };
            arr.push(serde_json::json!({
                "id": format!("tex_{index}"),
                "source": format!("textures/slot-{index}.{ext}"),
            }));
        }
        if !arr.is_empty() {
            self.note(
                "mapped",
                "embedded texture slots".to_string(),
                "textures are referenced by generated slot paths (textures/slot-<n>.<fmt>); payloads stay embedded in the source INP and are not extracted by this converter"
                    .to_string(),
            );
        }
        Value::Array(arr)
    }

    // ------------------------------------------------ meshes
    fn emit_meshes(&mut self) -> Value {
        let mut arr = Vec::new();
        let mut dropped_mesh_ids: HashSet<String> = HashSet::new();
        for walk_idx in 0..self.walk.nodes.len() {
            let node = self.node_of(walk_idx);
            if !is_part(node.kind) {
                continue;
            }
            let path = self.walk.path_of(self.puppet, walk_idx);
            let empty_mesh = torogao_engine::MeshData {
                vertices: Vec::new(),
                uvs: Vec::new(),
                indices: Vec::new(),
                origin: Vec2::new(0.0, 0.0),
            };
            let mesh = node.mesh.as_ref().unwrap_or(&empty_mesh);

            let drop_reason: Option<String> = if mesh.vertices.is_empty() {
                Some("part has no vertices".to_string())
            } else if mesh.vertices.len() > 65536 {
                Some(format!(
                    "part has {} vertices; lumen allows at most 65536 per mesh",
                    mesh.vertices.len()
                ))
            } else if mesh.indices.len() < 3 || mesh.indices.len() % 3 != 0 {
                Some(format!(
                    "index list ({}) is not a triangle list of at least 3",
                    mesh.indices.len()
                ))
            } else if mesh.uvs.len() != mesh.vertices.len() {
                Some(format!(
                    "uv count ({}) does not match vertex count ({})",
                    mesh.uvs.len(),
                    mesh.vertices.len()
                ))
            } else if node.texture_ids.is_empty() {
                Some(
                    "part references no texture; a lumen mesh requires a texture reference"
                        .to_string(),
                )
            } else if node.texture_ids[0] >= 0 && (node.texture_ids[0] as usize) >= self.slot_count
            {
                Some(format!(
                    "part's texture slot {} does not exist ({} slots embedded)",
                    node.texture_ids[0], self.slot_count
                ))
            } else {
                None
            };
            if let Some(reason) = drop_reason {
                if let Some(mesh_id) = &self.walk.nodes[walk_idx].mesh_id {
                    dropped_mesh_ids.insert(mesh_id.clone());
                }
                self.note("dropped", format!("Part '{path}'"), reason);
                continue;
            }

            for extra in &node.texture_ids[1..] {
                self.note(
                    "dropped",
                    format!("Part '{path}' secondary texture"),
                    format!(
                        "texture slot {extra} beyond the first: lumen v1 meshes reference exactly one texture (emissive/bump slots have no home)"
                    ),
                );
            }

            let mut vertices = Vec::new();
            let (mut cx, mut cy) = (0.0f64, 0.0f64);
            for vertex in &mesh.vertices {
                let point = self.walk.to_world(walk_idx, *vertex, mesh.origin);
                vertices.push(jv2f(point));
                cx += point[0] as f64;
                cy += point[1] as f64;
            }
            self.anchor_point.insert(
                node.uuid,
                [
                    (cx / mesh.vertices.len() as f64) as f32,
                    (cy / mesh.vertices.len() as f64) as f32,
                ],
            );

            let uvs: Vec<Value> = mesh.uvs.iter().map(|uv| jv2(uv.x, uv.y)).collect();
            let indices: Vec<Value> = mesh
                .indices
                .iter()
                .map(|index| Value::from(*index as u64))
                .collect();
            let opacity = node.opacity * self.composite_opacity_factor(walk_idx);

            let mut model_mesh = serde_json::json!({
                "id": self.walk.nodes[walk_idx].mesh_id,
                "indices": indices,
                "opacity": jv(opacity),
                "part": self.walk.nodes[walk_idx].part_id,
                "texture": format!("tex_{}", node.texture_ids[0]),
                "uvs": uvs,
                "vertices": vertices,
            });
            let deformer = if let Some(rot_id) = &self.walk.nodes[walk_idx].rot_id {
                Some(rot_id.clone())
            } else {
                self.walk
                    .nearest_deformer_ancestor(walk_idx)
                    .and_then(|ancestor| self.walk.deformer_id_of(ancestor))
                    .map(str::to_string)
            };
            if let Some(deformer) = deformer {
                model_mesh["deformer"] = Value::String(deformer);
            }
            // Blend mode (may note the AddGlow approximation
            // or an unknown-mode drop).
            let mut blend_notes: Vec<(&'static str, String, String)> = Vec::new();
            let blend_mode = {
                let path_ref = &path;
                blend_mode_for(
                    &node.blend_mode,
                    path_ref,
                    &mut |verdict, construct, detail| {
                        blend_notes.push((verdict, construct, detail));
                    },
                )
            };
            for (verdict, construct, detail) in blend_notes {
                self.note(verdict, construct, detail);
            }
            model_mesh["blend_mode"] = Value::String(blend_mode.to_string());
            model_mesh["draw_order"] =
                Value::from(*self.draw_rank.get(&node.uuid).unwrap_or(&0) as i64);
            model_mesh["rendered"] =
                Value::Bool(render_enabled(self.puppet, self.walk.nodes[walk_idx].node));

            // Clipping masks.
            let mut mask_ids = Vec::new();
            for mask in &node.masks {
                if mask.mode == "DodgeMask" {
                    self.note(
                        "dropped",
                        format!("Part '{path}' dodge mask"),
                        "dodge masks are inverted; the lumen v1 mask relation has no inversion"
                            .to_string(),
                    );
                    continue;
                }
                let src_walk = mask
                    .src
                    .and_then(|src| self.walk.index_of_node(self.puppet, src));
                let src_mesh_id = src_walk.and_then(|src| self.walk.nodes[src].mesh_id.clone());
                match src_mesh_id {
                    Some(mesh_id) if !dropped_mesh_ids.contains(&mesh_id) => {
                        mask_ids.push(Value::String(mesh_id));
                    }
                    _ => {
                        self.note(
                            "dropped",
                            format!("Part '{path}' mask"),
                            "mask source does not export as a lumen mesh".to_string(),
                        );
                    }
                }
            }
            if !mask_ids.is_empty() {
                model_mesh["clipping_mask_ids"] = Value::Array(mask_ids);
            }

            // Keyforms from deformation bindings.
            let node_idx = self.walk.nodes[walk_idx].node;
            let defs = self.deform_bindings_of(node_idx);
            if !defs.is_empty() {
                match self.collect_axes(&defs) {
                    Err(reason) => {
                        for binding in &defs {
                            self.consumed.insert(*binding);
                            let construct = self.binding_construct(*binding);
                            self.note(
                                "dropped",
                                construct,
                                format!("mesh deformation cannot form a lumen lattice: {reason}"),
                            );
                        }
                    }
                    Ok(axes) => {
                        let nx = axes[0].keys.len();
                        let ny = if axes.len() > 1 {
                            axes[1].keys.len()
                        } else {
                            1
                        };
                        let mut forms = Vec::new();
                        for i in 0..nx {
                            for j in 0..ny {
                                let mut verts = Vec::new();
                                for (vi, vertex) in mesh.vertices.iter().enumerate() {
                                    let mut point =
                                        self.walk.to_world(walk_idx, *vertex, mesh.origin);
                                    for binding in &defs {
                                        let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                                        let delta = self.walk.delta_to_world(
                                            walk_idx,
                                            self.deform_offset_at(*binding, bx, by, vi),
                                        );
                                        point[0] += delta[0];
                                        point[1] += delta[1];
                                    }
                                    verts.push(jv2f(point));
                                }
                                let mut at = vec![Value::from(i as u64)];
                                if axes.len() > 1 {
                                    at.push(Value::from(j as u64));
                                }
                                forms.push(serde_json::json!({
                                    "at": at,
                                    "vertices": verts,
                                }));
                            }
                        }
                        for binding in &defs {
                            self.consumed.insert(*binding);
                        }
                        model_mesh["keyforms"] = self.lattice_json(&axes, forms);
                    }
                }
            }

            // Non-representable part shading state.
            if node.tint.x != 1.0 || node.tint.y != 1.0 || node.tint.z != 1.0 {
                self.note(
                    "dropped",
                    format!("Part '{path}' tint"),
                    "lumen v1 meshes carry no tint".to_string(),
                );
            }
            if node.screen_tint.x != 0.0 || node.screen_tint.y != 0.0 || node.screen_tint.z != 0.0 {
                self.note(
                    "dropped",
                    format!("Part '{path}' screen tint"),
                    "lumen v1 meshes carry no screen tint".to_string(),
                );
            }
            if node.emission_strength != 1.0 {
                self.note(
                    "dropped",
                    format!("Part '{path}' emission strength"),
                    "lumen v1 meshes carry no emission channel".to_string(),
                );
            }

            arr.push(model_mesh);
        }
        Value::Array(arr)
    }

    // ------------------------------------------------ physics
    fn emit_physics(&mut self) -> Value {
        let mut arr = Vec::new();
        let mut written_outputs: HashSet<String> = HashSet::new();
        for walk_idx in 0..self.walk.nodes.len() {
            let node = self.node_of(walk_idx);
            let Some(state) = &node.physics else {
                continue;
            };
            let path = self.walk.path_of(self.puppet, walk_idx);

            // Anchor: nearest strict ancestor that exports
            // as a deformer or a mesh.
            let mut anchor: Option<(usize, &'static str, String)> = None;
            let mut cursor = self.walk.nodes[walk_idx].parent;
            while let Some(parent) = cursor {
                let parent_node = self.node_of(parent);
                if self.walk.nodes[parent].warp_id.is_some()
                    || self.walk.nodes[parent].rot_id.is_some()
                {
                    let id = self.walk.deformer_id_of(parent).unwrap_or("").to_string();
                    anchor = Some((parent, "deformer", id));
                    break;
                }
                if is_part(parent_node.kind) && self.walk.nodes[parent].mesh_id.is_some() {
                    let id = self.walk.nodes[parent].mesh_id.clone().unwrap_or_default();
                    anchor = Some((parent, "mesh", id));
                    break;
                }
                cursor = self.walk.nodes[parent].parent;
            }
            let Some((anchor_walk, anchor_kind, anchor_id)) = anchor else {
                self.note(
                    "dropped",
                    format!("SimplePhysics '{path}'"),
                    "no ancestor exports as a lumen deformer or mesh, so the strand has no anchor"
                        .to_string(),
                );
                continue;
            };
            if state.length <= 0.0 {
                self.note(
                    "dropped",
                    format!("SimplePhysics '{path}'"),
                    "pendulum length is not positive".to_string(),
                );
                continue;
            }
            if state.angle_damping < 0.0 || state.angle_damping >= 1.0 {
                self.note(
                    "dropped",
                    format!("SimplePhysics '{path}'"),
                    format!(
                        "angle damping {} lies outside the lumen particle damping range [0, 1)",
                        fmt_g(state.angle_damping as f64)
                    ),
                );
                continue;
            }
            let gravity_y = -((self.puppet.gravity as f64)
                * (self.puppet.pixels_per_meter as f64)
                * (state.gravity as f64));
            if gravity_y < -1e5 || gravity_y > 1e5 {
                self.note(
                    "dropped",
                    format!("SimplePhysics '{path}'"),
                    "effective gravity exceeds the lumen field bound (1e5)".to_string(),
                );
                continue;
            }

            let particle = serde_json::json!({
                "damping": jv(state.angle_damping),
                "delay": jv(0.0),
                "length": jv(state.length),
                "mobility": jv(if state.spring { 0.5 } else { 0.0 }),
                "radius": jv(0.0),
            });

            let my_pos = self.walk.world_pos(walk_idx);
            let anchor_node = self.node_of(anchor_walk);
            let ref_point = self
                .anchor_point
                .get(&anchor_node.uuid)
                .copied()
                .unwrap_or_else(|| self.walk.world_pos(anchor_walk));
            let mut strand = serde_json::json!({
                "anchor": {
                    "id": anchor_id,
                    "kind": anchor_kind,
                    "offset": jv2(my_pos[0] - ref_point[0], my_pos[1] - ref_point[1]),
                },
                "id": "strand",
                "particles": [particle],
            });

            let mut approx_detail = format!(
                "SimplePhysics ({}, map mode {}) becomes a one-particle lumen strand; INP integrates a rigid/spring pendulum in anchor space while lumen integrates a Verlet chain — trajectories will differ",
                physics_model_name(state.spring),
                map_mode_name(state.map_mode)
            );
            if let Some(driven) = state.param {
                if !self.param_dropped.contains(&driven) {
                    let use_y = state.map_mode == 2 || state.map_mode == 3;
                    let out_id = if use_y {
                        self.param_y_id.get(&driven).cloned()
                    } else {
                        self.param_x_id.get(&driven).cloned()
                    };
                    if let Some(out_id) = out_id {
                        if written_outputs.insert(out_id.clone()) {
                            self.physics_outputs.insert(out_id.clone());
                            let scale = (if use_y {
                                state.output_scale.y
                            } else {
                                state.output_scale.x
                            }) / 180.0f32;
                            strand["output"] = serde_json::json!({
                                "offset": jv(0.0),
                                "parameter": out_id,
                                "scale": jv(scale),
                            });
                        } else {
                            approx_detail += "; the strand's output was dropped because another strand already writes that parameter (lumen one-writer rule)";
                        }
                    }
                }
            }
            self.note(
                "approximated",
                format!("SimplePhysics '{path}'"),
                approx_detail,
            );

            arr.push(serde_json::json!({
                "gravity": jv2(0.0, gravity_y as f32),
                "id": suffixed_name("phys_", &node.name, node.uuid, 44),
                "inputs": Vec::<Value>::new(),
                "strands": [strand],
            }));
        }
        Value::Array(arr)
    }

    // ------------------------------------------------ motions (SPEC section 4)

    /// Converts the puppet's animations into lumen motion
    /// assets (SPEC section 4) — the torogao-side bridge
    /// for the lumen rig-core animation leg. Motions are a
    /// separate asset kind from the model: one document
    /// per animation, written beside the model by
    /// `export_rig_files`. Every lane is converted with
    /// its fidelity stated in the report (mapped /
    /// approximated / dropped-with-reason); nothing is
    /// silently carried and nothing silently lost. Each
    /// emitted document is validated against section 4's
    /// rules before it is kept. The lumen-side parser and
    /// evaluator are R2 work in lumen's own tree (the
    /// Phase F handoff); this side emits, validates, and
    /// accounts.
    fn emit_motions(&mut self, model_id: &str) {
        // Parameter ranges by lumen id, for LRF-303.
        let mut ranges: HashMap<String, (f64, f64)> = HashMap::new();
        for (param_idx, param) in self.puppet.params.iter().enumerate() {
            if let Some(id) = self.param_x_id.get(&param_idx) {
                ranges.insert(id.clone(), (f64::from(param.min.x), f64::from(param.max.x)));
            }
            if let Some(id) = self.param_y_id.get(&param_idx) {
                ranges.insert(id.clone(), (f64::from(param.min.y), f64::from(param.max.y)));
            }
        }
        let mut used_motion_ids: HashSet<String> = HashSet::new();
        for (name, animation) in &self.puppet.animations {
            let construct = format!("animation '{name}'");
            if !animation.timestep.is_finite() || animation.timestep <= 0.0 {
                self.note(
                    "dropped",
                    construct,
                    format!(
                        "timestep {} is not positive and finite; frame times cannot be expressed in seconds (SPEC section 4 requires duration_seconds > 0 and frames_per_second in 1..=240)",
                        animation.timestep
                    ),
                );
                continue;
            }
            if animation.length <= 0 {
                self.note(
                    "dropped",
                    construct,
                    format!(
                        "length {} is not positive; duration_seconds must be > 0 (SPEC section 4)",
                        animation.length
                    ),
                );
                continue;
            }
            let duration = f64::from(animation.length) * f64::from(animation.timestep);
            let fps = (1.0 / f64::from(animation.timestep))
                .round()
                .clamp(1.0, 240.0) as u32;
            if animation.lead_in >= 0 || animation.lead_out >= 0 {
                self.note(
                    "approximated",
                    format!("animation '{name}' lead-in/out"),
                    "leadIn/leadOut are playback segmentation in the reference player; the motion asset has no field for them and the full span is carried".to_string(),
                );
            }
            if animation.additive || animation.animation_weight != 1.0 {
                self.note(
                    "approximated",
                    format!("animation '{name}' additive/weight"),
                    "the additive flag and animation weight are payload metadata the reference player does not consume; they are not carried into the motion asset".to_string(),
                );
            }
            let mut curves: Vec<Value> = Vec::new();
            let mut used_params: HashSet<String> = HashSet::new();
            let mut approximated: Vec<String> = Vec::new();
            for lane in &animation.lanes {
                let Some(param_idx) = self
                    .puppet
                    .params
                    .iter()
                    .position(|param| param.uuid == lane.param_uuid)
                else {
                    self.note(
                        "dropped",
                        format!(
                            "animation '{name}' lane (parameter uuid {})",
                            lane.param_uuid
                        ),
                        "the lane's parameter uuid does not resolve in this puppet".to_string(),
                    );
                    continue;
                };
                if self.param_dropped.contains(&param_idx) {
                    self.note(
                        "dropped",
                        format!(
                            "animation '{name}' lane (parameter uuid {})",
                            lane.param_uuid
                        ),
                        "the lane's parameter was dropped during parameter conversion".to_string(),
                    );
                    continue;
                }
                let param = &self.puppet.params[param_idx];
                let (param_id, min, max) = match lane.axis {
                    0 => match self.param_x_id.get(&param_idx) {
                        Some(id) => (id.clone(), param.min.x, param.max.x),
                        None => {
                            self.note(
                                "dropped",
                                format!("animation '{name}' lane (parameter '{}')", param.name),
                                "the parameter has no lumen x channel".to_string(),
                            );
                            continue;
                        }
                    },
                    1 => match self.param_y_id.get(&param_idx) {
                        Some(id) => (id.clone(), param.min.y, param.max.y),
                        None => {
                            self.note(
                                "dropped",
                                format!("animation '{name}' lane (parameter '{}')", param.name),
                                "the parameter is scalar; the lane's y axis has no lumen channel"
                                    .to_string(),
                            );
                            continue;
                        }
                    },
                    other => {
                        self.note(
                            "dropped",
                            format!("animation '{name}' lane (parameter '{}')", param.name),
                            format!("axis {other} is not an INP parameter axis"),
                        );
                        continue;
                    }
                };
                if self.physics_outputs.contains(&param_id) {
                    self.note(
                        "dropped",
                        format!("animation '{name}' lane (parameter '{param_id}')"),
                        "the parameter is a physics output; a motion curve must not target it (LRF-602, one-writer rule)".to_string(),
                    );
                    continue;
                }
                if !used_params.insert(param_id.clone()) {
                    self.note(
                        "dropped",
                        format!("animation '{name}' lane (parameter '{param_id}')"),
                        "a second curve for the same parameter is not representable (LRF-202: at most one curve per parameter per motion)".to_string(),
                    );
                    continue;
                }
                if !matches!(lane.merge_mode, torogao_engine::MergeMode::Forced) {
                    approximated.push(format!(
                        "lane '{param_id}' merges with mode {}; lumen curves replace the parameter's motion contribution, so the merge rule is not carried",
                        merge_mode_name(lane.merge_mode)
                    ));
                }
                match convert_lane_curve(lane, &param_id, min, max, animation.timestep) {
                    Ok((curve, notes)) => {
                        approximated.extend(notes);
                        curves.push(curve);
                    }
                    Err(reason) => {
                        self.note(
                            "dropped",
                            format!("animation '{name}' lane (parameter '{param_id}')"),
                            reason,
                        );
                    }
                }
            }
            if curves.is_empty() {
                self.note(
                    "dropped",
                    construct,
                    format!(
                        "{} lane(s), {} keyframes: no lane survived conversion (see the lane entries)",
                        animation.lanes.len(),
                        animation.keyframe_count()
                    ),
                );
                continue;
            }
            let mut motion_id = slugify(name, 48);
            if motion_id.is_empty() {
                motion_id = "motion".to_string();
            }
            if !used_motion_ids.insert(motion_id.clone()) {
                let mut suffix = 2;
                loop {
                    let candidate = format!("{motion_id}-{suffix}");
                    if used_motion_ids.insert(candidate.clone()) {
                        motion_id = candidate;
                        break;
                    }
                    suffix += 1;
                }
            }
            let document = serde_json::json!({
                "curves": curves,
                "duration_seconds": duration,
                "format": "lumen-rig-motion",
                "format_version": 1,
                "frames_per_second": fps,
                "id": motion_id,
                "loop": false,
                "model": model_id,
            });
            if let Err(reason) = validate_motion(&document, &ranges, &self.physics_outputs) {
                self.note(
                    "dropped",
                    construct,
                    format!("the converted motion failed SPEC section 4 validation: {reason}"),
                );
                continue;
            }
            let file_name = format!("motion-{motion_id}.json");
            let keyframes: usize = animation.keyframe_count();
            if approximated.is_empty() {
                self.note(
                    "mapped",
                    construct,
                    format!(
                        "{} lane(s), {} keyframes -> motion asset '{file_name}' (loop is a player argument in the reference player; the asset carries loop: false)",
                        animation.lanes.len(),
                        keyframes
                    ),
                );
            } else {
                self.note(
                    "approximated",
                    construct,
                    format!(
                        "{} lane(s), {} keyframes -> motion asset '{file_name}'; {}",
                        animation.lanes.len(),
                        keyframes,
                        approximated.join("; ")
                    ),
                );
            }
            self.motions.push((file_name, document));
        }
    }

    // ------------------------------------------------ accounting
    fn account_bindings(&mut self) {
        for (param_idx, param) in self.puppet.params.iter().enumerate() {
            for (binding_idx, binding) in param.bindings.iter().enumerate() {
                let key = (param_idx, binding_idx);
                if !self.consumed.insert(key) {
                    continue;
                }
                let channel = binding.param_name.as_str();
                let reason = if matches!(binding.values, BindingValues::Deform(_)) {
                    "deformation binding was not consumed by any exporter path".to_string()
                } else if channel == "transform.t.z" {
                    "node transform channel transform.t.z has no lumen representation (BIND-1 part translations are 2D)"
                        .to_string()
                } else if channel.len() >= 10 && &channel.as_bytes()[..10] == b"transform." {
                    "node transform channel was not consumed by any exporter path (transform.r.z / s.x / s.y are consumed as rotation deformers and transform.t.x / t.y as BIND-1 part translation keyforms)"
                        .to_string()
                } else {
                    format!("property channel '{channel}' has no lumen v1 representation")
                };
                let construct = self.binding_construct(key);
                self.note("dropped", construct, reason);
            }
        }
    }

    fn account_whole_constructs(&mut self) {
        // Animations are accounted by `emit_motions` (they
        // leave as motion assets, SPEC section 4).
        for automation in &self.puppet.automations {
            self.note(
                "dropped",
                format!("automation '{}'", automation.type_id),
                "procedural parameter drivers have no lumen v1 representation".to_string(),
            );
        }
        for (name, _) in &self.puppet.ext_data {
            self.note(
                "dropped",
                format!("vendor EXT section '{name}'"),
                "vendor extension data is not carried into the lumen model".to_string(),
            );
        }
    }
}

/// One section 4 keyframe document.
fn key_json(
    time: f64,
    value: f64,
    interpolation: &str,
    tangent_in: Option<(f64, f64)>,
    tangent_out: Option<(f64, f64)>,
) -> Value {
    let mut key = serde_json::json!({
        "interpolation": interpolation,
        "time": time,
        "value": value,
    });
    if let Some((dt, dv)) = tangent_in {
        key["tangent_in"] = serde_json::json!([dt, dv]);
    }
    if let Some((dt, dv)) = tangent_out {
        key["tangent_out"] = serde_json::json!([dt, dv]);
    }
    key
}

/// Keyframes with no tangents, all in one mode.
fn plain_keys(times: &[f64], values: &[f64], mode: &str) -> Vec<Value> {
    times
        .iter()
        .zip(values.iter())
        .map(|(time, value)| key_json(*time, *value, mode, None, None))
        .collect()
}

/// Whether the reference Bezier lane's hermite fraction
/// `f(t) = (-2t^3 + 3t^2) + 2*tau*(2t^3 - 3t^2 + t)`
/// stays within [0, 1] on [0, 1] — the exactness
/// condition for the cubic conversion (outside it, the
/// engine clamps the fraction and the clamp has no
/// section 4 representation). Checked exactly, via the
/// roots of f' inside the interval.
fn hermite_fraction_in_unit(tau: f64) -> bool {
    let fraction =
        |t: f64| (-2.0 * t * t * t + 3.0 * t * t) + 2.0 * tau * (2.0 * t * t * t - 3.0 * t * t + t);
    let a = -6.0 + 12.0 * tau;
    let b = 6.0 - 12.0 * tau;
    let c = 2.0 * tau;
    let mut roots: Vec<f64> = Vec::new();
    if a.abs() <= 1e-15 {
        if b.abs() > 1e-15 {
            roots.push(-c / b);
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            roots.push((-b + root) / (2.0 * a));
            roots.push((-b - root) / (2.0 * a));
        }
    }
    roots.iter().filter(|t| **t > 0.0 && **t < 1.0).all(|t| {
        let value = fraction(*t);
        (-1e-12..=1.0 + 1e-12).contains(&value)
    })
}

/// Converts one engine lane into a SPEC section 4 curve
/// for `param_id` (range [`min`, `max`], step `timestep`
/// seconds). On success returns the curve plus fidelity
/// notes (empty = exact); on failure the drop reason.
///
/// Interpolation fidelity, by engine mode:
/// - Linear -> "linear", exact. Stepped -> "stepped",
///   exact.
/// - Cubic (the reference Catmull-Rom over values) ->
///   "cubic", exact: with tangents dt = segment/3 and dv
///   from the Catmull-Rom control values, the (time,
///   value) Bezier of SPEC 4.2 coincides with the
///   reference polynomial (equal time spacing of the
///   Bezier controls makes its time component linear in
///   the segment parameter, and the value controls are
///   the Catmull-Rom-to-Bezier conversion).
/// - Bezier (hermite ease keyed by the closing key's
///   tension): tension 0.5 is exactly linear; any other
///   tension becomes "cubic" with dv = span * 2*tau/3 —
///   exact while the hermite fraction stays within
///   [0, 1] (`hermite_fraction_in_unit`), approximated
///   where the engine's clamp would bite.
/// - Nearest -> "stepped" with an inserted midpoint key
///   per segment: exact except at the midpoint instants
///   themselves, which the next segment owns under SPEC
///   4.2 — noted approximated.
fn convert_lane_curve(
    lane: &torogao_engine::AnimationLane,
    param_id: &str,
    min: f32,
    max: f32,
    timestep: f32,
) -> Result<(Value, Vec<String>), String> {
    use torogao_engine::param::InterpolateMode;
    if lane.frames.is_empty() {
        return Err("the lane has no keyframes".to_string());
    }
    for pair in lane.frames.windows(2) {
        if pair[1].frame <= pair[0].frame {
            return Err(
                "keyframe frames are not strictly increasing; duplicate times are not representable (LRF-501)"
                    .to_string(),
            );
        }
    }
    for key in &lane.frames {
        if !key.value.is_finite() || key.value < min || key.value > max {
            return Err(format!(
                "keyframe value {} at frame {} lies outside the parameter range [{}, {}]; motion values must lie in range (LRF-303)",
                key.value, key.frame, min, max
            ));
        }
    }
    let ts = f64::from(timestep);
    let times: Vec<f64> = lane
        .frames
        .iter()
        .map(|key| f64::from(key.frame) * ts)
        .collect();
    let values: Vec<f64> = lane.frames.iter().map(|key| f64::from(key.value)).collect();
    let n = lane.frames.len();
    let mut notes: Vec<String> = Vec::new();
    let keyframes: Vec<Value> = match lane.interpolation {
        InterpolateMode::Linear => plain_keys(&times, &values, "linear"),
        InterpolateMode::Stepped => plain_keys(&times, &values, "stepped"),
        InterpolateMode::Nearest => {
            notes.push(
                "Nearest becomes stepped with an inserted midpoint key per segment: exact except at the midpoint instants, which the next segment owns under SPEC 4.2"
                    .to_string(),
            );
            let mut keys = Vec::new();
            for j in 0..n {
                keys.push(key_json(times[j], values[j], "stepped", None, None));
                if j + 1 < n {
                    keys.push(key_json(
                        (times[j] + times[j + 1]) / 2.0,
                        values[j + 1],
                        "stepped",
                        None,
                        None,
                    ));
                }
            }
            keys
        }
        InterpolateMode::Cubic | InterpolateMode::Bezier => {
            // Per segment: (dt, dv_out, dv_in).
            struct Segment {
                dt: f64,
                dv_out: f64,
                dv_in: f64,
            }
            let mut segments: Vec<Segment> = Vec::new();
            let mut any_cubic = false;
            let mut overshoot = false;
            for j in 0..n.saturating_sub(1) {
                let dt_seg = times[j + 1] - times[j];
                let span = values[j + 1] - values[j];
                match lane.interpolation {
                    InterpolateMode::Cubic => {
                        // The sampler's neighbor indices:
                        // prev = frames[i-2] for the closing
                        // key i = j+1 (saturating), next2 =
                        // frames[i+1] (clamped to the last).
                        let prev = values[j.saturating_sub(1)];
                        let next2 = values[(j + 2).min(n - 1)];
                        segments.push(Segment {
                            dt: dt_seg / 3.0,
                            dv_out: (values[j + 1] - prev) / 6.0,
                            dv_in: (next2 - values[j]) / 6.0,
                        });
                        any_cubic = true;
                    }
                    InterpolateMode::Bezier => {
                        let tau = f64::from(lane.frames[j + 1].tension);
                        if (tau - 0.5).abs() <= f64::EPSILON {
                            // The hermite fraction is t
                            // itself: exactly linear.
                            segments.push(Segment {
                                dt: dt_seg / 3.0,
                                dv_out: span / 3.0,
                                dv_in: span / 3.0,
                            });
                        } else {
                            if !hermite_fraction_in_unit(tau) {
                                overshoot = true;
                            }
                            segments.push(Segment {
                                dt: dt_seg / 3.0,
                                dv_out: span * 2.0 * tau / 3.0,
                                dv_in: span * 2.0 * tau / 3.0,
                            });
                            any_cubic = true;
                        }
                    }
                    _ => unreachable!("the outer match fixed the mode"),
                }
            }
            if overshoot {
                notes.push(
                    "a Bezier segment's hermite fraction overshoots [0, 1]; the engine clamps the fraction and the clamp is not representable, so the cubic carries the unclamped polynomial"
                        .to_string(),
                );
            }
            if !any_cubic {
                plain_keys(&times, &values, "linear")
            } else {
                // Every key is cubic; segments that are
                // linear carry the collinear tangents,
                // which reproduce them exactly.
                let mut keys = Vec::new();
                for j in 0..n {
                    let tangent_out = if j + 1 < n {
                        Some((segments[j].dt, segments[j].dv_out))
                    } else {
                        None
                    };
                    let tangent_in = if j > 0 {
                        Some((segments[j - 1].dt, segments[j - 1].dv_in))
                    } else {
                        None
                    };
                    keys.push(key_json(
                        times[j],
                        values[j],
                        "cubic",
                        tangent_in,
                        tangent_out,
                    ));
                }
                keys
            }
        }
    };
    Ok((
        serde_json::json!({
            "keyframes": keyframes,
            "parameter": param_id,
        }),
        notes,
    ))
}

/// Validates a motion document against SPEC section 4's
/// rules (field sets per 1.1, LRF-202/203/303/501/502/
/// 602) before it is written. `ranges` maps lumen
/// parameter ids to [min, max]; `physics_outputs` is
/// the model's physics-output parameter set.
fn validate_motion(
    document: &Value,
    ranges: &HashMap<String, (f64, f64)>,
    physics_outputs: &HashSet<String>,
) -> Result<(), String> {
    let obj = document
        .as_object()
        .ok_or_else(|| "the motion is not an object".to_string())?;
    let top_fields: HashSet<&str> = [
        "curves",
        "duration_seconds",
        "format",
        "format_version",
        "frames_per_second",
        "id",
        "loop",
        "model",
    ]
    .into_iter()
    .collect();
    for key in obj.keys() {
        if !top_fields.contains(key.as_str()) {
            return Err(format!("unknown top-level field '{key}' (SPEC 1.1)"));
        }
    }
    if obj.get("format").and_then(Value::as_str) != Some("lumen-rig-motion") {
        return Err("format is not 'lumen-rig-motion'".to_string());
    }
    if obj.get("format_version").and_then(Value::as_u64) != Some(1) {
        return Err("format_version is not 1".to_string());
    }
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "id is missing".to_string())?;
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "id '{id}' is outside [A-Za-z0-9_-]{{1,64}} (SPEC 1.1)"
        ));
    }
    if obj
        .get("model")
        .and_then(Value::as_str)
        .is_none_or(|model| model.is_empty())
    {
        return Err("model is missing (LRF-203 binds the motion to its model)".to_string());
    }
    let duration = obj
        .get("duration_seconds")
        .and_then(Value::as_f64)
        .ok_or_else(|| "duration_seconds is missing".to_string())?;
    if !duration.is_finite() || duration <= 0.0 {
        return Err("duration_seconds must be finite and > 0".to_string());
    }
    let fps = obj
        .get("frames_per_second")
        .and_then(Value::as_u64)
        .ok_or_else(|| "frames_per_second is missing".to_string())?;
    if !(1..=240).contains(&fps) {
        return Err("frames_per_second is outside 1..=240".to_string());
    }
    let curves = obj
        .get("curves")
        .and_then(Value::as_array)
        .ok_or_else(|| "curves is missing".to_string())?;
    if curves.is_empty() {
        return Err("the motion carries no curves".to_string());
    }
    let mut seen_params: HashSet<&str> = HashSet::new();
    for curve in curves {
        let curve_obj = curve
            .as_object()
            .ok_or_else(|| "a curve is not an object".to_string())?;
        for key in curve_obj.keys() {
            if ![
                "parameter",
                "keyframes",
                "fade_in_seconds",
                "fade_out_seconds",
            ]
            .contains(&key.as_str())
            {
                return Err(format!("unknown curve field '{key}' (SPEC 1.1)"));
            }
        }
        let param_id = curve_obj
            .get("parameter")
            .and_then(Value::as_str)
            .ok_or_else(|| "a curve has no parameter".to_string())?;
        let Some(&(min, max)) = ranges.get(param_id) else {
            return Err(format!(
                "curve parameter '{param_id}' is not in the model (LRF-203)"
            ));
        };
        if physics_outputs.contains(param_id) {
            return Err(format!(
                "curve parameter '{param_id}' is a physics output (LRF-602)"
            ));
        }
        if !seen_params.insert(param_id) {
            return Err(format!("a second curve targets '{param_id}' (LRF-202)"));
        }
        let keyframes = curve_obj
            .get("keyframes")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("curve '{param_id}' has no keyframes"))?;
        if keyframes.is_empty() {
            return Err(format!("curve '{param_id}' has no keyframes"));
        }
        // Collect the keys first: tangent bounds are
        // pairwise (LRF-502).
        struct Key {
            time: f64,
            tangent_in: Option<(f64, f64)>,
            tangent_out: Option<(f64, f64)>,
        }
        let mut keys: Vec<Key> = Vec::new();
        for keyframe in keyframes {
            let key_obj = keyframe
                .as_object()
                .ok_or_else(|| "a keyframe is not an object".to_string())?;
            for field in key_obj.keys() {
                if ![
                    "time",
                    "value",
                    "interpolation",
                    "tangent_in",
                    "tangent_out",
                ]
                .contains(&field.as_str())
                {
                    return Err(format!("unknown keyframe field '{field}' (SPEC 1.1)"));
                }
            }
            let time = key_obj
                .get("time")
                .and_then(Value::as_f64)
                .ok_or_else(|| "a keyframe has no time".to_string())?;
            let value = key_obj
                .get("value")
                .and_then(Value::as_f64)
                .ok_or_else(|| "a keyframe has no value".to_string())?;
            if !time.is_finite() || !value.is_finite() {
                return Err("keyframe numbers must be finite (SPEC 1.1)".to_string());
            }
            if value < min || value > max {
                return Err(format!(
                    "keyframe value {value} lies outside [{min}, {max}] for '{param_id}' (LRF-303)"
                ));
            }
            let interpolation = key_obj
                .get("interpolation")
                .and_then(Value::as_str)
                .unwrap_or("linear");
            if !["linear", "stepped", "cubic"].contains(&interpolation) {
                return Err(format!("unknown interpolation '{interpolation}'"));
            }
            let tangent = |name: &str| -> Result<Option<(f64, f64)>, String> {
                match key_obj.get(name) {
                    None => Ok(None),
                    Some(raw) => {
                        let pair = raw
                            .as_array()
                            .ok_or_else(|| format!("{name} is not a [dt, dv] pair"))?;
                        if pair.len() != 2 {
                            return Err(format!("{name} is not a [dt, dv] pair"));
                        }
                        let dt = pair[0].as_f64().ok_or_else(|| format!("{name} dt"))?;
                        let dv = pair[1].as_f64().ok_or_else(|| format!("{name} dv"))?;
                        if !dt.is_finite() || !dv.is_finite() || dt < 0.0 {
                            return Err(format!("{name} has dt < 0 or non-finite parts (LRF-502)"));
                        }
                        Ok(Some((dt, dv)))
                    }
                }
            };
            let tangent_in = tangent("tangent_in")?;
            let tangent_out = tangent("tangent_out")?;
            if interpolation != "cubic" && (tangent_in.is_some() || tangent_out.is_some()) {
                return Err("tangents on a non-cubic key are invalid (LRF-502)".to_string());
            }
            keys.push(Key {
                time,
                tangent_in,
                tangent_out,
            });
        }
        if keys[0].time < 0.0 {
            return Err("the first keyframe time is negative (LRF-501)".to_string());
        }
        for pair in keys.windows(2) {
            if pair[1].time <= pair[0].time {
                return Err("keyframe times are not strictly increasing (LRF-501)".to_string());
            }
            if let Some((dt, _)) = pair[0].tangent_out {
                if pair[0].time + dt > pair[1].time {
                    return Err("tangent_out overruns the segment (LRF-502)".to_string());
                }
            }
            if let Some((dt, _)) = pair[1].tangent_in {
                if pair[1].time - dt < pair[0].time {
                    return Err("tangent_in overruns the segment (LRF-502)".to_string());
                }
            }
        }
        if keys[keys.len() - 1].time > duration {
            return Err("the last keyframe lies past duration_seconds (LRF-501)".to_string());
        }
    }
    Ok(())
}

/// Runs the export: builds the model, writes the model,
/// stamp, and report files, and returns the report
/// document.
pub fn export_rig_files(
    load: &ToolLoad,
    out_dir: &str,
    base: &str,
    ext_view: &ExtView,
) -> Result<Value, ExportError> {
    let mut walk = load.walk.clone();
    let result = RigExporter::run(
        &load.loaded.puppet,
        &mut walk,
        load.loaded.puppet.texture_slots.len(),
    )
    .map_err(ExportError::Build)?;
    std::fs::create_dir_all(out_dir).map_err(|err| ExportError::Io(err.to_string()))?;
    let model_path = join_path(out_dir, &format!("{base}.model.json"));
    let report_path = join_path(out_dir, &format!("{base}.export-report.json"));
    let stamp_path = join_path(out_dir, &format!("{base}.stamp.json"));
    let model_text = format!("{}\n", torogao_format::jsonfmt::to_d_json(&result.model));
    std::fs::write(&model_path, model_text).map_err(|err| ExportError::Io(err.to_string()))?;
    for (file_name, motion) in &result.motions {
        let motion_path = join_path(out_dir, &format!("{base}.{file_name}"));
        std::fs::write(
            &motion_path,
            format!("{}\n", torogao_format::jsonfmt::to_d_json(motion)),
        )
        .map_err(|err| ExportError::Io(err.to_string()))?;
    }
    let stamp = stamp_doc(&load.path, &model_path, &ext_view.doc);
    std::fs::write(
        &stamp_path,
        format!("{}\n", torogao_format::jsonfmt::to_d_json(&stamp)),
    )
    .map_err(|err| ExportError::Io(err.to_string()))?;
    let entries: Vec<Value> = result.entries.iter().map(ConvEntry::to_json).collect();
    let report = serde_json::json!({
        "approximated": result.count("approximated"),
        "dropped": result.count("dropped"),
        "entries": entries,
        "file": load.path,
        "mapped": result.count("mapped"),
        "model": model_path,
        "ratingStamp": crate::extdoc::rating_stamp(&ext_view.doc),
        "stampFile": stamp_path,
    });
    std::fs::write(
        &report_path,
        format!("{}\n", torogao_format::jsonfmt::to_d_json(&report)),
    )
    .map_err(|err| ExportError::Io(err.to_string()))?;
    Ok(report)
}

/// The registry tool's shape over `export_rig_files`.
pub fn export_rig(load: &ToolLoad, out_dir: &str, base: &str, ext_view: &ExtView) -> ToolResult {
    match export_rig_files(load, out_dir, base, ext_view) {
        Ok(report) => ToolResult::ok(report),
        Err(ExportError::Build(message)) => ToolResult::fail("export_failed", &message),
        Err(ExportError::Io(message)) => ToolResult::fail("io_error", &message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torogao_engine::param::{InterpolateMode, MergeMode};
    use torogao_engine::{AnimationLane, Keyframe};

    fn lane(mode: InterpolateMode, frames: &[(i32, f32, f32)]) -> AnimationLane {
        AnimationLane {
            param_uuid: 7,
            axis: 0,
            interpolation: mode,
            merge_mode: MergeMode::Forced,
            frames: frames
                .iter()
                .map(|(frame, value, tension)| Keyframe {
                    frame: *frame,
                    value: *value,
                    tension: *tension,
                })
                .collect(),
        }
    }

    /// A SPEC section 4.2 evaluator over an emitted curve,
    /// written from the SPEC text (Newton-Raphson at most
    /// 8 iterations from the segment fraction, tolerance
    /// 1e-6 s, then bisection for exactly 24 iterations) —
    /// the fidelity reference the conversions are measured
    /// against.
    fn spec_evaluate(curve: &Value, t: f64) -> f64 {
        let keys = curve["keyframes"].as_array().expect("keyframes");
        let read = |key: &Value| {
            (
                key["time"].as_f64().expect("time"),
                key["value"].as_f64().expect("value"),
            )
        };
        let tangent = |key: &Value, name: &str| -> (f64, f64) {
            match key.get(name) {
                Some(pair) => (pair[0].as_f64().expect("dt"), pair[1].as_f64().expect("dv")),
                None => (0.0, 0.0),
            }
        };
        let last = keys.len() - 1;
        if t <= read(&keys[0]).0 {
            return read(&keys[0]).1;
        }
        if t >= read(&keys[last]).0 {
            return read(&keys[last]).1;
        }
        for i in 0..last {
            let (t0, v0) = read(&keys[i]);
            let (t1, v1) = read(&keys[i + 1]);
            if t >= t1 {
                continue;
            }
            return match keys[i]["interpolation"].as_str().unwrap_or("linear") {
                "stepped" => v0,
                "linear" => v0 + (v1 - v0) * (t - t0) / (t1 - t0),
                "cubic" => {
                    let (dt_out, dv_out) = tangent(&keys[i], "tangent_out");
                    let (dt_in, dv_in) = tangent(&keys[i + 1], "tangent_in");
                    let px = [t0, t0 + dt_out, t1 - dt_in, t1];
                    let py = [v0, v0 + dv_out, v1 - dv_in, v1];
                    let bezier = |p: [f64; 4], u: f64| {
                        let w = 1.0 - u;
                        w * w * w * p[0]
                            + 3.0 * w * w * u * p[1]
                            + 3.0 * w * u * u * p[2]
                            + u * u * u * p[3]
                    };
                    let bezier_dx = |u: f64| {
                        let w = 1.0 - u;
                        3.0 * w * w * (px[1] - px[0])
                            + 6.0 * w * u * (px[2] - px[1])
                            + 3.0 * u * u * (px[3] - px[2])
                    };
                    let mut u = (t - t0) / (t1 - t0);
                    let mut converged = false;
                    for _ in 0..8 {
                        let err = bezier(px, u) - t;
                        if err.abs() <= 1e-6 {
                            converged = true;
                            break;
                        }
                        let derivative = bezier_dx(u);
                        if derivative.abs() <= f64::EPSILON {
                            break;
                        }
                        u -= err / derivative;
                    }
                    if !converged {
                        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
                        for _ in 0..24 {
                            u = (lo + hi) / 2.0;
                            if bezier(px, u) < t {
                                lo = u;
                            } else {
                                hi = u;
                            }
                        }
                        u = (lo + hi) / 2.0;
                    }
                    bezier(py, u)
                }
                other => unreachable!("emitted mode {other}"),
            };
        }
        read(&keys[last]).1
    }

    /// The worst |engine - SPEC| over a dense frame sweep.
    fn max_deviation(lane: &AnimationLane, curve: &Value, timestep: f32) -> f64 {
        let last_frame = lane.frames[lane.frames.len() - 1].frame as f32;
        let mut worst = 0.0_f64;
        let mut frame = 0.0_f32;
        while frame <= last_frame {
            let engine = f64::from(lane.sample(frame, false));
            let spec = spec_evaluate(curve, f64::from(frame) * f64::from(timestep));
            worst = worst.max((engine - spec).abs());
            frame += 0.37;
        }
        worst
    }

    #[test]
    fn cubic_conversion_is_exact_against_the_spec_evaluator() {
        // Uneven spacing and clamped ends: the cases where
        // a naive conversion drifts.
        let source = lane(
            InterpolateMode::Cubic,
            &[
                (0, 0.0, 0.0),
                (10, 1.0, 0.0),
                (25, -0.5, 0.0),
                (40, 0.25, 0.0),
            ],
        );
        let (curve, notes) =
            convert_lane_curve(&source, "p_7_x", -1.0, 1.0, 0.05).expect("conversion succeeds");
        assert!(
            notes.is_empty(),
            "cubic conversion must be exact: {notes:?}"
        );
        let deviation = max_deviation(&source, &curve, 0.05);
        assert!(deviation < 2e-3, "cubic deviation {deviation}");
    }

    #[test]
    fn bezier_half_tension_is_linear_and_mild_tension_is_exact() {
        let half = lane(
            InterpolateMode::Bezier,
            &[(0, 0.0, 0.0), (20, 0.8, 0.5), (40, -0.4, 0.5)],
        );
        let (curve, notes) =
            convert_lane_curve(&half, "p_7_x", -1.0, 1.0, 0.05).expect("conversion succeeds");
        assert!(notes.is_empty());
        assert_eq!(curve["keyframes"][0]["interpolation"], "linear");
        assert!(max_deviation(&half, &curve, 0.05) < 2e-3);

        let mild = lane(
            InterpolateMode::Bezier,
            &[(0, 0.0, 0.0), (20, 0.8, 0.3), (40, -0.4, 0.3)],
        );
        let (curve, notes) =
            convert_lane_curve(&mild, "p_7_x", -1.0, 1.0, 0.05).expect("conversion succeeds");
        assert!(notes.is_empty(), "tau 0.3 stays in [0, 1]: {notes:?}");
        assert_eq!(curve["keyframes"][0]["interpolation"], "cubic");
        assert!(max_deviation(&mild, &curve, 0.05) < 2e-3);
    }

    #[test]
    fn bezier_wild_tension_reports_the_clamp_gap() {
        assert!(hermite_fraction_in_unit(0.3));
        assert!(!hermite_fraction_in_unit(6.0));
        let wild = lane(InterpolateMode::Bezier, &[(0, 0.0, 0.0), (20, 0.8, 6.0)]);
        let (_, notes) =
            convert_lane_curve(&wild, "p_7_x", -1.0, 1.0, 0.05).expect("conversion succeeds");
        assert_eq!(notes.len(), 1, "the clamp gap must be noted: {notes:?}");
        assert!(notes[0].contains("clamp"));
    }

    #[test]
    fn nearest_matches_except_at_midpoint_instants() {
        let source = lane(
            InterpolateMode::Nearest,
            &[(0, 0.25, 0.0), (20, 0.75, 0.0), (40, -0.5, 0.0)],
        );
        let (curve, notes) =
            convert_lane_curve(&source, "p_7_x", -1.0, 1.0, 0.05).expect("conversion succeeds");
        assert_eq!(notes.len(), 1);
        // The inserted midpoint keys are present.
        assert_eq!(curve["keyframes"].as_array().map(Vec::len), Some(5));
        // Away from the exact midpoints (frames 10 and 30)
        // the two samplers agree exactly.
        for frame in [0.0_f32, 5.0, 9.9, 10.1, 19.0, 25.0, 29.9, 30.1, 40.0] {
            let engine = f64::from(source.sample(frame, false));
            let spec = spec_evaluate(&curve, f64::from(frame) * 0.05);
            assert!(
                (engine - spec).abs() < 1e-6,
                "frame {frame}: {engine} vs {spec}"
            );
        }
    }

    #[test]
    fn linear_and_stepped_are_exact() {
        for mode in [InterpolateMode::Linear, InterpolateMode::Stepped] {
            let source = lane(mode, &[(0, 0.1, 0.0), (13, 0.9, 0.0), (40, -0.2, 0.0)]);
            let (curve, notes) =
                convert_lane_curve(&source, "p_7_x", -1.0, 1.0, 0.05).expect("conversion succeeds");
            assert!(notes.is_empty());
            assert!(max_deviation(&source, &curve, 0.05) < 1e-6, "mode {mode:?}");
        }
    }

    #[test]
    fn conversion_refuses_unrepresentable_lanes() {
        let duplicate = lane(InterpolateMode::Linear, &[(0, 0.0, 0.0), (0, 1.0, 0.0)]);
        assert!(
            convert_lane_curve(&duplicate, "p_7_x", -1.0, 1.0, 0.05)
                .expect_err("duplicate times")
                .contains("LRF-501")
        );
        let out_of_range = lane(InterpolateMode::Linear, &[(0, 0.0, 0.0), (10, 1.5, 0.0)]);
        assert!(
            convert_lane_curve(&out_of_range, "p_7_x", -1.0, 1.0, 0.05)
                .expect_err("out of range")
                .contains("LRF-303")
        );
        let empty = lane(InterpolateMode::Linear, &[]);
        assert!(convert_lane_curve(&empty, "p_7_x", -1.0, 1.0, 0.05).is_err());
    }

    fn valid_motion() -> Value {
        serde_json::json!({
            "curves": [{
                "keyframes": [
                    {"interpolation": "linear", "time": 0.0, "value": 0.0},
                    {"interpolation": "linear", "time": 1.0, "value": 0.5}
                ],
                "parameter": "p_7_x"
            }],
            "duration_seconds": 1.0,
            "format": "lumen-rig-motion",
            "format_version": 1,
            "frames_per_second": 60,
            "id": "greeting",
            "loop": false,
            "model": "prima"
        })
    }

    fn ranges() -> HashMap<String, (f64, f64)> {
        let mut ranges = HashMap::new();
        ranges.insert("p_7_x".to_string(), (-1.0, 1.0));
        ranges.insert("phys_9_x".to_string(), (-1.0, 1.0));
        ranges
    }

    #[test]
    fn validator_accepts_a_valid_motion_and_refuses_each_rule_breach() {
        let physics: HashSet<String> = ["phys_9_x".to_string()].into_iter().collect();
        assert!(validate_motion(&valid_motion(), &ranges(), &physics).is_ok());

        let mut doc = valid_motion();
        doc["bogus"] = serde_json::json!(1);
        assert!(
            validate_motion(&doc, &ranges(), &physics)
                .expect_err("unknown field")
                .contains("SPEC 1.1")
        );

        let mut doc = valid_motion();
        doc["curves"] = serde_json::json!([doc["curves"][0].clone(), doc["curves"][0].clone()]);
        assert!(
            validate_motion(&doc, &ranges(), &physics)
                .expect_err("second curve")
                .contains("LRF-202")
        );

        let mut doc = valid_motion();
        doc["curves"][0]["parameter"] = serde_json::json!("phys_9_x");
        assert!(
            validate_motion(&doc, &ranges(), &physics)
                .expect_err("physics target")
                .contains("LRF-602")
        );

        let mut doc = valid_motion();
        doc["curves"][0]["keyframes"][1]["value"] = serde_json::json!(2.0);
        assert!(
            validate_motion(&doc, &ranges(), &physics)
                .expect_err("out of range")
                .contains("LRF-303")
        );

        let mut doc = valid_motion();
        doc["curves"][0]["keyframes"][0]["tangent_out"] = serde_json::json!([0.1, 0.0]);
        assert!(
            validate_motion(&doc, &ranges(), &physics)
                .expect_err("tangent on a linear key")
                .contains("LRF-502")
        );

        let mut doc = valid_motion();
        doc["curves"][0]["keyframes"][1]["time"] = serde_json::json!(0.0);
        assert!(
            validate_motion(&doc, &ranges(), &physics)
                .expect_err("non-increasing times")
                .contains("LRF-501")
        );

        let mut doc = valid_motion();
        doc["frames_per_second"] = serde_json::json!(0);
        assert!(validate_motion(&doc, &ranges(), &physics).is_err());
    }

    #[test]
    fn aka_translation_bindings_become_bind1_part_lattices() {
        // BIND-1 end to end on a real puppet: Aka's
        // transform.t.x / t.y bindings (126 of them dropped
        // at the pre-BIND-1 baseline) now land as
        // translation_keyforms lattices on their parts
        // wherever they fit the format's 2-axis lattice law
        // (57 parts; the brow bindings span 3 axes and drop
        // with that limit named), the retired "parts carry
        // no transforms" reason appears nowhere, and the
        // surviving transform.t drops are strictly fewer
        // than the baseline.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/example-models/aka/Aka.inx");
        if !path.exists() {
            eprintln!("Aka fixture unavailable; BIND-1 bridge test skipped");
            return;
        }
        let mut puppet = crate::guards::load_puppet_guarded(path.to_str().expect("utf8 path"))
            .expect("Aka loads");
        let mut walk = Walk::build(&mut puppet);
        let slot_count = puppet.texture_slots.len();
        let result = RigExporter::run(&puppet, &mut walk, slot_count).expect("Aka exports");

        let parts = result.model["parts"]
            .as_array()
            .expect("model carries parts");
        let translated: Vec<&Value> = parts
            .iter()
            .filter(|part| part.get("translation_keyforms").is_some())
            .collect();
        assert!(
            translated.len() >= 10,
            "Aka parts carrying BIND-1 lattices: {}",
            translated.len()
        );
        // A named carrier part has a well-formed lattice.
        let tail = parts
            .iter()
            .find(|part| part["id"].as_str() == Some("Tail_3102679707"))
            .expect("Tail part present");
        let lattice = tail
            .get("translation_keyforms")
            .expect("Tail carries a translation lattice");
        assert!(!lattice["axes"].as_array().expect("axes").is_empty());
        assert!(!lattice["forms"].as_array().expect("forms").is_empty());
        for form in lattice["forms"].as_array().expect("forms") {
            assert_eq!(
                form["translation"].as_array().expect("translation").len(),
                2
            );
        }
        // The brow parts honestly do NOT convert: their
        // translation bindings span 3 parameter axes
        // (Head:: Yaw-Pitch x/y plus Eye:: Blink), past the
        // format's 2-axis lattice law — they drop with that
        // reason named, not silently.
        for brow_id in ["Eyebrow_Left_228585614", "Eyebrow_Right_4144177566"] {
            let brow = parts
                .iter()
                .find(|part| part["id"].as_str() == Some(brow_id))
                .unwrap_or_else(|| panic!("part {brow_id} present"));
            assert!(
                brow.get("translation_keyforms").is_none(),
                "{brow_id} spans 3 axes and must not carry a lattice"
            );
        }
        assert!(
            result.entries.iter().any(|entry| {
                entry.verdict == "dropped"
                    && entry.construct.contains("Eyebrow")
                    && entry.detail.contains("bindings span 3 parameter axes")
            }),
            "the brow drops name the 3-axis lattice limit"
        );
        // Accounting: the retired reason is gone, and fewer
        // transform.t bindings drop than the 126 baseline.
        let mut transform_t_drops = 0_usize;
        for entry in &result.entries {
            assert!(
                !entry.detail.contains("parts carry no transforms"),
                "retired drop reason leaked: {}",
                entry.detail
            );
            if entry.verdict == "dropped" && entry.construct.contains("transform.t") {
                transform_t_drops += 1;
            }
        }
        assert!(
            transform_t_drops < 126,
            "transform.t drops fell from the 126 baseline: {transform_t_drops}"
        );
    }

    #[test]
    fn exported_model_carries_the_lumen_token() {
        // The one-token law: the bridge export is a
        // lumen-rig model document, so it carries lumen's
        // token — never torogao's native token.
        assert_eq!(LUMEN_MODEL_FORMAT_TOKEN, "lumen-rig-model");
        assert_ne!(LUMEN_MODEL_FORMAT_TOKEN, torogao_format::MODEL_FORMAT_TOKEN);
    }

    #[test]
    fn keyform_scale_floor_repairs_feature_off_components() {
        // INP's "feature off" states are scale 0; lumen
        // requires sx, sy > 0 (LRF-305), so non-positive
        // components export at the floor.
        assert_eq!(keyform_scale_component(0.0), KEYFORM_SCALE_FLOOR);
        assert_eq!(keyform_scale_component(-2.5), KEYFORM_SCALE_FLOOR);
        assert_eq!(KEYFORM_SCALE_FLOOR, 1e-6);
        // Positive components pass through unchanged,
        // including small-but-positive ones.
        assert_eq!(keyform_scale_component(0.5), 0.5);
        assert_eq!(keyform_scale_component(1.0), 1.0);
        assert_eq!(keyform_scale_component(2.0e-6), 2.0e-6);
    }
}
