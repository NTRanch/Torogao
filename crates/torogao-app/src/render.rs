//! The wgpu renderer: draws the evaluated parts of a scene.
//!
//! Geometry comes from the engine's evaluated state only —
//! a part's world vertex is its world matrix applied to
//! `(vertex - mesh origin + deformation)`, the same
//! composition the reference vertex shader uses. Draw order
//! follows the engine's draw ranks: the puppet's z-sorted
//! root parts, each subtree in pre-order. The camera is the
//! model's canvas framing: the evaluated parts' world
//! bounds, padded, fitted to a bounded canvas.
//!
//! Colour follows the reference part shader's albedo path:
//! `screen(texel, screen_tint) * tint`, alpha scaled by the
//! part's effective opacity, where the screen/tint/opacity
//! values include the binding-driven offsets. Masks are
//! applied through the stencil path the reference uses
//! (mask sources write the stencil above their threshold,
//! masked content tests it), and `ClipToLower` carries its
//! reference blend factors. Emission and bump stages and
//! composite render-to-texture grouping are later parity
//! work; parts carrying them are drawn in order and
//! itemized in the render report.

use std::collections::HashMap;

use torogao_engine::{Puppet, Vec2, Vec4};
use torogao_format::NodeKind;
use wgpu::util::DeviceExt;

use crate::error::{AppError, AppResult};
use crate::scene::Scene;
use crate::textures::{RgbaImage, decode_texture};

/// Largest canvas side, in pixels.
pub const CANVAS_SIDE_MAX: u32 = 8192;
/// Viewport texture side cap for the windowed shell.
pub const VIEWPORT_SIDE_MAX: u32 = 2048;
/// Fraction of the bounds size padded on every side when
/// framing the canvas.
pub const CANVAS_PADDING_FRACTION: f32 = 0.05;
/// Largest part count one frame may carry.
pub const PARTS_MAX: usize = 4096;
/// Uniform slots in the shared buffer: one per part a frame
/// may carry, so a whole frame's uniforms are written once,
/// before the pass that reads them is submitted.
const UNIFORM_SLOTS: usize = PARTS_MAX;
/// Byte stride of one uniform slot (GPU alignment).
const UNIFORM_STRIDE: u64 = 256;

/// The frame's canvas: pixel size plus the world-to-NDC
/// view matrix (column-major).
#[derive(Debug, Clone, Copy)]
pub struct Canvas {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Column-major view matrix (model space → clip space).
    pub view: [f32; 16],
}

/// One drawable part, prepared for the GPU: world-space
/// vertices, indices, texture slot, blend and colour state.
#[derive(Debug, Clone)]
pub struct PreparedPart {
    /// Node path (for reports).
    pub path: String,
    /// World-space vertex positions.
    pub positions: Vec<[f32; 2]>,
    /// UV coordinates, parallel to `positions`.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle indices.
    pub indices: Vec<u32>,
    /// Texture slot index, when the part has a usable slot.
    pub texture_slot: Option<usize>,
    /// The blend slug actually rendered.
    pub blend_slug: &'static str,
    /// The authored blend mode name.
    pub blend_mode: String,
    /// A note when the rendered treatment differs from the
    /// authored one (approximation or fallback).
    pub blend_note: Option<String>,
    /// Effective tint rgb with effective opacity in alpha.
    pub tint: [f32; 4],
    /// Effective screen tint rgb.
    pub screen: [f32; 3],
    /// The arena node this part was prepared from.
    pub node: usize,
    /// Mask bindings to apply when drawing this part
    /// (indices into the frame's mask-source list).
    pub masks: Vec<PreparedMask>,
    /// The stencil threshold used when this part is drawn
    /// as a mask source: the source's own mask threshold
    /// plus its binding-driven offset, clamped to [0, 1]
    /// (the reference mask shader's rule). Zero for parts
    /// never used as mask sources.
    pub mask_threshold: f32,
}

/// One mask binding of a prepared part: the source
/// geometries that write the stencil, whether the binding
/// carves instead of admits (DodgeMask), in binding order
/// (later sources replace earlier stencil values, as the
/// reference's replace-stencil draw does).
#[derive(Debug, Clone)]
pub struct PreparedMask {
    /// Indices into the frame's mask-source list.
    pub sources: Vec<usize>,
    /// True for a DodgeMask binding (writes stencil 0).
    pub dodge: bool,
}

/// A fully prepared frame: canvas plus parts in draw order.
#[derive(Debug)]
pub struct PreparedFrame {
    /// The frame's canvas.
    pub canvas: Canvas,
    /// Parts in draw order.
    pub parts: Vec<PreparedPart>,
    /// Mask-source geometries, prepared once per distinct
    /// source node (a composite source contributes its
    /// drawable subtree, in draw order).
    pub mask_sources: Vec<PreparedPart>,
    /// Parts skipped, each with its reason (path: reason).
    pub skipped: Vec<String>,
    /// Frame-level notes (masks present, animated parts,
    /// secondary textures ignored, and similar).
    pub notes: Vec<String>,
}

/// Maps an authored blend mode name to the slug this
/// renderer implements and whether that slug is a faithful
/// implementation. Anything not implemented renders as
/// Normal in draw order and is itemized by the caller.
pub fn blend_slug(mode_name: &str) -> (&'static str, bool) {
    match mode_name {
        "Normal" => ("normal", true),
        "Multiply" => ("multiply", true),
        "Screen" => ("screen", true),
        "LinearDodge" | "AddGlow" => ("linear_dodge", true),
        "ClipToLower" => ("clip_to_lower", true),
        _ => ("normal", false),
    }
}

