//! The seat client.
//!
//! Blocking HTTP client for the estate ComfyUI seat that runs wai
//! (D-1525). The seat is not a standing service — an idle reaper
//! stops it — so "seat unreachable" is a first-class outcome the
//! shells present, not a crash. All work is bounded: connect/read
//! timeouts on every call, a hard deadline on history polling, and
//! a cap on the fetched image size.

use std::fmt;
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::cast::Character;
use crate::gallery::{Gallery, GalleryEntry};
use crate::workflow::{build_workflow, USER_PROMPT_BYTES_MAX};

/// Connect timeout for any seat call.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Read timeout for submit/history calls.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Read timeout for the image fetch (stills are a few MB).
pub const IMAGE_READ_TIMEOUT: Duration = Duration::from_secs(120);
/// Delay between history polls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Hard deadline for one generation, in seconds (10 minutes covers
/// a cold seat start plus a slow queue; past it the job is given up,
/// never polled forever).
pub const POLL_DEADLINE_SECS: u64 = 600;
/// Largest image the client will file, in bytes (64 MiB).
pub const IMAGE_BYTES_MAX: u64 = 64 * 1024 * 1024;

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
    /// The local gallery could not be read or written.
    Gallery(String),
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
            Self::Gallery(m) => write!(f, "gallery error: {m}"),
        }
    }
}

impl std::error::Error for GenError {}

impl From<std::io::Error> for GenError {
    fn from(e: std::io::Error) -> Self {
        Self::Gallery(format!("local file error: {e}"))
    }
}

/// A client bound to one seat base URL (e.g. `http://127.0.0.1:8188`
/// on primo, `http://100.x.y.z:8188` over the tailnet).
#[derive(Debug, Clone)]
pub struct SeatClient {
    base_url: String,
}

impl SeatClient {
    /// Create a client for the seat at `base_url` (no trailing slash
    /// needed; one is tolerated and stripped).
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// The seat base URL this client talks to.
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

    /// Generate one still of `character` with the user's scene
    /// prompt and file it in `gallery`. `seed: None` derives a seed
    /// from the clock; the filed entry records the seed either way,
    /// so any still can be reproduced at the seat.
    pub fn generate(
        &self,
        character: &Character,
        user_prompt: &str,
        seed: Option<u64>,
        gallery: &Gallery,
    ) -> Result<GalleryEntry, GenError> {
        if user_prompt.len() > USER_PROMPT_BYTES_MAX {
            return Err(GenError::BadRequest(format!(
                "scene prompt is {} bytes; the limit is {USER_PROMPT_BYTES_MAX}",
                user_prompt.len()
            )));
        }
        let seed = seed.unwrap_or_else(derive_seed);
        let workflow = build_workflow(character, user_prompt, seed);
        let prompt_id = self.submit(&workflow)?;
        let image_ref = self.await_image(&prompt_id)?;
        let bytes = self.fetch_image(&image_ref)?;
        let created_unix = unix_now();
        gallery.add(&bytes, character.id, user_prompt, seed, created_unix)
    }

    fn submit(&self, workflow: &Value) -> Result<String, GenError> {
        let url = format!("{}/prompt", self.base_url);
        let resp = Self::agent()
            .post(&url)
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
            Err(ureq::Error::Status(code, r)) => Err(GenError::Submit(format!(
                "HTTP {code}: {}",
                r.into_string().unwrap_or_default()
            ))),
        }
    }

    /// Poll history until the job completes; returns the first
    /// output image's (filename, subfolder, type).
    fn await_image(&self, prompt_id: &str) -> Result<(String, String, String), GenError> {
        let url = format!("{}/history/{prompt_id}", self.base_url);
        let deadline = unix_now() + POLL_DEADLINE_SECS;
        loop {
            let resp = Self::agent().get(&url).call();
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
        let resp = agent
            .get(&format!("{}/view", self.base_url))
            .query("filename", filename)
            .query("subfolder", subfolder)
            .query("type", kind)
            .call();
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
    let images = job
        .get("outputs")?
        .get("7")?
        .get("images")?
        .as_array()?;
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
        let history = json!({"abc": {"status": {"status_str": "error", "completed": true}, "outputs": {}}});
        assert!(find_error(&history, "abc").is_some());
    }
}
