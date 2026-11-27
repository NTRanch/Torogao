//! The runtime save envelope: the save service's document.
//!
//! A core's own save blob is canonical but core-shaped; the
//! envelope is the runtime-shaped wrapper every hosted title
//! shares. It binds the blob to the core id that produced it,
//! the tick it was taken at, and the hash chain's tip at that
//! tick — so a session loading an envelope can verify, after
//! the core restores, that the restored state IS the state
//! the envelope claims (tick and hash must match), whatever
//! the core's own binding checks already established.
//!
//! The envelope is the cross-target save artefact (plan
//! SS3.5): one format for all targets, written through
//! torogao-format's canonical JSON writer, so a save written
//! on Android loads on Windows byte-for-byte.

use crate::hash::Hash;
use serde::{Deserialize, Serialize};
use torogao_author::{Refusal, RefusalClass};

/// The envelope format name and version. A future format is a
/// deliberate, gated act, like every entry in torogao-format's
/// versioned-format registry.
pub const SAVE_FORMAT: &str = "torogao-runtime-save/1";

/// One runtime save: a core blob plus its binding facts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveEnvelope {
    /// The format name; must equal [`SAVE_FORMAT`].
    pub format: String,
    /// The producing core's [`crate::GameCore::core_id`].
    pub core_id: String,
    /// The tick the save was taken at (a tick boundary).
    pub tick: u64,
    /// The hash chain's tip at that tick.
    pub chain_tip: Hash,
    /// How many ticks the producing session's op log held.
    /// Informational: the envelope is a checkpoint, and the
    /// loading session's log restarts from it.
    pub log_len: u64,
    /// The core's canonical save blob, verbatim.
    pub core_blob: String,
}

impl SaveEnvelope {
    /// The canonical text form.
    #[must_use]
    pub fn to_text(&self) -> String {
        let value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        torogao_format::jsonfmt::to_d_json(&value)
    }

    /// Parses and validates the envelope's format stamp.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the text does not parse
    /// or names a different format.
    pub fn from_text(text: &str) -> Result<Self, Refusal> {
        let envelope: Self = serde_json::from_str(text).map_err(|error| {
            Refusal::new(
                RefusalClass::Malformed,
                format!("save envelope does not parse: {error}"),
            )
        })?;
        if envelope.format != SAVE_FORMAT {
            return Err(Refusal::new(
                RefusalClass::Malformed,
                format!(
                    "save envelope format is '{}', this build reads '{SAVE_FORMAT}'",
                    envelope.format
                ),
            ));
        }
        Ok(envelope)
    }
}
