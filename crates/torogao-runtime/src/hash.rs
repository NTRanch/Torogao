//! The runtime hash type: a SHA-256 digest rendered as
//! lowercase hex — the same rendering torogao-author's
//! `state_hash` produces, so an AUTH-G chain and a runtime
//! chain are directly comparable strings.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A SHA-256 digest (lowercase hex). This is the cross-platform
/// identity of a run: Tier 1 chains compare these bit-for-bit
/// on every target.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Hash(pub String);

impl Hash {
    /// Hashes bytes with SHA-256 and renders the digest.
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut text = String::with_capacity(64);
        for byte in digest {
            text.push_str(&format!("{byte:02x}"));
        }
        Self(text)
    }

    /// The hex rendering.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for Hash {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl From<&str> for Hash {
    fn from(text: &str) -> Self {
        Self(text.to_string())
    }
}
