//! The Prima arc's set, lights and camera, transcribed
//! from the arc renderer of record (`build/anim.py`) and
//! cross-checked against an evaluated-scene probe dumped
//! from Blender (scene-probe.json / frustum.json, staged
//! with the rung's inputs; the crate's tests hold the two
//! in agreement).
//!
//! Everything here is glTF-space by the time it leaves the
//! module: Blender `(x, y, z)` becomes glTF `(x, z, -y)`
//! (a proper rotation, determinant +1), and Blender world
//! matrices are conjugated `C * M * C^-1`.
//!
//! The plates are 512x640 (measured from the PNG headers of
//! `frames-canon-m100/`), rendered by EEVEE with AgX, world
//! (0.16, 0.15, 0.19), Sun 3.0 and Fill 1.1, both shadow-
//! casting (probe: `use_shadow = true` on both; gap
//! analysis item G10 resolved for the arc plates by that
//! evidence).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::RenderError;
use crate::math::{Mat4, Quat, V3};

/// Plate width in pixels (measured).
pub const PLATE_WIDTH: u32 = 512;
/// Plate height in pixels (measured).
pub const PLATE_HEIGHT: u32 = 640;
/// Plate frames in the arc.
pub const PLATE_FRAMES: u32 = 360;

/// Blender -> glTF axis map for a point or direction.
pub fn blender_to_gltf(v: V3) -> V3 {
    V3::new(v.x, v.z, -v.y)
}

/// The axis-map matrix C (glTF = C * blender).
pub fn axis_map() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// Convert the CAMERA's Blender world matrix into glTF
/// space: left-multiply by the axis map. A camera's local
/// frame is a fixed semantic convention in both worlds
/// (back = +Z, up = +Y), so the local basis is NOT
/// re-expressed — conjugating would swap the local Y/Z
/// basis and point the camera along its own up axis (the
/// rung-1 frozen-frame bug: every frame cleared to the
/// world colour, nothing in frustum).
pub fn convert_matrix(m: Mat4) -> Mat4 {
    axis_map().multiply(m)
}

/// Convert a set piece's Blender MODEL matrix into glTF
/// space: conjugate by the axis map. Piece geometry is
/// itself converted vertex-by-vertex through the same
/// map (`convert_geometry`), so the local basis IS
/// re-expressed and the full change of basis applies:
/// `A * M * A^-1` (rotation conjugated, translation
/// mapped, scale preserved).
pub fn convert_model_matrix(m: Mat4) -> Mat4 {
    let c = axis_map();
    c.multiply(m).multiply(c.inverse())
}

/// A Blender XYZ euler (radians) as a quaternion, in
/// Blender's application order (R = Rz * Ry * Rx).
pub fn euler_xyz(e: [f64; 3]) -> Quat {
    let qx = Quat::from_axis_angle(V3::new(1.0, 0.0, 0.0), e[0]);
    let qy = Quat::from_axis_angle(V3::new(0.0, 1.0, 0.0), e[1]);
    let qz = Quat::from_axis_angle(V3::new(0.0, 0.0, 1.0), e[2]);
    qz.multiply(qy).multiply(qx)
}

/// A directional light, glTF space.
#[derive(Debug, Clone, Copy)]
pub struct DirectionalLight {
    /// Unit direction from the surface toward the light.
    pub to_light: V3,
    /// Blender energy (radiance scale of the shading model).
    pub energy: f32,
}

fn sun_light(euler: [f64; 3], energy: f32) -> DirectionalLight {
    // Blender suns travel along their local -Z; the
    // direction toward the light is R * (0,0,1).
    let to_light_blender = euler_xyz(euler).rotate(V3::new(0.0, 0.0, 1.0));
    DirectionalLight {
        to_light: blender_to_gltf(to_light_blender).normalized(),
        energy,
    }
}

/// Set material (linear-space colours, as Blender's
/// script-set Principled defaults store them).
#[derive(Debug, Clone, Copy)]
pub struct SetMaterial {
    pub albedo: [f32; 3],
    pub roughness: f32,
    pub emission: [f32; 3],
    pub emission_strength: f32,
}

