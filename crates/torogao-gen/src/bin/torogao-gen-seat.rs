//! torogao-gen-seat — the resident generation seat.
//!
//! Loads the wai checkpoint once and serves the bounded engine
//! contract the governed facade speaks:
//!
//! - `GET  /health`            → `{"engine":"torogao-gen","model_loaded":bool}`
//! - `POST /jobs`              → `202 {"id":"job-N"}` for a
//!   `{positive, negative, seed, steps, cfg, width, height,
//!   sampler, scheduler}` body; the recipe of record's sampler
//!   and scheduler names are the only ones this seat runs.
//! - `GET  /jobs/<id>`         → `{"state":…}` plus, when done,
//!   `image_png_base64`.
//! - `GET  /jobs`              → counts by state (the idle
//!   reaper's evidence).
//! - `POST /jobs/<id>/interrupt` → mark a job cancelled.
//!
//! One worker thread owns the model and runs jobs serially —
//! the A6000 seat is one GPU, and a queue of one deep is the
//! honest shape. Interrupting a RUNNING job cannot stop the
//! sampler mid-flight (torogao-gen's `generate` has no mid-run
//! hook today): the job is marked cancelled and its result is
//! discarded when generation completes. A QUEUED job marked
//! cancelled never runs.
//!
//! The seat binds loopback only. It is not the public surface —
//! the governed facade (torogao-mobile-facade) is, and it alone
//! decides what reaches this process.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use serde_json::{Value, json};
use torogao_gen::{GenParams, SamplerKind, SchedulerKind, SdxlModel};

/// Largest request body accepted (prompts are small; this is a
/// seat, not a file store).
const BODY_BYTES_MAX: usize = 256 * 1024;

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
    params: Option<GenParams>,
    image_png_base64: Option<String>,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
    elapsed_ms: Option<u64>,
    noise_sha256: Option<String>,
}

struct Seat {
    jobs: Mutex<HashMap<String, Job>>,
    next_id: Mutex<u64>,
    model_loaded: AtomicBool,
    engine_error: Mutex<Option<String>>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> Option<String> {
        args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
    };
    let checkpoint = arg("--checkpoint").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".HF/models/strobe-gen/waiIllustriousSDXL_v170.safetensors")
    });
    let tokenizers = arg("--tokenizers").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".local/share/torogao-mobile-seat/tokenizers")
    });
    let port: u16 = arg("--port").and_then(|p| p.parse().ok()).unwrap_or(8192);

    let seat = Arc::new(Seat {
        jobs: Mutex::new(HashMap::new()),
        next_id: Mutex::new(1),
        model_loaded: AtomicBool::new(false),
        engine_error: Mutex::new(None),
    });

    // The worker thread loads the model, then runs jobs serially.
    let (tx, rx) = mpsc::channel::<String>();
    {
        let seat = Arc::clone(&seat);
        std::thread::spawn(move || {
            let device = match candle::Device::new_cuda(0) {
                Ok(d) => d,
                Err(e) => {
                    *seat.engine_error.lock().unwrap() =
                        Some(format!("no CUDA device for the seat: {e}"));
                    return;
                }
            };
            let model = match SdxlModel::load_single_file(&checkpoint, &tokenizers, &device, &[]) {
                Ok(m) => m,
                Err(e) => {
                    *seat.engine_error.lock().unwrap() =
                        Some(format!("checkpoint load failed: {e}"));
                    return;
                }
            };
            seat.model_loaded.store(true, Ordering::SeqCst);
            eprintln!("torogao-gen-seat: model loaded, serving jobs");
            while let Ok(id) = rx.recv() {
                run_one(&seat, &model, &id);
            }
        });
    }

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("torogao-gen-seat: cannot bind 127.0.0.1:{port}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("torogao-gen-seat: listening on 127.0.0.1:{port}");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let seat = Arc::clone(&seat);
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = handle(&mut stream, &seat, &tx);
        });
    }
}

fn run_one(seat: &Seat, model: &SdxlModel, id: &str) {
    let (params, cancel) = {
        let mut jobs = seat.jobs.lock().unwrap();
        let Some(job) = jobs.get_mut(id) else { return };
        if job.state != JobState::Queued || job.cancel.load(Ordering::SeqCst) {
            job.state = JobState::Cancelled;
            return;
        }
        job.state = JobState::Running;
        (job.params.clone(), Arc::clone(&job.cancel))
    };
    let Some(params) = params else { return };
    let outcome = model.generate(&params);
    let mut jobs = seat.jobs.lock().unwrap();
    let Some(job) = jobs.get_mut(id) else { return };
    if cancel.load(Ordering::SeqCst) {
        job.state = JobState::Cancelled;
        job.params = None;
        return;
    }
    match outcome {
        Ok(image) => {
            let mut png = Vec::new();
            let encoded = image::DynamicImage::ImageRgb8(
                image::ImageBuffer::from_raw(image.width, image.height, image.rgb)
                    .expect("generated rgb buffer matches its dimensions"),
            )
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            match encoded {
                Ok(()) => {
                    job.image_png_base64 = Some(base64_encode(&png));
                    job.elapsed_ms = Some(image.elapsed_ms);
                    job.noise_sha256 = Some(image.noise_sha256);
                    job.state = JobState::Done;
                }
                Err(e) => {
                    let msg = format!("png encode failed: {e}");
                    job.error = Some(msg.clone());
                    job.state = JobState::Error(msg);
                }
            }
        }
        Err(e) => {
            job.error = Some(e.to_string());
            job.state = JobState::Error(e.to_string());
        }
    }
    job.params = None;
}

