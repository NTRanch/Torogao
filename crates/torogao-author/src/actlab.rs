//! Act-record validation and the Crescendo simulator (D-943).
//!
//! An act record is the §6d act-registry entry of
//! `PARAMETER-UNIVERSE.md` (ratified D-911, 2026-11-04) as data:
//! `{slug, set_parameter, parts[], state_bindings[], source}` plus
//! the target rigs the record names. New acts are data, not code:
//! this module validates a record against two more data documents
//! — a parameter-universe snapshot (the governed set/state
//! parameters) and one parameter snapshot per rig (names, ranges,
//! defaults, part-variant lists) — all three parsed from RON text
//! through the crate's bounded RON parser ([`crate::ron`]).
//! Checked-in exports of the snapshots live in this crate's
//! `data/actlab/` directory with their provenance in the records
//! themselves; nothing here depends in code on the rig workspace
//! that produced them.
//!
//! The Crescendo simulator steps `ParamCrescendoStage`
//! (peaks passed, 0..=4) as a monotone state machine: holds and
//! +1 increments are admitted, every other transition is refused
//! with a typed error naming the transition.
//!
//! The author verb is `act_validate` (see [`run_actlab_verb`]);
//! like the game verbs it is a separate family from the rig
//! [`crate::op::MANIFEST`] and is routed by name in
//! [`crate::session::run_verb`]. It is read-only and
//! project-independent: the faces read the three documents
//! (bounded by [`crate::limits::ACTLAB_TEXT_BYTES_MAX`]) and pass
//! their text in the verb inputs.

use crate::limits::{
    ACTLAB_BINDINGS_MAX, ACTLAB_HASHES_MAX, ACTLAB_PARTS_MAX, ACTLAB_RIGS_MAX,
    ACTLAB_SLUG_BYTES_MAX, ACTLAB_SNAPSHOT_PARAMETERS_MAX, ACTLAB_SNAPSHOT_PARTS_MAX,
    ACTLAB_SOURCES_MAX, ACTLAB_TEXT_BYTES_MAX, PROVENANCE_TEXT_BYTES_MAX,
};
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::ron::{Ron, parse_document};
use serde::Deserialize;
use serde_json::{Value, json};

/// The author verb this module serves.
pub const ACTLAB_VERB: &str = "act_validate";

/// The one parameter whose bindings carry peaks passed: the
/// Crescendo stage encoding of §6c (0..=4, peaks passed).
pub const CRESCENDO_PARAMETER: &str = "ParamCrescendoStage";

/// Highest Crescendo stage: four peaks passed (apex).
pub const CRESCENDO_STAGE_MAX: i64 = 4;

/// The `format` tag of a parameter-universe snapshot document.
pub const UNIVERSE_FORMAT: &str = "torogao-actlab-universe/1";

/// The `format` tag of a rig parameter snapshot document.
pub const RIG_FORMAT: &str = "torogao-actlab-rig/1";

// ---- Documents ---------------------------------------------------

/// Provenance of a snapshot document: where the data came from,
/// how it was generated, and the hashes of the records it was
/// generated from. Carried in the document, never only beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Provenance {
    /// Source documents the snapshot was generated from.
    pub source_documents: Vec<String>,
    /// How the snapshot was produced, in one bounded string.
    pub generation_method: String,
    /// Generation date (`YYYY-MM-DD`).
    pub generated_on: String,
    /// `(name, sha256)` of each source record consumed.
    pub record_hashes: Vec<RecordHash>,
}

/// One source record's name and SHA-256 (lowercase hex).
#[derive(Clone, Debug, PartialEq)]
pub struct RecordHash {
    /// The record's file name.
    pub name: String,
    /// Its SHA-256, 64 lowercase hex characters.
    pub sha256: String,
}

/// One governed parameter of the universe snapshot: a name, its
/// governed range, and whether its values are integers.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterSpec {
    /// Parameter name (for example `ParamRopeSet`).
    pub name: String,
    /// Governed range minimum (inclusive).
    pub min: f64,
    /// Governed range maximum (inclusive).
    pub max: f64,
    /// True when values must be integers (`ParamCrescendoStage`).
    pub integer: bool,
}

/// The parameter-universe snapshot: the governed sets an act
/// record may use. `set_parameters` are the §6a gear-set
/// parameters (with Alma's `ParamHucowSet` precedent);
/// `state_parameters` are the §6b/§6c state parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterUniverse {
    /// Where this snapshot came from.
    pub provenance: Provenance,
    /// Parameters an act's `set_parameter` may name.
    pub set_parameters: Vec<ParameterSpec>,
    /// Parameters a state binding may name.
    pub state_parameters: Vec<ParameterSpec>,
}

impl ParameterUniverse {
    /// The set-parameter spec for `name`, if governed.
    pub fn set_spec(&self, name: &str) -> Option<&ParameterSpec> {
        self.set_parameters.iter().find(|spec| spec.name == name)
    }

    /// The state-parameter spec for `name`, if governed.
    pub fn state_spec(&self, name: &str) -> Option<&ParameterSpec> {
        self.state_parameters.iter().find(|spec| spec.name == name)
    }
}

/// One parameter row of a rig snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct RigParameter {
    /// Parameter name.
    pub name: String,
    /// Range minimum (inclusive).
    pub min: f64,
    /// Range maximum (inclusive).
    pub max: f64,
    /// Default value, inside the range.
    pub default: f64,
}

/// One rig's parameter snapshot: every parameter with its range
/// and default, and every part-variant name, as data.
#[derive(Clone, Debug, PartialEq)]
pub struct RigSnapshot {
    /// Rig id an act record names (for example `prima`).
    pub rig_id: String,
    /// Display name (for example `Prima`).
    pub display_name: String,
    /// Where this snapshot came from.
    pub provenance: Provenance,
    /// The rig's parameters, in the source record's order.
    pub parameters: Vec<RigParameter>,
    /// The rig's part-variant names, sorted.
    pub parts: Vec<String>,
}

impl RigSnapshot {
    /// The parameter row for `name`, if the rig carries it.
    pub fn parameter(&self, name: &str) -> Option<&RigParameter> {
        self.parameters.iter().find(|row| row.name == name)
    }

    /// True when `part` is a part variant of this rig.
    pub fn has_part(&self, part: &str) -> bool {
        self.parts.iter().any(|name| name == part)
    }
}

/// An act record's governing assignment: one set parameter driven
/// to one target value.
#[derive(Clone, Debug, PartialEq)]
pub struct SetParameter {
    /// The set parameter's name.
    pub name: String,
    /// The target value, inside the governed range.
    pub value: f64,
}

/// A state binding's bound value: a continuous value for a
/// continuous state parameter, or peaks passed for
/// `ParamCrescendoStage`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StateValue {
    /// A continuous value (for example corruption 0..1).
    Continuous(f64),
    /// Peaks passed, an integer (range-checked by the
    /// simulator, not at parse time).
    PeaksPassed(i64),
}

/// One state binding: a state parameter plus its bound value.
/// Exactly one of the grammar's `value` / `peaks_passed` fields
/// is present; the parser refuses both and neither.
#[derive(Clone, Debug, PartialEq)]
pub struct StateBinding {
    /// The state parameter's name.
    pub parameter: String,
    /// The bound value.
    pub value: StateValue,
}

/// One §6d act-registry entry, as data.
#[derive(Clone, Debug, PartialEq)]
pub struct ActRecord {
    /// Kebab-case slug (see [`slug_problem`]).
    pub slug: String,
    /// Target rig ids, in the record's order.
    pub rigs: Vec<String>,
    /// The governing set parameter and its target value.
    pub set_parameter: SetParameter,
    /// Part-variant names the act uses, in the record's order.
    pub parts: Vec<String>,
    /// State bindings, in the record's order. Bindings for
    /// `ParamCrescendoStage` form, in order, the event sequence
    /// the simulator steps.
    pub state_bindings: Vec<StateBinding>,
    /// Provenance string: where this act's definition comes from.
    pub source: String,
}

