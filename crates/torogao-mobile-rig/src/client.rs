//! The rig client: submits rig job documents to the
//! facade's rig routes and polls for results.
//!
//! The facade is the only door (ARCHITECTURE §2): the phone
//! never reaches the rig engine, the builders, or Blender.
//! This client mirrors the gen seat client's discipline —
//! blocking `ureq`, bounded reads, generous poll deadline for
//! derivation (a twin build plus the gate instruments is
//! minutes of primo work), and presentable errors.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::jobs::{RigJobDocument, RigJobResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Artefact reads (GLB / stills) get the gen client's image
/// budget; a gated GLB is a few megabytes.
const ARTEFACT_READ_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// Default poll deadline: a derive is two full Blender
/// builds plus the instruments; thirty minutes is generous
/// and bounded.
pub const DERIVE_POLL_DEADLINE: Duration = Duration::from_secs(1800);
/// Pose tests and previews are single renders: ten minutes.
pub const LIGHT_POLL_DEADLINE: Duration = Duration::from_secs(600);
/// Artefact byte cap (a GLB is ~3 MB; the cap is a guard,
/// not a target).
pub const ARTEFACT_BYTES_MAX: usize = 256 * 1024 * 1024;

/// A rig client failure, in presentable form.
#[derive(Debug)]
pub enum RigError {
    /// No facade address is configured.
    NotConfigured,
    /// The facade could not be reached.
    Unreachable(String),
    /// The facade refused the request (HTTP status + its
    /// message). A 422 carries the gate's refusal reasons.
    Refused(u16, String),
    /// The job did not finish inside the poll deadline.
    Timeout,
    /// A reply did not parse.
    Malformed(String),
    /// The job itself failed primo-side.
    Job(String),
}

impl RigError {
    /// A short message for the UI.
    pub fn message(&self) -> String {
        match self {
            RigError::NotConfigured => "Set the computer's address in Settings first.".to_string(),
            RigError::Unreachable(e) => format!("Could not reach the computer: {e}"),
            RigError::Refused(status, body) => format!("The computer refused ({status}): {body}"),
            RigError::Timeout => {
                "The job is still running on the computer; check back shortly.".to_string()
            }
            RigError::Malformed(e) => format!("The computer sent an unreadable reply: {e}"),
            RigError::Job(e) => format!("The rig job failed: {e}"),
        }
    }
}

impl std::fmt::Display for RigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for RigError {}

/// A finished rig job as the client reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct RigOutcome {
    /// The job result block.
    pub result: RigJobResult,
    /// The gated GLB's facade view filename (derive only).
    pub glb_filename: Option<String>,
}

/// The facade rig client.
#[derive(Debug, Clone)]
pub struct RigClient {
    facade_base: String,
    token: Option<String>,
}

impl RigClient {
    /// A client for a facade base URL (no trailing slash),
    /// without a token.
    pub fn new(facade_base: impl Into<String>) -> Self {
        Self {
            facade_base: facade_base.into(),
            token: None,
        }
    }

