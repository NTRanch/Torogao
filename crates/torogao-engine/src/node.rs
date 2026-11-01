//! The node arena: all puppet nodes in one vector, parents
//! and children as indices, with the reference update
//! traversal ported per node kind — begin/update phases,
//! offset accumulation, lazy world transforms, the
//! mesh-group warp filter, and the simple-physics driver's
//! anchor tracking.

use crate::math::{Mat3, Mat4, Vec2, Vec3, Vec4};
use crate::physics::{Pendulum, SpringPendulum};
use crate::transform::Transform;
use torogao_format::NodeKind;

/// A drawable's mesh (payload `mesh` record).
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    /// Vertex positions.
    pub vertices: Vec<Vec2>,
    /// UV coordinates.
    pub uvs: Vec<Vec2>,
    /// Triangle indices.
    pub indices: Vec<u16>,
    /// Mesh origin.
    pub origin: Vec2,
}

/// A mask binding from a part/composite to a drawable.
#[derive(Debug, Clone)]
pub struct MaskBinding {
    /// The source drawable's uuid (payload form).
    pub src_uuid: u32,
    /// Mask mode name as authored ("Mask" / "DodgeMask").
    pub mode: String,
    /// The resolved source node (finalize; dropped when it
    /// does not resolve to a drawable).
    pub src: Option<usize>,
}

/// One precalculated warp triangle.
#[derive(Debug, Clone, Copy, Default)]
pub struct WarpTriangle {
    /// The triangle's local transform matrix.
    pub transform: Mat3,
    /// The triangle's rest-space offset matrix.
    pub offset: Mat3,
}

/// A mesh group's precalculated warp state.
#[derive(Debug, Clone, Default)]
pub struct WarpData {
    /// Bounds (min.x, min.y, max.x, max.y), floored/ceiled.
    pub bounds: [f32; 4],
    /// Lattice width in cells.
    pub width: usize,
    /// Lattice height in cells.
    pub height: usize,
    /// Per-cell triangle index + 1 (0 = no triangle).
    pub bitmask: Vec<i32>,
    /// Per-triangle data.
    pub triangles: Vec<WarpTriangle>,
    /// The group's forward matrix (this frame).
    pub forward: Mat4,
    /// The group's inverse matrix (this frame).
    pub inverse: Mat4,
}

/// The physics system behind a simple-physics node.
#[derive(Debug, Clone)]
pub enum PhysicsState {
    /// A simple pendulum.
    Pendulum(Pendulum),
    /// A spring pendulum.
    Spring(SpringPendulum),
}

/// A simple-physics node's full state (payload fields,
/// offset accumulators, and the running system).
#[derive(Debug, Clone)]
pub struct SimplePhysicsState {
    /// The driven parameter's uuid (payload `param`).
    pub param_ref: u32,
    /// The resolved parameter index.
    pub param: Option<usize>,
    /// "pendulum" or "spring_pendulum".
    pub spring: bool,
    /// Output mapping mode ordinal: 0 angle_length,
    /// 1 xy, 2 length_angle, 3 yx.
    pub map_mode: u8,
    /// Base gravity multiplier.
    pub gravity: f32,
    /// Base length.
    pub length: f32,
    /// Base frequency.
    pub frequency: f32,
    /// Base angle damping.
    pub angle_damping: f32,
    /// Base length damping.
    pub length_damping: f32,
    /// Base output scale.
    pub output_scale: Vec2,
    /// Whether the anchor is read in local space.
    pub local_only: bool,
    /// Offset: gravity multiplier.
    pub offset_gravity: f32,
    /// Offset: length addition.
    pub offset_length: f32,
    /// Offset: frequency multiplier.
    pub offset_frequency: f32,
    /// Offset: angle damping multiplier.
    pub offset_angle_damping: f32,
    /// Offset: length damping multiplier.
    pub offset_length_damping: f32,
    /// Offset: output scale multiplier.
    pub offset_output_scale: Vec2,
    /// The anchor point (world or local).
    pub anchor: Vec2,
    /// The running system, built at reset.
    pub system: Option<PhysicsState>,
    /// Whether the previous anchor transform was captured.
    pub prev_anchor_set: bool,
    /// The previous anchor transform's inverse.
    pub prev_trans: Mat4,
    /// Fixed-step mode (the preview enables it).
    pub fixed_step: bool,
    /// Fixed-step time remainder.
    pub fixed_remainder: f32,
}

impl Default for SimplePhysicsState {
    fn default() -> SimplePhysicsState {
        SimplePhysicsState {
            param_ref: 0,
            param: None,
            spring: false,
            map_mode: 0,
            gravity: 1.0,
            length: 100.0,
            frequency: 1.0,
            angle_damping: 0.5,
            length_damping: 0.5,
            output_scale: Vec2::ONE,
            local_only: false,
            offset_gravity: 1.0,
            offset_length: 0.0,
            offset_frequency: 1.0,
            offset_angle_damping: 1.0,
            offset_length_damping: 1.0,
            offset_output_scale: Vec2::ONE,
            anchor: Vec2::ZERO,
            system: None,
            prev_anchor_set: false,
            prev_trans: Mat4::ZERO,
            fixed_step: false,
            fixed_remainder: 0.0,
        }
    }
}