// ---- Parse errors ------------------------------------------------

/// A typed failure to parse one of the three actlab documents.
/// Parsing never panics and never partially succeeds: a document
/// is whole or refused with exactly one of these.
#[derive(Clone, Debug, PartialEq)]
pub enum ActParseError {
    /// The text is not valid RON for the crate's parser, or
    /// exceeds the byte bound; carries the parser's message.
    Syntax {
        /// The underlying parser's bounded message.
        message: String,
    },
    /// The document's root is not the expected named struct.
    WrongDocument {
        /// The struct name the grammar requires.
        expected: String,
        /// The struct name (or value kind) found.
        actual: String,
    },
    /// The document's `format` field is not the expected tag.
    BadFormatTag {
        /// The document kind (`universe` / `rig`).
        document: &'static str,
        /// The expected format tag.
        expected: String,
        /// The tag found.
        actual: String,
    },
    /// A required field is absent.
    MissingField {
        /// The struct the field belongs to.
        document: String,
        /// The field's name.
        field: String,
    },
    /// A field the grammar does not define is present.
    UnknownField {
        /// The struct the field was found on.
        document: String,
        /// The field's name.
        field: String,
    },
    /// A field holds a value of the wrong kind.
    WrongType {
        /// The struct the field belongs to.
        document: String,
        /// The field's name.
        field: String,
        /// The kind the grammar requires.
        expected: &'static str,
        /// The kind found.
        actual: &'static str,
    },
    /// A text field is empty where the grammar requires content.
    EmptyText {
        /// The struct the field belongs to.
        document: String,
        /// The field's name.
        field: String,
    },
    /// A list exceeds its bound.
    TooMany {
        /// The struct the field belongs to.
        document: String,
        /// The field's name.
        field: String,
        /// The count found.
        count: usize,
        /// The bound.
        max: usize,
    },
    /// A state binding carries both `value` and `peaks_passed`,
    /// or neither.
    BindingShape {
        /// The binding's parameter name.
        parameter: String,
    },
    /// `peaks_passed` is not an integer literal.
    NonIntegerPeaksPassed {
        /// The value found.
        value: f64,
    },
    /// A provenance hash is not 64 lowercase hex characters.
    BadHash {
        /// The record's name.
        name: String,
        /// The hash text found.
        sha256: String,
    },
    /// A parameter row is internally inconsistent: `min > max`,
    /// a default outside its range, or a non-integral bound on
    /// an integer parameter.
    InconsistentRange {
        /// The parameter's name.
        parameter: String,
        /// What is inconsistent.
        reason: String,
    },
}

impl ActParseError {
    /// The stable code string for this failure.
    pub fn code(&self) -> &'static str {
        match self {
            ActParseError::Syntax { .. } => "ACTLAB-SYNTAX",
            ActParseError::WrongDocument { .. } => "ACTLAB-WRONG-DOCUMENT",
            ActParseError::BadFormatTag { .. } => "ACTLAB-BAD-FORMAT",
            ActParseError::MissingField { .. } => "ACTLAB-MISSING-FIELD",
            ActParseError::UnknownField { .. } => "ACTLAB-UNKNOWN-FIELD",
            ActParseError::WrongType { .. } => "ACTLAB-WRONG-TYPE",
            ActParseError::EmptyText { .. } => "ACTLAB-EMPTY-TEXT",
            ActParseError::TooMany { .. } => "ACTLAB-TOO-MANY",
            ActParseError::BindingShape { .. } => "ACTLAB-BINDING-SHAPE",
            ActParseError::NonIntegerPeaksPassed { .. } => "ACTLAB-NONINTEGER-PEAKS",
            ActParseError::BadHash { .. } => "ACTLAB-BAD-HASH",
            ActParseError::InconsistentRange { .. } => "ACTLAB-INCONSISTENT-RANGE",
        }
    }

    /// A one-line human-readable statement of the failure.
    pub fn message(&self) -> String {
        match self {
            ActParseError::Syntax { message } => message.clone(),
            ActParseError::WrongDocument { expected, actual } => {
                format!("expected a {expected} document, found {actual}")
            }
            ActParseError::BadFormatTag {
                document,
                expected,
                actual,
            } => format!("{document} format is '{actual}', expected '{expected}'"),
            ActParseError::MissingField { document, field } => {
                format!("{document} is missing required field '{field}'")
            }
            ActParseError::UnknownField { document, field } => {
                format!("{document} carries unknown field '{field}'")
            }
            ActParseError::WrongType {
                document,
                field,
                expected,
                actual,
            } => format!("{document} field '{field}' must be {expected}, found {actual}"),
            ActParseError::EmptyText { document, field } => {
                format!("{document} field '{field}' must not be empty")
            }
            ActParseError::TooMany {
                document,
                field,
                count,
                max,
            } => format!("{document} field '{field}' carries {count} entries, at most {max}"),
            ActParseError::BindingShape { parameter } => format!(
                "binding for '{parameter}' must carry exactly one of 'value' / 'peaks_passed'"
            ),
            ActParseError::NonIntegerPeaksPassed { value } => {
                format!("peaks_passed must be an integer in 0..=4, found {value}")
            }
            ActParseError::BadHash { name, sha256 } => {
                format!("record hash for '{name}' is not 64 lowercase hex: '{sha256}'")
            }
            ActParseError::InconsistentRange { parameter, reason } => {
                format!("parameter '{parameter}': {reason}")
            }
        }
    }

    /// The failure as one violation-shaped JSON object, so the
    /// verb's `violations` list carries parse failures and
    /// validation violations in one shape.
    pub fn to_json(&self) -> Value {
        json!({
            "actual": self.actual_text(),
            "check": "parse",
            "code": self.code(),
            "expected": self.expected_text(),
            "field": self.field_text(),
            "message": self.message(),
        })
    }

    fn field_text(&self) -> String {
        match self {
            ActParseError::MissingField { field, .. }
            | ActParseError::UnknownField { field, .. }
            | ActParseError::WrongType { field, .. }
            | ActParseError::EmptyText { field, .. }
            | ActParseError::TooMany { field, .. } => field.clone(),
            ActParseError::NonIntegerPeaksPassed { .. } => "peaks_passed".to_string(),
            ActParseError::BindingShape { .. } => "state_bindings".to_string(),
            ActParseError::BadFormatTag { .. } => "format".to_string(),
            ActParseError::BadHash { .. } => "record_hashes".to_string(),
            ActParseError::InconsistentRange { .. } => "parameters".to_string(),
            ActParseError::Syntax { .. } | ActParseError::WrongDocument { .. } => {
                "document".to_string()
            }
        }
    }

    fn expected_text(&self) -> String {
        match self {
            ActParseError::WrongDocument { expected, .. } => expected.clone(),
            ActParseError::BadFormatTag { expected, .. } => expected.clone(),
            ActParseError::WrongType { expected, .. } => (*expected).to_string(),
            ActParseError::NonIntegerPeaksPassed { .. } => "an integer".to_string(),
            ActParseError::BindingShape { .. } => "exactly one of value / peaks_passed".to_string(),
            ActParseError::BadHash { .. } => "64 lowercase hex characters".to_string(),
            ActParseError::TooMany { max, .. } => format!("at most {max} entries"),
            _ => "a well-formed document".to_string(),
        }
    }

    fn actual_text(&self) -> String {
        match self {
            ActParseError::WrongDocument { actual, .. } => actual.clone(),
            ActParseError::BadFormatTag { actual, .. } => actual.clone(),
            ActParseError::WrongType { actual, .. } => (*actual).to_string(),
            ActParseError::NonIntegerPeaksPassed { value } => value.to_string(),
            ActParseError::BadHash { sha256, .. } => sha256.clone(),
            ActParseError::TooMany { count, .. } => format!("{count} entries"),
            ActParseError::Syntax { message } => message.clone(),
            _ => self.message(),
        }
    }
}

