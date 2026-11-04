//! INP1 container and payload import.
//!
//! Container layout: magic `TRNSRTS\0`, a big-endian u32 JSON
//! length and the JSON payload, then `TEX_SECT` with a big-endian
//! u32 texture count and per-texture (length, encoding byte, bytes),
//! then an optional `EXT_SECT` of vendor entries.
//!
//! Import is validating and loud: structural violations are typed
//! errors. The reference payload parser's *drop* behaviors are kept
//! as counted drops, never silent ones: bindings whose interpolation
//! mode is neither Linear nor Nearest are dropped (the corpus's
//! cubic-interpolated bindings take this path), bindings with an
//! unknown `param_name` or target node are skipped, and non-albedo
//! texture slots carrying the null marker are counted as dropped.

use serde_json::{Map, Value};

use crate::error::RigError;
use crate::math::{Vec2, Vec3};
use crate::model::{
    Binding, BindingKind, BindingValues, Composite, Drawable, InterpolateMode, Mask, MaskMode,
    Mesh, Node, NodeKind, Param, Part, PhysicsMapMode, PhysicsNode, Puppet, TransformOffset,
};
use crate::texture::{DecodeStats, decode_texture};

/// Whole-input cap (512 MiB): records are puppets, not archives.
const INPUT_BYTES_MAX: usize = 512 * 1024 * 1024;
/// Texture count cap.
const TEXTURES_MAX: usize = 4096;
/// Single-texture payload cap (64 MiB).
const TEXTURE_BYTES_MAX: usize = 64 * 1024 * 1024;
/// Node count cap.
const NODES_MAX: usize = 65_536;
/// Node tree depth cap (recursion guard).
const NODE_DEPTH_MAX: usize = 128;
/// Parameter count cap.
const PARAMS_MAX: usize = 4096;
/// Vertices per mesh cap.
const VERTS_MAX: usize = 1 << 20;

/// Load a puppet from `.inx` bytes.
pub fn load_inp(bytes: &[u8]) -> Result<Puppet, RigError> {
    if bytes.len() > INPUT_BYTES_MAX {
        return Err(RigError::new("TRG-INP-002", "input exceeds the size cap"));
    }
    if bytes.len() < 12 || &bytes[0..8] != b"TRNSRTS\0" {
        return Err(RigError::new(
            "TRG-INP-001",
            "missing TRNSRTS\\0 container magic",
        ));
    }
    let json_len = be_u32(bytes, 8)? as usize;
    let json_end = 12usize
        .checked_add(json_len)
        .filter(|&end| end <= bytes.len())
        .ok_or_else(|| RigError::new("TRG-INP-002", "JSON payload length overruns input"))?;
    let payload: Value = serde_json::from_slice(&bytes[12..json_end])
        .map_err(|e| RigError::new("TRG-INP-003", format!("payload JSON invalid: {e}")))?;

    let mut cursor = json_end;
    let (textures, stats) = read_textures(bytes, &mut cursor)?;
    let extensions = read_extensions(bytes, &mut cursor)?;

    let root = payload
        .as_object()
        .ok_or_else(|| RigError::new("TRG-INP-006", "payload root is not an object"))?;
    let mut builder = Builder {
        textures_len: textures.len(),
        ..Builder::default()
    };
    let mut nodes = builder.parse_nodes(root)?;
    let params = builder.parse_params(root, &nodes)?;
    builder.apply_pending(&mut nodes, &params)?;

    let (name, version) = parse_meta(root);
    let (pixels_per_meter, gravity) = parse_physics_globals(root);
    Ok(Puppet {
        name,
        version,
        pixels_per_meter,
        gravity,
        nodes,
        params,
        textures,
        extensions,
        dropped_texture_slots: builder.dropped_texture_slots,
        dropped_unsupported_bindings: builder.dropped_unsupported_bindings,
        skipped_bindings: builder.skipped_bindings,
        tga_rle_line_crossings: stats.rle_line_crossings,
    })
}

