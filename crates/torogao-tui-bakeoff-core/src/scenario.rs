//! The one scripted workload rendered by both TUI routes.
//!
//! The scenario performs the PQC handshake, the mediated MCP
//! exchange, and the signed-card A2A exchange before either
//! renderer sees the resulting panel lines. Neither route can
//! invent a more favourable outcome: both consume this report.

use std::path::Path;

use crate::a2a::run_a2a_exchange;
use crate::data::{OperatorData, load_operator_data};
use crate::error::{BakeoffError, Result};
use crate::mcp::run_mcp_exchange;
use crate::pqc::establish_session;

/// One measured gate step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GateStep {
    /// Stable step identifier.
    pub id: &'static str,
    /// `PASS` for an expected success or `REFUSED` for the
    /// expected gate refusal.
    pub outcome: &'static str,
    /// Deterministic detail rendered in the Verdict panel.
    pub detail: String,
}

/// All panel lines and step outcomes for one scenario run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioReport {
    /// Operator data measured from Torogao.
    pub data: OperatorData,
    /// Session panel lines.
    pub session_lines: Vec<String>,
    /// Scene panel lines.
    pub scene_lines: Vec<String>,
    /// Rig panel lines.
    pub rig_lines: Vec<String>,
    /// Verdict panel lines.
    pub verdict_lines: Vec<String>,
    /// Gate steps in workload order.
    pub steps: Vec<GateStep>,
}

/// Runs the whole workload once against a Torogao workspace.
///
/// # Errors
///
/// Returns the first typed workload failure. A failed required
/// step is a run failure, not a rendered verdict, so both routes
/// stop identically instead of displaying invented outcomes.
pub fn run_scenario(workspace_root: &Path) -> Result<ScenarioReport> {
    let handshake = establish_session()?;
    if !handshake.valid_established || !handshake.tamper_rejected {
        return Err(BakeoffError::Crypto(format!(
            "ML-KEM handshake measured valid={} tamper_rejected={}",
            handshake.valid_established, handshake.tamper_rejected
        )));
    }

    let data = load_operator_data(workspace_root)?;
    let mcp = run_mcp_exchange(&data.rig)?;
    let a2a = run_a2a_exchange(&data.rig)?;
    if !a2a.card_verified {
        return Err(BakeoffError::Crypto(
            "A2A card verification returned false after a successful exchange".to_string(),
        ));
    }

    let steps = vec![
        GateStep {
            id: "pqc-valid",
            outcome: "PASS",
            detail: "ML-KEM-768 session established".to_string(),
        },
        GateStep {
            id: "pqc-tamper",
            outcome: "PASS",
            detail: "corrupted ciphertext rejected by key confirmation".to_string(),
        },
        GateStep {
            id: "mcp-allowed",
            outcome: "PASS",
            detail: format!("rig_census returned {} parameters", mcp.parameter_count),
        },
        GateStep {
            id: "mcp-label",
            outcome: "REFUSED",
            detail: format!("{} / {}", mcp.refusal_code, mcp.refusal_action),
        },
        GateStep {
            id: "a2a-card",
            outcome: "PASS",
            detail: "Ed25519 Agent Card signature verified".to_string(),
        },
        GateStep {
            id: "a2a-task",
            outcome: "PASS",
            detail: format!("{} {}", a2a.task_id, a2a.task_state),
        },
    ];

    let session_lines = vec![
        "protocol: ML-KEM-768 + key confirmation".to_string(),
        format!("ciphertext: {} bytes", handshake.ciphertext_bytes),
        format!(
            "shared secret: {} bytes (not retained)",
            handshake.shared_secret_bytes
        ),
        "valid handshake: ESTABLISHED".to_string(),
        "tampered ciphertext: REJECTED".to_string(),
        "MCP era: 2026-07-28 via rmcp 3.8.0".to_string(),
        "A2A card: Ed25519 signature VERIFIED".to_string(),
    ];
    let scene_lines = vec![
        format!("scene: {} — {}", data.scene.name, data.scene.title),
        format!("format: {}", data.scene.format),
        format!("characters: {}", data.scene.character_count),
        format!("fixtures: {}", data.scene.fixture_count),
        format!("props: {}", data.scene.prop_count),
        "source: scenes/studio.json".to_string(),
    ];
    let rig_lines = vec![
        format!("project: {}", data.rig.project_name),
        format!("project sha256 prefix: {}", data.rig.project_sha_prefix),
        format!("parameters: {}", data.rig.parameter_count),
        format!("parameter ids: {}", data.rig.parameter_ids.join(", ")),
        format!("parts: {}", data.rig.part_count),
        format!("deformers: {}", data.rig.deformer_count),
        format!("expressions: {}", data.rig.expression_count),
        format!(
            "mesh: {} vertices / {} triangles",
            data.rig.vertex_count, data.rig.triangle_count
        ),
        "wide text: 界面エンジン ✓".to_string(),
    ];
    let verdict_lines = steps
        .iter()
        .map(|step| format!("[{}] {} — {}", step.outcome, step.id, step.detail))
        .collect();

    Ok(ScenarioReport {
        data,
        session_lines,
        scene_lines,
        rig_lines,
        verdict_lines,
        steps,
    })
}

/// Runs only the setup-free panel datum used by core unit tests
/// when no Torogao tree is supplied.
pub fn unicode_probe_line() -> &'static str {
    "wide text: 界面エンジン ✓"
}
