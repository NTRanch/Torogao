//! A read model over a parsed GLB, shared by the asset-level
//! verbs (`asset`, `export-check`, `roundtrip`).
//!
//! GLB parsing is not re-implemented here: the estate's reader
//! is `torogao_vrm::glb` (the VRM crate's container parser and
//! structural census, proven on the VRM export path), reused
//! as-is — the B1/B2 records' one open dependency question,
//! answered in-tree: the workspace already carries a GLB
//! reader, so no external `gltf` dependency is added and no
//! second parser is hand-rolled. This module adds the typed,
//! bounded views the QC checks consume — materials, meshes,
//! nodes, images — plus world-space geometry (composed node
//! transforms, bounding box) and the structural summary the
//! round-trip tier diffs.

use crate::error::QcError;
use crate::limits::{ASSET_BYTES_MAX, NODE_DEPTH_MAX};
use crate::read_bounded;
use crate::report::round6;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use torogao_vrm::glb::{GlbFile, mesh_target_count, mesh_target_names, parse_glb};

/// One material's checkable surface.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialInfo {
    /// The material's name, when it declares one.
    pub name: Option<String>,
    /// `pbrMetallicRoughness.baseColorFactor`, or the glTF
    /// default (opaque white) when the material omits it.
    pub base_color: [f64; 4],
    /// True when the material binds a `normalTexture`.
    pub has_normal_texture: bool,
    /// Every texture index the material binds through its core
    /// texture fields (base colour, metallic-roughness, normal,
    /// emissive, occlusion).
    pub textures: BTreeSet<usize>,
}

/// One mesh primitive's checkable surface.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimitiveInfo {
    /// Index into the model's materials, when bound.
    pub material: Option<usize>,
    /// Attribute names present (`POSITION`, `TEXCOORD_0`,
    /// `TANGENT`, ...).
    pub attributes: BTreeSet<String>,
    /// POSITION accessor element count, when a POSITION
    /// attribute is declared.
    pub position_count: Option<usize>,
    /// POSITION accessor `min`/`max`, when both are declared.
    pub position_minmax: Option<([f64; 3], [f64; 3])>,
    /// Index accessor element count, when indexed.
    pub indices_count: Option<usize>,
    /// Primitive mode (4 = triangles; absent means 4).
    pub mode: u64,
    /// Morph-target count of this primitive's `targets` array.
    pub target_count: usize,
}

/// One mesh's checkable surface.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshInfo {
    /// The mesh's name, when it declares one.
    pub name: Option<String>,
    /// Its primitives, in document order.
    pub primitives: Vec<PrimitiveInfo>,
    /// Morph-target names from `extras.targetNames`.
    pub target_names: Vec<String>,
    /// Morph-target count (declared names when present, else
    /// the first primitive's `targets` length).
    pub target_count: usize,
}

/// One node's checkable surface. Transform fields are the
/// decomposed local transform whether the file stores TRS or
/// a composed `matrix`.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeInfo {
    /// The node's name, when it declares one.
    pub name: Option<String>,
    /// Parent node index, derived from the `children` lists.
    pub parent: Option<usize>,
    /// Child node indices, in document order.
    pub children: Vec<usize>,
    /// Mesh index, when the node carries one.
    pub mesh: Option<usize>,
    /// Local translation.
    pub translation: [f64; 3],
    /// Local rotation, quaternion `[x, y, z, w]`.
    pub rotation: [f64; 4],
    /// Local scale.
    pub scale: [f64; 3],
    /// Local matrix, column-major (`m[col * 4 + row]`).
    pub matrix: [f64; 16],
}

/// One image's checkable surface.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageInfo {
    /// The image's MIME type, when embedded.
    pub mime_type: Option<String>,
    /// Buffer view holding the image bytes, when embedded.
    pub buffer_view: Option<usize>,
    /// External URI, when the image is not embedded.
    pub uri: Option<String>,
}

/// The outcome of a world-space bounding-box measurement.
#[derive(Debug, Clone, PartialEq)]
pub enum AabbOutcome {
    /// The model declares no mesh geometry.
    NoMeshes,
    /// The measured world-space box.
    Measured([f64; 3], [f64; 3]),
    /// A node chain exceeded [`NODE_DEPTH_MAX`]; the box would
    /// be incomplete, so none is reported.
    TooDeep,
}