impl SimplePhysicsState {
    /// Effective gravity: base * offset * puppet gravity *
    /// pixels per meter (the reference `getGravity`, via
    /// `getScale`).
    pub fn eff_gravity(&self, puppet_gravity: f32, pixels_per_meter: f32) -> f32 {
        self.gravity * self.offset_gravity * puppet_gravity * pixels_per_meter
    }

    /// Effective length.
    pub fn eff_length(&self) -> f32 {
        self.length + self.offset_length
    }

    /// Effective frequency.
    pub fn eff_frequency(&self) -> f32 {
        self.frequency * self.offset_frequency
    }

    /// Effective angle damping.
    pub fn eff_angle_damping(&self) -> f32 {
        self.angle_damping * self.offset_angle_damping
    }

    /// Effective length damping.
    pub fn eff_length_damping(&self) -> f32 {
        self.length_damping * self.offset_length_damping
    }

    /// Effective output scale.
    pub fn eff_output_scale(&self) -> Vec2 {
        self.output_scale * self.offset_output_scale
    }
}

/// One node in the arena.
#[derive(Debug, Clone)]
pub struct Node {
    /// Node uuid.
    pub uuid: u32,
    /// Node name.
    pub name: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Whether update traversal runs this subtree.
    pub enabled: bool,
    /// Draw order offset base.
    pub zsort: f32,
    /// Whether the node transforms against the root.
    pub lock_to_root: bool,
    /// Parent index.
    pub parent: Option<usize>,
    /// Child indices, in payload order.
    pub children: Vec<usize>,
    /// Local transform.
    pub local: Transform,
    /// Offset transform (binding-driven).
    pub offset: Transform,
    /// Offset draw order.
    pub offset_sort: f32,
    /// Cached world transform.
    pub global: Transform,
    /// Whether the world transform needs recomputation.
    pub recalc: bool,
    /// Drawable mesh (Part / MeshGroup).
    pub mesh: Option<MeshData>,
    /// Current deformation (zeroed each begin-update).
    pub deformation: Vec<Vec2>,
    /// Part texture slot ids (uint.max entries dropped).
    pub texture_ids: Vec<i32>,
    /// Part/composite base opacity.
    pub opacity: f32,
    /// Part mask alpha threshold.
    pub mask_threshold: f32,
    /// Part/composite base tint.
    pub tint: Vec3,
    /// Part/composite base screen tint.
    pub screen_tint: Vec3,
    /// Blend mode name as authored.
    pub blend_mode: String,
    /// Mask bindings (Part / Composite).
    pub masks: Vec<MaskBinding>,
    /// Offset: opacity multiplier.
    pub offset_opacity: f32,
    /// Offset: mask threshold multiplier.
    pub offset_mask_threshold: f32,
    /// Offset: tint multiplier.
    pub offset_tint: Vec3,
    /// Offset: screen tint addition.
    pub offset_screen_tint: Vec3,
    /// Offset: emission strength addition (Part).
    pub offset_emission: f32,
    /// Whether the emission strength payload was present
    /// (kept for census parity; see the loader note about
    /// the reference's emissionStrength quirk).
    pub emission_strength: f32,
    /// MeshGroup: dynamic deformation mode.
    pub dynamic: bool,
    /// MeshGroup: translate children flag.
    pub translate_children: bool,
    /// MeshGroup: warp state.
    pub warp: Option<WarpData>,
    /// MeshGroup: warp precalculated flag.
    pub precalculated: bool,
    /// Warp filter installed by an ancestor group (pre phase).
    pub pre_warp: Option<usize>,
    /// Warp filter installed by an ancestor group (post phase).
    pub post_warp: Option<usize>,
    /// Pre-process guard flag.
    pub pre_processed: bool,
    /// Post-process guard flag.
    pub post_processed: bool,
    /// Composite: propagate mesh group flag.
    pub propagate_mesh_group: bool,
    /// Composite: mask threshold.
    pub comp_threshold: f32,
    /// Simple physics state.
    pub physics: Option<Box<SimplePhysicsState>>,
    /// Matrix override handed back by a dynamic warp filter
    /// (rendering-only in the reference; retained for
    /// structural parity, never consumed by the CLI).
    pub override_matrix: Option<Mat4>,
}

