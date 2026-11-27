//! GPU splat projection + per-frame sort (DESIGN.md
//! §6.1) and the Rung 5 animation data built on it.
//!
//! The CPU pack (`crate::splat`) projects and sorts on
//! the host: measured 439.7 ms (Rung 2) / 442.0 ms
//! (Rung 4 tip) at 1,157,141 splats — a ~2.3 fps
//! per-frame re-pack against the programme's 24 fps
//! premise, and the measured blocker Rung 5 stopped
//! on. This module is the data half of the cure: the
//! projection and the depth sort move on-device
//! (Vulkan compute, fp64 — the pack's arithmetic is
//! f64 and an f32 projection would diverge from it by
//! construction), driven from a resident record buffer
//! plus a per-layer parameter block, with NO CPU
//! re-pack per frame. The backend half (buffers,
//! pipelines, dispatch) lives in `backend::vulkan`.
//!
//! Determinism is the hard gate, not a hope. The
//! on-device path reproduces the pack's contract
//! exactly:
//!
//! - The same projection arithmetic, in f64, with the
//!   same operation order (the shader marks its
//!   accumulations `precise`; the transcendental-free
//!   projection needs only `sqrt`, which is correctly
//!   rounded in fp64 on both sides). The two
//!   transcendentals the pack does need — the opacity
//!   sigmoid and the scale exponentials — are per-splat
//!   constants, so they are evaluated ONCE on the host
//!   by the pack's own functions (`Splat::opacity`,
//!   `Splat::scale`) at upload and stored activated in
//!   the record; the shader never approximates them.
//! - The same culls, in the same order.
//! - The same total order: view depth descending
//!   under `f64::total_cmp`, asset index ascending.
//!   On-device this is a 96-bit radix key — the
//!   bitwise NOT of the depth's total-order sortable
//!   u64 over the asset index — sorted by a
//!   deterministic LSD radix sort (per-block
//!   histograms, a single-workgroup scan, and a
//!   local-rank scatter; no atomic ever decides a
//!   position). Culled splats carry the all-ones depth
//!   key (greater than every real key: a surviving
//!   depth is positive, so its NOT-sortable key keeps
//!   a clear top bit) and sort after every survivor;
//!   the survivor count
//!   (an atomic tally whose VALUE is order-free)
//!   bounds the draw.
//! - The same light interaction (Rung 4): the grade
//!   and the centre shadow test are evaluated in the
//!   projection shader from the same inputs the pack
//!   reads (the shadow map's depths upload verbatim).
//!
//! The gate instrument is the snapshot: the backend
//! can read the sorted projected records back, and
//! the proving battery compares them record-for-record
//! against `crate::splat::project_splats` — the pack's
//! own projection, public for exactly this purpose.
//!
//! Animation (Rung 5) rides on this path, per D-1397
//! (shipped animation is code; a frozen hashed scene
//! spec): a layer's transform is evaluated per frame
//! from the spec's motion keys ([`LayerMotion`]) and
//! pushed as parameters — the resident records never
//! move — and a hash-frozen `.ply` sequence is
//! resident as concatenated record frames (the
//! resident sequence format is this module's
//! [`GpuSplatRecord`] stream: one 256-byte record per
//! splat, frames concatenated in spec order, frame
//! `f` selected by a record offset at dispatch).

use bytemuck::{Pod, Zeroable};

use crate::math::Mat4;
use crate::setscene::ArcScene;
use crate::splat::{Splat, SplatCloud, SplatProjection, SplatTransform};
use crate::splat_light::{LightResponse, SplatShadowMap};
use crate::splat_rotate::{Rotation, ShRotation, is_identity_quat};

/// One splat's resident GPU record (std430, 256
/// bytes): the asset data the projection shader
/// reads, with the opacity and scales pre-activated
/// on the host by the pack's own functions (see the
/// module docs for why). A sequence is these records
/// concatenated frame after frame.
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct GpuSplatRecord {
    /// Centre (asset space) + activated opacity.
    pub pos_opacity: [f32; 4],
    /// SH band 0 per channel + padding.
    pub dc_pad: [f32; 4],
    /// Activated scales + padding.
    pub scale_pad: [f32; 4],
    /// Rotation quaternion, `(w, x, y, z)` — the
    /// splat storage order.
    pub rot: [f32; 4],
    /// SH bands 1–3, channel-major, as stored
    /// (three channels of 15; the flat 45 floats the
    /// shader reads).
    pub rest: [[f32; 15]; 3],
    /// Tail padding to the 256-byte record.
    pub tail: [f32; 3],
}

