//! The backend boundary. Everything the API-agnostic
//! layers produce is packed into the plain-data packets of
//! this module; a backend consumes packets and returns
//! pixels. No GPU handle types appear in the packets, and
//! no scene/pose/shading logic lives below the boundary —
//! so a second backend (the wgpu path the analysis
//! originally sketched) could implement [`RenderBackend`]
//! as a swap rather than a rewrite. Rung 1 ships the
//! Vulkan backend only (D-1217).
//!
//! Boundary cost, as commissioned to report: the packets
//! cost one packing pass over data the loader already
//! holds in near-identical shape, and the trait is three
//! methods. The one Vulkan-leaning seam above the boundary
//! is the shader binding layout (set/binding numbers in
//! the GLSL sources) — SPIR-V is the shared currency, and
//! a wgpu backend would remap bindings at its own edge.

pub mod vulkan;

use crate::error::RenderError;
use crate::math::{Mat4, V3};
use crate::model::Model;
use crate::morph::PackedMorphs;
use crate::setscene::{ArcScene, projection};
use crate::splat::PackedSplats;
use crate::toon::{ToonData, ToonImage};

/// One character mesh, upload-ready.
#[derive(Debug, Clone)]
pub struct PackedMesh {
    pub positions: Vec<[f32; 4]>,
    pub normals: Vec<[f32; 4]>,
    /// The PAINT set fed to the shader's albedo: COLOR_1
    /// when the mesh carries it, else COLOR_0. (Measured
    /// rung 1: on Prima's GLBs COLOR_0 is constant white
    /// and COLOR_1 is the paint — the builder baked even
    /// material base colours into it; see the proving
    /// test `color1_evidence`.)
    pub color0: Vec<[f32; 4]>,
    pub color1: Vec<[f32; 4]>,
    /// UV sets packed for the toon model: xy = TEXCOORD_0
    /// (face SDF space), zw = TEXCOORD_1 (packed-lightmap
    /// space). Zeros when the mesh carries no UVs.
    pub uv: Vec<[f32; 4]>,
    /// Planar UV set (GAP-4): xy = TEXCOORD_2, z = 1
    /// when the mesh carried TEXCOORD_2 else 0, w = 0.
    pub planar: Vec<[f32; 4]>,
    /// Shading (toon) normal set (GAP-8): TEXCOORD_3
    /// verbatim (xyz = bind-space direction, w =
    /// authored flag). All-zero when absent.
    pub toon_normal: Vec<[f32; 4]>,
    /// Outline rows (Amendment A4): xyz = outline normal
    /// (bind space), w = outline width from the RAW
    /// COLOR_0 alpha — the width channel lives in COLOR_0
    /// regardless of which colour set is the paint.
    pub outline: Vec<[f32; 4]>,
    /// True when the paint came from COLOR_1 (the
    /// material base-colour factor is then already baked
    /// into the paint and must not multiply again).
    pub paint_from_color1: bool,
    /// Joint indices widened to u32 (the shader reads
    /// uvec4).
    pub joints: Vec<[u32; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub morph_ranges: Vec<[u32; 2]>,
    pub morph_entry_target: Vec<u32>,
    pub morph_entry_dpos: Vec<[f32; 4]>,
    pub morph_entry_dnrm: Vec<[f32; 4]>,
    /// Index into `PackedScene::materials`.
    pub material: usize,
    pub vertex_count: u32,
    /// Morph target count (the per-frame weight array's
    /// length for this mesh).
    pub target_count: u32,
}

/// One set piece, upload-ready.
#[derive(Debug, Clone)]
pub struct PackedSetPiece {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Draw data as uploaded: model matrix, normal-matrix
    /// columns, albedo/roughness, emission/strength.
    pub model: [f32; 16],
    pub normal_cols: [[f32; 4]; 3],
    pub albedo_rough: [f32; 4],
    pub emission: [f32; 4],
    pub is_toy: bool,
    pub visible_from: u32,
    pub visible_until: u32,
}

impl PackedSetPiece {
    pub fn visible(&self, frame: u32) -> bool {
        (self.visible_from..=self.visible_until).contains(&frame)
    }
}

/// A flat material (mirrors `model::Material`).
#[derive(Debug, Clone, Copy)]
pub struct PackedMaterial {
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Slot into `PackedScene::albedos` (Family B v2).
    pub albedo_slot: Option<usize>,
    /// Alpha-overlay blend class (the material's glTF
    /// `alphaMode` is BLEND): the backend draws this
    /// material's meshes in the alpha-overlay pass.
    pub alpha_overlay: bool,
}

/// The whole scene as packets.
#[derive(Debug, Clone)]
pub struct PackedScene {
    pub meshes: Vec<PackedMesh>,
    pub materials: Vec<PackedMaterial>,
    pub set_pieces: Vec<PackedSetPiece>,
    /// Column-major view-projection.
    pub view_proj: [f32; 16],
    /// Column-major view matrix (the toon model's sphere
    /// map samples by view-space normal).
    pub view: [f32; 16],
    /// Toon shading data (gap G1) when the scene shades
    /// with the toon model; `None` keeps the legacy
    /// half-Lambert + GGX path bit-identical. `pack_scene`
    /// leaves this `None`; a toon-aware caller fills it.
    pub toon: Option<ToonData>,
    /// The model's base-colour textures (Family B v2),
    /// carried from the decoded GLB by `pack_scene`. Each
    /// mesh's material selects its texture through
    /// `PackedMaterial::albedo_slot`; the character shader
    /// multiplies it into the albedo (the glTF base-colour
    /// rule) on both shading paths.
    pub albedos: Vec<ToonImage>,
    /// A Gaussian splat layer (environment/reference
    /// class), already projected and sorted for this
    /// scene's camera by `splat::pack_splats`. `None`
    /// keeps the render bit-identical to the pre-splat
    /// renderer; `pack_scene` leaves this `None` and a
    /// splat-aware caller fills it.
    pub splats: Option<PackedSplats>,
    /// A Gaussian splat layer carried as resident
    /// data for the GPU projection path (DESIGN.md
    /// §6.1): the backend projects and sorts it
    /// on-device per frame from this packet plus the
    /// per-frame transform/sequence state set through
    /// the trait below. Independent of `splats`: a
    /// scene may carry either packet, and the proving
    /// battery carries both (in separate uploads) to
    /// gate one against the other.
    pub splat_gpu: Option<crate::splat_gpu::PackedGpuSplats>,
    /// Direction to the sun + energy in w.
    pub sun: [f32; 4],
    /// Direction to the fill + energy in w.
    pub fill: [f32; 4],
    /// World (ambient) colour.
    pub world: [f32; 4],
    /// Camera position, world space.
    pub camera_pos: [f32; 4],
    /// Background clear colour — the world colour through
    /// the display transform (what the plates show where
    /// no geometry covers the world).
    pub clear_rgba: [f32; 4],
    /// Joint count of the model's skin.
    pub joint_count: usize,
}

/// Per-frame draw packet.
#[derive(Debug, Clone)]
pub struct DrawFrame {
    pub frame: u32,
    /// Joint matrices (world * inverse-bind), skin order,
    /// column-major f32.
    pub joint_matrices: Vec<[f32; 16]>,
    /// Morph weights per mesh, in packed-mesh order.
    pub mesh_weights: Vec<Vec<f32>>,
    /// Which character meshes draw this frame.
    pub visible_meshes: Vec<bool>,
    /// Whether the set draws (false for the
    /// character-mask renders used by the comparison).
    pub include_set: bool,
    /// The Toy's model matrix this frame (glTF space).
    pub toy_model: Mat4,
}

/// One rendered frame, read back: RGBA8, top row first.
#[derive(Debug, Clone)]
pub struct RenderedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The boundary every backend implements.
pub trait RenderBackend {
    /// Human-readable device identification for the run
    /// manifest (determinism is per machine/adapter, R6).
    fn describe(&self) -> String;