impl std::fmt::Display for ActParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for ActParseError {}

// ---- RON decoding ------------------------------------------------

fn kind_of(value: &Ron) -> &'static str {
    match value {
        Ron::Bool(_) => "a bool",
        Ron::Float(_) => "a float",
        Ron::Ident(_) => "an identifier",
        Ron::Int(_) => "an integer",
        Ron::List(_) => "a list",
        Ron::Opt(_) => "an option",
        Ron::Str(_) => "a string",
        Ron::Struct(..) => "a struct",
        Ron::Tuple(_) => "a tuple",
    }
}

fn parse_root(text: &str) -> Result<Ron, ActParseError> {
    if text.len() > ACTLAB_TEXT_BYTES_MAX {
        return Err(ActParseError::Syntax {
            message: format!(
                "document is {} bytes, over the {ACTLAB_TEXT_BYTES_MAX}-byte bound",
                text.len()
            ),
        });
    }
    parse_document(text).map_err(|refusal| ActParseError::Syntax {
        message: refusal.message,
    })
}

fn expect_struct(value: &Ron, name: &str) -> Result<Vec<(String, Ron)>, ActParseError> {
    match value {
        Ron::Struct(found, fields) if found == name => Ok(fields.clone()),
        Ron::Struct(found, _) => Err(ActParseError::WrongDocument {
            expected: name.to_string(),
            actual: found.clone(),
        }),
        other => Err(ActParseError::WrongDocument {
            expected: name.to_string(),
            actual: kind_of(other).to_string(),
        }),
    }
}

/// A struct's fields under decode: required fields are taken,
/// leftovers are refused as unknown, so a document carries
/// exactly the grammar's fields (the RON parser already refuses
/// duplicate fields).
struct Fields {
    document: String,
    fields: Vec<(String, Ron)>,
}

impl Fields {
    fn new(document: &str, fields: Vec<(String, Ron)>) -> Fields {
        Fields {
            document: document.to_string(),
            fields,
        }
    }

    fn take(&mut self, field: &str) -> Result<Ron, ActParseError> {
        match self.fields.iter().position(|(name, _)| name == field) {
            Some(index) => Ok(self.fields.remove(index).1),
            None => Err(ActParseError::MissingField {
                document: self.document.clone(),
                field: field.to_string(),
            }),
        }
    }

    fn take_opt(&mut self, field: &str) -> Option<Ron> {
        self.fields
            .iter()
            .position(|(name, _)| name == field)
            .map(|index| self.fields.remove(index).1)
    }

    fn finish(self) -> Result<(), ActParseError> {
        match self.fields.into_iter().next() {
            Some((field, _)) => Err(ActParseError::UnknownField {
                document: self.document,
                field,
            }),
            None => Ok(()),
        }
    }
}

fn as_string(value: &Ron, document: &str, field: &str) -> Result<String, ActParseError> {
    match value {
        Ron::Str(text) => Ok(text.clone()),
        other => Err(ActParseError::WrongType {
            document: document.to_string(),
            field: field.to_string(),
            expected: "a string",
            actual: kind_of(other),
        }),
    }
}

fn as_nonempty_string(value: &Ron, document: &str, field: &str) -> Result<String, ActParseError> {
    let text = as_string(value, document, field)?;
    if text.is_empty() {
        return Err(ActParseError::EmptyText {
            document: document.to_string(),
            field: field.to_string(),
        });
    }
    if text.len() > PROVENANCE_TEXT_BYTES_MAX {
        return Err(ActParseError::TooMany {
            document: document.to_string(),
            field: field.to_string(),
            count: text.len(),
            max: PROVENANCE_TEXT_BYTES_MAX,
        });
    }
    Ok(text)
}

fn as_number(value: &Ron, document: &str, field: &str) -> Result<f64, ActParseError> {
    match value {
        Ron::Int(number) => Ok(*number as f64),
        Ron::Float(number) => Ok(*number),
        other => Err(ActParseError::WrongType {
            document: document.to_string(),
            field: field.to_string(),
            expected: "a number",
            actual: kind_of(other),
        }),
    }
}

fn as_bool(value: &Ron, document: &str, field: &str) -> Result<bool, ActParseError> {
    match value {
        Ron::Bool(flag) => Ok(*flag),
        other => Err(ActParseError::WrongType {
            document: document.to_string(),
            field: field.to_string(),
            expected: "a bool",
            actual: kind_of(other),
        }),
    }
}

fn as_list(
    value: &Ron,
    document: &str,
    field: &str,
    max: usize,
) -> Result<Vec<Ron>, ActParseError> {
    match value {
        Ron::List(items) => {
            if items.len() > max {
                return Err(ActParseError::TooMany {
                    document: document.to_string(),
                    field: field.to_string(),
                    count: items.len(),
                    max,
                });
            }
            Ok(items.clone())
        }
        other => Err(ActParseError::WrongType {
            document: document.to_string(),
            field: field.to_string(),
            expected: "a list",
            actual: kind_of(other),
        }),
    }
}

fn as_string_list(
    value: &Ron,
    document: &str,
    field: &str,
    max: usize,
) -> Result<Vec<String>, ActParseError> {
    let items = as_list(value, document, field, max)?;
    let mut out = Vec::with_capacity(items.len());
    for item in &items {
        out.push(as_string(item, document, field)?);
    }
    Ok(out)
}

fn decode_hash(value: &Ron) -> Result<RecordHash, ActParseError> {
    let mut fields = Fields::new("RecordHash", expect_struct(value, "RecordHash")?);
    let name = as_nonempty_string(&fields.take("name")?, "RecordHash", "name")?;
    let sha256 = as_string(&fields.take("sha256")?, "RecordHash", "sha256")?;
    fields.finish()?;
    let valid = sha256.len() == 64
        && sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !valid {
        return Err(ActParseError::BadHash { name, sha256 });
    }
    Ok(RecordHash { name, sha256 })
}

fn decode_provenance(value: &Ron) -> Result<Provenance, ActParseError> {
    let mut fields = Fields::new("Provenance", expect_struct(value, "Provenance")?);
    let documents = as_string_list(
        &fields.take("source_documents")?,
        "Provenance",
        "source_documents",
        ACTLAB_SOURCES_MAX,
    )?;
    if documents.iter().any(|doc| doc.is_empty()) {
        return Err(ActParseError::EmptyText {
            document: "Provenance".to_string(),
            field: "source_documents".to_string(),
        });
    }
    let method = as_nonempty_string(
        &fields.take("generation_method")?,
        "Provenance",
        "generation_method",
    )?;
    let generated_on =
        as_nonempty_string(&fields.take("generated_on")?, "Provenance", "generated_on")?;
    let hash_values = as_list(
        &fields.take("record_hashes")?,
        "Provenance",
        "record_hashes",
        ACTLAB_HASHES_MAX,
    )?;
    fields.finish()?;
    let mut record_hashes = Vec::with_capacity(hash_values.len());
    for hash in &hash_values {
        record_hashes.push(decode_hash(hash)?);
    }
    Ok(Provenance {
        source_documents: documents,
        generation_method: method,
        generated_on,
        record_hashes,
    })
}

fn check_spec(spec: &ParameterSpec) -> Result<(), ActParseError> {
    if spec.min > spec.max {
        return Err(ActParseError::InconsistentRange {
            parameter: spec.name.clone(),
            reason: format!("min {} is above max {}", spec.min, spec.max),
        });
    }
    if spec.integer && (spec.min.fract() != 0.0 || spec.max.fract() != 0.0) {
        return Err(ActParseError::InconsistentRange {
            parameter: spec.name.clone(),
            reason: "an integer parameter's bounds must be integers".to_string(),
        });
    }
    Ok(())
}

