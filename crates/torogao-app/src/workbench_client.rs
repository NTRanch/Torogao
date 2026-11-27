// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! The workbench's wire half: the governed facade client,
//! the edit-route queue probe, and the edit-driver runner.
//!
//! Security and honesty contracts:
//!
//! - The facade token is read from its single 0600 file of
//!   record and held in memory only. It is never printed,
//!   logged, persisted, or shown by `Debug` — the redaction
//!   test below guards this. A missing token file means
//!   submissions refuse with a named reason (health and
//!   queue reads stay open; the facade allows those).
//! - Origination goes ONLY through the facade's governed
//!   job contract. There is no code path here that speaks
//!   to the seat (ComfyUI :8189) for generation; the queue
//!   probe is a read-only GET so the UI can be honest about
//!   the shared edit queue.
//! - Edits run the primo-local driver (`edit-local.py`) as
//!   a subprocess — the same CLI the lanes use — so the
//!   workbench inherits its compositing and provenance
//!   prints rather than re-implementing them. The `--omni`
//!   flag is not constructible from this module: the enum
//!   has no variant for it (its proof strike failed as an
//!   edit tool, 2026-11-27).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::workbench::{CharacterRecord, RecipeSettings};

/// Default facade base URL (loopback, governed door).
pub const DEFAULT_FACADE_BASE: &str = "http://127.0.0.1:8190";
/// Default seat base URL — used ONLY for the read-only
/// queue probe that drives the UI's honesty display.
pub const DEFAULT_COMFY_BASE: &str = "http://127.0.0.1:8189";
/// Where the facade token lives (0600, written by the
/// facade at first boot). The value is never displayed.
pub const DEFAULT_TOKEN_PATH: &str =
    "/home/phaedrus/.local/share/torogao-mobile-facade/facade-token";
/// Where the primo-local edit route lives on primo.
pub const DEFAULT_DRIVER_DIR: &str =
    "/home/phaedrus/.local/share/pax/primo-local-edit-route-20261127";

/// Workbench client configuration. Built from the
/// environment with the defaults above.
#[derive(Clone)]
pub struct WbConfig {
    /// Facade base URL, no trailing slash.
    pub facade_base: String,
    /// Seat base URL for the queue probe, no trailing slash.
    pub comfy_base: String,
    /// The facade token, in memory only. `None` when the
    /// token file is absent (submissions then refuse).
    token: Option<String>,
    /// Directory holding `edit-local.py`.
    pub driver_dir: PathBuf,
}

impl std::fmt::Debug for WbConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WbConfig")
            .field("facade_base", &self.facade_base)
            .field("comfy_base", &self.comfy_base)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("driver_dir", &self.driver_dir)
            .finish()
    }
}

impl WbConfig {
    /// Build the configuration from the environment:
    /// `TOROGAO_WB_FACADE`, `TOROGAO_WB_COMFY`,
    /// `TOROGAO_WB_TOKEN_FILE`, `TOROGAO_WB_DRIVER_DIR`
    /// override the defaults.
    pub fn from_env() -> Self {
        let facade_base = env_or("TOROGAO_WB_FACADE", DEFAULT_FACADE_BASE);
        let comfy_base = env_or("TOROGAO_WB_COMFY", DEFAULT_COMFY_BASE);
        let token_path = env_or("TOROGAO_WB_TOKEN_FILE", DEFAULT_TOKEN_PATH);
        let token = std::fs::read_to_string(&token_path)
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let driver_dir = PathBuf::from(env_or("TOROGAO_WB_DRIVER_DIR", DEFAULT_DRIVER_DIR));
        Self {
            facade_base: facade_base.trim_end_matches('/').to_string(),
            comfy_base: comfy_base.trim_end_matches('/').to_string(),
            token,
            driver_dir,
        }
    }

    /// True when a facade token is held (submissions can be
    /// attempted; the facade still validates it).
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// The driver script path.
    pub fn driver_path(&self) -> PathBuf {
        self.driver_dir.join("edit-local.py")
    }