impl GpuSplatRecord {
    /// Convert one asset splat. The activations are
    /// the pack's own (`Splat::opacity`,
    /// `Splat::scale`), so the values the shader
    /// starts from are bit-identical to the values
    /// the pack computes.
    pub fn from_splat(splat: &Splat) -> GpuSplatRecord {
        let scale = splat.scale();
        GpuSplatRecord {
            pos_opacity: [splat.pos[0], splat.pos[1], splat.pos[2], splat.opacity()],
            dc_pad: [splat.dc[0], splat.dc[1], splat.dc[2], 0.0],
            scale_pad: [scale[0], scale[1], scale[2], 0.0],
            rot: splat.rot,
            rest: [
                splat.rest[0..15].try_into().expect("channel span"),
                splat.rest[15..30].try_into().expect("channel span"),
                splat.rest[30..45].try_into().expect("channel span"),
            ],
            tail: [0.0; 3],
        }
    }
}

/// A cloud's resident records, in asset order.
pub fn frame_records(cloud: &SplatCloud) -> Vec<GpuSplatRecord> {
    cloud
        .splats
        .iter()
        .map(GpuSplatRecord::from_splat)
        .collect()
}

/// One projected record as the GPU path writes it
/// (std430, 96 bytes): the on-device twin of
/// [`SplatProjection`], in the same field order the
/// projection shader fills. The second axis is not
/// stored: it is the first axis rotated 90°, exactly
/// as the pack derives it, and the gather step
/// re-derives it in f64.
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct ProjectedRecord {
    /// Quad centre, pixels.
    pub centre_px: [f64; 2],
    /// Quad radii along the two axes (3σ).
    pub radii: [f64; 2],
    /// First eigen-axis (unit, pixel space).
    pub axis1: [f64; 2],
    /// Inverse covariance (a, b, c).
    pub conic: [f32; 3],
    /// Activated opacity.
    pub opacity: f32,
    /// Evaluated SH colour (light applied when the
    /// layer names it), linear, clamped at 0.
    pub colour: [f32; 3],
    /// NDC depth of the centre.
    pub z_ndc: f32,
    /// Asset index of the splat.
    pub index: u32,
    /// 1 when the projection culled this splat (its
    /// sort key is the all-ones depth key; it never
    /// draws).
    pub culled: u32,
    /// Tail padding.
    pub tail: [u32; 2],
}

impl ProjectedRecord {
    /// The GPU record the CPU projection implies, for
    /// the record-for-record gate.
    pub fn from_projection(p: &SplatProjection) -> ProjectedRecord {
        ProjectedRecord {
            centre_px: p.centre_px,
            radii: p.radii,
            axis1: p.axes[0],
            conic: p.conic,
            opacity: p.opacity,
            colour: p.colour,
            z_ndc: p.z_ndc,
            index: p.index as u32,
            culled: 0,
            tail: [0; 2],
        }
    }
}

/// The sort key of one splat as three radix words,
/// low word first: `[asset index, depth key low,
/// depth key high]`, where the depth key is the
/// bitwise NOT of the depth's total-order sortable
/// u64. Ascending radix order over these words is
/// the pack's total order (depth descending under
/// `total_cmp`, index ascending) — including the
/// NaN and signed-zero corners, which the sortable
/// transform orders exactly as `total_cmp` does.
pub fn depth_key_words(depth: f64, index: u32) -> [u32; 3] {
    let bits = depth.to_bits();
    let sortable = bits
        ^ if bits >> 63 == 1 {
            u64::MAX
        } else {
            1_u64 << 63
        };
    let descending = !sortable;
    [index, descending as u32, (descending >> 32) as u32]
}

/// The camera a GPU splat layer is compiled for: the
/// f64 view data the projection reads, carried in the
/// packet because the boundary's scenes are
/// camera-bound at upload (the CPU packet is too —
/// it bakes the camera into the quad stream; this
/// packet keeps the camera as data and bakes nothing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplatCamera {
    /// World -> view, column-major f64.
    pub view: Mat4,
    /// Projection * view, column-major f64.
    pub view_proj: Mat4,
    /// Camera position, world space.
    pub camera_pos: [f64; 3],
    /// tan(fov/2) horizontally.
    pub tan_half_x: f64,
    /// tan(fov/2) vertically.
    pub tan_half_y: f64,
    /// Near plane (the depth cull).
    pub near: f64,
    /// Viewport width, pixels.
    pub width: u32,
    /// Viewport height, pixels.
    pub height: u32,
}