const MAT_FLOOR: SetMaterial = SetMaterial {
    albedo: [0.10, 0.09, 0.12],
    roughness: 0.85,
    emission: [0.0; 3],
    emission_strength: 0.0,
};
const MAT_COUCH: SetMaterial = SetMaterial {
    albedo: [0.23, 0.12, 0.24],
    roughness: 0.85,
    emission: [0.0; 3],
    emission_strength: 0.0,
};
const MAT_SCREEN: SetMaterial = SetMaterial {
    albedo: [0.10, 0.02, 0.10],
    roughness: 0.85,
    emission: [1.0, 0.15, 0.55],
    emission_strength: 1.6,
};
const MAT_RING: SetMaterial = SetMaterial {
    albedo: [0.2, 0.02, 0.1],
    roughness: 0.85,
    emission: [1.0, 0.35, 0.75],
    emission_strength: 2.2,
};
const MAT_METAL: SetMaterial = SetMaterial {
    albedo: [0.45, 0.45, 0.5],
    roughness: 0.35,
    emission: [0.0; 3],
    emission_strength: 0.0,
};
const MAT_PINK: SetMaterial = SetMaterial {
    albedo: [0.85, 0.30, 0.50],
    roughness: 0.4,
    emission: [0.0; 3],
    emission_strength: 0.0,
};

/// One set piece: local geometry (already axis-converted to
/// glTF), a model matrix (glTF space), its material, and the
/// frame window in which it renders.
#[derive(Debug, Clone)]
pub struct SetPiece {
    pub name: &'static str,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub material: SetMaterial,
    /// Model matrix for static pieces; for the Toy this is
    /// frame 0's matrix and the per-frame matrices come
    /// from `ProbeData`.
    pub model: Mat4,
    pub is_toy: bool,
    /// First visible plate frame (inclusive).
    pub visible_from: u32,
    /// Last visible plate frame (inclusive).
    pub visible_until: u32,
}

impl SetPiece {
    pub fn visible(&self, frame: u32) -> bool {
        (self.visible_from..=self.visible_until).contains(&frame)
    }
}

/// The two outfit meshes alternate on the show's plan
/// (`outfit_plan = [[0, 1], [90, 2]]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutfitSelection {
    /// `Prima_Outfit` (plan index 1): frames 0..=89.
    Outfit,
    /// `Prima_OutfitEnd` (plan index 2): frames 90..=359.
    OutfitEnd,
}

pub fn outfit_for_frame(frame: u32) -> OutfitSelection {
    if frame < 90 {
        OutfitSelection::Outfit
    } else {
        OutfitSelection::OutfitEnd
    }
}

/// The full arc scene.
#[derive(Debug, Clone)]
pub struct ArcScene {
    pub world: [f32; 3],
    pub sun: DirectionalLight,
    pub fill: DirectionalLight,
    /// World -> camera view matrix (glTF space).
    pub view: Mat4,
    /// tan(fov/2) horizontally / vertically, measured from
    /// the probe's `view_frame` corners (0.4 / 1.4444 and
    /// 0.5 / 1.4444 — lens 52 mm, sensor fit resolved by
    /// Blender; the corners are the ground truth).
    pub tan_half_x: f64,
    pub tan_half_y: f64,
    pub near: f64,
    pub far: f64,
    pub pieces: Vec<SetPiece>,
}

/// The evaluated-scene probe dump (scene-probe.json):
/// per-frame Toy world matrices and set visibility, plus
/// the static camera matrices — the ground truth the
/// transcriptions in this module are tested against.
#[derive(Debug, Clone)]
pub struct ProbeData {
    /// Toy world matrices (glTF space), per plate frame.
    pub toy_matrices: Vec<Mat4>,
    /// Visibility per frame for the probed object names.
    pub visibility: Vec<BTreeMap<String, bool>>,
    /// The camera's world matrix (glTF space); the probe
    /// verified it is identical on every sampled frame.
    pub camera_world: Mat4,
}