fn decode_spec(value: &Ron) -> Result<ParameterSpec, ActParseError> {
    let mut fields = Fields::new("ParameterSpec", expect_struct(value, "ParameterSpec")?);
    let spec = ParameterSpec {
        name: as_nonempty_string(&fields.take("name")?, "ParameterSpec", "name")?,
        min: as_number(&fields.take("min")?, "ParameterSpec", "min")?,
        max: as_number(&fields.take("max")?, "ParameterSpec", "max")?,
        integer: as_bool(&fields.take("integer")?, "ParameterSpec", "integer")?,
    };
    fields.finish()?;
    check_spec(&spec)?;
    Ok(spec)
}

fn decode_spec_list(value: &Ron, field: &str) -> Result<Vec<ParameterSpec>, ActParseError> {
    let items = as_list(
        value,
        "ParameterUniverse",
        field,
        ACTLAB_SNAPSHOT_PARAMETERS_MAX,
    )?;
    let mut out = Vec::with_capacity(items.len());
    for item in &items {
        out.push(decode_spec(item)?);
    }
    Ok(out)
}

/// Parses a parameter-universe snapshot from RON text.
///
/// Grammar: `ParameterUniverse(format, provenance,
/// set_parameters, state_parameters)` — see the module docs and
/// `data/actlab/universe.ron` for the checked-in export.
pub fn parse_parameter_universe(text: &str) -> Result<ParameterUniverse, ActParseError> {
    let root = parse_root(text)?;
    let mut fields = Fields::new(
        "ParameterUniverse",
        expect_struct(&root, "ParameterUniverse")?,
    );
    let format = as_string(&fields.take("format")?, "ParameterUniverse", "format")?;
    if format != UNIVERSE_FORMAT {
        return Err(ActParseError::BadFormatTag {
            document: "universe",
            expected: UNIVERSE_FORMAT.to_string(),
            actual: format,
        });
    }
    let provenance = decode_provenance(&fields.take("provenance")?)?;
    let set_parameters = decode_spec_list(&fields.take("set_parameters")?, "set_parameters")?;
    let state_parameters = decode_spec_list(&fields.take("state_parameters")?, "state_parameters")?;
    fields.finish()?;
    Ok(ParameterUniverse {
        provenance,
        set_parameters,
        state_parameters,
    })
}

fn decode_rig_parameter(value: &Ron) -> Result<RigParameter, ActParseError> {
    let mut fields = Fields::new("RigParameter", expect_struct(value, "RigParameter")?);
    let row = RigParameter {
        name: as_nonempty_string(&fields.take("name")?, "RigParameter", "name")?,
        min: as_number(&fields.take("min")?, "RigParameter", "min")?,
        max: as_number(&fields.take("max")?, "RigParameter", "max")?,
        default: as_number(&fields.take("default")?, "RigParameter", "default")?,
    };
    fields.finish()?;
    if row.min > row.max {
        return Err(ActParseError::InconsistentRange {
            parameter: row.name.clone(),
            reason: format!("min {} is above max {}", row.min, row.max),
        });
    }
    if row.default < row.min || row.default > row.max {
        return Err(ActParseError::InconsistentRange {
            parameter: row.name.clone(),
            reason: format!(
                "default {} is outside {}..={}",
                row.default, row.min, row.max
            ),
        });
    }
    Ok(row)
}

/// Parses one rig's parameter snapshot from RON text.
///
/// Grammar: `RigSnapshot(format, rig_id, display_name,
/// provenance, parameters, parts)` — see `data/actlab/rig-*.ron`
/// for the checked-in exports.
pub fn parse_rig_snapshot(text: &str) -> Result<RigSnapshot, ActParseError> {
    let root = parse_root(text)?;
    let mut fields = Fields::new("RigSnapshot", expect_struct(&root, "RigSnapshot")?);
    let format = as_string(&fields.take("format")?, "RigSnapshot", "format")?;
    if format != RIG_FORMAT {
        return Err(ActParseError::BadFormatTag {
            document: "rig",
            expected: RIG_FORMAT.to_string(),
            actual: format,
        });
    }
    let rig_id = as_nonempty_string(&fields.take("rig_id")?, "RigSnapshot", "rig_id")?;
    let display_name =
        as_nonempty_string(&fields.take("display_name")?, "RigSnapshot", "display_name")?;
    let provenance = decode_provenance(&fields.take("provenance")?)?;
    let param_values = as_list(
        &fields.take("parameters")?,
        "RigSnapshot",
        "parameters",
        ACTLAB_SNAPSHOT_PARAMETERS_MAX,
    )?;
    let parts = as_string_list(
        &fields.take("parts")?,
        "RigSnapshot",
        "parts",
        ACTLAB_SNAPSHOT_PARTS_MAX,
    )?;
    fields.finish()?;
    let mut parameters = Vec::with_capacity(param_values.len());
    for value in &param_values {
        parameters.push(decode_rig_parameter(value)?);
    }
    Ok(RigSnapshot {
        rig_id,
        display_name,
        provenance,
        parameters,
        parts,
    })
}

fn decode_set_parameter(value: &Ron) -> Result<SetParameter, ActParseError> {
    let mut fields = Fields::new("SetParameter", expect_struct(value, "SetParameter")?);
    let parsed = SetParameter {
        name: as_nonempty_string(&fields.take("name")?, "SetParameter", "name")?,
        value: as_number(&fields.take("value")?, "SetParameter", "value")?,
    };
    fields.finish()?;
    Ok(parsed)
}

fn decode_binding(value: &Ron) -> Result<StateBinding, ActParseError> {
    let mut fields = Fields::new("StateBinding", expect_struct(value, "StateBinding")?);
    let parameter = as_nonempty_string(&fields.take("parameter")?, "StateBinding", "parameter")?;
    let continuous = fields.take_opt("value");
    let peaks = fields.take_opt("peaks_passed");
    fields.finish()?;
    let bound = match (continuous, peaks) {
        (Some(raw), None) => StateValue::Continuous(as_number(&raw, "StateBinding", "value")?),
        (None, Some(Ron::Int(number))) => StateValue::PeaksPassed(number),
        (None, Some(Ron::Float(number))) => {
            return Err(ActParseError::NonIntegerPeaksPassed { value: number });
        }
        (None, Some(other)) => {
            return Err(ActParseError::WrongType {
                document: "StateBinding".to_string(),
                field: "peaks_passed".to_string(),
                expected: "an integer",
                actual: kind_of(&other),
            });
        }
        _ => return Err(ActParseError::BindingShape { parameter }),
    };
    Ok(StateBinding {
        parameter,
        value: bound,
    })
}

/// Parses one act record from RON text.
///
/// Grammar (the §6d entry, serialized):
/// `ActRecord(slug, rigs, set_parameter, parts, state_bindings,
/// source)` where `set_parameter` is `SetParameter(name, value)`
/// and each binding is `StateBinding(parameter, value)` or
/// `StateBinding(parameter, peaks_passed)` — exactly one of the
/// two value fields. Parsing checks shape only; whether the
/// record is *valid* is [`validate_act_record`]'s question.
pub fn parse_act_record(text: &str) -> Result<ActRecord, ActParseError> {
    let root = parse_root(text)?;
    let mut fields = Fields::new("ActRecord", expect_struct(&root, "ActRecord")?);
    let slug = as_string(&fields.take("slug")?, "ActRecord", "slug")?;
    let rigs = as_string_list(&fields.take("rigs")?, "ActRecord", "rigs", ACTLAB_RIGS_MAX)?;
    if rigs.is_empty() {
        return Err(ActParseError::EmptyText {
            document: "ActRecord".to_string(),
            field: "rigs".to_string(),
        });
    }
    let set_parameter = decode_set_parameter(&fields.take("set_parameter")?)?;
    let parts = as_string_list(
        &fields.take("parts")?,
        "ActRecord",
        "parts",
        ACTLAB_PARTS_MAX,
    )?;
    let binding_values = as_list(
        &fields.take("state_bindings")?,
        "ActRecord",
        "state_bindings",
        ACTLAB_BINDINGS_MAX,
    )?;
    let source = as_nonempty_string(&fields.take("source")?, "ActRecord", "source")?;
    fields.finish()?;
    let mut state_bindings = Vec::with_capacity(binding_values.len());
    for value in &binding_values {
        state_bindings.push(decode_binding(value)?);
    }
    Ok(ActRecord {
        slug,
        rigs,
        set_parameter,
        parts,
        state_bindings,
        source,
    })
}