impl SplatCamera {
    /// From the scene the pack projects for: the same
    /// expressions `pack_impl` evaluates (projection
    /// times view; camera from the inverse view's
    /// translation).
    pub fn from_scene(scene: &ArcScene, width: u32, height: u32) -> SplatCamera {
        let view_proj = crate::setscene::projection(scene).multiply(scene.view);
        let cam = scene.view.inverse().translation();
        SplatCamera {
            view: scene.view,
            view_proj,
            camera_pos: [cam.x, cam.y, cam.z],
            tan_half_x: scene.tan_half_x,
            tan_half_y: scene.tan_half_y,
            near: scene.near,
            width,
            height,
        }
    }
}

/// A GPU splat layer at the backend boundary: the
/// resident sequence (one cloud = a static layer),
/// its base transform and camera, and its Rung 4
/// light interaction. `PackedScene::splat_gpu`
/// carries it; the CPU packet (`PackedScene::splats`)
/// is untouched and stays the reference + fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedGpuSplats {
    /// Resident frames, in spec order. Frame 0 is
    /// the layer's still.
    pub frames: Vec<SplatCloud>,
    /// The layer's base transform (motion keys, when
    /// the spec names them, replace it per frame via
    /// `RenderBackend::set_splat_transform`).
    pub transform: SplatTransform,
    /// The camera the layer is compiled for.
    pub camera: SplatCamera,
    /// The scene's sun shadow map, when the layer
    /// receives shadow.
    pub shadow: Option<SplatShadowMap>,
    /// The spec-declared light response, when named.
    pub response: Option<LightResponse>,
}

/// The backend's readback of one on-device
/// projection + sort + gather: the surviving
/// projected records in draw order (the pack's total
/// order), the quad stream the gather emitted for
/// them, and the indirect draw's index count.
#[derive(Debug, Clone, PartialEq)]
pub struct SplatGpuSnapshot {
    /// Surviving records, sorted.
    pub records: Vec<ProjectedRecord>,
    /// How many splats survived the culls.
    pub survivor_count: usize,
    /// The emitted quad stream (4 vertices per
    /// surviving record, in draw order): the gather's
    /// output, gated against the CPU pack's vertices.
    pub stream: Vec<crate::splat::SplatVertex>,
    /// The indirect draw command's index count
    /// (survivors * 6 when the gather ran).
    pub draw_index_count: u32,
}

/// The projection shader's parameter block (std430,
/// 1408 bytes): everything one frame's projection
/// needs beyond the resident records — camera, layer
/// TRS, the layer rotation's Wigner-D matrices (when
/// the layer rotates), and the light interaction.
/// Built on the host in f64; the layout is shared
/// verbatim with `shaders/splat_project.comp` and
/// pinned by the layout test below.
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct SplatGpuParams {
    /// View matrix, column-major (16 doubles).
    pub view: [f64; 16],
    /// View-projection, column-major (16 doubles).
    pub view_proj: [f64; 16],
    /// Layer rotation matrix, row-major (9 doubles).
    pub rot_mat: [f64; 9],
    /// Wigner-D band matrices, flattened as
    /// `ShRotation::matrices_flat` (83 doubles, in
    /// five 16-double chunks + a 3-double tail: the
    /// flat array the shader reads, chunked only
    /// because bytemuck's `Pod` arrays cap at 32).
    pub sh_mats: [[f64; 16]; 5],
    /// The flat Wigner-D array's last 3 doubles.
    pub sh_tail: [f64; 3],
    /// Shadow light view-projection, column-major
    /// (16 doubles; zero when no shadow).
    pub light_vp: [f64; 16],
    /// Camera position xyz + near plane.
    pub cam: [f64; 4],
    /// tan_half_x, tan_half_y, viewport w, h.
    pub viewp: [f64; 4],
    /// Layer translation xyz + uniform scale.
    pub trs_a: [f64; 4],
    /// Layer rotation quaternion `(w, x, y, z)`.
    pub trs_q: [f64; 4],
    /// Grade row 0 xyz + world_mix.
    pub grade0: [f64; 4],
    /// Grade row 1 xyz + world colour r.
    pub grade1: [f64; 4],
    /// Grade row 2 xyz + world colour g.
    pub grade2: [f64; 4],
    /// World colour b + padding.
    pub misc: [f64; 4],
    /// rotate, has_shadow, has_response, shadow size.
    pub flags: [u32; 4],
    /// Tail padding to the std430 struct size.
    pub tail: [u32; 4],
}