fn clamp01(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

/// Whether a node renders: itself and every ancestor
/// enabled (the reference's recursive render-enabled rule),
/// computed iteratively.
fn render_enabled(puppet: &Puppet, mut index: usize) -> bool {
    loop {
        let node = &puppet.nodes.nodes[index];
        if !node.enabled {
            return false;
        }
        match node.parent {
            Some(parent) => index = parent,
            None => return true,
        }
    }
}

/// The draw order of drawable node indices: root parts in
/// the puppet's stored (z-sorted) order, each subtree in
/// pre-order, then any drawable not reached that way in
/// arena order. Iterative, with a node-count bound.
fn draw_order(puppet: &Puppet) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::new();
    let mut seen = vec![false; puppet.nodes.nodes.len()];
    let mut stack: Vec<usize> = Vec::new();
    for root_part in puppet.root_parts.iter().rev() {
        stack.push(*root_part);
    }
    while let Some(index) = stack.pop() {
        if seen[index] {
            continue;
        }
        seen[index] = true;
        if puppet.nodes.nodes[index].is_drawable() {
            order.push(index);
        }
        for child in puppet.nodes.nodes[index].children.iter().rev() {
            stack.push(*child);
        }
    }
    for index in 0..puppet.nodes.nodes.len() {
        if !seen[index] && puppet.nodes.nodes[index].is_drawable() {
            order.push(index);
        }
    }
    order
}

/// The effective opacity of a part: its own base × offset,
/// folded with the base opacity of composite ancestors
/// (the exporter's folding rule; composites have no
/// render-to-texture stage in this slice).
fn effective_opacity(puppet: &Puppet, index: usize) -> f32 {
    let node = &puppet.nodes.nodes[index];
    let mut opacity = node.opacity * node.offset_opacity;
    let mut cursor = node.parent;
    while let Some(parent) = cursor {
        let ancestor = &puppet.nodes.nodes[parent];
        if ancestor.kind == NodeKind::Composite {
            opacity *= ancestor.opacity;
        }
        cursor = ancestor.parent;
    }
    clamp01(opacity)
}

/// Whether `index` is a strict descendant of `ancestor`
/// in the node arena (iterative parent walk).
fn is_descendant(puppet: &Puppet, index: usize, ancestor: usize) -> bool {
    let mut cursor = puppet.nodes.nodes[index].parent;
    while let Some(parent) = cursor {
        if parent == ancestor {
            return true;
        }
        cursor = puppet.nodes.nodes[parent].parent;
    }
    false
}

/// Prepares one drawable node's geometry and colour state.
/// The error is the skip reason the frame report carries.
/// Mask fields are filled by the caller (content parts get
/// their bindings in `prepare_frame`; source parts get
/// their threshold there).
fn prepare_part(
    puppet: &mut Puppet,
    index: usize,
    texture_count: usize,
) -> Result<PreparedPart, String> {
    let override_matrix = puppet.nodes.nodes[index].override_matrix;
    let matrix = match override_matrix {
        Some(matrix) => matrix,
        None => puppet.nodes.transform(index).trs,
    };
    let node = &puppet.nodes.nodes[index];
    let Some(mesh) = &node.mesh else {
        return Err("drawable carries no mesh".to_string());
    };
    if mesh.vertices.is_empty() || mesh.indices.len() < 3 || mesh.indices.len() % 3 != 0 {
        return Err("mesh is empty or not a triangle list".to_string());
    }
    if mesh.uvs.len() != mesh.vertices.len() {
        return Err(format!(
            "uv count {} does not match vertex count {}",
            mesh.uvs.len(),
            mesh.vertices.len()
        ));
    }
    if node.texture_ids.is_empty() {
        return Err("part references no texture".to_string());
    }
    let slot = node.texture_ids[0];
    if slot < 0 || (slot as usize) >= texture_count {
        return Err(format!("texture slot {slot} does not exist"));
    }
    let mut positions: Vec<[f32; 2]> = Vec::with_capacity(mesh.vertices.len());
    for (vertex_index, vertex) in mesh.vertices.iter().enumerate() {
        let deform = node
            .deformation
            .get(vertex_index)
            .copied()
            .unwrap_or(Vec2::ZERO);
        let local = Vec4::new(
            vertex.x - mesh.origin.x + deform.x,
            vertex.y - mesh.origin.y + deform.y,
            0.0,
            1.0,
        );
        let world = matrix * local;
        let point = if world.w != 0.0 && world.w != 1.0 {
            [world.x / world.w, world.y / world.w]
        } else {
            [world.x, world.y]
        };
        positions.push(point);
    }
    let (slug, implemented) = blend_slug(&node.blend_mode);
    let blend_note = if !implemented {
        Some(format!(
            "blend mode {} is not implemented in this slice; rendered as normal in draw order",
            node.blend_mode
        ))
    } else if node.blend_mode == "AddGlow" {
        Some("AddGlow rendered as linear_dodge; the glow falloff is not carried".to_string())
    } else {
        None
    };
    Ok(PreparedPart {
        path: puppet.nodes.path_of(index),
        positions,
        uvs: mesh.uvs.iter().map(|uv| [uv.x, uv.y]).collect(),
        indices: mesh.indices.iter().map(|i| u32::from(*i)).collect(),
        texture_slot: Some(slot as usize),
        blend_slug: slug,
        blend_mode: node.blend_mode.clone(),
        blend_note,
        tint: [
            clamp01(node.tint.x * node.offset_tint.x),
            clamp01(node.tint.y * node.offset_tint.y),
            clamp01(node.tint.z * node.offset_tint.z),
            effective_opacity(puppet, index),
        ],
        screen: [
            clamp01(node.screen_tint.x + node.offset_screen_tint.x),
            clamp01(node.screen_tint.y + node.offset_screen_tint.y),
            clamp01(node.screen_tint.z + node.offset_screen_tint.z),
        ],
        node: index,
        masks: Vec::new(),
        mask_threshold: 0.0,
    })
}