// ---- Slug grammar --------------------------------------------------

/// Why `slug` is not a well-formed act slug, or `None` when it is.
///
/// The slug grammar (a deliberate subset of the estate id grammar
/// of [`crate::decide::check_id`], tightened for registry and
/// URL use): 1..=64 bytes of lowercase kebab — segments of
/// `[a-z0-9]+` joined by single hyphens; no leading, trailing, or
/// consecutive hyphens; no other characters.
pub fn slug_problem(slug: &str) -> Option<String> {
    if slug.is_empty() {
        return Some("slug is empty".to_string());
    }
    if slug.len() > ACTLAB_SLUG_BYTES_MAX {
        return Some(format!(
            "slug is {} bytes, over the {ACTLAB_SLUG_BYTES_MAX}-byte bound",
            slug.len()
        ));
    }
    if slug.starts_with('-') {
        return Some("slug starts with a hyphen".to_string());
    }
    if slug.ends_with('-') {
        return Some("slug ends with a hyphen".to_string());
    }
    if slug.contains("--") {
        return Some("slug contains consecutive hyphens".to_string());
    }
    for ch in slug.chars() {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
            return Some(format!("slug contains '{ch}', outside [a-z0-9-]"));
        }
    }
    None
}

// ---- Violations ----------------------------------------------------

/// One validation violation: a typed, named failure of one check
/// against one field, with the expected and actual values. A
/// record is valid iff [`validate_act_record`] returns none.
#[derive(Clone, Debug, PartialEq)]
pub enum ActViolation {
    /// The slug is not well-formed kebab (see [`slug_problem`]).
    MalformedSlug {
        /// The slug as written.
        slug: String,
        /// Why it is malformed.
        reason: String,
    },
    /// The record targets a rig no supplied snapshot describes.
    UnknownRig {
        /// The rig id as written.
        rig_id: String,
    },
    /// The record names the same rig twice.
    DuplicateRig {
        /// The repeated rig id.
        rig_id: String,
    },
    /// The record lists the same part variant twice.
    DuplicatePart {
        /// The repeated part name.
        part: String,
    },
    /// A continuous state parameter is bound twice (Crescendo
    /// bindings may repeat: they are the event sequence).
    DuplicateBinding {
        /// The repeated parameter name.
        parameter: String,
    },
    /// `set_parameter` names no governed set parameter.
    UnknownSetParameter {
        /// The parameter name as written.
        parameter: String,
    },
    /// The set parameter is governed but a target rig lacks it.
    SetParameterNotOnRig {
        /// The parameter name.
        parameter: String,
        /// The rig that lacks it.
        rig_id: String,
    },
    /// The set target value lies outside the governed range.
    /// `rig_id` is the rig whose range refused it, or
    /// `universe` when the universe range refused it and no
    /// target rig could report it.
    SetValueOutOfRange {
        /// The parameter name.
        parameter: String,
        /// The refusing range's owner.
        rig_id: String,
        /// The target value.
        value: f64,
        /// Range minimum (inclusive).
        min: f64,
        /// Range maximum (inclusive).
        max: f64,
    },
    /// A `parts[]` entry is not a part variant of a target rig.
    UnknownPartVariant {
        /// The part name as written.
        part: String,
        /// The rig that lacks the part.
        rig_id: String,
    },
    /// A state binding names no governed state parameter.
    UnknownStateParameter {
        /// The parameter name as written.
        parameter: String,
    },
    /// A governed state parameter is absent from a target rig.
    StateParameterNotOnRig {
        /// The parameter name.
        parameter: String,
        /// The rig that lacks it.
        rig_id: String,
    },
    /// A continuous state value lies outside its governed range.
    StateValueOutOfRange {
        /// The parameter name.
        parameter: String,
        /// The bound value.
        value: f64,
        /// Range minimum (inclusive).
        min: f64,
        /// Range maximum (inclusive).
        max: f64,
    },
    /// A binding's value kind disagrees with its parameter's
    /// kind: continuous value for an integer parameter, or
    /// `peaks_passed` for a continuous parameter.
    StateBindingShape {
        /// The parameter name.
        parameter: String,
        /// What disagrees.
        reason: String,
    },
    /// A Crescendo event is outside 0..=4.
    CrescendoOutOfRange {
        /// Index of the event in the sequence.
        index: usize,
        /// The event's value.
        value: i64,
    },
    /// A Crescendo event decreases the stage.
    CrescendoDecrease {
        /// Index of the event in the sequence.
        index: usize,
        /// Stage before the event.
        from: i64,
        /// The event's value.
        to: i64,
    },
    /// A Crescendo event jumps more than +1.
    CrescendoSkip {
        /// Index of the event in the sequence.
        index: usize,
        /// Stage before the event.
        from: i64,
        /// The event's value.
        to: i64,
    },
}

