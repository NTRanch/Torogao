//! The `torogao-character/1` record schema.
//!
//! One text document per character; the only thing the phone
//! edits. Sections follow ARCHITECTURE §3: `identity_lock` is
//! the gate input, `prompt_identity` replaces the compile-time
//! cast table's statics, `body_params`/`face_params` are the
//! named parameter layer over the frozen builder spec carried
//! in `rig`, `expressions` names morph targets from
//! [`crate::morphs`] with weights, and `art` carries the
//! candidate/admission state for generated stills.
//!
//! Serialisation discipline mirrors the estate's format rules:
//! serde_json's sorted-key map ordering makes the canonical
//! bytes a pure function of the record's content, and the
//! record's SHA-256 is taken over those bytes — the same
//! approval-hash discipline the workflow documents use. Schema
//! evolution is additive-only: new fields land with serde
//! defaults so older records keep parsing.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gate::GateResults;
use crate::params::{BodyParams, FaceParams};

/// The schema identifier every record carries.
pub const SCHEMA_ID: &str = "torogao-character/1";

/// One character record (`torogao-character/1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterRecord {
    /// Schema identifier; must equal [`SCHEMA_ID`].
    pub schema: String,
    /// Stable id (lowercase, digits, hyphens) used in requests
    /// and file names.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The Step 0 identity lock: gate input for derivation.
    pub identity_lock: IdentityLock,
    /// Prompt-side identity (the cast table's former statics).
    #[serde(default)]
    pub prompt_identity: PromptIdentity,
    /// Named body parameters over the builder spec.
    #[serde(default)]
    pub body_params: BodyParams,
    /// Named face parameters over the builder spec.
    #[serde(default)]
    pub face_params: FaceParams,
    /// Expression morph settings (names from
    /// [`crate::morphs::MORPH_MECHANISMS`]).
    #[serde(default)]
    pub expressions: Vec<ExpressionSetting>,
    /// Current outfit reference.
    #[serde(default)]
    pub outfit: OutfitRef,
    /// Rig state: builder family, frozen base, derivation
    /// spec, gated artefact and gate results.
    #[serde(default)]
    pub rig: RigSection,
    /// Generated-art candidates and admissions.
    #[serde(default)]
    pub art: ArtSection,
    /// Revision counter; every edit is a new revision, never
    /// a mutation (sync conflicts surface as named states).
    pub revision: u64,
    /// ISO-8601 timestamp of the last edit, supplied by the
    /// editing side (this crate reads no clock).
    pub updated_at: String,
}

/// Whether the Step 0 identity lock is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockStatus {
    /// Identity locked against canonical references (D-1410).
    Locked,
    /// No lock held; derivation is refused, art is not.
    Unlocked,
}

/// One lock artefact reference: a label naming the artefact
/// and, once hashed primo-side, its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRef {
    /// Human-readable artefact label (path or record name).
    pub label: String,
    /// Lowercase hex SHA-256 of the artefact; empty until the
    /// artefact has been hashed primo-side. A lock carrying an
    /// unhashed reference does not pass the derive gate.
    #[serde(default)]
    pub sha256: String,
}

/// The proportions an identity lock binds, in metres. These
/// are the values the A10.1 instrument's Step 0 block measures
/// on a derived GLB; the lock check compares the two.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LockProportions {
    /// Standing height including the hair cap.
    pub height_m: f64,
    /// Bust circumference.
    pub bust_circ_m: f64,
    /// Waist circumference.
    pub waist_circ_m: f64,
    /// Hip circumference.
    pub hip_circ_m: f64,
}

/// The Step 0 identity lock section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityLock {
    /// Locked or unlocked.
    pub status: LockStatus,
    /// Lock artefacts of record (governing references, lock
    /// documents).
    #[serde(default)]
    pub refs: Vec<LockRef>,
    /// The proportions the lock binds, when measured.
    #[serde(default)]
    pub proportions: Option<LockProportions>,
    /// Who held the lock (an Owner act).
    #[serde(default)]
    pub locked_by: Option<String>,
    /// ISO-8601 time the lock was held.
    #[serde(default)]
    pub locked_at: Option<String>,
    /// Citation of where the lock data was taken from.
    pub source: String,
}

/// Prompt-side identity: identity tags, signature outfit line,
/// per-character negative guards, and the citation they were
/// taken from. Field-for-field the former `cast.rs` record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptIdentity {
    /// Positive identity tags (Illustrious tag style).
    pub identity: String,
    /// Signature items / canonical outfit line.
    pub signature: String,
    /// Per-character additions to the shared negative prompt.
    pub negative_guards: String,
    /// The lock / style sheet this record was taken from.
    pub source: String,
}

/// One expression morph setting: a morph name from
/// [`crate::morphs`] and its weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpressionSetting {
    /// Morph target name (e.g. `Fac_Mth_Smile`).
    pub name: String,
    /// Weight; the fleet builder's keys are authored for
    /// 0..=1, overshoot to 2.0 is the accepted bound.
    pub weight: f64,
}

