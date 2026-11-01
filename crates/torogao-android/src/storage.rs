//! The phone storage seam: Addition S directories rooted
//! in app-private storage (AMENDMENT-ANDROID §3 Scope 4 /
//! §8).
//!
//! On desktop the Addition S base directories resolve
//! through XDG / `TOROGAO_*_PATH` overrides
//! (`torogao_cli::paths::base_dir`). The phone has no XDG;
//! it has one app-private internal storage directory. The
//! seam is therefore a single substitution: the shell takes
//! that root at entry, creates the `state/` subtree, and
//! passes the derived paths explicitly to the existing
//! persistence functions — the preference file, its
//! temp-then-rename write discipline, and its
//! write-on-change rule are the desktop code's, unchanged.
//! (The alternative — setting the `TOROGAO_STATE_PATH`
//! override from inside the process — mutates process env,
//! which edition 2024 makes an unsafe act; this crate
//! forbids unsafe, so paths travel as values instead.)
//! Nothing in the engines knows the phone exists.

use std::path::PathBuf;

use torogao_app::{AppError, AppResult};

/// Subdirectory (under the storage root) holding the
/// Addition S state kind: the persona preference and the
/// Settings area's saved sessions.
pub const STATE_SUBDIR: &str = "state";
/// Subdirectory (under the state dir) holding the Settings
/// area's named session saves.
pub const SAVES_SUBDIR: &str = "settings-saves";
/// Subdirectory (under the storage root) holding this
/// stage's gate evidence files (boot log, determinism and
/// performance records). Evidence is not app state; it is
/// kept apart so a state wipe never eats a gate record.
pub const EVIDENCE_SUBDIR: &str = "evidence";

/// The shell's storage root and its derived directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Storage {
    /// The app-private root (Android internal data path;
    /// an explicit directory in host tests).
    root: PathBuf,
}

impl Storage {
    /// Roots storage at `root`. The directory need not
    /// exist yet; `create_dirs` makes the subtree.
    pub fn new(root: PathBuf) -> Storage {
        Storage { root }
    }

    /// The app-private root itself.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// The Addition S state directory.
    pub fn state_dir(&self) -> PathBuf {
        self.root.join(STATE_SUBDIR)
    }

    /// The Settings area's saved-session directory.
    pub fn saves_dir(&self) -> PathBuf {
        self.state_dir().join(SAVES_SUBDIR)
    }

    /// The gate-evidence directory.
    pub fn evidence_dir(&self) -> PathBuf {
        self.root.join(EVIDENCE_SUBDIR)
    }

    /// The persona preference file — the same leaf name the
    /// desktop shell's `persona::preference_path` resolves
    /// to under its state dir.
    pub fn preference_file(&self) -> PathBuf {
        self.state_dir().join("persona-preference.json")
    }

    /// Creates the storage subtree (idempotent).
    pub fn create_dirs(&self) -> AppResult<()> {
        for dir in [self.state_dir(), self.saves_dir(), self.evidence_dir()] {
            std::fs::create_dir_all(&dir).map_err(|err| AppError::Io {
                context: "create storage directory",
                message: err.to_string(),
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Storage;
    use std::path::PathBuf;

    /// The derived directories hang off the root in the
    /// documented shape, and the preference leaf matches
    /// the desktop shell's file name.
    #[test]
    fn derived_dirs_follow_the_addition_s_shape() {
        let storage = Storage::new(PathBuf::from("/data/data/ai.ntranch.torogao/files"));
        assert_eq!(
            storage.state_dir(),
            PathBuf::from("/data/data/ai.ntranch.torogao/files/state")
        );
        assert_eq!(
            storage.saves_dir(),
            PathBuf::from("/data/data/ai.ntranch.torogao/files/state/settings-saves")
        );
        assert_eq!(
            storage.preference_file(),
            PathBuf::from("/data/data/ai.ntranch.torogao/files/state/persona-preference.json")
        );
        assert_eq!(
            storage.evidence_dir(),
            PathBuf::from("/data/data/ai.ntranch.torogao/files/evidence")
        );
    }

    /// Creating the subtree is idempotent and lands every
    /// directory the shell writes to.
    #[test]
    fn create_dirs_is_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "torogao-android-storage-test-{}",
            std::process::id()
        ));
        let storage = Storage::new(root.clone());
        storage.create_dirs().expect("first create");
        storage.create_dirs().expect("second create");
        assert!(storage.state_dir().is_dir());
        assert!(storage.saves_dir().is_dir());
        assert!(storage.evidence_dir().is_dir());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
