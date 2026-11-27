//! The session op log: append-only, one entry per executed
//! tick, carrying the tick's commands, their verdicts, and
//! the resulting state hash.
//!
//! The log is the replay artefact: `state = f(core genesis,
//! op log)`, re-executed headlessly, must reproduce the hash
//! chain exactly ([`crate::Session::replay`]). It serializes
//! through torogao-format's canonical JSON writer, so a log
//! written on one target is byte-identical on every target.

use crate::command::Command;
use crate::core::Verdict;
use crate::hash::Hash;
use serde::{Deserialize, Serialize};
use torogao_author::{Refusal, RefusalClass};

/// One executed tick in the op log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpLogEntry {
    /// The tick executed (1-based).
    pub tick: u64,
    /// The commands the tick consumed, in order.
    pub commands: Vec<Command>,
    /// One verdict per command, in order.
    pub verdicts: Vec<Verdict>,
    /// The state hash after the tick.
    pub hash: Hash,
}

/// An append-only log of [`OpLogEntry`]s. Entries can be
/// appended and read; there is deliberately no mutation or
/// removal API — a session's history is rewritten by starting
/// a new session, never by editing this log.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpLog {
    entries: Vec<OpLogEntry>,
}

impl OpLog {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one executed tick. Entries must arrive in tick
    /// order; the session is the only caller and guarantees it.
    pub fn append(&mut self, entry: OpLogEntry) {
        self.entries.push(entry);
    }

    /// The entries, in tick order.
    #[must_use]
    pub fn entries(&self) -> &[OpLogEntry] {
        &self.entries
    }

    /// How many ticks the log records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log records no ticks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The canonical text form (torogao-format's deterministic
    /// JSON writer: alphabetical keys, compact separators).
    #[must_use]
    pub fn to_text(&self) -> String {
        let value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        torogao_format::jsonfmt::to_d_json(&value)
    }

    /// Parses the canonical text form.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the text is not a
    /// well-formed op log document.
    pub fn from_text(text: &str) -> Result<Self, Refusal> {
        serde_json::from_str(text).map_err(|error| {
            Refusal::new(
                RefusalClass::Malformed,
                format!("op log does not parse: {error}"),
            )
        })
    }
}