/// The outfit section: a reference to the outfit spec of
/// record, never a second copy of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutfitRef {
    /// Label/path of the outfit spec of record (e.g. the
    /// design document section or garment spec the builder
    /// spec's garment blocks were taken from).
    pub spec_ref: String,
}

/// The rig section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RigSection {
    /// Builder family whose gated builder derives this
    /// character (`party`, `torogao`, `strobe`, `lana` — the
    /// fleet builder copies of record).
    pub builder_family: String,
    /// The frozen base this character derives from (template
    /// name + hash where the programme carries one).
    pub base_ref: String,
    /// The builder spec of record (the frozen base document
    /// the parameter layer applies over). `None` until a spec
    /// is imported primo-side; a record without one cannot
    /// derive.
    #[serde(default)]
    pub derivation_spec: Option<Value>,
    /// Expected joint count of the derived GLB, when the
    /// family's census carries one (MoonDeep party: 54).
    #[serde(default)]
    pub expected_joint_count: Option<u32>,
    /// SHA-256 of the gated GLB, once gates have passed
    /// primo-side. Written by the gate, never by an editor.
    #[serde(default)]
    pub gated_glb_sha256: Option<String>,
    /// The gate results for the gated GLB.
    #[serde(default)]
    pub gates: Option<GateResults>,
    /// Owner admission of the rig into the record, once given.
    #[serde(default)]
    pub admission: Option<Admission>,
}

/// An Owner admission act, recorded with its timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Admission {
    /// Who admitted (the Owner).
    pub by: String,
    /// ISO-8601 time of the admission.
    pub at: String,
}

/// One generated-art candidate, as filed by a generation job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtCandidate {
    /// SHA-256 of the candidate file.
    pub sha256: String,
    /// Route that produced it (`wai-txt2img`, `edit-local`, …).
    pub route: String,
    /// Generation seed.
    pub seed: u64,
    /// SHA-256 of the composed prompt document.
    pub prompt_sha256: String,
    /// Artefact class; generated art is `candidate` by
    /// construction (Rule 9) until an Owner admission.
    pub class: String,
}

/// The art section: candidates and Owner admissions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtSection {
    /// Candidate stills, in filing order.
    #[serde(default)]
    pub candidates: Vec<ArtCandidate>,
    /// Admitted stills (an Owner act, recorded primo-side).
    #[serde(default)]
    pub admitted: Vec<ArtAdmission>,
}

/// An admitted still: the candidate's hash plus the act.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtAdmission {
    /// SHA-256 of the admitted file.
    pub sha256: String,
    /// Who admitted (the Owner).
    pub admitted_by: String,
    /// ISO-8601 time of the admission.
    pub at: String,
}

impl CharacterRecord {
    /// A blank record for the new-character flow: unlocked,
    /// no derivation spec, revision 1. The flow fills identity
    /// text and lock references next; Derive stays disabled
    /// until a lock is held (the gate order is the design).
    pub fn blank(id: &str, name: &str, updated_at: &str) -> Self {
        Self {
            schema: SCHEMA_ID.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            identity_lock: IdentityLock {
                status: LockStatus::Unlocked,
                refs: Vec::new(),
                proportions: None,
                locked_by: None,
                locked_at: None,
                source: String::new(),
            },
            prompt_identity: PromptIdentity::default(),
            body_params: BodyParams::default(),
            face_params: FaceParams::default(),
            expressions: Vec::new(),
            outfit: OutfitRef::default(),
            rig: RigSection::default(),
            art: ArtSection::default(),
            revision: 1,
            updated_at: updated_at.to_string(),
        }
    }