    /// Set the facade token (sent as `X-Facade-Token`,
    /// matching the facade's gate).
    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token.filter(|t| !t.is_empty());
        self
    }

    fn agent(timeout: Duration) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(timeout)
            .build()
    }

    fn with_token_header(&self, req: ureq::Request) -> ureq::Request {
        match &self.token {
            Some(t) => req.set("X-Facade-Token", t),
            None => req,
        }
    }

    /// Submit a job document; returns the facade's prompt id.
    ///
    /// # Errors
    /// [`RigError`] on transport failure or refusal — a 422
    /// body carries the derive gate's refusal reasons.
    pub fn submit(&self, doc: &RigJobDocument) -> Result<String, RigError> {
        if self.facade_base.is_empty() {
            return Err(RigError::NotConfigured);
        }
        let body = String::from_utf8(doc.canonical_bytes())
            .map_err(|e| RigError::Malformed(e.to_string()))?;
        let req = Self::agent(READ_TIMEOUT)
            .post(&format!("{}/rig/prompt", self.facade_base))
            .set("Content-Type", "application/json")
            .set("X-Job-Sha256", &doc.sha256());
        let resp = self
            .with_token_header(req)
            .send_string(&body)
            .map_err(map_ureq_err)?;
        let json: Value = resp
            .into_json()
            .map_err(|e| RigError::Malformed(e.to_string()))?;
        json.get("prompt_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| RigError::Malformed("no prompt_id in the reply".to_string()))
    }

    /// Poll a submitted job until it finishes or the deadline
    /// passes.
    ///
    /// # Errors
    /// [`RigError::Timeout`] at the deadline, [`RigError::Job`]
    /// when the job failed primo-side, or a transport error.
    pub fn poll(&self, prompt_id: &str, deadline: Duration) -> Result<RigOutcome, RigError> {
        let started = Instant::now();
        loop {
            let req = Self::agent(READ_TIMEOUT)
                .get(&format!("{}/rig/history/{prompt_id}", self.facade_base));
            let resp = self.with_token_header(req).call().map_err(map_ureq_err)?;
            let json: Value = resp
                .into_json()
                .map_err(|e| RigError::Malformed(e.to_string()))?;
            let state = json
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            match state.as_str() {
                "done" => {
                    let rig = json.get("rig").cloned().unwrap_or(Value::Null);
                    let result: RigJobResult = serde_json::from_value(rig)
                        .map_err(|e| RigError::Malformed(e.to_string()))?;
                    let glb_filename = json
                        .get("glb_filename")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    return Ok(RigOutcome {
                        result,
                        glb_filename,
                    });
                }
                "error" => {
                    let msg = json
                        .get("rig")
                        .and_then(|r| r.get("error"))
                        .and_then(Value::as_str)
                        .unwrap_or("the job failed")
                        .to_string();
                    return Err(RigError::Job(msg));
                }
                "cancelled" => return Err(RigError::Job("the job was cancelled".to_string())),
                _ => {
                    if started.elapsed() > deadline {
                        return Err(RigError::Timeout);
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            }
        }
    }

    /// Submit and poll in one call, with the deadline for the
    /// document's kind.
    ///
    /// # Errors
    /// See [`RigClient::submit`] and [`RigClient::poll`].
    pub fn run(&self, doc: &RigJobDocument) -> Result<RigOutcome, RigError> {
        let id = self.submit(doc)?;
        let deadline = match doc.kind {
            crate::jobs::RigJobKind::Derive => DERIVE_POLL_DEADLINE,
            _ => LIGHT_POLL_DEADLINE,
        };
        self.poll(&id, deadline)
    }

    /// Interrupt a running job.
    ///
    /// # Errors
    /// [`RigError`] on transport failure.
    pub fn interrupt(&self, prompt_id: &str) -> Result<(), RigError> {
        let req = Self::agent(READ_TIMEOUT)
            .post(&format!("{}/rig/interrupt", self.facade_base))
            .set("Content-Type", "application/json");
        self.with_token_header(req)
            .send_json(serde_json::json!({ "prompt_id": prompt_id }))
            .map_err(map_ureq_err)?;
        Ok(())
    }

    /// Fetch one artefact (a still or the gated GLB) through
    /// the facade's view route.
    ///
    /// # Errors
    /// [`RigError`] on transport failure or an oversize reply.
    pub fn fetch_view(&self, filename: &str) -> Result<Vec<u8>, RigError> {
        let req = Self::agent(ARTEFACT_READ_TIMEOUT).get(&format!(
            "{}/rig/view?filename={filename}",
            self.facade_base
        ));
        let resp = self.with_token_header(req).call().map_err(map_ureq_err)?;
        let mut bytes = Vec::new();
        use std::io::Read;
        resp.into_reader()
            .take((ARTEFACT_BYTES_MAX + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| RigError::Unreachable(e.to_string()))?;
        if bytes.len() > ARTEFACT_BYTES_MAX {
            return Err(RigError::Malformed(
                "artefact exceeds the size cap".to_string(),
            ));
        }
        Ok(bytes)
    }
}

fn map_ureq_err(e: ureq::Error) -> RigError {
    match e {
        ureq::Error::Status(status, resp) => {
            let body = resp.into_string().unwrap_or_default();
            RigError::Refused(status, body)
        }
        ureq::Error::Transport(t) => RigError::Unreachable(t.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unconfigured_client_refuses_to_submit() {
        let client = RigClient::new("");
        let record = crate::schema::CharacterRecord::blank("x", "X", "t");
        // A pose doc cannot be built for an ungated record;
        // use the eligibility failure path instead: submit is
        // never reached. Directly assert the guard via a
        // minimal document built by hand.
        let doc = crate::jobs::RigJobDocument {
            kind: crate::jobs::RigJobKind::PreviewRender,
            record_sha256: record.sha256(),
            record,
            derived_spec: None,
            derived_spec_sha256: None,
            pose_morphs: Vec::new(),
        };
        assert!(matches!(client.submit(&doc), Err(RigError::NotConfigured)));
    }

    #[test]
    fn error_messages_are_presentable() {
        assert!(RigError::Timeout.message().contains("still running"));
        assert!(
            RigError::Refused(422, "no lock".into())
                .message()
                .contains("no lock")
        );
    }
}
