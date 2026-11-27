//! Gaussian splat clouds: the standard 3DGS `.ply`
//! asset, its spherical-harmonics colour model, and the
//! deterministic projection into the boundary's quad
//! stream.
//!
//! Design of record: `DESIGN.md` in
//! `~/workspace/render3d-splats-20261125/` (pass placement,
//! data contract, sorting/determinism contract, envelope).
//! The short version this module implements:
//!
//! - Load is strict. Binary little-endian PLY with the
//!   Inria 3DGS property layout; anything else is an
//!   error, never a fallback.
//! - Colour is the full degree-3 real-SH evaluation,
//!   clamped below at zero — a pure function of the
//!   coefficients and the view direction.
//! - Packing projects each Gaussian to a screen-space
//!   quad (EWA: the 2D covariance is `J * W * Sigma *
//!   W^T * J^T`), evaluates its colour for the scene's
//!   camera, and sorts the quads into the compositing
//!   order: **view depth descending, asset index
//!   ascending** — a total order, so the packed stream
//!   is a pure function of (cloud, transform, camera,
//!   viewport) on any machine.
//! - Culling at pack time is part of the contract: a
//!   splat whose centre is behind the near plane, or
//!   whose peak contribution (its activated opacity) is
//!   below one UNORM level (1/255), contributes nothing
//!   and is dropped — identically on every run.
//!
//! The layer transform is full TRS (programme Rung 3):
//! `x' = t + s * R * x`. The rotation R moves centres,
//! covariances (`Sigma' = R * Sigma * R^T`, through the
//! splat quaternions) and the SH field (band-wise real
//! Wigner-D, `crate::splat_rotate`) — the same R for
//! all three. An identity rotation takes the Rung 1
//! path untouched, so its packet is bit-identical.
//!
//! Light interaction is programme Rung 4
//! (`crate::splat_light`): [`pack_splats_lit`] applies
//! the layer's named interactions — sun shadow receive
//! at splat centres, the spec-declared light response
//! — to the evaluated colour at pack time. The plain
//! [`pack_splats`] path is the Rung 3 pack, unchanged.

use bytemuck::{Pod, Zeroable};

use crate::error::RenderError;
use crate::math::Mat4;
use crate::setscene::ArcScene;
use crate::splat_light::SplatLight;
use crate::splat_rotate::{Rotation, ShRotation, is_identity_quat, quat_mul};

/// SH band-0 basis constant (Inria 3DGS reference).
const SH_C0: f64 = 0.282_094_791_773_878_14;
/// SH band-1 basis constant.
const SH_C1: f64 = 0.488_602_511_902_919_9;
/// SH band-2 basis constants.
const SH_C2: [f64; 5] = [
    1.092_548_430_592_079_2,
    -1.092_548_430_592_079_2,
    0.315_391_565_252_52,
    -1.092_548_430_592_079_2,
    0.546_274_215_296_039_6,
];
/// SH band-3 basis constants.
const SH_C3: [f64; 7] = [
    -0.590_043_589_926_643_5,
    2.890_611_442_640_554,
    -0.457_045_799_464_465_8,
    0.373_176_332_590_115_4,
    -0.457_045_799_464_465_8,
    1.445_305_721_320_277,
    -0.590_043_589_926_643_5,
];
/// Coefficients per colour channel above band 0
/// (bands 1–3: 3 + 5 + 7).
pub const SH_REST_PER_CHANNEL: usize = 15;
/// Total `f_rest_*` properties in a degree-3 cloud.
pub const SH_REST_TOTAL: usize = 3 * SH_REST_PER_CHANNEL;
/// Peak contribution below which a splat is culled at
/// pack time: one UNORM level.
const ALPHA_CULL: f32 = 1.0 / 255.0;
/// Screen-space dilation added to every projected
/// covariance (the 3DGS reference's low-pass term,
/// in square pixels).
const COV_DILATION_PX2: f64 = 0.3;
/// Quad extent along each eigen-axis, in sigmas.
const QUAD_SIGMAS: f64 = 3.0;

/// One Gaussian, in the asset's raw (unactivated)
/// stored form — exactly what the `.ply` carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Splat {
    /// Centre, asset space (metres).
    pub pos: [f32; 3],
    /// SH band 0 per channel.
    pub dc: [f32; 3],
    /// SH bands 1–3, channel-major: channel `c`'s 15
    /// coefficients at `rest[c * 15 .. c * 15 + 15]`.
    /// Zeros when the asset carries a lower degree.
    pub rest: [f32; SH_REST_TOTAL],
    /// Opacity logit; activated alpha is its sigmoid.
    pub opacity_logit: f32,
    /// Log-scales; activated scales are their exps.
    pub log_scale: [f32; 3],
    /// Rotation quaternion, `(w, x, y, z)`.
    pub rot: [f32; 4],
}

impl Splat {
    /// Activated opacity in `[0, 1]`.
    pub fn opacity(&self) -> f32 {
        1.0 / (1.0 + (-self.opacity_logit).exp())
    }
    /// Activated scales (asset units).
    pub fn scale(&self) -> [f32; 3] {
        [
            self.log_scale[0].exp(),
            self.log_scale[1].exp(),
            self.log_scale[2].exp(),
        ]
    }
}

