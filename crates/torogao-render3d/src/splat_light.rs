//! Splat layer light interaction (splat programme
//! Rung 4): the two honest interactions a volumetric
//! layer can have with the scene's light, both
//! computed at pack time from data the renderer
//! already has. DESIGN.md §1 named the seam this rung
//! closes: the character is lit by the scene's lights,
//! the splats by their capture. This rung does NOT
//! give splats a surface lighting model — a Gaussian
//! has no surface, and inventing one would falsify
//! the capture.
//!
//! 1. **Shadow receive.** Each splat centre is tested
//!    against the scene's shadow map: a CPU
//!    rasterisation of the scene's shadow casters
//!    (the same geometry the backend's shadow pass
//!    draws: visible character meshes + set pieces)
//!    under the same light view-projection
//!    (`backend::light_view_proj`), sampled with the
//!    same rule the toon and set shaders apply
//!    (`shadow_factor` in `shaders/character.frag` /
//!    `shaders/set.frag`): project the point, outside
//!    the frustum is lit, otherwise a 3x3 PCF compare
//!    against the NEAREST-sampled depth map with the
//!    shaders' 0.0008 depth bias. The one shader term
//!    with no centre analogue is the 0.005 m
//!    normal-offset bias — a centre has no normal;
//!    it is a surface-acne bias and is omitted here,
//!    named rather than silently dropped. The
//!    evaluated SH colour is multiplied by the
//!    resulting visibility, per splat centre: a large
//!    splat is lit by its centre (the DESIGN's L2
//!    centre-depth limit, in the light domain).
//! 2. **Light response.** A per-layer colour grade
//!    declared in the spec and frozen with it: a 3x3
//!    matrix on the evaluated colour, then a mix
//!    toward the scene's world (ambient) colour as a
//!    multiplicative tint —
//!    `tint = (1 - mix) * white + mix * world`,
//!    `colour' = tint * (grade * colour)`, clamped
//!    below at zero like the SH evaluation itself.
//!    A pure function of (spec, scene world colour):
//!    the same spec compiles to the same packet.
//!
//! Both interactions are opt-in per layer. A layer
//! that names neither packs bit-identically to
//! Rung 3 (the pack's light hook is skipped whole,
//! not applied as an identity).
//!
//! Explicitly not here (programme Rung 4, stated
//! before building): splats casting shadows,
//! specular response, per-pixel relighting.

use crate::math::Mat4;
use crate::setscene::ArcScene;

/// Edge length of the CPU shadow map. Mirrors
/// `SHADOW_SIZE` in `backend::vulkan` (the GPU shadow
/// pass's map), so the centre test samples at the
/// same texel density the surfaces get.
pub const SHADOW_MAP_SIZE: u32 = 2048;

/// The shaders' PCF depth bias (`proj.z - 0.0008 <=
/// depth` in both `shadow_factor` implementations).
const SHADOW_DEPTH_BIAS: f64 = 0.0008;

/// A per-layer light response (Rung 4 interaction 2):
/// a 3x3 colour grade plus a mix toward the scene's
/// world colour. Declared in the scene spec and
/// frozen with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightResponse {
    /// Row-major 3x3: `out[i] = sum_j grade[i][j] *
    /// colour[j]`, in linear RGB.
    pub grade: [[f64; 3]; 3],
    /// 0 = the grade alone; 1 = the graded colour
    /// fully tinted by the scene's world colour.
    pub world_mix: f64,
}

impl LightResponse {
    /// Apply the grade and the world tint to an
    /// evaluated (linear, >= 0) SH colour. Pure.
    pub fn apply(&self, colour: [f32; 3], world: [f32; 3]) -> [f32; 3] {
        let c = [
            f64::from(colour[0]),
            f64::from(colour[1]),
            f64::from(colour[2]),
        ];
        let mut out = [0.0_f64; 3];
        for i in 0..3 {
            let graded =
                self.grade[i][0] * c[0] + self.grade[i][1] * c[1] + self.grade[i][2] * c[2];
            let tint = (1.0 - self.world_mix) + self.world_mix * f64::from(world[i]);
            out[i] = (graded * tint).max(0.0);
        }
        [out[0] as f32, out[1] as f32, out[2] as f32]
    }
}

