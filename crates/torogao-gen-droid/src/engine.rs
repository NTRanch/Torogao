//! The submit → poll → fetch engine: a single worker thread owns
//! the backend (native diffusion contexts are not thread-safe), a
//! bounded queue feeds it, and job state lives in a shared table.
//! This mirrors the seat client's semantics — submission is cheap
//! and non-blocking, progress is polled, results are fetched once.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use crate::params::{DroidError, GenParams, GeneratedImage, RawImage};

/// A job handle. Ids are issued monotonically from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(pub u64);

/// Where a job stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    /// Accepted, waiting for the worker.
    Queued,
    /// Sampling: `step` of `steps` denoiser steps reported so far.
    Running {
        /// Current step (0 before the first report).
        step: u32,
        /// Total steps for this job.
        steps: u32,
    },
    /// Finished; [`Engine::fetch`] will return the still.
    Done,
    /// Failed; carries the backend's reason. Fetching a failed job
    /// returns this as an error and consumes the job.
    Failed(String),
}

/// The pluggable inference backend. One backend instance is owned
/// by the engine's worker thread; `generate` blocks until the still
/// is decoded or the run fails.
pub trait Backend: Send {
    /// Run one generation. `progress` is called with
    /// (current_step, total_steps) as the sampler advances.
    fn generate(
        &mut self,
        params: &GenParams,
        progress: &mut dyn FnMut(u32, u32),
    ) -> Result<RawImage, DroidError>;
}

struct JobRecord {
    state: JobState,
    result: Option<Result<GeneratedImage, DroidError>>,
}

struct Shared {
    jobs: Mutex<HashMap<JobId, JobRecord>>,
}

enum WorkerMsg {
    Run(JobId, GenParams),
}

/// An engine instance over one backend. Cheap to share by reference
/// between the shell's submit and poll/fetch call sites.
pub struct Engine {
    shared: Arc<Shared>,
    tx: mpsc::Sender<WorkerMsg>,
    next_id: AtomicU64,
    worker: Option<JoinHandle<()>>,
}

