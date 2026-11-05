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