impl ActViolation {
    /// The stable code string for this violation.
    pub fn code(&self) -> &'static str {
        match self {
            ActViolation::MalformedSlug { .. } => "ACTLAB-SLUG",
            ActViolation::UnknownRig { .. } => "ACTLAB-UNKNOWN-RIG",
            ActViolation::DuplicateRig { .. } => "ACTLAB-DUPLICATE-RIG",
            ActViolation::DuplicatePart { .. } => "ACTLAB-DUPLICATE-PART",
            ActViolation::DuplicateBinding { .. } => "ACTLAB-DUPLICATE-BINDING",
            ActViolation::UnknownSetParameter { .. } => "ACTLAB-UNKNOWN-SET-PARAMETER",
            ActViolation::SetParameterNotOnRig { .. } => "ACTLAB-SET-PARAMETER-NOT-ON-RIG",
            ActViolation::SetValueOutOfRange { .. } => "ACTLAB-SET-VALUE-OUT-OF-RANGE",
            ActViolation::UnknownPartVariant { .. } => "ACTLAB-UNKNOWN-PART",
            ActViolation::UnknownStateParameter { .. } => "ACTLAB-UNKNOWN-STATE-PARAMETER",
            ActViolation::StateParameterNotOnRig { .. } => "ACTLAB-STATE-PARAMETER-NOT-ON-RIG",
            ActViolation::StateValueOutOfRange { .. } => "ACTLAB-STATE-VALUE-OUT-OF-RANGE",
            ActViolation::StateBindingShape { .. } => "ACTLAB-STATE-BINDING-SHAPE",
            ActViolation::CrescendoOutOfRange { .. } => "ACTLAB-CRESCENDO-OUT-OF-RANGE",
            ActViolation::CrescendoDecrease { .. } => "ACTLAB-CRESCENDO-DECREASE",
            ActViolation::CrescendoSkip { .. } => "ACTLAB-CRESCENDO-SKIP",
        }
    }

    /// The check that produced this violation.
    pub fn check(&self) -> &'static str {
        match self {
            ActViolation::MalformedSlug { .. } => "slug",
            ActViolation::UnknownRig { .. } | ActViolation::DuplicateRig { .. } => "rigs",
            ActViolation::UnknownSetParameter { .. }
            | ActViolation::SetParameterNotOnRig { .. }
            | ActViolation::SetValueOutOfRange { .. } => "set_parameter",
            ActViolation::DuplicatePart { .. } | ActViolation::UnknownPartVariant { .. } => "parts",
            ActViolation::DuplicateBinding { .. }
            | ActViolation::UnknownStateParameter { .. }
            | ActViolation::StateParameterNotOnRig { .. }
            | ActViolation::StateValueOutOfRange { .. }
            | ActViolation::StateBindingShape { .. } => "state_bindings",
            ActViolation::CrescendoOutOfRange { .. }
            | ActViolation::CrescendoDecrease { .. }
            | ActViolation::CrescendoSkip { .. } => "crescendo",
        }
    }

    /// The record field this violation is about.
    pub fn field(&self) -> &'static str {
        match self {
            ActViolation::MalformedSlug { .. } => "slug",
            ActViolation::UnknownRig { .. } | ActViolation::DuplicateRig { .. } => "rigs",
            ActViolation::UnknownSetParameter { .. }
            | ActViolation::SetParameterNotOnRig { .. }
            | ActViolation::SetValueOutOfRange { .. } => "set_parameter",
            ActViolation::DuplicatePart { .. } | ActViolation::UnknownPartVariant { .. } => "parts",
            _ => "state_bindings",
        }
    }

    /// A one-line human-readable statement of the violation.
    pub fn message(&self) -> String {
        match self {
            ActViolation::MalformedSlug { slug, reason } => {
                format!("slug '{slug}': {reason}")
            }
            ActViolation::UnknownRig { rig_id } => {
                format!("no supplied rig snapshot has rig_id '{rig_id}'")
            }
            ActViolation::DuplicateRig { rig_id } => {
                format!("rig '{rig_id}' is targeted twice")
            }
            ActViolation::DuplicatePart { part } => {
                format!("part '{part}' is listed twice")
            }
            ActViolation::DuplicateBinding { parameter } => {
                format!("state parameter '{parameter}' is bound twice")
            }
            ActViolation::UnknownSetParameter { parameter } => {
                format!("'{parameter}' is not a governed set parameter")
            }
            ActViolation::SetParameterNotOnRig { parameter, rig_id } => {
                format!("rig '{rig_id}' has no parameter '{parameter}'")
            }
            ActViolation::SetValueOutOfRange {
                parameter,
                rig_id,
                value,
                min,
                max,
            } => format!("set value {value} for '{parameter}' is outside {min}..={max} ({rig_id})"),
            ActViolation::UnknownPartVariant { part, rig_id } => {
                format!("rig '{rig_id}' has no part variant '{part}'")
            }
            ActViolation::UnknownStateParameter { parameter } => {
                format!("'{parameter}' is not a governed state parameter")
            }
            ActViolation::StateParameterNotOnRig { parameter, rig_id } => {
                format!("rig '{rig_id}' has no state parameter '{parameter}'")
            }
            ActViolation::StateValueOutOfRange {
                parameter,
                value,
                min,
                max,
            } => format!("bound value {value} for '{parameter}' is outside {min}..={max}"),
            ActViolation::StateBindingShape { parameter, reason } => {
                format!("binding for '{parameter}': {reason}")
            }
            ActViolation::CrescendoOutOfRange { index, value } => {
                format!("crescendo event {index} is {value}, outside 0..=4")
            }
            ActViolation::CrescendoDecrease { index, from, to } => {
                format!("crescendo event {index} decreases the stage {from} -> {to}")
            }
            ActViolation::CrescendoSkip { index, from, to } => {
                format!("crescendo event {index} jumps the stage {from} -> {to}")
            }
        }
    }

    fn expected_text(&self) -> String {
        match self {
            ActViolation::MalformedSlug { .. } => {
                "lowercase kebab [a-z0-9] segments joined by single hyphens".to_string()
            }
            ActViolation::UnknownRig { .. } => "a supplied rig snapshot's rig_id".to_string(),
            ActViolation::DuplicateRig { .. } | ActViolation::DuplicatePart { .. } => {
                "each entry at most once".to_string()
            }
            ActViolation::DuplicateBinding { .. } => {
                "a continuous state parameter bound at most once".to_string()
            }
            ActViolation::UnknownSetParameter { .. } => "a governed set parameter".to_string(),
            ActViolation::SetParameterNotOnRig { parameter, .. } => {
                format!("rig carries '{parameter}'")
            }
            ActViolation::SetValueOutOfRange { min, max, .. }
            | ActViolation::StateValueOutOfRange { min, max, .. } => {
                format!("a value in {min}..={max}")
            }
            ActViolation::UnknownPartVariant { .. } => "a part variant of the rig".to_string(),
            ActViolation::UnknownStateParameter { .. } => "a governed state parameter".to_string(),
            ActViolation::StateParameterNotOnRig { parameter, .. } => {
                format!("rig carries '{parameter}'")
            }
            ActViolation::StateBindingShape { .. } => {
                "the value kind matching the parameter kind".to_string()
            }
            ActViolation::CrescendoOutOfRange { .. } => "an integer in 0..=4".to_string(),
            ActViolation::CrescendoDecrease { from, .. } => {
                format!("a stage >= {from} (holds and +1 increments only)")
            }
            ActViolation::CrescendoSkip { from, .. } => {
                format!("a stage of {from} (hold) or {} (+1 increment)", from + 1)
            }
        }
    }

    fn actual_text(&self) -> String {
        match self {
            ActViolation::MalformedSlug { slug, .. } => slug.clone(),
            ActViolation::UnknownRig { rig_id } | ActViolation::DuplicateRig { rig_id } => {
                rig_id.clone()
            }
            ActViolation::DuplicatePart { part }
            | ActViolation::UnknownPartVariant { part, .. } => part.clone(),
            ActViolation::DuplicateBinding { parameter }
            | ActViolation::UnknownSetParameter { parameter }
            | ActViolation::SetParameterNotOnRig { parameter, .. }
            | ActViolation::UnknownStateParameter { parameter }
            | ActViolation::StateParameterNotOnRig { parameter, .. }
            | ActViolation::StateBindingShape { parameter, .. } => parameter.clone(),
            ActViolation::SetValueOutOfRange { value, .. }
            | ActViolation::StateValueOutOfRange { value, .. } => value.to_string(),
            ActViolation::CrescendoOutOfRange { value, .. } => value.to_string(),
            ActViolation::CrescendoDecrease { to, .. } | ActViolation::CrescendoSkip { to, .. } => {
                to.to_string()
            }
        }
    }

    /// The violation as a JSON object `{actual, check, code,
    /// expected, field, message}` — the verb's output shape.
    pub fn to_json(&self) -> Value {
        json!({
            "actual": self.actual_text(),
            "check": self.check(),
            "code": self.code(),
            "expected": self.expected_text(),
            "field": self.field(),
            "message": self.message(),
        })
    }
}

impl std::fmt::Display for ActViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.message())
    }
}

// ---- The validator -------------------------------------------------

/// Resolves the record's target rigs against the supplied
/// snapshots, in record order; unknown and duplicate targets
/// are violations, and unresolvable rigs drop out of the
/// per-rig checks that follow.
fn resolve_rigs<'a>(
    record: &ActRecord,
    rigs: &'a [RigSnapshot],
    violations: &mut Vec<ActViolation>,
) -> Vec<&'a RigSnapshot> {
    let mut resolved: Vec<&RigSnapshot> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for rig_id in &record.rigs {
        if seen.contains(&rig_id.as_str()) {
            violations.push(ActViolation::DuplicateRig {
                rig_id: rig_id.clone(),
            });
            continue;
        }
        seen.push(rig_id.as_str());
        match rigs.iter().find(|rig| &rig.rig_id == rig_id) {
            Some(rig) => resolved.push(rig),
            None => violations.push(ActViolation::UnknownRig {
                rig_id: rig_id.clone(),
            }),
        }
    }
    resolved
}