    /// True when the edit driver is present on this machine.
    pub fn driver_present(&self) -> bool {
        self.driver_path().is_file()
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

// ---------------------------------------------------------------------
// Minimal HTTP/1.1 client over std TcpStream (estate idiom: the
// facade itself is hand-rolled std HTTP; no async runtime, no new
// dependency for a loopback JSON conversation).
// ---------------------------------------------------------------------

/// One HTTP response: status plus raw body bytes.
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body (de-chunked when the server chunks).
    pub body: Vec<u8>,
}

/// Issue one HTTP request and read the full response.
/// `url` must be `http://host:port/path` (loopback only in
/// practice). Timeouts bound connect and each read.
pub fn http_request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<HttpResponse, String> {
    let (host_port, path) = parse_http_url(url)?;
    let mut stream =
        TcpStream::connect(&host_port).map_err(|e| format!("connect {host_port}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set read timeout: {e}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| format!("set write timeout: {e}"))?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n");
    if let Some(b) = body {
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("write request: {e}"))?;
    if let Some(b) = body {
        stream
            .write_all(b)
            .map_err(|e| format!("write body: {e}"))?;
    }
    // Read to EOF in slices: SO_RCVTIMEO surfaces a slow peer
    // as EAGAIN (os error 11), which is a retry signal inside
    // the overall budget, not a failure — the first live proof
    // run died on exactly that under load.
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| format!("set read slice: {e}"))?;
    let deadline = std::time::Instant::now() + timeout;
    let mut raw = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if response_complete(&raw) {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    return Err("read response: budget spent before EOF".to_string());
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if std::time::Instant::now() >= deadline {
                    return Err(format!(
                        "read response: no complete response within                          {timeout:?}"
                    ));
                }
            }
            Err(e) => return Err(format!("read response: {e}")),
        }
    }
    parse_http_response(&raw)
}

/// True when `raw` holds a complete response: the header
/// block plus Content-Length body bytes, or a chunked body
/// carrying its terminal chunk. A response with neither
/// completes only at EOF (handled by the read loop).
fn response_complete(raw: &[u8]) -> bool {
    let Some(header_end) = find_subslice(raw, b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&raw[..header_end]).to_ascii_lowercase();
    let body_len = raw.len() - (header_end + 4);
    if head.contains("transfer-encoding: chunked") {
        let body = &raw[header_end + 4..];
        return body.ends_with(b"0\r\n\r\n") || body.ends_with(b"0\r\n");
    }
    for line in head.lines() {
        if let Some(v) = line.strip_prefix("content-length:") {
            if let Ok(want) = v.trim().parse::<usize>() {
                return body_len >= want;
            }
        }
    }
    false
}

fn parse_http_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// URLs are supported: {url}"))?;
    match rest.find('/') {
        Some(i) => Ok((rest[..i].to_string(), rest[i..].to_string())),
        None => Ok((rest.to_string(), "/".to_string())),
    }
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, String> {
    let header_end = find_subslice(raw, b"\r\n\r\n")
        .ok_or_else(|| "response has no header terminator".to_string())?;
    let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("unparseable status line: {status_line}"))?;
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    let mut body = raw[header_end + 4..].to_vec();
    if chunked {
        body = dechunk(&body)?;
    }
    Ok(HttpResponse { status, body })
}

