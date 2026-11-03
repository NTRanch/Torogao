//! SDF face shading (T-11 Torogao half, D-740): a two-band cel
//! face material that consumes a painted-terminator SDF texture.
//!
//! The texture is painted per heroine in the strict face-UV
//! space the lumen census certifies (`lumen.face-uv-contract`):
//! each texel's red channel stores the light threshold at which
//! that point of the face flips from lit to shadow as the light
//! sweeps from frontal to the side — the painted terminator.
//! Shading is then a comparison, not a lighting computation.
//!
//! The comparison, precisely: the light's signed horizontal
//! direction is `h = sin(azimuth − facing)` ([`light_side`]).
//! The texture is authored for one sweep direction and sampled
//! **mirrored** when the light is on the other side
//! ([`mirrored_u`]), so in sampled space the dark side is always
//! the side away from the light. A texel with calibrated painted
//! value `p` is lit iff `|h| <= p`: frontal light (`h = 0`)
//! lights everything, and as `|h|` grows the terminator travels
//! across the face in painted order — texels painted low shadow
//! first, the nose ridge (painted high) holds the light longest.
//!
//! Scope, as commissioned: **faces only, one light height.** The
//! light travels in azimuth around the face at the material's
//! fixed height ([`LIGHT_HEIGHT`]); there is no elevation input,
//! no body shading, and no second light.
//!
//! Calibration is the three-number range-debug workflow from
//! the source technique: the calibration triple (min, mid, max)
//! brackets the painted value range — in debug mode, texels
//! below min render red, above max render blue, and the mid
//! value renders as a green line. The painter adjusts min until
//! no red remains, max until no blue remains, and places mid at
//! the face centre; production shading then uses the calibrated
//! range to normalize the painted values before the threshold
//! comparison.
//!
//! The WGSL in [`SDF_FACE_SHADER_WGSL`] and the CPU reference in
//! [`shade_face`] are the same computation written twice; the
//! gate tests pin the CPU side and the shader's interface, and
//! the `sdf_face_sweep` example renders the CPU side as
//! calibration evidence for the Owner's eye (evidence, not a
//! verdict).

#![forbid(unsafe_code)]

use crate::error::{AppError, AppResult};

/// The material's one light height: the light direction's fixed
/// vertical component in face-local space. There is deliberately
/// no elevation parameter (T-11 scope: one light height).
pub const LIGHT_HEIGHT: f32 = 0.35;
/// Half-width of the green mid band in range-debug mode.
pub const DEBUG_MID_BAND: f32 = 0.01;
/// The shadow band's brightness multiplier (two-band cel).
pub const SHADOW_BAND: f32 = 0.55;
/// Default terminator softness: the width (in light-sweep
/// units) over which the threshold comparison ramps. Small and
/// fixed for the cel look; 0 would be a hard step.
pub const TERMINATOR_SOFTNESS: f32 = 0.04;

/// The three-number range-debug calibration (D-740): the
/// painted range's floor, its face-centre value, and its
/// ceiling, each in `[0, 1]`, ordered `min <= mid <= max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SdfFaceCalibration {
    /// Painted values above this render blue in debug mode.
    pub max: f32,
    /// The face-centre value; renders as the green line in
    /// debug mode.
    pub mid: f32,
    /// Painted values below this render red in debug mode.
    pub min: f32,
}

impl SdfFaceCalibration {
    /// Build a calibration, refusing non-finite, out-of-range,
    /// or unordered triples.
    pub fn new(min: f32, mid: f32, max: f32) -> AppResult<Self> {
        for (name, value) in [("min", min), ("mid", mid), ("max", max)] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(AppError::Param(format!(
                    "sdf calibration {name} {value} is not a finite value in [0, 1]"
                )));
            }
        }
        if !(min <= mid && mid <= max) {
            return Err(AppError::Param(format!(
                "sdf calibration must satisfy min <= mid <= max, got {min} / {mid} / {max}"
            )));
        }
        Ok(Self { max, mid, min })
    }

    /// The uncalibrated default: the full painted range, mid at
    /// the centre value 0.5.
    pub fn identity() -> Self {
        Self {
            max: 1.0,
            mid: 0.5,
            min: 0.0,
        }
    }

    /// Normalize a painted value through the calibrated range:
    /// min maps to 0, max maps to 1. A degenerate range
    /// (min == max) maps everything to the mid position.
    pub fn normalize(&self, painted: f32) -> f32 {
        let span = self.max - self.min;
        if span <= f32::EPSILON {
            return self.mid;
        }
        ((painted - self.min) / span).clamp(0.0, 1.0)
    }
}

/// The light's signed horizontal direction for a light at
/// `azimuth_rad` (face-local, 0 = straight ahead) and a face
/// turned to `facing_rad`: `sin(azimuth − facing)` in `[−1, 1]`.
/// The sign says which side the light is on (and therefore which
/// way the SDF texture is mirrored for sampling); the magnitude
/// is how far the terminator has swept. The light's height is
/// the material constant, already accounted for in the painted
/// thresholds, so it does not appear here.
pub fn light_side(azimuth_rad: f32, facing_rad: f32) -> f32 {
    (azimuth_rad - facing_rad).sin()
}

/// The UV u to sample for a face UV u under a signed light side:
/// the painted texture is authored for one sweep direction, so
/// a light on the other side samples it mirrored.
pub fn mirrored_u(u: f32, side: f32) -> f32 {
    if side < 0.0 { 1.0 - u } else { u }
}

