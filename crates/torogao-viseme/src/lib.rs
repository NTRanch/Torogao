//! torogao-viseme: lip sync for Torogao, baked at
//! authoring time.
//!
//! Rhubarb Lip Sync (DanielSWolf) is the estate's adopted
//! oracle for lip sync (survey row #16, D-746): it
//! analyses an existing recording and emits timed
//! mouth-shape cues (A–H + X). Rhubarb is an
//! authoring-time tool only — nothing shipped depends on
//! it. This crate is the deterministic half of the pair:
//! it parses a Rhubarb cue file (`cue`), resolves every
//! shape through the versioned mapping table (`map`,
//! `torogao-viseme-map/1`) onto the governed Fac_ mouth
//! morph channels, and bakes a 60 Hz scalar track
//! (`bake`) in the shape of the motion-clip spec's face
//! group (§4.3), with the spec's provenance block (§6)
//! naming this converter and the cue file's hash.
//!
//! Same cue file in, byte-identical track out: the bake
//! law is stated in full in `bake`, and the determinism
//! is proven by the test suite's double-run hash check.
//!
//! Voice generation is tabled (D-481) and nothing here
//! synthesises speech; the oracle consumes recordings
//! that already exist, and this crate consumes the
//! oracle's cue files. Audio is handled as files, never
//! played (the audio silence rule).

#![forbid(unsafe_code)]

pub mod bake;
pub mod cue;
pub mod error;
pub mod map;

pub use bake::{BakeReport, BakedTrack, bake};
pub use cue::{Cue, Shape, parse_cues_json, parse_cues_tsv};
pub use error::{VisemeError, VisemeResult};
pub use map::VisemeMap;

/// This converter's id, as named in baked tracks'
/// provenance (the named-converter contract: a conversion
/// is owned by a versioned name).
pub const CONVERTER_ID: &str = "torogao-viseme/1";

/// Hex SHA-256 of a byte string — the form provenance
/// records carry (`source_sha256`).
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha256_hex_matches_the_empty_input_vector() {
        assert_eq!(
            super::sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