fn dechunk(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut rest = data;
    loop {
        let line_end =
            find_subslice(rest, b"\r\n").ok_or_else(|| "chunked body: no size line".to_string())?;
        let size_text = String::from_utf8_lossy(&rest[..line_end]).to_string();
        let size = usize::from_str_radix(size_text.trim(), 16)
            .map_err(|e| format!("chunked body: bad size {size_text:?}: {e}"))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size + 2 {
            return Err("chunked body: truncated chunk".to_string());
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
    Ok(out)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------
// The facade client (origination + seat queue honesty).
// ---------------------------------------------------------------------

/// Seat/facade health as the facade reports it.
#[derive(Debug, Clone, PartialEq)]
pub enum SeatHealth {
    /// Facade answered: engine + seat state strings.
    Up {
        /// Engine name ("torogao-gen").
        engine: String,
        /// Seat state ("ready", "absent", ...).
        seat: String,
    },
    /// Facade did not answer (message says why).
    Down(String),
}

/// The facade's own job queue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FacadeQueue {
    /// Jobs waiting.
    pub queued: usize,
    /// A job is running.
    pub running: bool,
}

/// The outcome of polling a submitted job's history.
#[derive(Debug, Clone, PartialEq)]
pub enum JobPoll {
    /// Still queued or running (or unknown id).
    Pending,
    /// Finished: the seat-side output file name.
    Done(String),
    /// The seat reported an error (message included).
    Failed(String),
}

/// Origination client: speaks only the facade's governed
/// contract. Construct one per job; it is cheap.
pub struct FacadeClient {
    cfg: WbConfig,
}

impl FacadeClient {
    /// A client over `cfg`.
    pub fn new(cfg: WbConfig) -> Self {
        Self { cfg }
    }

    /// The token header set for governed routes. The facade
    /// requires its token on every route except health —
    /// wake refused with HTTP 401 until it was sent
    /// (verified live 2026-11-27) — so every non-health
    /// call sends the held token when present.
    fn auth_headers(&self) -> Vec<(String, String)> {
        match &self.cfg.token {
            Some(t) => vec![("X-Facade-Token".to_string(), t.clone())],
            None => Vec::new(),
        }
    }

    /// GET /facade/health — open route, no token.
    pub fn health(&self) -> SeatHealth {
        let url = format!("{}/facade/health", self.cfg.facade_base);
        match http_request("GET", &url, &[], None, Duration::from_secs(90)) {
            Ok(resp) if resp.status == 200 => {
                let v: serde_json::Value = match serde_json::from_slice(&resp.body) {
                    Ok(v) => v,
                    Err(e) => return SeatHealth::Down(format!("health JSON: {e}")),
                };
                SeatHealth::Up {
                    engine: v["engine"].as_str().unwrap_or("?").to_string(),
                    seat: v["seat"].as_str().unwrap_or("?").to_string(),
                }
            }
            Ok(resp) => SeatHealth::Down(format!("health HTTP {}", resp.status)),
            Err(e) => SeatHealth::Down(e),
        }
    }

    /// POST /facade/wake — ask the facade to load the seat.
    pub fn wake(&self) -> Result<(), String> {
        let url = format!("{}/facade/wake", self.cfg.facade_base);
        let resp = http_request(
            "POST",
            &url,
            &self.auth_headers(),
            None,
            Duration::from_secs(120),
        )?;
        if resp.status == 200 {
            Ok(())
        } else {
            Err(format!(
                "wake refused: HTTP {} {}",
                resp.status,
                String::from_utf8_lossy(&resp.body)
            ))
        }
    }

    /// GET /facade/queue — the facade queue depth. The facade
    /// requires its token on this route (verified live 2026-11-27:
    /// an unauthenticated read is refused), so the held token
    /// is sent when present.
    pub fn queue(&self) -> Result<FacadeQueue, String> {
        let url = format!("{}/facade/queue", self.cfg.facade_base);
        let mut headers: Vec<(String, String)> = Vec::new();
        if let Some(token) = &self.cfg.token {
            headers.push(("X-Facade-Token".to_string(), token.clone()));
        }
        let resp = http_request("GET", &url, &headers, None, Duration::from_secs(90))?;
        if resp.status != 200 {
            return Err(format!("queue HTTP {}", resp.status));
        }
        let v: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| format!("queue JSON: {e}"))?;
        Ok(FacadeQueue {
            queued: v["queued"].as_u64().unwrap_or(0) as usize,
            running: v["running"].as_bool().unwrap_or(false),
        })
    }

    /// Submit one origination job under the governed
    /// contract: canonical workflow bytes + their SHA-256 +
    /// the facade token. Returns the seat's prompt id.
    pub fn submit(
        &self,
        record: &CharacterRecord,
        user_prompt: &str,
        seed: u64,
        recipe: &RecipeSettings,
    ) -> Result<String, String> {
        let Some(token) = &self.cfg.token else {
            return Err(
                "no facade token held (token file absent) — submissions refuse; \
                 the facade must be started once so it writes its token"
                    .to_string(),
            );
        };
        let workflow = crate::workbench::build_workflow(record, user_prompt, seed, recipe)?;
        let hash = crate::workbench::workflow_sha256(&workflow);
        // The facade's /prompt takes an envelope: the body is
        // a JSON object whose "prompt" member is the workflow
        // document; the hash header covers that member alone
        // (facade main.rs, read 2026-11-27). Posting the bare
        // workflow was refused with "body has no prompt
        // document" in proof run 4.
        let envelope = serde_json::json!({ "prompt": workflow });
        let bytes = crate::workbench::canonical_workflow_bytes(&envelope);
        let url = format!("{}/prompt", self.cfg.facade_base);
        let headers = vec![
            ("X-Facade-Token".to_string(), token.clone()),
            ("X-Workflow-Sha256".to_string(), hash),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let resp = http_request(
            "POST",
            &url,
            &headers,
            Some(&bytes),
            Duration::from_secs(90),
        )?;
        let text = String::from_utf8_lossy(&resp.body).to_string();
        if resp.status != 200 {
            return Err(format!(
                "facade refused the job: HTTP {} {text}",
                resp.status
            ));
        }
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("submit JSON: {e}"))?;
        v["prompt_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("submit response carries no prompt_id: {text}"))
    }

    /// Poll a job's history in the facade's ComfyUI-shaped
    /// answer.
    pub fn poll(&self, prompt_id: &str) -> Result<JobPoll, String> {
        let url = format!("{}/history/{prompt_id}", self.cfg.facade_base);
        let resp = http_request(
            "GET",
            &url,
            &self.auth_headers(),
            None,
            Duration::from_secs(30),
        )?;
        if resp.status != 200 {
            return Err(format!("history HTTP {}", resp.status));
        }
        let v: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| format!("history JSON: {e}"))?;
        let Some(entry) = v.get(prompt_id) else {
            return Ok(JobPoll::Pending);
        };
        let status = &entry["status"];
        match status["status_str"].as_str() {
            Some("success") => {
                let name = entry["outputs"]["7"]["images"][0]["filename"]
                    .as_str()
                    .ok_or_else(|| "success history carries no output filename".to_string())?;
                Ok(JobPoll::Done(name.to_string()))
            }
            Some("error") => Ok(JobPoll::Failed(format!(
                "seat error: {}",
                status["messages"]
            ))),
            _ => Ok(JobPoll::Pending),
        }
    }

    /// Download a finished still's PNG bytes via /view.
    pub fn fetch_view(&self, filename: &str) -> Result<Vec<u8>, String> {
        let url = format!(
            "{}/view?filename={}",
            self.cfg.facade_base,
            url_encode(filename)
        );
        let resp = http_request(
            "GET",
            &url,
            &self.auth_headers(),
            None,
            Duration::from_secs(60),
        )?;
        if resp.status != 200 {
            return Err(format!(
                "view HTTP {}: {}",
                resp.status,
                String::from_utf8_lossy(&resp.body)
            ));
        }
        Ok(resp.body)
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------
// The edit-route queue probe (read-only) + driver runner.
// ---------------------------------------------------------------------

/// The seat queue the edit route shares, as the UI shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditQueue {
    /// Nothing running or pending — fire is allowed.
    Idle,
    /// Work is in flight — fire is disabled, honestly.
    Busy {
        /// Jobs executing now.
        running: usize,
        /// Jobs waiting.
        pending: usize,
    },
    /// The seat did not answer the probe.
    Unknown,
}

/// Probe the edit route's shared queue (GET /queue on the
/// seat). Read-only: this never submits anything.
pub fn probe_edit_queue(cfg: &WbConfig) -> EditQueue {
    let url = format!("{}/queue", cfg.comfy_base);
    let Ok(resp) = http_request("GET", &url, &[], None, Duration::from_secs(4)) else {
        return EditQueue::Unknown;
    };
    if resp.status != 200 {
        return EditQueue::Unknown;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp.body) else {
        return EditQueue::Unknown;
    };
    let running = v["queue_running"].as_array().map(|a| a.len()).unwrap_or(0);
    let pending = v["queue_pending"].as_array().map(|a| a.len()).unwrap_or(0);
    if running == 0 && pending == 0 {
        EditQueue::Idle
    } else {
        EditQueue::Busy { running, pending }
    }
}

/// The edit configuration the Owner can pick. Faithful is
/// the default and the only chain-safe route. Turbo is the
/// fast LoRA: labelled not chain-safe wherever it appears.
/// Omni is deliberately absent — not constructible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditConfig {
    /// Qwen-Image-Edit-2511 faithful path (default).
    Faithful,
    /// Turbo LoRA path: fast, NOT chain-safe.
    Turbo,
}

impl EditConfig {
    /// The candidate route label for this config.
    pub fn route_label(&self) -> &'static str {
        match self {
            EditConfig::Faithful => crate::workbench::ROUTE_EDIT_FAITHFUL,
            EditConfig::Turbo => crate::workbench::ROUTE_EDIT_TURBO,
        }
    }

    /// The UI label, with the chain-safety warning attached
    /// to turbo wherever the choice is offered.
    pub fn ui_label(&self) -> &'static str {
        match self {
            EditConfig::Faithful => "faithful (default, chain-safe)",
            EditConfig::Turbo => "turbo (fast, NOT chain-safe)",
        }
    }

    /// Build the driver's argv tail for this config. The
    /// mask, feather, and seed flags are appended by the
    /// runner; this function is the single place config
    /// flags are chosen, and it can never emit `--omni`.
    pub fn driver_flags(&self) -> Vec<String> {
        match self {
            EditConfig::Faithful => Vec::new(),
            EditConfig::Turbo => vec!["--turbo".to_string()],
        }
    }
}

