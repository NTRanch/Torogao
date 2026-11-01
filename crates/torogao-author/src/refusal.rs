//! The `TG-AUTH-*` refusal taxonomy.
//!
//! Every refusal the crate produces carries exactly one class.
//! The first seven classes are the brief's (§Item 3); the rest
//! are this slice's named additions for failures the brief's
//! seven do not cover (malformed input, non-canonical project
//! bytes, replay divergence, header stamps, incomplete lattices,
//! and filesystem failure).

use serde_json::{Value, json};

/// A refusal class: the stable `TG-AUTH-*` code family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefusalClass {
    /// A keyform lattice lacks a form at some key intersection
    /// (a validation finding, never an op refusal).
    Incomplete,
    /// Two writes for one id disagree, an op would create a cycle,
    /// or a removal targets something still referenced.
    Conflict,
    /// A reference (parent, clip source, binding target, asset)
    /// names nothing that exists.
    Dangling,
    /// A filesystem operation failed or the project is locked.
    Io,
    /// Input is structurally invalid: syntax, arity, shape, ids,
    /// non-finite numbers, degenerate geometry.
    Malformed,
    /// `project.ron` does not reproduce byte-for-byte after a
    /// load/emit round trip.
    NonCanonical,
    /// Intake without the provenance or separation records the
    /// brief requires.
    Provenance,
    /// A value lies outside its declared or bounded range.
    Range,
    /// Intake from a path classified as read-only evidence.
    ReadonlySource,
    /// Replay of the op log does not reproduce the recorded
    /// hashes, or the log itself is corrupt.
    ReplayDivergence,
    /// The project header's lane stamp or consent record is
    /// missing or malformed.
    Stamp,
    /// A kind (deformer kind, asset kind, record name, export
    /// target) is not one this slice knows.
    UnknownKind,
    /// An op names a target id that does not exist.
    UnknownTarget,
}

impl RefusalClass {
    /// The stable code string.
    pub fn code(self) -> &'static str {
        match self {
            RefusalClass::Conflict => "TG-AUTH-CONFLICT",
            RefusalClass::Dangling => "TG-AUTH-DANGLING",
            RefusalClass::Incomplete => "TG-AUTH-INCOMPLETE",
            RefusalClass::Io => "TG-AUTH-IO",
            RefusalClass::Malformed => "TG-AUTH-MALFORMED",
            RefusalClass::NonCanonical => "TG-AUTH-NONCANONICAL",
            RefusalClass::Provenance => "TG-AUTH-PROVENANCE",
            RefusalClass::Range => "TG-AUTH-RANGE",
            RefusalClass::ReadonlySource => "TG-AUTH-READONLY-SOURCE",
            RefusalClass::ReplayDivergence => "TG-AUTH-REPLAY-DIVERGENCE",
            RefusalClass::Stamp => "TG-AUTH-STAMP",
            RefusalClass::UnknownKind => "TG-AUTH-UNKNOWN-KIND",
            RefusalClass::UnknownTarget => "TG-AUTH-UNKNOWN-TARGET",
        }
    }
}

/// One typed refusal: a class plus a bounded human-readable
/// message. Messages name ids and fields, never file contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refusal {
    /// The refusal class.
    pub class: RefusalClass,
    /// What was refused and why.
    pub message: String,
}

/// Longest message kept, in bytes; longer messages are cut at a
/// character boundary so a hostile id cannot bloat output.
const MESSAGE_BYTES_MAX: usize = 1024;

impl Refusal {
    /// Builds a refusal, bounding the message length.
    pub fn new(class: RefusalClass, message: impl Into<String>) -> Refusal {
        let mut message = message.into();
        if message.len() > MESSAGE_BYTES_MAX {
            let mut cut = MESSAGE_BYTES_MAX;
            while !message.is_char_boundary(cut) {
                cut -= 1;
            }
            message.truncate(cut);
        }
        Refusal { class, message }
    }

    /// The stable code string.
    pub fn code(&self) -> &'static str {
        self.class.code()
    }

    /// The refusal as a JSON object `{code, message}`.
    pub fn to_json(&self) -> Value {
        json!({"code": self.code(), "message": self.message})
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.message)
    }
}

impl std::error::Error for Refusal {}

/// Shorthand for `Err(Refusal::new(class, message))`.
pub fn refuse<T>(class: RefusalClass, message: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal::new(class, message))
}