impl Node {
    /// A node with reference field defaults.
    pub fn new(uuid: u32, name: String, kind: NodeKind) -> Node {
        Node {
            uuid,
            name,
            kind,
            enabled: true,
            zsort: 0.0,
            lock_to_root: false,
            parent: None,
            children: Vec::new(),
            local: Transform::default(),
            offset: Transform::default(),
            offset_sort: 0.0,
            global: Transform::default(),
            recalc: true,
            mesh: None,
            deformation: Vec::new(),
            texture_ids: Vec::new(),
            opacity: 1.0,
            mask_threshold: 0.0,
            tint: Vec3::ONE,
            screen_tint: Vec3::ZERO,
            blend_mode: "Normal".to_string(),
            masks: Vec::new(),
            offset_opacity: 1.0,
            offset_mask_threshold: 0.0,
            offset_tint: Vec3::ZERO,
            offset_screen_tint: Vec3::ZERO,
            offset_emission: 1.0,
            emission_strength: 1.0,
            dynamic: false,
            translate_children: true,
            warp: None,
            precalculated: false,
            pre_warp: None,
            post_warp: None,
            pre_processed: false,
            post_processed: false,
            propagate_mesh_group: true,
            comp_threshold: 0.5,
            physics: None,
            override_matrix: None,
        }
    }

    /// Whether this node is a drawable (Part / MeshGroup /
    /// AnimatedPart).
    pub fn is_drawable(&self) -> bool {
        matches!(
            self.kind,
            NodeKind::Part | NodeKind::AnimatedPart | NodeKind::MeshGroup
        )
    }

    /// The relative draw order (zsort + offset).
    pub fn rel_zsort(&self) -> f32 {
        self.zsort + self.offset_sort
    }
}

/// The node arena with the reference traversal operations.
#[derive(Debug, Clone, Default)]
pub struct NodeArena {
    /// All nodes; the root is index `root`.
    pub nodes: Vec<Node>,
    /// The root node index.
    pub root: usize,
}

impl NodeArena {
    /// Marks a node and all descendants as needing transform
    /// recomputation (the reference `transformChanged`).
    pub fn transform_changed(&mut self, idx: usize) {
        self.nodes[idx].recalc = true;
        let children = self.nodes[idx].children.clone();
        for child in children {
            self.transform_changed(child);
        }
    }

    /// The lazily computed world transform of a node (the
    /// reference `transform()`).
    pub fn transform(&mut self, idx: usize) -> Transform {
        if self.nodes[idx].recalc {
            let mut local = self.nodes[idx].local;
            local.update();
            self.nodes[idx].local = local;
            let offset = self.nodes[idx].offset;
            let base = local.calc_offset(&offset);
            let global = if self.nodes[idx].lock_to_root {
                let root_local = self.nodes[self.root].local;
                base * root_local
            } else if let Some(parent) = self.nodes[idx].parent {
                let parent_global = self.transform(parent);
                base * parent_global
            } else {
                base
            };
            self.nodes[idx].global = global;
            self.nodes[idx].recalc = false;
        }
        self.nodes[idx].global
    }

    /// The local transform with offsets applied (the
    /// reference `transformLocal()`).
    pub fn transform_local(&self, idx: usize) -> Transform {
        self.nodes[idx].local.calc_offset(&self.nodes[idx].offset)
    }

    /// The world position of a node's origin, with Y negated
    /// (the reference walk's `worldPos`).
    pub fn world_pos(&mut self, idx: usize) -> Vec2 {
        let global = self.transform(idx);
        let p = global.trs * Vec4::new(0.0, 0.0, 0.0, 1.0);
        Vec2::new(p.x, -p.y)
    }

    /// Finds a node by uuid, pre-order, first match (the
    /// reference `Puppet.find` / `findNode` order: self,
    /// then children in order).
    pub fn find_by_uuid(&self, uuid: u32) -> Option<usize> {
        fn walk(arena: &NodeArena, idx: usize, uuid: u32) -> Option<usize> {
            if arena.nodes[idx].uuid == uuid {
                return Some(idx);
            }
            for &child in &arena.nodes[idx].children {
                if let Some(found) = walk(arena, child, uuid) {
                    return Some(found);
                }
            }
            None
        }
        walk(self, self.root, uuid)
    }

