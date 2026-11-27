//! The character record — schema `torogao-character/1`, the
//! shape the mobile-capability architecture (§3) fixes as the
//! one source of truth a phone edits.
//!
//! The record is data, never UI state: sliders, pickers, and
//! prompt boxes are views over it. This trial implements the
//! full schema but only *applies* a named subset on-device
//! (expression morphs, the bust body morph, joint pose, root
//! height) — see `docs` in the trial record for the subset
//! statement. `face_params` values are carried and preserved
//! verbatim; no on-device consumer reads them yet, exactly as
//! the architecture's B2 note anticipates.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The schema identifier every record carries.
pub const SCHEMA: &str = "torogao-character/1";

/// Step 0 identity-lock state, carried in the record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityLock {
    /// `locked` or `unlocked`.
    pub step0_status: String,
    /// The lock artefacts / ruling this status rests on.
    pub source: String,
}

/// The prompt-side identity (what the art lanes compose from).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptIdentity {
    /// Positive identity tags (Illustrious tag style).
    pub tags: String,
    /// Signature items / canonical outfit line.
    pub signature: String,
    /// Per-character negative-prompt additions.
    pub negative_guards: String,
    /// The lock / style sheet these were taken from.
    pub source: String,
}

/// The rig section: which base the record derives from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RigRef {
    /// Human-citable base reference (artefact + hash).
    pub base_ref: String,
    /// Where the base mesh lives for this build.
    pub base_asset: String,
}

/// One character record (`torogao-character/1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterRecord {
    /// Schema identifier; must equal [`SCHEMA`].
    pub schema: String,
    /// Stable id: file name, picker key, export name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Step 0 identity lock state.
    pub identity_lock: IdentityLock,
    /// Prompt-side identity.
    pub prompt_identity: PromptIdentity,
    /// Named body parameters. On-device consumers:
    /// `bust_size` (the `Body_Bust_Size` morph weight) and
    /// `height` (root scale). Other keys are carried verbatim.
    pub body_params: BTreeMap<String, f32>,
    /// Named face parameters (carried; no on-device consumer).
    pub face_params: BTreeMap<String, f32>,
    /// Expression morph weights by morph-target name.
    pub expressions: BTreeMap<String, f32>,
    /// Pose overrides: joint name -> Euler degrees `[x, y, z]`
    /// in the joint's local space (see `math::quat_from_euler_deg`
    /// for the composition convention).
    pub pose: BTreeMap<String, [f32; 3]>,
    /// Rig/base reference.
    pub rig: RigRef,
    /// Monotonic revision, bumped by the shell on every save.
    pub revision: u32,
    /// Last-edit stamp, written by the shell (opaque string).
    pub updated_at: String,
}

impl CharacterRecord {
    /// The morph weight map the deform path applies: expression
    /// weights plus `body_params.bust_size` routed onto the
    /// `Body_Bust_Size` morph. Weights are clamped to
    /// `[0.0, 1.5]` — past 1.0 is deliberate overshoot room the
    /// estate's morphs are authored to tolerate, never a default.
    pub fn morph_weights(&self) -> BTreeMap<String, f32> {
        let mut out: BTreeMap<String, f32> = self
            .expressions
            .iter()
            .map(|(k, v)| (k.clone(), v.clamp(0.0, 1.5)))
            .collect();
        if let Some(bust) = self.body_params.get("bust_size") {
            out.insert("Body_Bust_Size".to_string(), bust.clamp(0.0, 1.5));
        }
        out
    }

    /// The root height scale (`body_params.height`, default 1.0),
    /// clamped to a sane band so a typo cannot flatten a rig.
    pub fn height_scale(&self) -> f32 {
        self.body_params
            .get("height")
            .copied()
            .unwrap_or(1.0)
            .clamp(0.85, 1.15)
    }

    /// Serialise canonically (struct field order is fixed,
    /// maps are `BTreeMap`s, so equal records serialise equal).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
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
/// the bundled Prima base GLB.
pub fn prima_template() -> CharacterRecord {
    CharacterRecord {
        schema: SCHEMA.to_string(),
        id: "prima".to_string(),
        name: "Prima".to_string(),
        identity_lock: IdentityLock {
            step0_status: "locked".to_string(),
            source: "Canonical-base RUNG0-LOCK (identity lock held 2026-11-24)".to_string(),
        },
        prompt_identity: PromptIdentity {
            tags: "1girl, solo, very long wavy silver-white hair, lavender-grey hair shading, \
                   centre part, face-framing locks, deep red eyes, fair skin"
                .to_string(),
            signature: "pink dress, frilled dress".to_string(),
            negative_guards: String::new(),
            source: "torogao-mobile-core cast table (PRIMA), from RUNG0-LOCK".to_string(),
        },
        body_params: BTreeMap::from([("bust_size".to_string(), 0.0), ("height".to_string(), 1.0)]),
        face_params: BTreeMap::new(),
        expressions: BTreeMap::new(),
        pose: BTreeMap::new(),
        rig: RigRef {
            base_ref:
                "prima.glb sha256 8cafa7ae32372319970a174cbadd9d48b82c7b0efa424cacd159a938970a8dfd \
                       (torogao-models-20261104, gated fleet GLB)"
                    .to_string(),
            base_asset: "bundled:assets/prima-base.glb".to_string(),
        },
        revision: 1,
        updated_at: "2026-11-27".to_string(),
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
    fn bust_param_routes_onto_the_body_morph_and_clamps() {
        let mut rec = prima_template();
        rec.body_params.insert("bust_size".to_string(), 4.0);
        let w = rec.morph_weights();
        assert_eq!(w.get("Body_Bust_Size"), Some(&1.5));
        assert_eq!(rec.height_scale(), 1.0);
        rec.body_params.insert("height".to_string(), 9.0);
        assert_eq!(rec.height_scale(), 1.15);
    }
}
