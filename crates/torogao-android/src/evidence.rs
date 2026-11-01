//! Gate evidence written from inside the app: an
//! append-only boot log and small JSON records in the
//! storage root's evidence directory.
//!
//! The gate harness pulls these files out of app-private
//! storage with `adb run-as` (the APK is debug-signed, so
//! its private directory is readable to the shell user).
//! Writing from the app's first instruction matters: a
//! crash anywhere later still leaves the boot log's last
//! line naming how far startup got. Every write is
//! best-effort-with-a-record: a failed evidence write is
//! itself logged to stderr (which Android routes to
//! logcat), never silently dropped, and never allowed to
//! take the shell down — evidence serves the app, not the
//! other way round.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The boot log's file name inside the evidence directory.
pub const BOOT_LOG_NAME: &str = "a1-boot.log";
/// The determinism record's file name.
pub const DETERMINISM_NAME: &str = "determinism.json";
/// The performance smoke record's file name.
pub const PERF_NAME: &str = "perf-smoke.json";
/// The phone audit record's file name.
pub const AUDIT_NAME: &str = "phone-audit.json";
/// The Settings walk record's file name (the shell writes
/// one line per Settings action outcome as the walk runs).
pub const SETTINGS_WALK_NAME: &str = "settings-walk.log";

/// An append-only line log in the evidence directory.
#[derive(Debug, Clone)]
pub struct EvidenceLog {
    /// The log file's path.
    path: PathBuf,
}

impl EvidenceLog {
    /// Opens (creating if needed) the named log under
    /// `evidence_dir`.
    pub fn new(evidence_dir: &Path, name: &str) -> EvidenceLog {
        EvidenceLog {
            path: evidence_dir.join(name),
        }
    }

    /// The log file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line and flushes. Failures go to stderr
    /// with their reason; the caller never branches on them.
    pub fn line(&self, message: &str) {
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(mut file) => {
                if let Err(err) = writeln!(file, "{message}") {
                    eprintln!("torogao-android: evidence write failed: {err}");
                }
            }
            Err(err) => {
                eprintln!("torogao-android: evidence open failed: {err}");
            }
        }
    }
}

/// Writes a JSON value as a whole file: sibling temp file,
/// then rename — the save module's discipline, so a crash
/// never leaves a torn gate record.
pub fn write_json_atomic(path: &Path, value: &serde_json::Value) {
    let text = match serde_json::to_string_pretty(value) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("torogao-android: evidence serialize failed: {err}");
            return;
        }
    };
    let temp = path.with_extension("json.partial");
    if let Err(err) = std::fs::write(&temp, text) {
        eprintln!("torogao-android: evidence write failed: {err}");
        return;
    }
    if let Err(err) = std::fs::rename(&temp, path) {
        eprintln!("torogao-android: evidence rename failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::{EvidenceLog, write_json_atomic};

    /// The boot log accumulates lines in order, and the
    /// atomic JSON writer lands a complete parseable file.
    #[test]
    fn log_appends_and_json_lands_whole() {
        let dir = std::env::temp_dir().join(format!(
            "torogao-android-evidence-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let log = EvidenceLog::new(&dir, "a1-boot.log");
        log.line("first");
        log.line("second");
        let text = std::fs::read_to_string(dir.join("a1-boot.log")).expect("log readable");
        assert_eq!(text, "first\nsecond\n");
        let target = dir.join("record.json");
        write_json_atomic(&target, &serde_json::json!({"hash": 42}));
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&target).expect("json readable"))
                .expect("json parses");
        assert_eq!(parsed["hash"], 42);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
