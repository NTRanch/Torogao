//! The seat client.
//!
//! Blocking HTTP client for the governed facade in front of the
//! generation engine (torogao-gen is the engine of record,
//! D-867/D-968; the facade is engine-agnostic, so an on-device
//! engine can register behind the same contract). The facade
//! speaks the ComfyUI-shaped contract this client was built
//! against — submit a workflow document, poll its history,
//! fetch the image — plus the facade's own health/wake surface.
//!
//! The seat behind the facade is started on demand and reaped
//! when idle, so "seat unreachable" and "seat starting" are
//! first-class outcomes the shells present, not crashes. All
//! work is bounded: connect/read timeouts on every call, a hard
//! deadline on history polling, and a cap on the fetched image
//! size. Submissions carry the workflow's SHA-256; the facade
//! recomputes it and refuses a document altered in transit
//! (the approval/hash discipline). When the seat's settings
//! carry a facade token, every gated route also carries it as
//! the `X-Facade-Token` header — the client half of the facade's
//! token gate (the health route is exempt facade-side).

use std::fmt;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::cast::Character;
use crate::gallery::{Gallery, GalleryEntry};
use crate::screen::{ScreenVerdict, screen_text};
use crate::style::RecipeSettings;
use crate::workflow::{USER_PROMPT_BYTES_MAX, build_workflow_with, workflow_sha256};

/// Connect timeout for any seat call.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Read timeout for submit/history calls.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Read timeout for the image fetch (stills are a few MB).
pub const IMAGE_READ_TIMEOUT: Duration = Duration::from_secs(120);
/// Delay between history polls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Hard deadline for one generation, in seconds (10 minutes covers
/// a cold seat start plus a slow queue; past it the job is given
/// up, never polled forever).
pub const POLL_DEADLINE_SECS: u64 = 600;
/// Largest image the client will file, in bytes (64 MiB).
pub const IMAGE_BYTES_MAX: u64 = 64 * 1024 * 1024;

/// The seat's state, as the facade reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatStatus {
    /// The engine is loaded and accepting jobs.
    Ready {
        /// Engine id behind the facade ("torogao-gen", …).
        engine: String,
    },
    /// The engine is starting (model loading); retry shortly.
    Starting {
        /// Engine id behind the facade.
        engine: String,
    },
    /// The facade is up; the engine is down. A wake (or a
    /// submission, which wakes on demand) starts it.
    Down {
        /// Engine id behind the facade.
        engine: String,
    },
    /// Nothing answered at the seat URL at all.
    Unreachable(String),
}

impl SeatStatus {
    /// A plain-language line for the shells and helper flows.
    pub fn describe(&self) -> String {
        match self {
            Self::Ready { engine } => format!("Seat ready ({engine})."),
            Self::Starting { engine } => {
                format!("Seat is starting ({engine}) — this can take a minute.")
            }
            Self::Down { engine } => {
                format!("Seat is asleep ({engine}). Generating will wake it.")
            }
            Self::Unreachable(m) => format!("No seat answered: {m}"),
        }
    }
}

/// Failures of a generation request. Every variant is presentable:
/// the shells show the message and keep running.
#[derive(Debug)]
pub enum GenError {
    /// The seat could not be reached at all (down, reaped, wrong URL).
    SeatUnreachable(String),
    /// The seat refused or failed the submission.
    Submit(String),
    /// The seat reported an error, or answered off-protocol.
    Seat(String),
    /// The job did not finish before [`POLL_DEADLINE_SECS`].
    Timeout,
    /// The job finished but produced no image.
    NoImage,
    /// The request itself was invalid before any seat contact.
    BadRequest(String),
    /// The content screen refused the prompt (client-side or
    /// seat-side; the message names the rule that fired).
    ScreenRefused(String),
    /// The 18+ attestation has not been given; generation is
    /// gated on it.
    AgeGate,
    /// The user cancelled the job.
    Cancelled,
    /// The local gallery could not be read or written.
    Gallery(String),
    /// Local state (settings, files) could not be read or written.
    Local(String),
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SeatUnreachable(m) => write!(f, "generation seat unreachable: {m}"),
            Self::Submit(m) => write!(f, "seat refused the job: {m}"),
            Self::Seat(m) => write!(f, "seat error: {m}"),
            Self::Timeout => {
                write!(f, "generation did not finish within {POLL_DEADLINE_SECS}s")
            }
            Self::NoImage => write!(f, "generation finished without an image"),
            Self::BadRequest(m) => write!(f, "bad request: {m}"),
            Self::ScreenRefused(m) => write!(f, "refused by the content screen: {m}"),
            Self::AgeGate => {
                write!(f, "generation needs the 18+ attestation first")
            }
            Self::Cancelled => write!(f, "generation cancelled"),
            Self::Gallery(m) => write!(f, "gallery error: {m}"),
            Self::Local(m) => write!(f, "local error: {m}"),
        }
    }
}