/// A parsed GLB plus the views the checks consume.
#[derive(Debug, Clone)]
pub struct GlbModel {
    file: GlbFile,
    byte_len: usize,
}

impl GlbModel {
    /// Reads and parses a GLB file. Fail-closed: an unreadable
    /// or unparseable asset is a typed error, never a finding —
    /// an asset that cannot be opened cannot be certified.
    pub fn load(path: &Path) -> Result<GlbModel, QcError> {
        let bytes = read_bounded(path, ASSET_BYTES_MAX, "ASSET")?;
        let byte_len = bytes.len();
        let file = parse_glb(&bytes).map_err(|err| {
            QcError::new(
                "QC-E-ASSET-PARSE",
                format!("{} is not a readable GLB: {err}", path.display()),
            )
        })?;
        Ok(GlbModel { file, byte_len })
    }

    /// The file's length in bytes, as read.
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// The glTF JSON document.
    pub fn json(&self) -> &Value {
        &self.file.json
    }

    /// Every material, in document order.
    pub fn materials(&self) -> Vec<MaterialInfo> {
        arr(&self.file.json, "materials")
            .iter()
            .map(|material| {
                let pbr = material.get("pbrMetallicRoughness");
                let base_color = pbr
                    .and_then(|p| p.get("baseColorFactor"))
                    .and_then(vec4_of)
                    .unwrap_or([1.0, 1.0, 1.0, 1.0]);
                let mut textures = BTreeSet::new();
                let mut bind = |holder: Option<&Value>, key: &str| {
                    if let Some(index) = holder
                        .and_then(|h| h.get(key))
                        .and_then(|t| t.get("index"))
                        .and_then(Value::as_u64)
                    {
                        textures.insert(index as usize);
                    }
                };
                bind(pbr, "baseColorTexture");
                bind(pbr, "metallicRoughnessTexture");
                bind(Some(material), "normalTexture");
                bind(Some(material), "emissiveTexture");
                bind(Some(material), "occlusionTexture");
                MaterialInfo {
                    name: get_str(material, "name").map(str::to_string),
                    base_color,
                    has_normal_texture: material.get("normalTexture").is_some(),
                    textures,
                }
            })
            .collect()
    }

    /// Every mesh, in document order.
    pub fn meshes(&self) -> Vec<MeshInfo> {
        let root = &self.file.json;
        arr(root, "meshes")
            .iter()
            .map(|mesh| {
                let primitives = arr(mesh, "primitives")
                    .iter()
                    .map(|primitive| {
                        let attributes: BTreeSet<String> = primitive
                            .get("attributes")
                            .and_then(Value::as_object)
                            .map(|map| map.keys().cloned().collect())
                            .unwrap_or_default();
                        let position = primitive
                            .get("attributes")
                            .and_then(|a| a.get("POSITION"))
                            .and_then(Value::as_u64)
                            .map(|index| index as usize);
                        let (position_count, position_minmax) = match position {
                            Some(accessor) => {
                                let accessor = accessor_at(root, accessor);
                                (
                                    accessor.and_then(|a| get_usize(a, "count")),
                                    accessor.and_then(|a| {
                                        let min = a.get("min").and_then(vec3_of);
                                        let max = a.get("max").and_then(vec3_of);
                                        min.zip(max)
                                    }),
                                )
                            }
                            None => (None, None),
                        };
                        let indices_count = primitive
                            .get("indices")
                            .and_then(Value::as_u64)
                            .and_then(|index| accessor_at(root, index as usize))
                            .and_then(|a| get_usize(a, "count"));
                        PrimitiveInfo {
                            material: get_usize(primitive, "material"),
                            attributes,
                            position_count,
                            position_minmax,
                            indices_count,
                            mode: primitive.get("mode").and_then(Value::as_u64).unwrap_or(4),
                            target_count: arr(primitive, "targets").len(),
                        }
                    })
                    .collect();
                MeshInfo {
                    name: get_str(mesh, "name").map(str::to_string),
                    primitives,
                    target_names: mesh_target_names(mesh),
                    target_count: mesh_target_count(mesh),
                }
            })
            .collect()
    }