#[derive(Deserialize)]
struct ProbeJson {
    /// Keyed by frame number as a string (the probe script
    /// dumped a dict).
    toy_matrices: BTreeMap<String, [[f64; 4]; 4]>,
    visibility: BTreeMap<String, BTreeMap<String, bool>>,
    camera_matrices: BTreeMap<String, [[f64; 4]; 4]>,
}

impl ProbeData {
    pub fn from_bytes(bytes: &[u8]) -> Result<ProbeData, RenderError> {
        let parsed: ProbeJson = serde_json::from_slice(bytes)
            .map_err(|e| RenderError::Document(format!("scene probe: {e}")))?;
        if parsed.toy_matrices.len() != PLATE_FRAMES as usize {
            return Err(RenderError::Document(format!(
                "probe carries {} toy matrices, expected {PLATE_FRAMES}",
                parsed.toy_matrices.len()
            )));
        }
        let mut toy_matrices = Vec::with_capacity(PLATE_FRAMES as usize);
        for frame in 0..PLATE_FRAMES {
            let rows = parsed.toy_matrices.get(&frame.to_string()).ok_or_else(|| {
                RenderError::Document(format!("probe toy matrix {frame} missing"))
            })?;
            toy_matrices.push(convert_model_matrix(Mat4::from_rows(*rows)));
        }
        let mut visibility = Vec::with_capacity(PLATE_FRAMES as usize);
        for frame in 0..PLATE_FRAMES {
            let entry = parsed.visibility.get(&frame.to_string()).ok_or_else(|| {
                RenderError::Document(format!("probe visibility {frame} missing"))
            })?;
            visibility.push(entry.clone());
        }
        let camera_rows = parsed
            .camera_matrices
            .get("0")
            .ok_or(RenderError::Missing("probe camera matrix"))?;
        Ok(ProbeData {
            toy_matrices,
            visibility,
            camera_world: convert_matrix(Mat4::from_rows(*camera_rows)),
        })
    }
}

fn piece_matrix(location: [f64; 3], euler: [f64; 3], scale: [f64; 3]) -> Mat4 {
    let q = euler_xyz(euler);
    let blender = Mat4::from_trs(location, [q.x, q.y, q.z, q.w], scale);
    convert_model_matrix(blender)
}

fn convert_geometry(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let p = positions
        .iter()
        .map(|v| blender_to_gltf(V3::from_f32(*v)).to_f32())
        .collect();
    let n = normals
        .iter()
        .map(|v| blender_to_gltf(V3::from_f32(*v)).normalized().to_f32())
        .collect();
    (p, n)
}

