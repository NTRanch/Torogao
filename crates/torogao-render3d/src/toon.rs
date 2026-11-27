//! The toon shading model's data contract (gap G1; Brief
//! Amendment A3 and the R6 channel assignment): the packed
//! lightmap, the per-ID ramp, the painted face SDF and the
//! hair sphere map, plus the shading settings the backend
//! uploads as uniforms. API-agnostic packets only — the
//! Vulkan side consumes these verbatim, and the Blender
//! parity fixture implements the same contract from the
//! same files.
//!
//! Determinism: textures are raw RGBA8 bytes authored by
//! the scene pipeline. Nothing here reads a clock or an
//! image codec, so identical inputs render identically.

use std::path::Path;

use crate::error::RenderError;

/// Hard bound on one texture dimension. The contract's
/// largest texture is 1024; 4096 leaves headroom without
/// admitting unbounded uploads.
pub const MAX_TEXTURE_DIM: u32 = 4096;
/// The ramp's contract dimensions: 256 lighting steps by
/// 32 rows (16 material IDs x warm/cool row banks).
pub const RAMP_WIDTH: u32 = 256;
pub const RAMP_HEIGHT: u32 = 32;
/// The per-ID parameter table's contract dimensions:
/// one column per material ID, five rows (see
/// [`ToonMaterialParams`] for the row assignment).
pub const PARAMS_WIDTH: u32 = 16;
pub const PARAMS_HEIGHT: u32 = 5;

/// One RGBA8 texture, row-major with row 0 at v = 0 (the
/// upload order the backend and the Blender fixture both
/// use).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToonImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl ToonImage {
    /// A texture from raw bytes, with the byte count and
    /// dimensions checked against the contract bounds.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<ToonImage, RenderError> {
        if width == 0 || height == 0 || width > MAX_TEXTURE_DIM || height > MAX_TEXTURE_DIM {
            return Err(RenderError::Document(format!(
                "toon texture dimensions {width}x{height} outside 1..={MAX_TEXTURE_DIM}"
            )));
        }
        let want = (width as usize) * (height as usize) * 4;
        if rgba.len() != want {
            return Err(RenderError::Document(format!(
                "toon texture {width}x{height} needs {want} bytes, got {}",
                rgba.len()
            )));
        }
        Ok(ToonImage {
            width,
            height,
            rgba,
        })
    }

    /// A solid-colour texture (backend dummies, tests).
    pub fn solid(width: u32, height: u32, px: [u8; 4]) -> Result<ToonImage, RenderError> {
        let count = (width as usize) * (height as usize);
        let mut rgba = Vec::with_capacity(count * 4);
        for _ in 0..count {
            rgba.extend_from_slice(&px);
        }
        ToonImage::new(width, height, rgba)
    }

    /// Load a raw `.rgba` file with declared dimensions
    /// (the fixture/manifest format; no image codec enters
    /// the render path).
    pub fn load_rgba(path: &Path, width: u32, height: u32) -> Result<ToonImage, RenderError> {
        let bytes = std::fs::read(path)
            .map_err(|e| RenderError::Document(format!("toon texture {}: {e}", path.display())))?;
        ToonImage::new(width, height, bytes)
    }

    /// The material ID at one texel of a packed texture:
    /// the alpha channel in steps of 1/15, rounded to the
    /// nearest step. The CPU mirror of the shader's ID
    /// decode (which samples with a NEAREST filter so IDs
    /// never blend across island borders).
    pub fn material_id_at(&self, x: u32, y: u32) -> u32 {
        assert!(x < self.width && y < self.height, "texel outside texture");
        let idx = ((y * self.width + x) * 4 + 3) as usize;
        let a = f32::from(self.rgba[idx]) / 255.0;
        ((a * 15.0 + 0.5) as u32).min(15)
    }
}

/// The four contract textures.
#[derive(Debug, Clone)]
pub struct ToonTextures {
    /// R = AO, G = specular mask, B = metalness,
    /// A = material ID (steps of 1/15). Sampled at UV1.
    pub packed: ToonImage,
    /// 256x32: u = remapped lighting term, v = ID row
    /// (rows 0..15 warm, 16..31 cool).
    pub ramp: ToonImage,
    /// Face plate only, sampled at UV0: R = painted SDF,
    /// G = blush mask, B = nose mask, A = lip mask.
    pub face_sdf: ToonImage,
    /// Hair sheen sphere map, sampled by view-space normal.
    pub sphere: ToonImage,
    /// Per-ID parameter table (16x5, see
    /// [`ToonMaterialParams`]). Optional at the API:
    /// `None` makes the backend upload
    /// [`ToonParams::default`], whose entries reproduce
    /// the pre-gap-fix shading except for the back-face
    /// colour (the grammar's deep blue, GAP-5).
    pub params: Option<ToonImage>,
}

