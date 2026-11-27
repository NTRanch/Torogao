//! torogao-edit-broker — the edit-route job broker service.
//!
//! One primo service owning the ComfyUI edit queue, so lanes
//! submit and poll instead of racing waiter scripts for idle
//! gaps (see the crate documentation in `lib.rs` for the job
//! document and the contract).
//!
//! Surface — the facade engine contract, shaped exactly as
//! the torogao-gen seat implements it:
//!
//! - `GET  /health` → `{"engine": "torogao-edit-broker",
//!   "model_loaded": bool, …}`. For this engine the "model"
//!   is the edit route itself: the broker reports loaded when
//!   the driver of record is in place, because the driver
//!   auto-starts ComfyUI on the first job — that auto-start
//!   IS this engine's load path. `comfyui_alive` and
//!   `driver_present` are reported separately so nothing
//!   about the mapping is hidden.
//! - `POST /jobs` → `202 {"id": "job-N"}` for a job document
//!   (validated by the library; refusals are 400 with the
//!   named reason).
//! - `GET  /jobs/<id>` → `{"state": …}` plus, when done, the
//!   delivered frame as `image_png_base64` and every output
//!   in `artefacts` with its sha256.
//! - `GET  /jobs` → counts by state plus the FIFO queue.
//! - `POST /jobs/<id>/interrupt` → mark a job cancelled. A
//!   QUEUED job never runs. A RUNNING job's driver subprocess
//!   is killed and ComfyUI is sent `/interrupt`, so the
//!   server-side prompt stops too; partial outputs are
//!   discarded.
//!
//! One worker thread runs jobs serially in submission order
//! — the edit route is one ComfyUI on one GPU, and a FIFO of
//! honest depth is the shape that replaces the racing
//! pollers. Each job runs under a deadline
//! ([`DRIVER_JOB_TIMEOUT_SECS`]): a driver that outlives it
//! is killed and the job errors, rather than holding the
//! queue forever.
//!
//! The broker binds loopback only. It is an engine, not the
//! public surface: the governed facade is the door, and its
//! governance (token gate, workflow hash, recipe bounds,
//! content screen) applies upstream when this engine is
//! registered behind it — that registration is the named
//! follow-on, not part of this tranche.
//!
//! Usage: `torogao-edit-broker [--port 8193]
//!   [--driver <edit-local.py>] [--work-dir <dir>]
//!   [--output-dir <route output dir>] [--comfy-url <url>]
//!   [--python <python3>]`

#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use torogao_edit_broker::{
    Artefact, BODY_BYTES_MAX, EditConfig, JobDocument, MaskDocument, MaskShape, base64_encode,
    collect_artefacts, delivered_artefact, edit_argv, parse_job_document,
};

/// The engine id reported on `/health`. The facade reads
/// this string and `model_loaded`; nothing else about the
/// health body is contractual.
const ENGINE_ID: &str = "torogao-edit-broker";

/// Largest request header block accepted, in bytes. Bodies
/// are bounded by the library's `BODY_BYTES_MAX`.
const HEADERS_BYTES_MAX: usize = 64 * 1024;

/// Most jobs retained in the broker's table at once.
/// Submissions beyond → 429, so a runaway caller cannot grow
/// the table (or the work directory) without bound.
const JOBS_RETAINED_MAX: usize = 256;

/// How long one driver run may take before the worker kills
/// it and errors the job. A faithful edit is ~100 s and a
/// cold ComfyUI start ~60 s on the record; 1800 s is thirty
/// minutes of headroom, not a target.
const DRIVER_JOB_TIMEOUT_SECS: u64 = 1800;

/// The worker's cancel/deadline poll cadence while a driver
/// runs.
const WORKER_POLL_MS: u64 = 100;

/// How much of the driver log's tail a failed job's error
/// message carries.
const ERROR_TAIL_CHARS: usize = 800;

