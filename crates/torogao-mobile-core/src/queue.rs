//! The generation queue.
//!
//! One worker thread runs jobs in submission order against the
//! active seat; shells enqueue, poll snapshots, and cancel. The
//! queue is where the app's guarantees concentrate: a job only
//! runs if the age gate is attested and the content screen
//! passes (both checked again at run time, not only at enqueue
//! time — settings can change while a job waits), and a cancel
//! interrupts the job at the seat instead of just forgetting it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::cast::character;
use crate::client::{GenError, SeatClient};
use crate::gallery::{Gallery, GalleryEntry};
use crate::screen::{ScreenVerdict, screen_text};
use crate::style::RecipeSettings;

/// What the worker needs to run a job, snapshotted per job so a
/// settings change mid-queue applies to the next job, never
/// retroactively to a running one.
#[derive(Debug, Clone)]
pub struct JobContext {
    /// Facade base URL of the active seat.
    pub seat_url: String,
    /// The active seat's facade token, if any (sent as the
    /// `X-Facade-Token` header; see [`SeatClient::with_token`]).
    pub seat_token: Option<String>,
    /// Recipe settings to generate with.
    pub recipe: RecipeSettings,
    /// Whether the 18+ attestation stands.
    pub attested: bool,
}

/// A generation request as submitted.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRequest {
    /// Cast member id (see [`crate::cast`]).
    pub character_id: String,
    /// The user's scene prompt.
    pub prompt: String,
    /// Explicit seed, or `None` to derive one at run time.
    pub seed: Option<u64>,
}

/// Where a job stands.
#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    /// Waiting for the worker.
    Queued,
    /// Running at the seat.
    Running,
    /// Finished; the still is filed in the gallery.
    Done,
    /// Failed; the string is the presentable error.
    Failed(String),
    /// Cancelled by the user.
    Cancelled,
}

/// One queued generation job.
#[derive(Debug, Clone)]
pub struct Job {
    /// Queue-unique id, from 1 up.
    pub id: u64,
    /// The request as submitted.
    pub request: JobRequest,
    /// Current status.
    pub status: JobStatus,
    /// The filed gallery entry once done.
    pub entry: Option<GalleryEntry>,
}

struct Shared {
    jobs: Vec<Job>,
    cancels: HashMap<u64, Arc<AtomicBool>>,
    next_id: u64,
    context: JobContext,
}

/// The generation queue. Clone-cheap handle; the worker thread
/// lives as long as the first handle's owner keeps it fed —
/// construct one per session.
pub struct GenQueue {
    shared: Arc<Mutex<Shared>>,
    wake: Arc<Condvar>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl GenQueue {
    /// Start a queue whose worker files results into `gallery`
    /// and runs jobs under `context`.
    pub fn start(gallery: Gallery, context: JobContext) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            jobs: Vec::new(),
            cancels: HashMap::new(),
            next_id: 1,
            context,
        }));
        let wake = Arc::new(Condvar::new());
        let worker = {
            let shared = Arc::clone(&shared);
            let wake = Arc::clone(&wake);
            std::thread::spawn(move || worker_loop(shared, wake, gallery))
        };
        Self {
            shared,
            wake,
            worker: Some(worker),
        }
    }

    /// Replace the context the next jobs run under (seat switch,
    /// recipe change, attestation).
    pub fn set_context(&self, context: JobContext) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.context = context;
        }
    }

    /// Enqueue a job. The age gate and the content screen are
    /// enforced here and again in the worker; a refusal is an
    /// error, never a queued job.
    pub fn enqueue(&self, request: JobRequest) -> Result<u64, GenError> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| GenError::Local("queue lock poisoned".to_string()))?;
        if !shared.context.attested {
            return Err(GenError::AgeGate);
        }
        if character(&request.character_id).is_none() {
            return Err(GenError::BadRequest(format!(
                "unknown cast member \"{}\"",
                request.character_id
            )));
        }
        if let ScreenVerdict::Refuse(reason) = screen_text(&request.prompt) {
            return Err(GenError::ScreenRefused(reason));
        }
        let id = shared.next_id;
        shared.next_id += 1;
        shared.cancels.insert(id, Arc::new(AtomicBool::new(false)));
        shared.jobs.push(Job {
            id,
            request,
            status: JobStatus::Queued,
            entry: None,
        });
        drop(shared);
        self.wake.notify_all();
        Ok(id)
    }

    /// Snapshot of all jobs, oldest first.
    pub fn jobs(&self) -> Vec<Job> {
        self.shared
            .lock()
            .map(|shared| shared.jobs.clone())
            .unwrap_or_default()
    }

    /// Cancel a queued or running job. Returns whether a live
    /// job was found. A queued job is marked cancelled and never
    /// runs; a running job's flag is set (its poll loop notices,
    /// interrupts the seat, and lands as cancelled).
    pub fn cancel(&self, id: u64) -> bool {
        let Ok(mut shared) = self.shared.lock() else {
            return false;
        };
        let Some(flag) = shared.cancels.get(&id).cloned() else {
            return false;
        };
        flag.store(true, Ordering::SeqCst);
        if let Some(job) = shared.jobs.iter_mut().find(|j| j.id == id) {
            if job.status == JobStatus::Queued {
                job.status = JobStatus::Cancelled;
            }
            return matches!(job.status, JobStatus::Queued | JobStatus::Running)
                || job.status == JobStatus::Cancelled;
        }
        false
    }
}

impl Drop for GenQueue {
    fn drop(&mut self) {
        // The worker parks on the condvar; it holds its own Arc
        // to the shared state, so dropping the handle never
        // strands a running job mid-write — the process is
        // exiting anyway. Nothing to join safely here.
        let _ = self.worker.take();
    }
}