    /// The canonical bytes of this record — the bytes the
    /// record hash covers. The record is serialised through
    /// its `Value` form, whose map ordering is sorted-key,
    /// so the same record serialises to the same bytes on
    /// both sides of the wire (a struct serialisation would
    /// follow field-declaration order instead — the hash
    /// rule is sorted-key JSON of the value, everywhere).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_value(self)
            .map(|v| serde_json::to_vec(&v).unwrap_or_default())
            .unwrap_or_default()
    }

    /// SHA-256 of the canonical bytes, lowercase hex.
    pub fn sha256(&self) -> String {
        sha256_hex(&self.canonical_bytes())
    }

    /// Validate the record's own consistency. Returns every
    /// problem found (empty = valid). Gate eligibility is a
    /// separate, stricter question ([`crate::gate`]).
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema != SCHEMA_ID {
            problems.push(format!("schema is {:?}, not {SCHEMA_ID:?}", self.schema));
        }
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            problems.push(format!("id {:?} is not a lowercase slug", self.id));
        }
        if self.name.is_empty() {
            problems.push("name is empty".to_string());
        }
        if self.revision == 0 {
            problems.push("revision must be >= 1".to_string());
        }
        for r in &self.identity_lock.refs {
            if !r.sha256.is_empty() && !is_sha256_hex(&r.sha256) {
                problems.push(format!("lock ref {:?} carries a malformed sha256", r.label));
            }
        }
        if let Some(p) = &self.identity_lock.proportions {
            for (field, v) in [
                ("height_m", p.height_m),
                ("bust_circ_m", p.bust_circ_m),
                ("waist_circ_m", p.waist_circ_m),
                ("hip_circ_m", p.hip_circ_m),
            ] {
                if !(0.2..=2.6).contains(&v) {
                    problems.push(format!("lock proportion {field} = {v} is implausible"));
                }
            }
        }
        problems.extend(self.body_params.validate());
        problems.extend(self.face_params.validate());
        let mut seen: Vec<&str> = Vec::new();
        for e in &self.expressions {
            if crate::morphs::mechanism(&e.name).is_none() {
                problems.push(format!(
                    "expression {:?} names no morph in the mechanism table \
                     (Rule 10: an unnamed mechanism does not ship)",
                    e.name
                ));
            }
            if !(0.0..=2.0).contains(&e.weight) {
                problems.push(format!(
                    "expression {:?} weight {} is outside 0..=2",
                    e.name, e.weight
                ));
            }
            if seen.contains(&e.name.as_str()) {
                problems.push(format!("expression {:?} is set twice", e.name));
            }
            seen.push(&e.name);
        }
        problems
    }
}

/// True when `s` is a lowercase hex SHA-256 (64 chars).
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// SHA-256 of arbitrary bytes, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked_record() -> CharacterRecord {
        let mut r = CharacterRecord::blank("test-girl", "Test Girl", "2026-11-27T00:00:00Z");
        r.identity_lock.status = LockStatus::Locked;
        r.identity_lock.refs = vec![LockRef {
            label: "lock.md".to_string(),
            sha256: "a".repeat(64),
        }];
        r.identity_lock.locked_by = Some("Owner".to_string());
        r.identity_lock.locked_at = Some("2026-11-27T00:00:00Z".to_string());
        r.identity_lock.source = "test lock".to_string();
        r
    }

    // ---- validation: well-formed records pass ----

    #[test]
    fn a_blank_record_validates() {
        assert!(
            CharacterRecord::blank("new-girl", "New Girl", "t")
                .validate()
                .is_empty()
        );
    }

    #[test]
    fn a_locked_record_validates() {
        assert!(locked_record().validate().is_empty());
    }

    #[test]
    fn canonical_bytes_and_hash_are_stable() {
        let r = locked_record();
        assert_eq!(r.canonical_bytes(), r.canonical_bytes());
        assert_eq!(r.sha256(), r.sha256());
        assert_eq!(r.sha256().len(), 64);
        let mut changed = r.clone();
        changed.revision += 1;
        assert_ne!(r.sha256(), changed.sha256());
    }

    #[test]
    fn serde_round_trips() {
        let r = locked_record();
        let text = serde_json::to_string(&r).expect("serialise");
        let back: CharacterRecord = serde_json::from_str(&text).expect("parse");
        assert_eq!(r, back);
    }

    // ---- adversarial: malformed records are named ----

    #[test]
    fn a_wrong_schema_is_refused() {
        let mut r = locked_record();
        r.schema = "torogao-character/99".to_string();
        assert!(r.validate().iter().any(|p| p.contains("schema")));
    }

    #[test]
    fn a_bad_id_is_refused() {
        let mut r = locked_record();
        r.id = "Test Girl!".to_string();
        assert!(r.validate().iter().any(|p| p.contains("slug")));
    }

    #[test]
    fn an_unknown_morph_is_refused() {
        let mut r = locked_record();
        r.expressions.push(ExpressionSetting {
            name: "Fac_Nope".to_string(),
            weight: 1.0,
        });
        assert!(r.validate().iter().any(|p| p.contains("Fac_Nope")));
    }

    #[test]
    fn an_out_of_range_weight_is_refused() {
        let mut r = locked_record();
        r.expressions.push(ExpressionSetting {
            name: "Fac_Mth_Smile".to_string(),
            weight: 2.5,
        });
        assert!(r.validate().iter().any(|p| p.contains("weight")));
    }

    #[test]
    fn a_duplicate_expression_is_refused() {
        let mut r = locked_record();
        for _ in 0..2 {
            r.expressions.push(ExpressionSetting {
                name: "Fac_Mth_Smile".to_string(),
                weight: 0.5,
            });
        }
        assert!(r.validate().iter().any(|p| p.contains("twice")));
    }

    #[test]
    fn a_malformed_lock_hash_is_refused() {
        let mut r = locked_record();
        r.identity_lock.refs[0].sha256 = "xyz".to_string();
        assert!(r.validate().iter().any(|p| p.contains("malformed")));
    }
}
