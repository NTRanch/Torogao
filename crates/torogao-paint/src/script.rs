//! A scripted session (SPEC SS8 Phase 1 gate): a fixed op
//! list plus its canvas, serializable as RON so gate runs
//! are data, not code. The harness bin and the
//! integration test both drive documents through this
//! exact path.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::error::{PaintError, PaintResult};
use crate::format::read_bounded;
use crate::ops::Op;
use crate::png_io::PNG_FILE_BYTES_MAX;

/// Ceiling on a session script's byte length.
pub const SCRIPT_FILE_BYTES_MAX: usize = 1024 * 1024 * 1024;

/// One scripted session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionScript {
    /// Canvas width (must match the base PNG's, if any).
    pub width: u32,
    /// Canvas height.
    pub height: u32,
    /// Optional base PNG file to import first.
    pub base_png: Option<String>,
    /// The ops, applied in order.
    pub ops: Vec<Op>,
}

impl SessionScript {
    /// Load a script from a RON file.
    pub fn load(path: &Path) -> PaintResult<SessionScript> {
        let bytes = read_bounded(path, SCRIPT_FILE_BYTES_MAX)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| PaintError::format("session script is not UTF-8"))?;
        ron::from_str(&text)
            .map_err(|err| PaintError::format(format!("session script parse failed: {err}")))
    }

    /// Run the script into a fresh document.
    pub fn run(&self) -> PaintResult<Document> {
        let mut doc = match &self.base_png {
            Some(path) => {
                let bytes = read_bounded(Path::new(path), PNG_FILE_BYTES_MAX)?;
                let doc = Document::from_png_bytes(&bytes, path)?;
                if doc.width() != self.width || doc.height() != self.height {
                    return Err(PaintError::format(format!(
                        "script canvas {}x{} disagrees with base {}x{}",
                        self.width,
                        self.height,
                        doc.width(),
                        doc.height()
                    )));
                }
                doc
            }
            None => Document::new_empty(self.width, self.height)?,
        };
        for op in &self.ops {
            doc.apply_op(op.clone())?;
        }
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerId;
    use crate::ops::{GradientShape, GradientSpec, GradientStop, Rect};
    use crate::stroke::{GrainSpec, Stamp, StampMode};

    /// Regression (richness experiment, 2026-11-27): the
    /// Phase 2 ops must survive the harness's RON
    /// round trip — serialise, parse back, replay. A
    /// derived enum serialisation for the gradient shape
    /// wrote a bare RON identifier that the op enum's
    /// internally-tagged parse could not read back; the
    /// shape now serialises as a string, like BlendMode.
    #[test]
    fn phase2_ops_survive_a_ron_round_trip() {
        let script = SessionScript {
            width: 16,
            height: 16,
            base_png: None,
            ops: vec![
                Op::LayerAdd {
                    id: LayerId(1),
                    name: "shade".to_string(),
                    after: None,
                },
                Op::GradientFill {
                    layer: LayerId(1),
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 16,
                        height: 16,
                    },
                    gradient: GradientSpec {
                        shape: GradientShape::Radial,
                        from: [8, 8],
                        to: [16, 8],
                        stops: vec![
                            GradientStop {
                                at: 0,
                                color: [10, 20, 30, 255],
                            },
                            GradientStop {
                                at: 255,
                                color: [200, 210, 220, 255],
                            },
                        ],
                    },
                },
                Op::TexturedStampRun {
                    layer: LayerId(1),
                    mode: StampMode::Paint,
                    color: [250, 240, 230, 255],
                    opacity: 200,
                    flow: 200,
                    stamps: vec![Stamp {
                        x_fixed: 8 * 64,
                        y_fixed: 8 * 64,
                        radius_px: 5,
                        alpha: 255,
                    }],
                    grain: GrainSpec {
                        seed: 3,
                        cell_px: 4,
                        strength: 180,
                        octaves: 2,
                    },
                },
            ],
        };
        let text = ron::to_string(&script).expect("serialise");
        let back: SessionScript = ron::from_str(&text).expect("parse back");
        assert_eq!(back.ops, script.ops);
        let doc = back.run().expect("replay");
        assert_eq!(doc.ops().len(), 3);
    }
}