/// A loaded splat cloud (asset order preserved — the
/// order is the sort's tie-break, so it is data).
#[derive(Debug, Clone, PartialEq)]
pub struct SplatCloud {
    pub splats: Vec<Splat>,
}

fn doc_err(detail: impl Into<String>) -> RenderError {
    RenderError::Document(format!("splat ply: {}", detail.into()))
}

/// The `.ply` property names this loader reads, in the
/// canonical write order. Load matches by name, so any
/// property order in the file is accepted.
fn canonical_properties() -> Vec<String> {
    let mut names = vec![
        "x".to_string(),
        "y".to_string(),
        "z".to_string(),
        "nx".to_string(),
        "ny".to_string(),
        "nz".to_string(),
        "f_dc_0".to_string(),
        "f_dc_1".to_string(),
        "f_dc_2".to_string(),
    ];
    for i in 0..SH_REST_TOTAL {
        names.push(format!("f_rest_{i}"));
    }
    names.extend([
        "opacity".to_string(),
        "scale_0".to_string(),
        "scale_1".to_string(),
        "scale_2".to_string(),
        "rot_0".to_string(),
        "rot_1".to_string(),
        "rot_2".to_string(),
        "rot_3".to_string(),
    ]);
    names
}

impl SplatCloud {
    /// Parse a binary little-endian 3DGS `.ply`.
    ///
    /// Strict by contract: ASCII or big-endian files,
    /// non-float properties, a missing required
    /// property, or a truncated payload are errors.
    /// `f_rest_*` properties may be absent in whole or
    /// in part (a lower-degree cloud); the bands they
    /// would fill stay zero.
    pub fn load_ply(bytes: &[u8]) -> Result<SplatCloud, RenderError> {
        // -- Header ------------------------------------
        let header_end = bytes
            .windows(10)
            .position(|w| w == b"end_header")
            .ok_or_else(|| doc_err("no end_header line"))?;
        let header = std::str::from_utf8(&bytes[..header_end])
            .map_err(|_| doc_err("header is not UTF-8"))?;
        let mut format_ok = false;
        let mut count: Option<usize> = None;
        let mut properties: Vec<String> = Vec::new();
        let mut in_vertex = false;
        for line in header.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.as_slice() {
                ["format", "binary_little_endian", "1.0"] => format_ok = true,
                ["format", ..] => {
                    return Err(doc_err(format!("unsupported format line: {line}")));
                }
                ["element", "vertex", n] => {
                    count = Some(
                        n.parse::<usize>()
                            .map_err(|_| doc_err("bad vertex count"))?,
                    );
                    in_vertex = true;
                }
                ["element", ..] => in_vertex = false,
                ["property", "float", name] if in_vertex => {
                    properties.push((*name).to_string());
                }
                ["property", ty, _] if in_vertex => {
                    return Err(doc_err(format!("non-float vertex property type: {ty}")));
                }
                _ => {}
            }
        }
        if !format_ok {
            return Err(doc_err("not a binary_little_endian 1.0 ply"));
        }
        let count = count.ok_or_else(|| doc_err("no vertex element"))?;
        let column = |name: &str| properties.iter().position(|p| p == name);
        let required = [
            "x", "y", "z", "f_dc_0", "f_dc_1", "f_dc_2", "opacity", "scale_0", "scale_1",
            "scale_2", "rot_0", "rot_1", "rot_2", "rot_3",
        ];
        for name in required {
            if column(name).is_none() {
                return Err(doc_err(format!("missing required property {name}")));
            }
        }
        // -- Payload -----------------------------------
        let payload_start = header_end + "end_header".len();
        let payload_start = bytes[payload_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| payload_start + i + 1)
            .ok_or_else(|| doc_err("header not newline-terminated"))?;
        let stride = properties.len() * 4;
        let needed = count
            .checked_mul(stride)
            .ok_or_else(|| doc_err("vertex count overflows payload size"))?;
        if bytes.len() - payload_start < needed {
            return Err(doc_err("payload truncated"));
        }
        let payload = &bytes[payload_start..payload_start + needed];
        let get = |row: usize, name: &str| -> f32 {
            let col = column(name).expect("required column checked");
            let off = row * stride + col * 4;
            f32::from_le_bytes(payload[off..off + 4].try_into().expect("4 bytes"))
        };
        let mut splats = Vec::with_capacity(count);
        for row in 0..count {
            let mut rest = [0.0_f32; SH_REST_TOTAL];
            for (i, slot) in rest.iter_mut().enumerate() {
                let name = format!("f_rest_{i}");
                if column(&name).is_some() {
                    *slot = get(row, &name);
                }
            }
            splats.push(Splat {
                pos: [get(row, "x"), get(row, "y"), get(row, "z")],
                dc: [get(row, "f_dc_0"), get(row, "f_dc_1"), get(row, "f_dc_2")],
                rest,
                opacity_logit: get(row, "opacity"),
                log_scale: [
                    get(row, "scale_0"),
                    get(row, "scale_1"),
                    get(row, "scale_2"),
                ],
                rot: [
                    get(row, "rot_0"),
                    get(row, "rot_1"),
                    get(row, "rot_2"),
                    get(row, "rot_3"),
                ],
            });
        }
        Ok(SplatCloud { splats })
    }

    /// Serialise in the canonical property order
    /// (normals written as zeros, as 3DGS does). The
    /// load/write round trip is exact.
    pub fn write_ply(&self) -> Vec<u8> {
        let names = canonical_properties();
        let mut out = Vec::new();
        out.extend_from_slice(b"ply\nformat binary_little_endian 1.0\n");
        out.extend_from_slice(format!("element vertex {}\n", self.splats.len()).as_bytes());
        for name in &names {
            out.extend_from_slice(format!("property float {name}\n").as_bytes());
        }
        out.extend_from_slice(b"end_header\n");
        for s in &self.splats {
            let mut row: Vec<f32> = Vec::with_capacity(names.len());
            row.extend_from_slice(&s.pos);
            row.extend_from_slice(&[0.0, 0.0, 0.0]);
            row.extend_from_slice(&s.dc);
            row.extend_from_slice(&s.rest);
            row.push(s.opacity_logit);
            row.extend_from_slice(&s.log_scale);
            row.extend_from_slice(&s.rot);
            for v in row {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }
}

/// Evaluate a splat's SH colour for a view direction
/// (unit vector from the splat centre toward the
/// camera, in the space the coefficients live in).
/// Full degree-3; result clamped below at zero, as
/// the reference model specifies.
pub fn sh_colour(splat: &Splat, dir: [f64; 3]) -> [f32; 3] {
    let (x, y, z) = (dir[0], dir[1], dir[2]);
    let (xx, yy, zz) = (x * x, y * y, z * z);
    // Basis values, bands 1–3 (band 0 is SH_C0).
    let b1 = [-SH_C1 * y, SH_C1 * z, -SH_C1 * x];
    let b2 = [
        SH_C2[0] * x * y,
        SH_C2[1] * y * z,
        SH_C2[2] * (2.0 * zz - xx - yy),
        SH_C2[3] * x * z,
        SH_C2[4] * (xx - yy),
    ];
    let b3 = [
        SH_C3[0] * y * (3.0 * xx - yy),
        SH_C3[1] * x * y * z,
        SH_C3[2] * y * (4.0 * zz - xx - yy),
        SH_C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy),
        SH_C3[4] * x * (4.0 * zz - xx - yy),
        SH_C3[5] * z * (xx - yy),
        SH_C3[6] * x * (xx - 3.0 * yy),
    ];
    let mut colour = [0.0_f64; 3];
    for ch in 0..3 {
        let rest = &splat.rest[ch * SH_REST_PER_CHANNEL..(ch + 1) * SH_REST_PER_CHANNEL];
        let mut v = 0.5 + SH_C0 * f64::from(splat.dc[ch]);
        for i in 0..3 {
            v += b1[i] * f64::from(rest[i]);
        }
        for i in 0..5 {
            v += b2[i] * f64::from(rest[3 + i]);
        }
        for i in 0..7 {
            v += b3[i] * f64::from(rest[8 + i]);
        }
        colour[ch] = v.max(0.0);
    }
    [colour[0] as f32, colour[1] as f32, colour[2] as f32]
}

