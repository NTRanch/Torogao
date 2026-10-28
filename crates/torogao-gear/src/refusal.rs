//! Typed refusals for the gear pipeline (AMENDMENT-T4 §2.7).
//!
//! Every refusal carries the stable code, the document path it
//! was raised against, and the field path inside the document,
//! so a surface can render it verbatim and a test can assert the
//! exact triple. Validation returns the FIRST refusal in the
//! §2.7 pipeline order; there is no error accumulation.

/// Malformed document or unsupported `torogao-gear/*` version.
pub const TG_GEAR_SCHEMA: &str = "TG-GEAR-SCHEMA";
/// Unknown field anywhere in the document.
pub const TG_GEAR_FIELD_UNKNOWN: &str = "TG-GEAR-FIELD-UNKNOWN";
/// Required class block or state variant missing.
pub const TG_GEAR_CLASS_INCOMPLETE: &str = "TG-GEAR-CLASS-INCOMPLETE";
/// Anchor not resolvable on the target rig template.
pub const TG_GEAR_ANCHOR_UNKNOWN: &str = "TG-GEAR-ANCHOR-UNKNOWN";
/// Any numeric field outside its class bound.
pub const TG_GEAR_BOUND: &str = "TG-GEAR-BOUND";
/// Referenced part/art payload absent.
pub const TG_GEAR_PART_MISSING: &str = "TG-GEAR-PART-MISSING";
/// Payload bytes do not match the declared hash.
pub const TG_GEAR_PART_HASH: &str = "TG-GEAR-PART-HASH";
/// Hook outside the §2.8 whitelist.
pub const TG_GEAR_HOOK_DENIED: &str = "TG-GEAR-HOOK-DENIED";
/// Waveform kind/parameter combination invalid.
pub const TG_GEAR_WAVEFORM: &str = "TG-GEAR-WAVEFORM";
/// File attempts to enable mirroring, bind a device, carry
/// session consent, or override ache's ceiling.
pub const TG_GEAR_MIRROR_AUTHORITY: &str = "TG-GEAR-MIRROR-AUTHORITY";
/// Duplicate item id inside a pack.
pub const TG_GEAR_PACK_DUPLICATE: &str = "TG-GEAR-PACK-DUPLICATE";
/// Gear dependency cycle.
pub const TG_GEAR_PACK_CYCLE: &str = "TG-GEAR-PACK-CYCLE";
/// Target character/scene lacks adult + consent-forward stamps.
pub const TG_GEAR_CONSENT_STAMP: &str = "TG-GEAR-CONSENT-STAMP";

/// One typed refusal: stable code + document path + field path
/// + a human-readable message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GearRefusal {
    /// One of the `TG-GEAR-*` constants in this module.
    pub code: &'static str,
    /// The document the refusal was raised against (a file path
    /// or a caller-supplied document label).
    pub document_path: String,
    /// Dotted field path inside the document, e.g.
    /// `anchors[1].anchor` or `device.waveform`.
    pub field_path: String,
    /// Human-readable detail, bounded and secret-free.
    pub message: String,
}

impl GearRefusal {
    /// Builds a refusal.
    pub fn new(
        code: &'static str,
        document_path: &str,
        field_path: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            document_path: document_path.to_string(),
            field_path: field_path.to_string(),
            message: message.into(),
        }
    }

    /// The verbatim one-line rendering surfaces display
    /// (§2.9 step 2: refusals rendered verbatim in the
    /// Creator's validator panel).
    pub fn verbatim(&self) -> String {
        format!(
            "{} at {}:{} — {}",
            self.code, self.document_path, self.field_path, self.message
        )
    }
}

impl std::fmt::Display for GearRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.verbatim())
    }
}

impl std::error::Error for GearRefusal {}

/// The gear pipeline's result type.
pub type GearResult<T> = Result<T, GearRefusal>;
