//! Measurement-grade CPU rasterization of an evaluated puppet.
//!
//! The rasterizer reproduces the reference GL route's observable
//! math: world vertices are mesh verts plus deform (the shader's
//! `offset` uniform is never set), the camera maps model units to
//! pixels as `world * scale + viewport_center`, textures sample
//! bilinear with clamp-to-border, the fragment math is the shared
//! part/composite shader (`screenOut = 1 - (1 - tex) * (1 -
//! screen * tex.a)`, times tint, times opacity on all four
//! channels), and blending uses the seven INP1 blend modes with
//! per-draw quantization to RGBA8, as an RGBA8 framebuffer does.
//!
//! Coverage is pixel-center sampling, the same class of rule the
//! estate's CPU evaluator of record uses; per-frame differences
//! against the GPU route land in the cross-route noise class
//! (D-869), not in systematic offsets.
//!
//! Masks follow the stencil semantics of the reference: the mask
//! value starts at 1 when the drawable has no Mask-mode masks and
//! at 0 otherwise; each mask source, in record order, replaces the
//! value with 1 (Mask) or 0 (DodgeMask) wherever its sampled albedo
//! alpha exceeds the drawable's threshold; content draws only
//! where the final value is 1.

use crate::eval::Evaluator;
use crate::math::{Mat4, Vec2, Vec3};
use crate::model::{BlendMode, Drawable, MaskMode, NodeKind};

/// A rigid 2D placement of one figure in a shared scene space:
/// rotation about the figure's model origin, then translation.
/// Used to composite two evaluated puppets into one frame and to
/// measure between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Rotation in radians (counterclockwise in model space).
    pub rotation: f32,
    /// Translation in model units.
    pub translation: Vec2,
}

impl Placement {
    /// The identity placement.
    pub fn identity() -> Placement {
        Placement {
            rotation: 0.0,
            translation: Vec2::ZERO,
        }
    }

    /// The placement as a matrix (applied to model-space points).
    pub fn matrix(self) -> Mat4 {
        Mat4::from_translation(Vec3::new(self.translation.x, self.translation.y, 0.0))
            .mul_mat(Mat4::from_euler_xyz(Vec3::new(0.0, 0.0, self.rotation)))
    }

    /// Apply the placement to a model-space point.
    pub fn apply(self, point: Vec2) -> Vec2 {
        self.matrix().transform_point2(point)
    }
}

/// An RGBA8 canvas, rows top-down, transparent at birth.
#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// RGBA bytes, row-major from the top row.
    pub pixels: Vec<u8>,
}

impl Canvas {
    /// A transparent canvas.
    pub fn new(width: usize, height: usize) -> Canvas {
        Canvas {
            width,
            height,
            pixels: vec![0; width * height * 4],
        }
    }

    /// Count of pixels with alpha > 0 (the estate's frame census
    /// metric).
    pub fn nontransparent_count(&self) -> usize {
        self.pixels.chunks_exact(4).filter(|px| px[3] > 0).count()
    }

    /// One pixel as normalized floats.
    fn pixel_f32(&self, index: usize) -> [f32; 4] {
        let base = index * 4;
        [
            self.pixels[base] as f32 / 255.0,
            self.pixels[base + 1] as f32 / 255.0,
            self.pixels[base + 2] as f32 / 255.0,
            self.pixels[base + 3] as f32 / 255.0,
        ]
    }

    /// Store normalized floats with round-to-nearest quantization,
    /// as an RGBA8 framebuffer does.
    fn set_pixel_f32(&mut self, index: usize, value: [f32; 4]) {
        let base = index * 4;
        for c in 0..4 {
            let quantized = (value[c].clamp(0.0, 1.0) * 255.0).round();
            self.pixels[base + c] = quantized as u8;
        }
    }
}

