//! The puppet: a loaded project — node arena, parameters,
//! automations, animations, drivers — plus the reference
//! load sequence (deserialize order, finalize order) and
//! the evaluation driver (`update`, driver stepping, the
//! scriptable clock).

use crate::animation::Animation;
use crate::automation::Automation;
use crate::math::{Vec2, Vec3, Vec4};
use crate::node::{MaskBinding, MeshData, Node, NodeArena, PhysicsState, SimplePhysicsState};
use crate::param::{apply_binding, Parameter};
use crate::physics::{Pendulum, PendulumIo, SpringIo, SpringPendulum};
use crate::transform::Transform;
use serde_json::Value;
use std::collections::HashMap;
use torogao_format::value::{jget, jnum, jstr};
use torogao_format::{FormatResult, InpFile, NodeKind, NodeRecord, PayloadDoc, TorogaoError};

/// Puppet metadata (payload `meta`).
#[derive(Debug, Clone, Default)]
pub struct PuppetMeta {
    /// Model name.
    pub name: String,
    /// Model version string.
    pub version: String,
    /// Artist credit.
    pub artist: String,
    /// Rigger credit.
    pub rigger: String,
}

/// The engine clock (the reference's global timing state):
/// `now` is the scriptable time source; `tick` is
/// `inUpdate()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Clock {
    /// The time source's current value (seconds).
    pub now: f64,
    /// currentTime().
    pub current: f64,
    /// lastTime().
    pub last: f64,
    /// deltaTime().
    pub delta: f64,
}

impl Clock {
    /// Advances the clock from the time source (the
    /// reference `inUpdate`).
    pub fn tick(&mut self) {
        self.current = self.now;
        self.delta = self.current - self.last;
        self.last = self.current;
    }
}

/// One embedded texture slot (non-empty slots only — the
/// reference's renderless texture list).
#[derive(Debug, Clone)]
pub struct TextureSlotInfo {
    /// Format name ("png" / "tga" / "bc7" / "unknown").
    pub format: String,
    /// Byte length of the texture data.
    pub bytes: usize,
}

/// A loaded puppet.
#[derive(Debug, Clone)]
pub struct Puppet {
    /// Metadata.
    pub meta: PuppetMeta,
    /// Gravity (payload physics, default 9.8).
    pub gravity: f32,
    /// Pixels per meter (default 1000).
    pub pixels_per_meter: f32,
    /// The node arena (root at `nodes.root`).
    pub nodes: NodeArena,
    /// Parameters, in payload order.
    pub params: Vec<Parameter>,
    /// Automations, in payload order (unknown types skipped).
    pub automations: Vec<Automation>,
    /// Animations by name, in payload order.
    pub animations: Vec<(String, Animation)>,
    /// Driver node indices, in scan order.
    pub drivers: Vec<usize>,
    /// Parameter index → driver node index.
    pub driven: HashMap<usize, usize>,
    /// Root parts (parts and composites outside composites),
    /// z-sorted descending after finalize.
    pub root_parts: Vec<usize>,
    /// Embedded texture slots (non-empty only).
    pub texture_slots: Vec<TextureSlotInfo>,
    /// EXT sections by name, in file order (the reference
    /// stores them in a map where a later duplicate wins;
    /// lookups here scan from the end).
    pub ext_data: Vec<(String, Vec<u8>)>,
    /// The puppet transform (identity in the CLI profile).
    pub transform: Transform,
    /// Whether parameter evaluation runs.
    pub render_parameters: bool,
    /// Whether drivers run.
    pub enable_drivers: bool,
    /// The engine clock.
    pub clock: Clock,
}

fn malformed(detail: &str) -> TorogaoError {
    TorogaoError::new(
        torogao_format::container::TG_CONTAINER_MALFORMED,
        format!("project could not be loaded: malformed INP container ({detail})"),
    )
}

fn f32_of(value: &Value) -> f32 {
    value.as_f64().unwrap_or(0.0) as f32
}

fn vec2_of(value: &Value) -> Vec2 {
    match value {
        Value::Array(items) => Vec2::new(
            items.first().map(f32_of).unwrap_or(0.0),
            items.get(1).map(f32_of).unwrap_or(0.0),
        ),
        _ => Vec2::ZERO,
    }
}

