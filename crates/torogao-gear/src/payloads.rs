//! Payload lookup: the validator and the pack writer resolve
//! part references against a payload store (§2.7 step 6), and
//! content hashes are sha256 over the payload bytes.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

/// The lowercase hex sha256 of a byte string — the content
/// hash form gear documents declare (§2.2).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// A read-only payload store: part name → payload bytes.
pub trait PayloadLookup {
    /// Returns the payload bytes for a name, or `None` when
    /// the store does not carry it.
    fn payload(&self, name: &str) -> Option<Vec<u8>>;
}

/// An in-memory payload store (tests, worked examples, and
/// the Creator preview all resolve against one of these).
#[derive(Clone, Debug, Default)]
pub struct MemoryPayloads {
    map: BTreeMap<String, Vec<u8>>,
}

impl MemoryPayloads {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts (or replaces) one payload.
    pub fn insert(&mut self, name: &str, bytes: Vec<u8>) {
        self.map.insert(name.to_string(), bytes);
    }

    /// Number of payloads carried.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// All payload names, sorted (BTreeMap order).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(|s| s.as_str())
    }
}

impl PayloadLookup for MemoryPayloads {
    fn payload(&self, name: &str) -> Option<Vec<u8>> {
        self.map.get(name).cloned()
    }
}
