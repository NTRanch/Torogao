//! On-device record storage: one JSON file per record under
//! `<data>/characters/`, exports as a single shareable JSON in
//! the exports directory. Plain files on purpose — the record
//! is the estate's interchange shape, and a file the owner can
//! copy off the phone beats a database they cannot.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::record::{CharacterRecord, RecordError};

/// Store failures.
#[derive(Debug)]
pub enum StoreError {
    /// Filesystem failure.
    Io(io::Error),
    /// A stored record failed to parse.
    Record(RecordError),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "store I/O failed: {e}"),
            StoreError::Record(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<RecordError> for StoreError {
    fn from(e: RecordError) -> Self {
        StoreError::Record(e)
    }
}

/// The record store rooted at the app's private data dir.
#[derive(Debug, Clone)]
pub struct LocalStore {
    characters_dir: PathBuf,
}

impl LocalStore {
    /// Open (creating if needed) the store under `data_dir`.
    pub fn open(data_dir: &Path) -> Result<Self, StoreError> {
        let characters_dir = data_dir.join("characters");
        fs::create_dir_all(&characters_dir)?;
        Ok(Self { characters_dir })
    }

    /// The directory records live in.
    pub fn characters_dir(&self) -> &Path {
        &self.characters_dir
    }

    /// All readable records, sorted by id, plus the names of
    /// any files that failed to parse (surfaced, never hidden).
    pub fn list_detailed(&self) -> (Vec<CharacterRecord>, Vec<String>) {
        let mut records = Vec::new();
        let mut unreadable = Vec::new();
        let Ok(entries) = fs::read_dir(&self.characters_dir) else {
            return (records, unreadable);
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match fs::read_to_string(&path)
                .map_err(StoreError::Io)
                .and_then(|text| CharacterRecord::from_json(&text).map_err(StoreError::Record))
            {
                Ok(rec) => records.push(rec),
                Err(_) => unreadable.push(name),
            }
        }
        records.sort_by(|a, b| a.id.cmp(&b.id));
        (records, unreadable)
    }

    /// All readable records, sorted by id.
    pub fn list(&self) -> Vec<CharacterRecord> {
        self.list_detailed().0
    }

    /// Load one record by id.
    pub fn load(&self, id: &str) -> Result<CharacterRecord, StoreError> {
        let text = fs::read_to_string(self.path_for(id))?;
        Ok(CharacterRecord::from_json(&text)?)
    }

    /// Persist a record (write-then-rename, so a kill mid-save
    /// leaves the previous revision intact).
    pub fn save(&self, record: &CharacterRecord) -> Result<PathBuf, StoreError> {
        let path = self.path_for(&record.id);
        let tmp = path.with_extension("json.tmp");
        fs::write(
            &tmp,
            record
                .to_json()
                .map_err(|e| StoreError::Record(RecordError::Parse(e.to_string())))?,
        )?;
        fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// Export a record as `<id>-character.json` into the
    /// shareable exports directory.
    pub fn export(
        &self,
        record: &CharacterRecord,
        exports_dir: &Path,
    ) -> Result<PathBuf, StoreError> {
        fs::create_dir_all(exports_dir)?;
        let path = exports_dir.join(format!("{}-character.json", record.id));
        fs::write(
            &path,
            record
                .to_json()
                .map_err(|e| StoreError::Record(RecordError::Parse(e.to_string())))?,
        )?;
        Ok(path)
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.characters_dir.join(format!("{id}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::prima_template;

    fn temp_store(tag: &str) -> (PathBuf, LocalStore) {
        let dir =
            std::env::temp_dir().join(format!("torogao-mobile-local-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = LocalStore::open(&dir).expect("open store");
        (dir, store)
    }

    #[test]
    fn save_list_load_round_trip() {
        let (dir, store) = temp_store("roundtrip");
        let rec = prima_template();
        store.save(&rec).expect("save");
        let listed = store.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], rec);
        assert_eq!(store.load("prima").expect("load"), rec);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_reported_not_fatal() {
        let (dir, store) = temp_store("corrupt");
        store.save(&prima_template()).expect("save");
        fs::write(dir.join("characters").join("broken.json"), "{not json").expect("plant");
        let (records, unreadable) = store.list_detailed();
        assert_eq!(records.len(), 1);
        assert_eq!(unreadable, ["broken.json"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_writes_a_parseable_record() {
        let (dir, store) = temp_store("export");
        let rec = prima_template();
        let exports = dir.join("exports");
        let path = store.export(&rec, &exports).expect("export");
        let text = fs::read_to_string(&path).expect("read export");
        assert_eq!(CharacterRecord::from_json(&text).expect("parse"), rec);
        let _ = fs::remove_dir_all(&dir);
    }
}
