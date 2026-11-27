//! The session: the one object both shells drive.
//!
//! A session owns the data directory's settings, gallery, and
//! generation queue, and keeps them consistent: every settings
//! change is persisted and pushed into the queue's context, so
//! the age gate, the active seat, and the recipe the shells show
//! are exactly what generation runs under. Shells hold no
//! behaviour of their own beyond rendering this state.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::client::{GenError, SeatClient, SeatStatus, now_unix};
use crate::gallery::Gallery;
use crate::queue::{GenQueue, Job, JobContext, JobRequest};
use crate::settings::{SeatConfig, Settings};
use crate::style::RecipeSettings;

/// An opened app session over one data directory.
pub struct Session {
    dir: PathBuf,
    settings: Mutex<Settings>,
    gallery: Gallery,
    queue: GenQueue,
}

impl Session {
    /// Open the session rooted at `dir` (settings + gallery live
    /// there). On first run the settings name
    /// `default_seat_url` / `default_seat_name` as the only seat.
    pub fn open(
        dir: &Path,
        default_seat_url: &str,
        default_seat_name: &str,
    ) -> Result<Self, GenError> {
        let settings = Settings::load(dir, default_seat_url, default_seat_name)?;
        let gallery = Gallery::open(&dir.join("gallery"))?;
        let context = JobContext {
            seat_url: settings
                .active_seat()
                .map(|s| s.base_url.clone())
                .unwrap_or_default(),
            seat_token: settings.active_seat().and_then(|s| s.token.clone()),
            recipe: settings.recipe,
            attested: settings.is_attested(),
        };
        let queue = GenQueue::start(Gallery::open(&dir.join("gallery"))?, context);
        Ok(Self {
            dir: dir.to_path_buf(),
            settings: Mutex::new(settings),
            gallery,
            queue,
        })
    }