    /// Finds a node by name, pre-order, first match.
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        fn walk(arena: &NodeArena, idx: usize, name: &str) -> Option<usize> {
            if arena.nodes[idx].name == name {
                return Some(idx);
            }
            for &child in &arena.nodes[idx].children {
                if let Some(found) = walk(arena, child, name) {
                    return Some(found);
                }
            }
            None
        }
        walk(self, self.root, name)
    }

    /// The slash-joined node path (the reference
    /// `getNodePath`).
    pub fn path_of(&self, idx: usize) -> String {
        let mut names = Vec::new();
        let mut current = Some(idx);
        while let Some(i) = current {
            names.push(self.nodes[i].name.clone());
            current = self.nodes[i].parent;
        }
        names.reverse();
        format!("/{}", names.join("/"))
    }

    /// Pre-order node indices starting at `idx`.
    pub fn preorder(&self, idx: usize) -> Vec<usize> {
        let mut out = Vec::new();
        fn walk(arena: &NodeArena, idx: usize, out: &mut Vec<usize>) {
            out.push(idx);
            for &child in &arena.nodes[idx].children {
                walk(arena, child, out);
            }
        }
        walk(self, idx, &mut out);
        out
    }

    /// The reference `setValue`: accumulates a value into
    /// the node's offsets; returns whether the key applied.
    pub fn set_value(&mut self, idx: usize, key: &str, value: f32) -> bool {
        let mut transform_touched = false;
        {
            let node = &mut self.nodes[idx];
            match key {
                "zSort" => {
                    node.offset_sort += value;
                    return true;
                }
                "transform.t.x" => {
                    node.offset.translation.x += value;
                    transform_touched = true;
                }
                "transform.t.y" => {
                    node.offset.translation.y += value;
                    transform_touched = true;
                }
                "transform.t.z" => {
                    node.offset.translation.z += value;
                    transform_touched = true;
                }
                "transform.r.x" => {
                    node.offset.rotation.x += value;
                    transform_touched = true;
                }
                "transform.r.y" => {
                    node.offset.rotation.y += value;
                    transform_touched = true;
                }
                "transform.r.z" => {
                    node.offset.rotation.z += value;
                    transform_touched = true;
                }
                "transform.s.x" => {
                    node.offset.scale.x *= value;
                    transform_touched = true;
                }
                "transform.s.y" => {
                    node.offset.scale.y *= value;
                    transform_touched = true;
                }
                _ => {}
            }
            if transform_touched {
                // handled below (borrow ends)
            } else {
                match node.kind {
                    NodeKind::Part | NodeKind::AnimatedPart => match key {
                        "alphaThreshold" => {
                            node.offset_mask_threshold *= value;
                            return true;
                        }
                        "opacity" => {
                            node.offset_opacity *= value;
                            return true;
                        }
                        "tint.r" => {
                            node.offset_tint.x *= value;
                            return true;
                        }
                        "tint.g" => {
                            node.offset_tint.y *= value;
                            return true;
                        }
                        "tint.b" => {
                            node.offset_tint.z *= value;
                            return true;
                        }
                        "screenTint.r" => {
                            node.offset_screen_tint.x += value;
                            return true;
                        }
                        "screenTint.g" => {
                            node.offset_screen_tint.y += value;
                            return true;
                        }
                        "screenTint.b" => {
                            node.offset_screen_tint.z += value;
                            return true;
                        }
                        "emissionStrength" => {
                            node.offset_emission += value;
                            return true;
                        }
                        _ => return false,
                    },
                    NodeKind::Composite => match key {
                        "opacity" => {
                            node.offset_opacity *= value;
                            return true;
                        }
                        "tint.r" => {
                            node.offset_tint.x *= value;
                            return true;
                        }
                        "tint.g" => {
                            node.offset_tint.y *= value;
                            return true;
                        }
                        "tint.b" => {
                            node.offset_tint.z *= value;
                            return true;
                        }
                        "screenTint.r" => {
                            node.offset_screen_tint.x += value;
                            return true;
                        }
                        "screenTint.g" => {
                            node.offset_screen_tint.y += value;
                            return true;
                        }
                        "screenTint.b" => {
                            node.offset_screen_tint.z += value;
                            return true;
                        }
                        _ => return false,
                    },
                    NodeKind::SimplePhysics => {
                        let Some(state) = node.physics.as_mut() else {
                            return false;
                        };
                        match key {
                            "gravity" => {
                                state.offset_gravity *= value;
                                return true;
                            }
                            "length" => {
                                state.offset_length += value;
                                return true;
                            }
                            "frequency" => {
                                state.offset_frequency *= value;
                                return true;
                            }
                            "angleDamping" => {
                                state.offset_angle_damping *= value;
                                return true;
                            }
                            "lengthDamping" => {
                                state.offset_length_damping *= value;
                                return true;
                            }
                            "outputScale.x" => {
                                state.offset_output_scale.x *= value;
                                return true;
                            }
                            "outputScale.y" => {
                                state.offset_output_scale.y *= value;
                                return true;
                            }
                            _ => return false,
                        }
                    }
                    _ => return false,
                }
            }
        }
        if transform_touched {
            self.transform_changed(idx);
            return true;
        }
        false
    }

    /// The reference `beginUpdate` traversal: per-kind
    /// offset resets, deformation zeroing, then children.
    pub fn begin_update(&mut self, idx: usize) {
        {
            let node = &mut self.nodes[idx];
            match node.kind {
                NodeKind::Part | NodeKind::AnimatedPart => {
                    node.offset_mask_threshold = 0.0;
                    node.offset_opacity = 1.0;
                    node.offset_tint = Vec3::ONE;
                    node.offset_screen_tint = Vec3::ZERO;
                    node.offset_emission = 1.0;
                }
                NodeKind::Composite => {
                    node.offset_opacity = 1.0;
                    node.offset_tint = Vec3::ONE;
                    node.offset_screen_tint = Vec3::ZERO;
                }
                NodeKind::SimplePhysics => {
                    if let Some(state) = node.physics.as_mut() {
                        state.offset_gravity = 1.0;
                        state.offset_length = 0.0;
                        state.offset_frequency = 1.0;
                        state.offset_angle_damping = 1.0;
                        state.offset_length_damping = 1.0;
                        state.offset_output_scale = Vec2::ONE;
                    }
                }
                _ => {}
            }
            if node.is_drawable() {
                for slot in node.deformation.iter_mut() {
                    *slot = Vec2::ZERO;
                }
            }
            node.pre_processed = false;
            node.post_processed = false;
            node.offset_sort = 0.0;
            node.offset.clear();
        }
        let children = self.nodes[idx].children.clone();
        for child in children {
            self.begin_update(child);
        }
    }

    /// The reference `update` traversal for one node.
    pub fn update(&mut self, idx: usize) {
        if self.nodes[idx].is_drawable() {
            self.pre_process(idx);
            self.deform_update(idx);
            if self.nodes[idx].kind == NodeKind::MeshGroup {
                self.meshgroup_triangles(idx);
            }
            self.node_update_body(idx);
            self.update_deform(idx);
        } else {
            self.node_update_body(idx);
        }
    }

    fn node_update_body(&mut self, idx: usize) {
        self.pre_process(idx);
        if !self.nodes[idx].enabled {
            return;
        }
        let children = self.nodes[idx].children.clone();
        for child in children {
            self.update(child);
        }
        self.post_process(idx);
    }

    fn deform_update(&mut self, idx: usize) {
        // refreshDeform -> updateDeform: postProcess, then
        // bounds (generation is off in the CLI profile, so
        // bounds stay as authored — nothing observable).
        self.post_process(idx);
    }

    fn update_deform(&mut self, idx: usize) {
        self.post_process(idx);
    }

    /// This frame's anchor position for a physics node.
    fn physics_anchor_pos(&mut self, idx: usize) -> Vec2 {
        let local_only = self.nodes[idx]
            .physics
            .as_ref()
            .map(|s| s.local_only)
            .unwrap_or(false);
        if local_only {
            self.transform_local(idx).translation.xy()
        } else {
            let global = self.transform(idx);
            (global.trs * Vec4::new(0.0, 0.0, 0.0, 1.0)).xy()
        }
    }

    /// The reference preProcess dispatch.
    pub fn pre_process(&mut self, idx: usize) {
        match self.nodes[idx].kind {
            NodeKind::SimplePhysics => {
                let prev = self.physics_anchor_pos(idx);
                self.node_pre_process(idx);
                let after = self.physics_anchor_pos(idx);
                if after != prev {
                    let inverse = self.transform(idx).trs.inverse();
                    let node = &mut self.nodes[idx];
                    if let Some(state) = node.physics.as_mut() {
                        state.anchor = after;
                        state.prev_trans = inverse;
                        state.prev_anchor_set = true;
                    }
                }
            }
            NodeKind::Composite => {
                if !self.nodes[idx].propagate_mesh_group {
                    self.node_pre_process(idx);
                }
            }
            _ => {
                if self.nodes[idx].is_drawable() {
                    self.drawable_pre_process(idx);
                } else {
                    self.node_pre_process(idx);
                }
            }
        }
    }

    /// The reference postProcess dispatch.
    pub fn post_process(&mut self, idx: usize) {
        match self.nodes[idx].kind {
            NodeKind::SimplePhysics => {
                let prev = self.physics_anchor_pos(idx);
                self.node_post_process(idx);
                let after = self.physics_anchor_pos(idx);
                if after != prev {
                    let inverse = self.transform(idx).trs.inverse();
                    let node = &mut self.nodes[idx];
                    if let Some(state) = node.physics.as_mut() {
                        state.anchor = after;
                        state.prev_trans = inverse;
                        state.prev_anchor_set = true;
                    }
                }
            }
            NodeKind::Composite => {
                if !self.nodes[idx].propagate_mesh_group {
                    self.node_post_process(idx);
                }
            }
            NodeKind::MeshGroup => {
                // The reference `MeshGroup.postProcess`
                // calls the superclass pre-process instead:
                // a mesh group runs its pre-process filter
                // in the post phase and never runs a
                // post-process filter (so a group nested in
                // a dynamic group, whose filter is assigned
                // to the post phase, is never warped by it).
                self.drawable_pre_process(idx);
            }
            _ => {
                if self.nodes[idx].is_drawable() {
                    self.drawable_post_process(idx);
                } else {
                    self.node_post_process(idx);
                }
            }
        }
    }

    fn node_pre_process(&mut self, idx: usize) {
        if self.nodes[idx].pre_processed {
            return;
        }
        self.nodes[idx].pre_processed = true;
        let Some(group) = self.nodes[idx].pre_warp else {
            return;
        };
        let local_xy = self.nodes[idx].local.translation.xy();
        let offset_xy = self.nodes[idx].offset.translation.xy();
        let parent_matrix = match self.nodes[idx].parent {
            Some(parent) => self.transform(parent).trs,
            None => Mat4::identity(),
        };
        let (out, _) = self.filter_children(group, &[local_xy], &[offset_xy], &parent_matrix);
        if let Some(first) = out.first() {
            let keep_z = self.nodes[idx].offset.translation.z;
            self.nodes[idx].offset.translation = Vec3::new(first.x, first.y, keep_z);
            self.transform_changed(idx);
        }
    }

    fn node_post_process(&mut self, idx: usize) {
        if self.nodes[idx].post_processed {
            return;
        }
        self.nodes[idx].post_processed = true;
        let Some(group) = self.nodes[idx].post_warp else {
            return;
        };
        let local_xy = self.nodes[idx].local.translation.xy();
        let offset_xy = self.nodes[idx].offset.translation.xy();
        let parent_matrix = match self.nodes[idx].parent {
            Some(parent) => self.transform(parent).trs,
            None => Mat4::identity(),
        };
        let (out, _) = self.filter_children(group, &[local_xy], &[offset_xy], &parent_matrix);
        if let Some(first) = out.first() {
            let keep_z = self.nodes[idx].offset.translation.z;
            self.nodes[idx].offset.translation = Vec3::new(first.x, first.y, keep_z);
            self.transform_changed(idx);
        }
    }

    fn drawable_pre_process(&mut self, idx: usize) {
        if self.nodes[idx].pre_processed {
            return;
        }
        self.nodes[idx].pre_processed = true;
        let Some(group) = self.nodes[idx].pre_warp else {
            return;
        };
        let vertices = self.nodes[idx]
            .mesh
            .as_ref()
            .map(|m| m.vertices.clone())
            .unwrap_or_default();
        let deformation = self.nodes[idx].deformation.clone();
        let matrix = self.transform(idx).trs;
        let (out, override_matrix) = self.filter_children(group, &vertices, &deformation, &matrix);
        if !out.is_empty() {
            self.nodes[idx].deformation = out;
        }
        if override_matrix.is_some() {
            self.nodes[idx].override_matrix = override_matrix;
        }
    }

    fn drawable_post_process(&mut self, idx: usize) {
        if self.nodes[idx].post_processed {
            return;
        }
        self.nodes[idx].post_processed = true;
        let Some(group) = self.nodes[idx].post_warp else {
            return;
        };
        let vertices = self.nodes[idx]
            .mesh
            .as_ref()
            .map(|m| m.vertices.clone())
            .unwrap_or_default();
        let deformation = self.nodes[idx].deformation.clone();
        let matrix = self.transform(idx).trs;
        let (out, override_matrix) = self.filter_children(group, &vertices, &deformation, &matrix);
        if !out.is_empty() {
            self.nodes[idx].deformation = out;
        }
        if override_matrix.is_some() {
            self.nodes[idx].override_matrix = override_matrix;
        }
    }

    /// The mesh-group warp filter (the reference
    /// `MeshGroup.filterChildren`): warps deformation
    /// through the group's triangle lattice. Returns the new
    /// deformation and an optional matrix override (dynamic
    /// groups only).
    pub fn filter_children(
        &mut self,
        group: usize,
        orig_vertices: &[Vec2],
        orig_deformation: &[Vec2],
        orig_transform: &Mat4,
    ) -> (Vec<Vec2>, Option<Mat4>) {
        let (precalculated, dynamic, warp) = {
            let node = &self.nodes[group];
            (node.precalculated, node.dynamic, node.warp.clone())
        };
        let Some(warp) = warp else {
            return (orig_deformation.to_vec(), None);
        };
        if !precalculated {
            return (orig_deformation.to_vec(), None);
        }
        let center = warp.inverse * *orig_transform;
        let mut out: Vec<Vec2> = orig_deformation.to_vec();
        if out.len() < orig_vertices.len() {
            out.resize(orig_vertices.len(), Vec2::ZERO);
        }
        for i in 0..orig_vertices.len() {
            let vertex = orig_vertices[i];
            let deform_i = orig_deformation.get(i).copied().unwrap_or(Vec2::ZERO);
            let base = if dynamic { vertex + deform_i } else { vertex };
            let c = center * Vec4::new(base.x, base.y, 0.0, 1.0);
            let c_vertex = Vec3::new(c.x, c.y, c.z);
            let mut triangle_index: i64 = -1;
            if c_vertex.x >= warp.bounds[0]
                && c_vertex.x < warp.bounds[2]
                && c_vertex.y >= warp.bounds[1]
                && c_vertex.y < warp.bounds[3]
            {
                let cell_x = (c_vertex.x - warp.bounds[0]) as i64;
                let cell_y = (c_vertex.y - warp.bounds[1]) as i64;
                let cell = (cell_y * warp.width as i64 + cell_x) as usize;
                if let Some(&bit) = warp.bitmask.get(cell) {
                    triangle_index = bit as i64 - 1;
                }
            }
            let new_pos = if triangle_index < 0 {
                Vec2::new(c_vertex.x, c_vertex.y)
            } else {
                let tri = &warp.triangles[triangle_index as usize];
                (tri.transform * Vec3::new(c_vertex.x, c_vertex.y, 1.0)).xy()
            };
            if dynamic {
                out[i] = new_pos - vertex;
            } else {
                // The reference applies only the linear part
                // of the inverse to the delta: the translation
                // column is zeroed before the multiply.
                let mut inv = center.inverse();
                inv.m[12] = 0.0;
                inv.m[13] = 0.0;
                inv.m[14] = 0.0;
                let diff =
                    inv * Vec4::new(new_pos.x - c_vertex.x, new_pos.y - c_vertex.y, 0.0, 1.0);
                out[i] += Vec2::new(diff.x, diff.y);
            }
        }
        let override_matrix = if dynamic { Some(warp.forward) } else { None };
        (out, override_matrix)
    }

    /// The mesh-group triangle pass: (re)precalculates the
    /// warp lattice and refreshes per-triangle transforms
    /// from the deformed vertices (the reference
    /// `MeshGroup.update` middle block).
    pub fn meshgroup_triangles(&mut self, idx: usize) {
        let has_indices = self.nodes[idx]
            .mesh
            .as_ref()
            .map(|m| !m.indices.is_empty())
            .unwrap_or(false);
        if !has_indices {
            return;
        }
        if !self.nodes[idx].precalculated {
            self.meshgroup_precalculate(idx);
            self.setup_children(idx);
        }
        let (transformed, indices) = {
            let node = &self.nodes[idx];
            let mesh = node.mesh.as_ref().expect("mesh present");
            let transformed: Vec<Vec2> = mesh
                .vertices
                .iter()
                .enumerate()
                .map(|(i, v)| *v + node.deformation.get(i).copied().unwrap_or(Vec2::ZERO))
                .collect();
            (transformed, mesh.indices.clone())
        };
        {
            let node = &mut self.nodes[idx];
            let warp = node.warp.as_mut().expect("warp precalculated");
            for (tri_idx, tri) in warp.triangles.iter_mut().enumerate() {
                let base = tri_idx * 3;
                if base + 2 >= indices.len() {
                    break;
                }
                let p1 = transformed[indices[base] as usize];
                let p2 = transformed[indices[base + 1] as usize];
                let p3 = transformed[indices[base + 2] as usize];
                let tri_matrix = Mat3::from_array([
                    p2.x - p1.x,
                    p3.x - p1.x,
                    p1.x,
                    p2.y - p1.y,
                    p3.y - p1.y,
                    p1.y,
                    0.0,
                    0.0,
                    1.0,
                ]);
                tri.transform = tri_matrix * tri.offset;
            }
        }
        let forward = self.transform(idx).trs;
        let inverse = forward.inverse();
        let node = &mut self.nodes[idx];
        if let Some(warp) = node.warp.as_mut() {
            warp.forward = forward;
            warp.inverse = inverse;
        }
    }

    fn meshgroup_precalculate(&mut self, idx: usize) {
        let mesh = self.nodes[idx].mesh.clone().expect("mesh present");
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for v in &mesh.vertices {
            min_x = min_x.min(v.x);
            min_y = min_y.min(v.y);
            max_x = max_x.max(v.x);
            max_y = max_y.max(v.y);
        }
        let bounds = [min_x.floor(), min_y.floor(), max_x.ceil(), max_y.ceil()];
        let width = (bounds[2] - bounds[0]) as usize + 1;
        let height = (bounds[3] - bounds[1]) as usize + 1;
        let mut bitmask = vec![0i32; width * height];
        let mut triangles = Vec::new();
        for tri_idx in 0..mesh.indices.len() / 3 {
            let base = tri_idx * 3;
            let p1 = mesh.vertices[mesh.indices[base] as usize];
            let p2 = mesh.vertices[mesh.indices[base + 1] as usize];
            let p3 = mesh.vertices[mesh.indices[base + 2] as usize];
            let axis0 = p2 - p1;
            let axis1 = p3 - p1;
            let axis0_len = axis0.length();
            let axis1_len = axis1.length();
            let axis0n = if axis0_len > 0.0 {
                axis0 / axis0_len
            } else {
                Vec2::ZERO
            };
            let axis1n = if axis1_len > 0.0 {
                axis1 / axis1_len
            } else {
                Vec2::ZERO
            };
            // Rotate axis1 by -axis0's angle: cosA = dot,
            // sinA = cross (z of axis0n x axis1n).
            let cos_a = axis0n.x * axis1n.x + axis0n.y * axis1n.y;
            let sin_a = axis0n.x * axis1n.y - axis0n.y * axis1n.x;
            let scale = Mat3::from_array([
                if axis0_len > 0.0 {
                    1.0 / axis0_len
                } else {
                    0.0
                },
                0.0,
                0.0,
                0.0,
                if axis1_len > 0.0 {
                    1.0 / axis1_len
                } else {
                    0.0
                },
                0.0,
                0.0,
                0.0,
                1.0,
            ]);
            let shear = Mat3::from_array([
                1.0,
                -cos_a / sin_a,
                0.0,
                0.0,
                1.0 / sin_a,
                0.0,
                0.0,
                0.0,
                1.0,
            ]);
            let rotate = Mat3::from_array([
                axis0n.x, axis0n.y, 0.0, -axis0n.y, axis0n.x, 0.0, 0.0, 0.0, 1.0,
            ]);
            let translate = Mat3::from_array([1.0, 0.0, -p1.x, 0.0, 1.0, -p1.y, 0.0, 0.0, 1.0]);
            let offset = scale * shear * rotate * translate;
            triangles.push(WarpTriangle {
                transform: Mat3::default(),
                offset,
            });
            let tri_min_x = p1.x.min(p2.x).min(p3.x).floor() as i64;
            let tri_max_x = p1.x.max(p2.x).max(p3.x).ceil() as i64;
            let tri_min_y = p1.y.min(p2.y).min(p3.y).floor() as i64;
            let tri_max_y = p1.y.max(p2.y).max(p3.y).ceil() as i64;
            for y in tri_min_y..=tri_max_y {
                for x in tri_min_x..=tri_max_x {
                    if point_in_triangle(Vec2::new(x as f32, y as f32), p1, p2, p3) {
                        let cell_x = x - bounds[0] as i64;
                        let cell_y = y - bounds[1] as i64;
                        if cell_x >= 0 && cell_y >= 0 {
                            let cell = (cell_y * width as i64 + cell_x) as usize;
                            if cell < bitmask.len() {
                                bitmask[cell] = tri_idx as i32 + 1;
                            }
                        }
                    }
                }
            }
        }
        let node = &mut self.nodes[idx];
        node.warp = Some(WarpData {
            bounds,
            width,
            height,
            bitmask,
            triangles,
            forward: Mat4::identity(),
            inverse: Mat4::identity(),
        });
        node.precalculated = true;
    }

    fn setup_children(&mut self, group: usize) {
        let has_indices = self.nodes[group]
            .mesh
            .as_ref()
            .map(|m| !m.indices.is_empty())
            .unwrap_or(false);
        if !has_indices {
            return;
        }
        let children = self.nodes[group].children.clone();
        for child in children {
            self.setup_child(group, child);
        }
    }

    fn setup_child(&mut self, group: usize, target: usize) {
        let translate_children = self.nodes[group].translate_children;
        let dynamic = self.nodes[group].dynamic;
        let is_drawable = self.nodes[target].is_drawable();
        let is_meshgroup = self.nodes[target].kind == NodeKind::MeshGroup;
        let is_composite = self.nodes[target].kind == NodeKind::Composite;
        let propagate = self.nodes[target].propagate_mesh_group;
        if translate_children || is_drawable {
            if dynamic && is_drawable {
                self.nodes[target].pre_warp = None;
                self.nodes[target].post_warp = Some(group);
            } else {
                self.nodes[target].pre_warp = Some(group);
                self.nodes[target].post_warp = None;
            }
        } else {
            self.nodes[target].pre_warp = None;
            self.nodes[target].post_warp = None;
        }
        // Propagation continues only into drawable non-group
        // nodes and propagating composites.
        if (is_drawable && !is_meshgroup) || (is_composite && propagate) {
            let children = self.nodes[target].children.clone();
            for child in children {
                self.setup_child(group, child);
            }
        }
    }
}