impl ToonTextures {
    /// Contract-shape checks beyond per-image validity.
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.ramp.width != RAMP_WIDTH || self.ramp.height != RAMP_HEIGHT {
            return Err(RenderError::Document(format!(
                "toon ramp must be {RAMP_WIDTH}x{RAMP_HEIGHT}, got {}x{}",
                self.ramp.width, self.ramp.height
            )));
        }
        if let Some(params) = &self.params {
            if params.width != PARAMS_WIDTH || params.height != PARAMS_HEIGHT {
                return Err(RenderError::Document(format!(
                    "toon params must be {PARAMS_WIDTH}x{PARAMS_HEIGHT}, got {}x{}",
                    params.width, params.height
                )));
            }
        }
        Ok(())
    }
}

/// One material ID's row of the parameter table: the
/// colour/parameter set the shading grammar requires per
/// material (grammar SS2/SS4.6 in the StellarToon/LSCherry
/// study) that the ramp texture cannot carry. Encoded
/// into the 16x5 params texture, one column per ID:
///
/// - row 0: SSS colour (RGB), A = SSS band width / 0.5
/// - row 1: rim colour (RGB), A = rim size
/// - row 2: R = rim smooth, G = specular tint,
///   B = (specular exponent - 1) / 255, A = 255
/// - row 3: back-face colour (RGB), A = 255
/// - row 4: hair highlight colour (RGB),
///   A = highlight strength / 4
///
/// Colours are linear, 0..=1. The shader decodes with
/// texelFetch, so no filtering ever blends two IDs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToonMaterialParams {
    /// SSS transition tint; black disables the band
    /// (GAP-1).
    pub sss_color: [f32; 3],
    /// SSS band width in ramp-u units, 0..=0.5.
    pub sss_width: f32,
    /// Rim colour (GAP-2).
    pub rim_color: [f32; 3],
    /// Rim band width, 0..=1 (fraction of the Fresnel
    /// range the band occupies from the silhouette in).
    pub rim_size: f32,
    /// Rim edge softness, 0..=1.
    pub rim_smooth: f32,
    /// Blinn exponent, 1..=256 (GAP-3; the pre-gap-fix
    /// shader constant was 48).
    pub spec_exponent: f32,
    /// Base-colour tint strength for the specular,
    /// independent of metalness, 0..=1 (GAP-3).
    pub spec_tint: f32,
    /// Back-face colour (GAP-5).
    pub back_color: [f32; 3],
    /// Hair planar-band highlight colour (GAP-4).
    pub highlight_color: [f32; 3],
    /// Hair planar-band highlight strength, 0..=4.
    pub highlight_strength: f32,
}

impl Default for ToonMaterialParams {
    fn default() -> Self {
        ToonMaterialParams {
            sss_color: [0.0, 0.0, 0.0],
            sss_width: 0.25,
            rim_color: [1.0, 1.0, 1.0],
            rim_size: 0.5,
            rim_smooth: 0.5,
            spec_exponent: 48.0,
            spec_tint: 0.0,
            back_color: [0.08, 0.10, 0.30],
            highlight_color: [1.0, 1.0, 1.0],
            highlight_strength: 1.0,
        }
    }
}

/// The full per-ID parameter table (16 entries).
#[derive(Debug, Clone, Copy, Default)]
pub struct ToonParams {
    pub entries: [ToonMaterialParams; 16],
}

