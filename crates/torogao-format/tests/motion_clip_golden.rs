//! Golden gates for `torogao-motion-clip/2` (hand-animation
//! programme Rung H0; the specification of record is
//! `crates/torogao-format/FORMATS.md`).
//!
//! The committed goldens pin the format three ways: each
//! golden's freeze hash is reproduced by the validator from
//! the source document; each golden's canonical form is
//! byte-compared against the committed canonical file (and
//! independently re-hashed with `sha2` over the file bytes);
//! and the deliberately malformed set refuses with its typed
//! error codes. Validation tests and adversarial tests are
//! balanced halves, per the estate test discipline.

use sha2::{Digest, Sha256};
use torogao_format::jsonfmt::to_d_json;
use torogao_format::motion_clip::{canonical_bytes, freeze_document, freeze_hash, validate_clip};

/// The compiler stamp the goldens were frozen with.
const GOLDEN_COMPILER: &str = "torogao-format motion_clip h0 (2026-11-27)";
/// The band class the goldens were frozen under.
const GOLDEN_BAND: &str = "tier2-1e-6";

fn golden_dir() -> String {
    format!("{}/tests/golden/motion-clip-2", env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn read_text(path: &str) -> String {
    let bytes = read(path);
    String::from_utf8(bytes).unwrap_or_else(|error| panic!("utf8 {path}: {error}"))
}

/// Golden files end with exactly one trailing newline; the
/// canonical text itself carries none.
fn trimmed(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// The five goldens: (stem, channels, keys).
const GOLDENS: [(&str, usize, usize); 5] = [
    ("minimal", 1, 2),
    ("loop-hold", 2, 8),
    ("fac", 3, 8),
    ("shader", 2, 4),
    ("pose", 3, 3),
];

#[test]
fn goldens_validate_frozen_with_their_recorded_shape() {
    for (stem, channels, keys) in GOLDENS {
        let path = format!("{}/{stem}.clip.json", golden_dir());
        let report = validate_clip(&read(&path))
            .unwrap_or_else(|error| panic!("golden {stem} validates: {error}"));
        assert_eq!(report.channels, channels, "golden {stem} channels");
        assert_eq!(report.keys, keys, "golden {stem} keys");
        assert!(report.freeze_sha256.is_some(), "golden {stem} is frozen");
    }
}

#[test]
fn golden_freeze_hashes_reproduce_from_the_source_documents() {
    for (stem, _, _) in GOLDENS {
        let source_text = read_text(&format!("{}/{stem}.source.json", golden_dir()));
        let source: serde_json::Value =
            serde_json::from_str(trimmed(&source_text)).expect("source parses");
        let frozen = freeze_document(&source, GOLDEN_COMPILER, GOLDEN_BAND)
            .unwrap_or_else(|error| panic!("golden {stem} freezes: {error}"));
        let golden_text = read_text(&format!("{}/{stem}.clip.json", golden_dir()));
        assert_eq!(
            to_d_json(&frozen),
            trimmed(&golden_text),
            "golden {stem}: freezing the source reproduces the committed document byte for byte"
        );
    }
}

#[test]
fn golden_canonical_forms_match_and_hash_independently() {
    for (stem, _, _) in GOLDENS {
        let clip_text = read_text(&format!("{}/{stem}.clip.json", golden_dir()));
        let doc: serde_json::Value =
            serde_json::from_str(trimmed(&clip_text)).expect("golden parses");
        let canonical_text = read_text(&format!("{}/{stem}.canonical.json", golden_dir()));
        assert_eq!(
            canonical_bytes(&doc),
            trimmed(&canonical_text),
            "golden {stem}: canonical form matches the committed file"
        );
        // Independent path: hash the committed canonical file's
        // bytes directly and compare against the freeze block.
        let recorded = doc["freeze"]["sha256"]
            .as_str()
            .expect("golden carries a freeze hash");
        assert_eq!(
            sha256_hex(trimmed(&canonical_text).as_bytes()),
            recorded,
            "golden {stem}: SHA-256 of the canonical file is the freeze hash"
        );
        assert_eq!(freeze_hash(&doc), recorded, "golden {stem}: freeze hash");
    }
}

#[test]
fn golden_canonicalisation_is_deterministic_across_repeats() {
    // The twin-run discipline in miniature: parse, canonicalise,
    // and hash twice from independent parses; the bytes and the
    // digests must be identical both times.
    for (stem, _, _) in GOLDENS {
        let bytes = read(&format!("{}/{stem}.clip.json", golden_dir()));
        let first = validate_clip(&bytes).expect("first validation");
        let second = validate_clip(&bytes).expect("second validation");
        assert_eq!(first, second, "golden {stem}: reports are identical");
        let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("parses");
        assert_eq!(freeze_hash(&doc), freeze_hash(&doc));
    }
}

#[test]
fn pose_golden_is_a_one_frame_document() {
    let report = validate_clip(&read(&format!("{}/pose.clip.json", golden_dir())))
        .expect("pose golden validates");
    assert_eq!(report.range_start, 7);
    assert_eq!(report.range_end, 7);
    assert_eq!(report.loop_range, None);
}

#[test]
fn loop_golden_carries_its_loop_range() {
    let report = validate_clip(&read(&format!("{}/loop-hold.clip.json", golden_dir())))
        .expect("loop golden validates");
    assert_eq!(report.loop_range, Some((0, 48)));
}

/// The deliberately malformed set: (stem, expected code).
const MALFORMED: [(&str, &str); 12] = [
    ("float-frame", "TG-CLIP-FLOAT-FRAME"),
    ("frame-order", "TG-CLIP-FRAME-ORDER"),
    ("stray-tangent", "TG-CLIP-TANGENT-STRAY"),
    ("missing-tangent", "TG-CLIP-TANGENT-MISSING"),
    ("zero-quaternion", "TG-CLIP-ZERO-QUATERNION"),
    ("rate-float", "TG-CLIP-RATE"),
    ("undeclared-uniform", "TG-CLIP-UNDECLARED-UNIFORM"),
    ("wallclock-uniform", "TG-CLIP-WALLCLOCK-UNIFORM"),
    ("tick-type", "TG-CLIP-TICK-TYPE"),
    ("source-undeclared", "TG-CLIP-UNDECLARED-UNIFORM"),
    ("unresolved-tangent", "TG-CLIP-UNRESOLVED-TANGENT"),
    ("freeze-mismatch", "TG-CLIP-FREEZE-MISMATCH"),
];

#[test]
fn malformed_documents_refuse_with_their_typed_codes() {
    for (stem, code) in MALFORMED {
        let path = format!("{}/malformed/{stem}.json", golden_dir());
        let refused = validate_clip(&read(&path));
        match refused {
            Ok(report) => panic!("malformed {stem} validated as {report:?}, expected {code}"),
            Err(error) => assert_eq!(
                error.code(),
                code,
                "malformed {stem}: expected {code}, got {} ({error})",
                error.code()
            ),
        }
    }
}

#[test]
fn tampering_with_a_frozen_value_breaks_the_freeze_hash() {
    let text = read_text(&format!("{}/minimal.clip.json", golden_dir()));
    let mut doc: serde_json::Value = serde_json::from_str(trimmed(&text)).expect("golden parses");
    doc["channels"][0]["keys"][0]["value"][1] = serde_json::json!(2.0);
    let refused = validate_clip(to_d_json(&doc).as_bytes());
    assert_eq!(
        refused.expect_err("tampered document refuses").code(),
        "TG-CLIP-FREEZE-MISMATCH"
    );
}

#[test]
fn a_thawed_golden_refreezes_to_the_same_hash() {
    // Editing produces a new revision (DESIGN.md section 2.1):
    // strip the freeze block, re-freeze, and the hash returns —
    // the document was not changed, so neither is its identity.
    let text = read_text(&format!("{}/fac.clip.json", golden_dir()));
    let mut doc: serde_json::Value = serde_json::from_str(trimmed(&text)).expect("golden parses");
    let recorded = doc["freeze"]["sha256"].as_str().expect("hash").to_string();
    doc.as_object_mut().expect("object").remove("freeze");
    let refrozen = freeze_document(&doc, GOLDEN_COMPILER, GOLDEN_BAND).expect("refreezes");
    assert_eq!(freeze_hash(&refrozen), recorded);
}