/// The scene's shadow map, rasterised on the CPU at
/// pack time: the casters' depth from the sun, in the
/// same projection and depth convention the backend's
/// shadow pass produces (NDC z in [0, 1], 0 at the
/// light's near plane; empty texels hold 1.0, the
/// pass's clear value). A pure function of (caster
/// triangles, light view-projection, size): identical
/// inputs give an identical map on any machine — the
/// rasteriser is single-threaded, min-depth only, no
/// atomics, fixed fill rule (pixel centre inside the
/// triangle, barycentric coordinates all >= 0).
#[derive(Debug, Clone, PartialEq)]
pub struct SplatShadowMap {
    size: u32,
    light_view_proj: Mat4,
    /// Row-major depths, `size * size`: row =
    /// floor((ndc.y * 0.5 + 0.5) * size), column =
    /// floor((ndc.x * 0.5 + 0.5) * size) — the
    /// framebuffer/sampler indexing the Vulkan pass
    /// and the shaders' `texture()` call share.
    depths: Vec<f32>,
}

fn apply_mat(m: &Mat4, v: [f64; 4]) -> [f64; 4] {
    // Mat4 storage is column-major: element (row i,
    // col j) lives at m[j * 4 + i].
    let mut out = [0.0; 4];
    for i in 0..4 {
        out[i] = m.m[i] * v[0] + m.m[4 + i] * v[1] + m.m[8 + i] * v[2] + m.m[12 + i] * v[3];
    }
    out
}

impl SplatShadowMap {
    /// Rasterise world-space caster triangles into a
    /// depth map from the sun. Triangles are clipped
    /// only by the rasteriser's bounds (a triangle
    /// crossing the frustum edge still covers the
    /// texels inside it, as the clipped GPU primitive
    /// does); interpolated depths are clamped to
    /// [0, 1], the depth range the pass can store.
    pub fn rasterise(
        triangles: &[[[f64; 3]; 3]],
        light_view_proj: Mat4,
        size: u32,
    ) -> SplatShadowMap {
        assert!(size > 0, "shadow map size must be positive");
        let mut depths = vec![1.0_f32; (size as usize) * (size as usize)];
        let s = f64::from(size);
        for tri in triangles {
            // Project to light NDC, then to texel
            // space (pixel centres at k + 0.5).
            let mut px = [[0.0; 2]; 3];
            let mut pz = [0.0; 3];
            let mut degenerate = false;
            for (k, v) in tri.iter().enumerate() {
                let clip = apply_mat(&light_view_proj, [v[0], v[1], v[2], 1.0]);
                if clip[3] == 0.0 {
                    degenerate = true;
                    break;
                }
                let ndc = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
                px[k] = [(ndc[0] * 0.5 + 0.5) * s, (ndc[1] * 0.5 + 0.5) * s];
                pz[k] = ndc[2];
            }
            if degenerate {
                continue;
            }
            let (ax, ay) = (px[0][0], px[0][1]);
            let (bx, by) = (px[1][0], px[1][1]);
            let (cx, cy) = (px[2][0], px[2][1]);
            let area2 = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            if area2 == 0.0 {
                continue;
            }
            let min_x = ax.min(bx).min(cx).floor().max(0.0) as u32;
            let max_x = ax.max(bx).max(cx).ceil().min(s) as u32;
            let min_y = ay.min(by).min(cy).floor().max(0.0) as u32;
            let max_y = ay.max(by).max(cy).ceil().min(s) as u32;
            for row in min_y..max_y {
                for col in min_x..max_x {
                    let (x, y) = (f64::from(col) + 0.5, f64::from(row) + 0.5);
                    let l0 = ((bx - x) * (cy - y) - (by - y) * (cx - x)) / area2;
                    let l1 = ((cx - x) * (ay - y) - (cy - y) * (ax - x)) / area2;
                    let l2 = 1.0 - l0 - l1;
                    if l0 < 0.0 || l1 < 0.0 || l2 < 0.0 {
                        continue;
                    }
                    let z = (l0 * pz[0] + l1 * pz[1] + l2 * pz[2]).clamp(0.0, 1.0) as f32;
                    let idx = (row * size + col) as usize;
                    if z < depths[idx] {
                        depths[idx] = z;
                    }
                }
            }
        }
        SplatShadowMap {
            size,
            light_view_proj,
            depths,
        }
    }