#[derive(Debug, Clone, PartialEq)]
enum JobState {
    Queued,
    Running,
    Done,
    Error(String),
    Cancelled,
}

impl JobState {
    fn name(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Error(_) => "error",
            Self::Cancelled => "cancelled",
        }
    }
}

struct Job {
    state: JobState,
    config: EditConfig,
    seed: u64,
    mask: Option<MaskShape>,
    /// The job's staging directory (canvas, prompt, mask,
    /// driver log). Owned by the job for its whole life.
    dir: PathBuf,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
    elapsed_ms: Option<u64>,
    artefacts: Vec<Artefact>,
}

struct Broker {
    jobs: Mutex<HashMap<String, Job>>,
    /// Queued job ids in submission order — the FIFO the
    /// worker drains, and the queue a poll reports positions
    /// from.
    queue: Mutex<VecDeque<String>>,
    next_id: Mutex<u64>,
    driver: PathBuf,
    output_dir: PathBuf,
    work_dir: PathBuf,
    comfy: ComfyEndpoint,
    python: String,
}

/// The ComfyUI endpoint, reduced to a socket address at
/// startup: the route is loopback by construction, and
/// resolving once means the worker's probe/interrupt calls
/// never do DNS in the hot path.
struct ComfyEndpoint {
    url: String,
    addr: SocketAddr,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> Option<String> {
        args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
    };
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let port: u16 = arg("--port").and_then(|p| p.parse().ok()).unwrap_or(8193);
    let driver = arg("--driver").map(PathBuf::from).unwrap_or_else(|| {
        home.join(".local/share/pax/primo-local-edit-route-20261127/edit-local.py")
    });
    let output_dir = arg("--output-dir").map(PathBuf::from).unwrap_or_else(|| {
        driver
            .parent()
            .map(|route| route.join("output"))
            .unwrap_or_else(|| PathBuf::from("output"))
    });
    let work_dir = arg("--work-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share/torogao-edit-broker"));
    let comfy_url = arg("--comfy-url").unwrap_or_else(|| "http://127.0.0.1:8189".to_string());
    let comfy = match parse_comfy_endpoint(&comfy_url) {
        Ok(endpoint) => endpoint,
        Err(message) => {
            eprintln!("torogao-edit-broker: {message}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&work_dir) {
        eprintln!(
            "torogao-edit-broker: cannot create work dir {}: {e}",
            work_dir.display()
        );
        std::process::exit(1);
    }
    let broker = Arc::new(Broker {
        jobs: Mutex::new(HashMap::new()),
        queue: Mutex::new(VecDeque::new()),
        next_id: Mutex::new(seed_next_id(&work_dir)),
        driver,
        output_dir,
        work_dir,
        comfy,
        python: arg("--python").unwrap_or_else(|| "python3".to_string()),
    });

    // The worker thread owns the queue: it runs one driver
    // subprocess at a time, in submission order.
    let (tx, rx) = mpsc::channel::<String>();
    {
        let broker = Arc::clone(&broker);
        std::thread::spawn(move || {
            while let Ok(id) = rx.recv() {
                run_one(&broker, &id);
            }
        });
    }

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("torogao-edit-broker: cannot bind 127.0.0.1:{port}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "torogao-edit-broker: listening on 127.0.0.1:{port}, driver {}, \
         outputs {}, work {}",
        broker.driver.display(),
        broker.output_dir.display(),
        broker.work_dir.display()
    );
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let broker = Arc::clone(&broker);
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = handle(&mut stream, &broker, &tx);
        });
    }
}

/// The next job number, seeded one past the highest `job-N`
/// staging directory already in the work dir — the facade's
/// seeding discipline, so a restarted broker never reissues
/// an id whose outputs already exist.
fn seed_next_id(work_dir: &Path) -> u64 {
    let mut highest: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(work_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(number) = name
                .strip_prefix("job-")
                .and_then(|rest| rest.parse::<u64>().ok())
            {
                highest = highest.max(number);
            }
        }
    }
    highest + 1
}