    /// Upload a packed scene and size the offscreen
    /// targets (`width` x `height`).
    fn upload_scene(
        &mut self,
        scene: &PackedScene,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError>;

    /// Render one frame offscreen and read it back.
    fn render_frame(&mut self, draw: &DrawFrame) -> Result<RenderedFrame, RenderError>;

    /// Set the GPU splat layer's transform for the
    /// next frames (Rung 5 animation: the frozen
    /// spec's motion keys, evaluated per frame by the
    /// caller). The resident records do not move;
    /// only the parameter block is rewritten. Backends
    /// without the GPU splat path (or scenes without
    /// the layer) reject it.
    fn set_splat_transform(
        &mut self,
        _transform: crate::splat::SplatTransform,
    ) -> Result<(), RenderError> {
        Err(RenderError::Backend(
            "this backend has no GPU splat layer to transform".to_string(),
        ))
    }

    /// Select which resident sequence frame the GPU
    /// splat layer projects next (Rung 5). Frame 0 is
    /// the layer's still.
    fn set_splat_sequence_frame(&mut self, _frame: usize) -> Result<(), RenderError> {
        Err(RenderError::Backend(
            "this backend has no GPU splat sequence to select".to_string(),
        ))
    }

    /// Run the GPU splat projection + sort for the
    /// current layer state and read the surviving
    /// projected records back, in draw order: the
    /// determinism gate's instrument (the records are
    /// compared against `splat::project_splats`).
    fn splat_gpu_snapshot(&mut self) -> Result<crate::splat_gpu::SplatGpuSnapshot, RenderError> {
        Err(RenderError::Backend(
            "this backend has no GPU splat layer to snapshot".to_string(),
        ))
    }

    /// Run the skinning path for one mesh and read the
    /// skinned positions/normals back (the cross-check's
    /// GPU side).
    fn skin_positions(
        &mut self,
        mesh_index: usize,
        joint_matrices: &[[f32; 16]],
        weights: &[f32],
    ) -> Result<SkinnedMesh, RenderError>;
}

/// One mesh's skinned output: (positions, normals), one
/// `[f32; 3]` per vertex.
pub type SkinnedMesh = (Vec<[f32; 3]>, Vec<[f32; 3]>);

/// Normal-matrix columns (inverse-transpose of the model
/// 3x3, as f32 vec4s) — for a 3x3, the inverse-transpose is
/// the cofactor matrix over the determinant.
fn normal_cols(model: Mat4) -> [[f32; 4]; 3] {
    let m = &model.m;
    // 3x3 entries a[row][col] from the 4x4's upper corner.
    let a = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]];
    let cof = [
        [
            a[1][1] * a[2][2] - a[1][2] * a[2][1],
            a[1][2] * a[2][0] - a[1][0] * a[2][2],
            a[1][0] * a[2][1] - a[1][1] * a[2][0],
        ],
        [
            a[0][2] * a[2][1] - a[0][1] * a[2][2],
            a[0][0] * a[2][2] - a[0][2] * a[2][0],
            a[0][1] * a[2][0] - a[0][0] * a[2][1],
        ],
        [
            a[0][1] * a[1][2] - a[0][2] * a[1][1],
            a[0][2] * a[1][0] - a[0][0] * a[1][2],
            a[0][0] * a[1][1] - a[0][1] * a[1][0],
        ],
    ];
    let det = a[0][0] * cof[0][0] + a[0][1] * cof[0][1] + a[0][2] * cof[0][2];
    let inv_det = if det.abs() > 1e-15 { 1.0 / det } else { 0.0 };
    // Columns of the cofactor matrix / det.
    let mut cols = [[0.0_f32; 4]; 3];
    for col in 0..3 {
        for row in 0..3 {
            cols[col][row] = (cof[row][col] * inv_det) as f32;
        }
        cols[col][3] = 0.0;
    }
    cols
}