impl Engine {
    /// Start an engine over `backend`, spawning its worker thread.
    pub fn new<B: Backend + 'static>(backend: B) -> Engine {
        let shared = Arc::new(Shared {
            jobs: Mutex::new(HashMap::new()),
        });
        let (tx, rx) = mpsc::channel::<WorkerMsg>();
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::spawn(move || {
            let mut backend = backend;
            while let Ok(WorkerMsg::Run(id, params)) = rx.recv() {
                {
                    let mut jobs = worker_shared.jobs.lock().expect("job table poisoned");
                    if let Some(rec) = jobs.get_mut(&id) {
                        rec.state = JobState::Running {
                            step: 0,
                            steps: params.steps,
                        };
                    }
                }
                let progress_shared = Arc::clone(&worker_shared);
                let mut progress = move |step: u32, steps: u32| {
                    let mut jobs = progress_shared.jobs.lock().expect("job table poisoned");
                    if let Some(rec) = jobs.get_mut(&id) {
                        rec.state = JobState::Running { step, steps };
                    }
                };
                let outcome = backend
                    .generate(&params, &mut progress)
                    .and_then(|raw| encode_png(&raw));
                let mut jobs = worker_shared.jobs.lock().expect("job table poisoned");
                if let Some(rec) = jobs.get_mut(&id) {
                    match &outcome {
                        Ok(_) => rec.state = JobState::Done,
                        Err(e) => rec.state = JobState::Failed(e.to_string()),
                    }
                    rec.result = Some(outcome);
                }
            }
        });
        Engine {
            shared,
            tx,
            next_id: AtomicU64::new(1),
            worker: Some(worker),
        }
    }

    /// Queue a generation. Validates the seam's bounds first; a
    /// refused request never reaches the backend.
    pub fn submit(&self, params: GenParams) -> Result<JobId, DroidError> {
        params.validate()?;
        let id = JobId(self.next_id.fetch_add(1, Ordering::Relaxed));
        {
            let mut jobs = self.shared.jobs.lock().expect("job table poisoned");
            jobs.insert(
                id,
                JobRecord {
                    state: JobState::Queued,
                    result: None,
                },
            );
        }
        self.tx
            .send(WorkerMsg::Run(id, params))
            .map_err(|_| DroidError::EngineUnavailable("worker thread has exited".to_owned()))?;
        Ok(id)
    }

    /// Current state of a job.
    pub fn poll(&self, id: JobId) -> Result<JobState, DroidError> {
        let jobs = self.shared.jobs.lock().expect("job table poisoned");
        jobs.get(&id)
            .map(|rec| rec.state.clone())
            .ok_or(DroidError::UnknownJob)
    }

    /// Take a finished job's still. Consumes the job: a second
    /// fetch, or fetching an unknown id, is
    /// [`DroidError::UnknownJob`]. Fetching a job that is still
    /// queued or running is an error, not a block — the caller
    /// polls.
    pub fn fetch(&self, id: JobId) -> Result<GeneratedImage, DroidError> {
        let mut jobs = self.shared.jobs.lock().expect("job table poisoned");
        let rec = jobs.get(&id).ok_or(DroidError::UnknownJob)?;
        match rec.state {
            JobState::Done | JobState::Failed(_) => {}
            JobState::Queued | JobState::Running { .. } => {
                return Err(DroidError::InvalidParams(
                    "job has not finished; poll until Done".to_owned(),
                ));
            }
        }
        let rec = jobs.remove(&id).expect("job vanished between checks");
        rec.result.expect("finished job holds a result")
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Closing the channel ends the worker after its current job.
        let (tx, _rx) = mpsc::channel::<WorkerMsg>();
        let old = std::mem::replace(&mut self.tx, tx);
        drop(old);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn encode_png(raw: &RawImage) -> Result<GeneratedImage, DroidError> {
    let expected = (raw.width as usize) * (raw.height as usize) * 3;
    if raw.rgb.len() != expected {
        return Err(DroidError::Backend(format!(
            "backend returned {} RGB bytes for {}x{} (expected {})",
            raw.rgb.len(),
            raw.width,
            raw.height,
            expected
        )));
    }
    let img = image::RgbImage::from_raw(raw.width, raw.height, raw.rgb.clone())
        .ok_or_else(|| DroidError::Backend("image buffer rejected".to_owned()))?;
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| DroidError::Backend(format!("PNG encode failed: {e}")))?;
    Ok(GeneratedImage {
        width: raw.width,
        height: raw.height,
        png,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub::StubBackend;
    use std::time::{Duration, Instant};

    fn params() -> GenParams {
        GenParams::recipe("a prompt".to_owned(), "a negative".to_owned(), 42)
    }

    fn wait_done(engine: &Engine, id: JobId) -> JobState {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = engine.poll(id).expect("poll");
            match state {
                JobState::Done | JobState::Failed(_) => return state,
                _ => {
                    assert!(Instant::now() < deadline, "job never finished");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    #[test]
    fn submit_poll_fetch_round_trip() {
        let engine = Engine::new(StubBackend::default());
        let id = engine.submit(params()).expect("submit");
        assert_eq!(wait_done(&engine, id), JobState::Done);
        let img = engine.fetch(id).expect("fetch");
        assert_eq!((img.width, img.height), (64, 64));
        assert!(img.png.starts_with(b"\x89PNG"));
        // Fetch consumes: a second fetch is UnknownJob.
        assert_eq!(engine.fetch(id), Err(DroidError::UnknownJob));
        assert_eq!(engine.poll(id), Err(DroidError::UnknownJob));
    }

    #[test]
    fn oversized_prompt_is_refused_before_the_backend() {
        let backend = StubBackend::default();
        let seen = backend.seen.clone();
        let engine = Engine::new(backend);
        let mut p = params();
        p.prompt = "x".repeat(crate::params::MAX_PROMPT_BYTES + 1);
        assert!(matches!(
            engine.submit(p),
            Err(DroidError::PromptTooLong { .. })
        ));
        assert!(seen.lock().expect("seen").is_empty());
    }

    #[test]
    fn backend_failure_surfaces_as_failed_state() {
        let engine = Engine::new(StubBackend {
            fail_with: Some("boom".to_owned()),
            ..Default::default()
        });
        let id = engine.submit(params()).expect("submit");
        assert_eq!(
            wait_done(&engine, id),
            JobState::Failed("backend failed: boom".to_owned())
        );
        assert_eq!(
            engine.fetch(id),
            Err(DroidError::Backend("boom".to_owned()))
        );
    }
}
