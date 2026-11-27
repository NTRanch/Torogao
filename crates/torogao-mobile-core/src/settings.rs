//! Persisted app settings.
//!
//! One JSON document in the app's data directory, owned by the
//! core so both shells read and write the same state: the 18+
//! attestation, the saved seats, and the recipe settings. Like
//! the gallery index, a corrupt settings file is an error, never
//! a silent reset — a reset would quietly drop the attestation
//! record and the user's seats.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::client::GenError;
use crate::style::RecipeSettings;

/// One saved generation seat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeatConfig {
    /// Display name ("Primo (tailnet)", "Home server", …).
    pub name: String,
    /// Facade base URL, no trailing slash needed.
    pub base_url: String,
}

/// The persisted settings document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Unix seconds of the user's 18+ attestation, if given.
    /// Generation is gated on this being present.
    pub age_attested_unix: Option<u64>,
    /// Saved seats; the app talks to the facade at the seat URL.
    pub seats: Vec<SeatConfig>,
    /// Index into `seats` of the active seat.
    pub active_seat: usize,
    /// The user's recipe settings (bounded by the recipe of
    /// record; see [`RecipeSettings::validate`]).
    pub recipe: RecipeSettings,
}

const SETTINGS_FILE: &str = "settings.json";

impl Settings {
    /// Settings for a first run pointing at `default_seat_url`.
    pub fn first_run(default_seat_url: &str, default_seat_name: &str) -> Self {
        Self {
            age_attested_unix: None,
            seats: vec![SeatConfig {
                name: default_seat_name.to_string(),
                base_url: default_seat_url.to_string(),
            }],
            active_seat: 0,
            recipe: RecipeSettings::of_record(),
        }
    }

    /// Load settings from `dir`, or first-run settings naming
    /// `default_seat_url` when no file exists yet. A corrupt
    /// file is an error.
    pub fn load(
        dir: &Path,
        default_seat_url: &str,
        default_seat_name: &str,
    ) -> Result<Self, GenError> {
        let path = dir.join(SETTINGS_FILE);
        if !path.exists() {
            return Ok(Self::first_run(default_seat_url, default_seat_name));
        }
        let bytes = fs::read(&path)?;
        let settings: Settings = serde_json::from_slice(&bytes)
            .map_err(|e| GenError::Local(format!("corrupt settings file: {e}")))?;
        Ok(settings)
    }

    /// Persist the settings into `dir` (created if needed).
    pub fn save(&self, dir: &Path) -> Result<(), GenError> {
        fs::create_dir_all(dir)?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| GenError::Local(format!("cannot encode settings: {e}")))?;
        fs::write(dir.join(SETTINGS_FILE), bytes)?;
        Ok(())
    }

    /// The settings file path inside `dir`.
    pub fn file_in(dir: &Path) -> PathBuf {
        dir.join(SETTINGS_FILE)
    }

    /// Whether the 18+ attestation has been given.
    pub fn is_attested(&self) -> bool {
        self.age_attested_unix.is_some()
    }

    /// Record the 18+ attestation at `unix_now`.
    pub fn attest(&mut self, unix_now: u64) {
        self.age_attested_unix = Some(unix_now);
    }

    /// The active seat, if the index is in range.
    pub fn active_seat(&self) -> Option<&SeatConfig> {
        self.seats.get(self.active_seat)
    }

    /// Add or replace a seat by name; returns its index. The
    /// first seat added becomes active if none was.
    pub fn upsert_seat(&mut self, seat: SeatConfig) -> usize {
        if let Some(i) = self.seats.iter().position(|s| s.name == seat.name) {
            self.seats[i] = seat;
            i
        } else {
            self.seats.push(seat);
            self.seats.len() - 1
        }
    }

    /// Make the seat at `index` active. Out-of-range indices are
    /// refused (`false`), never clamped silently.
    pub fn set_active_seat(&mut self, index: usize) -> bool {
        if index < self.seats.len() {
            self.active_seat = index;
            true
        } else {
            false
        }
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
        std::env::temp_dir().join(format!("tmg-settings-test-{tag}-{nanos}"))
    }

    #[test]
    fn first_run_is_unattested_with_the_default_seat() {
        let s = Settings::first_run("http://100.107.128.71:8190", "Primo (tailnet)");
        assert!(!s.is_attested());
        assert_eq!(
            s.active_seat().map(|x| x.base_url.as_str()),
            Some("http://100.107.128.71:8190")
        );
        assert_eq!(s.recipe, RecipeSettings::of_record());
    }

    #[test]
    fn attestation_and_seats_survive_save_load() {
        let dir = scratch("roundtrip");
        let mut s = Settings::first_run("http://a:8190", "A");
        s.attest(1_700_000_000);
        let i = s.upsert_seat(SeatConfig {
            name: "B".to_string(),
            base_url: "http://b:8190".to_string(),
        });
        assert!(s.set_active_seat(i));
        s.save(&dir).expect("save");
        let loaded = Settings::load(&dir, "http://unused:1", "unused").expect("load");
        assert_eq!(loaded, s);
        assert!(loaded.is_attested());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_loads_first_run() {
        let dir = scratch("missing");
        let s = Settings::load(&dir, "http://a:8190", "A").expect("load");
        assert!(!s.is_attested());
    }

    #[test]
    fn corrupt_settings_are_an_error_not_a_reset() {
        let dir = scratch("corrupt");
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(SETTINGS_FILE), b"{nope").expect("plant");
        assert!(Settings::load(&dir, "http://a:8190", "A").is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn out_of_range_active_seat_is_refused() {
        let mut s = Settings::first_run("http://a:8190", "A");
        assert!(!s.set_active_seat(7));
        assert_eq!(s.active_seat, 0);
    }

    #[test]
    fn upsert_replaces_by_name() {
        let mut s = Settings::first_run("http://a:8190", "A");
        let i = s.upsert_seat(SeatConfig {
            name: "A".to_string(),
            base_url: "http://a2:8190".to_string(),
        });
        assert_eq!(i, 0);
        assert_eq!(s.seats.len(), 1);
        assert_eq!(s.seats[0].base_url, "http://a2:8190");
    }
}
