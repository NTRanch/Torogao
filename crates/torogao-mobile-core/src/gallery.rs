//! The local gallery: generated stills plus a JSON index, owned by
//! a directory the shell chooses (the phone's app data dir, the
//! desktop user's data dir). The index is the gallery — image files
//! without an index entry are not shown, and an index entry records
//! exactly which canonical request produced its image.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::client::GenError;

/// One gallery entry: a generated still and its provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GalleryEntry {
    /// File name inside the gallery directory.
    pub file: String,
    /// Cast member id (see [`crate::cast`]).
    pub character_id: String,
    /// The user scene prompt the still was generated with.
    pub prompt: String,
    /// The seed the still was generated with.
    pub seed: u64,
    /// Unix seconds when the still was filed.
    pub created_unix: u64,
}

/// The gallery rooted at one directory.
#[derive(Debug)]
pub struct Gallery {
    dir: PathBuf,
}

const INDEX_FILE: &str = "index.json";

impl Gallery {
    /// Open (creating if needed) the gallery at `dir`.
    pub fn open(dir: &Path) -> Result<Self, GenError> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    /// The gallery directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Full path of an entry's image file.
    pub fn path_of(&self, entry: &GalleryEntry) -> PathBuf {
        self.dir.join(&entry.file)
    }

    /// Read the index. A gallery with no index yet is empty; a
    /// corrupt index is an error, never a silent wipe.
    pub fn list(&self) -> Result<Vec<GalleryEntry>, GenError> {
        let path = self.dir.join(INDEX_FILE);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&path)?;
        let entries: Vec<GalleryEntry> = serde_json::from_slice(&bytes)
            .map_err(|e| GenError::Gallery(format!("corrupt gallery index: {e}")))?;
        Ok(entries)
    }

    /// File image bytes as a new entry and append it to the index.
    /// The file lands first; the index append is the commit point.
    pub fn add(
        &self,
        image_bytes: &[u8],
        character_id: &str,
        prompt: &str,
        seed: u64,
        created_unix: u64,
    ) -> Result<GalleryEntry, GenError> {
        let file = format!("tmg-{created_unix}-{seed}.png");
        fs::write(self.dir.join(&file), image_bytes)?;
        let entry = GalleryEntry {
            file,
            character_id: character_id.to_string(),
            prompt: prompt.to_string(),
            seed,
            created_unix,
        };
        let mut entries = self.list()?;
        entries.push(entry.clone());
        let index_bytes = serde_json::to_vec_pretty(&entries)
            .map_err(|e| GenError::Gallery(format!("cannot encode gallery index: {e}")))?;
        fs::write(self.dir.join(INDEX_FILE), index_bytes)?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("tmg-gallery-test-{tag}-{nanos}"))
    }

    #[test]
    fn add_then_list_round_trips() {
        let dir = scratch("roundtrip");
        let g = Gallery::open(&dir).expect("open");
        assert!(g.list().expect("empty list").is_empty());
        let e = g
            .add(b"png-bytes", "sirena", "city night", 99, 1_700_000_000)
            .expect("add");
        assert_eq!(e.character_id, "sirena");
        assert!(g.path_of(&e).exists());
        let listed = g.list().expect("list");
        assert_eq!(listed, vec![e]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_index_is_an_error_not_a_wipe() {
        let dir = scratch("corrupt");
        let g = Gallery::open(&dir).expect("open");
        fs::write(dir.join(INDEX_FILE), b"{not json").expect("plant corrupt index");
        assert!(g.list().is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
