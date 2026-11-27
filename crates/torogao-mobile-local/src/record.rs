//! The character record — schema `torogao-character/1`, in
//! the exact shape of `SCHEMA-torogao-character-1.md` (the
//! programme's schema of record, authored by the rig lane
//! under ARCHITECTURE.md §3). Field names, section shapes,
//! and the hash discipline follow that document; where the
//! trial's on-device subset interprets a field, the
//! interpretation is stated on the field.
//!
//! The record is data, never UI state: sliders, pickers, and
//! prompt boxes are views over it. Pose is deliberately NOT
//! a record field — the schema keeps pose in clips, not in
//! the character record — so the trial treats pose as
//! preview state (see `render::PoseMap`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The schema identifier every record carries.
pub const SCHEMA: &str = "torogao-character/1";

/// One lock artefact reference. The hash is filled primo-side
/// when the artefact lands there; empty means unhashed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockRef {
    /// Artefact label.
    pub label: String,
    /// SHA-256 of the artefact, or "" when unhashed.
    pub sha256: String,
}

/// The locked Step 0 measurements a derivation is checked
/// against (tolerance 1 cm), when recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proportions {
    /// Height, metres.
    pub height_m: f32,
    /// Bust circumference, metres.
    pub bust_circ_m: f32,
    /// Waist circumference, metres.
    pub waist_circ_m: f32,
    /// Hip circumference, metres.
    pub hip_circ_m: f32,
}

/// Step 0 identity-lock state (D-1410).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityLock {
    /// `locked` or `unlocked`.
    pub status: String,
    /// Lock artefacts.
    #[serde(default)]
    pub refs: Vec<LockRef>,
    /// Locked proportions, when recorded.
    #[serde(default)]
    pub proportions: Option<Proportions>,
    /// Who held the lock.
    #[serde(default)]
    pub locked_by: Option<String>,
    /// When the lock was held (ISO-8601).
    #[serde(default)]
    pub locked_at: Option<String>,
    /// The lock's citation: source document, ruling id,
    /// verbatim facts.
    pub source: String,
}

/// The prompt-side identity (the art lanes compose from it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptIdentity {
    /// Identity tags, verbatim from the lock (Illustrious
    /// tag style).
    pub identity: String,
    /// Signature items / canonical outfit line.
    pub signature: String,
    /// Per-character negative-prompt additions.
    pub negative_guards: String,
    /// The lock / style sheet these were taken from.
    pub source: String,
}

/// One expression setting: a morph name and its weight
/// (schema range 0..=2). The name must be a morph the fleet
/// builder emits — on-device, the bundled base's target list
/// is the authority the preview validates against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    /// Morph target name.
    pub name: String,
    /// Weight, 0..=2.
    pub weight: f32,
}

/// The outfit section: a reference to the outfit spec of
/// record, never a second copy of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outfit {
    /// The outfit spec reference.
    pub spec_ref: String,
}

/// An admission act (`{by, at}`), set primo-side at a gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Admission {
    /// Who admitted.
    pub by: String,
    /// When (ISO-8601).
    pub at: String,
}

/// The rig section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RigRef {
    /// `moon-deep-party` | `torogao` — selects the gated
    /// builder copy primo-side.
    pub builder_family: String,
    /// The canonical base the derivation starts from.
    pub base_ref: String,
    /// The frozen base builder spec, imported primo-side.
    /// Absent = cannot derive.
    #[serde(default)]
    pub derivation_spec: Option<serde_json::Value>,
    /// Optional structural expectation for the A10.2 census
    /// (54 for the fleet party armature).
    #[serde(default)]
    pub expected_joint_count: Option<u32>,
    /// The gated GLB of the current admission, when one
    /// exists.
    #[serde(default)]
    pub gated_glb_sha256: Option<String>,
    /// The last derivation's full gate results, or null.
    #[serde(default)]
    pub gates: Option<serde_json::Value>,
    /// Who admitted the gated GLB and when. Set primo-side,
    /// never typed on the phone.
    #[serde(default)]
    pub admission: Option<Admission>,
}

/// One art candidate, hash-pinned to the exact prompt
/// document that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtCandidate {
    /// SHA-256 of the candidate image.
    pub sha256: String,
    /// The producing route (`wai-txt2img`, `edit-local`, …).
    pub route: String,
    /// The seed used.
    pub seed: u64,
    /// SHA-256 of the prompt document.
    pub prompt_sha256: String,
    /// Frame class (Rule 9): `candidate` until admitted.
    #[serde(rename = "class")]
    pub class: String,
}

/// One admitted keeper and its admission act.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtAdmitted {
    /// SHA-256 of the admitted image.
    pub sha256: String,
    /// Who admitted it.
    pub admitted_by: String,
    /// When (ISO-8601).
    pub at: String,
}

/// The art section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Art {
    /// Candidate stills.
    #[serde(default)]
    pub candidates: Vec<ArtCandidate>,
    /// Admitted keepers.
    #[serde(default)]
    pub admitted: Vec<ArtAdmitted>,
}

