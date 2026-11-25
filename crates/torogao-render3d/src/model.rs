//! The decoded model: an API-agnostic, renderer-owned view
//! of a GLB. Pure data — no GPU types cross this module, so
//! the pose layer, the CPU reference and the backend all read
//! the same facts.

use gltf::mesh::Semantic;
use serde::Deserialize;

use crate::accessor::{read_f64s, read_indices, read_joints, read_rows};
use crate::error::RenderError;
use crate::math::Mat4;
use crate::toon::ToonImage;

/// One morph target's sparse deltas, merged over the
/// position and normal sparsity maps (Blender exports the
/// same index set for both; a vertex missing on one side
/// contributes a zero delta there).
#[derive(Debug, Clone)]
pub struct MorphTarget {
    /// `(vertex, d_position, d_normal)` sorted by vertex:
    /// the union support of the position/normal deltas with
    /// at least one non-zero component.
    pub entries: Vec<(u32, [f32; 3], [f32; 3])>,
    /// Entries the GLB's sparse POSITION accessor stores
    /// (the census number the gate reports, e.g. 232 for
    /// Prima's `Fac_Eye_Close`). 0 for a dense accessor.
    pub stored_position_entries: usize,
}

/// One mesh primitive with everything the pipeline needs.
#[derive(Debug, Clone)]
pub struct Mesh {
    /// glTF mesh name (e.g. `Prima_Body`).
    pub name: String,
    /// Node that carries this mesh.
    pub node: usize,
    /// Index into `Model::materials`.
    pub material: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Vertex colour set 0 (albedo multiplier; glTF linear).
    pub color0: Vec<[f32; 4]>,
    /// Vertex colour set 1. On the Prima family this is
    /// the PAINT set (COLOR_0 is constant white there);
    /// the backend's packing chooses it as the albedo
    /// source when present. See the proving test
    /// `color1_evidence` for the measured basis.
    pub color1: Vec<[f32; 4]>,
    /// Whether the GLB actually carried COLOR_1 (vs the
    /// zero-filled fallback).
    pub has_color1: bool,
    /// UV set 0 (TEXCOORD_0): the paint-space UV the face
    /// SDF is authored in. Zeros when absent.
    pub texcoord0: Vec<[f32; 2]>,
    /// UV set 1 (TEXCOORD_1): the packed-lightmap UV of
    /// the R6 channel assignment. Zeros when absent.
    pub texcoord1: Vec<[f32; 2]>,
    /// Whether the GLB actually carried TEXCOORD_1.
    pub has_texcoord1: bool,
    /// Outline normals (Amendment A4): the smoothed second
    /// normal set the inverted-hull outline pass displaces
    /// along. Source order: the `_OUTLINE_NORMAL` custom
    /// attribute, then TANGENT xyz, then the geometric
    /// normal as the declared fallback.
    pub outline_normal: Vec<[f32; 3]>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub targets: Vec<MorphTarget>,
    /// Morph target names in GLB order (`extras.targetNames`).
    pub target_names: Vec<String>,
    /// The GLB's saved morph weights.
    pub default_weights: Vec<f32>,
}

impl Mesh {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Index of a morph target by name.
    pub fn target_index(&self, name: &str) -> Option<usize> {
        self.target_names.iter().position(|n| n == name)
    }
}

/// A scene node with its rest local transform.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Rest local transform (from TRS or a baked matrix).
    pub local: Mat4,
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
    pub scale: [f64; 3],
    pub mesh: Option<usize>,
}

/// The model's single skin.
#[derive(Debug, Clone)]
pub struct Skin {
    pub name: String,
    /// Node indices, in skin (joint-matrix) order.
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
}

/// A PBR material. Family B: the base colour may carry a
/// texture (a part atlas — face, hair, ...); `base_color_image`
/// is the GLB image index it samples, and `albedo_slot` is its
/// slot in `Model::albedos` after `resolve_albedos` runs.
#[derive(Debug, Clone)]
pub struct Material {
    pub name: String,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub double_sided: bool,
    /// GLB image index of the base-colour texture, when the
    /// material carries one (TEXCOORD_0 space only in v1).
    pub base_color_image: Option<usize>,
    /// Slot into `Model::albedos` for this material's decoded
    /// base-colour texture; `None` for untextured materials.
    /// Filled by `resolve_albedos` after the material loop.
    pub albedo_slot: Option<usize>,
}