fn check_set_parameter(
    record: &ActRecord,
    universe: &ParameterUniverse,
    resolved: &[&RigSnapshot],
    violations: &mut Vec<ActViolation>,
) {
    let set = &record.set_parameter;
    let Some(spec) = universe.set_spec(&set.name) else {
        violations.push(ActViolation::UnknownSetParameter {
            parameter: set.name.clone(),
        });
        return;
    };
    let mut range_reported = false;
    for rig in resolved {
        let Some(row) = rig.parameter(&set.name) else {
            violations.push(ActViolation::SetParameterNotOnRig {
                parameter: set.name.clone(),
                rig_id: rig.rig_id.clone(),
            });
            continue;
        };
        if set.value < row.min || set.value > row.max {
            range_reported = true;
            violations.push(ActViolation::SetValueOutOfRange {
                parameter: set.name.clone(),
                rig_id: rig.rig_id.clone(),
                value: set.value,
                min: row.min,
                max: row.max,
            });
        }
    }
    if !range_reported && (set.value < spec.min || set.value > spec.max) {
        violations.push(ActViolation::SetValueOutOfRange {
            parameter: set.name.clone(),
            rig_id: "universe".to_string(),
            value: set.value,
            min: spec.min,
            max: spec.max,
        });
    }
}

fn check_parts(record: &ActRecord, resolved: &[&RigSnapshot], violations: &mut Vec<ActViolation>) {
    for (index, part) in record.parts.iter().enumerate() {
        if record.parts[..index].contains(part) {
            violations.push(ActViolation::DuplicatePart { part: part.clone() });
            continue;
        }
        for rig in resolved {
            if !rig.has_part(part) {
                violations.push(ActViolation::UnknownPartVariant {
                    part: part.clone(),
                    rig_id: rig.rig_id.clone(),
                });
            }
        }
    }
}

fn check_binding(
    binding: &StateBinding,
    universe: &ParameterUniverse,
    resolved: &[&RigSnapshot],
    seen_continuous: &mut Vec<String>,
    violations: &mut Vec<ActViolation>,
) {
    let Some(spec) = universe.state_spec(&binding.parameter) else {
        violations.push(ActViolation::UnknownStateParameter {
            parameter: binding.parameter.clone(),
        });
        return;
    };
    let shape_ok = match (spec.integer, binding.value) {
        (true, StateValue::PeaksPassed(_)) | (false, StateValue::Continuous(_)) => true,
        (true, StateValue::Continuous(_)) => {
            violations.push(ActViolation::StateBindingShape {
                parameter: binding.parameter.clone(),
                reason: "the parameter is integer-typed; bind peaks_passed".to_string(),
            });
            false
        }
        (false, StateValue::PeaksPassed(_)) => {
            violations.push(ActViolation::StateBindingShape {
                parameter: binding.parameter.clone(),
                reason: "the parameter is continuous; bind value".to_string(),
            });
            false
        }
    };
    if !shape_ok {
        return;
    }
    if !spec.integer {
        if seen_continuous.contains(&binding.parameter) {
            violations.push(ActViolation::DuplicateBinding {
                parameter: binding.parameter.clone(),
            });
            return;
        }
        seen_continuous.push(binding.parameter.clone());
    }
    for rig in resolved {
        if rig.parameter(&binding.parameter).is_none() {
            violations.push(ActViolation::StateParameterNotOnRig {
                parameter: binding.parameter.clone(),
                rig_id: rig.rig_id.clone(),
            });
        }
    }
    if let StateValue::Continuous(value) = binding.value {
        if value < spec.min || value > spec.max {
            violations.push(ActViolation::StateValueOutOfRange {
                parameter: binding.parameter.clone(),
                value,
                min: spec.min,
                max: spec.max,
            });
        }
    }
}

/// Validates one act record against the universe snapshot and
/// the supplied rig snapshots.
///
/// Checks, in emission order (the order is fixed, so the output
/// is deterministic for a given input): slug well-formedness;
/// target-rig resolution; the set parameter (governed, present
/// on every target rig, target value in range); every part (a
/// real variant of every target rig, listed once); every state
/// binding (governed, shape-matching, present on every target
/// rig, continuous values in range); and the Crescendo event
/// sequence, stepped by [`simulate_crescendo`]. An empty result
/// means the record is valid.
pub fn validate_act_record(
    record: &ActRecord,
    universe: &ParameterUniverse,
    rigs: &[RigSnapshot],
) -> Vec<ActViolation> {
    let mut violations = Vec::new();
    if let Some(reason) = slug_problem(&record.slug) {
        violations.push(ActViolation::MalformedSlug {
            slug: record.slug.clone(),
            reason,
        });
    }
    let resolved = resolve_rigs(record, rigs, &mut violations);
    check_set_parameter(record, universe, &resolved, &mut violations);
    check_parts(record, &resolved, &mut violations);
    let mut seen_continuous: Vec<String> = Vec::new();
    for binding in &record.state_bindings {
        check_binding(
            binding,
            universe,
            &resolved,
            &mut seen_continuous,
            &mut violations,
        );
    }
    let events = crescendo_events(record);
    if let (_, Some(failure)) = trace_crescendo(&events) {
        violations.push(failure_violation(&failure));
    }
    violations
}

fn failure_violation(failure: &CrescendoFailure) -> ActViolation {
    match failure.refusal {
        CrescendoRefusal::OutOfRange { value } => ActViolation::CrescendoOutOfRange {
            index: failure.index,
            value,
        },
        CrescendoRefusal::Decrease { from, to } => ActViolation::CrescendoDecrease {
            index: failure.index,
            from,
            to,
        },
        CrescendoRefusal::Skip { from, to } => ActViolation::CrescendoSkip {
            index: failure.index,
            from,
            to,
        },
    }
}

// ---- The Crescendo simulator ---------------------------------------

/// The Crescendo state machine: a stage (peaks passed, 0..=4)
/// that only holds or advances by one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CrescendoMachine {
    stage: i64,
}

impl CrescendoMachine {
    /// A machine at stage 0 (no peaks passed).
    pub fn new() -> CrescendoMachine {
        CrescendoMachine { stage: 0 }
    }

    /// The current stage.
    pub fn stage(&self) -> i64 {
        self.stage
    }

    /// Advances the machine by one event. Holds (the current
    /// stage) and +1 increments are admitted; a decrease, a jump
    /// over +1, or a value outside 0..=4 is refused and the
    /// stage is left unchanged.
    pub fn advance(&mut self, peaks_passed: i64) -> Result<i64, CrescendoRefusal> {
        if !(0..=CRESCENDO_STAGE_MAX).contains(&peaks_passed) {
            return Err(CrescendoRefusal::OutOfRange {
                value: peaks_passed,
            });
        }
        if peaks_passed < self.stage {
            return Err(CrescendoRefusal::Decrease {
                from: self.stage,
                to: peaks_passed,
            });
        }
        if peaks_passed > self.stage + 1 {
            return Err(CrescendoRefusal::Skip {
                from: self.stage,
                to: peaks_passed,
            });
        }
        self.stage = peaks_passed;
        Ok(self.stage)
    }
}

/// One refused Crescendo transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CrescendoRefusal {
    /// The event's value is outside 0..=4.
    OutOfRange {
        /// The event's value.
        value: i64,
    },
    /// The event decreases the stage.
    Decrease {
        /// Stage before the event.
        from: i64,
        /// The event's value.
        to: i64,
    },
    /// The event jumps more than +1.
    Skip {
        /// Stage before the event.
        from: i64,
        /// The event's value.
        to: i64,
    },
}