/// Build the arc scene. The camera comes from the probe
/// (measured in the evaluated scene); everything else is
/// transcribed from anim.py and held against the probe by
/// the crate's tests.
pub fn prima_arc_scene(probe: &ProbeData) -> ArcScene {
    let deg = std::f64::consts::PI / 180.0;
    let mut pieces = Vec::new();
    let all = (0, PLATE_FRAMES - 1);

    // Floor (anim.py: box at (0, 0.3, -0.02), scale (4,4,0.02)).
    let (p, n) = convert_geometry(&cube_positions(), &cube_normals());
    pieces.push(SetPiece {
        name: "Floor",
        positions: p,
        normals: n,
        indices: cube_indices(),
        material: MAT_FLOOR,
        model: piece_matrix([0.0, 0.3, -0.02], [0.0; 3], [4.0, 4.0, 0.02]),
        is_toy: false,
        visible_from: all.0,
        visible_until: all.1,
    });
    // Couch base + back.
    for (name, loc, scale) in [
        ("CouchBase", [0.0, 0.15, 0.14], [0.85, 1.15, 0.14]),
        ("CouchBack", [0.0, 1.22, 0.30], [0.85, 0.16, 0.30]),
    ] {
        let (p, n) = convert_geometry(&cube_positions(), &cube_normals());
        pieces.push(SetPiece {
            name,
            positions: p,
            normals: n,
            indices: cube_indices(),
            material: MAT_COUCH,
            model: piece_matrix(loc, [0.0; 3], scale),
            is_toy: false,
            visible_from: all.0,
            visible_until: all.1,
        });
    }
    // Hypno screen (frames 0..=89).
    let (p, n) = convert_geometry(&cube_positions(), &cube_normals());
    pieces.push(SetPiece {
        name: "HypnoScreen",
        positions: p,
        normals: n,
        indices: cube_indices(),
        material: MAT_SCREEN,
        model: piece_matrix([0.0, 1.30, 1.02], [0.0; 3], [0.68, 0.02, 0.44]),
        is_toy: false,
        visible_from: 0,
        visible_until: 89,
    });
    // Hypno rings (frames 0..=89).
    for (i, major) in [0.10_f64, 0.19, 0.28].iter().enumerate() {
        let (pos, nrm, idx) = torus(*major, 0.008, 48, 12);
        let (p, n) = convert_geometry(&pos, &nrm);
        pieces.push(SetPiece {
            name: match i {
                0 => "HypnoRing0",
                1 => "HypnoRing1",
                _ => "HypnoRing2",
            },
            positions: p,
            normals: n,
            indices: idx,
            material: MAT_RING,
            model: piece_matrix([0.0, 1.27, 1.02], [90.0 * deg, 0.0, 0.0], [1.0, 1.0, 1.0]),
            is_toy: false,
            visible_from: 0,
            visible_until: 89,
        });
    }
    // Spreader bar (frames 90..=269).
    let (pos, nrm, idx) = cylinder(0.016, 0.62, 32);
    let (p, n) = convert_geometry(&pos, &nrm);
    pieces.push(SetPiece {
        name: "SpreaderBar",
        positions: p,
        normals: n,
        indices: idx,
        material: MAT_METAL,
        model: piece_matrix([0.0, -0.46, 0.30], [0.0, 90.0 * deg, 0.0], [1.0, 1.0, 1.0]),
        is_toy: false,
        visible_from: 90,
        visible_until: 269,
    });
    // Toy (all frames; per-frame matrix from the probe —
    // Blender's bezier-interpolated keyframes are evaluated
    // data, not something this module re-derives).
    let (pos, nrm, idx) = uv_sphere(32, 16);
    let (p, n) = convert_geometry(&pos, &nrm);
    pieces.push(SetPiece {
        name: "Toy",
        positions: p,
        normals: n,
        indices: idx,
        material: MAT_PINK,
        model: probe.toy_matrices[0],
        is_toy: true,
        visible_from: all.0,
        visible_until: all.1,
    });

    ArcScene {
        world: [0.16, 0.15, 0.19],
        sun: sun_light([55.0 * deg, 0.0, -25.0 * deg], 3.0),
        fill: sun_light([70.0 * deg, 0.0, 140.0 * deg], 1.1),
        view: probe.camera_world.inverse(),
        tan_half_x: 0.4 / 1.444_444_417_953_491_2,
        tan_half_y: 0.5 / 1.444_444_417_953_491_2,
        near: 0.1,
        far: 1000.0,
        pieces,
    }
}

