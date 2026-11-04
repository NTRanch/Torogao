//! The `TG-GENCOND-*` refusal taxonomy.
//!
//! Every refusal the crate produces carries exactly one stable
//! code. Refusals are data (typed errors), never prose panics,
//! matching the S-6 discipline: named code, bounded message,
//! no partial compilation.

use serde_json::{Value, json};

/// A spatial source's map decoded to a single uniform colour:
/// the false off, refused by name (DESIGN §2.1; Phase 0 measured
/// a solid-black map at strength 0.8 perturbing output at full
/// conditioning scale, MAD 49.491).
pub const CODE_BLANK_MAP: &str = "TG-GENCOND-BLANK-MAP";
/// Two definitions for one id or text slot disagree.
pub const CODE_CONFLICT: &str = "TG-GENCOND-CONFLICT";
/// A named model, asset blob, or binding names nothing that
/// exists in the compile inputs.
pub const CODE_DANGLING: &str = "TG-GENCOND-DANGLING";
/// Asset bytes do not hash to the SHA-256 the record carries.
pub const CODE_HASH_MISMATCH: &str = "TG-GENCOND-HASH-MISMATCH";
/// Input is structurally invalid: shape, arity, ids, unknown
/// fields or variants, non-finite numbers.
pub const CODE_MALFORMED: &str = "TG-GENCOND-MALFORMED";
/// A pose map is not a decodable 8/16-bit PNG.
pub const CODE_MAP_FORMAT: &str = "TG-GENCOND-MAP-FORMAT";
/// A value lies outside its declared or bounded range.
pub const CODE_RANGE: &str = "TG-GENCOND-RANGE";
/// An embed measurement was taken under a different reference
/// hash or vision encoder than the source declares: stale
/// evidence, refused rather than applied (DESIGN §2.2).
pub const CODE_STALE_MEASUREMENT: &str = "TG-GENCOND-STALE-MEASUREMENT";
/// A record names a schema version this build does not know.
pub const CODE_UNSUPPORTED_SCHEMA: &str = "TG-GENCOND-UNSUPPORTED-SCHEMA";
/// An enabled reference-embed source has no embed measurement
/// in the compile inputs. Normalisation is a named step, never
/// assumed (DESIGN §2.2), so compiling blind refuses closed.
pub const CODE_UNMEASURED_REFERENCE: &str = "TG-GENCOND-UNMEASURED-REFERENCE";
/// A record or graph names a kind this build does not know.
pub const CODE_UNKNOWN_KIND: &str = "TG-GENCOND-UNKNOWN-KIND";

/// Longest message kept, in bytes; longer messages are cut at a
/// character boundary so a hostile id cannot bloat output.
const MESSAGE_BYTES_MAX: usize = 1024;

/// One typed refusal: a stable code plus a bounded
/// human-readable message. Messages name ids and fields, never
/// file contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GencondError {
    /// The stable `TG-GENCOND-*` code.
    pub code: &'static str,
    /// What was refused and why.
    pub message: String,
}

impl GencondError {
    /// Builds a refusal, bounding the message length.
    pub fn new(code: &'static str, message: impl Into<String>) -> GencondError {
        let mut message = message.into();
        if message.len() > MESSAGE_BYTES_MAX {
            let mut cut = MESSAGE_BYTES_MAX;
            while !message.is_char_boundary(cut) {
                cut -= 1;
            }
            message.truncate(cut);
        }
        GencondError { code, message }
    }

    /// The refusal as a JSON object `{code, message}`.
    pub fn to_json(&self) -> Value {
        json!({"code": self.code, "message": self.message})
    }
}

impl std::fmt::Display for GencondError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for GencondError {}

/// Shorthand for `Err(GencondError::new(code, message))`.
pub fn refuse<T>(code: &'static str, message: impl Into<String>) -> Result<T, GencondError> {
    Err(GencondError::new(code, message))
}

/// The result type of the seam's fallible operations.
pub type GencondResult<T> = Result<T, GencondError>;