fn parse_comfy_endpoint(url: &str) -> Result<ComfyEndpoint, String> {
    let authority = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("--comfy-url {url:?} must be an http:// loopback URL"))?;
    let authority = authority.split('/').next().unwrap_or_default();
    let resolved = authority
        .to_socket_addrs()
        .map_err(|e| format!("--comfy-url {url:?} does not resolve: {e}"))?
        .next()
        .ok_or_else(|| format!("--comfy-url {url:?} resolves to no address"))?;
    if !resolved.ip().is_loopback() {
        return Err(format!(
            "--comfy-url {url:?} is not loopback; the edit route is loopback-only"
        ));
    }
    Ok(ComfyEndpoint {
        url: url.trim_end_matches('/').to_string(),
        addr: resolved,
    })
}

// ---------------------------------------------------------------------------
// The worker: one job at a time, FIFO.

fn run_one(broker: &Broker, id: &str) {
    let (config, seed, mask, dir, cancel) = {
        let mut jobs = broker.jobs.lock().unwrap();
        let Some(job) = jobs.get_mut(id) else { return };
        dequeue(broker, id);
        if job.state != JobState::Queued || job.cancel.load(Ordering::SeqCst) {
            job.state = JobState::Cancelled;
            return;
        }
        job.state = JobState::Running;
        (
            job.config,
            job.seed,
            job.mask,
            job.dir.clone(),
            Arc::clone(&job.cancel),
        )
    };
    let started = Instant::now();
    let outcome = run_driver(broker, id, &dir, config, seed, mask.as_ref(), &cancel);
    let mut jobs = broker.jobs.lock().unwrap();
    let Some(job) = jobs.get_mut(id) else { return };
    job.elapsed_ms = Some(started.elapsed().as_millis() as u64);
    match outcome {
        DriverOutcome::Completed(artefacts) => {
            job.artefacts = artefacts;
            job.state = JobState::Done;
        }
        DriverOutcome::Failed(message) => {
            job.error = Some(message.clone());
            job.state = JobState::Error(message);
        }
        DriverOutcome::Cancelled => {
            job.state = JobState::Cancelled;
        }
    }
}

fn dequeue(broker: &Broker, id: &str) {
    let mut queue = broker.queue.lock().unwrap();
    queue.retain(|queued| queued != id);
}

enum DriverOutcome {
    Completed(Vec<Artefact>),
    Failed(String),
    Cancelled,
}