/// The whole decoded GLB.
#[derive(Debug, Clone)]
pub struct Model {
    pub meshes: Vec<Mesh>,
    pub nodes: Vec<Node>,
    pub skin: Skin,
    pub materials: Vec<Material>,
    /// The model's decoded base-colour textures (Family B
    /// v2): one entry per distinct GLB image any material
    /// samples, in first-use order, decoded to RGBA8 with
    /// row 0 at glTF v = 0 (the image's top row). Materials
    /// point here through `Material::albedo_slot`. Empty
    /// for vertex-colour models.
    pub albedos: Vec<ToonImage>,
}

impl Model {
    /// First mesh whose name ends with `suffix`.
    pub fn mesh_named(&self, suffix: &str) -> Option<&Mesh> {
        self.meshes.iter().find(|m| m.name.ends_with(suffix))
    }

    /// Node index for a node name.
    pub fn node_named(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }
}

#[derive(Deserialize)]
struct TargetNames {
    #[serde(rename = "targetNames")]
    target_names: Vec<String>,
}

/// Read a colour accessor (VEC3 or VEC4) into RGBA rows;
/// a VEC3 source gains alpha 1 per the glTF specification.
fn read_colors<'a>(
    document: &'a gltf::Document,
    buffers: &[gltf::buffer::Data],
    accessor: &gltf::Accessor<'a>,
) -> Result<Vec<[f32; 4]>, RenderError> {
    use gltf::accessor::Dimensions;
    match accessor.dimensions() {
        Dimensions::Vec4 => read_rows::<4>(document, buffers, accessor),
        Dimensions::Vec3 => {
            let rows = read_rows::<3>(document, buffers, accessor)?;
            Ok(rows.into_iter().map(|c| [c[0], c[1], c[2], 1.0]).collect())
        }
        other => Err(RenderError::Accessor {
            index: accessor.index(),
            detail: format!("colour accessor has dimensions {other:?}"),
        }),
    }
}

/// Accessor indices of the `_OUTLINE_NORMAL` custom
/// attribute, keyed by (mesh index, primitive index).
/// The gltf crate does not surface custom vertex
/// attributes, so the GLB's JSON chunk is read directly;
/// non-GLB inputs carry no chunk and yield an empty map
/// (the loader then falls back to TANGENT / NORMAL).
fn outline_normal_accessors(bytes: &[u8]) -> std::collections::HashMap<(usize, usize), usize> {
    let mut out = std::collections::HashMap::new();
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return out;
    }
    let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let json_end = 20usize.saturating_add(json_len).min(bytes.len());
    let Ok(root) = serde_json::from_slice::<serde_json::Value>(&bytes[20..json_end]) else {
        return out;
    };
    let Some(meshes) = root.get("meshes").and_then(|m| m.as_array()) else {
        return out;
    };
    for (mesh_index, mesh) in meshes.iter().enumerate() {
        let Some(prims) = mesh.get("primitives").and_then(|p| p.as_array()) else {
            continue;
        };
        for (prim_index, prim) in prims.iter().enumerate() {
            let index = prim
                .get("attributes")
                .and_then(|a| a.get("_OUTLINE_NORMAL"))
                .and_then(serde_json::Value::as_u64);
            if let Some(index) = index {
                out.insert((mesh_index, prim_index), index as usize);
            }
        }
    }
    out
}

/// The outline-normal set for one primitive (Amendment
/// A4): the custom attribute when present and well-sized,
/// else TANGENT xyz, else the geometric normals.
#[allow(clippy::too_many_arguments)]
fn read_outline_normals(
    custom: &std::collections::HashMap<(usize, usize), usize>,
    mesh_index: usize,
    prim_index: usize,
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    prim: &gltf::mesh::Primitive,
    normals: &[[f32; 3]],
) -> Result<Vec<[f32; 3]>, RenderError> {
    if let Some(&accessor_index) = custom.get(&(mesh_index, prim_index)) {
        let accessor = document
            .accessors()
            .nth(accessor_index)
            .ok_or(RenderError::Accessor {
                index: accessor_index,
                detail: "_OUTLINE_NORMAL accessor index out of range".to_string(),
            })?;
        let rows = read_rows::<3>(document, buffers, &accessor)?;
        if rows.len() == normals.len() {
            return Ok(rows);
        }
    }
    if let Some(tangent) = prim.get(&Semantic::Tangents) {
        let rows = read_rows::<4>(document, buffers, &tangent)?;
        if rows.len() == normals.len() {
            return Ok(rows.iter().map(|t| [t[0], t[1], t[2]]).collect());
        }
    }
    Ok(normals.to_vec())
}

