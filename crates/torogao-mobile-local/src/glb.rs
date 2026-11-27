//! A minimal GLB (binary glTF 2.0) reader for the bundled
//! canonical base: geometry, vertex colours, one skin, and
//! morph targets — including SPARSE-encoded morph accessors,
//! which is how the estate's Blender exports store the face
//! set. Read-only and total: malformed input is a typed error,
//! never a panic.
//!
//! Scope honesty: this is not a general glTF loader. It reads
//! the first scene, the first skin, and every mesh node bound
//! to it; it ignores cameras, lights, animations, and any
//! extension. That covers the canonical-base GLBs the estate
//! ships and nothing more is claimed.

use serde_json::Value;

use crate::math::Mat4;

/// A node's bind TRS: translation, rotation quaternion
/// `[x, y, z, w]`, scale.
pub type NodeTrs = ([f32; 3], [f32; 4], [f32; 3]);

/// One morph target in sparse form (index + delta), the shape
/// the deform path consumes directly.
#[derive(Debug, Clone)]
pub struct MorphTarget {
    /// Target name (from the mesh's `extras.targetNames`).
    pub name: String,
    /// Vertex indices the position deltas apply to.
    pub pos_idx: Vec<u32>,
    /// Position deltas, parallel to `pos_idx`.
    pub pos_delta: Vec<[f32; 3]>,
    /// Vertex indices the normal deltas apply to.
    pub nrm_idx: Vec<u32>,
    /// Normal deltas, parallel to `nrm_idx`.
    pub nrm_delta: Vec<[f32; 3]>,
}

/// One drawable primitive of the base mesh.
#[derive(Debug, Clone)]
pub struct Prim {
    /// Bind-pose positions.
    pub positions: Vec<[f32; 3]>,
    /// Bind-pose normals.
    pub normals: Vec<[f32; 3]>,
    /// `COLOR_0` vertex colours (RGBA), if the export has them.
    pub colors: Option<Vec<[f32; 4]>>,
    /// `JOINTS_0` per vertex; empty when unskinned.
    pub joints: Vec<[u16; 4]>,
    /// `WEIGHTS_0` per vertex; empty when unskinned.
    pub weights: Vec<[f32; 4]>,
    /// Triangle indices.
    pub indices: Vec<u32>,
    /// Morph targets in the mesh's declared order.
    pub morphs: Vec<MorphTarget>,
    /// Material base-colour factor (multiplies vertex colour).
    pub tint: [f32; 4],
}

/// The parsed canonical base: primitives plus the node/joint
/// data the pose path needs.
#[derive(Debug, Clone)]
pub struct BaseMesh {
    /// All primitives of all skinned mesh nodes, in node order.
    pub prims: Vec<Prim>,
    /// Morph target names, in declaration order.
    pub morph_names: Vec<String>,
    /// Joint names in skin order.
    pub joint_names: Vec<String>,
    /// Per-node bind-pose local matrices.
    pub node_local: Vec<Mat4>,
    /// Per-node bind TRS, when the node used TRS (pose overrides
    /// need it); `None` for matrix-form nodes.
    pub node_trs: Vec<Option<NodeTrs>>,
    /// Per-node parent index, -1 for scene roots.
    pub node_parents: Vec<i32>,
    /// Per-node name ("" when unnamed).
    pub node_names: Vec<String>,
    /// Node index per joint, parallel to `joint_names`.
    pub joint_nodes: Vec<usize>,
    /// Inverse bind matrix per joint.
    pub inverse_bind: Vec<Mat4>,
    /// Joint index per node, -1 when the node is not a joint.
    pub joint_of_node: Vec<i32>,
}

impl BaseMesh {
    /// Total vertex count across primitives.
    pub fn vertex_count(&self) -> usize {
        self.prims.iter().map(|p| p.positions.len()).sum()
    }

    /// Total triangle count across primitives.
    pub fn triangle_count(&self) -> usize {
        self.prims.iter().map(|p| p.indices.len() / 3).sum()
    }

    /// Joint index for a joint name, if the base carries it.
    pub fn joint_index(&self, name: &str) -> Option<usize> {
        self.joint_names.iter().position(|n| n == name)
    }
}

/// GLB load failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlbError {
    /// The container or JSON did not parse.
    Parse(String),
    /// A required piece of the file is absent.
    Missing(&'static str),
    /// A construct outside this reader's stated scope.
    Unsupported(String),
}

impl std::fmt::Display for GlbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GlbError::Parse(e) => write!(f, "GLB parse failed: {e}"),
            GlbError::Missing(what) => write!(f, "GLB is missing {what}"),
            GlbError::Unsupported(what) => write!(f, "GLB construct unsupported: {what}"),
        }
    }
}