/// Run one job's driver subprocess to a terminal outcome.
/// Owns the child for its whole life: polls it, kills it on
/// cancel or deadline, and reaps it on every path.
fn run_driver(
    broker: &Broker,
    id: &str,
    dir: &Path,
    config: EditConfig,
    seed: u64,
    mask: Option<&MaskShape>,
    cancel: &AtomicBool,
) -> DriverOutcome {
    // A previous life of this prefix (a restarted broker, a
    // reissued id) must not leak stale outputs into this
    // job's artefact list.
    remove_prefix_outputs(&broker.output_dir, id);
    let mask_file = dir.join("mask.png");
    let argv = edit_argv(
        &dir.join("canvas.png"),
        &dir.join("prompt.txt"),
        id,
        config,
        seed,
        mask,
        matches!(mask, Some(MaskShape::Explicit { .. })).then_some(mask_file.as_path()),
    );
    let log_path = dir.join("driver.log");
    let log_file = match std::fs::File::create(&log_path) {
        Ok(file) => file,
        Err(e) => return DriverOutcome::Failed(format!("cannot create driver log: {e}")),
    };
    let log_stderr = match log_file.try_clone() {
        Ok(file) => file,
        Err(e) => return DriverOutcome::Failed(format!("cannot clone driver log: {e}")),
    };
    let mut child = match Command::new(&broker.python)
        .arg(&broker.driver)
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_stderr))
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return DriverOutcome::Failed(format!(
                "cannot spawn driver {} {}: {e}",
                broker.python,
                broker.driver.display()
            ));
        }
    };
    let deadline = Instant::now() + Duration::from_secs(DRIVER_JOB_TIMEOUT_SECS);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if cancel.load(Ordering::SeqCst) {
                    remove_prefix_outputs(&broker.output_dir, id);
                    return DriverOutcome::Cancelled;
                }
                if status.success() {
                    return match collect_artefacts(&broker.output_dir, id) {
                        Ok(artefacts) if delivered_artefact(&artefacts).is_some() => {
                            DriverOutcome::Completed(artefacts)
                        }
                        Ok(_) => DriverOutcome::Failed(format!(
                            "driver exited 0 but produced no output for prefix {id}; \
                             log tail: {}",
                            log_tail(&log_path)
                        )),
                        Err(e) => {
                            DriverOutcome::Failed(format!("cannot collect outputs for {id}: {e}"))
                        }
                    };
                }
                return DriverOutcome::Failed(format!(
                    "driver exited {status}; log tail: {}",
                    log_tail(&log_path)
                ));
            }
            Ok(None) => {
                if cancel.load(Ordering::SeqCst) {
                    stop_child(broker, &mut child);
                    remove_prefix_outputs(&broker.output_dir, id);
                    return DriverOutcome::Cancelled;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return DriverOutcome::Failed(format!(
                        "driver exceeded the {DRIVER_JOB_TIMEOUT_SECS} s job deadline \
                         and was killed; log tail: {}",
                        log_tail(&log_path)
                    ));
                }
                std::thread::sleep(Duration::from_millis(WORKER_POLL_MS));
            }
            Err(e) => {
                return DriverOutcome::Failed(format!("cannot wait on the driver: {e}"));
            }
        }
    }
}

/// Stop a running job's work on both sides of the route:
/// ComfyUI's `/interrupt` ends the server-side prompt, and
/// killing the driver ends the client that would otherwise
/// wait for it. The ComfyUI call is best-effort by design —
/// the kill below is the guarantee; a wedged server cannot
/// be allowed to veto a cancel.
fn stop_child(broker: &Broker, child: &mut Child) {
    comfy_post_interrupt(&broker.comfy);
    let _ = child.kill();
    let _ = child.wait();
}

/// Delete this job's prefix outputs from the route's output
/// directory: exactly the names `collect_artefacts` would
/// claim for the prefix, nothing else.
fn remove_prefix_outputs(output_dir: &Path, prefix: &str) {
    let stem = format!("{prefix}_");
    if let Ok(entries) = std::fs::read_dir(output_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&stem) && name.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// The last [`ERROR_TAIL_CHARS`] characters of a driver log,
/// for a failed job's error message.
fn log_tail(log_path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(log_path) else {
        return "<driver log unreadable>".to_string();
    };
    let tail: String = text
        .chars()
        .rev()
        .take(ERROR_TAIL_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let tail = tail.trim();
    if tail.is_empty() {
        "<driver log empty>".to_string()
    } else {
        tail.to_string()
    }
}

// ---------------------------------------------------------------------------
// Minimal ComfyUI client: two calls, hand-rolled like the
// seat's server, loopback only.

fn comfy_request(comfy: &ComfyEndpoint, request: &str, timeout: Duration) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&comfy.addr, timeout).ok()?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    Some(response)
}

fn comfy_alive(comfy: &ComfyEndpoint) -> bool {
    let request = format!(
        "GET /system_stats HTTP/1.1\r\nhost: {}\r\nconnection: close\r\n\r\n",
        comfy.addr
    );
    comfy_request(comfy, &request, Duration::from_millis(1500))
        .is_some_and(|response| response.starts_with("HTTP/1.1 200"))
}