/// Recompute a packed set piece's draw data for a new
/// model matrix (the Toy, per frame).
pub fn piece_draw_data(
    model: Mat4,
    albedo: [f32; 3],
    roughness: f32,
    emission: [f32; 3],
    emission_strength: f32,
) -> ([f32; 16], [[f32; 4]; 3], [f32; 4], [f32; 4]) {
    (
        model.to_f32_cols(),
        normal_cols(model),
        [albedo[0], albedo[1], albedo[2], roughness],
        [emission[0], emission[1], emission[2], emission_strength],
    )
}

/// Pack the decoded model + arc scene into boundary
/// packets.
pub fn pack_scene(model: &Model, scene: &ArcScene) -> PackedScene {
    let mut meshes = Vec::with_capacity(model.meshes.len());
    for mesh in &model.meshes {
        let packed = PackedMorphs::pack(mesh);
        let widen = |rows: &[[f32; 3]]| -> Vec<[f32; 4]> {
            rows.iter().map(|v| [v[0], v[1], v[2], 0.0]).collect()
        };
        meshes.push(PackedMesh {
            positions: widen(&mesh.positions),
            normals: widen(&mesh.normals),
            color0: if mesh.has_color1 {
                mesh.color1.clone()
            } else {
                mesh.color0.clone()
            },
            color1: mesh.color1.clone(),
            uv: mesh
                .texcoord0
                .iter()
                .zip(&mesh.texcoord1)
                .map(|(uv0, uv1)| [uv0[0], uv0[1], uv1[0], uv1[1]])
                .collect(),
            planar: mesh
                .texcoord2
                .iter()
                .map(|uv2| {
                    [
                        uv2[0],
                        uv2[1],
                        if mesh.has_texcoord2 { 1.0 } else { 0.0 },
                        0.0,
                    ]
                })
                .collect(),
            toon_normal: mesh.toon_normal.clone(),
            outline: mesh
                .outline_normal
                .iter()
                .zip(&mesh.color0)
                .map(|(n, c)| [n[0], n[1], n[2], c[3]])
                .collect(),
            paint_from_color1: mesh.has_color1,
            joints: mesh
                .joints
                .iter()
                .map(|j| {
                    [
                        u32::from(j[0]),
                        u32::from(j[1]),
                        u32::from(j[2]),
                        u32::from(j[3]),
                    ]
                })
                .collect(),
            weights: mesh.weights.clone(),
            indices: mesh.indices.clone(),
            morph_ranges: packed.ranges,
            morph_entry_target: packed.entry_target,
            morph_entry_dpos: packed.entry_dpos,
            morph_entry_dnrm: packed.entry_dnrm,
            material: mesh.material,
            vertex_count: mesh.vertex_count() as u32,
            target_count: mesh.targets.len() as u32,
        });
    }
    let materials = model
        .materials
        .iter()
        .map(|m| PackedMaterial {
            base_color: m.base_color,
            metallic: m.metallic,
            roughness: m.roughness,
            albedo_slot: m.albedo_slot,
            alpha_overlay: m.alpha_overlay,
        })
        .collect();
    let set_pieces = scene
        .pieces
        .iter()
        .map(|piece| {
            let (model_m, cols, albedo_rough, emission) = piece_draw_data(
                piece.model,
                piece.material.albedo,
                piece.material.roughness,
                piece.material.emission,
                piece.material.emission_strength,
            );
            PackedSetPiece {
                positions: piece.positions.clone(),
                normals: piece.normals.clone(),
                indices: piece.indices.clone(),
                model: model_m,
                normal_cols: cols,
                albedo_rough,
                emission,
                is_toy: piece.is_toy,
                visible_from: piece.visible_from,
                visible_until: piece.visible_until,
            }
        })
        .collect();
    let view_proj = projection(scene).multiply(scene.view).to_f32_cols();
    let camera_world = scene.view.inverse();
    let camera_pos = camera_world.translation();
    let clear = crate::setscene::display_transform_mirror(scene.world);
    PackedScene {
        meshes,
        materials,
        set_pieces,
        view_proj,
        view: scene.view.to_f32_cols(),
        toon: None,
        albedos: model.albedos.clone(),
        splats: None,
        splat_gpu: None,
        sun: [
            scene.sun.to_light.x as f32,
            scene.sun.to_light.y as f32,
            scene.sun.to_light.z as f32,
            scene.sun.energy,
        ],
        fill: [
            scene.fill.to_light.x as f32,
            scene.fill.to_light.y as f32,
            scene.fill.to_light.z as f32,
            scene.fill.energy,
        ],
        world: [scene.world[0], scene.world[1], scene.world[2], 1.0],
        camera_pos: [
            camera_pos.x as f32,
            camera_pos.y as f32,
            camera_pos.z as f32,
            1.0,
        ],
        clear_rgba: [clear[0], clear[1], clear[2], 1.0],
        joint_count: model.skin.joints.len(),
    }
}