    /// The map's edge length in texels.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Sun visibility at a world-space point, in
    /// [0, 1]: the shaders' `shadow_factor` with the
    /// normal-offset bias omitted (a centre has no
    /// normal — see the module docs). Points outside
    /// the light frustum are fully lit, as in the
    /// shaders.
    pub fn visibility(&self, world_pos: [f64; 3]) -> f32 {
        let clip = apply_mat(
            &self.light_view_proj,
            [world_pos[0], world_pos[1], world_pos[2], 1.0],
        );
        if clip[3] == 0.0 {
            return 1.0;
        }
        let proj = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
        let uv = [proj[0] * 0.5 + 0.5, proj[1] * 0.5 + 0.5];
        if uv[0] < 0.0 || uv[0] > 1.0 || uv[1] < 0.0 || uv[1] > 1.0 || proj[2] > 1.0 {
            return 1.0;
        }
        let s = f64::from(self.size);
        let max_texel = self.size as i64 - 1;
        let mut lit = 0.0_f64;
        for oy in -1_i32..=1 {
            for ox in -1_i32..=1 {
                // The shader samples uv + (ox, oy) *
                // texel with a NEAREST, CLAMP_TO_EDGE
                // sampler: the texel read is
                // floor(sample_uv * size), clamped.
                let su = uv[0] + f64::from(ox) / s;
                let sv = uv[1] + f64::from(oy) / s;
                let col = (su * s).floor() as i64;
                let row = (sv * s).floor() as i64;
                let col = col.clamp(0, max_texel) as u32;
                let row = row.clamp(0, max_texel) as u32;
                let depth = self.depths[(row * self.size + col) as usize];
                if proj[2] - SHADOW_DEPTH_BIAS <= f64::from(depth) {
                    lit += 1.0;
                }
            }
        }
        (lit / 9.0) as f32
    }
}

/// The light interaction a pack applies to one layer
/// (Rung 4). Both fields absent = the Rung 3 pack,
/// bit-identical: the pack checks `is_active` and
/// skips the hook whole rather than applying an
/// identity transform.
#[derive(Debug, Clone, Copy, Default)]
pub struct SplatLight<'a> {
    /// Sun shadow map sampled at splat centres. The
    /// caller builds it from the scene's casters (see
    /// [`set_piece_triangles`] and
    /// [`mesh_triangles`]); the pack only samples it.
    pub shadow: Option<&'a SplatShadowMap>,
    /// The spec-declared light response.
    pub response: Option<LightResponse>,
}

impl SplatLight<'_> {
    /// True when at least one interaction is named.
    pub fn is_active(&self) -> bool {
        self.shadow.is_some() || self.response.is_some()
    }

    /// Apply the named interactions to one splat's
    /// evaluated colour at its world centre: grade
    /// first (it defines the layer's response to the
    /// rig), then the centre's sun visibility (a
    /// scalar; it commutes with the grade's tint but
    /// the order is fixed so the function has one
    /// definition).
    pub fn apply(&self, colour: [f32; 3], centre: [f64; 3], world: [f32; 3]) -> [f32; 3] {
        let mut colour = colour;
        if let Some(response) = &self.response {
            colour = response.apply(colour, world);
        }
        if let Some(shadow) = self.shadow {
            let v = shadow.visibility(centre);
            colour = [colour[0] * v, colour[1] * v, colour[2] * v];
        }
        colour
    }
}