/// Placement of a splat layer: `x' = t + s * R * x`
/// (translation, rotation, uniform scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplatTransform {
    pub translation: [f64; 3],
    /// Rotation quaternion `(w, x, y, z)` — the splat
    /// storage order, NOT the spec's glTF (x, y, z, w);
    /// the spec compiler converts. Must be finite and
    /// non-zero (normalised at pack); identity takes the
    /// unrotated path exactly.
    pub rotation: [f64; 4],
    pub scale: f64,
}

impl Default for SplatTransform {
    fn default() -> Self {
        SplatTransform {
            translation: [0.0; 3],
            rotation: crate::splat_rotate::IDENTITY_QUAT,
            scale: 1.0,
        }
    }
}

/// Rotate a whole cloud rigidly by `rotation`: centres
/// `R * x`, splat frames `q_R * q` (so each covariance
/// becomes `R * Sigma * R^T`), SH bands 1–3 by the
/// Wigner-D matrices of R, band 0 untouched. f64
/// arithmetic, cast back to the stored f32 form (the
/// one rounding this adds is named in the rung record).
pub fn rotate_cloud(cloud: &SplatCloud, rotation: &Rotation) -> SplatCloud {
    let sh = ShRotation::new(&rotation.matrix);
    let splats = cloud
        .splats
        .iter()
        .map(|s| {
            let pos = rotation.apply([
                f64::from(s.pos[0]),
                f64::from(s.pos[1]),
                f64::from(s.pos[2]),
            ]);
            let rot = quat_mul(rotation.quat, s.rot.map(f64::from));
            let mut rest = [0.0_f32; SH_REST_TOTAL];
            for ch in 0..3 {
                let span = ch * SH_REST_PER_CHANNEL..(ch + 1) * SH_REST_PER_CHANNEL;
                let mut c = [0.0_f64; SH_REST_PER_CHANNEL];
                for (dst, src) in c.iter_mut().zip(&s.rest[span.clone()]) {
                    *dst = f64::from(*src);
                }
                for (dst, src) in rest[span].iter_mut().zip(sh.rotate_channel(&c)) {
                    *dst = src as f32;
                }
            }
            Splat {
                pos: pos.map(|v| v as f32),
                rot: rot.map(|v| v as f32),
                rest,
                ..*s
            }
        })
        .collect();
    SplatCloud { splats }
}