fn comfy_post_interrupt(comfy: &ComfyEndpoint) {
    let request = format!(
        "POST /interrupt HTTP/1.1\r\nhost: {}\r\ncontent-length: 0\r\n\
         connection: close\r\n\r\n",
        comfy.addr
    );
    let _ = comfy_request(comfy, &request, Duration::from_secs(2));
}

// ---------------------------------------------------------------------------
// The HTTP surface.

fn handle(
    stream: &mut TcpStream,
    broker: &Broker,
    tx: &mpsc::Sender<String>,
) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end;
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if buf.len() > HEADERS_BYTES_MAX {
            return respond(stream, 431, &json!({"error": "headers too large"}));
        }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let content_length: usize = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(0);
    if content_length > BODY_BYTES_MAX {
        return respond(stream, 413, &json!({"error": "body too large"}));
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);

    match (method, path) {
        ("GET", "/health") => {
            let driver_present = broker.driver.is_file();
            let queue_depth = broker.queue.lock().unwrap().len();
            respond(
                stream,
                200,
                &json!({
                    "engine": ENGINE_ID,
                    // The driver auto-starts ComfyUI on the
                    // first job; that auto-start is this
                    // engine's load path, so readiness is the
                    // driver of record being in place. The two
                    // facts are also reported separately.
                    "model_loaded": driver_present,
                    "driver_present": driver_present,
                    "comfyui_alive": comfy_alive(&broker.comfy),
                    "comfyui_url": broker.comfy.url,
                    "queue_depth": queue_depth,
                }),
            )
        }
        ("GET", "/jobs") => {
            let jobs = broker.jobs.lock().unwrap();
            let mut counts = HashMap::new();
            for job in jobs.values() {
                *counts.entry(job.state.name()).or_insert(0u64) += 1;
            }
            let queue: Vec<String> = broker.queue.lock().unwrap().iter().cloned().collect();
            respond(stream, 200, &json!({"counts": counts, "queue": queue}))
        }
        ("POST", "/jobs") => submit(stream, broker, tx, &body),
        ("GET", p) if p.starts_with("/jobs/") => {
            let id = &p["/jobs/".len()..];
            poll(stream, broker, id)
        }
        ("POST", p) if p.starts_with("/jobs/") && p.ends_with("/interrupt") => {
            let id = &p["/jobs/".len()..p.len() - "/interrupt".len()];
            let jobs = broker.jobs.lock().unwrap();
            if let Some(job) = jobs.get(id) {
                job.cancel.store(true, Ordering::SeqCst);
                respond(stream, 200, &json!({"id": id, "interrupted": true}))
            } else {
                respond(stream, 404, &json!({"error": "no such job"}))
            }
        }
        _ => respond(stream, 404, &json!({"error": "not found"})),
    }
}

fn submit(
    stream: &mut TcpStream,
    broker: &Broker,
    tx: &mpsc::Sender<String>,
    body: &[u8],
) -> std::io::Result<()> {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(e) => return respond(stream, 400, &json!({"error": format!("bad json: {e}")})),
    };
    let document = match parse_job_document(&parsed) {
        Ok(document) => document,
        Err(refusal) => {
            return respond(stream, 400, &json!({"error": refusal.to_string()}));
        }
    };
    if broker.jobs.lock().unwrap().len() >= JOBS_RETAINED_MAX {
        return respond(
            stream,
            429,
            &json!({"error": "broker job table is full; poll and let jobs finish"}),
        );
    }
    let id = {
        let mut next = broker.next_id.lock().unwrap();
        let id = format!("job-{}", *next);
        *next += 1;
        id
    };
    let dir = broker.work_dir.join(&id);
    if let Err(e) = stage_job_files(&dir, &document) {
        return respond(
            stream,
            500,
            &json!({"error": format!("cannot stage job files for {id}: {e}")}),
        );
    }
    let mask = document.mask.as_ref().map(MaskDocument::shape);
    broker.jobs.lock().unwrap().insert(
        id.clone(),
        Job {
            state: JobState::Queued,
            config: document.config,
            seed: document.seed,
            mask,
            dir,
            error: None,
            cancel: Arc::new(AtomicBool::new(false)),
            elapsed_ms: None,
            artefacts: Vec::new(),
        },
    );
    broker.queue.lock().unwrap().push_back(id.clone());
    let _ = tx.send(id.clone());
    respond(stream, 202, &json!({"id": id}))
}