/// Read a big-endian u32 at `at`.
fn be_u32(bytes: &[u8], at: usize) -> Result<u32, RigError> {
    let slice = bytes
        .get(at..at + 4)
        .ok_or_else(|| RigError::new("TRG-INP-002", "input truncated at a length field"))?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Read the TEX_SECT section starting at `cursor`.
fn read_textures(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<(Vec<crate::texture::Texture>, DecodeStats), RigError> {
    if bytes.get(*cursor..*cursor + 8) != Some(b"TEX_SECT") {
        return Err(RigError::new("TRG-INP-002", "TEX_SECT magic missing"));
    }
    *cursor += 8;
    let count = be_u32(bytes, *cursor)? as usize;
    *cursor += 4;
    if count > TEXTURES_MAX {
        return Err(RigError::new("TRG-INP-002", "texture count exceeds cap"));
    }
    let mut stats = DecodeStats::default();
    let mut textures = Vec::with_capacity(count);
    for _ in 0..count {
        let len = be_u32(bytes, *cursor)? as usize;
        *cursor += 4;
        if len > TEXTURE_BYTES_MAX {
            return Err(RigError::new("TRG-INP-002", "texture payload exceeds cap"));
        }
        let encoding = *bytes
            .get(*cursor)
            .ok_or_else(|| RigError::new("TRG-INP-002", "texture encoding byte missing"))?;
        *cursor += 1;
        let end = cursor
            .checked_add(len)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| RigError::new("TRG-INP-002", "texture payload overruns input"))?;
        let texture = decode_texture(&bytes[*cursor..end], encoding, &mut stats)?;
        textures.push(texture);
        *cursor = end;
    }
    Ok((textures, stats))
}

/// Read the optional EXT_SECT section, returning vendor names.
fn read_extensions(bytes: &[u8], cursor: &mut usize) -> Result<Vec<String>, RigError> {
    if bytes.get(*cursor..*cursor + 8) != Some(b"EXT_SECT") {
        return Ok(Vec::new());
    }
    *cursor += 8;
    let count = be_u32(bytes, *cursor)? as usize;
    *cursor += 4;
    let mut names = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let name_len = be_u32(bytes, *cursor)? as usize;
        *cursor += 4;
        let name_end = cursor
            .checked_add(name_len)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| RigError::new("TRG-INP-002", "extension name overruns input"))?;
        names.push(String::from_utf8_lossy(&bytes[*cursor..name_end]).into_owned());
        *cursor = name_end;
        let payload_len = be_u32(bytes, *cursor)? as usize;
        *cursor += 4;
        *cursor = cursor
            .checked_add(payload_len)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| RigError::new("TRG-INP-002", "extension payload overruns input"))?;
    }
    Ok(names)
}

/// Meta fields: (name, version), both tolerant of absence.
fn parse_meta(root: &Map<String, Value>) -> (Option<String>, String) {
    let Some(meta) = root.get("meta").and_then(Value::as_object) else {
        return (None, String::new());
    };
    let name = meta.get("name").and_then(Value::as_str).map(str::to_owned);
    let version = match meta.get("version") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    (name, version)
}

/// Physics globals with the format's usual defaults.
fn parse_physics_globals(root: &Map<String, Value>) -> (f32, f32) {
    let Some(physics) = root.get("physics").and_then(Value::as_object) else {
        return (100.0, 1.0);
    };
    (
        get_f32(physics, "pixelsPerMeter").unwrap_or(100.0),
        get_f32(physics, "gravity").unwrap_or(1.0),
    )
}

/// Import scratch state: counters plus fix-ups that need the whole
/// record (mask sources, physics parameter targets).
#[derive(Default)]
struct Builder {
    dropped_texture_slots: usize,
    dropped_unsupported_bindings: usize,
    skipped_bindings: usize,
    textures_len: usize,
    pending_masks: Vec<(usize, u32, MaskMode)>,
    pending_physics: Vec<(usize, u32)>,
}

impl Builder {
    fn parse_nodes(&mut self, root: &Map<String, Value>) -> Result<Vec<Node>, RigError> {
        let root_value = root
            .get("nodes")
            .ok_or_else(|| RigError::new("TRG-INP-006", "payload has no nodes tree"))?;
        let mut nodes: Vec<Node> = Vec::new();
        let mut uuid_seen: Vec<u32> = Vec::new();
        self.parse_node(root_value, None, 0, &mut nodes, &mut uuid_seen)?;
        Ok(nodes)
    }