/// Render the evaluator's current frame onto a fresh canvas.
/// `scale` is model-units-to-pixels (the corpus contracts use 0.15
/// for 600x800 spikes and 1.0 for the staged scene); the model
/// origin projects to the canvas center.
pub fn render_frame(evaluator: &Evaluator, width: usize, height: usize, scale: f32) -> Canvas {
    let mut canvas = Canvas::new(width, height);
    render_onto(evaluator, &mut canvas, scale, Placement::identity());
    canvas
}

/// Render the evaluator's current frame onto an existing canvas
/// under a scene placement (for multi-figure composites).
pub fn render_onto(evaluator: &Evaluator, canvas: &mut Canvas, scale: f32, placement: Placement) {
    let mut rasterizer = Rasterizer {
        evaluator,
        width: canvas.width,
        height: canvas.height,
        scale,
        placement: placement.matrix(),
    };
    let root: Vec<usize> = evaluator.draw_root().to_vec();
    for node in root {
        rasterizer.draw_drawable(canvas, node);
    }
}

/// Stateful renderer bound to one evaluation.
struct Rasterizer<'a> {
    evaluator: &'a Evaluator,
    width: usize,
    height: usize,
    scale: f32,
    placement: Mat4,
}

impl Rasterizer<'_> {
    /// Model units to screen pixels, through the scene placement.
    fn to_screen(&self, world: Vec2) -> Vec2 {
        let placed = self.placement.transform_point2(world);
        Vec2::new(
            placed.x * self.scale + self.width as f32 * 0.5,
            placed.y * self.scale + self.height as f32 * 0.5,
        )
    }

    /// Draw one drawable (part or composite) onto the canvas.
    fn draw_drawable(&mut self, canvas: &mut Canvas, node: usize) {
        let model = self.evaluator.model();
        let Some(drawable) = model.nodes[node].drawable().cloned() else {
            return;
        };
        let coverage = if drawable.masks.is_empty() {
            None
        } else {
            Some(self.compute_coverage(&drawable))
        };
        match &model.nodes[node].kind {
            NodeKind::Part(part) => {
                let sampler = TextureSampler {
                    evaluator: self.evaluator,
                    texture: part.texture_slots[0],
                };
                self.draw_part(canvas, node, &drawable, sampler, coverage.as_deref());
            }
            NodeKind::Composite(_) => {
                let mut offscreen = Canvas::new(self.width, self.height);
                let children: Vec<usize> = self
                    .evaluator
                    .draw_children()
                    .iter()
                    .find(|(comp, _)| *comp == node)
                    .map(|(_, list)| list.clone())
                    .unwrap_or_default();
                for child in children {
                    self.draw_drawable(&mut offscreen, child);
                }
                self.finish_composite(canvas, &offscreen, &drawable, coverage.as_deref());
            }
            _ => {}
        }
    }

    /// Rasterize one part's triangles with its fragment parameters.
    fn draw_part(
        &self,
        canvas: &mut Canvas,
        node: usize,
        drawable: &Drawable,
        sampler: TextureSampler<'_>,
        coverage: Option<&[bool]>,
    ) {
        let model = self.evaluator.model();
        let NodeKind::Part(part) = &model.nodes[node].kind else {
            return;
        };
        let world = self.evaluator.world_vertices(node);
        let screen: Vec<Vec2> = world.iter().map(|&w| self.to_screen(w)).collect();
        let opacity = self.evaluator.working_opacity(node);
        let frag = FragParams {
            tint: drawable.tint,
            screen_tint: drawable.screen_tint,
            opacity,
            blend: drawable.blend_mode,
        };
        for tri in part.mesh.indices.chunks_exact(3) {
            let verts = [
                screen[tri[0] as usize],
                screen[tri[1] as usize],
                screen[tri[2] as usize],
            ];
            let uvs = [
                part.mesh.uvs[tri[0] as usize],
                part.mesh.uvs[tri[1] as usize],
                part.mesh.uvs[tri[2] as usize],
            ];
            self.rasterize(canvas, verts, uvs, &sampler, &frag, coverage);
        }
    }

    /// Draw a finished composite offscreen onto the canvas through
    /// the composite's fragment parameters. The reference route
    /// clamps a composite's tint, screen tint, and opacity to
    /// [0, 1] for this pass.
    fn finish_composite(
        &self,
        canvas: &mut Canvas,
        offscreen: &Canvas,
        drawable: &Drawable,
        coverage: Option<&[bool]>,
    ) {
        let clamp3 = |v: [f32; 3]| v.map(|c| c.clamp(0.0, 1.0));
        let frag = FragParams {
            tint: clamp3(drawable.tint),
            screen_tint: clamp3(drawable.screen_tint),
            opacity: drawable.opacity.clamp(0.0, 1.0),
            blend: drawable.blend_mode,
        };
        for index in 0..(self.width * self.height) {
            if let Some(cov) = coverage {
                if !cov[index] {
                    continue;
                }
            }
            let tex = offscreen.pixel_f32(index);
            if tex[3] == 0.0 && frag.blend == BlendMode::Normal {
                continue; // fully transparent source cannot change the pixel
            }
            let src = fragment(tex, &frag);
            blend_into(canvas, index, src, frag.blend);
        }
    }

    /// The stencil-equivalent coverage buffer for a masked
    /// drawable: per-pixel mask values after every source applied
    /// in record order.
    fn compute_coverage(&self, drawable: &Drawable) -> Vec<bool> {
        let starts_lit = !drawable.masks.iter().any(|m| m.mode == MaskMode::Mask);
        let mut value = vec![starts_lit; self.width * self.height];
        let threshold = drawable.mask_threshold.clamp(0.0, 1.0);
        for mask in &drawable.masks {
            let model = self.evaluator.model();
            let NodeKind::Part(source_part) = &model.nodes[mask.source].kind else {
                // A composite mask source panics in the reference GL
                // route; the corpus contains none. It contributes no
                // coverage here either.
                continue;
            };
            let sampler = TextureSampler {
                evaluator: self.evaluator,
                texture: source_part.texture_slots[0],
            };
            let world = self.evaluator.world_vertices(mask.source);
            let screen: Vec<Vec2> = world.iter().map(|&w| self.to_screen(w)).collect();
            let set_to = mask.mode == MaskMode::Mask;
            for tri in source_part.mesh.indices.chunks_exact(3) {
                let verts = [
                    screen[tri[0] as usize],
                    screen[tri[1] as usize],
                    screen[tri[2] as usize],
                ];
                let uvs = [
                    source_part.mesh.uvs[tri[0] as usize],
                    source_part.mesh.uvs[tri[1] as usize],
                    source_part.mesh.uvs[tri[2] as usize],
                ];
                self.cover_triangle(&mut value, verts, uvs, &sampler, threshold, set_to);
            }
        }
        value
    }

    /// Mark one mask-source triangle into the coverage buffer.
    #[allow(clippy::too_many_arguments)]
    fn cover_triangle(
        &self,
        value: &mut [bool],
        verts: [Vec2; 3],
        uvs: [Vec2; 3],
        sampler: &TextureSampler<'_>,
        threshold: f32,
        set_to: bool,
    ) {
        self.for_each_pixel(verts, |index, _px, _py, bary| {
            let uv = interpolate(uvs, bary);
            if sampler.sample(uv.x, uv.y)[3] > threshold {
                value[index] = set_to;
            }
        });
    }

    /// Rasterize one textured triangle.
    fn rasterize(
        &self,
        canvas: &mut Canvas,
        verts: [Vec2; 3],
        uvs: [Vec2; 3],
        sampler: &TextureSampler<'_>,
        frag: &FragParams,
        coverage: Option<&[bool]>,
    ) {
        // Collect first: the closure borrows the canvas immutably
        // for dst reads and we write after. Per-pixel read-modify-
        // write happens in `blend_into`, so iterate directly with a
        // split: gather covered pixels, then apply.
        let mut hits: Vec<(usize, [f32; 4])> = Vec::new();
        self.for_each_pixel(verts, |index, _px, _py, bary| {
            if let Some(cov) = coverage {
                if !cov[index] {
                    return;
                }
            }
            let uv = interpolate(uvs, bary);
            let tex = sampler.sample(uv.x, uv.y);
            hits.push((index, fragment(tex, frag)));
        });
        for (index, src) in hits {
            blend_into(canvas, index, src, frag.blend);
        }
    }

    /// Visit every pixel whose center lies inside the triangle,
    /// with its barycentric weights.
    fn for_each_pixel(
        &self,
        verts: [Vec2; 3],
        mut visit: impl FnMut(usize, usize, usize, [f32; 3]),
    ) {
        let [a, b, c] = verts;
        let area = edge(a, b, c);
        if area == 0.0 {
            return;
        }
        let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as usize;
        let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as usize;
        let max_x = (a.x.max(b.x).max(c.x).ceil() as usize).min(self.width);
        let max_y = (a.y.max(b.y).max(c.y).ceil() as usize).min(self.height);
        for py in min_y..max_y {
            for px in min_x..max_x {
                let p = Vec2::new(px as f32 + 0.5, py as f32 + 0.5);
                let w0 = edge(b, c, p) / area;
                let w1 = edge(c, a, p) / area;
                let w2 = edge(a, b, p) / area;
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    visit(py * self.width + px, px, py, [w0, w1, w2]);
                }
            }
        }
    }
}

