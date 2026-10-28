//! The authority census (AMENDMENT-T4 §2.7 step 9, §2.1).
//!
//! A gear document may declare that an item is
//! mirror-*capable*. It may never enable mirroring, bind a
//! physical device, carry a user/session consent state, or
//! override ache's ceiling — that authority exists only in the
//! user's hands, in the app, per session. Any field that
//! attempts it is refused `TG-GEAR-MIRROR-AUTHORITY`.
//!
//! The census is generic over the project's RON value tree so
//! the same scan covers gear documents, scene files, and
//! preference-set files (gate T4-G1.4 spans all three file
//! kinds). Authority field names are *recognized* names: the
//! unknown-field census (validation step 2) deliberately does
//! not claim them, so the authority refusal — not a generic
//! unknown-field refusal — is what an authority-bearing file
//! meets at step 9.

use torogao_author::ron::Ron;

/// Field names that carry authority a file may never hold,
/// alphabetical. `mirror_capable` is deliberately absent: it
/// is the one legal capability declaration (§2.3 C).
pub const AUTHORITY_FIELDS: &[&str] = &[
    "ache_ceiling",
    "bind_device",
    "bound_device",
    "ceiling",
    "ceiling_override",
    "consent",
    "consent_state",
    "device_binding",
    "mirror",
    "mirror_ceiling",
    "mirror_enable",
    "mirror_enabled",
    "session_consent",
];

/// Whether a field name is an authority field.
pub fn is_authority_field(name: &str) -> bool {
    AUTHORITY_FIELDS.contains(&name)
}

/// Finds the first authority field in a RON value tree, in
/// document order, returning its field path (e.g.
/// `device.mirror_enabled`, `anchors[0].ceiling`).
pub fn find_authority_field(value: &Ron) -> Option<String> {
    find_in(value, "")
}

fn child_path(path: &str, field: &str) -> String {
    if path.is_empty() {
        field.to_string()
    } else {
        format!("{path}.{field}")
    }
}

fn find_in(value: &Ron, path: &str) -> Option<String> {
    match value {
        Ron::Struct(_, fields) => {
            for (name, inner) in fields {
                let here = child_path(path, name);
                if is_authority_field(name) {
                    return Some(here);
                }
                if let Some(found) = find_in(inner, &here) {
                    return Some(found);
                }
            }
            None
        }
        Ron::List(items) => {
            for (index, item) in items.iter().enumerate() {
                let here = format!("{path}[{index}]");
                if let Some(found) = find_in(item, &here) {
                    return Some(found);
                }
            }
            None
        }
        Ron::Tuple(items) => {
            for (index, item) in items.iter().enumerate() {
                let here = format!("{path}({index})");
                if let Some(found) = find_in(item, &here) {
                    return Some(found);
                }
            }
            None
        }
        Ron::Opt(Some(inner)) => find_in(inner, path),
        _ => None,
    }
}

/// Scans RON *text* (any of the file kinds T4-G1.4 covers)
/// and returns the first authority field path, if any. A text
/// that does not parse has no authority field to find; parse
/// failure is the caller's schema concern, not this census's.
pub fn scan_authority_text(text: &str) -> Option<String> {
    let value = torogao_author::ron::parse_document(text).ok()?;
    find_authority_field(&value)
}