impl std::error::Error for GlbError {}

fn get<'a>(v: &'a Value, key: &str) -> Result<&'a Value, GlbError> {
    v.get(key).ok_or(GlbError::Missing("field"))
}

fn arr<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, GlbError> {
    get(v, key)?
        .as_array()
        .ok_or_else(|| GlbError::Parse(format!("{key} is not an array")))
}

fn idx(v: &Value, key: &str) -> Option<usize> {
    v.get(key).and_then(Value::as_u64).map(|n| n as usize)
}

fn f32s(v: &Value, n: usize, fill: f32) -> [f32; 4] {
    let mut out = [fill; 4];
    if let Some(a) = v.as_array() {
        for (i, x) in a.iter().take(n).enumerate() {
            out[i] = x.as_f64().unwrap_or(fill as f64) as f32;
        }
    }
    out
}

fn comp_size(component_type: u64) -> Result<usize, GlbError> {
    match component_type {
        5120 | 5121 => Ok(1),
        5122 | 5123 => Ok(2),
        5125 | 5126 => Ok(4),
        other => Err(GlbError::Unsupported(format!("componentType {other}"))),
    }
}

fn type_components(ty: &str) -> Result<usize, GlbError> {
    match ty {
        "SCALAR" => Ok(1),
        "VEC2" => Ok(2),
        "VEC3" => Ok(3),
        "VEC4" | "MAT2" => Ok(4),
        "MAT3" => Ok(9),
        "MAT4" => Ok(16),
        other => Err(GlbError::Unsupported(format!("accessor type {other}"))),
    }
}

/// One raw accessor read: which buffer view, at what offset,
/// in what component shape.
struct RawSpec {
    view_idx: usize,
    byte_offset: usize,
    component_type: u64,
    count: usize,
    components: usize,
    normalized: bool,
}

/// Raw component read from one buffer view region.
fn read_raw(bin: &[u8], views: &[Value], spec: &RawSpec) -> Result<Vec<f32>, GlbError> {
    let RawSpec {
        view_idx,
        byte_offset,
        component_type,
        count,
        components,
        normalized,
    } = *spec;
    let view = views.get(view_idx).ok_or(GlbError::Missing("bufferView"))?;
    let cs = comp_size(component_type)?;
    let elem = cs * components;
    let view_off = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let stride = view
        .get("byteStride")
        .and_then(Value::as_u64)
        .map(|s| s as usize)
        .unwrap_or(elem);
    let start = view_off + byte_offset;
    let mut out = Vec::with_capacity(count * components);
    for i in 0..count {
        let base = start + i * stride;
        for c in 0..components {
            let at = base + c * cs;
            let bytes = bin
                .get(at..at + cs)
                .ok_or_else(|| GlbError::Parse("accessor overruns BIN chunk".into()))?;
            let v = match component_type {
                5126 => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                5121 => {
                    let v = bytes[0] as f32;
                    if normalized { v / 255.0 } else { v }
                }
                5120 => {
                    let v = bytes[0] as i8 as f32;
                    if normalized { (v / 127.0).max(-1.0) } else { v }
                }
                5123 => {
                    let v = u16::from_le_bytes([bytes[0], bytes[1]]) as f32;
                    if normalized { v / 65535.0 } else { v }
                }
                5122 => {
                    let v = i16::from_le_bytes([bytes[0], bytes[1]]) as f32;
                    if normalized {
                        (v / 32767.0).max(-1.0)
                    } else {
                        v
                    }
                }
                5125 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32,
                _ => unreachable!("comp_size already refused this type"),
            };
            out.push(v);
        }
    }
    Ok(out)
}