    /// The data directory this session owns.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A snapshot of the current settings.
    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| Settings::first_run("", ""))
    }

    /// Whether the 18+ attestation stands.
    pub fn is_attested(&self) -> bool {
        self.settings().is_attested()
    }

    /// Record the 18+ attestation now and persist it.
    pub fn attest(&self) -> Result<(), GenError> {
        self.mutate(|s| s.attest(now_unix()))
    }

    /// Add or replace a saved seat (by name) and persist.
    pub fn upsert_seat(&self, seat: SeatConfig) -> Result<usize, GenError> {
        let mut index = 0;
        self.mutate(|s| {
            index = s.upsert_seat(seat);
        })?;
        Ok(index)
    }

    /// Make the seat at `index` active and persist. Refused
    /// (error) for an out-of-range index.
    pub fn set_active_seat(&self, index: usize) -> Result<(), GenError> {
        let ok = self.mutate(|s| s.set_active_seat(index))?;
        if ok {
            Ok(())
        } else {
            Err(GenError::BadRequest(format!("no seat at index {index}")))
        }
    }

    /// Replace the recipe settings and persist. Out-of-bounds
    /// values are refused with the bounds named.
    pub fn set_recipe(&self, recipe: RecipeSettings) -> Result<(), GenError> {
        recipe.validate().map_err(GenError::BadRequest)?;
        self.mutate(|s| {
            s.recipe = recipe;
            true
        })?;
        Ok(())
    }

    /// Ask the active seat's facade for its state.
    pub fn seat_status(&self) -> SeatStatus {
        match self.settings().active_seat() {
            Some(seat) => SeatClient::with_token(&seat.base_url, seat.token.clone()).health(),
            None => SeatStatus::Unreachable("no seat is configured".to_string()),
        }
    }

    /// Ask the active seat's facade to wake its engine.
    pub fn wake_seat(&self) -> SeatStatus {
        match self.settings().active_seat() {
            Some(seat) => SeatClient::with_token(&seat.base_url, seat.token.clone()).wake(),
            None => SeatStatus::Unreachable("no seat is configured".to_string()),
        }
    }

    /// Queue one generation. The age gate and the content
    /// screen are enforced by the queue.
    pub fn enqueue(
        &self,
        character_id: &str,
        prompt: &str,
        seed: Option<u64>,
    ) -> Result<u64, GenError> {
        self.queue.enqueue(JobRequest {
            character_id: character_id.to_string(),
            prompt: prompt.to_string(),
            seed,
        })
    }

    /// All jobs, oldest first.
    pub fn jobs(&self) -> Vec<Job> {
        self.queue.jobs()
    }

    /// Cancel a queued or running job.
    pub fn cancel(&self, id: u64) -> bool {
        self.queue.cancel(id)
    }

    /// The gallery (view, delete, export).
    pub fn gallery(&self) -> &Gallery {
        &self.gallery
    }

    /// Apply a settings mutation, persist, and push the new
    /// queue context. The mutation returns a value the caller
    /// may need; persistence failure is an error.
    fn mutate<T>(&self, f: impl FnOnce(&mut Settings) -> T) -> Result<T, GenError> {
        let mut guard = self
            .settings
            .lock()
            .map_err(|_| GenError::Local("settings lock poisoned".to_string()))?;
        let out = f(&mut guard);
        guard.save(&self.dir)?;
        self.queue.set_context(JobContext {
            seat_url: guard
                .active_seat()
                .map(|s| s.base_url.clone())
                .unwrap_or_default(),
            seat_token: guard.active_seat().and_then(|s| s.token.clone()),
            recipe: guard.recipe,
            attested: guard.is_attested(),
        });
        Ok(out)
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
        std::env::temp_dir().join(format!("tmg-session-test-{tag}-{nanos}"))
    }

    #[test]
    fn session_opens_unattested_and_the_gate_holds_until_attest() {
        let dir = scratch("gate");
        let session = Session::open(&dir, "http://127.0.0.1:9", "Dead seat").expect("open");
        assert!(!session.is_attested());
        let err = session
            .enqueue("prima", "rain", None)
            .expect_err("gate must hold");
        assert!(matches!(err, GenError::AgeGate), "got {err}");
        session.attest().expect("attest");
        assert!(session.is_attested());
        // Attested now: enqueue succeeds (the job then fails at
        // the dead seat, which is the seat's business, not the
        // gate's).
        let id = session.enqueue("prima", "rain", Some(1)).expect("enqueue");
        assert!(id >= 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn attestation_persists_across_sessions() {
        let dir = scratch("persist");
        {
            let session = Session::open(&dir, "http://127.0.0.1:9", "Dead seat").expect("open");
            session.attest().expect("attest");
        }
        let session = Session::open(&dir, "http://127.0.0.1:9", "Dead seat").expect("reopen");
        assert!(session.is_attested());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recipe_bounds_are_enforced_at_the_session_door() {
        let dir = scratch("recipe");
        let session = Session::open(&dir, "http://127.0.0.1:9", "Dead seat").expect("open");
        let bad = RecipeSettings {
            steps: 500,
            cfg: 6.0,
            width: 832,
            height: 1344,
        };
        assert!(session.set_recipe(bad).is_err());
        let good = RecipeSettings {
            steps: 32,
            cfg: 6.5,
            width: 832,
            height: 1344,
        };
        session.set_recipe(good).expect("set");
        assert_eq!(session.settings().recipe, good);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seat_switching_is_persisted_and_range_checked() {
        let dir = scratch("seats");
        let session = Session::open(&dir, "http://127.0.0.1:9", "Dead seat").expect("open");
        let i = session
            .upsert_seat(SeatConfig {
                name: "Other".to_string(),
                base_url: "http://127.0.0.1:10".to_string(),
                token: None,
            })
            .expect("upsert");
        session.set_active_seat(i).expect("activate");
        assert_eq!(
            session.settings().active_seat().map(|s| s.name.as_str()),
            Some("Other")
        );
        assert!(session.set_active_seat(99).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seat_status_without_a_seat_is_unreachable_not_a_panic() {
        let status = SeatStatus::Unreachable("no seat is configured".to_string());
        assert!(status.describe().contains("No seat answered"));
    }
}