impl std::error::Error for GenError {}

impl From<std::io::Error> for GenError {
    fn from(e: std::io::Error) -> Self {
        Self::Gallery(format!("local file error: {e}"))
    }
}

/// A client bound to one facade base URL (e.g.
/// `http://127.0.0.1:8190` on primo, `http://100.x.y.z:8190`
/// over the tailnet).
#[derive(Debug, Clone)]
pub struct SeatClient {
    base_url: String,
    /// The facade token from the seat's settings, if any.
    token: Option<String>,
}

impl SeatClient {
    /// Create a client for the facade at `base_url` (no trailing
    /// slash needed; one is tolerated and stripped). No token:
    /// right for a facade whose token gate is off.
    pub fn new(base_url: &str) -> Self {
        Self::with_token(base_url, None)
    }

    /// Create a client that sends `token` as the
    /// `X-Facade-Token` header on every gated route (all but the
    /// facade's health route, which is exempt). A `None` or blank
    /// token sends no header. The token comes from the seat's
    /// settings — the client half of the facade's token gate
    /// (the activation contract the engine lane published,
    /// 2026-11-26).
    pub fn with_token(base_url: &str, token: Option<String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.filter(|t| !t.trim().is_empty()),
        }
    }

    /// Apply the facade token header to a request, when this
    /// client carries one.
    fn authed(&self, req: ureq::Request) -> ureq::Request {
        match &self.token {
            Some(token) => req.set("X-Facade-Token", token),
            None => req,
        }
    }

    /// The facade base URL this client talks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .timeout_write(READ_TIMEOUT)
            .build()
    }

    /// Ask the facade for the seat's state. Never panics: any
    /// transport failure is [`SeatStatus::Unreachable`].
    pub fn health(&self) -> SeatStatus {
        let url = format!("{}/facade/health", self.base_url);
        match Self::agent().get(&url).call() {
            Ok(r) => match r.into_json::<Value>() {
                Ok(body) => parse_health(&body),
                Err(e) => SeatStatus::Unreachable(format!("unreadable health reply: {e}")),
            },
            Err(ureq::Error::Transport(t)) => SeatStatus::Unreachable(t.to_string()),
            Err(ureq::Error::Status(code, _)) => {
                SeatStatus::Unreachable(format!("health HTTP {code}"))
            }
        }
    }

    /// Ask the facade to wake the engine. Returns the seat state
    /// after the request (typically `Starting`).
    pub fn wake(&self) -> SeatStatus {
        let url = format!("{}/facade/wake", self.base_url);
        match self
            .authed(Self::agent().post(&url))
            .send_json(serde_json::json!({}))
        {
            Ok(r) => match r.into_json::<Value>() {
                Ok(body) => parse_health(&body),
                Err(e) => SeatStatus::Unreachable(format!("unreadable wake reply: {e}")),
            },
            Err(ureq::Error::Transport(t)) => SeatStatus::Unreachable(t.to_string()),
            Err(ureq::Error::Status(code, _)) => {
                SeatStatus::Unreachable(format!("wake HTTP {code}"))
            }
        }
    }

    /// Best-effort interrupt of a running job at the seat. Used
    /// when a generation is cancelled; failure is not an error
    /// worth surfacing (the job's poll loop has already stopped).
    pub fn interrupt(&self, prompt_id: &str) {
        let url = format!("{}/interrupt", self.base_url);
        let _ = self
            .authed(Self::agent().post(&url))
            .send_json(serde_json::json!({ "prompt_id": prompt_id }));
    }

    /// Generate one still of `character` with the user's scene
    /// prompt and file it in `gallery`, using the recipe of
    /// record. `seed: None` derives a seed from the clock; the
    /// filed entry records the seed either way, so any still can
    /// be reproduced at the seat.
    pub fn generate(
        &self,
        character: &Character,
        user_prompt: &str,
        seed: Option<u64>,
        gallery: &Gallery,
    ) -> Result<GalleryEntry, GenError> {
        self.generate_with(
            character,
            user_prompt,
            seed,
            &RecipeSettings::of_record(),
            gallery,
            None,
        )
    }

    /// Generate one still with explicit recipe settings and an
    /// optional cancellation flag (checked between history
    /// polls; cancelling also interrupts the job at the seat).
    pub fn generate_with(
        &self,
        character: &Character,
        user_prompt: &str,
        seed: Option<u64>,
        recipe: &RecipeSettings,
        gallery: &Gallery,
        cancel: Option<&AtomicBool>,
    ) -> Result<GalleryEntry, GenError> {
        if user_prompt.len() > USER_PROMPT_BYTES_MAX {
            return Err(GenError::BadRequest(format!(
                "scene prompt is {} bytes; the limit is {USER_PROMPT_BYTES_MAX}",
                user_prompt.len()
            )));
        }
        if let ScreenVerdict::Refuse(reason) = screen_text(user_prompt) {
            return Err(GenError::ScreenRefused(reason));
        }
        if let Err(reason) = recipe.validate() {
            return Err(GenError::BadRequest(reason));
        }
        let seed = seed.unwrap_or_else(derive_seed);
        let workflow = build_workflow_with(character, user_prompt, seed, recipe);
        let prompt_id = self.submit(&workflow)?;
        let image_ref = self.await_image(&prompt_id, cancel)?;
        let bytes = self.fetch_image(&image_ref)?;
        let created_unix = unix_now();
        gallery.add(&bytes, character.id, user_prompt, seed, created_unix)
    }

    fn submit(&self, workflow: &Value) -> Result<String, GenError> {
        let url = format!("{}/prompt", self.base_url);
        let req = Self::agent()
            .post(&url)
            .set("X-Workflow-Sha256", &workflow_sha256(workflow));
        let resp = self
            .authed(req)
            .send_json(serde_json::json!({ "prompt": workflow }));
        match resp {
            Ok(r) => {
                let body: Value = r
                    .into_json()
                    .map_err(|e| GenError::Submit(format!("unreadable reply: {e}")))?;
                body.get("prompt_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| GenError::Submit(format!("reply has no prompt_id: {body}")))
            }
            Err(ureq::Error::Transport(t)) => Err(GenError::SeatUnreachable(t.to_string())),
            Err(ureq::Error::Status(422, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(GenError::ScreenRefused(body))
            }
            Err(ureq::Error::Status(code, r)) => Err(GenError::Submit(format!(
                "HTTP {code}: {}",
                r.into_string().unwrap_or_default()
            ))),
        }
    }

    /// Poll history until the job completes; returns the first
    /// output image's (filename, subfolder, type).
    fn await_image(
        &self,
        prompt_id: &str,
        cancel: Option<&AtomicBool>,
    ) -> Result<(String, String, String), GenError> {
        let url = format!("{}/history/{prompt_id}", self.base_url);
        let deadline = unix_now() + POLL_DEADLINE_SECS;
        loop {
            if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                self.interrupt(prompt_id);
                return Err(GenError::Cancelled);
            }
            let resp = self.authed(Self::agent().get(&url)).call();
            match resp {
                Ok(r) => {
                    let body: Value = r
                        .into_json()
                        .map_err(|e| GenError::Seat(format!("unreadable history: {e}")))?;
                    if let Some(image) = find_completed_image(&body, prompt_id) {
                        return image;
                    }
                    if let Some(msg) = find_error(&body, prompt_id) {
                        return Err(GenError::Seat(msg));
                    }
                }
                Err(ureq::Error::Transport(t)) => {
                    return Err(GenError::SeatUnreachable(t.to_string()));
                }
                Err(ureq::Error::Status(code, _)) => {
                    return Err(GenError::Seat(format!("history HTTP {code}")));
                }
            }
            if unix_now() >= deadline {
                return Err(GenError::Timeout);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn fetch_image(&self, image_ref: &(String, String, String)) -> Result<Vec<u8>, GenError> {
        let (filename, subfolder, kind) = image_ref;
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(IMAGE_READ_TIMEOUT)
            .build();
        let req = agent
            .get(&format!("{}/view", self.base_url))
            .query("filename", filename)
            .query("subfolder", subfolder)
            .query("type", kind);
        let resp = self.authed(req).call();
        match resp {
            Ok(r) => {
                let mut buf = Vec::new();
                r.into_reader()
                    .take(IMAGE_BYTES_MAX + 1)
                    .read_to_end(&mut buf)
                    .map_err(|e| GenError::Seat(format!("image download failed: {e}")))?;
                if buf.len() as u64 > IMAGE_BYTES_MAX {
                    return Err(GenError::Seat(format!(
                        "image exceeds the {IMAGE_BYTES_MAX}-byte cap"
                    )));
                }
                Ok(buf)
            }
            Err(ureq::Error::Transport(t)) => Err(GenError::SeatUnreachable(t.to_string())),
            Err(ureq::Error::Status(code, _)) => {
                Err(GenError::Seat(format!("image fetch HTTP {code}")))
            }
        }
    }
}

/// Parse a facade health/wake reply into a seat status.
fn parse_health(body: &Value) -> SeatStatus {
    let engine = body
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    match body.get("seat").and_then(Value::as_str) {
        Some("ready") => SeatStatus::Ready { engine },
        Some("starting") => SeatStatus::Starting { engine },
        Some("down") => SeatStatus::Down { engine },
        _ => SeatStatus::Unreachable(format!("off-protocol health reply: {body}")),
    }
}

/// Extract the first output image from a history document once the
/// job is complete; `Err(NoImage)` if it completed with no image,
/// `Ok(None)` while it is still running.
fn find_completed_image(
    history: &Value,
    prompt_id: &str,
) -> Option<Result<(String, String, String), GenError>> {
    let job = history.get(prompt_id)?;
    let completed = job
        .get("status")
        .and_then(|s| s.get("completed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !completed {
        return None;
    }
    let images = job.get("outputs")?.get("7")?.get("images")?.as_array()?;
    let first = images.first()?;
    let filename = first.get("filename")?.as_str()?.to_string();
    let subfolder = first
        .get("subfolder")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let kind = first
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("output")
        .to_string();
    Some(Ok((filename, subfolder, kind)))
}

/// Extract a seat-side execution error from a history document.
fn find_error(history: &Value, prompt_id: &str) -> Option<String> {
    let status = history.get(prompt_id)?.get("status")?;
    if status.get("status_str").and_then(Value::as_str) == Some("error") {
        return Some(format!("job failed at the seat: {status}"));
    }
    None
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Current unix time in seconds (shared with the session layer
/// for attestation and filing timestamps).
pub fn now_unix() -> u64 {
    unix_now()
}

fn derive_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos ^ u64::from(std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast::SIRENA;
    use serde_json::json;

    #[test]
    fn dead_seat_is_unreachable_not_a_panic() {
        // Port 9 (discard) on loopback refuses immediately.
        let client = SeatClient::new("http://127.0.0.1:9");
        let dir = std::env::temp_dir().join("tmg-client-test-dead-seat");
        let gallery = Gallery::open(&dir).expect("gallery");
        let err = client
            .generate(&SIRENA, "city night", Some(1), &gallery)
            .expect_err("a dead seat must fail");
        assert!(
            matches!(err, GenError::SeatUnreachable(_)),
            "expected SeatUnreachable, got {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn token_comes_from_seat_settings_and_blank_means_none() {
        let gated = SeatClient::with_token("http://127.0.0.1:8190/", Some("sekret".to_string()));
        assert_eq!(gated.token.as_deref(), Some("sekret"));
        assert_eq!(gated.base_url(), "http://127.0.0.1:8190");
        let blank = SeatClient::with_token("http://127.0.0.1:8190", Some("  ".to_string()));
        assert_eq!(blank.token, None);
        let plain = SeatClient::new("http://127.0.0.1:8190");
        assert_eq!(plain.token, None);
    }

    #[test]
    fn dead_seat_health_is_unreachable() {
        let client = SeatClient::new("http://127.0.0.1:9");
        assert!(matches!(client.health(), SeatStatus::Unreachable(_)));
    }

    #[test]
    fn oversized_prompt_is_refused_before_any_seat_contact() {
        let client = SeatClient::new("http://127.0.0.1:9");
        let dir = std::env::temp_dir().join("tmg-client-test-big-prompt");
        let gallery = Gallery::open(&dir).expect("gallery");
        let big = "x".repeat(USER_PROMPT_BYTES_MAX + 1);
        let err = client
            .generate(&SIRENA, &big, Some(1), &gallery)
            .expect_err("oversized prompt must fail");
        assert!(matches!(err, GenError::BadRequest(_)), "got {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn screened_prompt_is_refused_before_any_seat_contact() {
        let client = SeatClient::new("http://127.0.0.1:9");
        let dir = std::env::temp_dir().join("tmg-client-test-screened");
        let gallery = Gallery::open(&dir).expect("gallery");
        let err = client
            .generate(&SIRENA, "schoolgirl at the pool", Some(1), &gallery)
            .expect_err("screened prompt must fail");
        assert!(matches!(err, GenError::ScreenRefused(_)), "got {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn health_parsing_covers_the_seat_states() {
        assert_eq!(
            parse_health(&json!({"engine": "torogao-gen", "seat": "ready"})),
            SeatStatus::Ready {
                engine: "torogao-gen".to_string()
            }
        );
        assert!(matches!(
            parse_health(&json!({"engine": "torogao-gen", "seat": "starting"})),
            SeatStatus::Starting { .. }
        ));
        assert!(matches!(
            parse_health(&json!({"engine": "torogao-gen", "seat": "down"})),
            SeatStatus::Down { .. }
        ));
        assert!(matches!(
            parse_health(&json!({"what": "is this"})),
            SeatStatus::Unreachable(_)
        ));
    }

    #[test]
    fn completed_history_yields_the_image_ref() {
        let history = json!({
            "abc": {
                "status": {"status_str": "success", "completed": true},
                "outputs": {"7": {"images": [
                    {"filename": "torogao_mobile_00001_.png", "subfolder": "", "type": "output"}
                ]}}
            }
        });
        let found = find_completed_image(&history, "abc").expect("completed job");
        let (f, s, t) = found.expect("image ref");
        assert_eq!(f, "torogao_mobile_00001_.png");
        assert_eq!(s, "");
        assert_eq!(t, "output");
    }

    #[test]
    fn running_history_yields_nothing_yet() {
        let history = json!({"abc": {"status": {"status_str": "running", "completed": false}, "outputs": {}}});
        assert!(find_completed_image(&history, "abc").is_none());
        assert!(find_error(&history, "abc").is_none());
    }

    #[test]
    fn errored_history_reports_the_seats_error() {
        let history =
            json!({"abc": {"status": {"status_str": "error", "completed": true}, "outputs": {}}});
        assert!(find_error(&history, "abc").is_some());
    }
}