/// Directional-light view-projection for the shadow pass:
/// an orthographic frustum fitted to the arc set
/// (half-extent 2.6 m around (0, 0.85, 0)) — a backend
/// concern, but the fitting constants live here so every
/// backend shares them.
pub fn light_view_proj(sun_to_light: V3) -> Mat4 {
    let centre = V3::new(0.0, 0.85, 0.0);
    let eye = centre.plus(sun_to_light.scale(6.0));
    let up = if sun_to_light.y.abs() > 0.9 {
        V3::new(1.0, 0.0, 0.0)
    } else {
        V3::new(0.0, 1.0, 0.0)
    };
    let forward = centre.minus(eye).normalized();
    let right = V3::new(
        forward.y * up.z - forward.z * up.y,
        forward.z * up.x - forward.x * up.z,
        forward.x * up.y - forward.y * up.x,
    )
    .normalized();
    let true_up = V3::new(
        right.y * forward.z - right.z * forward.y,
        right.z * forward.x - right.x * forward.z,
        right.x * forward.y - right.y * forward.x,
    );
    let view = Mat4::from_rows([
        [right.x, right.y, right.z, -right.dot(eye)],
        [true_up.x, true_up.y, true_up.z, -true_up.dot(eye)],
        [-forward.x, -forward.y, -forward.z, forward.dot(eye)],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let (extent, near, far) = (2.6_f64, 1.0_f64, 12.0_f64);
    let mut ortho = [0.0_f64; 16];
    ortho[0] = 1.0 / extent;
    ortho[5] = 1.0 / extent;
    ortho[10] = 1.0 / (near - far);
    ortho[14] = near / (near - far);
    ortho[15] = 1.0;
    Mat4 { m: ortho }.multiply(view)
}