    fn parse_node(
        &mut self,
        value: &Value,
        parent: Option<usize>,
        depth: usize,
        nodes: &mut Vec<Node>,
        uuid_seen: &mut Vec<u32>,
    ) -> Result<usize, RigError> {
        if depth > NODE_DEPTH_MAX {
            return Err(RigError::new("TRG-INP-006", "node tree exceeds depth cap"));
        }
        if nodes.len() >= NODES_MAX {
            return Err(RigError::new("TRG-INP-006", "node count exceeds cap"));
        }
        let obj = value
            .as_object()
            .ok_or_else(|| RigError::new("TRG-INP-006", "node is not an object"))?;
        let uuid = get_u32(obj, "uuid")
            .ok_or_else(|| RigError::new("TRG-INP-006", "node without a uuid"))?;
        if uuid_seen.contains(&uuid) {
            return Err(RigError::new(
                "TRG-INP-007",
                format!("duplicate node uuid {uuid}"),
            ));
        }
        uuid_seen.push(uuid);
        let index = nodes.len();
        let kind = self.parse_kind(obj, index)?;
        nodes.push(Node {
            uuid,
            name: get_str(obj, "name").unwrap_or_default().to_owned(),
            enabled: get_bool(obj, "enabled").unwrap_or(true),
            zsort: get_f32(obj, "zsort").unwrap_or(0.0),
            offset: parse_transform(obj),
            lock_to_root: get_bool(obj, "lockToRoot").unwrap_or(false),
            parent,
            children: Vec::new(),
            kind,
        });
        if let Some(children) = obj.get("children").and_then(Value::as_array) {
            for child in children {
                let child_index =
                    self.parse_node(child, Some(index), depth + 1, nodes, uuid_seen)?;
                nodes[index].children.push(child_index);
            }
        }
        Ok(index)
    }

    fn parse_kind(&mut self, obj: &Map<String, Value>, index: usize) -> Result<NodeKind, RigError> {
        match get_str(obj, "type") {
            Some("Part") => Ok(NodeKind::Part(self.parse_part(obj, index)?)),
            Some("Composite") => Ok(NodeKind::Composite(Composite {
                drawable: self.parse_drawable(obj, index),
            })),
            Some("SimplePhysics") => {
                let param_uuid = get_u32(obj, "param").ok_or_else(|| {
                    RigError::new("TRG-INP-006", "SimplePhysics node without a param uuid")
                })?;
                self.pending_physics.push((index, param_uuid));
                Ok(NodeKind::Physics(PhysicsNode {
                    param: usize::MAX, // patched in finish_physics
                    spring: get_str(obj, "model_type") == Some("SpringPendulum"),
                    map_mode: match get_str(obj, "map_mode") {
                        Some("LengthAngle") => PhysicsMapMode::LengthAngle,
                        Some("XY") => PhysicsMapMode::Xy,
                        Some("YX") => PhysicsMapMode::Yx,
                        _ => PhysicsMapMode::AngleLength,
                    },
                    gravity: get_f32(obj, "gravity").unwrap_or(1.0),
                    length: get_f32(obj, "length").unwrap_or(0.0),
                    frequency: get_f32(obj, "frequency").unwrap_or(1.0),
                    angle_damping: get_f32(obj, "angle_damping").unwrap_or(0.0),
                    length_damping: get_f32(obj, "length_damping").unwrap_or(0.0),
                    output_scale: get_vec2(obj, "output_scale").unwrap_or(Vec2::new(1.0, 1.0)),
                    local_only: get_bool(obj, "local_only").unwrap_or(false),
                }))
            }
            Some("MeshGroup") => Ok(NodeKind::MeshGroup),
            _ => Ok(NodeKind::Group),
        }
    }

