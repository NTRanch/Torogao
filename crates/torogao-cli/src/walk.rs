//! Puppet traversal and id assignment (puppetwalk.d):
//! one walk builds the lumen-style ids and rest world
//! matrices shared by inspect, validate, and export-rig.

use std::collections::HashMap;
use torogao_engine::{Puppet, Vec2, Vec4};
use torogao_format::NodeKind;

/// One walked node.
#[derive(Debug, Clone)]
pub struct WNode {
    /// Arena index of the engine node.
    pub node: usize,
    /// Walk index of the parent, if any.
    pub parent: Option<usize>,
    /// Walk indices of the children, in tree order.
    pub children: Vec<usize>,
    /// Lumen part id ("root" for the root node).
    pub part_id: String,
    /// Lumen mesh id, set for Parts.
    pub mesh_id: Option<String>,
    /// Lumen warp deformer id, set for grid MeshGroups.
    pub warp_id: Option<String>,
    /// Lumen rotation deformer id (assigned by the exporter).
    pub rot_id: Option<String>,
    /// Rest-state world matrix.
    pub world: torogao_engine::Mat4,
}

/// The type identifier of an engine node kind, matching
/// the reference class names.
pub fn kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Node => "Node",
        NodeKind::Tmp => "Tmp",
        NodeKind::Part => "Part",
        NodeKind::AnimatedPart => "AnimatedPart",
        NodeKind::Composite => "Composite",
        NodeKind::MeshGroup => "MeshGroup",
        NodeKind::SimplePhysics => "SimplePhysics",
        NodeKind::DynamicComposite => "DynamicComposite",
        NodeKind::Projectable => "Projectable",
    }
}

/// Whether the kind is a drawable (Part family or MeshGroup).
pub fn is_drawable(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Part | NodeKind::AnimatedPart | NodeKind::MeshGroup
    )
}

/// Whether the kind is a Part (animated parts included).
pub fn is_part(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Part | NodeKind::AnimatedPart)
}

/// Reduces a display name to the lumen id alphabet
/// ([A-Za-z0-9_-]); every other byte becomes '_'.
/// Byte-wise, matching the reference implementation.
pub fn slugify(name: &str, max_len: usize) -> String {
    let mut mapped: Vec<u8> = Vec::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' {
            mapped.push(byte);
        } else {
            mapped.push(b'_');
        }
    }
    // Collapse runs of '_' (also trims a leading run).
    let mut collapsed: Vec<u8> = Vec::with_capacity(mapped.len());
    let mut last_underscore = true;
    for byte in mapped {
        if byte == b'_' {
            if !last_underscore {
                collapsed.push(byte);
            }
            last_underscore = true;
        } else {
            collapsed.push(byte);
            last_underscore = false;
        }
    }
    while collapsed.last() == Some(&b'_') {
        collapsed.pop();
    }
    if collapsed.is_empty() {
        collapsed = b"node".to_vec();
    }
    collapsed.truncate(max_len);
    String::from_utf8(collapsed).expect("slug alphabet is ASCII")
}

fn suffixed(prefix: &str, name: &str, uuid: u32, slug_len: usize) -> String {
    format!("{}{}_{}", prefix, slugify(name, slug_len), uuid)
}

/// The suffixed id form (`<prefix><slug>_<uuid>`), shared
/// with the exporter.
pub fn suffixed_name(prefix: &str, name: &str, uuid: u32, slug_len: usize) -> String {
    suffixed(prefix, name, uuid, slug_len)
}

/// Determines whether a mesh is a regular grid, and its
/// cell dimensions, from the vertex lattice itself
/// (row-major, x fastest).
pub fn grid_dims(vertices: &[Vec2]) -> Option<(usize, usize)> {
    if vertices.len() < 4 {
        return None;
    }
    let mut xs: Vec<f32> = vertices.iter().map(|v| v.x).collect();
    let mut ys: Vec<f32> = vertices.iter().map(|v| v.y).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs.dedup();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.dedup();
    if xs.len() < 2 || ys.len() < 2 {
        return None;
    }
    if vertices.len() != xs.len() * ys.len() {
        return None;
    }
    for (k, v) in vertices.iter().enumerate() {
        if v.x != xs[k % xs.len()] || v.y != ys[k / xs.len()] {
            return None;
        }
    }
    Some((xs.len() - 1, ys.len() - 1))
}