/// One character record (`torogao-character/1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterRecord {
    /// Schema identifier; must equal [`SCHEMA`].
    pub schema: String,
    /// Stable id: file name, picker key, export name.
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// Revision, incremented on every accepted edit.
    pub revision: u32,
    /// Last-edit stamp (ISO-8601), written by the editor.
    pub updated_at: String,
    /// Step 0 identity lock state.
    pub identity_lock: IdentityLock,
    /// Prompt-side identity.
    pub prompt_identity: PromptIdentity,
    /// Body knobs (schema §body_params). The trial applies
    /// exactly one on-device: `z_scale` (0.90–1.15), as a
    /// uniform root scale in the preview. The rest are
    /// derivation inputs, carried verbatim for primo.
    pub body_params: BTreeMap<String, f32>,
    /// Face knobs (schema §face_params): derivation inputs,
    /// carried verbatim; no on-device consumer.
    pub face_params: BTreeMap<String, f32>,
    /// Expression morph weights.
    #[serde(default)]
    pub expressions: Vec<Expression>,
    /// Outfit spec reference.
    pub outfit: Outfit,
    /// Rig/base reference.
    pub rig: RigRef,
    /// Art candidates and admissions.
    #[serde(default)]
    pub art: Art,
}

impl CharacterRecord {
    /// The weight set for one morph name (0 when unset).
    pub fn expression_weight(&self, name: &str) -> f32 {
        self.expressions
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.weight)
            .unwrap_or(0.0)
    }

    /// Set (or, at ~0, clear) one expression weight. Weights
    /// clamp to the schema range 0..=2.
    pub fn set_expression(&mut self, name: &str, weight: f32) {
        let weight = weight.clamp(0.0, 2.0);
        if weight < 0.001 {
            self.expressions.retain(|e| e.name != name);
            return;
        }
        if let Some(e) = self.expressions.iter_mut().find(|e| e.name == name) {
            e.weight = weight;
        } else {
            self.expressions.push(Expression {
                name: name.to_string(),
                weight,
            });
        }
        // Stable order = canonical order: sorted by name, so
        // equal records serialise equal.
        self.expressions.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// The morph weight map the deform path applies, from the
    /// expressions list.
    pub fn morph_weights(&self) -> BTreeMap<String, f32> {
        self.expressions
            .iter()
            .map(|e| (e.name.clone(), e.weight.clamp(0.0, 2.0)))
            .collect()
    }

    /// The root scale the preview applies: `body_params`
    /// knob `z_scale`, schema range 0.90–1.15, default 1.0.
    /// (Primo's builder reads the same knob as a derivation
    /// input; the trial's root-scale application is a preview
    /// interpretation, named as such.)
    pub fn z_scale(&self) -> f32 {
        self.body_params
            .get("z_scale")
            .copied()
            .unwrap_or(1.0)
            .clamp(0.90, 1.15)
    }

    /// Serialise (struct field order fixed, maps sorted, the
    /// expressions list name-sorted: equal records serialise
    /// equal).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// The schema's canonical bytes: compact serialisation
    /// with recursively sorted keys.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let value = serde_json::to_value(self).expect("a record is always a Value");
        let mut out = Vec::new();
        write_canonical(&value, &mut out);
        out
    }

    /// The record's SHA-256 over its canonical bytes (the
    /// schema's hash discipline — computable on-device, which
    /// is what lets a phone participate in the facade's
    /// recompute-and-compare protocol offline).
    pub fn sha256(&self) -> String {
        crate::hash::sha256_hex(&self.canonical_bytes())
    }

    /// Parse a record, refusing a foreign schema by name.
    pub fn from_json(text: &str) -> Result<Self, RecordError> {
        let rec: CharacterRecord =
            serde_json::from_str(text).map_err(|e| RecordError::Parse(e.to_string()))?;
        if rec.schema != SCHEMA {
            return Err(RecordError::Schema(rec.schema));
        }
        if !is_valid_id(&rec.id) {
            return Err(RecordError::Id(rec.id));
        }
        Ok(rec)
    }
}

/// Write a JSON value canonically: object keys sorted
/// recursively, compact separators. (Independent of
/// serde_json's map-ordering features by construction.)
fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(
                    serde_json::to_string(key)
                        .expect("a key serialises")
                        .as_bytes(),
                );
                out.push(b':');
                write_canonical(&map[*key], out);
            }
            out.push(b'}');
        }
        serde_json::Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        other => {
            out.extend_from_slice(
                serde_json::to_string(other)
                    .expect("a scalar serialises")
                    .as_bytes(),
            );
        }
    }
}

/// Record-level failures, typed so the shell can show them
/// verbatim instead of panicking on a bad file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// JSON did not parse as a record.
    Parse(String),
    /// The record carries a schema this build does not read.
    Schema(String),
    /// The id is not a safe file stem.
    Id(String),
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::Parse(e) => write!(f, "record does not parse: {e}"),
            RecordError::Schema(s) => write!(f, "record schema {s:?} is not {SCHEMA}"),
            RecordError::Id(id) => write!(f, "record id {id:?} is not a safe file stem"),
        }
    }
}

