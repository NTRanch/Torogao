//! The known-option set for `QC_UNUSED_OPTION`, carried as
//! data.
//!
//! Honest provenance: no single scene-spec compiler exists in
//! the torogao tree at this commit whose source an option set
//! could be derived from mechanically — the scene format
//! (`torogao-scene/1`), the motion-clip format
//! (`torogao-motion-clip/1`), and the render/gen crates each
//! consume their own vocabulary. So the set below is seeded
//! from the option vocabulary those in-tree formats and the
//! RENDER-QC-CONTRACT name, and is meant to grow as data when
//! compilers grow: extend the list, never the mechanism. A
//! caller may replace the set wholesale with `--known-options`
//! (a JSON array of strings, or `{"options": [...]}`).

use crate::error::QcError;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

/// The built-in known-option set, sorted.
pub const KNOWN_OPTIONS: &[&str] = &[
    "assets",
    "audio",
    "camera",
    "color_grade",
    "cues",
    "denoise",
    "duration_frames",
    "elements",
    "fps",
    "frame",
    "height",
    "identity_lock",
    "lighting",
    "loop",
    "output",
    "physics",
    "quality",
    "resolution",
    "samples",
    "seed",
    "tone_mapping",
    "width",
];

/// The built-in set as a lookup structure.
pub fn builtin_set() -> BTreeSet<String> {
    KNOWN_OPTIONS.iter().map(|s| (*s).to_string()).collect()
}

/// Loads a caller-supplied known-option set from a JSON file:
/// either a bare array of strings or an object with an
/// `options` array. Anything else is a typed error.
pub fn load_set(path: &Path) -> Result<BTreeSet<String>, QcError> {
    let bytes = std::fs::read(path).map_err(|err| {
        QcError::new(
            "QC-E-OPTIONS-READ",
            format!("cannot read known-options file {}: {err}", path.display()),
        )
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|err| {
        QcError::new(
            "QC-E-OPTIONS-PARSE",
            format!(
                "known-options file {} is not valid JSON: {err}",
                path.display()
            ),
        )
    })?;
    let items = match &value {
        Value::Array(items) => items,
        Value::Object(map) => match map.get("options") {
            Some(Value::Array(items)) => items,
            _ => {
                return Err(QcError::new(
                    "QC-E-OPTIONS-SHAPE",
                    "known-options object must carry an `options` array",
                ));
            }
        },
        _ => {
            return Err(QcError::new(
                "QC-E-OPTIONS-SHAPE",
                "known-options file must be a JSON array or an object with `options`",
            ));
        }
    };
    let mut set = BTreeSet::new();
    for item in items {
        match item.as_str() {
            Some(name) => {
                set.insert(name.to_string());
            }
            None => {
                return Err(QcError::new(
                    "QC-E-OPTIONS-SHAPE",
                    "known-options entries must be strings",
                ));
            }
        }
    }
    Ok(set)
}
