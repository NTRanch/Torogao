//! torogao-mobile-facade — the governed seat surface.
//!
//! The app never talks to a raw engine. It talks to this
//! facade, which enforces the governance seat-side before
//! anything reaches an engine:
//!
//! - the workflow approval hash: a submission's
//!   `X-Workflow-Sha256` header must equal the SHA-256 of the
//!   workflow document received, so a document altered in
//!   transit does not run;
//! - the checkpoint of record: only the wai checkpoint named in
//!   the core's style record may be requested;
//! - the recipe bounds: steps/CFG/size within the recipe of
//!   record's bounds, sampler and scheduler exactly the
//!   recipe's;
//! - the prompt cap, and the content screen over the composed
//!   positive prompt — the same [`torogao_mobile_core::screen`]
//!   code the core enforces client-side.
//!
//! Contract (what the mobile core speaks):
//! - `POST /prompt` {prompt: workflow} → `{prompt_id}`
//! - `GET /history/<id>` → ComfyUI-shaped history document
//! - `GET /view?filename&subfolder&type` → the still's PNG
//! - `POST /interrupt` {prompt_id}
//! - `GET /facade/health` → `{engine, seat: ready|starting|down}`
//! - `POST /facade/wake` → start the engine service, report state
//! - `GET /facade/queue` → `{queued, running}` (reaper evidence)
//!
//! Engine contract (what the facade speaks to an engine):
//! `GET /health`, `POST /jobs`, `GET /jobs/<id>`,
//! `POST /jobs/<id>/interrupt`, with the engine answering
//! `{"engine": <id>, "model_loaded": bool}` on health. The
//! torogao-gen seat implements it today; an on-device engine
//! registers behind the same seam tomorrow.
//!
//! Lifecycle: the facade itself is standing; the engine is
//! started on demand (a submission or a wake starts it) and
//! reaped when idle by the reap timer. A submission accepted
//! while the engine sleeps is held by the facade and forwarded
//! when the engine reports ready — bounded by
//! [`ENGINE_WAKE_DEADLINE_SECS`], after which the job errors
//! honestly instead of waiting forever.
//!
//! Token gate (engine lane, 2026-11-26): when the
//! `TOROGAO_FACADE_TOKEN` environment variable is set
//! (non-empty), every route except `GET /facade/health`
//! requires the request header `X-Facade-Token` to carry the
//! same value; a missing or wrong token is a 401. When the
//! variable is unset the gate is off — the pre-gate behaviour,
//! kept so the shells built before the gate keep working until
//! they learn the header. Activation contract: the variable
//! lands in the facade's service environment and the client
//! sends the header from its seat settings — the two land
//! together or not at all. ACTIVATED 2026-11-26 (completion
//! lane): the core client sends the header from
//! `SeatConfig::token`, and the live unit's launcher reads the
//! token from the state dir's `facade-token` file, so the value
//! lives in exactly one 0600 file and in no unit file.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use torogao_mobile_core::screen::{ScreenVerdict, screen_text};
use torogao_mobile_core::style::{self, RecipeSettings};
use torogao_mobile_core::workflow::{USER_PROMPT_BYTES_MAX, workflow_sha256};

/// Largest workflow body accepted.
const BODY_BYTES_MAX: usize = 256 * 1024;
/// Allowance over the user-prompt cap for the composed positive
/// prompt (style prefix + canonical identity + signature). The
/// client's 2000-byte cap governs the user portion; the facade
/// cannot separate the portions back out, so it caps the whole.
const COMPOSED_POSITIVE_BYTES_MAX: usize = USER_PROMPT_BYTES_MAX + 1200;
/// How long a held job waits for a waking engine before erroring.
const ENGINE_WAKE_DEADLINE_SECS: u64 = 240;
/// Poll cadence for engine readiness while a job is held.
const ENGINE_POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq)]
enum JobPhase {
    /// Accepted by the facade; the engine is waking.
    Waking,
    /// Forwarded to the engine under this engine-side id.
    AtEngine(String),
    /// Engine reported done; the image is filed under this name.
    Done(String),
    /// The job failed; the string is the presentable reason.
    Error(String),
    /// Cancelled by the user.
    Cancelled,
}

struct Job {
    phase: JobPhase,
}