    fn parse_drawable(&mut self, obj: &Map<String, Value>, index: usize) -> Drawable {
        let drawable = Drawable {
            blend_mode: get_str(obj, "blend_mode")
                .and_then(crate::model::BlendMode::from_name)
                .unwrap_or(crate::model::BlendMode::Normal),
            tint: get_vec3(obj, "tint")
                .map(|v| [v.x, v.y, v.z])
                .unwrap_or([1.0, 1.0, 1.0]),
            screen_tint: get_vec3(obj, "screenTint")
                .map(|v| [v.x, v.y, v.z])
                .unwrap_or([0.0, 0.0, 0.0]),
            opacity: get_f32(obj, "opacity").unwrap_or(1.0),
            mask_threshold: get_f32(obj, "mask_threshold").unwrap_or(0.5),
            // Mask sources resolve in apply_pending, once every node
            // uuid is known.
            masks: Vec::new(),
        };
        if let Some(masks) = obj.get("masks").and_then(Value::as_array) {
            for mask in masks {
                let Some(mask_obj) = mask.as_object() else {
                    continue;
                };
                let Some(source) = get_u32(mask_obj, "source") else {
                    continue;
                };
                let mode = if get_str(mask_obj, "mode") == Some("DodgeMask") {
                    MaskMode::DodgeMask
                } else {
                    MaskMode::Mask
                };
                self.pending_masks.push((index, source, mode));
            }
        }
        drawable
    }

    fn parse_part(&mut self, obj: &Map<String, Value>, index: usize) -> Result<Part, RigError> {
        let drawable = self.parse_drawable(obj, index);
        let mut texture_slots = [None, None, None];
        if let Some(textures) = obj.get("textures").and_then(Value::as_array) {
            for (slot, value) in textures.iter().take(3).enumerate() {
                match value.as_u64() {
                    Some(raw) if raw == u64::from(u32::MAX) => {
                        if slot > 0 {
                            self.dropped_texture_slots += 1;
                        }
                    }
                    Some(raw) => {
                        let tex_index = usize::try_from(raw).map_err(|_| {
                            RigError::new("TRG-INP-006", "texture index out of range")
                        })?;
                        if tex_index >= self.textures_len {
                            return Err(RigError::new(
                                "TRG-INP-006",
                                "texture slot references a missing texture",
                            ));
                        }
                        texture_slots[slot] = Some(tex_index);
                    }
                    None => {}
                }
            }
        }
        let mesh = parse_mesh(obj)?;
        Ok(Part {
            mesh,
            drawable,
            texture_slots,
        })
    }