/// World-space triangles of a set piece list at one
/// frame: local positions carried by each piece's
/// model matrix. This is the set half of the shadow
/// pass's caster set (the pass draws the pieces
/// visible at the frame when the draw includes the
/// set).
pub fn set_piece_triangles(scene: &ArcScene, frame: u32) -> Vec<[[f64; 3]; 3]> {
    let mut triangles = Vec::new();
    for piece in &scene.pieces {
        if !piece.visible(frame) {
            continue;
        }
        let world: Vec<[f64; 3]> = piece
            .positions
            .iter()
            .map(|p| {
                let v = apply_mat(
                    &piece.model,
                    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2]), 1.0],
                );
                [v[0], v[1], v[2]]
            })
            .collect();
        for idx in piece.indices.chunks_exact(3) {
            triangles.push([
                world[idx[0] as usize],
                world[idx[1] as usize],
                world[idx[2] as usize],
            ]);
        }
    }
    triangles
}

/// World-space triangles from a posed mesh's
/// positions + index list (the character half of the
/// caster set: feed it the CPU reference skinning's
/// output, `reference::skin_mesh`, for the meshes the
/// frame draws).
pub fn mesh_triangles(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[[f64; 3]; 3]> {
    let mut triangles = Vec::with_capacity(indices.len() / 3);
    for idx in indices.chunks_exact(3) {
        let p = |i: u32| {
            let v = positions[i as usize];
            [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
        };
        triangles.push([p(idx[0]), p(idx[1]), p(idx[2])]);
    }
    triangles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::V3;

    /// Sun directly overhead (to_light = +Y), the
    /// light view-projection the backend would build
    /// for it. Frustum: half-extent 2.6 m around
    /// (0, 0.85, 0), near 1 / far 12 from an eye 6 m
    /// above the centre.
    fn overhead_vp() -> Mat4 {
        crate::backend::light_view_proj(V3::new(0.0, 1.0, 0.0))
    }

    /// A horizontal quad at height `y`, half-extent
    /// `h` around the origin: two triangles.
    fn quad(y: f64, h: f64) -> Vec<[[f64; 3]; 3]> {
        let c = [[-h, y, -h], [h, y, -h], [h, y, h], [-h, y, h]];
        vec![[c[0], c[1], c[2]], [c[0], c[2], c[3]]]
    }

    #[test]
    fn empty_map_lights_everything() {
        let map = SplatShadowMap::rasterise(&[], overhead_vp(), 64);
        for p in [[0.0, 0.0, 0.0], [1.0, 2.0, -1.0], [0.0, 0.85, 0.0]] {
            assert_eq!(map.visibility(p), 1.0, "no casters: {p:?} is lit");
        }
    }

    #[test]
    fn caster_overhead_shadows_the_point_below_it() {
        // Quad at y = 2 (between the overhead sun and
        // the ground): the origin is in full shadow,
        // a point above the quad is lit, a point
        // beside the quad is lit.
        let map = SplatShadowMap::rasterise(&quad(2.0, 1.0), overhead_vp(), 256);
        assert_eq!(map.visibility([0.0, 0.0, 0.0]), 0.0);
        assert_eq!(map.visibility([0.0, 3.0, 0.0]), 1.0);
        assert_eq!(map.visibility([2.0, 0.0, 0.0]), 1.0);
    }

    #[test]
    fn point_outside_the_light_frustum_is_lit() {
        // The frustum's half-extent is 2.6 m around
        // (0, 0.85, 0); far outside it the shader
        // rule returns 1.0 even with a caster present.
        let map = SplatShadowMap::rasterise(&quad(2.0, 10.0), overhead_vp(), 64);
        assert_eq!(map.visibility([20.0, 0.0, 0.0]), 1.0);
    }

    #[test]
    fn pcf_edge_gives_partial_visibility() {
        // A centre near the quad's edge: some of the
        // 3x3 taps land on covered texels, some not.
        let map = SplatShadowMap::rasterise(&quad(2.0, 1.0), overhead_vp(), 256);
        let mut saw_partial = false;
        for step in 0..40 {
            let x = 0.9 + f64::from(step) * 0.01;
            let v = map.visibility([x, 0.0, 0.0]);
            if v > 0.0 && v < 1.0 {
                saw_partial = true;
            }
        }
        assert!(saw_partial, "the quad edge must produce a PCF gradient");
    }

    #[test]
    fn rasterise_is_deterministic() {
        let tris = quad(2.0, 1.0);
        let a = SplatShadowMap::rasterise(&tris, overhead_vp(), 128);
        let b = SplatShadowMap::rasterise(&tris, overhead_vp(), 128);
        assert_eq!(a, b);
    }

    #[test]
    fn nearest_caster_wins() {
        // Two quads stacked along the light: the
        // nearer one's depth is what the compare
        // sees — a point between them is lit by the
        // far quad's texels only if the near quad
        // does not cover it; here both cover, and the
        // point below both is shadowed either way,
        // while a point between the quads is shadowed
        // by the upper one (it is behind it).
        let mut tris = quad(2.0, 1.0);
        tris.extend(quad(3.0, 1.0));
        let map = SplatShadowMap::rasterise(&tris, overhead_vp(), 256);
        assert_eq!(map.visibility([0.0, 2.5, 0.0]), 0.0);
        assert_eq!(map.visibility([0.0, 3.5, 0.0]), 1.0);
    }

    #[test]
    fn grade_identity_and_zero_mix_is_exact() {
        let response = LightResponse {
            grade: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            world_mix: 0.0,
        };
        let c = [0.3, 0.55, 0.8];
        assert_eq!(response.apply(c, [0.16, 0.15, 0.19]), c);
    }

    #[test]
    fn grade_swaps_channels_and_mix_tints_by_world() {
        let swap = LightResponse {
            grade: [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            world_mix: 0.0,
        };
        assert_eq!(swap.apply([0.25, 0.5, 0.75], [0.0; 3]), [0.75, 0.5, 0.25]);
        let full_mix = LightResponse {
            grade: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            world_mix: 1.0,
        };
        assert_eq!(
            full_mix.apply([0.4, 0.4, 0.4], [0.5, 0.25, 0.0]),
            [0.2, 0.1, 0.0]
        );
        let half_mix = LightResponse {
            world_mix: 0.5,
            ..full_mix
        };
        let out = half_mix.apply([0.4, 0.4, 0.4], [0.5, 0.25, 0.0]);
        assert!((out[0] - 0.3).abs() < 1e-6, "{out:?}");
        assert!((out[1] - 0.25).abs() < 1e-6, "{out:?}");
        assert!((out[2] - 0.2).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn grade_clamps_below_zero() {
        let response = LightResponse {
            grade: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            world_mix: 0.0,
        };
        assert_eq!(response.apply([0.5, 0.5, 0.5], [0.0; 3]), [0.0, 0.5, 0.5]);
    }

    #[test]
    fn set_piece_triangles_apply_the_model_matrix() {
        use crate::setscene::{SetMaterial, SetPiece};
        let scene = ArcScene {
            world: [0.0; 3],
            sun: crate::setscene::DirectionalLight {
                to_light: V3::new(0.0, 1.0, 0.0),
                energy: 1.0,
            },
            fill: crate::setscene::DirectionalLight {
                to_light: V3::new(0.0, 1.0, 0.0),
                energy: 1.0,
            },
            view: Mat4::IDENTITY,
            tan_half_x: 0.5,
            tan_half_y: 0.5,
            near: 0.1,
            far: 100.0,
            pieces: vec![SetPiece {
                name: "probe",
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: vec![[0.0, 0.0, 1.0]; 3],
                indices: vec![0, 1, 2],
                material: SetMaterial {
                    albedo: [0.0; 3],
                    roughness: 0.0,
                    emission: [0.0; 3],
                    emission_strength: 0.0,
                },
                model: Mat4::from_rows([
                    [1.0, 0.0, 0.0, 10.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ]),
                is_toy: false,
                visible_from: 0,
                visible_until: 10,
            }],
        };
        let tris = set_piece_triangles(&scene, 0);
        assert_eq!(tris.len(), 1);
        assert_eq!(tris[0][0], [10.0, 0.0, 0.0]);
        // Outside the visibility window: no casters.
        assert!(set_piece_triangles(&scene, 99).is_empty());
    }
}
