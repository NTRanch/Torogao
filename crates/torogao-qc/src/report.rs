//! The section-4 report shape: findings with stable codes and
//! a summary. One `Report` is built per verb run and serialized
//! once; the field order below is the published order (serde's
//! default map ordering keeps object keys sorted, and struct
//! fields serialize in declaration order — both deterministic).

use crate::limits::FINDINGS_MAX;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Severity of a finding. `Fail` findings make the summary's
/// `pass` false; `Warning` and `Finding` do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Contract-decisive failure (missing media, NaN, identity).
    Fail,
    /// Dead or suspect configuration (unused option).
    Warning,
    /// A per-item finding for the verdict pack (geometry, sync,
    /// snapshot diff).
    Finding,
}

impl Severity {
    /// The wire name of this severity.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Fail => "fail",
            Severity::Warning => "warning",
            Severity::Finding => "finding",
        }
    }
}

/// One finding: the contract's `{code, severity, scene,
/// element, frame, detail, spec_ref}` plus a human `message`
/// naming what the code means (section 4).
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Stable finding code (`QC_*`); append-only namespace.
    pub code: &'static str,
    /// How decisive this finding is.
    pub severity: Severity,
    /// The element id the finding belongs to, when it has one.
    pub element: Option<String>,
    /// The frame the finding belongs to, when it has one.
    pub frame: Option<f64>,
    /// Instance evidence: paths, values, measured deltas.
    pub detail: String,
    /// RFC 6901 JSON Pointer to the spec node responsible.
    pub spec_ref: String,
    /// Optional structured evidence attached to the finding
    /// (for example a snapshot diff's change regions). Serialized
    /// as `data` only when present, so findings without it keep
    /// their published shape byte-for-byte.
    pub data: Option<Value>,
}

impl Finding {
    /// The human sentence for a finding code (section 4: every
    /// finding carries one). Unknown codes get a generic line.
    pub fn message_for(code: &str) -> &'static str {
        match code {
            "QC_MISSING_MEDIA" => {
                "an asset the spec names is absent or does not match its pinned hash"
            }
            "QC_NAN_VALUE" => "a numeric field in the spec is NaN or infinite",
            "QC_UNUSED_OPTION" => {
                "an option is set that the compiler's known option set does not consume"
            }
            "QC_TEXT_CLIPPED" => "a text element's bounds leave its panel or the frame",
            "QC_TEXT_OVERLAP" => {
                "a text element overlaps another element without a whitelist entry"
            }
            "QC_OFF_FRAME" => "an element's bounds leave the frame and no exit is declared",
            "QC_SYNC_DRIFT" => "a cue's time disagrees with its frame by more than one frame",
            "QC_IDENTITY_DRIFT" => {
                "a character's identity parameters differ from the locked record"
            }
            "QC_SNAPSHOT_DIFF" => "a frame differs from the reference beyond the named threshold",
            _ => "render-QC finding",
        }
    }

    fn to_json(&self, scene: &str) -> Value {
        let mut value = json!({
            "code": self.code,
            "severity": self.severity.as_str(),
            "scene": scene,
            "element": self.element,
            "frame": self.frame,
            "detail": self.detail,
            "message": Finding::message_for(self.code),
            "spec_ref": self.spec_ref,
        });
        if let Some(data) = &self.data {
            value["data"] = data.clone();
        }
        value
    }
}

/// A whole verb run's report, ready to serialize.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Verb that produced this report (`inspect` or `diff`).
    pub verb: &'static str,
    /// Scene name (spec `name`, else the spec file's stem; for
    /// `diff`, the reference directory's name).
    pub scene: String,
    /// Extra top-level fields the verb adds (for example the
    /// diff counters), inserted before `findings`.
    pub extra: Vec<(&'static str, Value)>,
    findings: Vec<Finding>,
    truncated: bool,
}

impl Report {
    /// Starts an empty report for a verb and scene.
    pub fn new(verb: &'static str, scene: String) -> Report {
        Report {
            verb,
            scene,
            extra: Vec::new(),
            findings: Vec::new(),
            truncated: false,
        }
    }

    /// Adds a finding, honouring the findings cap; past the cap
    /// the finding is dropped and the report is marked
    /// truncated (the summary says so — never silent).
    pub fn push(&mut self, finding: Finding) {
        if self.findings.len() >= FINDINGS_MAX {
            self.truncated = true;
            return;
        }
        self.findings.push(finding);
    }

    /// The findings, in emission order.
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// True when any finding has `Fail` severity.
    pub fn failed(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Fail)
    }

    /// Sorts findings into the published order — by code, then
    /// spec_ref, then element — so the report is a pure function
    /// of the finding set.
    pub fn sort_findings(&mut self) {
        self.findings.sort_by(|a, b| {
            a.code
                .cmp(b.code)
                .then_with(|| a.spec_ref.cmp(&b.spec_ref))
                .then_with(|| a.element.cmp(&b.element))
                .then_with(|| a.detail.cmp(&b.detail))
        });
    }

    /// Serializes the report to its section-4 JSON document.
    pub fn to_json(&self) -> Value {
        let mut by_code: BTreeMap<&str, u64> = BTreeMap::new();
        let mut fail_count = 0u64;
        for finding in &self.findings {
            *by_code.entry(finding.code).or_insert(0) += 1;
            if finding.severity == Severity::Fail {
                fail_count += 1;
            }
        }
        let mut root = serde_json::Map::new();
        root.insert("tool".into(), json!("torogao-qc"));
        root.insert("verb".into(), json!(self.verb));
        root.insert("scene".into(), json!(self.scene));
        for (key, value) in &self.extra {
            root.insert((*key).into(), value.clone());
        }
        let findings: Vec<Value> = self
            .findings
            .iter()
            .map(|f| f.to_json(&self.scene))
            .collect();
        root.insert("findings".into(), Value::Array(findings));
        root.insert(
            "summary".into(),
            json!({
                "pass": fail_count == 0,
                "fail": fail_count,
                "findings": self.findings.len(),
                "findings_by_code": by_code,
                "truncated": self.truncated,
            }),
        );
        Value::Object(root)
    }

    /// The report as a pretty JSON string (the CLI's stdout).
    pub fn to_json_string(&self) -> String {
        let value = self.to_json();
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Rounds to 6 decimal places so float fields in reports are
/// stable across platforms and runs.
pub fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

/// Rounds to 2 decimal places (mean deltas, drift readouts).
pub fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