/// The lit factor for one texel: its calibrated painted value
/// against the light's sweep magnitude, ramped over
/// [`TERMINATOR_SOFTNESS`]. `painted` is the post-mirror sample
/// (see [`mirrored_u`]). 0 = full shadow band, 1 = lit.
pub fn sdf_lit_factor(painted: f32, side: f32, calibration: SdfFaceCalibration) -> f32 {
    let threshold = calibration.normalize(painted);
    ((threshold - side.abs()) / TERMINATOR_SOFTNESS + 0.5).clamp(0.0, 1.0)
}

/// Range-debug colour for one painted value: red below min,
/// blue above max, green on the mid line, greyscale between —
/// the calibration view the painter adjusts against.
pub fn debug_colour(painted: f32, calibration: SdfFaceCalibration) -> [f32; 3] {
    if painted < calibration.min {
        [1.0, 0.0, 0.0]
    } else if painted > calibration.max {
        [0.0, 0.0, 1.0]
    } else if (painted - calibration.mid).abs() <= DEBUG_MID_BAND {
        [0.0, 1.0, 0.0]
    } else {
        [painted, painted, painted]
    }
}

/// The CPU reference shading for one face texel: two-band cel
/// from the painted threshold, or the range-debug colour when
/// `debug` is set. `painted` is the post-mirror SDF sample in
/// production mode (see [`mirrored_u`]); in debug mode it is the
/// raw painted value, because the debug view inspects the
/// texture as authored. Mirrors [`SDF_FACE_SHADER_WGSL`].
pub fn shade_face(
    base: [f32; 3],
    painted: f32,
    side: f32,
    calibration: SdfFaceCalibration,
    debug: bool,
) -> [f32; 3] {
    if debug {
        return debug_colour(painted, calibration);
    }
    let lit = sdf_lit_factor(painted, side, calibration);
    let band = if lit >= 0.5 { 1.0 } else { SHADOW_BAND };
    [base[0] * band, base[1] * band, base[2] * band]
}

/// The material's uniform block, laid out for the WGSL below.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SdfFaceUniforms {
    /// (min, mid, max, debug flag) — the calibration triple and
    /// the range-debug switch (0.0 production, 1.0 debug).
    pub calibration: [f32; 4],
    /// (light side, light height, shadow band, softness) — the
    /// signed light side per frame (its sign mirrors the SDF
    /// sampling in the shader); the rest are the material's
    /// constants, carried so the shader has one source.
    pub light: [f32; 4],
}

impl SdfFaceUniforms {
    /// Pack the uniforms for one frame.
    pub fn new(calibration: SdfFaceCalibration, side: f32, debug: bool) -> Self {
        Self {
            calibration: [
                calibration.min,
                calibration.mid,
                calibration.max,
                if debug { 1.0 } else { 0.0 },
            ],
            light: [side, LIGHT_HEIGHT, SHADOW_BAND, TERMINATOR_SOFTNESS],
        }
    }
}

/// The SDF face material's shader: a face-quad vertex stage and
/// the threshold fragment. Bindings: group 0 binding 0 the
/// uniforms, binding 1 the SDF texture (red channel = painted
/// threshold), binding 2 its sampler, binding 3 the face's base
/// colour texture.
pub const SDF_FACE_SHADER_WGSL: &str = r#"
struct SdfFaceUniforms {
    calibration: vec4<f32>,
    light: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: SdfFaceUniforms;
@group(0) @binding(1) var sdf_texture: texture_2d<f32>;
@group(0) @binding(2) var sdf_sampler: sampler;
@group(0) @binding(3) var base_texture: texture_2d<f32>;

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
    out.clip = vec4<f32>(input.position, 0.0, 1.0);
    out.uv = input.uv;
    return out;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    // The painted texture is authored for one sweep direction:
    // a light on the other side samples it mirrored, so in
    // sampled space the dark side is always away from the light.
    // (Debug mode inspects the texture as authored, unmirrored.)
    var sdf_uv = input.uv;
    if (uniforms.light.x < 0.0 && uniforms.calibration.w <= 0.5) {
        sdf_uv.x = 1.0 - sdf_uv.x;
    }
    let painted = textureSample(sdf_texture, sdf_sampler, sdf_uv).r;
    let base = textureSample(base_texture, sdf_sampler, input.uv).rgb;
    let cal_min = uniforms.calibration.x;
    let cal_mid = uniforms.calibration.y;
    let cal_max = uniforms.calibration.z;
    if (uniforms.calibration.w > 0.5) {
        // Range-debug: red below min, blue above max, green on
        // the mid line, greyscale between.
        if (painted < cal_min) {
            return vec4<f32>(1.0, 0.0, 0.0, 1.0);
        }
        if (painted > cal_max) {
            return vec4<f32>(0.0, 0.0, 1.0, 1.0);
        }
        if (abs(painted - cal_mid) <= 0.01) {
            return vec4<f32>(0.0, 1.0, 0.0, 1.0);
        }
        return vec4<f32>(painted, painted, painted, 1.0);
    }
    let span = cal_max - cal_min;
    var threshold = cal_mid;
    if (span > 0.0000001) {
        threshold = clamp((painted - cal_min) / span, 0.0, 1.0);
    }
    // Lit iff the sweep magnitude has not passed the texel's
    // painted threshold: |h| <= p, ramped over the softness.
    let lit = clamp(
        (threshold - abs(uniforms.light.x)) / uniforms.light.w + 0.5,
        0.0,
        1.0,
    );
    var band = uniforms.light.z;
    if (lit >= 0.5) {
        band = 1.0;
    }
    return vec4<f32>(base * band, 1.0);
}
"#;