/// One edit firing, fully specified.
pub struct EditSpec {
    /// Canvas PNG (the strike's base frame).
    pub canvas: PathBuf,
    /// The edit instruction text.
    pub prompt: String,
    /// Output path prefix (driver appends suffixes).
    pub out_prefix: PathBuf,
    /// Faithful or turbo.
    pub config: EditConfig,
    /// Optional explicit seed.
    pub seed: Option<i64>,
    /// Explicit mask PNG (white = take the edit). Mutually
    /// exclusive with the driver's auto-mask, which the
    /// workbench does not offer (it measured weak).
    pub mask: Option<PathBuf>,
    /// Edge feather in pixels (0 = hard edge).
    pub feather: u32,
}

/// What a finished edit produced.
#[derive(Debug)]
pub struct EditOutcome {
    /// The composited frame when a mask was given, else the
    /// raw frame. This is the gallery candidate.
    pub delivered: PathBuf,
    /// The raw full-frame edit, kept for provenance.
    pub raw: PathBuf,
    /// The driver's mask echo, when produced.
    pub mask_echo: Option<PathBuf>,
    /// The driver's stdout, for the record.
    pub stdout: String,
}

/// Run one edit through the driver CLI, exactly as the
/// lanes invoke it. Bounded: the child is polled and killed
/// past `timeout`.
pub fn run_edit(cfg: &WbConfig, spec: &EditSpec, timeout: Duration) -> Result<EditOutcome, String> {
    if spec.mask.is_none() && spec.feather > 0 {
        // The driver itself would refuse this; refuse earlier
        // with the same reason so the UI never fires it.
        return Err("--feather applies to masked edits only".to_string());
    }
    let driver = cfg.driver_path();
    if !driver.is_file() {
        return Err(format!("edit driver not found at {}", driver.display()));
    }
    let prompt_path = spec.out_prefix.with_extension("prompt.txt");
    if let Some(parent) = spec.out_prefix.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create job dir {}: {e}", parent.display()))?;
    }
    std::fs::write(&prompt_path, &spec.prompt).map_err(|e| format!("write prompt file: {e}"))?;

    let mut args: Vec<String> = vec![
        "edit".to_string(),
        spec.canvas.to_string_lossy().to_string(),
        prompt_path.to_string_lossy().to_string(),
        spec.out_prefix.to_string_lossy().to_string(),
    ];
    args.extend(spec.config.driver_flags());
    if let Some(seed) = spec.seed {
        args.push("--seed".to_string());
        args.push(seed.to_string());
    }
    if let Some(mask) = &spec.mask {
        args.push("--mask".to_string());
        args.push(mask.to_string_lossy().to_string());
    }
    if spec.feather > 0 {
        args.push("--feather".to_string());
        args.push(spec.feather.to_string());
    }

    let mut child = std::process::Command::new("python3")
        .arg(&driver)
        .args(&args)
        .current_dir(&cfg.driver_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn edit driver: {e}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "edit driver exceeded {}s and was killed",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(format!("wait on edit driver: {e}")),
        }
    };
    let output = child
        .wait_with_output()
        .map_err(|e| format!("collect edit output: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !status.success() {
        return Err(format!("edit driver failed ({status}): {stdout} {stderr}"));
    }
    // The raw frame is <prefix>_NNNNN_.png; the composited
    // frame is <prefix>_comp.png when a mask was given.
    let raw = find_raw_output(&spec.out_prefix)?;
    let comp = PathBuf::from(format!("{}_comp.png", spec.out_prefix.display()));
    let delivered = if spec.mask.is_some() && comp.is_file() {
        comp.clone()
    } else {
        raw.clone()
    };
    let mask_echo = PathBuf::from(format!("{}_mask.png", spec.out_prefix.display()));
    Ok(EditOutcome {
        delivered,
        raw,
        mask_echo: if mask_echo.is_file() {
            Some(mask_echo)
        } else {
            None
        },
        stdout,
    })
}