/// Mirror of the shaders' AgX fit (shade.glsl — the
/// Filament fit of Blender's AgX), used for exactly one
/// thing: the background clear colour, which must equal
/// what EEVEE shows where no geometry covers the world.
/// The matrices here are row-major forms of the GLSL's
/// column-declared matrices (M[row][col] = the GLSL
/// constructor's col-th vec3, row-th component); keep the
/// two in step. The unit test pins the neutral points
/// (0.18 grey -> 0.5005, 1.0 -> 0.7919, verified against
/// the reference fit).
pub fn display_transform_mirror(linear: [f32; 3]) -> [f32; 3] {
    fn contrast(x: f32) -> f32 {
        let x2 = x * x;
        let x4 = x2 * x2;
        15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x
            - 0.00232
    }
    const SRGB_TO_REC2020: [[f32; 3]; 3] = [
        [0.6274, 0.3293, 0.0433],
        [0.0691, 0.9195, 0.0113],
        [0.0164, 0.0880, 0.8956],
    ];
    const REC2020_TO_SRGB: [[f32; 3]; 3] = [
        [1.6605, -0.5876, -0.0728],
        [-0.1246, 1.1329, -0.0083],
        [-0.0182, -0.1006, 1.1187],
    ];
    const INSET: [[f32; 3]; 3] = [
        [0.85662715, 0.09512124, 0.04825161],
        [0.13731897, 0.761242, 0.10143904],
        [0.11189821, 0.07679942, 0.81130237],
    ];
    const OUTSET: [[f32; 3]; 3] = [
        [1.1271006, -0.11060664, -0.01649394],
        [-0.14132976, 1.1578237, -0.01649394],
        [-0.14132976, -0.11060664, 1.2519364],
    ];
    let apply = |m: [[f32; 3]; 3], v: [f32; 3]| {
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    };
    let c = apply(SRGB_TO_REC2020, linear);
    let c = apply(INSET, c);
    let c = c.map(|x| {
        let logged = (x.max(1e-10).log2() - (-12.47393_f32)) / (4.026069_f32 - (-12.47393_f32));
        contrast(logged.clamp(0.0, 1.0))
    });
    let c = apply(OUTSET, c);
    let c = c.map(|x| x.max(0.0).powf(2.2));
    let c = apply(REC2020_TO_SRGB, c);
    let srgb = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.003_130_8 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    [srgb(c[0]), srgb(c[1]), srgb(c[2])]
}

/// Projection matrix (Vulkan clip space, z in [0,1], y
/// flipped so the standard viewport maps camera-up to
/// image-up).
pub fn projection(scene: &ArcScene) -> Mat4 {
    let (n, f) = (scene.near, scene.far);
    let mut m = [0.0_f64; 16];
    m[0] = 1.0 / scene.tan_half_x;
    m[5] = -1.0 / scene.tan_half_y;
    m[10] = f / (n - f);
    m[11] = -1.0;
    m[14] = n * f / (n - f);
    Mat4 { m }
}

// -- Primitive generators (Blender local space: Z up) -----

fn cube_positions() -> Vec<[f32; 3]> {
    // Blender's primitive cube is 2x2x2 (verts at +/-1).
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [1.0, 0.0, 0.0],
            [
                [1.0, -1.0, -1.0],
                [1.0, 1.0, -1.0],
                [1.0, 1.0, 1.0],
                [1.0, -1.0, 1.0],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-1.0, 1.0, -1.0],
                [-1.0, -1.0, -1.0],
                [-1.0, -1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-1.0, 1.0, -1.0],
                [1.0, 1.0, -1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [1.0, -1.0, -1.0],
                [-1.0, -1.0, -1.0],
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [1.0, -1.0, -1.0],
                [-1.0, -1.0, -1.0],
                [-1.0, 1.0, -1.0],
                [1.0, 1.0, -1.0],
            ],
        ),
    ];
    let mut out = Vec::with_capacity(24);
    let _ = &faces;
    for (_, corners) in &faces {
        out.extend_from_slice(corners);
    }
    out
}

fn cube_normals() -> Vec<[f32; 3]> {
    let normals = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut out = Vec::with_capacity(24);
    for n in normals {
        out.extend_from_slice(&[n; 4]);
    }
    out
}