    fn parse_params(
        &mut self,
        root: &Map<String, Value>,
        nodes: &[Node],
    ) -> Result<Vec<Param>, RigError> {
        let Some(param_values) = root.get("param").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };
        if param_values.len() > PARAMS_MAX {
            return Err(RigError::new("TRG-INP-006", "parameter count exceeds cap"));
        }
        let mut params = Vec::with_capacity(param_values.len());
        for value in param_values {
            params.push(self.parse_param(value, nodes)?);
        }
        Ok(params)
    }

    fn parse_param(&mut self, value: &Value, nodes: &[Node]) -> Result<Param, RigError> {
        let obj = value
            .as_object()
            .ok_or_else(|| RigError::new("TRG-INP-006", "parameter is not an object"))?;
        let mut axis_points = [
            get_f32_list(obj, "axis_points", 0),
            get_f32_list(obj, "axis_points", 1),
        ];
        for axis in &mut axis_points {
            if axis.is_empty() {
                axis.push(0.0);
            }
        }
        let mut bindings = Vec::new();
        if let Some(binding_values) = obj.get("bindings").and_then(Value::as_array) {
            for binding_value in binding_values {
                match self.parse_binding(binding_value, nodes, &axis_points) {
                    Ok(Some(binding)) => bindings.push(binding),
                    Ok(None) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(Param {
            uuid: get_u32(obj, "uuid").unwrap_or(0),
            name: get_str(obj, "name").unwrap_or_default().to_owned(),
            is_vec2: get_bool(obj, "is_vec2").unwrap_or(false),
            min: get_vec2(obj, "min").unwrap_or(Vec2::ZERO),
            max: get_vec2(obj, "max").unwrap_or(Vec2::ZERO),
            defaults: get_vec2(obj, "defaults").unwrap_or(Vec2::ZERO),
            axis_points,
            bindings,
        })
    }

    fn parse_binding(
        &mut self,
        value: &Value,
        nodes: &[Node],
        axis_points: &[Vec<f32>; 2],
    ) -> Result<Option<Binding>, RigError> {
        let obj = value
            .as_object()
            .ok_or_else(|| RigError::new("TRG-INP-006", "binding is not an object"))?;
        let interpolate = match get_str(obj, "interpolate_mode") {
            Some("Linear") => InterpolateMode::Linear,
            Some("Nearest") => InterpolateMode::Nearest,
            _ => {
                // The reference payload parser drops bindings whose
                // interpolation mode it does not know (the corpus's
                // cubic bindings land here). Counted, not silent.
                self.dropped_unsupported_bindings += 1;
                return Ok(None);
            }
        };
        let Some(kind) = get_str(obj, "param_name").and_then(BindingKind::from_param_name) else {
            self.skipped_bindings += 1;
            return Ok(None);
        };
        let Some(node_uuid) = get_u32(obj, "node") else {
            self.skipped_bindings += 1;
            return Ok(None);
        };
        let Some(node_index) = nodes.iter().position(|n| n.uuid == node_uuid) else {
            self.skipped_bindings += 1;
            return Ok(None);
        };
        let grid = obj
            .get("values")
            .ok_or_else(|| RigError::new("TRG-INP-006", "binding without values"))?;
        let values = parse_binding_values(grid, kind, axis_points, nodes, node_index)?;
        let is_set = parse_is_set(obj);
        Ok(Some(Binding {
            node: node_index,
            kind,
            interpolate,
            values,
            is_set,
        }))
    }

    /// Apply the deferred fix-ups: physics nodes learn their
    /// parameter's index, and drawables learn their mask sources'
    /// arena indices. Unknown references are import errors, matching
    /// the reference route, which treats both as structural defects.
    fn apply_pending(&self, nodes: &mut [Node], params: &[Param]) -> Result<(), RigError> {
        for (node_index, param_uuid) in &self.pending_physics {
            let Some(param_index) = params.iter().position(|p| p.uuid == *param_uuid) else {
                return Err(RigError::new(
                    "TRG-INP-006",
                    format!("SimplePhysics node references unknown param uuid {param_uuid}"),
                ));
            };
            if let NodeKind::Physics(physics) = &mut nodes[*node_index].kind {
                physics.param = param_index;
            }
        }
        for (owner_index, source_uuid, mode) in &self.pending_masks {
            let Some(source_index) = nodes.iter().position(|n| n.uuid == *source_uuid) else {
                return Err(RigError::new(
                    "TRG-INP-006",
                    format!("mask references unknown node uuid {source_uuid}"),
                ));
            };
            let mask = Mask {
                source: source_index,
                mode: *mode,
            };
            match &mut nodes[*owner_index].kind {
                NodeKind::Part(part) => part.drawable.masks.push(mask),
                NodeKind::Composite(comp) => comp.drawable.masks.push(mask),
                _ => {
                    return Err(RigError::new(
                        "TRG-INP-006",
                        "mask declared on a non-drawable node",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The `transform` object of a node.
fn parse_transform(obj: &Map<String, Value>) -> TransformOffset {
    let Some(transform) = obj.get("transform").and_then(Value::as_object) else {
        return TransformOffset::default();
    };
    TransformOffset {
        translation: get_vec3(transform, "trans").unwrap_or(Vec3::new(0.0, 0.0, 0.0)),
        rotation: get_vec3(transform, "rot").unwrap_or(Vec3::new(0.0, 0.0, 0.0)),
        scale: get_vec2(transform, "scale").unwrap_or(Vec2::new(1.0, 1.0)),
        pixel_snap: get_bool(transform, "pixel_snap").unwrap_or(false),
    }
}

/// A part's mesh object.
fn parse_mesh(obj: &Map<String, Value>) -> Result<Mesh, RigError> {
    let Some(mesh) = obj.get("mesh").and_then(Value::as_object) else {
        return Ok(Mesh::default());
    };
    let verts = parse_flat_vec2(mesh, "verts")?;
    if verts.len() > VERTS_MAX {
        return Err(RigError::new(
            "TRG-INP-006",
            "mesh vertex count exceeds cap",
        ));
    }
    let uvs = parse_flat_vec2(mesh, "uvs")?;
    if uvs.len() != verts.len() {
        return Err(RigError::new(
            "TRG-INP-006",
            "mesh uv count does not match vertex count",
        ));
    }
    let mut indices = Vec::new();
    if let Some(raw_indices) = mesh.get("indices").and_then(Value::as_array) {
        for raw in raw_indices {
            // The reference parser collects indices with map_while:
            // the first non-u32 value ends the list.
            match raw.as_u64() {
                Some(v) if v <= u64::from(u32::MAX) => indices.push(v as u32),
                _ => break,
            }
        }
    }
    if indices.len() % 3 != 0 {
        return Err(RigError::new(
            "TRG-INP-006",
            "mesh index count is not a multiple of 3",
        ));
    }
    if indices.iter().any(|&i| i as usize >= verts.len()) {
        return Err(RigError::new(
            "TRG-INP-006",
            "mesh index references a missing vertex",
        ));
    }
    Ok(Mesh {
        verts,
        uvs,
        indices,
        origin: get_vec2(mesh, "origin").unwrap_or(Vec2::ZERO),
    })
}

/// Parse a binding's value grid for its kind, validating the grid
/// shape against the parameter's axis points.
fn parse_binding_values(
    grid: &Value,
    kind: BindingKind,
    axis_points: &[Vec<f32>; 2],
    nodes: &[Node],
    node_index: usize,
) -> Result<BindingValues, RigError> {
    let rows = grid
        .as_array()
        .ok_or_else(|| RigError::new("TRG-INP-006", "binding values are not a grid"))?;
    if rows.len() != axis_points[0].len() {
        return Err(RigError::new(
            "TRG-INP-006",
            "binding grid x size does not match axis points",
        ));
    }
    match kind {
        BindingKind::Deform => {
            // Deform bindings may target any node (Inochi2D aims
            // them at MeshGroups as well as parts). Only a part's
            // stack is ever sampled, so the cell-length contract is
            // enforced for part targets and the values are carried
            // verbatim for the rest, as the reference parser does.
            let vertex_count = match &nodes[node_index].kind {
                NodeKind::Part(part) => Some(part.mesh.verts.len()),
                _ => None,
            };
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let cells = row_array(row, axis_points[1].len())?;
                let mut out_row = Vec::with_capacity(cells.len());
                for cell in cells {
                    let offsets = parse_vec2_list(cell)?;
                    if let Some(expected) = vertex_count {
                        if offsets.len() != expected {
                            return Err(RigError::new(
                                "TRG-INP-006",
                                "deform cell length does not match vertex count",
                            ));
                        }
                    }
                    out_row.push(offsets);
                }
                out.push(out_row);
            }
            Ok(BindingValues::Deforms(out))
        }
        kind if kind.is_vector() => {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let cells = row_array(row, axis_points[1].len())?;
                let mut out_row = Vec::with_capacity(cells.len());
                for cell in cells {
                    out_row.push(value_as_vec2(cell).ok_or_else(|| {
                        RigError::new("TRG-INP-006", "vector binding cell is not a pair")
                    })?);
                }
                out.push(out_row);
            }
            Ok(BindingValues::Vectors(out))
        }
        _ => {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let cells = row_array(row, axis_points[1].len())?;
                let mut out_row = Vec::with_capacity(cells.len());
                for cell in cells {
                    out_row.push(cell.as_f64().map(|v| v as f32).ok_or_else(|| {
                        RigError::new("TRG-INP-006", "scalar binding cell is not a number")
                    })?);
                }
                out.push(out_row);
            }
            Ok(BindingValues::Scalars(out))
        }
    }
}

/// One row of a binding grid, shape-checked.
fn row_array(row: &Value, expected: usize) -> Result<&Vec<Value>, RigError> {
    let cells = row
        .as_array()
        .ok_or_else(|| RigError::new("TRG-INP-006", "binding grid row is not an array"))?;
    if cells.len() != expected {
        return Err(RigError::new(
            "TRG-INP-006",
            "binding grid y size does not match axis points",
        ));
    }
    Ok(cells)
}

/// The `isSet` grid of a binding (carried, never consulted).
fn parse_is_set(obj: &Map<String, Value>) -> Vec<Vec<bool>> {
    let Some(rows) = obj.get("isSet").and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .map(|row| {
            row.as_array()
                .map(|cells| cells.iter().map(|c| c.as_bool().unwrap_or(false)).collect())
                .unwrap_or_default()
        })
        .collect()
}

/// A flat `[x, y, x, y, ...]` array as points.
fn parse_flat_vec2(obj: &Map<String, Value>, key: &str) -> Result<Vec<Vec2>, RigError> {
    let Some(values) = obj.get(key).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    if values.len() % 2 != 0 {
        return Err(RigError::new(
            "TRG-INP-006",
            format!("{key} has an odd number of components"),
        ));
    }
    let mut out = Vec::with_capacity(values.len() / 2);
    for pair in values.chunks_exact(2) {
        let x = pair[0]
            .as_f64()
            .ok_or_else(|| RigError::new("TRG-INP-006", format!("{key} holds a non-number")))?
            as f32;
        let y = pair[1]
            .as_f64()
            .ok_or_else(|| RigError::new("TRG-INP-006", format!("{key} holds a non-number")))?
            as f32;
        out.push(Vec2::new(x, y));
    }
    Ok(out)
}

/// A JSON array of `[x, y]` pairs (deform cells).
fn parse_vec2_list(value: &Value) -> Result<Vec<Vec2>, RigError> {
    let Some(items) = value.as_array() else {
        return Err(RigError::new(
            "TRG-INP-006",
            "deform cell is not a list of pairs",
        ));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(
            value_as_vec2(item)
                .ok_or_else(|| RigError::new("TRG-INP-006", "deform offset is not a pair"))?,
        );
    }
    Ok(out)
}

/// A JSON value as a Vec2: `[x, y]`, or a bare number as `(n, 0)`.
fn value_as_vec2(value: &Value) -> Option<Vec2> {
    match value {
        Value::Array(items) if items.len() == 2 => Some(Vec2::new(
            items[0].as_f64()? as f32,
            items[1].as_f64()? as f32,
        )),
        Value::Number(n) => Some(Vec2::new(n.as_f64()? as f32, 0.0)),
        _ => None,
    }
}

fn get_str<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

fn get_bool(obj: &Map<String, Value>, key: &str) -> Option<bool> {
    obj.get(key).and_then(Value::as_bool)
}

fn get_f32(obj: &Map<String, Value>, key: &str) -> Option<f32> {
    obj.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

fn get_u32(obj: &Map<String, Value>, key: &str) -> Option<u32> {
    obj.get(key)
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

fn get_vec2(obj: &Map<String, Value>, key: &str) -> Option<Vec2> {
    obj.get(key).and_then(value_as_vec2)
}

fn get_vec3(obj: &Map<String, Value>, key: &str) -> Option<Vec3> {
    let items = obj.get(key)?.as_array()?;
    if items.len() != 3 {
        return None;
    }
    Some(Vec3::new(
        items[0].as_f64()? as f32,
        items[1].as_f64()? as f32,
        items[2].as_f64()? as f32,
    ))
}

/// One axis of a parameter's `axis_points` (`axis` 0 = x, 1 = y).
fn get_f32_list(obj: &Map<String, Value>, key: &str, axis: usize) -> Vec<f32> {
    let Some(axes) = obj.get(key).and_then(Value::as_array) else {
        return Vec::new();
    };
    let Some(points) = axes.get(axis).and_then(Value::as_array) else {
        return Vec::new();
    };
    points
        .iter()
        .filter_map(|v| v.as_f64().map(|n| n as f32))
        .collect()
}
