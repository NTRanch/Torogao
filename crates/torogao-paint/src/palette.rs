//! Per-character palettes (SPEC SS4.4): a palette is data,
//! a RON list of (name, RGB) swatches. Loading one returns
//! its SHA-256 alongside, because a document records which
//! palette record it was painted against (SS3.5).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{PaintError, PaintResult};
use crate::hash::sha256_bytes;

/// Swatches in one palette: bounded, like everything else.
pub const SWATCHES_MAX: usize = 1024;
/// Ceiling on a palette file's byte length.
pub const PALETTE_FILE_BYTES_MAX: usize = 1024 * 1024;

/// One palette entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Swatch {
    /// The swatch's name in the character record.
    pub name: String,
    /// The swatch colour.
    pub rgb: [u8; 3],
}

/// A loaded palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    /// The palette's name (a character, a scene, a study).
    pub name: String,
    /// The swatches, in record order.
    pub swatches: Vec<Swatch>,
}

impl Palette {
    /// Load a palette from a RON file, returning the
    /// palette and the file's SHA-256 (hex record form is
    /// the caller's `hash::to_hex`).
    pub fn load(path: &Path) -> PaintResult<(Palette, [u8; 32])> {
        let bytes = crate::format::read_bounded(path, PALETTE_FILE_BYTES_MAX)?;
        let sha = sha256_bytes(&bytes);
        let text = String::from_utf8(bytes)
            .map_err(|_| PaintError::format("palette file is not UTF-8"))?;
        let palette: Palette = ron::from_str(&text)
            .map_err(|err| PaintError::format(format!("palette parse failed: {err}")))?;
        palette.validate()?;
        Ok((palette, sha))
    }

    /// Serialize to RON text (the on-disk record form).
    pub fn to_ron(&self) -> PaintResult<String> {
        self.validate()?;
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::new())
            .map_err(|err| PaintError::format(format!("palette serialize failed: {err}")))
    }

    /// The built-in working palette: paper, ink, and a
    /// small spread, so the editor's panel is never empty
    /// before a character record is loaded. Named as what
    /// it is — working defaults, not a character record.
    pub fn working_defaults() -> Palette {
        let named: [(&str, [u8; 3]); 10] = [
            ("ink", [24, 22, 28]),
            ("paper", [244, 240, 232]),
            ("shadow", [88, 80, 96]),
            ("mid", [152, 144, 156]),
            ("skin light", [246, 205, 178]),
            ("skin shade", [196, 132, 108]),
            ("accent red", [178, 44, 52]),
            ("accent blue", [52, 84, 168]),
            ("accent gold", [206, 158, 58]),
            ("accent green", [74, 138, 92]),
        ];
        Palette {
            name: "working defaults".to_string(),
            swatches: named
                .iter()
                .map(|(name, rgb)| Swatch {
                    name: (*name).to_string(),
                    rgb: *rgb,
                })
                .collect(),
        }
    }

    fn validate(&self) -> PaintResult<()> {
        if self.swatches.len() > SWATCHES_MAX {
            return Err(PaintError::limit(
                "palette swatches",
                self.swatches.len() as u64,
                SWATCHES_MAX as u64,
            ));
        }
        if self.name.is_empty() {
            return Err(PaintError::format("palette name is empty"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_ron_round_trip() {
        let palette = Palette::working_defaults();
        let text = palette.to_ron().expect("serialize");
        let back: Palette = ron::from_str(&text).expect("parse");
        assert_eq!(back, palette);
    }

    #[test]
    fn adversarial_palette_refusals() {
        let unknown_field = "(name: \"x\", swatches: [], bogus: 1)";
        assert!(ron::from_str::<Palette>(unknown_field).is_err());
        let too_many = Palette {
            name: "x".to_string(),
            swatches: (0..SWATCHES_MAX + 1)
                .map(|i| Swatch {
                    name: format!("s{i}"),
                    rgb: [0, 0, 0],
                })
                .collect(),
        };
        assert!(too_many.validate().is_err());
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;

    #[test]
    fn adversarial_palette_empty_name_refuses() {
        let palette = Palette {
            name: String::new(),
            swatches: Vec::new(),
        };
        assert!(palette.to_ron().is_err());
    }
}