fn vec3_of(value: &Value, default: Vec3) -> Vec3 {
    match value {
        Value::Array(items) => Vec3::new(
            items.first().map(f32_of).unwrap_or(default.x),
            items.get(1).map(f32_of).unwrap_or(default.y),
            items.get(2).map(f32_of).unwrap_or(default.z),
        ),
        _ => default,
    }
}

fn transform_of(raw: &Value) -> Result<Transform, TorogaoError> {
    let mut transform = Transform::default();
    match raw {
        Value::Object(_) => {
            transform.translation = vec3_of(jget(raw, "trans"), Vec3::ZERO);
            transform.rotation = vec3_of(jget(raw, "rot"), Vec3::ZERO);
            transform.scale = vec2_of(jget(raw, "scale"));
            if jget(raw, "scale").is_null() {
                transform.scale = Vec2::ONE;
            } else if let Value::Array(items) = jget(raw, "scale") {
                transform.scale = Vec2::new(
                    items.first().map(f32_of).unwrap_or(1.0),
                    items.get(1).map(f32_of).unwrap_or(1.0),
                );
            }
            transform.update();
            Ok(transform)
        }
        Value::Null => Ok(transform),
        _ => Err(malformed("node transform is not an object")),
    }
}

fn mesh_of(raw: &Value) -> Result<MeshData, TorogaoError> {
    if !raw.is_object() {
        return Err(malformed("drawable carries no mesh object"));
    }
    let flat_pairs = |value: &Value| -> Vec<Vec2> {
        match value {
            Value::Array(items) => items
                .chunks(2)
                .map(|pair| {
                    Vec2::new(
                        pair.first().map(f32_of).unwrap_or(0.0),
                        pair.get(1).map(f32_of).unwrap_or(0.0),
                    )
                })
                .collect(),
            _ => Vec::new(),
        }
    };
    let indices = match jget(raw, "indices") {
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_u64().unwrap_or(0) as u16)
            .collect(),
        _ => Vec::new(),
    };
    Ok(MeshData {
        vertices: flat_pairs(jget(raw, "verts")),
        uvs: flat_pairs(jget(raw, "uvs")),
        indices,
        origin: vec2_of(jget(raw, "origin")),
    })
}

fn masks_of(raw: &Value) -> Vec<MaskBinding> {
    let mut masks = Vec::new();
    if let Value::Array(items) = jget(raw, "masks") {
        for item in items {
            masks.push(MaskBinding {
                src_uuid: jnum(item, "source", 0.0) as u32,
                mode: jstr(item, "mode").to_string(),
                src: None,
            });
        }
        return masks;
    }
    if let Value::Array(items) = jget(raw, "masked_by") {
        let mode = jstr(raw, "mask_mode").to_string();
        for item in items {
            masks.push(MaskBinding {
                src_uuid: item.as_u64().unwrap_or(0) as u32,
                mode: mode.clone(),
                src: None,
            });
        }
    }
    masks
}