impl SplatGpuParams {
    /// Build the block for one frame: camera + layer
    /// transform + the scene's world colour + the
    /// layer's light interaction. The rotation data
    /// mirrors the pack's branch exactly: an identity
    /// rotation (either sign) sets `rotate = 0` and
    /// the shader takes the unrotated path, as the
    /// pack skips `rotate_cloud` whole.
    pub fn new(
        camera: &SplatCamera,
        transform: &SplatTransform,
        world: [f32; 3],
        response: Option<LightResponse>,
        shadow: Option<&SplatShadowMap>,
    ) -> SplatGpuParams {
        let rotate = !is_identity_quat(transform.rotation);
        let (rot_mat, sh_flat) = if rotate {
            let rotation = Rotation::from_quaternion(transform.rotation)
                .expect("SplatTransform.rotation must be a finite, non-zero quaternion");
            let sh = ShRotation::new(&rotation.matrix);
            let mut flat_mat = [0.0_f64; 9];
            for r in 0..3 {
                for c in 0..3 {
                    flat_mat[r * 3 + c] = rotation.matrix[r][c];
                }
            }
            (flat_mat, sh.matrices_flat())
        } else {
            ([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], [0.0; 83])
        };
        let mut sh_mats = [[0.0_f64; 16]; 5];
        let mut sh_tail = [0.0_f64; 3];
        for (i, v) in sh_flat.iter().enumerate() {
            if i < 80 {
                sh_mats[i / 16][i % 16] = *v;
            } else {
                sh_tail[i - 80] = *v;
            }
        }
        let (grade, world_mix) = match response {
            Some(r) => (r.grade, r.world_mix),
            None => ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], 0.0),
        };
        SplatGpuParams {
            view: camera.view.m,
            view_proj: camera.view_proj.m,
            rot_mat,
            sh_mats,
            sh_tail,
            light_vp: shadow.map(|s| s.light_view_proj().m).unwrap_or([0.0; 16]),
            cam: [
                camera.camera_pos[0],
                camera.camera_pos[1],
                camera.camera_pos[2],
                camera.near,
            ],
            viewp: [
                camera.tan_half_x,
                camera.tan_half_y,
                f64::from(camera.width),
                f64::from(camera.height),
            ],
            trs_a: [
                transform.translation[0],
                transform.translation[1],
                transform.translation[2],
                transform.scale,
            ],
            trs_q: transform.rotation,
            grade0: [grade[0][0], grade[0][1], grade[0][2], world_mix],
            grade1: [grade[1][0], grade[1][1], grade[1][2], f64::from(world[0])],
            grade2: [grade[2][0], grade[2][1], grade[2][2], f64::from(world[1])],
            misc: [f64::from(world[2]), 0.0, 0.0, 0.0],
            flags: [
                u32::from(rotate),
                u32::from(shadow.is_some()),
                u32::from(response.is_some()),
                shadow.map(|s| s.size()).unwrap_or(0),
            ],
            tail: [0; 4],
        }
    }
}

/// One motion key of a layer's TRS animation: the
/// layer transform at `frame`. Rotation is the
/// storage order `(w, x, y, z)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrsKey {
    /// Frame this key sits on.
    pub frame: u32,
    /// Layer translation.
    pub translation: [f64; 3],
    /// Layer rotation quaternion `(w, x, y, z)`.
    pub rotation: [f64; 4],
    /// Layer uniform scale.
    pub scale: f64,
}

/// A layer's TRS animation, frozen in the scene
/// spec (Rung 5): keys evaluated per frame in f64 —
/// translation and scale lerp, rotation slerps —
/// clamped outside the key range. Pure: the same
/// frame always evaluates to the same transform,
/// which is what makes the animated path's twin-run
/// gate a statement about the renderer, not the
/// clock (D-1397).
#[derive(Debug, Clone, PartialEq)]
pub struct LayerMotion {
    keys: Vec<TrsKey>,
}