fn handle(
    stream: &mut std::net::TcpStream,
    seat: &Seat,
    tx: &mpsc::Sender<String>,
) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    // Read until the header ends, then exactly content-length more.
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
        if buf.len() > BODY_BYTES_MAX {
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
            let loaded = seat.model_loaded.load(Ordering::SeqCst);
            let error = seat.engine_error.lock().unwrap().clone();
            respond(
                stream,
                200,
                &json!({"engine": "torogao-gen", "model_loaded": loaded, "error": error}),
            )
        }
        ("GET", "/jobs") => {
            let jobs = seat.jobs.lock().unwrap();
            let mut counts = HashMap::new();
            for job in jobs.values() {
                *counts.entry(job.state.name()).or_insert(0u64) += 1;
            }
            respond(stream, 200, &json!({"counts": counts}))
        }
        ("POST", "/jobs") => submit(stream, seat, tx, &body),
        ("GET", p) if p.starts_with("/jobs/") => {
            let id = &p["/jobs/".len()..];
            poll(stream, seat, id)
        }
        ("POST", p) if p.starts_with("/jobs/") && p.ends_with("/interrupt") => {
            let id = &p["/jobs/".len()..p.len() - "/interrupt".len()];
            let jobs = seat.jobs.lock().unwrap();
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
    stream: &mut std::net::TcpStream,
    seat: &Seat,
    tx: &mpsc::Sender<String>,
    body: &[u8],
) -> std::io::Result<()> {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return respond(stream, 400, &json!({"error": format!("bad json: {e}")})),
    };
    let params = match parse_params(&parsed) {
        Ok(p) => p,
        Err(e) => return respond(stream, 400, &json!({"error": e})),
    };
    if let Err(e) = params.validate() {
        return respond(stream, 400, &json!({"error": e.to_string()}));
    }
    let id = {
        let mut next = seat.next_id.lock().unwrap();
        let id = format!("job-{}", *next);
        *next += 1;
        id
    };
    seat.jobs.lock().unwrap().insert(
        id.clone(),
        Job {
            state: JobState::Queued,
            params: Some(params),
            image_png_base64: None,
            error: None,
            cancel: Arc::new(AtomicBool::new(false)),
            elapsed_ms: None,
            noise_sha256: None,
        },
    );
    let _ = tx.send(id.clone());
    respond(stream, 202, &json!({"id": id}))
}

fn poll(stream: &mut std::net::TcpStream, seat: &Seat, id: &str) -> std::io::Result<()> {
    let jobs = seat.jobs.lock().unwrap();
    let Some(job) = jobs.get(id) else {
        return respond(stream, 404, &json!({"error": "no such job"}));
    };
    let mut out = json!({
        "id": id,
        "state": job.state.name(),
        "error": job.error,
        "elapsed_ms": job.elapsed_ms,
        "noise_sha256": job.noise_sha256,
    });
    if let Some(b64) = &job.image_png_base64 {
        out["image_png_base64"] = Value::String(b64.clone());
    }
    respond(stream, 200, &out)
}

/// Parse the engine contract's job body. Only the recipe of
/// record's sampler/scheduler names are accepted — this seat
/// runs the recipe, not arbitrary configurations.
fn parse_params(body: &Value) -> Result<GenParams, String> {
    let s = |key: &str| -> Result<String, String> {
        body.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("missing string field {key}"))
    };
    let u = |key: &str| -> Result<u64, String> {
        body.get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("missing integer field {key}"))
    };
    let sampler = match s("sampler")?.as_str() {
        "euler_ancestral" => SamplerKind::EulerAncestral,
        "dpmpp_2m" => SamplerKind::Dpmpp2m,
        other => return Err(format!("unsupported sampler {other}")),
    };
    let scheduler = match s("scheduler")?.as_str() {
        "normal" => SchedulerKind::Normal,
        "karras" => SchedulerKind::Karras,
        other => return Err(format!("unsupported scheduler {other}")),
    };
    Ok(GenParams {
        prompt: s("positive")?,
        negative: s("negative")?,
        seed: u("seed")?,
        steps: u("steps")? as usize,
        sampler,
        scheduler,
        cfg: body
            .get("cfg")
            .and_then(Value::as_f64)
            .ok_or_else(|| "missing number field cfg".to_string())? as f32,
        width: u("width")? as u32,
        height: u("height")? as u32,
    })
}

fn respond(stream: &mut std::net::TcpStream, code: u32, body: &Value) -> std::io::Result<()> {
    let text = body.to_string();
    let reason = match code {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
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

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Minimal base64 encoder (standard alphabet, padded) — the
/// seat's loopback contract carries the PNG inline.
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
