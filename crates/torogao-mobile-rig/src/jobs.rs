//! Rig job documents: the hash-pinned payloads the facade
//! carries to the primo rig adapter.
//!
//! A job document is built here, client-side, from a
//! [`CharacterRecord`]: it embeds the record, its canonical
//! hash, and — for a derive — the derived builder spec and
//! its hash, both computed by [`crate::params::derive_spec`].
//! The facade refuses a document whose stated hashes do not
//! match its content; the adapter builds exactly the spec it
//! was handed and verifies the record hash again. Knob
//! application therefore happens in exactly one place, and a
//! derive is reproducible from the document alone.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gate::derive_eligibility;
use crate::morphs;
use crate::params::derive_spec;
use crate::schema::{CharacterRecord, ExpressionSetting, sha256_hex};

/// Canonical bytes of an arbitrary JSON value: serde_json's
/// default map ordering is sorted-key, so the same value
/// serialises to the same bytes on both sides of the wire
/// (the record's own discipline, in [`crate::schema`]).
fn canonical_value_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

/// The rig job kinds (facade job kinds of the same names,
/// additive to the gen contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RigJobKind {
    /// Derive (or re-derive) the character's rig from its
    /// record: twin build, A10.1, A10.2, lock check.
    Derive,
    /// Pose-test expression morphs on the gated GLB and
    /// measure what each morph moves.
    PoseTest,
    /// Render facade stills of the gated GLB (rest pose /
    /// current expressions).
    PreviewRender,
}

/// One hash-pinned rig job document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RigJobDocument {
    /// The job kind.
    pub kind: RigJobKind,
    /// The character record this job acts on.
    pub record: CharacterRecord,
    /// Canonical SHA-256 of `record` — the facade and the
    /// adapter both recompute and compare.
    pub record_sha256: String,
    /// Derive only: the derived builder spec, from
    /// [`crate::params::derive_spec`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_spec: Option<Value>,
    /// Derive only: canonical SHA-256 of `derived_spec`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_spec_sha256: Option<String>,
    /// Pose-test only: the morph weights to apply.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pose_morphs: Vec<ExpressionSetting>,
}

impl RigJobDocument {
    /// Build a derive document. Runs the derive gate first —
    /// an ineligible record produces no document, and so no
    /// job (the gate order is enforced here, in the client,
    /// before the facade's own copy of the check).
    ///
    /// # Errors
    /// Returns the eligibility or derivation problems.
    pub fn derive(record: &CharacterRecord) -> Result<Self, Vec<String>> {
        derive_eligibility(record)?;
        let base = record
            .rig
            .derivation_spec
            .as_ref()
            .expect("eligibility guarantees a derivation spec");
        let spec = derive_spec(base, &record.body_params, &record.face_params)?;
        let spec_sha = sha256_hex(&canonical_value_bytes(&spec));
        Ok(Self {
            kind: RigJobKind::Derive,
            record_sha256: record.sha256(),
            record: record.clone(),
            derived_spec: Some(spec),
            derived_spec_sha256: Some(spec_sha),
            pose_morphs: Vec::new(),
        })
    }

    /// Build a pose-test document for the record's gated GLB.
    ///
    /// # Errors
    /// Returns the problems: no gated GLB on the record, a
    /// morph the builder does not emit (no mechanism — Rule
    /// 10), or a weight outside 0..=2.
    pub fn pose_test(
        record: &CharacterRecord,
        morphs_to_test: Vec<ExpressionSetting>,
    ) -> Result<Self, Vec<String>> {
        let mut problems = Vec::new();
        if record.rig.gated_glb_sha256.is_none() {
            problems.push(
                "the record carries no gated GLB: pose tests run on the gated \
                 artefact of a passed derivation"
                    .to_string(),
            );
        }
        for m in &morphs_to_test {
            if morphs::mechanism(&m.name).is_none() {
                problems.push(format!(
                    "morph {:?} has no mechanism in the table and cannot be posed",
                    m.name
                ));
            }
            if !(0.0..=2.0).contains(&m.weight) {
                problems.push(format!(
                    "morph {:?} weight {} is outside 0..=2",
                    m.name, m.weight
                ));
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Self {
            kind: RigJobKind::PoseTest,
            record_sha256: record.sha256(),
            record: record.clone(),
            derived_spec: None,
            derived_spec_sha256: None,
            pose_morphs: morphs_to_test,
        })
    }

    /// Build a preview-render document for the record's
    /// gated GLB.
    ///
    /// # Errors
    /// Returns the problem when the record carries no gated
    /// GLB.
    pub fn preview_render(record: &CharacterRecord) -> Result<Self, Vec<String>> {
        if record.rig.gated_glb_sha256.is_none() {
            return Err(vec![
                "the record carries no gated GLB: previews render the gated artefact".to_string(),
            ]);
        }
        Ok(Self {
            kind: RigJobKind::PreviewRender,
            record_sha256: record.sha256(),
            record: record.clone(),
            derived_spec: None,
            derived_spec_sha256: None,
            pose_morphs: Vec::new(),
        })
    }

    /// Canonical bytes of the document (the hash the facade
    /// is handed in `X-Job-Sha256`).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical_value_bytes(&serde_json::to_value(self).unwrap_or(Value::Null))
    }