/// 2D edge function (z of the cross product of (b - a) and (p - a)).
fn edge(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// Barycentric interpolation of per-vertex UVs.
fn interpolate(uvs: [Vec2; 3], bary: [f32; 3]) -> Vec2 {
    Vec2::new(
        uvs[0].x * bary[0] + uvs[1].x * bary[1] + uvs[2].x * bary[2],
        uvs[0].y * bary[0] + uvs[1].y * bary[1] + uvs[2].y * bary[2],
    )
}

/// Texture lookup for one part; a missing albedo slot samples the
/// transparent void, matching the reference route's empty texture.
struct TextureSampler<'a> {
    evaluator: &'a Evaluator,
    texture: Option<usize>,
}

impl TextureSampler<'_> {
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        match self.texture {
            Some(index) => self.evaluator.model().textures[index].sample(u, v),
            None => [0.0, 0.0, 0.0, 0.0],
        }
    }
}

/// Fragment parameters for one drawable.
struct FragParams {
    tint: [f32; 3],
    screen_tint: [f32; 3],
    opacity: f32,
    blend: BlendMode,
}

/// The shared part/composite fragment math.
fn fragment(tex: [f32; 4], frag: &FragParams) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for c in 0..3 {
        let screen_out = 1.0 - (1.0 - tex[c]) * (1.0 - frag.screen_tint[c] * tex[3]);
        out[c] = screen_out * frag.tint[c] * frag.opacity;
    }
    out[3] = tex[3] * frag.opacity;
    out
}

/// Blend a fragment into a canvas pixel per the drawable's mode.
fn blend_into(canvas: &mut Canvas, index: usize, src: [f32; 4], mode: BlendMode) {
    let dst = canvas.pixel_f32(index);
    let sa = src[3];
    let da = dst[3];
    let mut out = [0.0f32; 4];
    for c in 0..4 {
        let (src_factor, dst_factor, subtract) = match mode {
            BlendMode::Normal => (1.0, 1.0 - sa, false),
            BlendMode::Multiply => (dst[c], 1.0 - sa, false),
            BlendMode::ColorDodge => (dst[c], 1.0, false),
            BlendMode::LinearDodge => (1.0, 1.0, false),
            BlendMode::Screen => (1.0, 1.0 - src[c], false),
            BlendMode::ClipToLower => (da, 1.0 - sa, false),
            BlendMode::SliceFromLower => (1.0 - da, 1.0 - sa, true),
        };
        out[c] = if subtract {
            src[c] * src_factor - dst[c] * dst_factor
        } else {
            src[c] * src_factor + dst[c] * dst_factor
        };
    }
    canvas.set_pixel_f32(index, out);
}