/// Prepares one frame from the scene's evaluated state.
pub fn prepare_frame(scene: &mut Scene) -> AppResult<PreparedFrame> {
    let texture_count = scene.textures.len();
    let puppet = &mut scene.puppet;
    let order = draw_order(puppet);
    if order.len() > PARTS_MAX {
        return Err(AppError::Render(format!(
            "model carries {} drawable parts; the frame limit is {PARTS_MAX}",
            order.len()
        )));
    }
    let mut parts: Vec<PreparedPart> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut masked_parts = 0_usize;
    let mut animated_parts = 0_usize;
    let mut secondary_textures = 0_usize;
    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    for index in &order {
        let index = *index;
        let path = puppet.nodes.path_of(index);
        if !render_enabled(puppet, index) {
            skipped.push(format!("{path}: node or an ancestor is disabled"));
            continue;
        }
        let node = &puppet.nodes.nodes[index];
        if !node.masks.is_empty() {
            masked_parts += 1;
        }
        if node.kind == NodeKind::AnimatedPart {
            animated_parts += 1;
        }
        if node.texture_ids.len() > 1 {
            secondary_textures += 1;
        }
        match prepare_part(puppet, index, texture_count) {
            Ok(part) => {
                for point in &part.positions {
                    min.x = min.x.min(point[0]);
                    min.y = min.y.min(point[1]);
                    max.x = max.x.max(point[0]);
                    max.y = max.y.max(point[1]);
                }
                parts.push(part);
            }
            Err(reason) => skipped.push(format!("{path}: {reason}")),
        }
    }

    // Mask bindings: resolve each content part's bindings
    // to prepared source geometries. A source is prepared
    // regardless of its own content eligibility (the
    // reference draws mask sources directly, bypassing the
    // content path); a composite source contributes its
    // drawable subtree in draw order.
    let mut mask_sources: Vec<PreparedPart> = Vec::new();
    let mut source_slots: HashMap<usize, usize> = HashMap::new();
    let mut applied_bindings = 0_usize;
    let mut dropped_bindings = 0_usize;
    for part in &mut parts {
        let bindings = puppet.nodes.nodes[part.node].masks.clone();
        for binding in bindings {
            let Some(src) = binding.src else {
                dropped_bindings += 1;
                continue;
            };
            let src_nodes: Vec<usize> = if puppet.nodes.nodes[src].kind == NodeKind::Composite {
                order
                    .iter()
                    .copied()
                    .filter(|candidate| is_descendant(puppet, *candidate, src))
                    .collect()
            } else {
                vec![src]
            };
            let mut slots: Vec<usize> = Vec::new();
            for src_node in src_nodes {
                if let Some(slot) = source_slots.get(&src_node) {
                    slots.push(*slot);
                    continue;
                }
                if let Ok(mut source) = prepare_part(puppet, src_node, texture_count) {
                    let source_node = &puppet.nodes.nodes[src_node];
                    source.mask_threshold =
                        clamp01(source_node.offset_mask_threshold + source_node.mask_threshold);
                    let slot = mask_sources.len();
                    mask_sources.push(source);
                    source_slots.insert(src_node, slot);
                    slots.push(slot);
                }
            }
            if slots.is_empty() {
                dropped_bindings += 1;
                continue;
            }
            applied_bindings += 1;
            part.masks.push(PreparedMask {
                sources: slots,
                dodge: binding.mode == "DodgeMask",
            });
        }
    }
    if parts.len() + mask_sources.len() > PARTS_MAX {
        return Err(AppError::Render(format!(
            "frame carries {} parts and {} mask sources; the uniform limit is {PARTS_MAX}",
            parts.len(),
            mask_sources.len()
        )));
    }
    if masked_parts > 0 {
        notes.push(format!(
            "{masked_parts} part(s) carry masks; {applied_bindings} binding(s) applied via the stencil path"
        ));
    }
    if dropped_bindings > 0 {
        notes.push(format!(
            "{dropped_bindings} mask binding(s) dropped: source unresolved or carries no usable geometry"
        ));
    }
    if animated_parts > 0 {
        notes.push(format!(
            "{animated_parts} animated part(s) drawn as static parts (no frame splits in this slice)"
        ));
    }
    if secondary_textures > 0 {
        notes.push(format!(
            "{secondary_textures} part(s) carry secondary (emission/bump) textures; not sampled in this slice"
        ));
    }
    Ok(PreparedFrame {
        canvas: frame_canvas(min, max),
        parts,
        mask_sources,
        skipped,
        notes,
    })
}

