//! Typed errors for `torogao-motion-clip/1`.
//!
//! Every variant maps to exactly one stable code in the
//! specification's loud-error table (SPEC.md section 10.1).
//! Codes are never renumbered; a conforming validator reports
//! the FIRST failure in the validation order of SPEC.md
//! section 9, so at most one of these is ever produced by a
//! load. Context strings copied from the input are truncated
//! before they enter an error, so hostile input cannot bloat
//! error values.

use std::fmt;

/// Maximum bytes of an input string quoted inside an error.
const ERROR_QUOTE_BYTES_MAX: usize = 96;

/// Truncate a string to at most `ERROR_QUOTE_BYTES_MAX` bytes
/// on a char boundary.
pub(crate) fn bounded_quote(text: &str) -> String {
    if text.len() <= ERROR_QUOTE_BYTES_MAX {
        return text.to_string();
    }
    let mut end = ERROR_QUOTE_BYTES_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// A parse, validation, or binding failure. The stage that
/// raises each code (document vs binding) is noted per variant
/// and matches SPEC.md section 10.1.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipError {
    /// TMC-101 (D): input exceeds `CLIP_BYTES_MAX` bytes.
    InputTooLarge {
        /// Input length in bytes.
        bytes: usize,
        /// The bound that was exceeded.
        max: usize,
    },
    /// TMC-102 (D): JSON syntax error, or a byte-order mark.
    Syntax {
        /// 1-based line of the failure.
        line: usize,
        /// 1-based column of the failure.
        column: usize,
        /// The parser's message.
        message: String,
    },
    /// TMC-103 (D): schema error — missing required field,
    /// wrong type, duplicate key, malformed integer, empty
    /// component key list, track with no component lists,
    /// empty group list.
    Schema {
        /// What was wrong, in bounded terms.
        message: String,
    },
    /// TMC-104 (D): unknown field present.
    UnknownField {
        /// The parser's message naming the field.
        message: String,
    },
    /// TMC-105 (D): `format` string is not a
    /// `torogao-motion-clip` token, or missing.
    UnsupportedFormat {
        /// The token found, if any.
        found: String,
    },
    /// TMC-106 (D): `format` token names an unsupported
    /// version of this format.
    UnsupportedVersion {
        /// The token found.
        found: String,
    },
    /// TMC-107 (D): a collection or per-object count exceeds
    /// the SPEC.md section 12 bounds.
    CountExceeded {
        /// Which collection or bounded field.
        collection: &'static str,
        /// The count found.
        count: usize,
        /// The bound that was exceeded.
        max: usize,
    },
    /// TMC-108 (D): a declaration-only group (`camera`,
    /// `hands`) carries a `tracks` field.
    DeclarationOnlyTracks {
        /// The group kind that carried tracks.
        kind: String,
    },
    /// TMC-201 (D): an id violates the charset/length rule.
    InvalidId {
        /// The offending id, bounded.
        id: String,
    },
    /// TMC-202 (D): duplicate within a collection — group
    /// kind, track target in a group, layer id, scope entry.
    Duplicate {
        /// Which collection.
        collection: &'static str,
        /// The duplicated value, bounded.
        id: String,
    },
    /// TMC-203 (B): a bone or channel name is not in the
    /// bound records.
    UnresolvedReference {
        /// The name that did not resolve.
        name: String,
    },
    /// TMC-204 (B): the presented SkeletonDef does not hash
    /// to `skeleton.sha256`, or is not the named def.
    SkeletonHashMismatch {
        /// The hash the clip binds to.
        expected: String,
        /// The hash of the presented def document.
        found: String,
    },
    /// TMC-205 (D): canonical order violated (groups, tracks,
    /// or scope lists not in canonical order, SPEC.md §1.2).
    CanonicalOrder {
        /// Which collection was out of order.
        what: &'static str,
    },
    /// TMC-301 (D): a non-finite number, or a numeric value
    /// that overflows f64 in parsing.
    NonFinite {
        /// Where the non-finite value sits.
        context: String,
    },
    /// TMC-302 (D): layer weight outside `[0, 1]`.
    WeightOutOfRange {
        /// The layer whose weight is out of range.
        layer: String,
        /// The weight found.
        weight: f64,
    },
    /// TMC-303 (D): a quaternion key (base or delta) is not
    /// unit within `QUAT_UNIT_TOLERANCE`.
    NonUnitQuaternion {
        /// Where the key sits.
        context: String,
    },
    /// TMC-304 (D): scale violation — base scale component
    /// ≤ 0, or layer scale factor outside
    /// `(0, SCALE_FACTOR_MAX]`.
    ScaleViolation {
        /// Where the component sits.
        context: String,
    },
    /// TMC-305 (D): magnitude bound exceeded — translation
    /// component beyond `COORD_MAGNITUDE_MAX`, scalar value
    /// or delta beyond `SCALAR_MAGNITUDE_MAX`.
    MagnitudeExceeded {
        /// Where the value sits.
        context: String,
    },
    /// TMC-306 (D): `frames` is 0 or exceeds `FRAMES_MAX`.
    FramesOutOfRange {
        /// The frame count found.
        frames: u32,
    },
    /// TMC-501 (D): key frames not strictly increasing
    /// within a track, or a key frame ≥ the clip's `frames`.
    KeyOrder {
        /// Where the key list sits.
        context: String,
    },
    /// TMC-502 (D): interpolation token unknown.
    InterpolationToken {
        /// The token found, bounded.
        found: String,
    },
    /// TMC-601 (D): a layer track targets a bone/channel
    /// outside the layer's declared scope.
    ScopeViolation {
        /// The layer whose scope was exceeded.
        layer: String,
        /// The out-of-scope target.
        target: String,
    },
    /// TMC-602 (B): a layer scope names a bone/channel the
    /// bound records do not declare.
    ScopeUnresolved {
        /// The layer whose scope did not resolve.
        layer: String,
        /// The name that did not resolve.
        name: String,
    },
    /// TMC-603 (D): duplicate target within one layer.
    DuplicateLayerTarget {
        /// The layer with the duplicate.
        layer: String,
        /// The duplicated target.
        target: String,
    },
    /// TMC-701 (D): provenance `source_class` token unknown.
    SourceClassToken {
        /// The token found, bounded.
        found: String,
    },
    /// TMC-702 (D): frame map invalid — entry count ≠ clip
    /// `frames`, entries not non-decreasing, an entry ≥
    /// `source_frame_count`, or a malformed `source_rate`.
    FrameMapInvalid {
        /// What was wrong with the map.
        message: String,
    },
    /// TMC-703 (D): provenance group entries do not cover
    /// the clip's groups exactly once each.
    ProvenanceCoverage {
        /// What was wrong with the coverage.
        message: String,
    },
    /// TMC-704 (D): a hash field is malformed (not 64
    /// lowercase hex characters).
    HashMalformed {
        /// Which hash field.
        field: &'static str,
    },
    /// TMC-705 (D): self-hash mismatch — `sha256` present
    /// but does not match the document.
    SelfHashMismatch,
}