impl LayerMotion {
    /// From keys in any order (sorted here; on a
    /// duplicate frame the later key wins). `None`
    /// for an empty key list — a motion with no keys
    /// is not a motion.
    pub fn new(mut keys: Vec<TrsKey>) -> Option<LayerMotion> {
        if keys.is_empty() {
            return None;
        }
        keys.sort_by_key(|k| k.frame);
        let mut deduped: Vec<TrsKey> = Vec::with_capacity(keys.len());
        for key in keys {
            match deduped.last_mut() {
                Some(kept) if kept.frame == key.frame => *kept = key,
                _ => deduped.push(key),
            }
        }
        Some(LayerMotion { keys: deduped })
    }

    /// The keys, in frame order.
    pub fn keys(&self) -> &[TrsKey] {
        &self.keys
    }

    /// The layer transform at `frame`.
    pub fn transform_at(&self, frame: u32) -> SplatTransform {
        let keys = &self.keys;
        if frame <= keys[0].frame {
            return key_transform(&keys[0]);
        }
        let last = &keys[keys.len() - 1];
        if frame >= last.frame {
            return key_transform(last);
        }
        let hi = keys.partition_point(|k| k.frame <= frame);
        let (a, b) = (&keys[hi - 1], &keys[hi]);
        if frame == a.frame {
            return key_transform(a);
        }
        let t = f64::from(frame - a.frame) / f64::from(b.frame - a.frame);
        SplatTransform {
            translation: [
                lerp(a.translation[0], b.translation[0], t),
                lerp(a.translation[1], b.translation[1], t),
                lerp(a.translation[2], b.translation[2], t),
            ],
            rotation: slerp(a.rotation, b.rotation, t),
            scale: lerp(a.scale, b.scale, t),
        }
    }
}