    /// Every node, in document order, with parents derived
    /// from the `children` lists.
    pub fn nodes(&self) -> Vec<NodeInfo> {
        let raw = arr(&self.file.json, "nodes");
        let mut parents: Vec<Option<usize>> = vec![None; raw.len()];
        for (index, node) in raw.iter().enumerate() {
            for child in arr(node, "children") {
                if let Some(child) = child.as_u64() {
                    if let Some(slot) = parents.get_mut(child as usize) {
                        *slot = Some(index);
                    }
                }
            }
        }
        raw.iter()
            .enumerate()
            .map(|(index, node)| {
                let (translation, rotation, scale, matrix) =
                    if let Some(values) = node.get("matrix").and_then(Value::as_array) {
                        let mut matrix = [0.0f64; 16];
                        for (slot, value) in matrix.iter_mut().zip(values.iter()) {
                            *slot = value.as_f64().unwrap_or(0.0);
                        }
                        let (t, r, s) = decompose(&matrix);
                        (t, r, s, matrix)
                    } else {
                        let translation = node
                            .get("translation")
                            .and_then(vec3_of)
                            .unwrap_or([0.0, 0.0, 0.0]);
                        let rotation = node
                            .get("rotation")
                            .and_then(vec4_of)
                            .unwrap_or([0.0, 0.0, 0.0, 1.0]);
                        let scale = node
                            .get("scale")
                            .and_then(vec3_of)
                            .unwrap_or([1.0, 1.0, 1.0]);
                        let matrix = compose(&translation, &rotation, &scale);
                        (translation, rotation, scale, matrix)
                    };
                NodeInfo {
                    name: get_str(node, "name").map(str::to_string),
                    parent: parents[index],
                    children: arr(node, "children")
                        .iter()
                        .filter_map(Value::as_u64)
                        .map(|c| c as usize)
                        .collect(),
                    mesh: get_usize(node, "mesh"),
                    translation,
                    rotation,
                    scale,
                    matrix,
                }
            })
            .collect()
    }