/// Read one accessor fully as f32 components, applying its
/// SPARSE overlay when present.
fn read_accessor(json: &Value, bin: &[u8], accessor_idx: usize) -> Result<Vec<f32>, GlbError> {
    let accessors = arr(json, "accessors")?;
    let views = arr(json, "bufferViews")?;
    let acc = accessors
        .get(accessor_idx)
        .ok_or(GlbError::Missing("accessor"))?;
    let component_type = acc
        .get("componentType")
        .and_then(Value::as_u64)
        .ok_or(GlbError::Missing("componentType"))?;
    let count = acc
        .get("count")
        .and_then(Value::as_u64)
        .ok_or(GlbError::Missing("accessor count"))? as usize;
    let components = type_components(
        acc.get("type")
            .and_then(Value::as_str)
            .ok_or(GlbError::Missing("accessor type"))?,
    )?;
    let normalized = acc
        .get("normalized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let byte_offset = acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let mut out = match acc.get("bufferView").and_then(Value::as_u64) {
        Some(view_idx) => read_raw(
            bin,
            views,
            &RawSpec {
                view_idx: view_idx as usize,
                byte_offset,
                component_type,
                count,
                components,
                normalized,
            },
        )?,
        None => vec![0.0; count * components],
    };
    if let Some(sparse) = acc.get("sparse") {
        let scount = sparse
            .get("count")
            .and_then(Value::as_u64)
            .ok_or(GlbError::Missing("sparse count"))? as usize;
        let indices_obj = get(sparse, "indices")?;
        let values_obj = get(sparse, "values")?;
        let itype = indices_obj
            .get("componentType")
            .and_then(Value::as_u64)
            .ok_or(GlbError::Missing("sparse index type"))?;
        let iview = indices_obj
            .get("bufferView")
            .and_then(Value::as_u64)
            .ok_or(GlbError::Missing("sparse index view"))? as usize;
        let ioff = indices_obj
            .get("byteOffset")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let idx_f = read_raw(
            bin,
            views,
            &RawSpec {
                view_idx: iview,
                byte_offset: ioff,
                component_type: itype,
                count: scount,
                components: 1,
                normalized: false,
            },
        )?;
        let vview = values_obj
            .get("bufferView")
            .and_then(Value::as_u64)
            .ok_or(GlbError::Missing("sparse value view"))? as usize;
        let voff = values_obj
            .get("byteOffset")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let vals = read_raw(
            bin,
            views,
            &RawSpec {
                view_idx: vview,
                byte_offset: voff,
                component_type,
                count: scount,
                components,
                normalized,
            },
        )?;
        for (i, &idx) in idx_f.iter().enumerate() {
            let target = idx as usize;
            if target >= count {
                return Err(GlbError::Parse("sparse index overruns accessor".into()));
            }
            out[target * components..(target + 1) * components]
                .copy_from_slice(&vals[i * components..(i + 1) * components]);
        }
    }
    Ok(out)
}

/// Read an accessor in sparse (index, value) form: for dense
/// accessors every vertex is listed, for sparse ones only the
/// stored entries. Values are grouped in `components`-tuples.
fn read_sparse(
    json: &Value,
    bin: &[u8],
    accessor_idx: usize,
    components: usize,
) -> Result<(Vec<u32>, Vec<f32>), GlbError> {
    let accessors = arr(json, "accessors")?;
    let acc = accessors
        .get(accessor_idx)
        .ok_or(GlbError::Missing("accessor"))?;
    let count = acc
        .get("count")
        .and_then(Value::as_u64)
        .ok_or(GlbError::Missing("accessor count"))? as usize;
    if acc.get("sparse").is_some() {
        // Recover the sparse index list by reading the dense
        // form and the sparse indices directly: cheaper to read
        // the indices object than to diff against zeros (deltas
        // can legitimately be zero on some axes).
        let views = arr(json, "bufferViews")?;
        let sparse = get(acc, "sparse")?;
        let scount = sparse
            .get("count")
            .and_then(Value::as_u64)
            .ok_or(GlbError::Missing("sparse count"))? as usize;
        let indices_obj = get(sparse, "indices")?;
        let itype = indices_obj
            .get("componentType")
            .and_then(Value::as_u64)
            .ok_or(GlbError::Missing("sparse index type"))?;
        let iview = indices_obj
            .get("bufferView")
            .and_then(Value::as_u64)
            .ok_or(GlbError::Missing("sparse index view"))? as usize;
        let ioff = indices_obj
            .get("byteOffset")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let idx_f = read_raw(
            bin,
            views,
            &RawSpec {
                view_idx: iview,
                byte_offset: ioff,
                component_type: itype,
                count: scount,
                components: 1,
                normalized: false,
            },
        )?;
        let dense = read_accessor(json, bin, accessor_idx)?;
        let mut indices = Vec::with_capacity(scount);
        let mut values = Vec::with_capacity(scount * components);
        for &idx in &idx_f {
            let t = idx as usize;
            indices.push(t as u32);
            values.extend_from_slice(&dense[t * components..(t + 1) * components]);
        }
        Ok((indices, values))
    } else {
        let dense = read_accessor(json, bin, accessor_idx)?;
        let indices: Vec<u32> = (0..count as u32).collect();
        Ok((indices, dense))
    }
}

fn to_vec3(flat: &[f32]) -> Vec<[f32; 3]> {
    flat.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()
}