/// One projected splat quad vertex — the boundary's
/// GPU-ready stream. Field order is the vertex-input
/// contract the backend's splat pipeline declares.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct SplatVertex {
    /// Clip-space position (w = 1; z is the splat
    /// centre's NDC depth for all four corners).
    pub clip: [f32; 4],
    /// The 2D conic (inverse covariance), packed
    /// `(a, b, c)`: weight =
    /// `exp(-0.5 * (a*dx^2 + 2*b*dx*dy + c*dy^2))`
    /// over pixel offsets; w is the activated opacity.
    pub conic: [f32; 4],
    /// SH colour for the scene camera (linear RGB);
    /// w unused (0).
    pub colour: [f32; 4],
    /// This corner's offset from the splat centre, in
    /// pixels (the fragment's Gaussian argument).
    pub offset_px: [f32; 2],
}

/// A packed splat layer: sorted quads, ready to
/// upload. Produced by [`pack_splats`]; consumed by
/// the backend. Plain data, no GPU handles — the
/// boundary rule.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedSplats {
    pub vertices: Vec<SplatVertex>,
    pub indices: Vec<u32>,
    /// Splats that survived culling (vertices = 4x,
    /// indices = 6x this).
    pub splat_count: usize,
}

fn apply(m: &Mat4, v: [f64; 4]) -> [f64; 4] {
    // Mat4 storage is column-major: element (row i,
    // col j) lives at m[j * 4 + i].
    let mut out = [0.0; 4];
    for i in 0..4 {
        out[i] = m.m[i] * v[0] + m.m[4 + i] * v[1] + m.m[8 + i] * v[2] + m.m[12 + i] * v[3];
    }
    out
}

/// The 3x3 covariance `R * diag(s^2) * R^T` of a
/// splat (asset space), from its quaternion + scales.
fn covariance(rot: [f32; 4], scale: [f32; 3]) -> [[f64; 3]; 3] {
    let (w, x, y, z) = (
        f64::from(rot[0]),
        f64::from(rot[1]),
        f64::from(rot[2]),
        f64::from(rot[3]),
    );
    let norm = (w * w + x * x + y * y + z * z).sqrt();
    let (w, x, y, z) = if norm > 0.0 {
        (w / norm, x / norm, y / norm, z / norm)
    } else {
        (1.0, 0.0, 0.0, 0.0)
    };
    let r = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    let s2 = [
        f64::from(scale[0]).powi(2),
        f64::from(scale[1]).powi(2),
        f64::from(scale[2]).powi(2),
    ];
    let mut cov = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            cov[i][j] = (0..3).map(|k| r[i][k] * s2[k] * r[j][k]).sum();
        }
    }
    cov
}

/// Project, colour, cull and sort a cloud into the
/// boundary packet, for `scene`'s camera at a
/// `width` x `height` viewport. Pure: identical
/// inputs give an identical packet (the determinism
/// contract, DESIGN.md §3).
///
/// Panics if `transform.rotation` is zero or
/// non-finite: that is a caller bug (the spec compiler
/// refuses such rotations with a typed error first).
pub fn pack_splats(
    cloud: &SplatCloud,
    transform: &SplatTransform,
    scene: &ArcScene,
    width: u32,
    height: u32,
) -> PackedSplats {
    pack_impl(cloud, transform, scene, width, height, None)
}

/// [`pack_splats`] with the layer's Rung 4 light
/// interaction applied to each splat's evaluated
/// colour at its world centre (see
/// `crate::splat_light`). An inactive `light` (both
/// fields absent) is the plain pack, bit-identical —
/// the hook is skipped whole, not applied as an
/// identity.
///
/// Panics under the same condition as
/// [`pack_splats`].
pub fn pack_splats_lit(
    cloud: &SplatCloud,
    transform: &SplatTransform,
    scene: &ArcScene,
    width: u32,
    height: u32,
    light: &SplatLight<'_>,
) -> PackedSplats {
    let light = if light.is_active() {
        Some(*light)
    } else {
        None
    };
    pack_impl(cloud, transform, scene, width, height, light)
}