impl CrescendoRefusal {
    /// The stable code string for this refusal.
    pub fn code(&self) -> &'static str {
        match self {
            CrescendoRefusal::OutOfRange { .. } => "ACTLAB-CRESCENDO-OUT-OF-RANGE",
            CrescendoRefusal::Decrease { .. } => "ACTLAB-CRESCENDO-DECREASE",
            CrescendoRefusal::Skip { .. } => "ACTLAB-CRESCENDO-SKIP",
        }
    }

    /// A one-line statement naming the refused transition.
    pub fn message(&self) -> String {
        match self {
            CrescendoRefusal::OutOfRange { value } => {
                format!("peaks passed {value} is outside 0..=4")
            }
            CrescendoRefusal::Decrease { from, to } => {
                format!("stage may not decrease {from} -> {to}")
            }
            CrescendoRefusal::Skip { from, to } => {
                format!("stage may not jump {from} -> {to} (at most +1)")
            }
        }
    }
}

impl std::fmt::Display for CrescendoRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for CrescendoRefusal {}

/// A refused event in a simulated sequence: which event, and
/// the transition it attempted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrescendoFailure {
    /// Index of the refused event in the sequence.
    pub index: usize,
    /// The refused transition.
    pub refusal: CrescendoRefusal,
}

/// Steps an ordered sequence of peaks-passed events from stage
/// 0, returning the stage after each admitted event. The first
/// illegal transition refuses the whole sequence with a
/// [`CrescendoFailure`] naming its index and transition; no
/// partial trace is returned (use a [`CrescendoMachine`]
/// directly when a caller needs the accepted prefix).
pub fn simulate_crescendo(events: &[i64]) -> Result<Vec<i64>, CrescendoFailure> {
    let (trace, failure) = trace_crescendo(events);
    match failure {
        Some(failure) => Err(failure),
        None => Ok(trace),
    }
}

fn trace_crescendo(events: &[i64]) -> (Vec<i64>, Option<CrescendoFailure>) {
    let mut machine = CrescendoMachine::new();
    let mut trace = Vec::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        match machine.advance(*event) {
            Ok(stage) => trace.push(stage),
            Err(refusal) => {
                return (trace, Some(CrescendoFailure { index, refusal }));
            }
        }
    }
    (trace, None)
}

/// The record's Crescendo event sequence: the `peaks_passed`
/// values of its `ParamCrescendoStage` bindings, in order.
pub fn crescendo_events(record: &ActRecord) -> Vec<i64> {
    record
        .state_bindings
        .iter()
        .filter_map(|binding| match binding.value {
            StateValue::PeaksPassed(peaks) if binding.parameter == CRESCENDO_PARAMETER => {
                Some(peaks)
            }
            _ => None,
        })
        .collect()
}

// ---- The verb --------------------------------------------------------

/// `act_validate` inputs: the three documents as text. Faces
/// read the files (bounded) and pass text; they hold no
/// validation logic of their own.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActValidateInput {
    record: String,
    universe: String,
    rigs: Vec<String>,
}

/// Runs the `act_validate` verb: parses the three documents,
/// validates the record, and returns `(valid, document)`.
///
/// The document is `{"result": "valid" | "invalid" |
/// "unparseable", "slug": ..., "violations": [...]}`, with a
/// `"crescendo_trace"` (the stage after each admitted event,
/// the accepted prefix when a transition is refused) whenever
/// the record carries a Crescendo event sequence. A malformed
/// universe or rig snapshot is a refusal, not a verdict: the
/// record was never really checked.
pub fn run_actlab_verb(inputs: &Value) -> Result<(bool, Value), Refusal> {
    let input: ActValidateInput = serde_json::from_value(inputs.clone()).or_else(|err| {
        refuse(
            RefusalClass::Malformed,
            format!("inputs for '{ACTLAB_VERB}' do not match its schema: {err}"),
        )
    })?;
    if input.rigs.len() > ACTLAB_RIGS_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "at most {ACTLAB_RIGS_MAX} rig snapshots, got {}",
                input.rigs.len()
            ),
        );
    }
    let universe = parse_parameter_universe(&input.universe).or_else(|err| {
        refuse(
            RefusalClass::Malformed,
            format!("universe snapshot: {}", err.message()),
        )
    })?;
    let mut rigs: Vec<RigSnapshot> = Vec::with_capacity(input.rigs.len());
    for (index, text) in input.rigs.iter().enumerate() {
        let rig = parse_rig_snapshot(text).or_else(|err| {
            refuse(
                RefusalClass::Malformed,
                format!("rig snapshot {}: {}", index + 1, err.message()),
            )
        })?;
        if rigs.iter().any(|seen| seen.rig_id == rig.rig_id) {
            return refuse(
                RefusalClass::Malformed,
                format!("two rig snapshots carry rig_id '{}'", rig.rig_id),
            );
        }
        rigs.push(rig);
    }
    let record = match parse_act_record(&input.record) {
        Ok(record) => record,
        Err(err) => {
            return Ok((
                false,
                json!({
                    "result": "unparseable",
                    "violations": [err.to_json()],
                }),
            ));
        }
    };
    let violations = validate_act_record(&record, &universe, &rigs);
    let events = crescendo_events(&record);
    let (trace, _) = trace_crescendo(&events);
    let mut doc = serde_json::Map::new();
    if !events.is_empty() {
        doc.insert("crescendo_trace".to_string(), json!(trace));
    }
    doc.insert(
        "result".to_string(),
        json!(if violations.is_empty() {
            "valid"
        } else {
            "invalid"
        }),
    );
    doc.insert("slug".to_string(), json!(record.slug));
    doc.insert(
        "violations".to_string(),
        Value::Array(violations.iter().map(ActViolation::to_json).collect()),
    );
    Ok((violations.is_empty(), Value::Object(doc)))
}

#[cfg(test)]
mod tests {
    use super::{CrescendoMachine, CrescendoRefusal, simulate_crescendo, slug_problem};

    #[test]
    fn slug_grammar_accepts_kebab_and_refuses_the_rest() {
        assert_eq!(slug_problem("rope-set"), None);
        assert_eq!(slug_problem("a1-b2"), None);
        assert!(slug_problem("").is_some());
        assert!(slug_problem("Rope").is_some());
        assert!(slug_problem("-rope").is_some());
        assert!(slug_problem("rope-").is_some());
        assert!(slug_problem("rope--set").is_some());
        assert!(slug_problem("rope_set").is_some());
        assert!(slug_problem(&"a".repeat(65)).is_some());
    }

    #[test]
    fn machine_holds_and_steps_and_refuses() {
        let mut machine = CrescendoMachine::new();
        assert_eq!(machine.advance(0), Ok(0));
        assert_eq!(machine.advance(1), Ok(1));
        assert_eq!(machine.advance(1), Ok(1));
        assert_eq!(
            machine.advance(0),
            Err(CrescendoRefusal::Decrease { from: 1, to: 0 })
        );
        assert_eq!(machine.stage(), 1, "a refusal leaves the stage unchanged");
        assert_eq!(
            machine.advance(3),
            Err(CrescendoRefusal::Skip { from: 1, to: 3 })
        );
        assert_eq!(
            machine.advance(5),
            Err(CrescendoRefusal::OutOfRange { value: 5 })
        );
        assert_eq!(machine.advance(2), Ok(2));
    }

    #[test]
    fn simulate_reports_the_first_illegal_index() {
        assert_eq!(
            simulate_crescendo(&[0, 1, 1, 2, 3, 4]),
            Ok(vec![0, 1, 1, 2, 3, 4])
        );
        assert_eq!(simulate_crescendo(&[]), Ok(vec![]));
        let failure = simulate_crescendo(&[0, 1, 0]).expect_err("decrease refuses");
        assert_eq!(failure.index, 2);
        assert_eq!(
            failure.refusal,
            CrescendoRefusal::Decrease { from: 1, to: 0 }
        );
    }
}