/// Write a validated job's inputs into its staging
/// directory: the canvas and prompt the driver reads, and
/// the mask image for an explicit-mask job. After staging,
/// the broker holds no image bytes in memory — the files are
/// the job's inputs of record.
fn stage_job_files(dir: &Path, document: &JobDocument) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("canvas.png"), &document.canvas_png)?;
    std::fs::write(dir.join("prompt.txt"), &document.prompt)?;
    if let Some(MaskDocument::Explicit { mask_png, .. }) = &document.mask {
        std::fs::write(dir.join("mask.png"), mask_png)?;
    }
    Ok(())
}

fn poll(stream: &mut TcpStream, broker: &Broker, id: &str) -> std::io::Result<()> {
    let jobs = broker.jobs.lock().unwrap();
    let Some(job) = jobs.get(id) else {
        return respond(stream, 404, &json!({"error": "no such job"}));
    };
    let queue_position = if job.state == JobState::Queued {
        broker
            .queue
            .lock()
            .unwrap()
            .iter()
            .position(|queued| queued == id)
    } else {
        None
    };
    let mut out = json!({
        "id": id,
        "state": job.state.name(),
        "error": job.error,
        "elapsed_ms": job.elapsed_ms,
        "config": job.config.name(),
        "config_label": job.config.label(),
        "seed": job.seed,
        "queue_position": queue_position,
    });
    if job.state == JobState::Done {
        let (artefacts_json, delivered_json, artefact_error) = artefacts_json(&job.artefacts);
        out["artefacts"] = artefacts_json;
        out["image_png_base64"] = delivered_json;
        out["delivered"] = delivered_artefact(&job.artefacts)
            .map(|artefact| Value::from(artefact.kind.name()))
            .unwrap_or(Value::Null);
        out["artefact_error"] = artefact_error;
    }
    respond(stream, 200, &out)
}

/// Render a finished job's artefacts for a poll: each with
/// its name, kind, sha256, size, and bytes inline (the
/// contract carries images inline, as the gen seat's does),
/// plus the delivered frame's bytes for the top-level
/// `image_png_base64` the facade's poll consumer reads. A
/// file that vanished since collection is reported as a null
/// image with the reason in `artefact_error`, never as a
/// panic or a silent omission.
fn artefacts_json(artefacts: &[Artefact]) -> (Value, Value, Value) {
    let mut list = Vec::new();
    let mut first_error: Option<String> = None;
    for artefact in artefacts {
        let image = match std::fs::read(&artefact.path) {
            Ok(bytes) => Value::String(base64_encode(&bytes)),
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(format!("artefact {} unreadable: {e}", artefact.name));
                }
                Value::Null
            }
        };
        list.push(json!({
            "name": artefact.name,
            "kind": artefact.kind.name(),
            "sha256": artefact.sha256,
            "bytes": artefact.bytes,
            "image_png_base64": image,
        }));
    }
    let delivered = delivered_artefact(artefacts)
        .and_then(|artefact| {
            list.iter()
                .find(|entry| entry["name"] == artefact.name)
                .map(|entry| entry["image_png_base64"].clone())
        })
        .unwrap_or(Value::Null);
    (
        Value::Array(list),
        delivered,
        first_error.map(Value::from).unwrap_or(Value::Null),
    )
}

fn respond(stream: &mut TcpStream, code: u32, body: &Value) -> std::io::Result<()> {
    let text = body.to_string();
    let reason = match code {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\ncontent-type: application/json\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n",
        text.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