fn build_node(
    arena: &mut NodeArena,
    record: &NodeRecord,
    parent: Option<usize>,
) -> FormatResult<usize> {
    let raw = &record.raw;
    let uuid = match jget(raw, "uuid") {
        Value::Number(number) => number.as_u64().unwrap_or(u32::MAX as u64) as u32,
        _ => return Err(malformed("node record carries no numeric uuid")),
    };
    let enabled = match jget(raw, "enabled") {
        Value::Bool(flag) => *flag,
        _ => return Err(malformed("node record carries no boolean enabled")),
    };
    let zsort = match jget(raw, "zsort") {
        Value::Number(number) => number.as_f64().unwrap_or(0.0) as f32,
        _ => return Err(malformed("node record carries no numeric zsort")),
    };
    let name = {
        let text = jstr(raw, "name");
        if text.is_empty() {
            "Unnamed Node".to_string()
        } else {
            text.to_string()
        }
    };
    let mut node = Node::new(uuid, name, record.kind);
    node.enabled = enabled;
    node.zsort = zsort;
    node.parent = parent;
    node.lock_to_root = matches!(jget(raw, "lockToRoot"), Value::Bool(true));
    node.local = transform_of(jget(raw, "transform"))?;

    match record.kind {
        NodeKind::Part | NodeKind::AnimatedPart | NodeKind::MeshGroup => {
            let mesh = mesh_of(jget(raw, "mesh"))?;
            node.deformation = vec![Vec2::ZERO; mesh.vertices.len()];
            node.mesh = Some(mesh);
        }
        _ => {}
    }
    match record.kind {
        NodeKind::Part | NodeKind::AnimatedPart => {
            if let Value::Array(items) = jget(raw, "textures") {
                node.texture_ids = items
                    .iter()
                    .map(|item| item.as_u64().unwrap_or(0) as u32)
                    .filter(|id| *id != u32::MAX)
                    .map(|id| id as i32)
                    .collect();
            }
            node.opacity = jnum(raw, "opacity", 1.0) as f32;
            node.mask_threshold = jnum(raw, "mask_threshold", 0.0) as f32;
            node.tint = vec3_of(jget(raw, "tint"), Vec3::ONE);
            node.screen_tint = vec3_of(jget(raw, "screenTint"), Vec3::ZERO);
            // The reference deserializes the payload's
            // emissionStrength INTO tint (a float into a
            // vec3), which fails silently: the field never
            // loads and tint is unaffected. Mirrored by
            // ignoring the key entirely.
            let blend = jstr(raw, "blend_mode");
            if !blend.is_empty() {
                node.blend_mode = blend.to_string();
            }
            node.masks = masks_of(raw);
        }
        NodeKind::MeshGroup => {
            node.dynamic = matches!(jget(raw, "dynamic_deformation"), Value::Bool(true));
            node.translate_children = false;
            if let Value::Bool(flag) = jget(raw, "translate_children") {
                node.translate_children = *flag;
            }
        }
        NodeKind::Composite => {
            node.propagate_mesh_group =
                !matches!(jget(raw, "propagate_meshgroup"), Value::Bool(false));
            node.opacity = jnum(raw, "opacity", 1.0) as f32;
            node.comp_threshold = jnum(raw, "threshold", 0.5) as f32;
            node.tint = vec3_of(jget(raw, "tint"), Vec3::ONE);
            node.screen_tint = vec3_of(jget(raw, "screenTint"), Vec3::ZERO);
            let blend = jstr(raw, "blending_mode");
            if !blend.is_empty() {
                node.blend_mode = blend.to_string();
            }
            node.masks = masks_of(raw);
        }
        NodeKind::SimplePhysics => {
            let mut state = SimplePhysicsState::default();
            state.param_ref = jnum(raw, "param", 0.0) as u32;
            // Both spellings occur in the wild: payload
            // fixtures carry the serde values ("pendulum"),
            // while the template writer emits the enum
            // member names ("Pendulum"). The reference's
            // enum deserialization accepts both; so does
            // this loader.
            state.spring = matches!(
                jstr(raw, "model_type"),
                "SpringPendulum" | "spring_pendulum"
            );
            state.map_mode = match jstr(raw, "map_mode") {
                "XY" | "xy" => 1,
                "LengthAngle" | "length_angle" => 2,
                "YX" | "yx" => 3,
                _ => 0,
            };
            state.gravity = jnum(raw, "gravity", 1.0) as f32;
            state.length = jnum(raw, "length", 100.0) as f32;
            state.frequency = jnum(raw, "frequency", 1.0) as f32;
            state.angle_damping = jnum(raw, "angle_damping", 0.5) as f32;
            state.length_damping = jnum(raw, "length_damping", 0.5) as f32;
            if let Value::Array(items) = jget(raw, "output_scale") {
                state.output_scale = Vec2::new(
                    items.first().map(f32_of).unwrap_or(1.0),
                    items.get(1).map(f32_of).unwrap_or(1.0),
                );
            }
            state.local_only = matches!(jget(raw, "local_only"), Value::Bool(true));
            node.physics = Some(Box::new(state));
        }
        _ => {}
    }

    let idx = arena.nodes.len();
    arena.nodes.push(node);
    let mut child_indices = Vec::new();
    for child in &record.children {
        child_indices.push(build_node(arena, child, Some(idx))?);
    }
    arena.nodes[idx].children = child_indices;
    Ok(idx)
}