/// The canvas framing for world bounds: pad, then fit the
/// longer side to at most `CANVAS_SIDE_MAX`, preserving
/// aspect. Degenerate bounds yield a 1×1 canvas.
fn frame_canvas(min: Vec2, max: Vec2) -> Canvas {
    if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
        return Canvas {
            width: 1,
            height: 1,
            view: identity_view(),
        };
    }
    let span_x = (max.x - min.x).max(1.0);
    let span_y = (max.y - min.y).max(1.0);
    let pad_x = span_x * CANVAS_PADDING_FRACTION;
    let pad_y = span_y * CANVAS_PADDING_FRACTION;
    let min_x = min.x - pad_x;
    let min_y = min.y - pad_y;
    let span_x = span_x + 2.0 * pad_x;
    let span_y = span_y + 2.0 * pad_y;
    let scale = (CANVAS_SIDE_MAX as f32 / span_x.max(span_y)).min(1.0);
    let width = ((span_x * scale).ceil() as u32).max(1);
    let height = ((span_y * scale).ceil() as u32).max(1);
    // Model space is y-down; the view maps bounds min to
    // clip (-1, +1) and max to (+1, -1).
    let view = [
        2.0 / span_x,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / span_y,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -1.0 - 2.0 * min_x / span_x,
        1.0 + 2.0 * min_y / span_y,
        0.0,
        1.0,
    ];
    Canvas {
        width,
        height,
        view,
    }
}

fn identity_view() -> [f32; 16] {
    let mut view = [0.0_f32; 16];
    view[0] = 1.0;
    view[5] = 1.0;
    view[10] = 1.0;
    view[15] = 1.0;
    view
}

/// Scales a frame's canvas down to a side cap (windowed
/// viewport), keeping the view matrix (aspect preserved).
pub fn cap_canvas(canvas: Canvas, side_max: u32) -> Canvas {
    let longest = canvas.width.max(canvas.height);
    if longest <= side_max {
        return canvas;
    }
    let scale = f64::from(side_max) / f64::from(longest);
    Canvas {
        width: ((f64::from(canvas.width) * scale).ceil() as u32).max(1),
        height: ((f64::from(canvas.height) * scale).ceil() as u32).max(1),
        view: canvas.view,
    }
}

/// What one render produced, beyond the pixels.
#[derive(Debug, Default)]
pub struct RenderReport {
    /// Parts actually drawn.
    pub parts_drawn: usize,
    /// Parts skipped, with reasons.
    pub skipped: Vec<String>,
    /// Frame notes from preparation.
    pub notes: Vec<String>,
    /// Per-part blend itemizations (approximations and
    /// fallbacks only).
    pub blend_notes: Vec<String>,
    /// Texture slots that failed to decode (drawn with the
    /// fallback texture).
    pub decode_failures: Vec<String>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view: [f32; 16],
    tint: [f32; 4],
    screen: [f32; 4],
    misc: [f32; 4],
}

const SHADER_WGSL: &str = r#"
struct Uniforms {
    view: mat4x4<f32>,
    tint: vec4<f32>,
    screen: vec4<f32>,
    misc: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var part_texture: texture_2d<f32>;
@group(0) @binding(2) var part_sampler: sampler;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = uniforms.view * vec4<f32>(input.position, 0.0, 1.0);
    out.uv = input.uv;
    return out;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(part_texture, part_sampler, input.uv);
    let screened = vec3<f32>(1.0)
        - (vec3<f32>(1.0) - texel.rgb) * (vec3<f32>(1.0) - uniforms.screen.rgb * texel.a);
    return vec4<f32>(screened * uniforms.tint.rgb, texel.a * uniforms.tint.a);
}

// The mask-source fragment: texels at or below the
// threshold (uniforms.misc.x) do not write the stencil,
// exactly the reference basic-mask shader's discard rule.
@fragment
fn fs_mask(input: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(part_texture, part_sampler, input.uv);
    if (texel.a <= uniforms.misc.x) {
        discard;
    }
    return vec4<f32>(1.0);
}
"#;

