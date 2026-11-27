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
        Ok(())
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