impl Puppet {
    /// Loads a puppet from a decoded payload + its container
    /// (the reference load sequence: sections deserialize,
    /// then finalize runs root, parameters, automations,
    /// animations, and the parts scan in that order).
    pub fn load(doc: &PayloadDoc, inp: &InpFile) -> FormatResult<Puppet> {
        let meta = PuppetMeta {
            name: jstr(&doc.meta, "name").to_string(),
            version: jstr(&doc.meta, "version").to_string(),
            artist: jstr(&doc.meta, "artist").to_string(),
            rigger: jstr(&doc.meta, "rigger").to_string(),
        };
        let gravity = jnum(&doc.physics, "gravity", 9.8) as f32;
        let pixels_per_meter = jnum(&doc.physics, "pixelsPerMeter", 1000.0) as f32;

        let mut arena = NodeArena::default();
        let root = build_node(&mut arena, &doc.root, None)?;
        arena.root = root;

        let mut params = Vec::new();
        for raw in &doc.params {
            let param = Parameter::from_payload(raw).map_err(|err| malformed(&err.0))?;
            params.push(param);
        }
        let automations: Vec<Automation> = doc
            .automation
            .iter()
            .filter_map(Automation::from_payload)
            .collect();
        let animations = decode_animations(&doc.animations);
        let texture_slots = inp
            .textures
            .iter()
            .filter(|slot| !slot.data.is_empty())
            .map(|slot| TextureSlotInfo {
                format: slot.format_name().to_string(),
                bytes: slot.data.len(),
            })
            .collect();

        let mut puppet = Puppet {
            meta,
            gravity,
            pixels_per_meter,
            nodes: arena,
            params,
            automations,
            animations,
            drivers: Vec::new(),
            driven: HashMap::new(),
            root_parts: Vec::new(),
            texture_slots,
            ext_data: inp
                .ext_sections
                .iter()
                .map(|section| (section.name.clone(), section.data.clone()))
                .collect(),
            transform: Transform::default(),
            render_parameters: true,
            enable_drivers: true,
            clock: Clock::default(),
        };
        puppet.finalize()?;
        Ok(puppet)
    }

    fn finalize(&mut self) -> FormatResult<()> {
        // Root name is forced at finalize.
        let root = self.nodes.root;
        self.nodes.nodes[root].name = "Root".to_string();
        // Node finalize: composite children order is payload
        // order already; parts resolve mask sources,
        // physics nodes resolve their parameter + reset.
        let order = self.nodes.preorder(root);
        for idx in order {
            if matches!(
                self.nodes.nodes[idx].kind,
                NodeKind::Part | NodeKind::AnimatedPart | NodeKind::Composite
            ) {
                let masks = self.nodes.nodes[idx].masks.clone();
                let mut resolved = Vec::new();
                for mask in masks {
                    if let Some(src) = self.nodes.find_by_uuid(mask.src_uuid) {
                        if self.nodes.nodes[src].is_drawable() {
                            resolved.push(MaskBinding {
                                src: Some(src),
                                ..mask
                            });
                        }
                    }
                }
                self.nodes.nodes[idx].masks = resolved;
            }
            if self.nodes.nodes[idx].kind == NodeKind::SimplePhysics {
                let param_ref = self.nodes.nodes[idx]
                    .physics
                    .as_ref()
                    .map(|s| s.param_ref)
                    .unwrap_or(0);
                let param = self.params.iter().position(|p| p.uuid == param_ref);
                if let Some(state) = self.nodes.nodes[idx].physics.as_mut() {
                    state.param = param;
                }
                self.physics_reset(idx);
            }
        }
        // Parameter finalize: value = defaults; bindings
        // whose node does not resolve are dropped.
        let resolutions: Vec<Vec<Option<usize>>> = self
            .params
            .iter()
            .map(|param| {
                param
                    .bindings
                    .iter()
                    .map(|binding| self.nodes.find_by_uuid(binding.node_ref))
                    .collect()
            })
            .collect();
        for (param, resolved) in self.params.iter_mut().zip(resolutions) {
            param.value = param.defaults;
            let mut kept = Vec::with_capacity(param.bindings.len());
            for (mut binding, node) in std::mem::take(&mut param.bindings)
                .into_iter()
                .zip(resolved)
            {
                if let Some(node) = node {
                    binding.node = Some(node);
                    kept.push(binding);
                }
            }
            param.bindings = kept;
        }
        // Automation finalize (parameter resolution by name).
        let params_snapshot = self.params.clone();
        for automation in self.automations.iter_mut() {
            automation.finalize(&params_snapshot);
        }
        self.scan_parts();
        Ok(())
    }