struct Facade {
    jobs: Mutex<HashMap<String, Job>>,
    next_id: Mutex<u64>,
    engine_url: String,
    engine_service: String,
    outputs_dir: PathBuf,
    /// The token gate's expected `X-Facade-Token` value, from
    /// `TOROGAO_FACADE_TOKEN`; `None` = the gate is off.
    token: Option<String>,
}

/// The first job number a fresh facade process may hand out:
/// one above the highest `torogao_mobile_facade-<n>.png` already
/// in the outputs directory (1 when there are none). A job's id
/// names its output file, so a restart that renumbered from 1
/// would hand a later job an earlier job's id and overwrite its
/// filed still (the engine lane's counter flag, 2026-11-26). The
/// jobs map itself is in-memory by design and is not resurrected;
/// only the numbering is made restart-safe.
fn seed_next_id(outputs_dir: &Path) -> u64 {
    let mut max_seen = 0_u64;
    if let Ok(entries) = std::fs::read_dir(outputs_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(num) = name
                .strip_prefix("torogao_mobile_facade-")
                .and_then(|rest| rest.strip_suffix(".png"))
            else {
                continue;
            };
            if let Ok(n) = num.parse::<u64>() {
                max_seen = max_seen.max(n);
            }
        }
    }
    max_seen + 1
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> Option<String> {
        args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
    };
    let bind = arg("--bind").unwrap_or_else(|| "0.0.0.0:8190".to_string());
    let engine_url = arg("--engine-url")
        .unwrap_or_else(|| "http://127.0.0.1:8192".to_string())
        .trim_end_matches('/')
        .to_string();
    let engine_service =
        arg("--engine-service").unwrap_or_else(|| "torogao-gen-seat.service".to_string());
    let state_dir = arg("--state-dir").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".local/share/torogao-mobile-facade")
    });
    let outputs_dir = state_dir.join("outputs");
    if let Err(e) = std::fs::create_dir_all(&outputs_dir) {
        eprintln!(
            "torogao-mobile-facade: cannot create {}: {e}",
            outputs_dir.display()
        );
        std::process::exit(1);
    }
    let token = std::env::var("TOROGAO_FACADE_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let facade = Arc::new(Facade {
        jobs: Mutex::new(HashMap::new()),
        next_id: Mutex::new(seed_next_id(&outputs_dir)),
        engine_url,
        engine_service,
        outputs_dir,
        token,
    });
    let listener = match TcpListener::bind(&bind) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("torogao-mobile-facade: cannot bind {bind}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "torogao-mobile-facade: listening on {bind}, engine at {}, token gate {}",
        facade.engine_url,
        if facade.token.is_some() { "on" } else { "off" }
    );
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let facade = Arc::clone(&facade);
        std::thread::spawn(move || {
            let _ = handle(&mut stream, &facade);
        });
    }
}

// ---------------------------------------------------------------------------
// Engine side

/// The engine's state as the facade sees it.
#[derive(Debug, Clone, PartialEq)]
enum EngineState {
    Ready,
    Starting,
    Down,
}

fn engine_state(facade: &Facade) -> (EngineState, String) {
    let url = format!("{}/health", facade.engine_url);
    match ureq::get(&url).timeout(Duration::from_secs(5)).call() {
        Ok(r) => match r.into_json::<Value>() {
            Ok(body) => {
                let engine = body
                    .get("engine")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                if body.get("model_loaded").and_then(Value::as_bool) == Some(true) {
                    (EngineState::Ready, engine)
                } else {
                    (EngineState::Starting, engine)
                }
            }
            Err(_) => (EngineState::Down, "unknown".to_string()),
        },
        Err(_) => (EngineState::Down, "unknown".to_string()),
    }
}

/// Start the engine's user service. Fire-and-forget: readiness
/// is polled, never assumed.
fn wake_engine(facade: &Facade) {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "start", &facade.engine_service])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

// ---------------------------------------------------------------------------
// Governance: validate a submitted workflow document.

/// A validated generation request, translated out of the
/// workflow document into the engine contract's terms.
#[derive(Debug, Clone, PartialEq)]
struct EngineJob {
    positive: String,
    negative: String,
    seed: u64,
    steps: u64,
    cfg: f64,
    width: u64,
    height: u64,
    sampler: String,
    scheduler: String,
}