impl ClipError {
    /// The stable `TMC-…` code for this error (SPEC.md §10.1).
    pub fn code(&self) -> &'static str {
        match self {
            ClipError::InputTooLarge { .. } => "TMC-101",
            ClipError::Syntax { .. } => "TMC-102",
            ClipError::Schema { .. } => "TMC-103",
            ClipError::UnknownField { .. } => "TMC-104",
            ClipError::UnsupportedFormat { .. } => "TMC-105",
            ClipError::UnsupportedVersion { .. } => "TMC-106",
            ClipError::CountExceeded { .. } => "TMC-107",
            ClipError::DeclarationOnlyTracks { .. } => "TMC-108",
            ClipError::InvalidId { .. } => "TMC-201",
            ClipError::Duplicate { .. } => "TMC-202",
            ClipError::UnresolvedReference { .. } => "TMC-203",
            ClipError::SkeletonHashMismatch { .. } => "TMC-204",
            ClipError::CanonicalOrder { .. } => "TMC-205",
            ClipError::NonFinite { .. } => "TMC-301",
            ClipError::WeightOutOfRange { .. } => "TMC-302",
            ClipError::NonUnitQuaternion { .. } => "TMC-303",
            ClipError::ScaleViolation { .. } => "TMC-304",
            ClipError::MagnitudeExceeded { .. } => "TMC-305",
            ClipError::FramesOutOfRange { .. } => "TMC-306",
            ClipError::KeyOrder { .. } => "TMC-501",
            ClipError::InterpolationToken { .. } => "TMC-502",
            ClipError::ScopeViolation { .. } => "TMC-601",
            ClipError::ScopeUnresolved { .. } => "TMC-602",
            ClipError::DuplicateLayerTarget { .. } => "TMC-603",
            ClipError::SourceClassToken { .. } => "TMC-701",
            ClipError::FrameMapInvalid { .. } => "TMC-702",
            ClipError::ProvenanceCoverage { .. } => "TMC-703",
            ClipError::HashMalformed { .. } => "TMC-704",
            ClipError::SelfHashMismatch => "TMC-705",
        }
    }
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ClipError::InputTooLarge { bytes, max } => {
                write!(f, "{code}: input is {bytes} bytes, max is {max}")
            }
            ClipError::Syntax {
                line,
                column,
                message,
            } => write!(f, "{code}: JSON syntax error at {line}:{column}: {message}"),
            ClipError::Schema { message } => write!(f, "{code}: schema error: {message}"),
            ClipError::UnknownField { message } => {
                write!(f, "{code}: unknown field: {message}")
            }
            ClipError::UnsupportedFormat { found } => {
                write!(f, "{code}: unsupported format token `{found}`")
            }
            ClipError::UnsupportedVersion { found } => {
                write!(f, "{code}: unsupported format version `{found}`")
            }
            ClipError::CountExceeded {
                collection,
                count,
                max,
            } => write!(f, "{code}: {collection} count {count} exceeds max {max}"),
            ClipError::DeclarationOnlyTracks { kind } => {
                write!(f, "{code}: declaration-only group `{kind}` carries tracks")
            }
            ClipError::InvalidId { id } => write!(f, "{code}: invalid id `{id}`"),
            ClipError::Duplicate { collection, id } => {
                write!(f, "{code}: duplicate `{id}` in {collection}")
            }
            ClipError::UnresolvedReference { name } => {
                write!(f, "{code}: unresolved reference `{name}`")
            }
            ClipError::SkeletonHashMismatch { expected, found } => write!(
                f,
                "{code}: skeleton hash mismatch, clip binds {expected}, def hashes to {found}"
            ),
            ClipError::CanonicalOrder { what } => {
                write!(f, "{code}: canonical order violated in {what}")
            }
            ClipError::NonFinite { context } => {
                write!(f, "{code}: non-finite number in {context}")
            }
            ClipError::WeightOutOfRange { layer, weight } => {
                write!(f, "{code}: layer `{layer}` weight {weight} outside [0, 1]")
            }
            ClipError::NonUnitQuaternion { context } => {
                write!(f, "{code}: non-unit quaternion in {context}")
            }
            ClipError::ScaleViolation { context } => {
                write!(f, "{code}: scale violation in {context}")
            }
            ClipError::MagnitudeExceeded { context } => {
                write!(f, "{code}: magnitude bound exceeded in {context}")
            }
            ClipError::FramesOutOfRange { frames } => {
                write!(f, "{code}: frames {frames} out of range")
            }
            ClipError::KeyOrder { context } => {
                write!(f, "{code}: key frame order/range violation in {context}")
            }
            ClipError::InterpolationToken { found } => {
                write!(f, "{code}: unknown interpolation token `{found}`")
            }
            ClipError::ScopeViolation { layer, target } => write!(
                f,
                "{code}: layer `{layer}` track targets `{target}` outside its scope"
            ),
            ClipError::ScopeUnresolved { layer, name } => {
                write!(f, "{code}: layer `{layer}` scope names undeclared `{name}`")
            }
            ClipError::DuplicateLayerTarget { layer, target } => {
                write!(f, "{code}: layer `{layer}` has two tracks for `{target}`")
            }
            ClipError::SourceClassToken { found } => {
                write!(f, "{code}: unknown source_class token `{found}`")
            }
            ClipError::FrameMapInvalid { message } => {
                write!(f, "{code}: frame map invalid: {message}")
            }
            ClipError::ProvenanceCoverage { message } => {
                write!(f, "{code}: provenance coverage: {message}")
            }
            ClipError::HashMalformed { field } => {
                write!(f, "{code}: hash field `{field}` is malformed")
            }
            ClipError::SelfHashMismatch => {
                write!(f, "{code}: self-hash does not match the document")
            }
        }
    }
}

impl std::error::Error for ClipError {}
