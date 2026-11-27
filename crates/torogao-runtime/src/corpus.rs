//! The witness corpora: fixed command scripts shared by the
//! conformance suite, the replay harness bins, and the
//! cross-architecture spot gate, so every consumer runs the
//! SAME script against the SAME witnesses.
//!
//! A corpus is one command list per step (one step = one
//! tick). Both corpora are pure data: they live in the
//! runtime crate, contain no floats, and serialize through
//! the op log unchanged.

use crate::command::{Command, Key};

/// The corpus step index at which the interpreter witness's
/// save/load check checkpoints (after this many steps).
pub const INTERPRETER_SAVE_AT: usize = 12;

/// The corpus step index at which the Loop witness's
/// save/load check checkpoints (after this many steps).
pub const LOOP_SAVE_AT: usize = 12;

/// The interpreter corpus, run against the Changr-parity
/// fixture (seed 7): `apply_control` lands on tick 1, is
/// refreshed on tick 20 (re-arming the stat's 6-day lifetime),
/// and the run crosses three day boundaries. Two steps carry
/// host vocabulary alongside the game input to exercise the
/// verdict stream (pointer/key commands are ignored by the
/// interpreter; the game input still applies).
#[must_use]
pub fn interpreter_corpus() -> Vec<Vec<Command>> {
    let mut corpus: Vec<Vec<Command>> = Vec::new();
    // Tick 1: apply control.
    corpus.push(vec![Command::Event {
        id: "apply_control".to_string(),
    }]);
    // Ticks 2..=19: plain advances, with host vocabulary
    // riding along on ticks 5 and 9.
    for tick in 2..=19 {
        match tick {
            5 => corpus.push(vec![
                Command::PointerTap {
                    target: "room".to_string(),
                },
                Command::Advance,
            ]),
            9 => corpus.push(vec![
                Command::KeyPress { key: Key::Confirm },
                Command::Advance,
            ]),
            _ => corpus.push(vec![Command::Advance]),
        }
    }
    // Tick 20: refresh control (re-arms the lifetime window).
    corpus.push(vec![Command::Event {
        id: "apply_control".to_string(),
    }]);
    // Ticks 21..=28: plain advances.
    for _ in 21..=28 {
        corpus.push(vec![Command::Advance]);
    }
    corpus
}

/// The Loop Service corpus, run against the measured dynamics
/// (seed 0x1005_2026_1116, the fixture's own seed): it mirrors
/// the shape of torogao-loop's scripted session — phase
/// advances through the day cycle, escalator and track-down
/// waits, in-window actions, a level boost, chase attempts,
/// and an item purchase — expressed in the runtime vocabulary
/// (one step per script beat; `wait` carries the long tick
/// runs as a single Invoke).
#[must_use]
pub fn loop_corpus() -> Vec<Vec<Command>> {
    let invoke = |verb: &str, args: &[i64]| Command::Invoke {
        verb: verb.to_string(),
        args: args.to_vec(),
    };
    vec![
        vec![invoke("advance_phase", &[])], // Station
        vec![invoke("advance_phase", &[])], // Escalator
        vec![invoke("wait", &[700])],
        vec![invoke("advance_phase", &[])], // TrackDown
        vec![invoke("wait", &[120])],
        vec![invoke("action", &[1, 400])],
        vec![invoke("action", &[1, 400])],
        vec![invoke("action", &[1, 400])],
        vec![invoke("grant_exp", &[1, 2140]), invoke("action", &[1, 500])],
        vec![invoke("chase", &[])],
        vec![invoke("chase", &[])],
        vec![invoke("buy_item", &[1])],
        vec![invoke("advance_phase", &[])], // Evening
        vec![invoke("advance_phase", &[])], // MyRoom
        vec![invoke("advance_phase", &[])], // Morning, next day
    ]
}

// ---------------------------------------------------------------------------
// The corpus file format: `torogao-runtime-corpus/1` (runtime
// plan Rung R2). A corpus on disk is one canonical JSON
// document, written through torogao-format's canonical JSON
// writer, so the same corpus file is byte-identical on every
// target and any title's replay corpus is consumable by the
// one cross-title twin gate (plan SS1.2, SS4.1). The format is
// registered in torogao-format's FORMATS.md.

use serde::{Deserialize, Serialize};
use torogao_author::{Refusal, RefusalClass};

/// The corpus document format name and version.
pub const CORPUS_FORMAT: &str = "torogao-runtime-corpus/1";

/// One corpus document: a named core's fixed command script,
/// one command list per step, plus the step index at which the
/// cross-target save gate checkpoints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusDoc {
    /// The format name; must equal [`CORPUS_FORMAT`].
    pub format: String,
    /// The [`crate::GameCore::core_id`] this corpus drives.
    pub core_id: String,
    /// The corpus step index at which the save gate takes its
    /// checkpoint (after this many steps).
    pub save_at: usize,
    /// The command script: one command list per step.
    pub steps: Vec<Vec<Command>>,
}

impl CorpusDoc {
    /// The canonical text form of this document.
    #[must_use]
    pub fn to_text(&self) -> String {
        let value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        torogao_format::jsonfmt::to_d_json(&value)
    }

    /// Parses and validates a corpus document's format stamp.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the text does not parse or
    /// names a different format.
    pub fn from_text(text: &str) -> Result<Self, Refusal> {
        let doc: Self = serde_json::from_str(text).map_err(|error| {
            Refusal::new(
                RefusalClass::Malformed,
                format!("corpus document does not parse: {error}"),
            )
        })?;
        if doc.format != CORPUS_FORMAT {
            return Err(Refusal::new(
                RefusalClass::Malformed,
                format!(
                    "corpus format is '{}', this build reads '{CORPUS_FORMAT}'",
                    doc.format
                ),
            ));
        }
        Ok(doc)
    }
}

/// The interpreter corpus as a file-format document.
#[must_use]
pub fn interpreter_corpus_doc() -> CorpusDoc {
    CorpusDoc {
        format: CORPUS_FORMAT.to_string(),
        core_id: "auth-g:changr_fixture".to_string(),
        save_at: INTERPRETER_SAVE_AT,
        steps: interpreter_corpus(),
    }
}

/// The Loop corpus as a file-format document.
#[must_use]
pub fn loop_corpus_doc() -> CorpusDoc {
    CorpusDoc {
        format: CORPUS_FORMAT.to_string(),
        core_id: "loop-service/own-core".to_string(),
        save_at: LOOP_SAVE_AT,
        steps: loop_corpus(),
    }
}