    fn scan_parts(&mut self) {
        fn recurse(puppet: &mut Puppet, idx: usize, drivers_only: bool) {
            let kind = puppet.nodes.nodes[idx].kind;
            let mut drivers_only = drivers_only;
            if kind == NodeKind::SimplePhysics {
                puppet.drivers.push(idx);
                if let Some(param) = puppet.nodes.nodes[idx]
                    .physics
                    .as_ref()
                    .and_then(|s| s.param)
                {
                    puppet.driven.insert(param, idx);
                }
            } else if !drivers_only {
                if kind == NodeKind::Composite {
                    puppet.root_parts.push(idx);
                    drivers_only = true;
                } else if matches!(kind, NodeKind::Part | NodeKind::AnimatedPart) {
                    puppet.root_parts.push(idx);
                }
            }
            let children = puppet.nodes.nodes[idx].children.clone();
            for child in children {
                recurse(puppet, child, drivers_only);
            }
        }
        let root = self.nodes.root;
        recurse(self, root, false);
        // selfSort: stable sort by zSort descending.
        let nodes = &self.nodes;
        self.root_parts
            .sort_by(|a, b| nodes.nodes[*b].zsort.partial_cmp(&nodes.nodes[*a].zsort).unwrap_or(std::cmp::Ordering::Equal));
    }

    /// One evaluation frame (the reference `Puppet.update`).
    pub fn update(&mut self) -> Result<(), String> {
        self.transform.update();
        let current = self.clock.current;
        let delta = self.clock.delta;
        let mut automations = std::mem::take(&mut self.automations);
        for automation in automations.iter_mut() {
            if automation.enabled {
                automation.update(&mut self.params, current, delta);
            }
        }
        self.automations = automations;
        let root = self.nodes.root;
        self.nodes.begin_update(root);
        if self.render_parameters {
            for idx in 0..self.params.len() {
                if self.enable_drivers && self.driven.contains_key(&idx) {
                    continue;
                }
                self.update_param(idx)?;
            }
        }
        self.nodes.transform_changed(root);
        if self.render_parameters && self.enable_drivers {
            let drivers = self.drivers.clone();
            for driver in drivers {
                self.update_driver(driver)?;
            }
        }
        self.nodes.update(root);
        Ok(())
    }

    /// Runs one parameter's update (head, bindings, tail).
    pub fn update_param(&mut self, idx: usize) -> Result<(), String> {
        if !self.params[idx].update_head() {
            return Ok(());
        }
        let param = self.params[idx].clone();
        for binding in &param.bindings {
            apply_binding(&mut self.nodes, binding, &param)?;
        }
        self.params[idx].update_tail();
        Ok(())
    }

    /// Resets every driver (the reference `resetDrivers`,
    /// which also advances the clock once).
    pub fn reset_drivers(&mut self) {
        let drivers = self.drivers.clone();
        for driver in drivers {
            self.physics_reset(driver);
        }
        self.clock.tick();
    }

    /// Enables fixed-step mode on every physics driver
    /// (the preview's D15 behavior).
    pub fn set_drivers_fixed_step(&mut self, fixed: bool) {
        for idx in 0..self.nodes.nodes.len() {
            if let Some(state) = self.nodes.nodes[idx].physics.as_mut() {
                state.fixed_step = fixed;
            }
        }
    }