/// Whether a point lies inside a triangle (the reference
/// `isPointInTriangle`: barycentric sign test).
pub fn point_in_triangle(point: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> bool {
    fn sign(a: Vec2, b: Vec2, c: Vec2) -> f32 {
        (a.x - c.x) * (b.y - c.y) - (b.x - c.x) * (a.y - c.y)
    }
    let d1 = sign(point, p1, p2);
    let d2 = sign(point, p2, p3);
    let d3 = sign(point, p3, p1);
    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    !(has_neg && has_pos)
}

#[cfg(test)]
mod tests {
    use super::{Node, NodeArena};
    use crate::math::Vec2;
    use torogao_format::NodeKind;

    #[test]
    fn set_value_accumulates_transform_offsets() {
        let mut arena = NodeArena::default();
        arena
            .nodes
            .push(Node::new(1, "Root".to_string(), NodeKind::Node));
        arena.root = 0;
        assert!(arena.set_value(0, "transform.t.x", 5.0));
        assert!(arena.set_value(0, "transform.t.x", 2.0));
        assert_eq!(arena.nodes[0].offset.translation.x, 7.0);
        assert!(!arena.set_value(0, "nope", 1.0));
    }

    #[test]
    fn world_pos_negates_y() {
        let mut arena = NodeArena::default();
        let mut root = Node::new(1, "Root".to_string(), NodeKind::Node);
        root.local.translation = crate::math::Vec3::new(10.0, 20.0, 0.0);
        root.local.update();
        arena.nodes.push(root);
        arena.root = 0;
        assert_eq!(arena.world_pos(0), Vec2::new(10.0, -20.0));
    }
}