/// Validate a workflow document against the governance rules.
/// `Err((status, message))`: 422 for a screen refusal (the
/// client maps it to its typed screen error), 400 for the rest.
fn validate_workflow(
    workflow: &Value,
    hash_header: Option<&str>,
) -> Result<EngineJob, (u32, String)> {
    let bad = |m: &str| (400, m.to_string());
    // Approval/hash discipline.
    let expected = workflow_sha256(workflow);
    match hash_header {
        Some(h) if h == expected => {}
        Some(_) => {
            return Err(bad(
                "workflow hash mismatch: the document was altered in transit",
            ));
        }
        None => return Err(bad("missing X-Workflow-Sha256 header")),
    }
    let node = |id: &str| -> Result<&Value, (u32, String)> {
        workflow
            .get(id)
            .ok_or_else(|| bad(&format!("workflow has no node {id}")))
    };
    let text = |id: &str| -> Result<String, (u32, String)> {
        node(id)?
            .get("inputs")
            .and_then(|i| i.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| bad(&format!("node {id} has no text input")))
    };
    // Checkpoint of record.
    let ckpt = node("1")?
        .get("inputs")
        .and_then(|i| i.get("ckpt_name"))
        .and_then(Value::as_str)
        .ok_or_else(|| bad("node 1 has no ckpt_name"))?;
    if ckpt != style::CHECKPOINT {
        return Err(bad("checkpoint is not the checkpoint of record"));
    }
    let positive = text("2")?;
    let negative = text("3")?;
    if positive.len() > COMPOSED_POSITIVE_BYTES_MAX {
        return Err(bad("composed positive prompt exceeds the prompt cap"));
    }
    // The content screen, seat-side, over the composed prompt.
    if let ScreenVerdict::Refuse(reason) = screen_text(&positive) {
        return Err((422, reason));
    }
    let sampler_inputs = node("5")?
        .get("inputs")
        .ok_or_else(|| bad("node 5 has no inputs"))?;
    let sampler = sampler_inputs
        .get("sampler_name")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("node 5 has no sampler_name"))?;
    let scheduler = sampler_inputs
        .get("scheduler")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("node 5 has no scheduler"))?;
    if sampler != style::SAMPLER || scheduler != style::SCHEDULER {
        return Err(bad("sampler/scheduler are not the recipe of record"));
    }
    let steps = sampler_inputs
        .get("steps")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("node 5 has no steps"))?;
    let cfg = sampler_inputs
        .get("cfg")
        .and_then(Value::as_f64)
        .ok_or_else(|| bad("node 5 has no cfg"))?;
    let seed = sampler_inputs
        .get("seed")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("node 5 has no seed"))?;
    let latent_inputs = node("4")?
        .get("inputs")
        .ok_or_else(|| bad("node 4 has no inputs"))?;
    let width = latent_inputs
        .get("width")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("node 4 has no width"))?;
    let height = latent_inputs
        .get("height")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("node 4 has no height"))?;
    let recipe = RecipeSettings {
        steps: steps as u32,
        cfg: cfg as f32,
        width: width as u32,
        height: height as u32,
    };
    recipe.validate().map_err(|m| (400, m))?;
    Ok(EngineJob {
        positive,
        negative,
        seed,
        steps,
        cfg,
        width,
        height,
        sampler: sampler.to_string(),
        scheduler: scheduler.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Job lifecycle

/// Hold a job until the engine is ready, then forward it. Runs
/// on its own thread per waking job.
fn forward_when_ready(facade: Arc<Facade>, prompt_id: String, job: EngineJob) {
    let deadline = unix_now() + ENGINE_WAKE_DEADLINE_SECS;
    loop {
        {
            let jobs = facade.jobs.lock().unwrap();
            match jobs.get(&prompt_id).map(|j| &j.phase) {
                Some(JobPhase::Cancelled) | None => return,
                _ => {}
            }
        }
        match engine_state(&facade).0 {
            EngineState::Ready => break,
            EngineState::Down => wake_engine(&facade),
            EngineState::Starting => {}
        }
        if unix_now() >= deadline {
            set_phase(
                &facade,
                &prompt_id,
                JobPhase::Error(format!(
                    "the seat did not become ready within {ENGINE_WAKE_DEADLINE_SECS}s"
                )),
            );
            return;
        }
        std::thread::sleep(ENGINE_POLL_INTERVAL);
    }
    let body = json!({
        "positive": job.positive,
        "negative": job.negative,
        "seed": job.seed,
        "steps": job.steps,
        "cfg": job.cfg,
        "width": job.width,
        "height": job.height,
        "sampler": job.sampler,
        "scheduler": job.scheduler,
    });
    let url = format!("{}/jobs", facade.engine_url);
    match ureq::post(&url)
        .timeout(Duration::from_secs(30))
        .send_json(body)
    {
        Ok(r) => match r.into_json::<Value>() {
            Ok(v) => match v.get("id").and_then(Value::as_str) {
                Some(id) => set_phase(&facade, &prompt_id, JobPhase::AtEngine(id.to_string())),
                None => set_phase(
                    &facade,
                    &prompt_id,
                    JobPhase::Error("engine reply had no job id".to_string()),
                ),
            },
            Err(e) => set_phase(
                &facade,
                &prompt_id,
                JobPhase::Error(format!("unreadable engine reply: {e}")),
            ),
        },
        Err(e) => set_phase(
            &facade,
            &prompt_id,
            JobPhase::Error(format!("engine refused the job: {e}")),
        ),
    }
}

fn set_phase(facade: &Facade, prompt_id: &str, phase: JobPhase) {
    if let Some(job) = facade.jobs.lock().unwrap().get_mut(prompt_id) {
        // A user cancel wins over any later phase write.
        if job.phase == JobPhase::Cancelled && phase != JobPhase::Cancelled {
            return;
        }
        job.phase = phase;
    }
}

/// Build the history document for one job, polling the engine
/// and filing the image when the engine reports done.
fn history_for(facade: &Facade, prompt_id: &str) -> Value {
    let phase = {
        let jobs = facade.jobs.lock().unwrap();
        jobs.get(prompt_id).map(|j| j.phase.clone())
    };
    let Some(phase) = phase else {
        return json!({});
    };
    let running = |msg: &str| {
        json!({ prompt_id: {
            "status": {"status_str": "running", "completed": false, "messages": [msg]},
            "outputs": {}
        }})
    };
    let errored = |msg: String| {
        json!({ prompt_id: {
            "status": {"status_str": "error", "completed": true, "messages": [msg]},
            "outputs": {}
        }})
    };
    match phase {
        JobPhase::Waking => running("seat is starting"),
        JobPhase::Cancelled => errored("cancelled by the user".to_string()),
        JobPhase::Error(m) => errored(m),
        JobPhase::Done(filename) => json!({ prompt_id: {
            "status": {"status_str": "success", "completed": true, "messages": []},
            "outputs": {"7": {"images": [{"filename": filename, "subfolder": "", "type": "output"}]}}
        }}),
        JobPhase::AtEngine(engine_id) => {
            let url = format!("{}/jobs/{engine_id}", facade.engine_url);
            match ureq::get(&url).timeout(Duration::from_secs(10)).call() {
                Ok(r) => match r.into_json::<Value>() {
                    Ok(v) => match v.get("state").and_then(Value::as_str) {
                        Some("done") => match v.get("image_png_base64").and_then(Value::as_str) {
                            Some(b64) => match base64_decode(b64) {
                                Some(png) => {
                                    let filename = format!("torogao_mobile_{prompt_id}.png");
                                    let path = facade.outputs_dir.join(&filename);
                                    match std::fs::write(&path, &png) {
                                        Ok(()) => {
                                            set_phase(
                                                facade,
                                                prompt_id,
                                                JobPhase::Done(filename.clone()),
                                            );
                                            json!({ prompt_id: {
                                                "status": {"status_str": "success", "completed": true, "messages": []},
                                                "outputs": {"7": {"images": [{"filename": filename, "subfolder": "", "type": "output"}]}}
                                            }})
                                        }
                                        Err(e) => errored(format!("cannot file the image: {e}")),
                                    }
                                }
                                None => errored("engine image was not valid base64".to_string()),
                            },
                            None => errored("engine finished without an image".to_string()),
                        },
                        Some("error") => {
                            let msg = v
                                .get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("engine error")
                                .to_string();
                            set_phase(facade, prompt_id, JobPhase::Error(msg.clone()));
                            errored(msg)
                        }
                        Some("cancelled") => {
                            set_phase(facade, prompt_id, JobPhase::Cancelled);
                            errored("cancelled by the user".to_string())
                        }
                        _ => running("generating"),
                    },
                    Err(e) => errored(format!("unreadable engine reply: {e}")),
                },
                Err(e) => errored(format!("engine unreachable while polling: {e}")),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP surface

/// One reply: a JSON document, or raw PNG bytes for the view
/// route.
enum Reply {
    Json(u32, Value),
    Png(Vec<u8>),
}

fn handle(stream: &mut std::net::TcpStream, facade: &Arc<Facade>) -> std::io::Result<()> {
    let (method, path, headers, body) = match read_request(stream)? {
        Some(r) => r,
        None => return Ok(()),
    };
    respond(stream, &route(facade, &method, &path, &headers, &body))
}

/// Constant-time token comparison: the gate is a network
/// boundary, so do not hand it a timing oracle.
fn token_matches(presented: Option<&String>, expected: &str) -> bool {
    let Some(presented) = presented else {
        return false;
    };
    let presented = presented.as_bytes();
    let expected = expected.as_bytes();
    if presented.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in presented.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn route(
    facade: &Arc<Facade>,
    method: &str,
    path: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Reply {
    let json = |code: u32, v: Value| Reply::Json(code, v);
    // The token gate: with a token configured, every route
    // except the health probe (engine id + seat state only)
    // requires the header. See the crate docs for activation.
    if let Some(token) = facade.token.as_deref() {
        let is_health = method == "GET" && path == "/facade/health";
        if !is_health && !token_matches(headers.get("x-facade-token"), token) {
            return json(401, json!({"error": "facade token required"}));
        }
    }
    match (method, path) {
        ("GET", "/facade/health") => {
            let (state, engine) = engine_state(facade);
            let seat = match state {
                EngineState::Ready => "ready",
                EngineState::Starting => "starting",
                EngineState::Down => "down",
            };
            json(200, json!({"engine": engine, "seat": seat}))
        }
        ("POST", "/facade/wake") => {
            wake_engine(facade);
            let (state, engine) = engine_state(facade);
            let seat = match state {
                EngineState::Ready => "ready",
                EngineState::Starting => "starting",
                EngineState::Down => "starting",
            };
            json(200, json!({"engine": engine, "seat": seat}))
        }
        ("GET", "/facade/queue") => {
            let jobs = facade.jobs.lock().unwrap();
            let queued = jobs
                .values()
                .filter(|j| j.phase == JobPhase::Waking)
                .count();
            let running = jobs
                .values()
                .filter(|j| matches!(j.phase, JobPhase::AtEngine(_)))
                .count();
            json(200, json!({"queued": queued, "running": running}))
        }
        ("POST", "/prompt") => {
            let parsed: Value = match serde_json::from_slice(body) {
                Ok(v) => v,
                Err(e) => return json(400, json!({"error": format!("bad json: {e}")})),
            };
            let Some(workflow) = parsed.get("prompt").cloned() else {
                return json(400, json!({"error": "body has no prompt document"}));
            };
            let hash = headers.get("x-workflow-sha256").map(String::as_str);
            match validate_workflow(&workflow, hash) {
                Ok(job) => {
                    let prompt_id = {
                        let mut next = facade.next_id.lock().unwrap();
                        let id = format!("facade-{}", *next);
                        *next += 1;
                        id
                    };
                    facade.jobs.lock().unwrap().insert(
                        prompt_id.clone(),
                        Job {
                            phase: JobPhase::Waking,
                        },
                    );
                    let f = Arc::clone(facade);
                    let pid = prompt_id.clone();
                    std::thread::spawn(move || forward_when_ready(f, pid, job));
                    json(200, json!({"prompt_id": prompt_id}))
                }
                Err((code, message)) => json(code, json!({"error": message})),
            }
        }
        ("POST", "/interrupt") => {
            let parsed: Value = serde_json::from_slice(body).unwrap_or(json!({}));
            let Some(prompt_id) = parsed.get("prompt_id").and_then(Value::as_str) else {
                return json(400, json!({"error": "no prompt_id"}));
            };
            let engine_job = {
                let jobs = facade.jobs.lock().unwrap();
                jobs.get(prompt_id).and_then(|j| match &j.phase {
                    JobPhase::AtEngine(id) => Some(id.clone()),
                    _ => None,
                })
            };
            set_phase(facade, prompt_id, JobPhase::Cancelled);
            if let Some(engine_id) = engine_job {
                let url = format!("{}/jobs/{engine_id}/interrupt", facade.engine_url);
                let _ = ureq::post(&url)
                    .timeout(Duration::from_secs(5))
                    .send_json(json!({}));
            }
            json(200, json!({"interrupted": prompt_id}))
        }
        ("GET", p) if p.starts_with("/history/") => {
            let id = &p["/history/".len()..];
            json(200, history_for(facade, id))
        }
        ("GET", p) if p.starts_with("/view") => view(facade, p),
        _ => json(404, json!({"error": "not found"})),
    }
}

/// Serve one filed image. Only plain file names inside the
/// outputs directory are served — no subfolders, no traversal.
fn view(facade: &Facade, path: &str) -> Reply {
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut filename: Option<String> = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "filename" {
                filename = Some(percent_decode(v));
            }
            if k == "subfolder" && !v.is_empty() {
                return Reply::Json(400, json!({"error": "subfolder views are not served"}));
            }
        }
    }
    let Some(filename) = filename else {
        return Reply::Json(400, json!({"error": "no filename"}));
    };
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Reply::Json(400, json!({"error": "bad filename"}));
    }
    let file_path = facade.outputs_dir.join(&filename);
    match std::fs::read(&file_path) {
        Ok(bytes) => Reply::Png(bytes),
        Err(_) => Reply::Json(404, json!({"error": "no such image"})),
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

// ---------------------------------------------------------------------------
// HTTP plumbing

type Request = (String, String, HashMap<String, String>, Vec<u8>);

fn read_request(stream: &mut std::net::TcpStream) -> std::io::Result<Option<Request>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end;
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if buf.len() > BODY_BYTES_MAX {
            return Ok(None);
        }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = HashMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let value = v.trim().to_string();
            if key == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.insert(key, value);
        }
    }
    if content_length > BODY_BYTES_MAX {
        return Ok(None);
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
    Ok(Some((method, path, headers, body)))
}

fn respond(stream: &mut std::net::TcpStream, reply: &Reply) -> std::io::Result<()> {
    let (code, body) = match reply {
        Reply::Png(bytes) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: image/png\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                bytes.len()
            );
            stream.write_all(head.as_bytes())?;
            stream.write_all(bytes)?;
            return stream.flush();
        }
        Reply::Json(code, body) => (*code, body),
    };
    let text = body.to_string();
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        422 => "Unprocessable Entity",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        text.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Minimal base64 encoder (standard alphabet, padded).
/// Exercised by the round-trip test; the wire path only decodes.
#[cfg(test)]
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Minimal base64 decoder; `None` on any off-alphabet byte.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value_of(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some(u32::from(b - b'A')),
            b'a'..=b'z' => Some(u32::from(b - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(b - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|&&b| b == b'=').count();
        let mut n: u32 = 0;
        for (i, &b) in chunk.iter().enumerate() {
            let v = if b == b'=' { 0 } else { value_of(b)? };
            n |= v << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torogao_mobile_core::cast::PRIMA;
    use torogao_mobile_core::workflow::build_workflow;

    fn valid() -> (Value, String) {
        let w = build_workflow(&PRIMA, "city night, rain", 42);
        let hash = workflow_sha256(&w);
        (w, hash)
    }

    fn gated_facade() -> Arc<Facade> {
        Arc::new(Facade {
            jobs: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            engine_url: "http://127.0.0.1:1".to_string(),
            engine_service: "torogao-gen-seat.service".to_string(),
            outputs_dir: PathBuf::from("/tmp"),
            token: Some("test-token".to_string()),
        })
    }

    fn code_of(reply: Reply) -> u32 {
        match reply {
            Reply::Json(code, _) => code,
            Reply::Png(_) => 200,
        }
    }

    #[test]
    fn token_gate_refuses_without_the_header() {
        let facade = gated_facade();
        let headers = HashMap::new();
        let reply = route(&facade, "GET", "/facade/queue", &headers, &[]);
        assert_eq!(code_of(reply), 401);
    }

    #[test]
    fn token_gate_refuses_a_wrong_token() {
        let facade = gated_facade();
        let mut headers = HashMap::new();
        headers.insert("x-facade-token".to_string(), "wrong-token".to_string());
        let reply = route(&facade, "POST", "/facade/wake", &headers, &[]);
        assert_eq!(code_of(reply), 401);
    }

    #[test]
    fn token_gate_admits_the_header_and_health_stays_open() {
        let facade = gated_facade();
        let mut headers = HashMap::new();
        headers.insert("x-facade-token".to_string(), "test-token".to_string());
        let reply = route(&facade, "GET", "/facade/queue", &headers, &[]);
        assert_eq!(code_of(reply), 200);
        let no_headers = HashMap::new();
        let reply = route(&facade, "GET", "/facade/health", &no_headers, &[]);
        assert_eq!(code_of(reply), 200);
    }

    #[test]
    fn a_canonical_workflow_validates() {
        let (w, hash) = valid();
        let job = validate_workflow(&w, Some(&hash)).expect("must validate");
        assert_eq!(job.seed, 42);
        assert_eq!(job.steps, style::STEPS as u64);
        assert!(job.positive.contains("silver-white hair"));
    }

    #[test]
    fn a_tampered_workflow_fails_the_hash() {
        let (mut w, hash) = valid();
        w["2"]["inputs"]["text"] = json!("something else entirely");
        let err = validate_workflow(&w, Some(&hash)).expect_err("tamper must fail");
        assert_eq!(err.0, 400);
        assert!(err.1.contains("hash"));
    }

    #[test]
    fn a_missing_hash_is_refused() {
        let (w, _) = valid();
        assert!(validate_workflow(&w, None).is_err());
    }

    #[test]
    fn a_foreign_checkpoint_is_refused() {
        let (mut w, _) = valid();
        w["1"]["inputs"]["ckpt_name"] = json!("someone_elses_model.safetensors");
        let hash = workflow_sha256(&w);
        let err = validate_workflow(&w, Some(&hash)).expect_err("foreign ckpt must fail");
        assert!(err.1.contains("checkpoint"));
    }

    #[test]
    fn a_screened_prompt_is_refused_with_422() {
        let w = build_workflow(&PRIMA, "schoolgirl, studio portrait", 42);
        let hash = workflow_sha256(&w);
        let err = validate_workflow(&w, Some(&hash)).expect_err("screen must refuse");
        assert_eq!(err.0, 422);
    }

    #[test]
    fn out_of_bounds_recipe_values_are_refused() {
        let (mut w, _) = valid();
        w["5"]["inputs"]["steps"] = json!(500);
        let hash = workflow_sha256(&w);
        let err = validate_workflow(&w, Some(&hash)).expect_err("steps must be bounded");
        assert_eq!(err.0, 400);
    }

    #[test]
    fn a_foreign_sampler_is_refused() {
        let (mut w, _) = valid();
        w["5"]["inputs"]["sampler_name"] = json!("ddim");
        let hash = workflow_sha256(&w);
        let err = validate_workflow(&w, Some(&hash)).expect_err("sampler must be the recipe's");
        assert!(err.1.contains("sampler"));
    }

    #[test]
    fn base64_round_trips() {
        for data in [
            vec![],
            vec![1],
            vec![1, 2],
            vec![1, 2, 3],
            (0u8..=255).collect(),
        ] {
            if data.is_empty() {
                continue; // the empty string decodes to None by design (no image)
            }
            assert_eq!(base64_decode(&base64_encode(&data)), Some(data));
        }
        assert_eq!(base64_decode("not base64!!"), None);
    }

    #[test]
    fn next_id_seeds_above_existing_outputs() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("tmg-facade-seed-test-{nanos}"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        assert_eq!(seed_next_id(&dir), 1);
        std::fs::write(dir.join("torogao_mobile_facade-3.png"), b"x").expect("plant");
        std::fs::write(dir.join("torogao_mobile_facade-17.png"), b"x").expect("plant");
        std::fs::write(dir.join("unrelated.png"), b"x").expect("plant");
        assert_eq!(seed_next_id(&dir), 18);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn percent_decoding_handles_filenames() {
        assert_eq!(
            percent_decode("torogao_mobile_facade-1.png"),
            "torogao_mobile_facade-1.png"
        );
        assert_eq!(percent_decode("a%20b.png"), "a b.png");
    }
}
