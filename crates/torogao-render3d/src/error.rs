//! One typed error for the whole crate. No panics cross a
//! public boundary; every failure names what was being loaded
//! or executed when it happened.

use std::fmt;

/// Errors raised by the API-agnostic layers (decode, pose,
/// scene, comparison) and by the backend boundary.
#[derive(Debug)]
pub enum RenderError {
    /// The GLB bytes could not be parsed as a glTF document.
    GltfParse(String),
    /// An accessor's data is malformed or out of bounds.
    Accessor {
        /// glTF accessor index.
        index: usize,
        /// What was wrong.
        detail: String,
    },
    /// A structural fact the renderer requires is absent
    /// (a skin, a named morph target, a joint node...).
    Missing(&'static str),
    /// A document (clip, skeleton def, channels, probe dump)
    /// failed to parse or bind.
    Document(String),
    /// A pose was requested for a frame outside the arc.
    FrameOutOfRange {
        /// Requested plate frame.
        frame: u32,
        /// Total plate frames in the arc.
        frames: u32,
    },
    /// The backend (below the boundary) failed.
    Backend(String),
    /// Filesystem failure while reading inputs or writing
    /// evidence.
    Io {
        /// Path being accessed.
        path: String,
        /// The OS error text.
        detail: String,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::GltfParse(detail) => write!(f, "GLB parse failed: {detail}"),
            RenderError::Accessor { index, detail } => {
                write!(f, "accessor {index}: {detail}")
            }
            RenderError::Missing(what) => write!(f, "required data missing: {what}"),
            RenderError::Document(detail) => write!(f, "document error: {detail}"),
            RenderError::FrameOutOfRange { frame, frames } => {
                write!(f, "frame {frame} outside 0..{frames}")
            }
            RenderError::Backend(detail) => write!(f, "backend error: {detail}"),
            RenderError::Io { path, detail } => write!(f, "io at {path}: {detail}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<std::io::Error> for RenderError {
    fn from(error: std::io::Error) -> Self {
        RenderError::Io {
            path: String::new(),
            detail: error.to_string(),
        }
    }
}

/// Read a file into memory, tagging failures with the path.
pub fn read_file(path: &std::path::Path) -> Result<Vec<u8>, RenderError> {
    std::fs::read(path).map_err(|error| RenderError::Io {
        path: path.display().to_string(),
        detail: error.to_string(),
    })
}

/// Write bytes to a file, tagging failures with the path.
pub fn write_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), RenderError> {
    std::fs::write(path, bytes).map_err(|error| RenderError::Io {
        path: path.display().to_string(),
        detail: error.to_string(),
    })
}