    /// The anchor for a physics node, per its local-only
    /// flag (the reference `updateInputs`, run when no
    /// previous anchor was captured).
    fn physics_anchor(&mut self, idx: usize) -> Vec2 {
        let local_only = self.nodes.nodes[idx]
            .physics
            .as_ref()
            .map(|s| s.local_only)
            .unwrap_or(false);
        if local_only {
            self.nodes.transform_local(idx).translation.xy()
        } else {
            let global = self.nodes.transform(idx);
            (global.trs * Vec4::new(0.0, 0.0, 0.0, 1.0)).xy()
        }
    }

    /// Resets one physics driver: refresh the anchor, build
    /// a fresh system hanging below it (the reference
    /// `SimplePhysics.reset`).
    pub fn physics_reset(&mut self, idx: usize) {
        let anchor = self.physics_anchor(idx);
        let node = &mut self.nodes.nodes[idx];
        let Some(state) = node.physics.as_mut() else {
            return;
        };
        state.fixed_remainder = 0.0;
        state.anchor = anchor;
        let length = state.eff_length();
        state.system = Some(if state.spring {
            PhysicsState::Spring(SpringPendulum::new(anchor, length))
        } else {
            PhysicsState::Pendulum(Pendulum::new(anchor, length))
        });
    }

    /// Steps one physics driver (the reference
    /// `SimplePhysics.updateDriver`): integrate with the
    /// frame delta (fixed-step when enabled), then push the
    /// output into the driven parameter and update it.
    pub fn update_driver(&mut self, idx: usize) -> Result<(), String> {
        let h = (self.clock.delta as f32).min(10.0);
        // updateInputs
        {
            let state = self.nodes.nodes[idx].physics.as_ref().expect("physics");
            if !state.prev_anchor_set {
                let anchor = self.physics_anchor(idx);
                self.nodes.nodes[idx].physics.as_mut().expect("physics").anchor = anchor;
            }
        }
        // integration
        {
            let (gravity, ppm) = (self.gravity, self.pixels_per_meter);
            let node = &mut self.nodes.nodes[idx];
            let state = node.physics.as_mut().expect("physics");
            let io_gravity = state.eff_gravity(gravity, ppm);
            let io_length = state.eff_length();
            let io_frequency = state.eff_frequency();
            let io_angle_damping = state.eff_angle_damping();
            let io_length_damping = state.eff_length_damping();
            let anchor = state.anchor;
            let fixed = state.fixed_step;
            if fixed {
                state.fixed_remainder += h;
                while state.fixed_remainder >= 0.01 {
                    tick_system(state, anchor, io_gravity, io_length, io_frequency, io_angle_damping, io_length_damping, 0.01);
                    state.fixed_remainder -= 0.01;
                }
            } else {
                let mut rest = h;
                while rest > 0.01 {
                    tick_system(state, anchor, io_gravity, io_length, io_frequency, io_angle_damping, io_length_damping, 0.01);
                    rest -= 0.01;
                }
                tick_system(state, anchor, io_gravity, io_length, io_frequency, io_angle_damping, io_length_damping, rest);
            }
        }
        // updateOutputs
        self.physics_update_outputs(idx)?;
        if let Some(state) = self.nodes.nodes[idx].physics.as_mut() {
            state.prev_anchor_set = false;
        }
        Ok(())
    }