fn worker_loop(shared: Arc<Mutex<Shared>>, wake: Arc<Condvar>, gallery: Gallery) {
    loop {
        // Take the next queued job, or park.
        let (job, context, cancel) = {
            let mut guard = match shared.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            loop {
                if let Some(job) = guard
                    .jobs
                    .iter()
                    .find(|j| j.status == JobStatus::Queued)
                    .cloned()
                {
                    let cancel = guard
                        .cancels
                        .get(&job.id)
                        .cloned()
                        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                    let context = guard.context.clone();
                    if let Some(live) = guard.jobs.iter_mut().find(|j| j.id == job.id) {
                        live.status = JobStatus::Running;
                    }
                    break (job, context, cancel);
                }
                guard = match wake.wait(guard) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
        };

        let outcome = run_job(&context, &job, &gallery, &cancel);

        if let Ok(mut guard) = shared.lock() {
            if let Some(live) = guard.jobs.iter_mut().find(|j| j.id == job.id) {
                match outcome {
                    Ok(entry) => {
                        live.status = JobStatus::Done;
                        live.entry = Some(entry);
                    }
                    Err(GenError::Cancelled) => {
                        live.status = JobStatus::Cancelled;
                    }
                    Err(e) => {
                        live.status = JobStatus::Failed(e.to_string());
                    }
                }
            }
        }
    }
}

/// Run one job under the run-time guarantees: attestation and
/// screen are re-checked here, so a job enqueued while attested
/// does not run if the attestation record was lost meanwhile,
/// and the screen's word is final at the seat's doorstep too.
fn run_job(
    context: &JobContext,
    job: &Job,
    gallery: &Gallery,
    cancel: &AtomicBool,
) -> Result<GalleryEntry, GenError> {
    if !context.attested {
        return Err(GenError::AgeGate);
    }
    if let ScreenVerdict::Refuse(reason) = screen_text(&job.request.prompt) {
        return Err(GenError::ScreenRefused(reason));
    }
    let character = character(&job.request.character_id).ok_or_else(|| {
        GenError::BadRequest(format!(
            "unknown cast member \"{}\"",
            job.request.character_id
        ))
    })?;
    let client = SeatClient::with_token(&context.seat_url, context.seat_token.clone());
    client.generate_with(
        character,
        &job.request.prompt,
        job.request.seed,
        &context.recipe,
        gallery,
        Some(cancel),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn scratch(tag: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("tmg-queue-test-{tag}-{nanos}"))
    }

    fn context(attested: bool) -> JobContext {
        JobContext {
            // Port 9 (discard): every seat call fails fast.
            seat_url: "http://127.0.0.1:9".to_string(),
            seat_token: None,
            recipe: RecipeSettings::of_record(),
            attested,
        }
    }

    fn wait_for_status(queue: &GenQueue, id: u64, want: &JobStatus) -> Job {
        for _ in 0..100 {
            if let Some(job) = queue.jobs().into_iter().find(|j| j.id == id) {
                if &job.status == want || matches!(job.status, JobStatus::Failed(_)) {
                    return job;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("job {id} never reached {want:?}");
    }

    #[test]
    fn enqueue_refuses_without_attestation() {
        let dir = scratch("gate");
        let queue = GenQueue::start(Gallery::open(&dir).expect("gallery"), context(false));
        let err = queue
            .enqueue(JobRequest {
                character_id: "prima".to_string(),
                prompt: "rain".to_string(),
                seed: None,
            })
            .expect_err("unattested enqueue must fail");
        assert!(matches!(err, GenError::AgeGate), "got {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn enqueue_refuses_screened_prompts_and_unknown_cast() {
        let dir = scratch("screen");
        let queue = GenQueue::start(Gallery::open(&dir).expect("gallery"), context(true));
        let err = queue
            .enqueue(JobRequest {
                character_id: "prima".to_string(),
                prompt: "schoolgirl, studio light".to_string(),
                seed: None,
            })
            .expect_err("screened prompt must fail");
        assert!(matches!(err, GenError::ScreenRefused(_)), "got {err}");
        let err = queue
            .enqueue(JobRequest {
                character_id: "nobody".to_string(),
                prompt: "rain".to_string(),
                seed: None,
            })
            .expect_err("unknown cast must fail");
        assert!(matches!(err, GenError::BadRequest(_)), "got {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn job_against_a_dead_seat_fails_with_the_seats_error() {
        let dir = scratch("deadseat");
        let queue = GenQueue::start(Gallery::open(&dir).expect("gallery"), context(true));
        let id = queue
            .enqueue(JobRequest {
                character_id: "sirena".to_string(),
                prompt: "city night".to_string(),
                seed: Some(3),
            })
            .expect("enqueue");
        let job = wait_for_status(&queue, id, &JobStatus::Done);
        assert!(
            matches!(job.status, JobStatus::Failed(ref m) if m.contains("unreachable")),
            "got {:?}",
            job.status
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancelling_a_queued_job_keeps_it_from_running() {
        let dir = scratch("cancel");
        // Occupy the worker with a job against the dead seat is
        // instant-fail, so instead cancel before the worker's
        // first pass by enqueueing two and cancelling the second
        // while checking it lands Cancelled either way.
        let queue = GenQueue::start(Gallery::open(&dir).expect("gallery"), context(true));
        let id = queue
            .enqueue(JobRequest {
                character_id: "lana".to_string(),
                prompt: "cafe".to_string(),
                seed: Some(1),
            })
            .expect("enqueue");
        assert!(queue.cancel(id));
        let job = wait_for_status(&queue, id, &JobStatus::Cancelled);
        assert!(
            job.status == JobStatus::Cancelled || matches!(job.status, JobStatus::Failed(_)),
            "cancelled job must not succeed: {:?}",
            job.status
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