/// Load and decode a GLB from memory.
pub fn load_model(bytes: &[u8]) -> Result<Model, RenderError> {
    let custom_outline = outline_normal_accessors(bytes);
    let (document, buffers, images) =
        gltf::import_slice(bytes).map_err(|e| RenderError::GltfParse(e.to_string()))?;

    // Materials first (meshes index into them).
    let mut materials = Vec::new();
    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        // Family B: a base-colour texture is accepted when
        // it samples TEXCOORD_0 (the paint space the atlas
        // is authored in). Any other UV set is a precise
        // error, not a silent mis-sample.
        let base_color_image = match pbr.base_color_texture() {
            Some(info) => {
                if info.tex_coord() != 0 {
                    return Err(RenderError::Document(format!(
                        "material {}: base-colour texture samples TEXCOORD_{}, \
                         Family B v1 supports TEXCOORD_0 only",
                        material.name().unwrap_or("?"),
                        info.tex_coord()
                    )));
                }
                Some(info.texture().source().index())
            }
            None => None,
        };
        materials.push(Material {
            name: material.name().unwrap_or("").to_string(),
            base_color: pbr.base_color_factor(),
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            double_sided: material.double_sided(),
            base_color_image,
            albedo_slot: None,
        });
    }
    let albedos = resolve_albedos(&mut materials, &images)?;
    if materials.is_empty() {
        return Err(RenderError::Missing("materials"));
    }

    // Nodes with parents resolved from children lists.
    let mut nodes: Vec<Node> = Vec::new();
    for node in document.nodes() {
        let (translation, rotation, scale, local) = match node.transform() {
            gltf::scene::Transform::Decomposed {
                translation,
                rotation,
                scale,
            } => {
                let t = [
                    f64::from(translation[0]),
                    f64::from(translation[1]),
                    f64::from(translation[2]),
                ];
                let r = [
                    f64::from(rotation[0]),
                    f64::from(rotation[1]),
                    f64::from(rotation[2]),
                    f64::from(rotation[3]),
                ];
                let s = [
                    f64::from(scale[0]),
                    f64::from(scale[1]),
                    f64::from(scale[2]),
                ];
                (t, r, s, Mat4::from_trs(t, r, s))
            }
            gltf::scene::Transform::Matrix { matrix } => {
                let mut m = [0.0_f64; 16];
                for col in 0..4 {
                    for row in 0..4 {
                        m[col * 4 + row] = f64::from(matrix[col][row]);
                    }
                }
                (
                    [m[12], m[13], m[14]],
                    [0.0, 0.0, 0.0, 1.0],
                    [1.0, 1.0, 1.0],
                    Mat4 { m },
                )
            }
        };
        nodes.push(Node {
            name: node.name().unwrap_or("").to_string(),
            parent: None,
            children: node.children().map(|c| c.index()).collect(),
            local,
            translation,
            rotation,
            scale,
            mesh: node.mesh().map(|m| m.index()),
        });
    }
    for parent in 0..nodes.len() {
        let children = nodes[parent].children.clone();
        for child in children {
            nodes[child].parent = Some(parent);
        }
    }

    // Skin (rung 1 models carry exactly one).
    let skin_gltf = document
        .skins()
        .next()
        .ok_or(RenderError::Missing("skin"))?;
    let ibm_accessor = skin_gltf
        .inverse_bind_matrices()
        .ok_or(RenderError::Missing("inverse bind matrices"))?;
    let ibm_flat = read_f64s(&document, &buffers, &ibm_accessor)?;
    if ibm_flat.len() % 16 != 0 {
        return Err(RenderError::Accessor {
            index: ibm_accessor.index(),
            detail: "inverse bind data not a multiple of 16".to_string(),
        });
    }
    let inverse_bind: Vec<Mat4> = ibm_flat
        .chunks_exact(16)
        .map(|c| {
            let mut m = [0.0_f64; 16];
            m.copy_from_slice(c);
            Mat4 { m }
        })
        .collect();
    let joints: Vec<usize> = skin_gltf.joints().map(|j| j.index()).collect();
    if joints.len() != inverse_bind.len() {
        return Err(RenderError::Document(format!(
            "skin has {} joints but {} inverse bind matrices",
            joints.len(),
            inverse_bind.len()
        )));
    }
    let skin = Skin {
        name: skin_gltf.name().unwrap_or("").to_string(),
        joints,
        inverse_bind,
    };

    // Meshes (flattened over primitives).
    let mut meshes = Vec::new();
    for mesh_gltf in document.meshes() {
        let mesh_node = nodes
            .iter()
            .position(|n| n.mesh == Some(mesh_gltf.index()))
            .ok_or(RenderError::Missing("mesh node"))?;
        let extras_str = mesh_gltf
            .extras()
            .as_ref()
            .map(|raw| raw.get())
            .unwrap_or("null");
        let target_names: Vec<String> = serde_json::from_str::<TargetNames>(extras_str)
            .map(|parsed| parsed.target_names)
            .unwrap_or_default();
        for (prim_index, prim) in mesh_gltf.primitives().enumerate() {
            let get = |semantic: &Semantic| prim.get(semantic);
            let positions_acc =
                get(&Semantic::Positions).ok_or(RenderError::Missing("POSITION"))?;
            let positions = read_rows::<3>(&document, &buffers, &positions_acc)?;
            let normals_acc = get(&Semantic::Normals).ok_or(RenderError::Missing("NORMAL"))?;
            let normals = read_rows::<3>(&document, &buffers, &normals_acc)?;
            let count = positions.len();
            let color0 = match get(&Semantic::Colors(0)) {
                Some(accessor) => read_colors(&document, &buffers, &accessor)?,
                None => vec![[1.0, 1.0, 1.0, 1.0]; count],
            };
            let (color1, has_color1) = match get(&Semantic::Colors(1)) {
                Some(accessor) => (read_colors(&document, &buffers, &accessor)?, true),
                None => (vec![[0.0, 0.0, 0.0, 0.0]; count], false),
            };
            let joints_acc = get(&Semantic::Joints(0)).ok_or(RenderError::Missing("JOINTS_0"))?;
            let joints = read_joints(&document, &buffers, &joints_acc)?;
            let weights_acc =
                get(&Semantic::Weights(0)).ok_or(RenderError::Missing("WEIGHTS_0"))?;
            let weights = read_rows::<4>(&document, &buffers, &weights_acc)?;
            let texcoord0 = match get(&Semantic::TexCoords(0)) {
                Some(accessor) => read_rows::<2>(&document, &buffers, &accessor)?,
                None => vec![[0.0, 0.0]; count],
            };
            let (texcoord1, has_texcoord1) = match get(&Semantic::TexCoords(1)) {
                Some(accessor) => (read_rows::<2>(&document, &buffers, &accessor)?, true),
                None => (vec![[0.0, 0.0]; count], false),
            };
            let outline_normal = read_outline_normals(
                &custom_outline,
                mesh_gltf.index(),
                prim_index,
                &document,
                &buffers,
                &prim,
                &normals,
            )?;
            let indices_acc = prim.indices().ok_or(RenderError::Missing("indices"))?;
            let indices = read_indices(&document, &buffers, &indices_acc)?;
            for (what, len) in [
                ("NORMAL", normals.len()),
                ("COLOR_0", color0.len()),
                ("COLOR_1", color1.len()),
                ("JOINTS_0", joints.len()),
                ("WEIGHTS_0", weights.len()),
                ("TEXCOORD_0", texcoord0.len()),
                ("TEXCOORD_1", texcoord1.len()),
                ("OUTLINE_NORMAL", outline_normal.len()),
            ] {
                if len != count {
                    return Err(RenderError::Document(format!(
                        "mesh {} primitive {prim_index}: {what} has {len} rows, POSITION has {count}",
                        mesh_gltf.name().unwrap_or("?")
                    )));
                }
            }
            let mut targets = Vec::new();
            for target in prim.morph_targets() {
                let stored_position_entries = target
                    .positions()
                    .and_then(|a| a.sparse())
                    .map(|s| s.count())
                    .unwrap_or(0);
                let mut by_vertex: std::collections::BTreeMap<u32, ([f32; 3], [f32; 3])> =
                    std::collections::BTreeMap::new();
                if let Some(pos_accessor) = target.positions() {
                    let dense = read_f64s(&document, &buffers, &pos_accessor)?;
                    // Re-derive the sparse support: positions not
                    // identically zero. (The decoder expanded
                    // sparse storage; zero deltas contribute
                    // nothing, so the support is the set of
                    // non-zero rows.)
                    for (v, chunk) in dense.chunks_exact(3).enumerate() {
                        if chunk.iter().any(|c| *c != 0.0) {
                            by_vertex.entry(v as u32).or_default().0 =
                                [chunk[0] as f32, chunk[1] as f32, chunk[2] as f32];
                        }
                    }
                }
                if let Some(nrm_accessor) = target.normals() {
                    let dense = read_f64s(&document, &buffers, &nrm_accessor)?;
                    for (v, chunk) in dense.chunks_exact(3).enumerate() {
                        if chunk.iter().any(|c| *c != 0.0) {
                            by_vertex.entry(v as u32).or_default().1 =
                                [chunk[0] as f32, chunk[1] as f32, chunk[2] as f32];
                        }
                    }
                }
                let entries = by_vertex
                    .into_iter()
                    .map(|(v, (dp, dn))| (v, dp, dn))
                    .collect();
                targets.push(MorphTarget {
                    entries,
                    stored_position_entries,
                });
            }
            if !targets.is_empty() && target_names.len() != targets.len() {
                return Err(RenderError::Document(format!(
                    "mesh {}: {} morph targets but {} names",
                    mesh_gltf.name().unwrap_or("?"),
                    targets.len(),
                    target_names.len()
                )));
            }
            let default_weights = mesh_gltf
                .weights()
                .map(<[f32]>::to_vec)
                .unwrap_or_else(|| vec![0.0; targets.len()]);
            meshes.push(Mesh {
                name: if prim_index == 0 {
                    mesh_gltf.name().unwrap_or("").to_string()
                } else {
                    format!("{}#{prim_index}", mesh_gltf.name().unwrap_or(""))
                },
                node: mesh_node,
                material: prim.material().index().unwrap_or(0),
                positions,
                normals,
                color0,
                color1,
                has_color1,
                texcoord0,
                texcoord1,
                has_texcoord1,
                outline_normal,
                joints,
                weights,
                indices,
                targets,
                target_names: target_names.clone(),
                default_weights,
            });
        }
    }

    Ok(Model {
        meshes,
        nodes,
        skin,
        materials,
        albedos,
    })
}