fn cube_indices() -> Vec<u32> {
    let mut out = Vec::with_capacity(36);
    for face in 0..6_u32 {
        let base = face * 4;
        out.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    out
}

/// Unit UV sphere: `segments` around, `rings` pole to pole
/// (Blender's defaults: 32 x 16). Smooth normals.
fn uv_sphere(segments: usize, rings: usize) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    // Rings from top pole (exclusive) to bottom pole.
    for ring in 0..=rings {
        let phi = std::f64::consts::PI * f64::from(ring as u32) / f64::from(rings as u32);
        let (sp, cp) = (phi.sin(), phi.cos());
        for seg in 0..segments {
            let theta =
                2.0 * std::f64::consts::PI * f64::from(seg as u32) / f64::from(segments as u32);
            let v = V3::new(sp * theta.cos(), sp * theta.sin(), cp);
            positions.push(v.to_f32());
            normals.push(v.to_f32());
        }
    }
    for ring in 0..rings {
        for seg in 0..segments {
            let a = (ring * segments + seg) as u32;
            let b = (ring * segments + (seg + 1) % segments) as u32;
            let c = ((ring + 1) * segments + seg) as u32;
            let d = ((ring + 1) * segments + (seg + 1) % segments) as u32;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    (positions, normals, indices)
}

/// Cylinder along Z (Blender convention), smooth sides,
/// flat caps, `segments` around.
fn cylinder(radius: f64, depth: f64, segments: usize) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let half = depth / 2.0;
    // Side vertices: bottom ring then top ring.
    for ring_z in [-half, half] {
        for seg in 0..segments {
            let theta =
                2.0 * std::f64::consts::PI * f64::from(seg as u32) / f64::from(segments as u32);
            let dir = V3::new(theta.cos(), theta.sin(), 0.0);
            positions.push(V3::new(dir.x * radius, dir.y * radius, ring_z).to_f32());
            normals.push(dir.to_f32());
        }
    }
    for seg in 0..segments {
        let a = seg as u32;
        let b = ((seg + 1) % segments) as u32;
        let c = (segments + seg) as u32;
        let d = (segments + (seg + 1) % segments) as u32;
        indices.extend_from_slice(&[a, b, c, b, d, c]);
    }
    // Caps as fans around a centre vertex.
    for (z, nz) in [(-half, -1.0_f64), (half, 1.0)] {
        let centre = positions.len() as u32;
        positions.push([0.0, 0.0, z as f32]);
        normals.push([0.0, 0.0, nz as f32]);
        let ring_start = positions.len() as u32;
        for seg in 0..segments {
            let theta =
                2.0 * std::f64::consts::PI * f64::from(seg as u32) / f64::from(segments as u32);
            positions.push([
                (radius * theta.cos()) as f32,
                (radius * theta.sin()) as f32,
                z as f32,
            ]);
            normals.push([0.0, 0.0, nz as f32]);
        }
        for seg in 0..segments {
            let a = ring_start + seg as u32;
            let b = ring_start + ((seg + 1) % segments) as u32;
            if nz > 0.0 {
                indices.extend_from_slice(&[centre, a, b]);
            } else {
                indices.extend_from_slice(&[centre, b, a]);
            }
        }
    }
    (positions, normals, indices)
}

/// Torus in the XY plane (Blender's primitive lies flat,
/// axis +Z), smooth normals.
fn torus(
    major: f64,
    minor: f64,
    major_segments: usize,
    minor_segments: usize,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for i in 0..major_segments {
        let u = 2.0 * std::f64::consts::PI * f64::from(i as u32) / f64::from(major_segments as u32);
        let centre = V3::new(major * u.cos(), major * u.sin(), 0.0);
        for j in 0..minor_segments {
            let v =
                2.0 * std::f64::consts::PI * f64::from(j as u32) / f64::from(minor_segments as u32);
            let n = V3::new(v.cos() * u.cos(), v.cos() * u.sin(), v.sin());
            positions.push(centre.plus(n.scale(minor)).to_f32());
            normals.push(n.to_f32());
        }
    }
    for i in 0..major_segments {
        for j in 0..minor_segments {
            let a = (i * minor_segments + j) as u32;
            let b = (((i + 1) % major_segments) * minor_segments + j) as u32;
            let c = (i * minor_segments + (j + 1) % minor_segments) as u32;
            let d = (((i + 1) % major_segments) * minor_segments + (j + 1) % minor_segments) as u32;
            indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }
    (positions, normals, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_map_is_a_proper_rotation() {
        // det(C) = +1: the conjugation preserves handedness,
        // so Blender windings remain valid.
        let c = axis_map();
        let det = c.m[0] * (c.m[5] * c.m[10] - c.m[6] * c.m[9])
            - c.m[4] * (c.m[1] * c.m[10] - c.m[2] * c.m[9])
            + c.m[8] * (c.m[1] * c.m[6] - c.m[2] * c.m[5]);
        assert!((det - 1.0).abs() < 1e-12);
        let v = blender_to_gltf(V3::new(1.0, 2.0, 3.0));
        assert_eq!(v, V3::new(1.0, 3.0, -2.0));
    }

    #[test]
    fn world_matrix_conversion_keeps_camera_semantics() {
        // The arc camera's Blender matrix_world (probe2,
        // rounded): translation (1.8, -2.6, 1.57); its back
        // column is (0.5081, -0.8185, 0.2681). In glTF
        // space the position must be (1.8, 1.57, 2.6) and
        // the back column (0.5081, 0.2681, 0.8185) — a
        // conjugation would swap the up/back columns and
        // point the camera along its up axis.
        let blender = Mat4::from_rows([
            [0.84964, -0.14141, 0.50805, 1.8],
            [0.52736, 0.22782, -0.81853, -2.6],
            [0.0, 0.96338, 0.26814, 1.57],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let gltf = convert_matrix(blender);
        let t = gltf.translation();
        assert!((t.x - 1.8).abs() < 1e-4, "{t:?}");
        assert!((t.y - 1.57).abs() < 1e-4, "{t:?}");
        assert!((t.z - 2.6).abs() < 1e-4, "{t:?}");
        // Back = third basis column (column-major storage:
        // m[8], m[9], m[10]); A * back_b = (x, z, -y).
        let back = V3::new(gltf.m[8], gltf.m[9], gltf.m[10]);
        assert!((back.x - 0.50805).abs() < 1e-4, "{back:?}");
        assert!((back.y - 0.26814).abs() < 1e-4, "{back:?}");
        assert!((back.z - 0.81853).abs() < 1e-4, "{back:?}");
    }

    #[test]
    fn outfit_plan_boundaries() {
        assert_eq!(outfit_for_frame(0), OutfitSelection::Outfit);
        assert_eq!(outfit_for_frame(89), OutfitSelection::Outfit);
        assert_eq!(outfit_for_frame(90), OutfitSelection::OutfitEnd);
        assert_eq!(outfit_for_frame(359), OutfitSelection::OutfitEnd);
    }

    #[test]
    fn generators_have_consistent_counts() {
        let (p, n, i) = uv_sphere(32, 16);
        assert_eq!(p.len(), n.len());
        assert_eq!(i.len() % 3, 0);
        assert!(i.iter().all(|&v| (v as usize) < p.len()));
        let (p, n, i) = cylinder(0.016, 0.62, 32);
        assert_eq!(p.len(), n.len());
        assert!(i.iter().all(|&v| (v as usize) < p.len()));
        let (p, n, i) = torus(0.19, 0.008, 48, 12);
        assert_eq!(p.len(), n.len());
        assert!(i.iter().all(|&v| (v as usize) < p.len()));
    }

    #[test]
    fn display_mirror_is_neutral_and_bounded() {
        // Reference values from the verified fit: 0.18
        // grey lands on 0.5005 exactly neutral, 1.0 rolls
        // off to 0.7919, 0.0 maps to 0.0.
        let grey = display_transform_mirror([0.18, 0.18, 0.18]);
        assert!(
            (grey[0] - grey[1]).abs() < 0.002 && (grey[1] - grey[2]).abs() < 0.002,
            "{grey:?}"
        );
        assert!((grey[0] - 0.5005).abs() < 0.01, "{grey:?}");
        let white = display_transform_mirror([1.0, 1.0, 1.0]);
        assert!((white[0] - 0.7919).abs() < 0.01, "{white:?}");
        let black = display_transform_mirror([0.0, 0.0, 0.0]);
        assert!(black.iter().all(|v| *v == 0.0), "{black:?}");
    }

    // Adversarial: a truncated probe dump is a typed error.
    #[test]
    fn short_probe_is_an_error() {
        let bad = br#"{"toy_matrices": [], "visibility": [], "camera_matrices": {}}"#;
        assert!(ProbeData::from_bytes(bad).is_err());
    }
}
