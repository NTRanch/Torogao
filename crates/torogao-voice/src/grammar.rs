//! The voice command grammar: a data document, not code.
//!
//! The grammar registry is generated from its sources of
//! truth (the tool registry, the authoring verb manifests,
//! the CLI modes) by [`crate::generate`] and checked in as JSON.
//! The parser loads the document and contains no per-tool
//! knowledge, so a tool added to the registry inherits voice
//! coverage by regeneration — or the coverage test fails.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Grammar document format version.
pub const GRAMMAR_VERSION: &str = "torogao-voice-grammar/1";

/// What a grammar entry drives when it executes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Target {
    /// One of the 57 registry tools, called through
    /// `torogao_cli::registry::call_tool`.
    RegistryTool {
        /// The registry tool name.
        name: String,
    },
    /// One of the 69 authoring verbs (`torogao author <verb>`).
    AuthorVerb {
        /// The canonical verb name (snake_case).
        name: String,
    },
    /// A CLI mode that is not a registry tool.
    CliMode {
        /// The mode name (`mcp`, `a2a`, `registry-dump`, `paths`).
        name: String,
    },
    /// An application-shell feature (Persona menus, Playroom,
    /// Settings, Nerd mode).
    AppCommand {
        /// The app command id (e.g. `open-playroom`).
        name: String,
    },
    /// The reserved stop class. Checked before every template;
    /// binds to the one synchronous total-stop path.
    SafetyStop,
}

impl Target {
    /// A stable sort key for deterministic listings.
    pub fn sort_key(&self) -> String {
        match self {
            Target::RegistryTool { name } => format!("tool:{name}"),
            Target::AuthorVerb { name } => format!("verb:{name}"),
            Target::CliMode { name } => format!("mode:{name}"),
            Target::AppCommand { name } => format!("app:{name}"),
            Target::SafetyStop => "safety:stop".to_string(),
        }
    }
}

/// The confirmation a parsed command needs before it executes.
///
/// The class derives from the target's posture in its source
/// registry; the voice layer never re-decides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfirmClass {
    /// Execute on parse (read-only targets).
    None,
    /// Render the parse on screen and require an explicit
    /// confirm act before executing (export / mutating).
    Confirm,
}

/// How a slot's spoken value is captured and typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotKind {
    /// A project reference: a bare name resolved against the
    /// session index, the session default ("current"), or a
    /// literal path token.
    Project,
    /// A filesystem path token, taken verbatim.
    Path,
    /// A single-token identifier.
    Name,
    /// A number (digits, or number words after normalization).
    Number,
    /// A boolean flag: present when its phrase is spoken.
    Flag,
    /// A list of names: captures the trailing span and splits
    /// it into items (final position only).
    List,
    /// The trailing span of the utterance (final position
    /// only), for free-form argument tails.
    Rest,
}

/// One argument slot of a grammar entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotSpec {
    /// The argument name (the tool's JSON property, or the
    /// verb's argument name).
    pub name: String,
    /// Capture and typing rule.
    pub kind: SlotKind,
    /// Whether a parse without this slot refuses
    /// `TG-VOICE-MISSING-ARG`.
    pub required: bool,
    /// Flag slots only: the spoken phrase that sets the flag.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub phrase: String,
}

/// One voice-addressable command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrammarEntry {
    /// Stable entry id, e.g. `tool.scene-validate`.
    pub id: String,
    /// What the entry drives.
    pub target: Target,
    /// Posture label copied from the source registry
    /// (`read-only` / `export` / `disabled` / `mutating` /
    /// `app` / `safety`).
    pub posture: String,
    /// Confirmation class (see [`ConfirmClass`]).
    pub confirm: ConfirmClass,
    /// Spoken form templates. `{name}` marks a slot; a segment
    /// in `[ ]` is optional; a trailing `{name}` whose slot
    /// kind is [`SlotKind::Rest`] captures the remaining span.
    pub templates: Vec<String>,
    /// Argument slots, in template order.
    pub slots: Vec<SlotSpec>,
}

/// The whole grammar registry document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrammarDoc {
    /// Format version ([`GRAMMAR_VERSION`]).
    pub version: String,
    /// Entries, sorted by id.
    pub entries: Vec<GrammarEntry>,
}

impl GrammarDoc {
    /// Serializes the document deterministically: entries
    /// sorted by id, fixed field order, two-space indent, one
    /// trailing newline. Byte-stability is a tested property —
    /// the checked-in file must equal a fresh generation.
    pub fn to_canonical_json(&self) -> String {
        let mut sorted = self.clone();
        sorted.entries.sort_by(|a, b| a.id.cmp(&b.id));
        let mut text = serde_json::to_string_pretty(&sorted).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Parses a grammar document.
    pub fn from_json(text: &str) -> Result<GrammarDoc, GrammarError> {
        let doc: GrammarDoc = serde_json::from_str(text)
            .map_err(|err| GrammarError::Malformed(format!("grammar JSON: {err}")))?;
        if doc.version != GRAMMAR_VERSION {
            return Err(GrammarError::Malformed(format!(
                "grammar version '{}' is not {GRAMMAR_VERSION}",
                doc.version
            )));
        }
        Ok(doc)
    }

    /// Finds one entry by id.
    pub fn entry(&self, id: &str) -> Option<&GrammarEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

/// Grammar loading failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarError {
    /// The document is not a well-formed grammar.
    Malformed(String),
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrammarError::Malformed(detail) => write!(f, "malformed grammar: {detail}"),
        }
    }
}

impl std::error::Error for GrammarError {}