/// Convert one decoded GLB image to the renderer's RGBA8
/// texture form. Rows pass through in decoded order: row 0
/// is the image's top row, which is glTF v = 0 — the same
/// order the backend uploads (the Family B proving test
/// pins this orientation empirically). 8-bit formats only;
/// wider formats are a precise error.
fn image_to_rgba(data: &gltf::image::Data) -> Result<ToonImage, RenderError> {
    use gltf::image::Format;
    let px = (data.width as usize) * (data.height as usize);
    let mut rgba = Vec::with_capacity(px * 4);
    match data.format {
        Format::R8G8B8A8 => rgba.extend_from_slice(&data.pixels),
        Format::R8G8B8 => {
            for rgb in data.pixels.chunks_exact(3) {
                rgba.extend_from_slice(rgb);
                rgba.push(255);
            }
        }
        Format::R8 => {
            for v in &data.pixels {
                rgba.extend_from_slice(&[*v, *v, *v, 255]);
            }
        }
        other => {
            return Err(RenderError::Document(format!(
                "base-colour image format {other:?} is not an 8-bit format; \
                 Family B v1 accepts R8 / R8G8B8 / R8G8B8A8"
            )));
        }
    }
    ToonImage::new(data.width, data.height, rgba)
}

/// Resolve the materials' base-colour textures (Family B
/// v2): the estate's authoring route paints one atlas PER
/// PART GROUP (face, hair, ...), so a character legitimately
/// samples several images. Decode each distinct image once,
/// in first-use order, and record every material's slot.
/// An out-of-range index or a non-8-bit image is a precise
/// error, never a silent fallback.
fn resolve_albedos(
    materials: &mut [Material],
    images: &[gltf::image::Data],
) -> Result<Vec<ToonImage>, RenderError> {
    let mut order: Vec<usize> = Vec::new();
    let mut albedos: Vec<ToonImage> = Vec::new();
    for material in materials.iter_mut() {
        let Some(index) = material.base_color_image else {
            continue;
        };
        let slot = match order.iter().position(|&i| i == index) {
            Some(slot) => slot,
            None => {
                let data = images.get(index).ok_or(RenderError::Document(format!(
                    "base-colour image index {index} out of range ({} images decoded)",
                    images.len()
                )))?;
                albedos.push(image_to_rgba(data)?);
                order.push(index);
                order.len() - 1
            }
        };
        material.albedo_slot = Some(slot);
    }
    Ok(albedos)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Adversarial: garbage bytes are a typed parse error,
    // never a panic.
    #[test]
    fn garbage_bytes_error_not_panic() {
        assert!(matches!(
            load_model(b"not a glb at all"),
            Err(RenderError::GltfParse(_))
        ));
        assert!(load_model(b"").is_err());
    }

    fn image(
        format: gltf::image::Format,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> gltf::image::Data {
        gltf::image::Data {
            format,
            width,
            height,
            pixels,
        }
    }

    fn material(name: &str, base_color_image: Option<usize>) -> Material {
        Material {
            name: name.to_string(),
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            double_sided: false,
            base_color_image,
            albedo_slot: None,
        }
    }

    // Family B: the 8-bit conversions land byte-exact.
    #[test]
    fn image_conversion_formats() {
        use gltf::image::Format;
        let rgba = image_to_rgba(&image(Format::R8G8B8A8, 1, 1, vec![9, 8, 7, 6])).unwrap();
        assert_eq!(rgba.rgba, vec![9, 8, 7, 6]);
        let rgb = image_to_rgba(&image(Format::R8G8B8, 1, 1, vec![9, 8, 7])).unwrap();
        assert_eq!(rgb.rgba, vec![9, 8, 7, 255]);
        let gray = image_to_rgba(&image(Format::R8, 2, 1, vec![5, 6])).unwrap();
        assert_eq!(gray.rgba, vec![5, 5, 5, 255, 6, 6, 6, 255]);
        // A wide format is a precise error, never a panic
        // or a silent truncation.
        let wide = image_to_rgba(&image(Format::R16G16B16A16, 1, 1, vec![0; 8]));
        assert!(matches!(wide, Err(RenderError::Document(_))));
    }

    // Family B v2: per-material images — none, one shared,
    // and two distinct images landing in first-use slots.
    #[test]
    fn resolve_albedos_rule() {
        use gltf::image::Format;
        let images = vec![
            image(Format::R8G8B8A8, 1, 1, vec![255, 0, 0, 255]),
            image(Format::R8G8B8A8, 1, 1, vec![0, 0, 255, 255]),
        ];
        // No textured materials: no albedos, no slots.
        let mut plain = vec![material("a", None), material("b", None)];
        let none = resolve_albedos(&mut plain, &images).unwrap();
        assert!(none.is_empty());
        assert!(plain.iter().all(|m| m.albedo_slot.is_none()));
        // Two materials, one shared image: one albedo, both
        // materials in slot 0.
        let mut shared = vec![material("a", Some(1)), material("b", Some(1))];
        let one = resolve_albedos(&mut shared, &images).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].rgba, vec![0, 0, 255, 255]);
        assert!(shared.iter().all(|m| m.albedo_slot == Some(0)));
        // Two materials, different images: two albedos in
        // first-use order, slots follow their materials.
        let mut divergent = vec![material("a", Some(0)), material("b", Some(1))];
        let two = resolve_albedos(&mut divergent, &images).unwrap();
        assert_eq!(two.len(), 2);
        assert_eq!(divergent[0].albedo_slot, Some(0));
        assert_eq!(divergent[1].albedo_slot, Some(1));
        // An out-of-range image index: refusal.
        let mut dangling = vec![material("a", Some(7))];
        let err = resolve_albedos(&mut dangling, &images);
        assert!(matches!(err, Err(RenderError::Document(_))));
    }
}