impl ToonParams {
    /// Encode the table as the 16x5 params texture. Any
    /// out-of-range entry is a typed error: a corrupt
    /// table must fail at authoring time, never shade
    /// wrong silently.
    pub fn to_image(&self) -> Result<ToonImage, RenderError> {
        let bad = |what: &str| RenderError::Document(format!("toon params {what} out of range"));
        let byte = |v: f32| -> u8 { (v.clamp(0.0, 1.0) * 255.0).round() as u8 };
        let colour = |c: [f32; 3]| -> Result<[u8; 3], RenderError> {
            if c.iter().any(|v| !(0.0..=1.0).contains(v)) {
                return Err(bad("colour"));
            }
            Ok([byte(c[0]), byte(c[1]), byte(c[2])])
        };
        let mut rgba = vec![0u8; (PARAMS_WIDTH * PARAMS_HEIGHT * 4) as usize];
        let put = |rgba: &mut Vec<u8>, id: usize, row: usize, px: [u8; 4]| {
            let idx = (row * PARAMS_WIDTH as usize + id) * 4;
            rgba[idx..idx + 4].copy_from_slice(&px);
        };
        for (id, e) in self.entries.iter().enumerate() {
            if !(0.0..=0.5).contains(&e.sss_width) {
                return Err(bad("sss_width"));
            }
            if !(0.0..=1.0).contains(&e.rim_size) || !(0.0..=1.0).contains(&e.rim_smooth) {
                return Err(bad("rim size/smooth"));
            }
            if !(1.0..=256.0).contains(&e.spec_exponent) {
                return Err(bad("spec_exponent"));
            }
            if !(0.0..=1.0).contains(&e.spec_tint) {
                return Err(bad("spec_tint"));
            }
            if !(0.0..=4.0).contains(&e.highlight_strength) {
                return Err(bad("highlight_strength"));
            }
            let sss = colour(e.sss_color)?;
            put(
                &mut rgba,
                id,
                0,
                [sss[0], sss[1], sss[2], byte(e.sss_width / 0.5)],
            );
            let rim = colour(e.rim_color)?;
            put(&mut rgba, id, 1, [rim[0], rim[1], rim[2], byte(e.rim_size)]);
            put(
                &mut rgba,
                id,
                2,
                [
                    byte(e.rim_smooth),
                    byte(e.spec_tint),
                    byte((e.spec_exponent - 1.0) / 255.0),
                    255,
                ],
            );
            let back = colour(e.back_color)?;
            put(&mut rgba, id, 3, [back[0], back[1], back[2], 255]);
            let hl = colour(e.highlight_color)?;
            put(
                &mut rgba,
                id,
                4,
                [hl[0], hl[1], hl[2], byte(e.highlight_strength / 4.0)],
            );
        }
        ToonImage::new(PARAMS_WIDTH, PARAMS_HEIGHT, rgba)
    }
}

/// Shading settings for the toon model, uploaded to the
/// character shader as uniforms.
#[derive(Debug, Clone, Copy)]
pub struct ToonSettings {
    /// Skin index of the head joint; negative disables the
    /// face-SDF branch (faces shade by the plain ramp).
    pub head_joint: i32,
    /// Head-local forward axis (unit, head joint space).
    /// The face SDF is sampled by the light's angle
    /// against the head basis these two axes define.
    pub head_forward_local: [f32; 3],
    /// Head-local right axis (unit, head joint space).
    pub head_right_local: [f32; 3],
    /// 0 = warm ramp rows, 1 = cool ramp rows.
    pub warm_cool: f32,
    /// Inverted-hull outline width in metres at vertex
    /// alpha 1 (Amendment A4).
    pub outline_width_m: f32,
    /// Rim-light strength multiplier.
    pub rim_strength: f32,
    /// Face SDF smoothstep half-width.
    pub sdf_softness: f32,
    /// Scene-level shadow tint colour (GAP-6): injected
    /// into the shaded band after the sun/fill sum,
    /// scaled by `shadow_tint_factor`. Black / zero
    /// factor disables it.
    pub shadow_tint: [f32; 3],
    /// Shadow tint injection strength, 0..=1.
    pub shadow_tint_factor: f32,
    /// Display-transform calibration (GAP-7): exposure
    /// gain applied to the linear colour before the AgX
    /// display transform. 1.0 = identity.
    pub exposure: f32,
    /// World-colour saturation multiplier (GAP-7): the
    /// ambient the character shader consumes is
    /// HSV-rebalanced by this and `world_value`.
    /// 1.0 = identity.
    pub world_saturation: f32,
    /// World-colour value multiplier (GAP-7). 1.0 =
    /// identity.
    pub world_value: f32,
    /// Ambient value enhance (GAP-7): the calibrated
    /// world colour is scaled by (1 + value_enhance).
    /// 0.0 = identity.
    pub value_enhance: f32,
    /// Face blush tint colour (GAP-9; data since the
    /// gap-fix round — was a GLSL constant).
    pub blush_color: [f32; 3],
    /// Blush tint strength, 0..=1.
    pub blush_strength: f32,
    /// Nose-mask darkening strength, 0..=1 (GAP-9).
    pub nose_strength: f32,
    /// Face lip tint colour (GAP-9).
    pub lip_color: [f32; 3],
    /// Lip tint strength, 0..=1.
    pub lip_strength: f32,
    /// Hair planar-band frequency (GAP-4): band repeats
    /// per unit of the planar along-length coordinate.
    pub band_frequency: f32,
    /// Hair band layout mix (GAP-4): 0 = low-frequency
    /// layout only, 1 = high-frequency layout only.
    pub band_mix: f32,
    /// Hair band phase offset in band cycles (GAP-4).
    pub band_offset: f32,
    /// Hair band window start on the along-length
    /// coordinate (GAP-4).
    pub band_window_lo: f32,
    /// Hair band window end (GAP-4).
    pub band_window_hi: f32,
    /// Hair band view dependence (GAP-4): 0 = bands at
    /// full strength at every view angle, 1 = bands
    /// fade out facing the camera.
    pub band_view: f32,
}