    /// SHA-256 of the document's canonical bytes.
    pub fn sha256(&self) -> String {
        sha256_hex(&self.canonical_bytes())
    }

    /// Recompute the embedded hashes and compare: the check
    /// the facade runs on receipt.
    pub fn verify_hashes(&self) -> bool {
        if self.record_sha256 != self.record.sha256() {
            return false;
        }
        match (&self.derived_spec, &self.derived_spec_sha256) {
            (Some(spec), Some(sha)) => *sha == sha256_hex(&canonical_value_bytes(spec)),
            (None, None) => true,
            _ => false,
        }
    }
}

/// One morph's measured movement in a pose-test result:
/// the Rule 10 citation (mechanism + artefact measurement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasuredMorph {
    /// Morph target name.
    pub name: String,
    /// The weight applied.
    pub weight: f64,
    /// Maximum vertex displacement the artefact shows at
    /// that weight, metres (base → posed comparison in the
    /// render bench).
    pub max_displacement_m: f64,
    /// The mechanism, from [`crate::morphs`].
    pub mechanism: String,
}

/// A finished rig job's result, as the facade's history
/// returns it under the `rig` key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RigJobResult {
    /// Terminal state: `done`, `error`, or `cancelled`.
    pub state: String,
    /// Derive only: the record as updated primo-side with
    /// the gate results and gated GLB hash. The phone adopts
    /// it whole; it never merges gate fields itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<CharacterRecord>,
    /// Facade view filenames of the job's stills.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stills: Vec<String>,
    /// Pose-test only: per-morph measurements.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measured_morphs: Vec<MeasuredMorph>,
    /// The failure, when `state` is `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{LockRef, LockStatus};
    use serde_json::json;

    fn eligible_record() -> CharacterRecord {
        let mut r = CharacterRecord::blank("job-test", "Job Test", "t");
        r.identity_lock.status = LockStatus::Locked;
        r.identity_lock.refs = vec![LockRef {
            label: "lock.md".to_string(),
            sha256: "c".repeat(64),
        }];
        r.identity_lock.locked_by = Some("Owner".to_string());
        r.identity_lock.locked_at = Some("2026-11-27T00:00:00Z".to_string());
        r.rig.derivation_spec = Some(json!({
            "character": "Job Test",
            "bust": {"amp": 0.052},
            "face": {"eye_x": 0.0325}
        }));
        r
    }

    // ---- validation ----

    #[test]
    fn a_derive_document_carries_matching_hashes() {
        let doc = RigJobDocument::derive(&eligible_record()).expect("derive doc");
        assert_eq!(doc.kind, RigJobKind::Derive);
        assert!(doc.verify_hashes());
        assert!(doc.derived_spec.is_some());
        assert_eq!(doc.record_sha256, doc.record.sha256());
    }

    #[test]
    fn a_pose_test_document_builds_on_a_gated_record() {
        let mut r = eligible_record();
        r.rig.gated_glb_sha256 = Some("d".repeat(64));
        let doc = RigJobDocument::pose_test(
            &r,
            vec![ExpressionSetting {
                name: "Fac_Mth_Smile".to_string(),
                weight: 1.0,
            }],
        )
        .expect("pose doc");
        assert_eq!(doc.kind, RigJobKind::PoseTest);
        assert!(doc.verify_hashes());
    }

    // ---- adversarial ----

    #[test]
    fn an_unlocked_record_produces_no_derive_document() {
        let mut r = eligible_record();
        r.identity_lock.status = LockStatus::Unlocked;
        assert!(RigJobDocument::derive(&r).is_err());
    }

    #[test]
    fn a_pose_test_without_a_gated_glb_is_refused() {
        let r = eligible_record();
        let err = RigJobDocument::pose_test(
            &r,
            vec![ExpressionSetting {
                name: "Fac_Mth_Smile".to_string(),
                weight: 1.0,
            }],
        )
        .expect_err("must refuse");
        assert!(err.iter().any(|e| e.contains("gated GLB")));
    }

    #[test]
    fn a_pose_test_of_an_unknown_morph_is_refused() {
        let mut r = eligible_record();
        r.rig.gated_glb_sha256 = Some("d".repeat(64));
        let err = RigJobDocument::pose_test(
            &r,
            vec![ExpressionSetting {
                name: "Fac_Mth_Pout".to_string(),
                weight: 1.0,
            }],
        )
        .expect_err("must refuse");
        assert!(err.iter().any(|e| e.contains("no mechanism")));
    }

    #[test]
    fn a_tampered_document_fails_hash_verification() {
        let mut doc = RigJobDocument::derive(&eligible_record()).expect("derive doc");
        doc.record.name = "Tampered".to_string();
        assert!(!doc.verify_hashes());
    }
}