fn key_transform(key: &TrsKey) -> SplatTransform {
    SplatTransform {
        translation: key.translation,
        rotation: key.rotation,
        scale: key.scale,
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Quaternion slerp in `(w, x, y, z)` order: the
/// short arc (a negative dot flips the second
/// quaternion), a normalised lerp for near-parallel
/// inputs, the sine form otherwise. Pure f64.
fn slerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let mut b = b;
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        b = [-b[0], -b[1], -b[2], -b[3]];
        dot = -dot;
    }
    if dot > 0.9995 {
        let mut out = [
            lerp(a[0], b[0], t),
            lerp(a[1], b[1], t),
            lerp(a[2], b[2], t),
            lerp(a[3], b[3], t),
        ];
        let n = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2] + out[3] * out[3]).sqrt();
        if n > 0.0 {
            out = [out[0] / n, out[1] / n, out[2] / n, out[3] / n];
        }
        return out;
    }
    let theta = dot.clamp(-1.0, 1.0).acos();
    let sin_theta = theta.sin();
    let wa = ((1.0 - t) * theta).sin() / sin_theta;
    let wb = (t * theta).sin() / sin_theta;
    [
        wa * a[0] + wb * b[0],
        wa * a[1] + wb * b[1],
        wa * a[2] + wb * b[2],
        wa * a[3] + wb * b[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_layout_matches_the_shader_block() {
        // The std430 layout splat_project.comp declares;
        // a drift here corrupts every projected frame.
        assert_eq!(std::mem::size_of::<SplatGpuParams>(), 1408);
        assert_eq!(std::mem::offset_of!(SplatGpuParams, view), 0);
        assert_eq!(std::mem::offset_of!(SplatGpuParams, view_proj), 128);
        assert_eq!(std::mem::offset_of!(SplatGpuParams, rot_mat), 256);
        assert_eq!(std::mem::offset_of!(SplatGpuParams, sh_mats), 328);
        assert_eq!(std::mem::offset_of!(SplatGpuParams, light_vp), 992);
        assert_eq!(std::mem::offset_of!(SplatGpuParams, cam), 1120);
        assert_eq!(std::mem::offset_of!(SplatGpuParams, flags), 1376);
        assert_eq!(std::mem::size_of::<GpuSplatRecord>(), 256);
        assert_eq!(std::mem::size_of::<ProjectedRecord>(), 96);
        assert_eq!(std::mem::offset_of!(ProjectedRecord, conic), 48);
        assert_eq!(std::mem::offset_of!(ProjectedRecord, colour), 64);
        assert_eq!(std::mem::offset_of!(ProjectedRecord, index), 80);
    }

    #[test]
    fn depth_key_orders_like_the_cpu_sort() {
        // Deterministic pseudo-random depths across the
        // corners: signs, zeros, subnormals, NaN.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut depths: Vec<f64> = (0..2048).map(|_| f64::from_bits(next())).collect();
        depths.extend([
            0.0,
            -0.0,
            f64::NAN,
            -f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1.0,
            1.0,
        ]);
        let mut by_cpu: Vec<u32> = (0..depths.len() as u32).collect();
        by_cpu.sort_by(|&a, &b| {
            depths[b as usize]
                .total_cmp(&depths[a as usize])
                .then_with(|| a.cmp(&b))
        });
        let mut by_key: Vec<u32> = (0..depths.len() as u32).collect();
        by_key.sort_by_key(|&i| {
            let w = depth_key_words(depths[i as usize], i);
            (w[2], w[1], w[0])
        });
        assert_eq!(by_cpu, by_key);
    }

    #[test]
    fn record_conversion_uses_the_packs_activations() {
        let splat = Splat {
            pos: [1.0, 2.0, 3.0],
            dc: [0.1, 0.2, 0.3],
            rest: [0.5; 45],
            opacity_logit: 0.25,
            log_scale: [-1.0, -2.0, -3.0],
            rot: [1.0, 0.0, 0.0, 0.0],
        };
        let record = GpuSplatRecord::from_splat(&splat);
        assert_eq!(record.pos_opacity, [1.0, 2.0, 3.0, splat.opacity()]);
        assert_eq!(record.scale_pad[..3], splat.scale());
        assert_eq!(record.dc_pad[..3], splat.dc);
        assert_eq!(record.rot, splat.rot);
        assert_eq!(record.rest[0], splat.rest[0..15]);
        assert_eq!(record.rest[1], splat.rest[15..30]);
        assert_eq!(record.rest[2], splat.rest[30..45]);
    }

    #[test]
    fn flat_sh_matrices_match_rotate_channel() {
        // The shader applies the flattened matrices;
        // they must reproduce ShRotation::rotate_channel
        // bit-for-bit (same accumulation order).
        let rotation = Rotation::from_quaternion([0.8, 0.3, -0.4, 0.387_298_334_620_741_7])
            .expect("non-zero quaternion");
        let sh = ShRotation::new(&rotation.matrix);
        let flat = sh.matrices_flat();
        let channel: [f64; 15] = [
            0.11, -0.23, 0.37, 0.41, -0.53, 0.59, -0.61, 0.67, 0.71, -0.73, 0.79, -0.83, 0.89,
            -0.97, 1.01,
        ];
        let expected = sh.rotate_channel(&channel);
        let mut got = [0.0_f64; 15];
        for (side, base, offset) in [(3_usize, 0_usize, 0_usize), (5, 3, 9), (7, 8, 34)] {
            for m in 0..side {
                let mut acc = 0.0;
                for n in 0..side {
                    acc += flat[offset + m * side + n] * channel[base + n];
                }
                got[base + m] = acc;
            }
        }
        assert_eq!(got, expected);
    }

    #[test]
    fn motion_endpoints_midpoints_and_clamps() {
        let motion = LayerMotion::new(vec![
            TrsKey {
                frame: 10,
                translation: [2.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 1.0, 0.0],
                scale: 2.0,
            },
            TrsKey {
                frame: 0,
                translation: [0.0, 0.0, 0.0],
                rotation: [1.0, 0.0, 0.0, 0.0],
                scale: 1.0,
            },
        ])
        .expect("keys");
        assert_eq!(motion.transform_at(0).translation, [0.0, 0.0, 0.0]);
        assert_eq!(motion.transform_at(10).scale, 2.0);
        assert_eq!(motion.transform_at(99).translation, [2.0, 0.0, 0.0]);
        let mid = motion.transform_at(5);
        assert_eq!(mid.translation, [1.0, 0.0, 0.0]);
        assert_eq!(mid.scale, 1.5);
        // Halfway between identity and 180° about Y is
        // 90° about Y: (cos45°, 0, sin45°, 0).
        let s = std::f64::consts::FRAC_1_SQRT_2;
        assert!((mid.rotation[0] - s).abs() < 1e-12);
        assert!((mid.rotation[2] - s).abs() < 1e-12);
        assert_eq!(mid.rotation[1], 0.0);
        assert_eq!(mid.rotation[3], 0.0);
        assert!(LayerMotion::new(vec![]).is_none());
    }
}
