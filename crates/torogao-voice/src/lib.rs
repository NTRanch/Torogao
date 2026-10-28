//! Torogao voice control: a deterministic command grammar
//! over the tool registry, the authoring verbs, the CLI
//! modes, and the app features, with a local-only speech
//! recognition adapter.
//!
//! Voice is an input surface, never an authority: a parsed
//! voice command produces the same op every other surface
//! produces, passes the same gates (`decide()`, the output
//! guard, the unlock law), and adds no force path of its own.
//! Design: `~/workspace/torogao-voice-20261027/
//! VOICE-CONTROL-DESIGN.md`.

#![forbid(unsafe_code)]

pub mod audit;
pub mod dispatch;
pub mod generate;
pub mod grammar;
pub mod parse;
pub mod stt;

use grammar::{GrammarDoc, GrammarError};

/// The checked-in grammar document, embedded at build time.
/// The parser runs on this data — never on per-tool code —
/// and a test requires it to be byte-identical to a fresh
/// generation from the sources of truth.
pub const EMBEDDED_GRAMMAR_JSON: &str = include_str!("../grammar/torogao-voice-grammar.json");

/// Loads the embedded grammar document.
pub fn embedded_grammar() -> Result<GrammarDoc, GrammarError> {
    GrammarDoc::from_json(EMBEDDED_GRAMMAR_JSON)
}

/// Loads a grammar document from a file.
pub fn load_grammar_file(path: &std::path::Path) -> Result<GrammarDoc, GrammarError> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| GrammarError::Malformed(format!("cannot read {}: {err}", path.display())))?;
    GrammarDoc::from_json(&text)
}