impl Default for ToonSettings {
    fn default() -> Self {
        ToonSettings {
            head_joint: -1,
            head_forward_local: [0.0, 0.0, -1.0],
            head_right_local: [1.0, 0.0, 0.0],
            warm_cool: 0.0,
            outline_width_m: 0.0035,
            rim_strength: 0.35,
            sdf_softness: 0.08,
            shadow_tint: [0.0, 0.0, 0.0],
            shadow_tint_factor: 0.0,
            exposure: 1.0,
            world_saturation: 1.0,
            world_value: 1.0,
            value_enhance: 0.0,
            // The pre-gap-fix GLSL constants, now data
            // (GAP-9): identical values, so default
            // renders are unchanged by the move.
            blush_color: [0.72, 0.30, 0.26],
            blush_strength: 0.55,
            nose_strength: 0.15,
            lip_color: [0.55, 0.16, 0.14],
            lip_strength: 0.6,
            band_frequency: 18.0,
            band_mix: 0.5,
            band_offset: 0.0,
            band_window_lo: 0.15,
            band_window_hi: 0.95,
            band_view: 0.5,
        }
    }
}

impl ToonSettings {
    /// Range checks: every value lands in a shader uniform,
    /// so an out-of-range setting is a typed error here,
    /// never a silent visual corruption.
    pub fn validate(&self) -> Result<(), RenderError> {
        let bad = |what: &str| RenderError::Document(format!("toon setting {what} out of range"));
        if !(0.0..=1.0).contains(&self.warm_cool) {
            return Err(bad("warm_cool"));
        }
        if !(0.0..=0.05).contains(&self.outline_width_m) {
            return Err(bad("outline_width_m"));
        }
        if !(0.0..=4.0).contains(&self.rim_strength) {
            return Err(bad("rim_strength"));
        }
        if !(0.005..=0.5).contains(&self.sdf_softness) {
            return Err(bad("sdf_softness"));
        }
        if self.shadow_tint.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(bad("shadow_tint"));
        }
        if !(0.0..=1.0).contains(&self.shadow_tint_factor) {
            return Err(bad("shadow_tint_factor"));
        }
        if !(0.05..=8.0).contains(&self.exposure) {
            return Err(bad("exposure"));
        }
        if !(0.0..=4.0).contains(&self.world_saturation) {
            return Err(bad("world_saturation"));
        }
        if !(0.0..=4.0).contains(&self.world_value) {
            return Err(bad("world_value"));
        }
        if !(0.0..=2.0).contains(&self.value_enhance) {
            return Err(bad("value_enhance"));
        }
        if self.blush_color.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(bad("blush_color"));
        }
        if self.lip_color.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(bad("lip_color"));
        }
        if !(0.0..=1.0).contains(&self.blush_strength)
            || !(0.0..=1.0).contains(&self.nose_strength)
            || !(0.0..=1.0).contains(&self.lip_strength)
        {
            return Err(bad("face tint strength"));
        }
        if !(0.0..=128.0).contains(&self.band_frequency) {
            return Err(bad("band_frequency"));
        }
        if !(0.0..=1.0).contains(&self.band_mix) || !(0.0..=1.0).contains(&self.band_view) {
            return Err(bad("band mix/view"));
        }
        if !(-16.0..=16.0).contains(&self.band_offset) {
            return Err(bad("band_offset"));
        }
        if !(0.0..=1.0).contains(&self.band_window_lo)
            || !(0.0..=1.0).contains(&self.band_window_hi)
            || self.band_window_lo >= self.band_window_hi
        {
            return Err(bad("band_window"));
        }
        if self.head_joint >= 0 {
            for axis in [self.head_forward_local, self.head_right_local] {
                let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
                if (len - 1.0).abs() > 1e-3 {
                    return Err(bad("head axis length"));
                }
            }
        }
        Ok(())
    }
}