    /// The default scene's root node indices (the document's
    /// `scene` field, else scene 0; when the document declares
    /// no scenes, every parentless node).
    pub fn scene_roots(&self) -> Vec<usize> {
        let nodes = self.nodes();
        let scene_index = self
            .file
            .json
            .get("scene")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        if let Some(scene) = arr(&self.file.json, "scenes").get(scene_index) {
            return arr(scene, "nodes")
                .iter()
                .filter_map(Value::as_u64)
                .map(|n| n as usize)
                .filter(|n| *n < nodes.len())
                .collect();
        }
        nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.parent.is_none())
            .map(|(index, _)| index)
            .collect()
    }

    /// Every image, in document order.
    pub fn images(&self) -> Vec<ImageInfo> {
        arr(&self.file.json, "images")
            .iter()
            .map(|image| ImageInfo {
                mime_type: get_str(image, "mimeType").map(str::to_string),
                buffer_view: get_usize(image, "bufferView"),
                uri: get_str(image, "uri").map(str::to_string),
            })
            .collect()
    }

    /// The bytes of an embedded image, when it lives in the
    /// GLB's own binary chunk (buffer 0). External or malformed
    /// references yield `None`.
    pub fn image_bytes(&self, index: usize) -> Option<&[u8]> {
        let image = arr(&self.file.json, "images").get(index)?;
        let view_index = get_usize(image, "bufferView")?;
        let view = arr(&self.file.json, "bufferViews").get(view_index)?;
        if get_usize(view, "buffer").unwrap_or(0) != 0 {
            return None;
        }
        let offset = get_usize(view, "byteOffset").unwrap_or(0);
        let length = get_usize(view, "byteLength")?;
        let end = offset.checked_add(length)?;
        self.file.bin.get(offset..end)
    }

    /// Total triangle count over all triangle-mode primitives
    /// (indexed: index count / 3; otherwise position count / 3).
    pub fn triangle_count(&self) -> u64 {
        self.meshes().iter().map(mesh_triangles).sum()
    }

    /// The world-space bounding box over every mesh primitive's
    /// declared POSITION min/max, composed through the node
    /// tree from the scene roots.
    pub fn world_aabb(&self) -> AabbOutcome {
        let nodes = self.nodes();
        let meshes = self.meshes();
        let mut acc: Option<([f64; 3], [f64; 3])> = None;
        // Iterative walk: (node, world matrix, depth).
        let mut stack: Vec<(usize, [f64; 16], usize)> = self
            .scene_roots()
            .into_iter()
            .map(|root| (root, IDENTITY, 0))
            .collect();
        while let Some((index, parent_world, depth)) = stack.pop() {
            if depth > NODE_DEPTH_MAX {
                return AabbOutcome::TooDeep;
            }
            let Some(node) = nodes.get(index) else {
                continue;
            };
            let world = mat_mul(&parent_world, &node.matrix);
            if let Some(mesh) = node.mesh.and_then(|m| meshes.get(m)) {
                for primitive in &mesh.primitives {
                    if let Some((min, max)) = primitive.position_minmax {
                        for corner in corners(&min, &max) {
                            let point = mat_transform_point(&world, &corner);
                            match &mut acc {
                                None => acc = Some((point, point)),
                                Some((lo, hi)) => {
                                    for axis in 0..3 {
                                        lo[axis] = lo[axis].min(point[axis]);
                                        hi[axis] = hi[axis].max(point[axis]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for child in &node.children {
                stack.push((*child, world, depth + 1));
            }
        }
        match acc {
            Some((lo, hi)) => AabbOutcome::Measured(lo, hi),
            None => AabbOutcome::NoMeshes,
        }
    }

    /// The structural summary the round-trip tier diffs:
    /// node inventory with local transforms and per-mesh
    /// counts, in the same shape the Blender reimport runner
    /// emits. Floats are rounded to 6 places so the summary is
    /// stable across producers.
    pub fn summarize(&self) -> Value {
        let nodes = self.nodes();
        let meshes = self.meshes();
        let materials = self.materials();
        let node_values: Vec<Value> = nodes
            .iter()
            .map(|node| {
                let mesh_value = node.mesh.and_then(|index| meshes.get(index)).map(|mesh| {
                    let vertices: usize = mesh
                        .primitives
                        .iter()
                        .map(|p| p.position_count.unwrap_or(0))
                        .sum();
                    let mut mat_names: Vec<String> = mesh
                        .primitives
                        .iter()
                        .filter_map(|p| p.material)
                        .filter_map(|m| materials.get(m))
                        .map(|m| m.name.clone().unwrap_or_default())
                        .collect();
                    mat_names.sort();
                    mat_names.dedup();
                    json!({
                        "name": mesh.name.clone().unwrap_or_default(),
                        "vertices": vertices,
                        "triangles": mesh_triangles(mesh),
                        "materials": mat_names,
                        "targets": mesh.target_names,
                        "target_count": mesh.target_count,
                    })
                });
                json!({
                    "name": node.name.clone().unwrap_or_default(),
                    "parent": node.parent
                        .and_then(|p| nodes.get(p))
                        .map(|pn| pn.name.clone().unwrap_or_default()),
                    "translation": rounded3(&node.translation),
                    "rotation": rounded4(&canonical_quat(&node.rotation)),
                    "scale": rounded3(&node.scale),
                    "mesh": mesh_value,
                })
            })
            .collect();
        json!({ "nodes": node_values })
    }
}

/// The triangle count of one mesh (triangle-mode primitives
/// only).
pub fn mesh_triangles(mesh: &MeshInfo) -> u64 {
    mesh.primitives
        .iter()
        .filter(|p| p.mode == 4)
        .map(|p| match p.indices_count {
            Some(count) => (count / 3) as u64,
            None => (p.position_count.unwrap_or(0) / 3) as u64,
        })
        .sum()
}

/// The 4x4 identity, column-major.
pub const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// Composes a local matrix from translation, rotation
/// (quaternion `[x, y, z, w]`), and scale: `T * R * S`.
pub fn compose(t: &[f64; 3], q: &[f64; 4], s: &[f64; 3]) -> [f64; 16] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    // Rotation columns (basis vectors), from the quaternion.
    let r00 = 1.0 - 2.0 * (y * y + z * z);
    let r10 = 2.0 * (x * y + z * w);
    let r20 = 2.0 * (x * z - y * w);
    let r01 = 2.0 * (x * y - z * w);
    let r11 = 1.0 - 2.0 * (x * x + z * z);
    let r21 = 2.0 * (y * z + x * w);
    let r02 = 2.0 * (x * z + y * w);
    let r12 = 2.0 * (y * z - x * w);
    let r22 = 1.0 - 2.0 * (x * x + y * y);
    [
        r00 * s[0],
        r10 * s[0],
        r20 * s[0],
        0.0,
        r01 * s[1],
        r11 * s[1],
        r21 * s[1],
        0.0,
        r02 * s[2],
        r12 * s[2],
        r22 * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

/// Decomposes a column-major matrix into translation,
/// rotation quaternion, and scale. A negative determinant is
/// carried on the X scale, per the usual convention.
pub fn decompose(m: &[f64; 16]) -> ([f64; 3], [f64; 4], [f64; 3]) {
    let translation = [m[12], m[13], m[14]];
    let columns = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
    let mut scale = [norm(&columns[0]), norm(&columns[1]), norm(&columns[2])];
    let det = dot(&columns[0], &cross(&columns[1], &columns[2]));
    if det < 0.0 {
        scale[0] = -scale[0];
    }
    let mut basis = [[0.0f64; 3]; 3];
    for (axis, column) in columns.iter().enumerate() {
        let s = scale[axis];
        if s != 0.0 {
            for (row, value) in column.iter().enumerate() {
                basis[axis][row] = value / s;
            }
        }
    }
    // basis[axis] is the rotated axis (a matrix column); the
    // quaternion routine wants m[row][col].
    let r = [
        [basis[0][0], basis[1][0], basis[2][0]],
        [basis[0][1], basis[1][1], basis[2][1]],
        [basis[0][2], basis[1][2], basis[2][2]],
    ];
    (translation, quat_from_rotation(&r), scale)
}

/// Matrix product `a * b` (b applied first), column-major.
pub fn mat_mul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut out = [0.0f64; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum();
        }
    }
    out
}

/// Transforms a point by a column-major matrix.
pub fn mat_transform_point(m: &[f64; 16], p: &[f64; 3]) -> [f64; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

/// Canonicalises a quaternion's sign (w >= 0) so equal
/// rotations serialise identically.
pub fn canonical_quat(q: &[f64; 4]) -> [f64; 4] {
    if q[3] < 0.0 {
        [-q[0], -q[1], -q[2], -q[3]]
    } else {
        *q
    }
}

fn quat_from_rotation(m: &[[f64; 3]; 3]) -> [f64; 4] {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let (x, y, z, w);
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        w = 0.25 * s;
        x = (m[2][1] - m[1][2]) / s;
        y = (m[0][2] - m[2][0]) / s;
        z = (m[1][0] - m[0][1]) / s;
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        w = (m[2][1] - m[1][2]) / s;
        x = 0.25 * s;
        y = (m[0][1] + m[1][0]) / s;
        z = (m[0][2] + m[2][0]) / s;
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        w = (m[0][2] - m[2][0]) / s;
        x = (m[0][1] + m[1][0]) / s;
        y = 0.25 * s;
        z = (m[1][2] + m[2][1]) / s;
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        w = (m[1][0] - m[0][1]) / s;
        x = (m[0][2] + m[2][0]) / s;
        y = (m[1][2] + m[2][1]) / s;
        z = 0.25 * s;
    }
    canonical_quat(&[x, y, z, w])
}

fn corners(min: &[f64; 3], max: &[f64; 3]) -> [[f64; 3]; 8] {
    let mut out = [[0.0f64; 3]; 8];
    for (i, corner) in out.iter_mut().enumerate() {
        *corner = [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ];
    }
    out
}

fn norm(v: &[f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn rounded3(v: &[f64; 3]) -> Value {
    json!([round6(v[0]), round6(v[1]), round6(v[2])])
}

fn rounded4(v: &[f64; 4]) -> Value {
    json!([round6(v[0]), round6(v[1]), round6(v[2]), round6(v[3])])
}

/// A JSON array field as a slice (empty when absent or not an
/// array).
fn arr<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn get_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn get_usize(value: &Value, key: &str) -> Option<usize> {
    value.get(key).and_then(Value::as_u64).map(|n| n as usize)
}

fn accessor_at(root: &Value, index: usize) -> Option<&Value> {
    arr(root, "accessors").get(index)
}

/// A 3-number array as `[f64; 3]`, when it has exactly that
/// shape and all values are numbers.
pub fn vec3_of(value: &Value) -> Option<[f64; 3]> {
    let items = value.as_array()?;
    if items.len() != 3 {
        return None;
    }
    Some([items[0].as_f64()?, items[1].as_f64()?, items[2].as_f64()?])
}

/// A 4-number array as `[f64; 4]`.
pub fn vec4_of(value: &Value) -> Option<[f64; 4]> {
    let items = value.as_array()?;
    if items.len() != 4 {
        return None;
    }
    Some([
        items[0].as_f64()?,
        items[1].as_f64()?,
        items[2].as_f64()?,
        items[3].as_f64()?,
    ])
}