/// The built walk: nodes in pre-order, root first.
#[derive(Debug, Clone)]
pub struct Walk {
    /// Walk nodes in pre-order.
    pub nodes: Vec<WNode>,
    /// Walk index of the root.
    pub root: usize,
    /// Engine uuid -> walk index.
    pub by_uuid: HashMap<u32, usize>,
}

impl Walk {
    /// Builds the walk over a loaded puppet, capturing
    /// rest world matrices.
    pub fn build(puppet: &mut Puppet) -> Walk {
        let mut walk = Walk {
            nodes: Vec::new(),
            root: 0,
            by_uuid: HashMap::new(),
        };
        let root = puppet.nodes.root;
        walk.build_node(puppet, root, None, true);
        walk
    }

    fn build_node(
        &mut self,
        puppet: &mut Puppet,
        node_idx: usize,
        parent: Option<usize>,
        is_root: bool,
    ) -> usize {
        let node = &puppet.nodes.nodes[node_idx];
        let uuid = node.uuid;
        let name = node.name.clone();
        let kind = node.kind;
        let part_id = if is_root {
            "root".to_string()
        } else {
            suffixed("", &name, uuid, 48)
        };
        let mesh_id = if is_part(kind) {
            Some(suffixed("mesh_", &name, uuid, 44))
        } else {
            None
        };
        let warp_id = if kind == NodeKind::MeshGroup {
            node.mesh
                .as_ref()
                .and_then(|mesh| grid_dims(&mesh.vertices))
                .map(|_| suffixed("warp_", &name, uuid, 44))
        } else {
            None
        };
        let world = puppet.nodes.transform(node_idx).trs;
        let walk_idx = self.nodes.len();
        self.nodes.push(WNode {
            node: node_idx,
            parent,
            children: Vec::new(),
            part_id,
            mesh_id,
            warp_id,
            rot_id: None,
            world,
        });
        self.by_uuid.insert(uuid, walk_idx);
        let children: Vec<usize> = puppet.nodes.nodes[node_idx].children.clone();
        for child in children {
            let child_walk = self.build_node(puppet, child, Some(walk_idx), false);
            self.nodes[walk_idx].children.push(child_walk);
        }
        walk_idx
    }

    /// Walk index for an engine node index.
    pub fn index_of_node(&self, puppet: &Puppet, node_idx: usize) -> Option<usize> {
        let uuid = puppet.nodes.nodes[node_idx].uuid;
        self.by_uuid.get(&uuid).copied()
    }

    /// Slash-joined node path ("/Root/Head/Face").
    pub fn path_of(&self, puppet: &Puppet, walk_idx: usize) -> String {
        let mut path = String::new();
        let mut cursor = Some(walk_idx);
        while let Some(idx) = cursor {
            let w = &self.nodes[idx];
            path = format!("/{}{}", puppet.nodes.nodes[w.node].name, path);
            cursor = w.parent;
        }
        path
    }

    /// World-space position of a node's origin, lumen
    /// orientation (y flipped).
    pub fn world_pos(&self, walk_idx: usize) -> [f32; 2] {
        let p = self.nodes[walk_idx].world * Vec4::new(0.0, 0.0, 0.0, 1.0);
        [p.x, -p.y]
    }

    /// A rest-state local point carried to lumen world
    /// space (mesh-origin relative).
    pub fn to_world(&self, walk_idx: usize, local: Vec2, origin: Vec2) -> [f32; 2] {
        let p = self.nodes[walk_idx].world
            * Vec4::new(local.x - origin.x, local.y - origin.y, 0.0, 1.0);
        [p.x, -p.y]
    }

    /// A local-space delta carried to lumen world space
    /// (linear part only, y flipped).
    pub fn delta_to_world(&self, walk_idx: usize, delta: Vec2) -> [f32; 2] {
        let p = self.nodes[walk_idx].world * Vec4::new(delta.x, delta.y, 0.0, 0.0);
        [p.x, -p.y]
    }

    /// Nearest strict ancestor carrying a lumen deformer
    /// (warp or rotation).
    pub fn nearest_deformer_ancestor(&self, walk_idx: usize) -> Option<usize> {
        let mut cursor = self.nodes[walk_idx].parent;
        while let Some(idx) = cursor {
            let w = &self.nodes[idx];
            if w.warp_id.is_some() || w.rot_id.is_some() {
                return Some(idx);
            }
            cursor = w.parent;
        }
        None
    }

    /// The lumen deformer id of a walk node, if it has one.
    pub fn deformer_id_of(&self, walk_idx: usize) -> Option<&str> {
        let w = &self.nodes[walk_idx];
        w.warp_id.as_deref().or(w.rot_id.as_deref())
    }
}