    fn physics_update_outputs(&mut self, idx: usize) -> Result<(), String> {
        let (param_idx, output, anchor) = {
            let state = self.nodes.nodes[idx].physics.as_ref().expect("physics");
            let Some(param) = state.param else {
                return Ok(());
            };
            let output = match &state.system {
                Some(PhysicsState::Pendulum(p)) => p.output,
                Some(PhysicsState::Spring(s)) => s.output,
                None => Vec2::ZERO,
            };
            (param, output, state.anchor)
        };
        let state = self.nodes.nodes[idx].physics.as_ref().expect("physics");
        let oscale = state.eff_output_scale();
        let local_only = state.local_only;
        let map_mode = state.map_mode;
        let length = state.eff_length();
        let local_pos = if local_only {
            Vec4::new(output.x, output.y, 0.0, 1.0)
        } else {
            let matrix = if state.prev_anchor_set {
                state.prev_trans
            } else {
                self.nodes.transform(idx).trs.inverse()
            };
            matrix * Vec4::new(output.x, output.y, 0.0, 1.0)
        };
        let mut local_angle = local_pos.xy();
        local_angle.normalize();
        let rel_length = output.distance(anchor) / length;
        let angle = (-local_angle.x).atan2(local_angle.y) as f64 / std::f64::consts::PI;
        let p = match map_mode {
            1 => {
                let mut xy = Vec2::new(local_angle.x * rel_length, local_angle.y * rel_length);
                xy.y -= 1.0;
                xy.y = -xy.y;
                xy
            }
            2 => Vec2::new(rel_length, angle as f32),
            3 => {
                let mut xy = Vec2::new(local_angle.x * rel_length, local_angle.y * rel_length);
                xy.y -= 1.0;
                xy.y = -xy.y;
                Vec2::new(xy.y, xy.x)
            }
            _ => Vec2::new(angle as f32, rel_length),
        };
        let push = Vec2::new(p.x * oscale.x, p.y * oscale.y);
        self.params[param_idx].push_offset(push, crate::param::MergeMode::Forced, 1.0);
        self.update_param(param_idx)
    }

    /// Finds a parameter's index by name (first match).
    pub fn param_index(&self, name: &str) -> Option<usize> {
        self.params.iter().position(|p| p.name == name)
    }

    /// The raw bytes of a named EXT section (last one wins,
    /// as in the reference's map).
    pub fn ext_section(&self, name: &str) -> Option<&[u8]> {
        self.ext_data
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, data)| data.as_slice())
    }

    /// Whether a parameter is driven by a driver.
    pub fn is_driven(&self, idx: usize) -> bool {
        self.driven.contains_key(&idx)
    }
}

fn tick_system(
    state: &mut SimplePhysicsState,
    anchor: Vec2,
    gravity: f32,
    length: f32,
    frequency: f32,
    angle_damping: f32,
    length_damping: f32,
    h: f32,
) {
    let Some(system) = state.system.as_mut() else {
        return;
    };
    match system {
        PhysicsState::Pendulum(pendulum) => {
            let io = PendulumIo {
                anchor,
                gravity,
                length,
                angle_damping,
            };
            pendulum.tick(h, &io);
        }
        PhysicsState::Spring(spring) => {
            let io = SpringIo {
                anchor,
                gravity,
                length,
                frequency,
                angle_damping,
                length_damping,
            };
            spring.tick(h, &io);
        }
    }
}

fn decode_animations(raw: &Value) -> Vec<(String, Animation)> {
    let mut out = Vec::new();
    match raw {
        Value::Object(map) => {
            for (name, value) in map {
                out.push((name.clone(), Animation::from_payload(value)));
            }
        }
        Value::Array(items) => {
            for item in items {
                out.push((jstr(item, "name").to_string(), Animation::from_payload(item)));
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Clock, Puppet};
    use torogao_format::{read_inp, decode_payload};

    fn rich_puppet() -> Puppet {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/p2/fixtures/rich.inx"
        );
        let bytes = std::fs::read(path).expect("rich fixture");
        let inp = read_inp(&bytes).expect("container");
        let doc = decode_payload(&inp.payload).expect("payload");
        Puppet::load(&doc, &inp).expect("puppet loads")
    }

    #[test]
    fn rich_fixture_loads_and_scans() {
        let puppet = rich_puppet();
        assert_eq!(puppet.meta.name, "Rich Fixture");
        assert_eq!(puppet.params.len(), 4);
        assert_eq!(puppet.drivers.len(), 1);
        assert_eq!(puppet.texture_slots.len(), 1);
    }

    #[test]
    fn clock_tick_computes_delta() {
        let mut clock = Clock::default();
        clock.now = 0.5;
        clock.tick();
        assert_eq!(clock.delta, 0.5);
        clock.now = 0.75;
        clock.tick();
        assert_eq!(clock.delta, 0.25);
    }

    #[test]
    fn update_runs_without_error() {
        let mut puppet = rich_puppet();
        puppet.clock.now = 1.0 / 60.0;
        puppet.clock.tick();
        puppet.update().expect("update");
    }
}