fn find_raw_output(prefix: &std::path::Path) -> Result<PathBuf, String> {
    let parent = prefix
        .parent()
        .ok_or_else(|| "out prefix has no parent dir".to_string())?;
    let stem = prefix
        .file_name()
        .ok_or_else(|| "out prefix has no file name".to_string())?
        .to_string_lossy()
        .to_string();
    let mut found: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(parent).map_err(|e| format!("scan job dir: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("scan job dir: {e}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&format!("{stem}_"))
            && name.ends_with("_.png")
            && !name.ends_with("_comp.png")
            && !name.ends_with("_mask.png")
        {
            found.push(entry.path());
        }
    }
    found.sort();
    found
        .into_iter()
        .next()
        .ok_or_else(|| format!("no raw output for prefix {}", prefix.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- validation-half tests -------------------------------------

    #[test]
    fn url_parse_splits_host_and_path() {
        let (host, path) = parse_http_url("http://127.0.0.1:8190/facade/health").unwrap();
        assert_eq!(host, "127.0.0.1:8190");
        assert_eq!(path, "/facade/health");
        let (host, path) = parse_http_url("http://127.0.0.1:8189").unwrap();
        assert_eq!(host, "127.0.0.1:8189");
        assert_eq!(path, "/");
    }

    #[test]
    fn response_parse_reads_status_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"{}");
    }

    #[test]
    fn dechunk_reassembles() {
        let body = dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap();
        assert_eq!(body, b"Wikipedia");
    }

    #[test]
    fn turbo_flags_and_label_carry_the_warning() {
        assert!(
            EditConfig::Turbo
                .driver_flags()
                .contains(&"--turbo".to_string())
        );
        assert!(EditConfig::Turbo.ui_label().contains("NOT chain-safe"));
        assert!(EditConfig::Faithful.driver_flags().is_empty());
    }

    // ---- adversarial-half tests ------------------------------------

    #[test]
    fn omni_is_not_constructible() {
        // The config enum is the whole surface: neither
        // variant's flags may ever carry --omni.
        for cfg in [EditConfig::Faithful, EditConfig::Turbo] {
            assert!(
                !cfg.driver_flags().iter().any(|f| f.contains("omni")),
                "{cfg:?} must never emit an omni flag"
            );
        }
    }

    #[test]
    fn token_is_redacted_in_debug() {
        let cfg = WbConfig {
            facade_base: DEFAULT_FACADE_BASE.to_string(),
            comfy_base: DEFAULT_COMFY_BASE.to_string(),
            token: Some("wb-test-token-value".to_string()),
            driver_dir: PathBuf::from("/tmp"),
        };
        let dbg = format!("{cfg:?}");
        assert!(
            !dbg.contains("wb-test-token-value"),
            "Debug leaked the token: {dbg}"
        );
        assert!(dbg.contains("<redacted>"));
    }

    #[test]
    fn feather_without_mask_refuses_before_spawning() {
        let cfg = WbConfig {
            facade_base: DEFAULT_FACADE_BASE.to_string(),
            comfy_base: DEFAULT_COMFY_BASE.to_string(),
            token: None,
            driver_dir: PathBuf::from("/nonexistent-wb-test"),
        };
        let spec = EditSpec {
            canvas: PathBuf::from("/tmp/canvas.png"),
            prompt: "x".to_string(),
            out_prefix: PathBuf::from("/tmp/wb-feather-test/edit"),
            config: EditConfig::Faithful,
            seed: None,
            mask: None,
            feather: 8,
        };
        let err = run_edit(&cfg, &spec, Duration::from_secs(1)).unwrap_err();
        assert!(err.contains("feather"), "unexpected error: {err}");
    }
}
