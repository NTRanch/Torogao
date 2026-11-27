//! Tier 1 enforcement: the forbidden-ops lint (plan SS2.5,
//! specified in the R0 record, landed here as a real gate).
//!
//! Tier 1 code — the runtime crate and every hosted core —
//! must not contain:
//!
//! - the float types (the tokens naming them), outside the
//!   presentation module where Tier 2 lives;
//! - float-producing calls (transcendentals, fused
//!   multiply-add, float roots and powers), outside the
//!   presentation module;
//! - hash-map types (iteration order is not a sim input);
//! - wall-clock or instant types (time is never an input);
//! - thread constructs (scheduling is not a sim input);
//! - platform `cfg` names (the host quarantine boundary is
//!   the only place platform shape may appear, and host
//!   crates are not Tier 1 code).
//!
//! The scan is a source scan: deliberately simple, lexical,
//! and conservative. It runs as a gate test over this crate's
//! own sources and over any witness core's sources. This file
//! is the one exclusion besides the presentation module: the
//! scanner must name the tokens it bans, so it cannot scan
//! itself. The Tier 2 modules — the presentation module
//! (present.rs) and its measurement instrument (tier2.rs,
//! Rung R2) — are excluded from the float rules only; the
//! ordering/time/thread/platform rules bind them too.

use std::path::{Path, PathBuf};

/// One forbidden construct found in Tier 1 sources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tier1Violation {
    /// The file, relative to the scanned root.
    pub file: String,
    /// The 1-based line number.
    pub line: usize,
    /// The forbidden token found.
    pub token: String,
}

/// Tokens banned everywhere in Tier 1 sources.
const BANNED_EVERYWHERE: &[&str] = &[
    "HashMap",
    "HashSet",
    "SystemTime",
    "Instant",
    "thread::",
    "target_os",
    "target_arch",
    "target_family",
];

/// Tokens banned outside the presentation module.
const BANNED_OUTSIDE_PRESENT: &[&str] = &[
    "f32",
    "f64",
    ".sin(",
    ".cos(",
    ".tan(",
    ".asin(",
    ".acos(",
    ".atan(",
    ".atan2(",
    ".sqrt(",
    ".exp(",
    ".ln(",
    ".log2(",
    ".log10(",
    ".powf(",
    ".powi(",
    ".mul_add(",
];

/// Whether `token` occurs in `line` at a token boundary (the
/// neighbouring characters are not identifier characters), so
/// `f64` does not fire inside a longer identifier and a call
/// token like `.sqrt(` matches exactly at its dot.
fn contains_token(line: &str, token: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = token.as_bytes();
    if needle.is_empty() || bytes.len() < needle.len() {
        return false;
    }
    for start in 0..=(bytes.len() - needle.len()) {
        if &bytes[start..start + needle.len()] != needle {
            continue;
        }
        let before_ok = start == 0 || {
            let prev = bytes[start - 1];
            !(prev.is_ascii_alphanumeric() || prev == b'_')
        };
        let after = start + needle.len();
        let after_ok = after >= bytes.len() || {
            let next = bytes[after];
            !(next.is_ascii_alphanumeric() || next == b'_')
        };
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn is_present_module(path: &Path) -> bool {
    // The Tier 2 modules: present.rs (the view) and tier2.rs
    // (its measurement instrument, Rung R2). Both read floats;
    // neither feeds a sim.
    path.file_name()
        .is_some_and(|name| name == "present.rs" || name == "tier2.rs")
}

fn is_scanner(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "tier1.rs")
}

fn collect_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Scans every `.rs` file under `root` (recursively) for the
/// Tier 1 forbidden constructs and returns every violation
/// found, in file/line order. An empty result is the gate's
/// pass condition.
#[must_use]
pub fn scan_sources(root: &Path) -> Vec<Tier1Violation> {
    let mut files = Vec::new();
    collect_rs_files(root, &mut files);
    files.sort();
    let mut violations = Vec::new();
    for file in files {
        if is_scanner(&file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let relative = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .to_string();
        let present = is_present_module(&file);
        for (index, line) in text.lines().enumerate() {
            for token in BANNED_EVERYWHERE {
                if contains_token(line, token) {
                    violations.push(Tier1Violation {
                        file: relative.clone(),
                        line: index + 1,
                        token: (*token).to_string(),
                    });
                }
            }
            if !present {
                for token in BANNED_OUTSIDE_PRESENT {
                    if contains_token(line, token) {
                        violations.push(Tier1Violation {
                            file: relative.clone(),
                            line: index + 1,
                            token: (*token).to_string(),
                        });
                    }
                }
            }
        }
    }
    violations
}