impl std::error::Error for RecordError {}

/// Ids are file stems: lowercase ASCII, digits, `-`, `_`.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The trial's seed record: Prima, from the mobile core's cast
/// table (itself from the Canonical-base RUNG0-LOCK), bound to
/// the bundled Prima base GLB. Lock artefact refs are filled
/// primo-side per the schema and are empty here — stated, not
/// hidden: the trial build carries no lock artefacts.
pub fn prima_template() -> CharacterRecord {
    CharacterRecord {
        schema: SCHEMA.to_string(),
        id: "prima".to_string(),
        display_name: "Prima".to_string(),
        revision: 1,
        updated_at: "2026-11-27T00:00:00Z".to_string(),
        identity_lock: IdentityLock {
            status: "locked".to_string(),
            refs: Vec::new(),
            proportions: None,
            locked_by: None,
            locked_at: None,
            source: "Canonical-base RUNG0-LOCK (identity lock held 2026-11-24)".to_string(),
        },
        prompt_identity: PromptIdentity {
            identity: "1girl, solo, very long wavy silver-white hair, lavender-grey hair shading, \
                       centre part, face-framing locks, deep red eyes, fair skin"
                .to_string(),
            signature: "pink dress, frilled dress".to_string(),
            negative_guards: String::new(),
            source: "torogao-mobile-core cast table (PRIMA), from RUNG0-LOCK".to_string(),
        },
        body_params: BTreeMap::from([("z_scale".to_string(), 1.0)]),
        face_params: BTreeMap::new(),
        expressions: Vec::new(),
        outfit: Outfit {
            spec_ref: "Prima DressPink — canonical-base outfit of record".to_string(),
        },
        rig: RigRef {
            builder_family: "torogao".to_string(),
            base_ref: "prima.glb (torogao-models-20261104 gated fleet artefact), \
                       bundled in the trial build as assets/prima-base.glb"
                .to_string(),
            derivation_spec: None,
            expected_joint_count: Some(54),
            gated_glb_sha256: Some(
                "8cafa7ae32372319970a174cbadd9d48b82c7b0efa424cacd159a938970a8dfd".to_string(),
            ),
            gates: None,
            admission: None,
        },
        art: Art::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_round_trips_through_json() {
        let rec = prima_template();
        let text = rec.to_json().expect("serialise");
        let back = CharacterRecord::from_json(&text).expect("parse");
        assert_eq!(rec, back);
    }

    #[test]
    fn foreign_schema_is_refused_by_name() {
        let mut rec = prima_template();
        rec.schema = "torogao-character/99".to_string();
        let text = rec.to_json().expect("serialise");
        match CharacterRecord::from_json(&text) {
            Err(RecordError::Schema(s)) => assert_eq!(s, "torogao-character/99"),
            other => panic!("expected Schema refusal, got {other:?}"),
        }
    }

    #[test]
    fn unsafe_id_is_refused() {
        let mut rec = prima_template();
        rec.id = "../escape".to_string();
        let text = rec.to_json().expect("serialise");
        assert!(matches!(
            CharacterRecord::from_json(&text),
            Err(RecordError::Id(_))
        ));
    }

    #[test]
    fn expressions_upsert_clamp_and_stay_sorted() {
        let mut rec = prima_template();
        rec.set_expression("Fac_Mth_Smile", 0.8);
        rec.set_expression("Fac_Eye_Close", 4.0);
        assert_eq!(rec.expression_weight("Fac_Eye_Close"), 2.0);
        let names: Vec<&str> = rec.expressions.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Fac_Eye_Close", "Fac_Mth_Smile"]);
        rec.set_expression("Fac_Mth_Smile", 0.0);
        assert_eq!(rec.expression_weight("Fac_Mth_Smile"), 0.0);
        assert_eq!(rec.expressions.len(), 1);
    }

    #[test]
    fn hash_is_stable_across_round_trip_and_sensitive_to_edits() {
        let rec = prima_template();
        let h1 = rec.sha256();
        let back = CharacterRecord::from_json(&rec.to_json().expect("ser")).expect("parse");
        assert_eq!(h1, back.sha256(), "canonical hash survives a round trip");
        let mut edited = rec.clone();
        edited.set_expression("Fac_Mth_Smile", 1.0);
        assert_ne!(h1, edited.sha256(), "an edit changes the hash");
    }

    #[test]
    fn z_scale_clamps_to_the_schema_range() {
        let mut rec = prima_template();
        assert_eq!(rec.z_scale(), 1.0);
        rec.body_params.insert("z_scale".to_string(), 9.0);
        assert_eq!(rec.z_scale(), 1.15);
        rec.body_params.insert("z_scale".to_string(), 0.1);
        assert_eq!(rec.z_scale(), 0.90);
    }
}
