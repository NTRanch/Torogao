//! The rig exporter (rigexport.d): INP -> lumen-rig model
//! conversion. Every source construct is MAPPED,
//! APPROXIMATED, or DROPPED-WITH-REASON in the conversion
//! report; the exporter mechanically accounts for every
//! parameter binding, so silent loss is impossible by
//! construction.
//!
//! Orientation: INP space is y-down, lumen model space is
//! y-up. All baked positions flip y; rotation angles flip
//! sign. Under ruling RP1-D1, writers emit the current
//! format token only: the model document carries
//! `torogao-rig-model` (the reference writes its
//! predecessor token; that one field is the pre-authorized
//! difference between the two exporters).

use crate::extdoc::ExtView;
use crate::handlers::{ToolLoad, join_path, stamp_doc};
use crate::registry::ToolResult;
use crate::util::{f64_of_f32, fmt_g};
use crate::walk::{Walk, grid_dims, is_drawable, is_part, kind_name, slugify, suffixed_name};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use torogao_engine::param::BindingValues;
use torogao_engine::{Node, Puppet, Vec2};
use torogao_format::MODEL_FORMAT_TOKEN;

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
        };
        exporter.emit_parameters();
        exporter.assign_draw_ranks();
        let deformers = exporter.emit_deformers();
        let parts = exporter.emit_parts();
        let textures = exporter.emit_textures();
        let meshes = exporter.emit_meshes();
        let physics = exporter.emit_physics();
        exporter.account_bindings();
        exporter.account_whole_constructs();

        let model_name = if puppet.meta.name.is_empty() {
            "model".to_string()
        } else {
            slugify(&puppet.meta.name, 48)
        };
        let model = serde_json::json!({
            "deformers": deformers,
            "format": MODEL_FORMAT_TOKEN,
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
            let degenerate = !(param.min.x < param.max.x)
                || param.defaults.x < param.min.x
                || param.defaults.x > param.max.x
                || (param.is_vec2
                    && (!(param.min.y < param.max.y)
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
    fn collect_axes(
        &self,
        bindings: &[BindingRef],
    ) -> Result<Vec<AxisSrc>, String> {
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
                return Err(
                    "an axis parameter was dropped during parameter conversion".to_string(),
                );
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
            .map(|axis| {
                Value::Array(axis.keys.iter().map(|key| jv(*key)).collect())
            })
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
    if spring {
        "SpringPendulum"
    } else {
        "Pendulum"
    }
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

        let base_rz_deg =
            (-(node.local.rotation.z as f64) * 180.0 / std::f64::consts::PI) as f32;
        let base_sx = node.local.scale.x;
        let base_sy = node.local.scale.y;

        let nx = axes[0].keys.len();
        let ny = if axes.len() > 1 { axes[1].keys.len() } else { 1 };
        let mut forms = Vec::new();
        for i in 0..nx {
            for j in 0..ny {
                let mut angle = base_rz_deg;
                let (mut scale_x, mut scale_y) = (base_sx, base_sy);
                for binding in &rz {
                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                    let delta = (self.value_at(*binding, bx, by) as f64) * 180.0
                        / std::f64::consts::PI;
                    angle = (angle as f64 - delta) as f32;
                }
                for binding in &sx {
                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                    scale_x *= self.value_at(*binding, bx, by);
                }
                for binding in &sy {
                    let (bx, by) = self.binding_indices(*binding, &axes, i, j);
                    scale_y *= self.value_at(*binding, bx, by);
                }
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
                            format!(
                                "warp deformation cannot form a lumen lattice: {reason}"
                            ),
                        );
                    }
                }
                Ok(axes) => {
                    let nx = axes[0].keys.len();
                    let ny = if axes.len() > 1 { axes[1].keys.len() } else { 1 };
                    let mut forms = Vec::new();
                    for i in 0..nx {
                        for j in 0..ny {
                            let mut positions = Vec::new();
                            for (vi, vertex) in mesh.vertices.iter().enumerate() {
                                let mut point =
                                    self.walk.to_world(walk_idx, *vertex, mesh.origin);
                                for binding in &defs {
                                    let (bx, by) =
                                        self.binding_indices(*binding, &axes, i, j);
                                    let offset =
                                        self.deform_offset_at(*binding, bx, by, vi);
                                    let delta =
                                        self.walk.delta_to_world(walk_idx, offset);
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
                part["parent"] =
                    Value::String(self.walk.nodes[parent].part_id.clone());
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
            } else if node.texture_ids[0] >= 0
                && (node.texture_ids[0] as usize) >= self.slot_count
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
                blend_mode_for(&node.blend_mode, path_ref, &mut |verdict, construct, detail| {
                    blend_notes.push((verdict, construct, detail));
                })
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
                let src_mesh_id = src_walk
                    .and_then(|src| self.walk.nodes[src].mesh_id.clone());
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
                                format!(
                                    "mesh deformation cannot form a lumen lattice: {reason}"
                                ),
                            );
                        }
                    }
                    Ok(axes) => {
                        let nx = axes[0].keys.len();
                        let ny = if axes.len() > 1 { axes[1].keys.len() } else { 1 };
                        let mut forms = Vec::new();
                        for i in 0..nx {
                            for j in 0..ny {
                                let mut verts = Vec::new();
                                for (vi, vertex) in mesh.vertices.iter().enumerate() {
                                    let mut point = self.walk
                                        .to_world(walk_idx, *vertex, mesh.origin);
                                    for binding in &defs {
                                        let (bx, by) =
                                            self.binding_indices(*binding, &axes, i, j);
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
            if node.screen_tint.x != 0.0
                || node.screen_tint.y != 0.0
                || node.screen_tint.z != 0.0
            {
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
                    let id = self
                        .walk
                        .deformer_id_of(parent)
                        .unwrap_or("")
                        .to_string();
                    anchor = Some((parent, "deformer", id));
                    break;
                }
                if is_part(parent_node.kind)
                    && self.walk.nodes[parent].mesh_id.is_some()
                {
                    let id = self.walk.nodes[parent]
                        .mesh_id
                        .clone()
                        .unwrap_or_default();
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
            if !(state.length > 0.0) {
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
                    "deformation binding was not consumed by any exporter path"
                        .to_string()
                } else if channel.len() >= 10 && &channel.as_bytes()[..10] == b"transform." {
                    "node transform channels have no lumen v1 representation (parts carry no transforms); only transform.r.z / s.x / s.y are consumed, as rotation deformers"
                        .to_string()
                } else {
                    format!(
                        "property channel '{channel}' has no lumen v1 representation"
                    )
                };
                let construct = self.binding_construct(key);
                self.note("dropped", construct, reason);
            }
        }
    }

    fn account_whole_constructs(&mut self) {
        for (name, animation) in &self.puppet.animations {
            self.note(
                "dropped",
                format!("animation '{name}'"),
                format!(
                    "{} lane(s), {} keyframes: lumen motion assets are a separate asset kind (SPEC section 4) with no parser in lumen-rig-core R1; animations are not part of the model asset",
                    animation.lanes.len(),
                    animation.keyframe_count()
                ),
            );
        }
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
    let model_text = format!(
        "{}\n",
        torogao_format::jsonfmt::to_d_json(&result.model)
    );
    std::fs::write(&model_path, model_text).map_err(|err| ExportError::Io(err.to_string()))?;
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
pub fn export_rig(
    load: &ToolLoad,
    out_dir: &str,
    base: &str,
    ext_view: &ExtView,
) -> ToolResult {
    match export_rig_files(load, out_dir, base, ext_view) {
        Ok(report) => ToolResult::ok(report),
        Err(ExportError::Build(message)) => ToolResult::fail("export_failed", &message),
        Err(ExportError::Io(message)) => ToolResult::fail("io_error", &message),
    }
}