fn to_vec4(flat: &[f32]) -> Vec<[f32; 4]> {
    flat.chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect()
}

fn attr<'a>(prim: &'a Value, name: &str) -> Option<&'a Value> {
    prim.get("attributes").and_then(|a| a.get(name))
}

/// Parse a GLB byte slice into a [`BaseMesh`].
pub fn load_glb(bytes: &[u8]) -> Result<BaseMesh, GlbError> {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return Err(GlbError::Parse("not a GLB container".into()));
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != 2 {
        return Err(GlbError::Unsupported(format!("glTF version {version}")));
    }
    let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let json_end = 20 + json_len;
    let json_bytes = bytes
        .get(20..json_end)
        .ok_or_else(|| GlbError::Parse("JSON chunk overruns file".into()))?;
    let json: Value =
        serde_json::from_slice(json_bytes).map_err(|e| GlbError::Parse(e.to_string()))?;
    let bin_off = json_end + 8;
    let bin = bytes.get(bin_off..).ok_or(GlbError::Missing("BIN chunk"))?;

    // Nodes: local transforms, parents, names.
    let nodes = arr(&json, "nodes")?;
    let mut node_local = Vec::with_capacity(nodes.len());
    let mut node_trs = Vec::with_capacity(nodes.len());
    let mut node_names = Vec::with_capacity(nodes.len());
    let mut node_parents = vec![-1i32; nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        node_names.push(
            node.get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        );
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for c in children {
                if let Some(ci) = c.as_u64() {
                    if let Some(p) = node_parents.get_mut(ci as usize) {
                        *p = i as i32;
                    }
                }
            }
        }
        if let Some(m) = node.get("matrix").and_then(Value::as_array) {
            let mut flat = [0.0f32; 16];
            for (k, x) in m.iter().take(16).enumerate() {
                flat[k] = x.as_f64().unwrap_or(0.0) as f32;
            }
            node_local.push(Mat4(flat));
            node_trs.push(None);
        } else {
            let t4 = node
                .get("translation")
                .map(|v| f32s(v, 3, 0.0))
                .unwrap_or([0.0; 4]);
            let r4 = node
                .get("rotation")
                .map(|v| f32s(v, 4, 0.0))
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let s4 = node
                .get("scale")
                .map(|v| f32s(v, 3, 1.0))
                .unwrap_or([1.0; 4]);
            let trs = ([t4[0], t4[1], t4[2]], r4, [s4[0], s4[1], s4[2]]);
            node_trs.push(Some(trs));
            node_local.push(Mat4::from_trs(trs.0, trs.1, trs.2));
        }
    }

    // Skin 0.
    let skins = arr(&json, "skins")?;
    let skin = skins.first().ok_or(GlbError::Missing("skin"))?;
    let joint_nodes: Vec<usize> = arr(skin, "joints")?
        .iter()
        .filter_map(Value::as_u64)
        .map(|n| n as usize)
        .collect();
    let joint_names: Vec<String> = joint_nodes
        .iter()
        .map(|&n| node_names.get(n).cloned().unwrap_or_default())
        .collect();
    let ibm_idx =
        idx(skin, "inverseBindMatrices").ok_or(GlbError::Missing("inverseBindMatrices"))?;
    let ibm_flat = read_accessor(&json, bin, ibm_idx)?;
    let inverse_bind: Vec<Mat4> = ibm_flat
        .chunks_exact(16)
        .map(|c| {
            let mut m = [0.0f32; 16];
            m.copy_from_slice(c);
            Mat4(m)
        })
        .collect();
    if inverse_bind.len() != joint_nodes.len() {
        return Err(GlbError::Parse(
            "inverseBindMatrices count does not match joints".into(),
        ));
    }
    let mut joint_of_node = vec![-1i32; nodes.len()];
    for (j, &n) in joint_nodes.iter().enumerate() {
        if let Some(slot) = joint_of_node.get_mut(n) {
            *slot = j as i32;
        }
    }

    // Materials for tints.
    let materials = json
        .get("materials")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tint_of = |prim: &Value| -> [f32; 4] {
        let Some(mi) = idx(prim, "material") else {
            return [1.0; 4];
        };
        let Some(mat) = materials.get(mi) else {
            return [1.0; 4];
        };
        match mat
            .get("pbrMetallicRoughness")
            .and_then(|p| p.get("baseColorFactor"))
        {
            Some(v) => f32s(v, 4, 1.0),
            None => [1.0; 4],
        }
    };

    // Primitives from every mesh node bound to skin 0.
    let meshes = arr(&json, "meshes")?;
    let mut prims = Vec::new();
    let mut morph_names: Vec<String> = Vec::new();
    for node in nodes.iter() {
        let (Some(mesh_idx), skin_idx) = (idx(node, "mesh"), idx(node, "skin")) else {
            continue;
        };
        if skin_idx != Some(0) {
            continue;
        }
        let mesh = meshes.get(mesh_idx).ok_or(GlbError::Missing("mesh"))?;
        let names: Vec<String> = mesh
            .get("extras")
            .and_then(|e| e.get("targetNames"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if morph_names.is_empty() && !names.is_empty() {
            morph_names = names.clone();
        }
        for prim_v in arr(mesh, "primitives")? {
            let pos_idx = attr(prim_v, "POSITION")
                .and_then(Value::as_u64)
                .ok_or(GlbError::Missing("POSITION"))? as usize;
            let positions = to_vec3(&read_accessor(&json, bin, pos_idx)?);
            let normals = match attr(prim_v, "NORMAL").and_then(Value::as_u64) {
                Some(i) => to_vec3(&read_accessor(&json, bin, i as usize)?),
                None => vec![[0.0, 1.0, 0.0]; positions.len()],
            };
            let colors = match attr(prim_v, "COLOR_0").and_then(Value::as_u64) {
                Some(i) => {
                    let flat = read_accessor(&json, bin, i as usize)?;
                    let comps = flat.len() / positions.len().max(1);
                    if comps == 4 {
                        Some(to_vec4(&flat))
                    } else if comps == 3 {
                        Some(
                            flat.chunks_exact(3)
                                .map(|c| [c[0], c[1], c[2], 1.0])
                                .collect(),
                        )
                    } else {
                        None
                    }
                }
                None => None,
            };
            let joints = match attr(prim_v, "JOINTS_0").and_then(Value::as_u64) {
                Some(i) => {
                    let flat = read_accessor(&json, bin, i as usize)?;
                    flat.chunks_exact(4)
                        .map(|c| [c[0] as u16, c[1] as u16, c[2] as u16, c[3] as u16])
                        .collect()
                }
                None => Vec::new(),
            };
            let weights = match attr(prim_v, "WEIGHTS_0").and_then(Value::as_u64) {
                Some(i) => to_vec4(&read_accessor(&json, bin, i as usize)?),
                None => Vec::new(),
            };
            let indices = match prim_v.get("indices").and_then(Value::as_u64) {
                Some(i) => read_accessor(&json, bin, i as usize)?
                    .iter()
                    .map(|v| *v as u32)
                    .collect(),
                None => (0..positions.len() as u32).collect(),
            };
            let mut morphs = Vec::new();
            if let Some(targets) = prim_v.get("targets").and_then(Value::as_array) {
                for (ti, target) in targets.iter().enumerate() {
                    let name = names.get(ti).cloned().unwrap_or_default();
                    let (pos_idx_s, pos_vals) = match target.get("POSITION").and_then(Value::as_u64)
                    {
                        Some(i) => read_sparse(&json, bin, i as usize, 3)?,
                        None => (Vec::new(), Vec::new()),
                    };
                    let (nrm_idx_s, nrm_vals) = match target.get("NORMAL").and_then(Value::as_u64) {
                        Some(i) => read_sparse(&json, bin, i as usize, 3)?,
                        None => (Vec::new(), Vec::new()),
                    };
                    morphs.push(MorphTarget {
                        name,
                        pos_idx: pos_idx_s,
                        pos_delta: to_vec3(&pos_vals),
                        nrm_idx: nrm_idx_s,
                        nrm_delta: to_vec3(&nrm_vals),
                    });
                }
            }
            prims.push(Prim {
                positions,
                normals,
                colors,
                joints,
                weights,
                indices,
                morphs,
                tint: tint_of(prim_v),
            });
        }
    }
    if prims.is_empty() {
        return Err(GlbError::Missing("skinned primitives"));
    }

    Ok(BaseMesh {
        prims,
        morph_names,
        joint_names,
        node_local,
        node_trs,
        node_parents,
        node_names,
        joint_nodes,
        inverse_bind,
        joint_of_node,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_is_a_typed_error_not_a_panic() {
        assert!(matches!(load_glb(b"nope"), Err(GlbError::Parse(_))));
        assert!(matches!(load_glb(&[]), Err(GlbError::Parse(_))));
        let mut fake = b"glTF".to_vec();
        fake.extend_from_slice(&2u32.to_le_bytes());
        fake.extend_from_slice(&20u32.to_le_bytes());
        assert!(load_glb(&fake).is_err());
    }
}