/// The debug-overlay line shader: world-space segment
/// endpoints in, one flat colour out. The overlay's uniform
/// block carries the frame's view matrix and the line
/// colour.
const LINE_SHADER_WGSL: &str = r#"
struct LineUniforms {
    view: mat4x4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: LineUniforms;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> @builtin(position) vec4<f32> {
    return uniforms.view * vec4<f32>(position, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return uniforms.color;
}
"#;

/// The physics debug overlay's line colour (opaque amber,
/// chosen to read against both skin and hair tones).
pub const DEBUG_LINE_COLOUR: [f32; 4] = [1.0, 0.62, 0.05, 1.0];

/// The GPU renderer, built once and reused for every frame
/// and every scene: device, queue, one pipeline per blend
/// slug, samplers, a fallback texture, and the uniform ring.
pub struct Renderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipelines: HashMap<&'static str, wgpu::RenderPipeline>,
    /// Per-slug pipeline variants carrying the mask
    /// stencil-test state, for masked content draws.
    pipelines_masked: HashMap<&'static str, wgpu::RenderPipeline>,
    /// The mask-source pipeline (stencil replace, colour
    /// writes off).
    mask_pipeline: wgpu::RenderPipeline,
    /// The physics debug-overlay line pipeline and its
    /// uniform plumbing.
    line_pipeline: wgpu::RenderPipeline,
    line_bind_group_layout: wgpu::BindGroupLayout,
    line_uniform_buffer: wgpu::Buffer,
    sampler_linear: wgpu::Sampler,
    sampler_nearest: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    texture_views: HashMap<usize, wgpu::TextureView>,
    texture_scene: u64,
    white_view: wgpu::TextureView,
    /// The stencil target, cached by size; created only
    /// when a frame actually carries masked parts.
    stencil_target: Option<(wgpu::TextureView, u32, u32)>,
}

impl Renderer {
    /// Creates the renderer: requests an adapter and device
    /// for offscreen work. Fails with a typed error when no
    /// adapter exists (the shell still loads and evaluates).
    pub fn new() -> AppResult<Renderer> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|err| AppError::Render(format!("no GPU adapter available: {err}")))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("torogao-app device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|err| AppError::Render(format!("GPU device request failed: {err}")))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("torogao part shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("torogao part bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(
                            std::mem::size_of::<Uniforms>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("torogao part pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let mut pipelines = HashMap::new();
        let mut pipelines_masked = HashMap::new();
        for slug in [
            "normal",
            "multiply",
            "screen",
            "linear_dodge",
            "clip_to_lower",
        ] {
            pipelines.insert(
                slug,
                build_pipeline(&device, &pipeline_layout, &shader, slug, false),
            );
            pipelines_masked.insert(
                slug,
                build_pipeline(&device, &pipeline_layout, &shader, slug, true),
            );
        }
        let mask_pipeline = build_mask_pipeline(&device, &pipeline_layout, &shader);
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("torogao debug line shader"),
            source: wgpu::ShaderSource::Wgsl(LINE_SHADER_WGSL.into()),
        });
        let line_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("torogao debug line bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(80),
                    },
                    count: None,
                }],
            });
        let line_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("torogao debug line pipeline layout"),
            bind_group_layouts: &[Some(&line_bind_group_layout)],
            immediate_size: 0,
        });
        let line_pipeline = build_line_pipeline(&device, &line_pipeline_layout, &line_shader);
        let line_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("torogao debug line uniforms"),
            size: UNIFORM_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler_linear = build_sampler(&device, wgpu::FilterMode::Linear);
        let sampler_nearest = build_sampler(&device, wgpu::FilterMode::Nearest);
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("torogao part uniforms"),
            size: UNIFORM_STRIDE * UNIFORM_SLOTS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let white = RgbaImage::solid(1, 1, [255, 255, 255, 255]);
        let white_view = upload_texture(&device, &queue, &white, "torogao fallback texture");
        Ok(Renderer {
            instance,
            adapter,
            device,
            queue,
            bind_group_layout,
            pipelines,
            pipelines_masked,
            mask_pipeline,
            line_pipeline,
            line_bind_group_layout,
            line_uniform_buffer,
            sampler_linear,
            sampler_nearest,
            uniform_buffer,
            texture_views: HashMap::new(),
            texture_scene: 0,
            white_view,
            stencil_target: None,
        })
    }

    /// The device (the windowed shell builds its surface and
    /// UI renderer on the same device).
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Creates a window surface on this renderer's instance
    /// (a surface and the device that draws to it must come
    /// from the same instance).
    pub fn create_surface(
        &self,
        window: std::sync::Arc<winit::window::Window>,
    ) -> AppResult<wgpu::Surface<'static>> {
        self.instance
            .create_surface(window)
            .map_err(|err| AppError::Render(format!("window surface creation failed: {err}")))
    }

    /// Creates a surface from a caller-supplied target on
    /// this renderer's instance: the seam through which a
    /// non-winit presentation (the companion's Wayland
    /// layer-shell surface, Stage 1.1) draws with this
    /// renderer. The target's handles must stay valid for
    /// the surface's whole lifetime — the caller's surface
    /// object owns that guarantee, exactly as the winit
    /// window does for `create_surface`.
    pub fn create_surface_from_target(
        &self,
        target: wgpu::SurfaceTarget<'static>,
    ) -> AppResult<wgpu::Surface<'static>> {
        self.instance
            .create_surface(target)
            .map_err(|err| AppError::Render(format!("window surface creation failed: {err}")))
    }

    /// The preferred surface format for a surface on this
    /// renderer's adapter (sRGB when offered).
    pub fn surface_format(&self, surface: &wgpu::Surface<'_>) -> wgpu::TextureFormat {
        let capabilities = surface.get_capabilities(&self.adapter);
        capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .or_else(|| capabilities.formats.first().copied())
            .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb)
    }

    /// The queue paired with the device.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The adapter the device was made on — surface
    /// capability queries (the companion's configure path)
    /// need it.
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    /// The adapter's maximum 2D texture dimension (the 8K
    /// texture-target evidence reads this).
    pub fn max_texture_dimension_2d(&self) -> u32 {
        self.adapter.limits().max_texture_dimension_2d
    }

    /// Renders the scene's current evaluated state offscreen
    /// at canvas size and reads the pixels back.
    pub fn render(&mut self, scene: &mut Scene) -> AppResult<(RgbaImage, RenderReport)> {
        self.render_inner(scene, &[])
    }

    /// Renders offscreen with the physics debug overlay:
    /// the engine's anchor-to-bob segments drawn over the
    /// frame (G-10). The segment count lands in the
    /// report's notes.
    pub fn render_debug(&mut self, scene: &mut Scene) -> AppResult<(RgbaImage, RenderReport)> {
        let segments = physics_overlay_segments(&scene.puppet);
        self.render_inner(scene, &segments)
    }

    fn render_inner(
        &mut self,
        scene: &mut Scene,
        debug_segments: &[[f32; 2]],
    ) -> AppResult<(RgbaImage, RenderReport)> {
        let frame = prepare_frame(scene)?;
        let canvas = frame.canvas;
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("torogao offscreen target"),
            size: wgpu::Extent3d {
                width: canvas.width,
                height: canvas.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut report = RenderReport {
            parts_drawn: frame.parts.len(),
            skipped: frame.skipped.clone(),
            notes: frame.notes.clone(),
            blend_notes: Vec::new(),
            decode_failures: Vec::new(),
        };
        if !debug_segments.is_empty() {
            report.notes.push(format!(
                "physics debug overlay: {} segment(s) drawn",
                debug_segments.len() / 2
            ));
        }
        let point_filter = scene.preserve_pixels;
        self.draw_frame(
            scene,
            &frame,
            &view,
            (canvas.width, canvas.height),
            point_filter,
            wgpu::Color::TRANSPARENT,
            debug_segments,
            &mut report,
        );
        let image = self.read_back(&target, canvas.width, canvas.height)?;
        Ok((image, report))
    }

    /// Draws a prepared frame into an existing view (the
    /// windowed viewport) of the given pixel size; the
    /// frame's view matrix already fits the model to it.
    /// `debug_segments` are world-space overlay segment
    /// endpoints (empty for no overlay).
    pub fn draw_into(
        &mut self,
        scene: &mut Scene,
        frame: &PreparedFrame,
        view: &wgpu::TextureView,
        size: (u32, u32),
        clear: wgpu::Color,
        debug_segments: &[[f32; 2]],
    ) {
        let mut report = RenderReport::default();
        let point_filter = scene.preserve_pixels;
        self.draw_frame(
            scene,
            frame,
            view,
            size,
            point_filter,
            clear,
            debug_segments,
            &mut report,
        );
    }

    /// Ensures the scene's textures are uploaded, decoding
    /// each slot at most once per scene. Decode failures fall
    /// back to the white texture and are reported once.
    fn ensure_textures(&mut self, scene: &Scene, report: &mut RenderReport) {
        if self.texture_scene != scene.generation {
            self.texture_views.clear();
            self.texture_scene = scene.generation;
        }
        for (index, slot) in scene.textures.iter().enumerate() {
            if self.texture_views.contains_key(&index) {
                continue;
            }
            if slot.data.is_empty() {
                self.texture_views.insert(index, self.white_view.clone());
                continue;
            }
            match decode_texture(slot) {
                Ok(image) => {
                    let view =
                        upload_texture(&self.device, &self.queue, &image, "torogao part texture");
                    self.texture_views.insert(index, view);
                }
                Err(err) => {
                    report
                        .decode_failures
                        .push(format!("texture slot {index}: {err}"));
                    self.texture_views.insert(index, self.white_view.clone());
                }
            }
        }
    }

    /// Builds the GPU draw resources for one prepared part
    /// at the given uniform slot.
    fn build_draw(&self, part: &PreparedPart, slot: usize, sampler: &wgpu::Sampler) -> PartDraw {
        let vertices: Vec<Vertex> = part
            .positions
            .iter()
            .zip(part.uvs.iter())
            .map(|(position, uv)| Vertex {
                position: *position,
                uv: *uv,
            })
            .collect();
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("torogao part vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("torogao part indices"),
                contents: bytemuck::cast_slice(&part.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        let texture_view = part
            .texture_slot
            .and_then(|slot| self.texture_views.get(&slot))
            .unwrap_or(&self.white_view);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("torogao part bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.uniform_buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(std::mem::size_of::<Uniforms>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        PartDraw {
            vertex_buffer,
            index_buffer,
            bind_group,
            index_count: part.indices.len() as u32,
            slot: slot as u32,
        }
    }

    /// Ensures the stencil target matches the frame size,
    /// creating it on first masked use.
    fn ensure_stencil(&mut self, width: u32, height: u32) -> wgpu::TextureView {
        if let Some((view, w, h)) = &self.stencil_target {
            if *w == width && *h == height {
                return view.clone();
            }
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("torogao mask stencil"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24PlusStencil8,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.stencil_target = Some((view.clone(), width, height));
        view
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_frame(
        &mut self,
        scene: &mut Scene,
        frame: &PreparedFrame,
        view: &wgpu::TextureView,
        size: (u32, u32),
        point_filter: bool,
        clear: wgpu::Color,
        debug_segments: &[[f32; 2]],
        report: &mut RenderReport,
    ) {
        self.ensure_textures(scene, report);
        for part in &frame.parts {
            if let Some(note) = &part.blend_note {
                report.blend_notes.push(format!("{}: {note}", part.path));
            }
        }
        let sampler = if point_filter {
            &self.sampler_nearest
        } else {
            &self.sampler_linear
        };
        // All uniforms are written before the pass is
        // submitted: queue writes land in submission order,
        // so a write issued while recording would still be
        // ordered before the submit — writing up front keeps
        // that ordering obvious instead of relying on it.
        // Content parts take slots [0, parts); mask sources
        // follow, carrying their threshold in misc.x.
        for (slot_index, part) in frame.parts.iter().enumerate() {
            let uniforms = Uniforms {
                view: frame.canvas.view,
                tint: part.tint,
                screen: [part.screen[0], part.screen[1], part.screen[2], 1.0],
                misc: [0.0; 4],
            };
            self.queue.write_buffer(
                &self.uniform_buffer,
                slot_index as u64 * UNIFORM_STRIDE,
                bytemuck::bytes_of(&uniforms),
            );
        }
        let source_slot_base = frame.parts.len();
        for (offset, source) in frame.mask_sources.iter().enumerate() {
            let uniforms = Uniforms {
                view: frame.canvas.view,
                tint: source.tint,
                screen: [source.screen[0], source.screen[1], source.screen[2], 1.0],
                misc: [source.mask_threshold, 0.0, 0.0, 0.0],
            };
            self.queue.write_buffer(
                &self.uniform_buffer,
                (source_slot_base + offset) as u64 * UNIFORM_STRIDE,
                bytemuck::bytes_of(&uniforms),
            );
        }
        let part_draws: Vec<PartDraw> = frame
            .parts
            .iter()
            .enumerate()
            .map(|(index, part)| self.build_draw(part, index, sampler))
            .collect();
        let source_draws: Vec<PartDraw> = frame
            .mask_sources
            .iter()
            .enumerate()
            .map(|(offset, source)| self.build_draw(source, source_slot_base + offset, sampler))
            .collect();
        let has_masks = frame.parts.iter().any(|part| !part.masks.is_empty());
        let stencil_view = if has_masks {
            Some(self.ensure_stencil(size.0, size.1))
        } else {
            None
        };
        if !debug_segments.is_empty() {
            let mut line_uniforms = [0.0_f32; 20];
            line_uniforms[..16].copy_from_slice(&frame.canvas.view);
            line_uniforms[16..].copy_from_slice(&DEBUG_LINE_COLOUR);
            self.queue.write_buffer(
                &self.line_uniform_buffer,
                0,
                bytemuck::cast_slice(&line_uniforms),
            );
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("torogao frame encoder"),
            });
        let mut first_pass = true;
        let mut index = 0_usize;
        while index < frame.parts.len() {
            if frame.parts[index].masks.is_empty() {
                // A run of unmasked parts: one plain pass,
                // exactly the pre-mask pass structure.
                let start = index;
                while index < frame.parts.len() && frame.parts[index].masks.is_empty() {
                    index += 1;
                }
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("torogao part pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: if first_pass {
                                wgpu::LoadOp::Clear(clear)
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                for draw_index in start..index {
                    let pipeline = self
                        .pipelines
                        .get(frame.parts[draw_index].blend_slug)
                        .expect("every rendered slug has a pipeline");
                    draw_part(&mut pass, &part_draws[draw_index], pipeline);
                }
            } else {
                // One masked part: its own pass, stencil
                // cleared to the reference's base value (0
                // when any Mask binding exists, 1 for a
                // dodge-only set), sources replace the
                // stencil in binding order, then the content
                // draws where the stencil equals 1.
                let part = &frame.parts[index];
                let stencil_view = stencil_view
                    .as_ref()
                    .expect("a masked frame has a stencil target");
                let base: u32 = if part.masks.iter().any(|mask| !mask.dodge) {
                    0
                } else {
                    1
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("torogao masked part pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: if first_pass {
                                wgpu::LoadOp::Clear(clear)
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: stencil_view,
                        depth_ops: None,
                        stencil_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(base),
                            store: wgpu::StoreOp::Store,
                        }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                for mask in &part.masks {
                    pass.set_stencil_reference(if mask.dodge { 0 } else { 1 });
                    for source in &mask.sources {
                        draw_part(&mut pass, &source_draws[*source], &self.mask_pipeline);
                    }
                }
                pass.set_stencil_reference(1);
                let pipeline = self
                    .pipelines_masked
                    .get(part.blend_slug)
                    .expect("every rendered slug has a masked pipeline");
                draw_part(&mut pass, &part_draws[index], pipeline);
                index += 1;
            }
            first_pass = false;
        }
        if !debug_segments.is_empty() {
            let vertex_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("torogao debug line vertices"),
                    contents: bytemuck::cast_slice(debug_segments),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("torogao debug line bind group"),
                layout: &self.line_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.line_uniform_buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(80),
                    }),
                }],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("torogao debug overlay pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if first_pass {
                            wgpu::LoadOp::Clear(clear)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.line_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(0..debug_segments.len() as u32, 0..1);
        }
        self.queue.submit([encoder.finish()]);
    }

    fn read_back(&self, target: &wgpu::Texture, width: u32, height: u32) -> AppResult<RgbaImage> {
        let row_bytes = width as usize * 4;
        let padded_row = row_bytes.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("torogao readback"),
            size: (padded_row * height as usize) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("torogao readback encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(
                        u32::try_from(padded_row)
                            .map_err(|_| AppError::Render("readback row is too wide".into()))?,
                    ),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|err| AppError::Render(format!("GPU poll failed: {err:?}")))?;
        receiver
            .recv()
            .map_err(|_| AppError::Render("GPU readback channel closed".to_string()))?
            .map_err(|err| AppError::Render(format!("GPU readback map failed: {err:?}")))?;
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        for row in 0..height as usize {
            let start = row * padded_row;
            pixels.extend_from_slice(&mapped[start..start + row_bytes]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(RgbaImage::new(width, height, pixels))
    }
}

/// The GPU resources of one part draw, built once per
/// frame and reused across the frame's passes.
struct PartDraw {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    index_count: u32,
    slot: u32,
}

/// Issues one part draw into a pass with the given
/// pipeline.
fn draw_part(pass: &mut wgpu::RenderPass<'_>, draw: &PartDraw, pipeline: &wgpu::RenderPipeline) {
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &draw.bind_group, &[draw.slot * UNIFORM_STRIDE as u32]);
    pass.set_vertex_buffer(0, draw.vertex_buffer.slice(..));
    pass.set_index_buffer(draw.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..draw.index_count, 0, 0..1);
}

/// The physics debug overlay's segment endpoints, in
/// world space, flattened to line-list vertex pairs
/// (anchor, bob) per driver the engine reports.
pub fn physics_overlay_segments(puppet: &Puppet) -> Vec<[f32; 2]> {
    let mut points = Vec::new();
    for (anchor, bob) in puppet.physics_debug_segments() {
        points.push([anchor.x, anchor.y]);
        points.push([bob.x, bob.y]);
    }
    points
}

fn build_sampler(device: &wgpu::Device, filter: wgpu::FilterMode) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("torogao part sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &RgbaImage,
    label: &'static str,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &image.pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width * 4),
            rows_per_image: Some(image.height),
        },
        wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn blend_state(slug: &str) -> wgpu::BlendState {
    let component = |src: wgpu::BlendFactor, dst: wgpu::BlendFactor| wgpu::BlendComponent {
        src_factor: src,
        dst_factor: dst,
        operation: wgpu::BlendOperation::Add,
    };
    // The reference's legacy blend functions
    // (inSetBlendModeLegacy): one function applies to
    // colour and alpha alike in GL; wgpu states them per
    // component. ClipToLower is the one mode whose alpha
    // factors differ from the shared default — the
    // destination alpha becomes the result alpha, which is
    // what makes the clip clean.
    let alpha = if slug == "clip_to_lower" {
        component(
            wgpu::BlendFactor::DstAlpha,
            wgpu::BlendFactor::OneMinusSrcAlpha,
        )
    } else {
        component(wgpu::BlendFactor::One, wgpu::BlendFactor::OneMinusSrcAlpha)
    };
    let color = match slug {
        "multiply" => component(wgpu::BlendFactor::Dst, wgpu::BlendFactor::Zero),
        "screen" => component(wgpu::BlendFactor::One, wgpu::BlendFactor::OneMinusSrc),
        "linear_dodge" => component(wgpu::BlendFactor::One, wgpu::BlendFactor::One),
        "clip_to_lower" => component(
            wgpu::BlendFactor::DstAlpha,
            wgpu::BlendFactor::OneMinusSrcAlpha,
        ),
        _ => component(
            wgpu::BlendFactor::SrcAlpha,
            wgpu::BlendFactor::OneMinusSrcAlpha,
        ),
    };
    wgpu::BlendState { color, alpha }
}

/// The stencil state masked content draws with: pass
/// where the stencil equals the reference value (1),
/// never write (the reference's mask-content state).
fn stencil_test_state() -> wgpu::StencilState {
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Equal,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Keep,
    };
    wgpu::StencilState {
        front: face,
        back: face,
        read_mask: 0xFF,
        write_mask: 0x00,
    }
}

/// The stencil state mask sources draw with: always pass,
/// replace the stencil with the bound reference value
/// (1 for Mask bindings, 0 for DodgeMask bindings).
fn stencil_write_state() -> wgpu::StencilState {
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Always,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Replace,
    };
    wgpu::StencilState {
        front: face,
        back: face,
        read_mask: 0x00,
        write_mask: 0xFF,
    }
}

fn depth_stencil_state(stencil: wgpu::StencilState) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: wgpu::TextureFormat::Depth24PlusStencil8,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Always),
        stencil,
        bias: wgpu::DepthBiasState::default(),
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    slug: &'static str,
    masked: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("torogao part pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
            }],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: if masked {
            Some(depth_stencil_state(stencil_test_state()))
        } else {
            None
        },
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: Some(blend_state(slug)),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The mask-source pipeline: the mask fragment shader,
/// colour writes off, stencil replace on pass.
fn build_mask_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("torogao mask pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
            }],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(depth_stencil_state(stencil_write_state())),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_mask"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The debug-overlay line pipeline (position-only
/// vertices, line list, plain alpha blending).
fn build_line_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("torogao debug line pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<[f32; 2]>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2],
            }],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::blend_slug;

    #[test]
    fn blend_slugs_cover_the_fixture_set() {
        assert_eq!(blend_slug("Normal"), ("normal", true));
        assert_eq!(blend_slug("Multiply"), ("multiply", true));
        assert_eq!(blend_slug("Screen"), ("screen", true));
        assert_eq!(blend_slug("AddGlow"), ("linear_dodge", true));
        assert_eq!(blend_slug("ClipToLower"), ("clip_to_lower", true));
        assert_eq!(blend_slug("SomethingElse"), ("normal", false));
    }
}