fn pack_impl(
    cloud: &SplatCloud,
    transform: &SplatTransform,
    scene: &ArcScene,
    width: u32,
    height: u32,
    light: Option<SplatLight<'_>>,
) -> PackedSplats {
    // Rotation first (TRS: x' = t + s * R * x). The
    // identity skips it, leaving the Rung 1 path —
    // and so its packet — exactly as it was.
    let rotated;
    let cloud = if is_identity_quat(transform.rotation) {
        cloud
    } else {
        let rotation = Rotation::from_quaternion(transform.rotation)
            .expect("SplatTransform.rotation must be a finite, non-zero quaternion");
        rotated = rotate_cloud(cloud, &rotation);
        &rotated
    };
    let view = scene.view;
    let view_proj = crate::setscene::projection(scene).multiply(view);
    let camera_world = view.inverse();
    let cam = camera_world.translation();
    let camera_pos = [cam.x, cam.y, cam.z];
    // Rotation part of the view matrix (world -> view):
    // element (row i, col j) at m[j * 4 + i].
    let w3 = [
        [view.m[0], view.m[4], view.m[8]],
        [view.m[1], view.m[5], view.m[9]],
        [view.m[2], view.m[6], view.m[10]],
    ];
    let (wf, hf) = (f64::from(width), f64::from(height));

    struct Projected {
        depth: f64,
        index: usize,
        centre_px: [f64; 2],
        z_ndc: f32,
        conic: [f32; 3],
        radii: [f64; 2],
        axes: [[f64; 2]; 2],
        colour: [f32; 3],
        opacity: f32,
    }
    let mut projected: Vec<Projected> = Vec::new();
    for (index, splat) in cloud.splats.iter().enumerate() {
        let opacity = splat.opacity();
        if opacity < ALPHA_CULL {
            continue;
        }
        let centre = [
            f64::from(splat.pos[0]) * transform.scale + transform.translation[0],
            f64::from(splat.pos[1]) * transform.scale + transform.translation[1],
            f64::from(splat.pos[2]) * transform.scale + transform.translation[2],
        ];
        let t = apply(&view, [centre[0], centre[1], centre[2], 1.0]);
        let depth = -t[2];
        if depth < scene.near {
            continue;
        }
        // Covariance: asset -> layer (uniform scale)
        // -> view.
        let cov_asset = covariance(splat.rot, splat.scale());
        let s2 = transform.scale * transform.scale;
        let mut cov_view = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                cov_view[i][j] = s2
                    * (0..3)
                        .map(|a| {
                            (0..3)
                                .map(|b| w3[i][a] * cov_asset[a][b] * w3[j][b])
                                .sum::<f64>()
                        })
                        .sum::<f64>();
            }
        }
        // Perspective Jacobian at the centre (NDC).
        let (tx, ty, tz) = (t[0], t[1], t[2]);
        let j = [
            [
                1.0 / (scene.tan_half_x * -tz),
                0.0,
                tx / (scene.tan_half_x * tz * tz),
            ],
            [
                0.0,
                1.0 / (scene.tan_half_y * tz),
                -ty / (scene.tan_half_y * tz * tz),
            ],
        ];
        // Sigma_ndc = J * Sigma_view * J^T (2x2), then
        // to pixel space.
        let mut cov_ndc = [[0.0; 2]; 2];
        for i in 0..2 {
            for k in 0..2 {
                cov_ndc[i][k] = (0..3)
                    .map(|a| {
                        (0..3)
                            .map(|b| j[i][a] * cov_view[a][b] * j[k][b])
                            .sum::<f64>()
                    })
                    .sum::<f64>();
            }
        }
        let d = [wf / 2.0, hf / 2.0];
        let mut cov_px = [
            [
                cov_ndc[0][0] * d[0] * d[0] + COV_DILATION_PX2,
                cov_ndc[0][1] * d[0] * d[1],
            ],
            [
                cov_ndc[1][0] * d[1] * d[0],
                cov_ndc[1][1] * d[1] * d[1] + COV_DILATION_PX2,
            ],
        ];
        cov_px[1][0] = cov_px[0][1];
        let det = cov_px[0][0] * cov_px[1][1] - cov_px[0][1] * cov_px[1][0];
        if det <= 0.0 {
            continue;
        }
        let conic = [
            (cov_px[1][1] / det) as f32,
            (-cov_px[0][1] / det) as f32,
            (cov_px[0][0] / det) as f32,
        ];
        // Eigen-decomposition of the symmetric 2x2
        // for the quad extent.
        let (a, b, c) = (cov_px[0][0], cov_px[0][1], cov_px[1][1]);
        let mid = (a + c) / 2.0;
        let disc = (((a - c) / 2.0).powi(2) + b * b).sqrt();
        let (l1, l2) = (mid + disc, (mid - disc).max(0.0));
        let axis1 = if b.abs() > 1e-12 {
            let v = [b, l1 - a];
            let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
            [v[0] / n, v[1] / n]
        } else if a >= c {
            [1.0, 0.0]
        } else {
            [0.0, 1.0]
        };
        let axis2 = [-axis1[1], axis1[0]];
        let radii = [QUAD_SIGMAS * l1.sqrt(), QUAD_SIGMAS * l2.sqrt()];
        // Centre: pixel position + NDC depth.
        let clip = apply(&view_proj, [centre[0], centre[1], centre[2], 1.0]);
        if clip[3] <= 0.0 {
            continue;
        }
        let ndc = [clip[0] / clip[3], clip[1] / clip[3]];
        let centre_px = [(ndc[0] * 0.5 + 0.5) * wf, (ndc[1] * 0.5 + 0.5) * hf];
        // SH colour for this camera.
        let to_cam = [
            camera_pos[0] - centre[0],
            camera_pos[1] - centre[1],
            camera_pos[2] - centre[2],
        ];
        let len = (to_cam[0].powi(2) + to_cam[1].powi(2) + to_cam[2].powi(2)).sqrt();
        if len <= 0.0 {
            continue;
        }
        let colour = sh_colour(splat, [to_cam[0] / len, to_cam[1] / len, to_cam[2] / len]);
        // Rung 4: the layer's light interaction, at
        // the splat's world centre, when named. The
        // unlit path flows through untouched.
        let colour = match &light {
            Some(light) => light.apply(colour, centre, scene.world),
            None => colour,
        };
        projected.push(Projected {
            depth,
            index,
            centre_px,
            z_ndc: (clip[2] / clip[3]) as f32,
            conic,
            radii,
            axes: [axis1, axis2],
            colour,
            opacity,
        });
    }
    // The compositing order: depth descending, asset
    // index ascending — a total order (DESIGN.md §3).
    projected.sort_by(|a, b| {
        b.depth
            .total_cmp(&a.depth)
            .then_with(|| a.index.cmp(&b.index))
    });
    let mut vertices = Vec::with_capacity(projected.len() * 4);
    let mut indices = Vec::with_capacity(projected.len() * 6);
    for p in &projected {
        let base = vertices.len() as u32;
        let signs = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        for sign in signs {
            let off = [
                sign[0] * p.radii[0] * p.axes[0][0] + sign[1] * p.radii[1] * p.axes[1][0],
                sign[0] * p.radii[0] * p.axes[0][1] + sign[1] * p.radii[1] * p.axes[1][1],
            ];
            let px = [p.centre_px[0] + off[0], p.centre_px[1] + off[1]];
            vertices.push(SplatVertex {
                clip: [
                    (px[0] / wf * 2.0 - 1.0) as f32,
                    (px[1] / hf * 2.0 - 1.0) as f32,
                    p.z_ndc,
                    1.0,
                ],
                conic: [p.conic[0], p.conic[1], p.conic[2], p.opacity],
                colour: [p.colour[0], p.colour[1], p.colour[2], 0.0],
                offset_px: [off[0] as f32, off[1] as f32],
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    PackedSplats {
        splat_count: projected.len(),
        vertices,
        indices,
    }
}

/// A deterministic splitmix64 — the procedural
/// cloud's only randomness, fixed-seeded so the
/// cloud is a constant of the code.
struct Rng(u64);

impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
    fn jitter(&mut self, amp: f64) -> f32 {
        ((self.next_f64() * 2.0 - 1.0) * amp) as f32
    }
}

fn splat_from_colour(pos: [f32; 3], colour: [f32; 3], scale: [f32; 3], opacity: f32) -> Splat {
    let dc = [
        (colour[0] - 0.5) / SH_C0 as f32,
        (colour[1] - 0.5) / SH_C0 as f32,
        (colour[2] - 0.5) / SH_C0 as f32,
    ];
    Splat {
        pos,
        dc,
        rest: [0.0; SH_REST_TOTAL],
        opacity_logit: (opacity / (1.0 - opacity)).ln(),
        log_scale: [scale[0].ln(), scale[1].ln(), scale[2].ln()],
        rot: [1.0, 0.0, 0.0, 0.0],
    }
}

/// The spike's procedural test cloud: a room corner
/// (floor + two walls) with a warm sphere prop,
/// authored natively in glTF space (Y-up, metres).
/// The corner stands at (+x, +z) — the far side from
/// the renderer's conventional camera side (-Z) — so
/// a camera in front of the character sees floor
/// below, walls behind, and the prop behind-left of
/// the subject. Known ground truth by construction —
/// the proving test reads its colours and occlusion
/// back. Deterministic: same cloud on every run,
/// every machine.
pub fn room_corner_cloud() -> SplatCloud {
    let mut rng = Rng(0x5EED_2026_1125);
    let mut splats = Vec::new();
    let j = |rng: &mut Rng| rng.jitter(0.02);
    // Floor (y = 0), slate blue-grey.
    let mut gx = -2.4;
    while gx <= 2.4 {
        let mut gz = -2.4;
        while gz <= 2.4 {
            splats.push(splat_from_colour(
                [gx + j(&mut rng), j(&mut rng) * 0.25, gz + j(&mut rng)],
                [0.32, 0.34, 0.40],
                [0.17, 0.02, 0.17],
                0.97,
            ));
            gz += 0.3;
        }
        gx += 0.3;
    }
    // Back wall (z = +2.4), warm grey.
    let mut gx = -2.4;
    while gx <= 2.4 {
        let mut gy = 0.15;
        while gy <= 2.4 {
            splats.push(splat_from_colour(
                [gx + j(&mut rng), gy + j(&mut rng), 2.4 + j(&mut rng)],
                [0.45, 0.40, 0.36],
                [0.17, 0.17, 0.02],
                0.97,
            ));
            gy += 0.3;
        }
        gx += 0.3;
    }
    // Right wall (x = +2.4), cooler grey.
    let mut gz = -2.4;
    while gz <= 2.4 {
        let mut gy = 0.15;
        while gy <= 2.4 {
            splats.push(splat_from_colour(
                [2.4 + j(&mut rng), gy + j(&mut rng), gz + j(&mut rng)],
                [0.36, 0.40, 0.44],
                [0.02, 0.17, 0.17],
                0.97,
            ));
            gy += 0.3;
        }
        gz += 0.3;
    }
    // The prop: a warm sphere (Fibonacci lattice),
    // behind-left of the subject.
    let centre = [-0.8_f32, 0.55, 1.0];
    let radius = 0.38_f32;
    let n = 220;
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    for i in 0..n {
        let y = 1.0 - (f64::from(i) / f64::from(n - 1)) * 2.0;
        let r = (1.0 - y * y).max(0.0).sqrt();
        let theta = golden * f64::from(i);
        let dir = [theta.cos() * r, y, theta.sin() * r];
        splats.push(splat_from_colour(
            [
                centre[0] + (dir[0] as f32) * radius,
                centre[1] + (dir[1] as f32) * radius,
                centre[2] + (dir[2] as f32) * radius,
            ],
            [0.85, 0.45, 0.20],
            [0.09, 0.09, 0.09],
            0.97,
        ));
    }
    SplatCloud { splats }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::V3;
    use crate::setscene::DirectionalLight;

    fn test_scene() -> ArcScene {
        // Camera at (0, 0, 5) looking down -Z (glTF
        // view = world translated by -5 in z).
        ArcScene {
            world: [0.16, 0.15, 0.19],
            sun: DirectionalLight {
                to_light: V3::new(0.0, 1.0, 0.0),
                energy: 1.0,
            },
            fill: DirectionalLight {
                to_light: V3::new(0.0, 1.0, 0.0),
                energy: 1.0,
            },
            view: Mat4::from_rows([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, -5.0],
                [0.0, 0.0, 0.0, 1.0],
            ]),
            tan_half_x: 0.5,
            tan_half_y: 0.5,
            near: 0.1,
            far: 100.0,
            pieces: Vec::new(),
        }
    }

    #[test]
    fn ply_round_trip_is_exact() {
        let cloud = room_corner_cloud();
        let bytes = cloud.write_ply();
        let back = SplatCloud::load_ply(&bytes).expect("round trip loads");
        assert_eq!(back.splats.len(), cloud.splats.len());
        assert_eq!(back, cloud);
    }

    #[test]
    fn ply_rejects_ascii_and_truncation() {
        assert!(SplatCloud::load_ply(b"ply\nformat ascii 1.0\nend_header\n").is_err());
        let bytes = room_corner_cloud().write_ply();
        let cut = &bytes[..bytes.len() - 8];
        assert!(SplatCloud::load_ply(cut).is_err());
    }

    #[test]
    fn sh_dc_only_is_direction_independent() {
        let s = splat_from_colour([0.0; 3], [0.25, 0.5, 0.75], [0.1; 3], 0.9);
        for dir in [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            let c = sh_colour(&s, dir);
            assert!((c[0] - 0.25).abs() < 1e-5, "{c:?}");
            assert!((c[1] - 0.5).abs() < 1e-5, "{c:?}");
            assert!((c[2] - 0.75).abs() < 1e-5, "{c:?}");
        }
    }

    #[test]
    fn sh_band1_matches_reference_basis() {
        // Channel 0, first band-1 coefficient: the
        // basis is -C1 * y, so looking from +Y with
        // rest = 1 gives 0.5 - C1 (plus the DC term 0).
        let mut s = splat_from_colour([0.0; 3], [0.5, 0.5, 0.5], [0.1; 3], 0.9);
        s.rest[0] = 1.0;
        let c = sh_colour(&s, [0.0, 1.0, 0.0]);
        let expected = 0.5 - SH_C1;
        assert!(
            (f64::from(c[0]) - expected).abs() < 1e-5,
            "got {}, want {expected}",
            c[0]
        );
        // Other channels untouched.
        assert!((c[1] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn pack_culls_behind_camera_and_sorts_back_to_front() {
        let near = splat_from_colour([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.2; 3], 0.97);
        let far = splat_from_colour([0.0, 0.0, -2.0], [0.0, 0.0, 1.0], [0.2; 3], 0.97);
        let behind = splat_from_colour([0.0, 0.0, 9.0], [0.0, 1.0, 0.0], [0.2; 3], 0.97);
        let cloud = SplatCloud {
            splats: vec![near, far, behind],
        };
        let packed = pack_splats(&cloud, &SplatTransform::default(), &test_scene(), 512, 640);
        assert_eq!(packed.splat_count, 2, "behind-camera splat culled");
        // Farthest first: the first quad is the far
        // (blue) splat.
        let first = packed.vertices[0].colour;
        assert!(
            first[2] > 0.9 && first[0] < 0.1,
            "far splat first: {first:?}"
        );
    }

    #[test]
    fn pack_with_explicit_identity_rotation_equals_rung1_packet() {
        // Programme Rung 3, gate 1: rotation by
        // identity reproduces the Rung 1 packet
        // bit-identically (the new path contains
        // the old).
        let cloud = room_corner_cloud();
        let rung1 = pack_splats(&cloud, &SplatTransform::default(), &test_scene(), 512, 640);
        let explicit = pack_splats(
            &cloud,
            &SplatTransform {
                translation: [0.0; 3],
                rotation: crate::splat_rotate::IDENTITY_QUAT,
                scale: 1.0,
            },
            &test_scene(),
            512,
            640,
        );
        assert_eq!(rung1, explicit);
        assert!(rung1.splat_count > 500, "room packs: {}", rung1.splat_count);
    }

    #[test]
    fn pack_is_deterministic() {
        let cloud = room_corner_cloud();
        let a = pack_splats(&cloud, &SplatTransform::default(), &test_scene(), 512, 640);
        let b = pack_splats(&cloud, &SplatTransform::default(), &test_scene(), 512, 640);
        assert_eq!(a, b);
        assert!(a.splat_count > 500, "room cloud packs: {}", a.splat_count);
    }

    #[test]
    fn pack_lit_with_inactive_light_equals_rung3_packet() {
        // Programme Rung 4, gate 3 (direct side): a
        // light with neither interaction named is the
        // Rung 3 pack, bit-identically.
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let plain = pack_splats(&cloud, &SplatTransform::default(), &scene, 512, 640);
        let lit = pack_splats_lit(
            &cloud,
            &SplatTransform::default(),
            &scene,
            512,
            640,
            &crate::splat_light::SplatLight::default(),
        );
        assert_eq!(plain, lit);
    }

    #[test]
    fn pack_lit_response_grades_every_colour() {
        // A channel-swap grade recolours every quad
        // and touches nothing else in the stream.
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let plain = pack_splats(&cloud, &SplatTransform::default(), &scene, 512, 640);
        let light = crate::splat_light::SplatLight {
            shadow: None,
            response: Some(crate::splat_light::LightResponse {
                grade: [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
                world_mix: 0.0,
            }),
        };
        let lit = pack_splats_lit(&cloud, &SplatTransform::default(), &scene, 512, 640, &light);
        assert_eq!(plain.splat_count, lit.splat_count);
        assert_eq!(plain.indices, lit.indices);
        for (a, b) in plain.vertices.iter().zip(&lit.vertices) {
            assert_eq!(a.clip, b.clip);
            assert_eq!(a.conic, b.conic);
            assert_eq!(a.offset_px, b.offset_px);
            assert_eq!(
                b.colour,
                [a.colour[2], a.colour[1], a.colour[0], a.colour[3]],
                "colour is the graded colour"
            );
        }
    }

    #[test]
    fn pack_lit_shadow_darkens_only_covered_centres() {
        // Programme Rung 4, gate 1 (packet side): a
        // caster quad overhead (the test scene's sun
        // is +Y) shadows the splat below it to black;
        // the splat beside the quad keeps its Rung 3
        // vertices bit-identically.
        use crate::splat_light::{SplatLight, SplatShadowMap};
        let scene = test_scene();
        let quad = [
            [[-1.0, 2.0, -1.0], [1.0, 2.0, -1.0], [1.0, 2.0, 1.0]],
            [[-1.0, 2.0, -1.0], [1.0, 2.0, 1.0], [-1.0, 2.0, 1.0]],
        ];
        let map = SplatShadowMap::rasterise(
            &quad,
            crate::backend::light_view_proj(scene.sun.to_light),
            256,
        );
        let covered = splat_from_colour([0.0, 0.0, 0.0], [1.0, 0.2, 0.2], [0.2; 3], 0.97);
        let clear = splat_from_colour([1.8, 0.0, 0.0], [0.2, 1.0, 0.2], [0.2; 3], 0.97);
        let cloud = SplatCloud {
            splats: vec![covered, clear],
        };
        let plain = pack_splats(&cloud, &SplatTransform::default(), &scene, 512, 640);
        let lit = pack_splats_lit(
            &cloud,
            &SplatTransform::default(),
            &scene,
            512,
            640,
            &SplatLight {
                shadow: Some(&map),
                response: None,
            },
        );
        assert_eq!(plain.splat_count, 2);
        assert_eq!(lit.splat_count, 2);
        // Same order (colours do not sort): quad 0 is
        // one splat, quad 1 the other; identify by
        // the plain pack's colours.
        for q in 0..2 {
            let plain_c = plain.vertices[q * 4].colour;
            let lit_q = &lit.vertices[q * 4..q * 4 + 4];
            let plain_q = &plain.vertices[q * 4..q * 4 + 4];
            if plain_c[0] > 0.9 {
                // The covered (red) splat: fully
                // shadowed at its centre — colour
                // black, geometry untouched.
                for (a, b) in lit_q.iter().zip(plain_q.iter()) {
                    assert_eq!(a.colour, [0.0, 0.0, 0.0, 0.0], "covered centre is black");
                    assert_eq!(a.clip, b.clip);
                    assert_eq!(a.conic, b.conic);
                    assert_eq!(a.offset_px, b.offset_px);
                }
            } else {
                // The clear (green) splat: untouched,
                // bit-identical vertices.
                for (a, b) in lit_q.iter().zip(plain_q.iter()) {
                    assert_eq!(a, b);
                }
            }
        }
    }
}