/// Everything the backend needs to shade one scene with
/// the toon model: contract textures + settings.
#[derive(Debug, Clone)]
pub struct ToonData {
    pub textures: ToonTextures,
    pub settings: ToonSettings,
}

impl ToonData {
    pub fn validate(&self) -> Result<(), RenderError> {
        self.textures.validate()?;
        self.settings.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> ToonImage {
        ToonImage::solid(RAMP_WIDTH, RAMP_HEIGHT, [255, 255, 255, 255]).unwrap()
    }

    fn textures() -> ToonTextures {
        ToonTextures {
            packed: ToonImage::solid(4, 4, [200, 100, 0, 0]).unwrap(),
            ramp: ramp(),
            face_sdf: ToonImage::solid(4, 4, [128, 0, 0, 0]).unwrap(),
            sphere: ToonImage::solid(4, 4, [255, 255, 255, 255]).unwrap(),
            params: None,
        }
    }

    // -- validation half -----------------------------------

    #[test]
    fn well_formed_textures_validate() {
        assert!(textures().validate().is_ok());
        assert!(
            ToonData {
                textures: textures(),
                settings: ToonSettings::default(),
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn material_id_round_trips_all_sixteen_steps() {
        // Alpha bytes are authored as id * 17 (255 / 15);
        // the decode must return every ID exactly.
        for id in 0..16u32 {
            let img = ToonImage::solid(1, 1, [0, 0, 0, (id * 17) as u8]).unwrap();
            assert_eq!(img.material_id_at(0, 0), id, "id {id}");
        }
    }

    #[test]
    fn head_axes_checked_only_when_face_branch_on() {
        let mut settings = ToonSettings {
            head_joint: -1,
            head_forward_local: [5.0, 0.0, 0.0],
            ..ToonSettings::default()
        };
        assert!(settings.validate().is_ok());
        settings.head_joint = 32;
        assert!(settings.validate().is_err());
    }

    // -- adversarial half ----------------------------------

    #[test]
    fn byte_count_mismatch_is_a_typed_error() {
        assert!(ToonImage::new(4, 4, vec![0u8; 63]).is_err());
        assert!(ToonImage::new(4, 4, vec![0u8; 65]).is_err());
    }

    #[test]
    fn degenerate_dimensions_are_typed_errors() {
        assert!(ToonImage::new(0, 4, Vec::new()).is_err());
        assert!(ToonImage::new(4, 0, Vec::new()).is_err());
        assert!(ToonImage::new(MAX_TEXTURE_DIM + 1, 1, Vec::new()).is_err());
    }

    #[test]
    fn wrong_ramp_shape_is_a_typed_error() {
        let mut tex = textures();
        tex.ramp = ToonImage::solid(128, 32, [255; 4]).unwrap();
        assert!(tex.validate().is_err());
        tex.ramp = ToonImage::solid(256, 16, [255; 4]).unwrap();
        assert!(tex.validate().is_err());
    }

    // -- gap-fix round: params table + new settings -----

    #[test]
    fn default_params_encode_to_the_contract_shape() {
        let img = ToonParams::default().to_image().unwrap();
        assert_eq!(img.width, PARAMS_WIDTH);
        assert_eq!(img.height, PARAMS_HEIGHT);
        // ID 2 (hair), row 2: exponent 48 encodes as
        // (48 - 1) / 255 of the byte range.
        let idx = ((2 * PARAMS_WIDTH + 2) * 4 + 2) as usize;
        assert_eq!(img.rgba[idx], 47);
    }

    #[test]
    fn params_round_trip_the_shader_decode() {
        // Mirror of the shader's texelFetch decode: the
        // encoded bytes must return the authored values
        // within one byte step.
        let mut params = ToonParams::default();
        params.entries[7] = ToonMaterialParams {
            sss_color: [0.8, 0.3, 0.1],
            sss_width: 0.3,
            rim_color: [0.2, 0.4, 0.9],
            rim_size: 0.25,
            rim_smooth: 0.75,
            spec_exponent: 128.0,
            spec_tint: 0.6,
            back_color: [0.1, 0.1, 0.4],
            highlight_color: [1.0, 0.9, 0.7],
            highlight_strength: 2.0,
        };
        let img = params.to_image().unwrap();
        let texel = |row: usize| -> [f32; 4] {
            let idx = (row * PARAMS_WIDTH as usize + 7) * 4;
            [
                f32::from(img.rgba[idx]) / 255.0,
                f32::from(img.rgba[idx + 1]) / 255.0,
                f32::from(img.rgba[idx + 2]) / 255.0,
                f32::from(img.rgba[idx + 3]) / 255.0,
            ]
        };
        let step = 1.0 / 255.0;
        let close = |a: f32, b: f32| (a - b).abs() <= step;
        let r0 = texel(0);
        assert!(close(r0[0], 0.8) && close(r0[1], 0.3) && close(r0[2], 0.1));
        assert!(close(r0[3] * 0.5, 0.3), "sss width {}", r0[3] * 0.5);
        let r2 = texel(2);
        assert!(close(r2[0], 0.75) && close(r2[1], 0.6));
        assert!(
            close(1.0 + r2[2] * 255.0, 128.0),
            "exponent {}",
            1.0 + r2[2] * 255.0
        );
        let r4 = texel(4);
        assert!(
            (r4[3] * 4.0 - 2.0).abs() <= 4.0 / 255.0,
            "highlight strength {}",
            r4[3] * 4.0
        );
    }

    #[test]
    fn gap_fix_settings_defaults_validate() {
        // The defaults are the identity for every new
        // term (GAP-6/7) and the old constants for the
        // face tints (GAP-9).
        assert!(ToonSettings::default().validate().is_ok());
    }

    #[test]
    fn wrong_params_shape_is_a_typed_error() {
        let mut tex = textures();
        tex.params = Some(ToonImage::solid(16, 4, [0; 4]).unwrap());
        assert!(tex.validate().is_err());
        tex.params = Some(ToonParams::default().to_image().unwrap());
        assert!(tex.validate().is_ok());
    }

    #[test]
    fn out_of_range_params_are_typed_errors() {
        let mut params = ToonParams::default();
        params.entries[0].spec_exponent = 0.5;
        assert!(params.to_image().is_err());
        params.entries[0].spec_exponent = 300.0;
        assert!(params.to_image().is_err());
        let mut params = ToonParams::default();
        params.entries[3].sss_color = [1.2, 0.0, 0.0];
        assert!(params.to_image().is_err());
        let mut params = ToonParams::default();
        params.entries[3].highlight_strength = f32::NAN;
        assert!(params.to_image().is_err());
    }

    #[test]
    fn out_of_range_gap_fix_settings_are_typed_errors() {
        let base = ToonSettings::default();
        for settings in [
            ToonSettings {
                exposure: 0.0,
                ..base
            },
            ToonSettings {
                world_saturation: f32::NAN,
                ..base
            },
            ToonSettings {
                shadow_tint_factor: 1.5,
                ..base
            },
            ToonSettings {
                blush_strength: -0.1,
                ..base
            },
            ToonSettings {
                band_window_lo: 0.9,
                band_window_hi: 0.2,
                ..base
            },
            ToonSettings {
                band_frequency: 200.0,
                ..base
            },
        ] {
            assert!(settings.validate().is_err(), "{settings:?}");
        }
    }

    #[test]
    fn out_of_range_settings_are_typed_errors() {
        let base = ToonSettings::default();
        for settings in [
            ToonSettings {
                warm_cool: 1.5,
                ..base
            },
            ToonSettings {
                warm_cool: -0.1,
                ..base
            },
            ToonSettings {
                outline_width_m: -0.001,
                ..base
            },
            ToonSettings {
                outline_width_m: 0.5,
                ..base
            },
            ToonSettings {
                sdf_softness: 0.0,
                ..base
            },
            ToonSettings {
                rim_strength: f32::NAN,
                ..base
            },
        ] {
            assert!(settings.validate().is_err(), "{settings:?}");
        }
    }
}
