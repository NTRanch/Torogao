//! MMD ingest validation (Stage 2, Route A — D-633).
//!
//! Torogao consumes two artefacts produced on the lumen side of
//! Route A: the mapping profile as data (`mmd-profile/1`, emitted
//! by lumen's `gltf_ingest --emit-profile` from its `profile.rs`
//! tables and committed at `crates/torogao-format/data/
//! mmd-profile/1.ron`), and one `torogao-mmd-ingest/1` record per
//! converted model (lumen's `gltf-import/1` output). This module
//! is the consumer-side validator: it checks an ingest record
//! against the profile and against the torogao-humanoid/1 bone
//! vocabulary, with zero silent drops — every check either passes
//! or names its failure.
//!
//! RON parsing: the workspace `Cargo.lock` carries no `ron`
//! dependency, and this crate already builds on serde, so the
//! profile is parsed with the `ron` crate's serde route (added to
//! this crate only). The ingest record is JSON and parses through
//! the crate's existing serde_json dependency.

use serde::Deserialize;

/// The torogao-humanoid/1 bone vocabulary: the VMC bone names.
///
/// torogao-vmc carries no enumerated bone list — it anchors on
/// the `Head` and `Chest` bone poses (`arkit::HEAD_BONE` /
/// `arkit::BODY_BONE`) and the `Root` pose in `vmc.rs` — so this
/// set is the standard VMC humanoid vocabulary those anchors
/// belong to: the core chain, both arms and legs, and the full
/// VMC finger set (SPEC §6.1's mapping targets). A mapped name
/// outside this set is a validator failure, by name.
pub const VMC_HUMANOID_BONES: &[&str] = &[
    "Root",
    "Hips",
    "Spine",
    "Chest",
    "UpperChest",
    "Neck",
    "Head",
    "LeftShoulder",
    "LeftUpperArm",
    "LeftLowerArm",
    "LeftHand",
    "RightShoulder",
    "RightUpperArm",
    "RightLowerArm",
    "RightHand",
    "LeftUpperLeg",
    "LeftLowerLeg",
    "LeftFoot",
    "LeftToes",
    "RightUpperLeg",
    "RightLowerLeg",
    "RightFoot",
    "RightToes",
    "LeftThumbProximal",
    "LeftThumbIntermediate",
    "LeftThumbDistal",
    "LeftIndexProximal",
    "LeftIndexIntermediate",
    "LeftIndexDistal",
    "LeftMiddleProximal",
    "LeftMiddleIntermediate",
    "LeftMiddleDistal",
    "LeftRingProximal",
    "LeftRingIntermediate",
    "LeftRingDistal",
    "LeftLittleProximal",
    "LeftLittleIntermediate",
    "LeftLittleDistal",
    "RightThumbProximal",
    "RightThumbIntermediate",
    "RightThumbDistal",
    "RightIndexProximal",
    "RightIndexIntermediate",
    "RightIndexDistal",
    "RightMiddleProximal",
    "RightMiddleIntermediate",
    "RightMiddleDistal",
    "RightRingProximal",
    "RightRingIntermediate",
    "RightRingDistal",
    "RightLittleProximal",
    "RightLittleIntermediate",
    "RightLittleDistal",
];

/// One bone row of `mmd-profile/1` (machine-derived; see header).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProfileBone {
    /// PMX local (Japanese) bone name.
    pub pmx_local: String,
    /// English gloss.
    pub gloss: String,
    /// torogao-humanoid/1 target, or `None` when ledgered unmapped.
    pub torogao: Option<String>,
}

/// One morph row of `mmd-profile/1`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProfileMorph {
    /// PMX local (Japanese) morph name.
    pub pmx_local: String,
    /// English gloss.
    pub gloss: String,
    /// SPEC §6.2 governed channel, when the row carries one.
    pub torogao_channel: Option<String>,
    /// Canonical D-630 `Fac_` channel, when the row carries one.
    pub fac_channel: Option<String>,
}

/// One physics-posture row of `mmd-profile/1` (SPEC §6.4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProfilePhysics {
    /// The PMX record class the row covers.
    pub pmx_record: String,
    /// Its Torogao home, or `None` when unmapped/ledgered.
    pub torogao_home: Option<String>,
    /// Posture token (`mapped_shape`, `unmapped_ledgered_verbatim`, ...).
    pub posture: String,
}

/// The `mmd-profile/1` document.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MmdProfile {
    /// Profile id (`mmd-profile/1`).
    pub id: String,
    /// PMX units → metres scale (profile data, SPEC §5.1).
    pub scale_to_meters: f32,
    /// Bone rows (SPEC §6.1).
    pub bones: Vec<ProfileBone>,
    /// Morph rows (SPEC §6.2).
    pub morphs: Vec<ProfileMorph>,
    /// The D-630 canonical `Fac_` vocabulary (29 channels).
    pub fac_channels: Vec<String>,
    /// Physics posture rows (SPEC §6.4).
    pub physics: Vec<ProfilePhysics>,
}

/// Parse `mmd-profile/1` RON text.
pub fn parse_profile(text: &str) -> Result<MmdProfile, String> {
    ron::from_str(text).map_err(|err| format!("profile RON does not parse: {err}"))
}

/// The provenance block of a `torogao-mmd-ingest/1` record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IngestProvenance {
    /// SHA-256 of the source PMX (census hash), when carried.
    pub source_pmx_sha256: Option<String>,
    /// SHA-256 of the GLB the record was derived from.
    pub glb_sha256: String,
    /// Profile id the record was built against.
    pub profile_id: String,
    /// Converter id (`gltf-import/1`).
    pub converter: String,
}

/// One skeleton entry of an ingest record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IngestSkeletonEntry {
    /// PMX local bone name, verbatim.
    pub name_local: String,
    /// PMX universal bone name, verbatim.
    pub name_universal: String,
    /// torogao-humanoid/1 target, or `None` when unmapped.
    pub torogao: Option<String>,
    /// Why the bone is unmapped (required when `torogao` is None).
    pub unmapped_reason: Option<String>,
}

/// One face-parameter entry of an ingest record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IngestFaceEntry {
    /// PMX local morph name, verbatim.
    pub name_local: String,
    /// Census class (group/vertex/bone/uv/material/flip/impulse).
    #[serde(rename = "class")]
    pub class: String,
    /// Disposition: `morph_target`, `recipe`, or `ledgered`.
    pub route: String,
    /// Canonical `Fac_` channel, or `None` when unmapped.
    pub fac_channel: Option<String>,
    /// SPEC §6.2 governed channel, or `None` when unmapped.
    pub governed_channel: Option<String>,
}

/// One zero-silent-drop accounting row of an ingest record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IngestAccountingRow {
    /// Feature class (`bones`, `morphs:vertex`, `joints`, ...).
    pub feature: String,
    /// Count in the source census.
    pub census_count: usize,
    /// Count mapped into the record.
    pub mapped_count: usize,
    /// Count carried in the dropped ledger / unmapped entries.
    pub ledgered_count: usize,
}

/// A `torogao-mmd-ingest/1` record (the fields the validator
/// checks; the record's physics and ledger bodies are data for
/// downstream consumers and are not re-validated here beyond the
/// accounting identities that cover them).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IngestRecord {
    /// Schema id (`torogao-mmd-ingest/1`).
    pub schema: String,
    /// Provenance anchor block.
    pub provenance: IngestProvenance,
    /// Skeleton entries.
    pub skeleton: Vec<IngestSkeletonEntry>,
    /// Face-parameter entries (all morph classes).
    pub face_parameters: Vec<IngestFaceEntry>,
    /// Zero-silent-drop accounting rows.
    pub accounting: Vec<IngestAccountingRow>,
}

/// Parse an ingest record's JSON text.
pub fn parse_record(text: &str) -> Result<IngestRecord, String> {
    serde_json::from_str(text).map_err(|err| format!("ingest record does not parse: {err}"))
}

/// A successful validation summary (counts the verdict line names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationSummary {
    /// Skeleton entries checked.
    pub skeleton_entries: usize,
    /// Face entries checked.
    pub face_entries: usize,
    /// Accounting rows whose identities held.
    pub accounting_rows: usize,
    /// Profile bone rows consumed by a record entry.
    pub profile_bones_consumed: usize,
    /// Profile morph rows consumed by a record entry.
    pub profile_morphs_consumed: usize,
}

/// Validate an ingest record against the profile. Returns the
/// summary on success, or every named failure found (validation
/// does not stop at the first failure).
///
/// Checks (D-633 Deliverable B):
/// 1. every mapped skeleton name belongs to the
///    torogao-humanoid/1 VMC bone set;
/// 2. every face channel belongs to the profile's `Fac_`
///    vocabulary;
/// 3. the record's zero-silent-drop identities hold
///    (`mapped + ledgered == census` per accounting row);
/// 4. profile agreement: every record entry that consumes a
///    profile row agrees with it (a mapped name must be the
///    row's target and must come from a row — invented mappings
///    fail), and every unmapped entry carries its reason.
pub fn validate(
    profile: &MmdProfile,
    record: &IngestRecord,
) -> Result<ValidationSummary, Vec<String>> {
    let mut failures: Vec<String> = Vec::new();

    if record.schema != "torogao-mmd-ingest/1" {
        failures.push(format!(
            "record schema is {:?}, expected \"torogao-mmd-ingest/1\"",
            record.schema
        ));
    }
    if record.provenance.profile_id != profile.id {
        failures.push(format!(
            "record profile_id {:?} does not match profile id {:?}",
            record.provenance.profile_id, profile.id
        ));
    }

    // (1) + (4) skeleton.
    let mut profile_bones_consumed = 0usize;
    for entry in &record.skeleton {
        if let Some(target) = &entry.torogao {
            if !VMC_HUMANOID_BONES.contains(&target.as_str()) {
                failures.push(format!(
                    "skeleton entry {:?} maps to {target:?}, which is not in the \
                     torogao-humanoid/1 VMC bone set",
                    entry.name_local
                ));
            }
        } else if entry.unmapped_reason.as_deref().unwrap_or("").is_empty() {
            failures.push(format!(
                "skeleton entry {:?} is unmapped but carries no unmapped_reason",
                entry.name_local
            ));
        }
        match profile
            .bones
            .iter()
            .find(|row| row.pmx_local == entry.name_local)
        {
            Some(row) => {
                profile_bones_consumed += 1;
                if row.torogao != entry.torogao {
                    failures.push(format!(
                        "skeleton entry {:?} maps to {:?} but mmd-profile/1 says {:?}",
                        entry.name_local, entry.torogao, row.torogao
                    ));
                }
            }
            None => {
                if let Some(target) = &entry.torogao {
                    failures.push(format!(
                        "skeleton entry {:?} maps to {target:?} with no mmd-profile/1 row \
                         (invented mapping)",
                        entry.name_local
                    ));
                }
            }
        }
    }

    // (2) + (4) face parameters.
    let mut profile_morphs_consumed = 0usize;
    for entry in &record.face_parameters {
        if let Some(channel) = &entry.fac_channel
            && !profile.fac_channels.iter().any(|known| known == channel)
        {
            failures.push(format!(
                "face entry {:?} carries channel {channel:?}, which is not in the \
                 Fac_ vocabulary",
                entry.name_local
            ));
        }
        if let Some(row) = profile
            .morphs
            .iter()
            .find(|row| row.pmx_local == entry.name_local)
        {
            profile_morphs_consumed += 1;
            if row.torogao_channel != entry.governed_channel {
                failures.push(format!(
                    "face entry {:?} carries governed channel {:?} but mmd-profile/1 \
                     says {:?}",
                    entry.name_local, entry.governed_channel, row.torogao_channel
                ));
            }
            // Fac_ agreement is one-directional by design: the
            // profile row's channel comes from the standard-name
            // table, but a model's own verbatim `Fac_` universal
            // name takes precedence over that table (the D-630
            // rule lumen's `fac_channel` implements — e.g. a model
            // may author its wink-variant shape under
            // `Fac_Eye_Close`). So a *different* in-vocabulary
            // channel is the model's naming winning, not an error;
            // a *missing* channel where the profile row carries
            // one is a dropped mapping and fails.
            if row.fac_channel.is_some()
                && entry.route == "morph_target"
                && entry.fac_channel.is_none()
            {
                failures.push(format!(
                    "face entry {:?} carries no Fac_ channel but mmd-profile/1 \
                     maps it to {:?}",
                    entry.name_local, row.fac_channel
                ));
            }
        }
    }

    // (3) zero-silent-drop identities.
    for row in &record.accounting {
        if row.mapped_count + row.ledgered_count != row.census_count {
            failures.push(format!(
                "accounting identity failed for {}: mapped {} + ledgered {} != census {}",
                row.feature, row.mapped_count, row.ledgered_count, row.census_count
            ));
        }
    }

    if failures.is_empty() {
        Ok(ValidationSummary {
            skeleton_entries: record.skeleton.len(),
            face_entries: record.face_parameters.len(),
            accounting_rows: record.accounting.len(),
            profile_bones_consumed,
            profile_morphs_consumed,
        })
    } else {
        Err(failures)
    }
}

/// Validate profile RON text + record JSON text (the CLI route).
pub fn validate_texts(
    profile_text: &str,
    record_text: &str,
) -> Result<ValidationSummary, Vec<String>> {
    let profile = parse_profile(profile_text).map_err(|err| vec![err])?;
    let record = parse_record(record_text).map_err(|err| vec![err])?;
    validate(&profile, &record)
}

#[cfg(test)]
mod tests {
    use super::{parse_profile, validate, validate_texts};

    const PROFILE_RON: &str = r##"(
        id: "mmd-profile/1",
        scale_to_meters: 0.08,
        bones: [
            (pmx_local: "センター", gloss: "center", torogao: Some("Hips")),
            (pmx_local: "頭", gloss: "head", torogao: Some("Head")),
            (pmx_local: "左手先", gloss: "left hand tip", torogao: None),
        ],
        morphs: [
            (pmx_local: "まばたき", gloss: "blink", torogao_channel: Some("blink"), fac_channel: Some("Fac_Eye_Close")),
        ],
        fac_channels: ["Fac_Eye_Close", "Fac_Mth_Aa"],
        physics: [
            (pmx_record: "joint spring constants (per axis)", torogao_home: None, posture: "unmapped_ledgered_verbatim"),
        ],
    )"##;

    fn record_json(skeleton_torogao: &str, ledgered: usize) -> String {
        format!(
            r##"{{
                "schema": "torogao-mmd-ingest/1",
                "provenance": {{
                    "source_pmx_sha256": null,
                    "glb_sha256": "abc",
                    "profile_id": "mmd-profile/1",
                    "converter": "gltf-import/1"
                }},
                "skeleton": [
                    {{"name_local": "センター", "name_universal": "center",
                      "torogao": {skeleton_torogao}, "unmapped_reason": null}},
                    {{"name_local": "左手先", "name_universal": "",
                      "torogao": null, "unmapped_reason": "no humanoid slot"}}
                ],
                "face_parameters": [
                    {{"name_local": "まばたき", "name_universal": "blink",
                      "class": "vertex", "route": "morph_target",
                      "fac_channel": "Fac_Eye_Close", "governed_channel": "blink"}}
                ],
                "accounting": [
                    {{"feature": "bones", "census_count": 2, "mapped_count": 1,
                      "ledgered_count": {ledgered}}}
                ]
            }}"##
        )
    }

    // ---- Validation ------------------------------------------------

    #[test]
    fn valid_record_passes() {
        let summary = validate_texts(PROFILE_RON, &record_json("\"Hips\"", 1))
            .expect("valid record must pass");
        assert_eq!(summary.skeleton_entries, 2);
        assert_eq!(summary.face_entries, 1);
        assert_eq!(summary.accounting_rows, 1);
        assert_eq!(summary.profile_bones_consumed, 2);
        assert_eq!(summary.profile_morphs_consumed, 1);
    }

    #[test]
    fn verbatim_fac_name_may_override_the_profile_row() {
        // The model's own verbatim Fac_ universal name wins over
        // the standard-name table (D-630): a different
        // in-vocabulary channel is not a disagreement.
        let record = record_json("\"Hips\"", 1).replace("Fac_Eye_Close", "Fac_Mth_Aa");
        validate_texts(PROFILE_RON, &record).expect("verbatim override must pass");
    }

    #[test]
    fn committed_profile_data_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/mmd-profile/1.ron");
        let text = std::fs::read_to_string(path).expect("committed profile data exists");
        let profile = parse_profile(&text).expect("committed profile parses");
        assert_eq!(profile.id, "mmd-profile/1");
        assert_eq!(profile.fac_channels.len(), 29);
        assert!(profile.bones.len() >= 30);
        assert!(profile.morphs.len() >= 20);
    }

    // ---- Adversarial -----------------------------------------------

    #[test]
    fn renamed_bone_fails_by_name() {
        let failures = validate_texts(PROFILE_RON, &record_json("\"Hipz\"", 1))
            .expect_err("renamed bone must fail");
        assert!(
            failures.iter().any(|failure| failure.contains("Hipz")),
            "failure names the bone target: {failures:?}"
        );
    }

    #[test]
    fn ledger_tampering_fails_the_identity_check() {
        let failures = validate_texts(PROFILE_RON, &record_json("\"Hips\"", 2))
            .expect_err("tampered ledger must fail");
        assert!(
            failures
                .iter()
                .any(|failure| failure.contains("accounting identity failed for bones")),
            "failure names the identity: {failures:?}"
        );
    }

    #[test]
    fn profile_disagreement_fails() {
        // センター mapped to Head: in the VMC set, but not the
        // profile's target for this PMX name.
        let failures = validate_texts(PROFILE_RON, &record_json("\"Head\"", 1))
            .expect_err("profile disagreement must fail");
        assert!(
            failures.iter().any(|failure| failure.contains("センター")),
            "failure names the entry: {failures:?}"
        );
    }

    #[test]
    fn bogus_fac_channel_fails() {
        let record = record_json("\"Hips\"", 1).replace("Fac_Eye_Close", "Fac_Eye_Bogus");
        let failures = validate_texts(PROFILE_RON, &record).expect_err("bogus channel must fail");
        assert!(
            failures
                .iter()
                .any(|failure| failure.contains("Fac_Eye_Bogus")),
            "failure names the channel: {failures:?}"
        );
    }

    #[test]
    fn dropped_fac_channel_fails() {
        // The profile row maps まばたき to a Fac_ channel; a
        // record that drops it (null) has silently lost a mapping.
        let record = record_json("\"Hips\"", 1).replace(
            "\"fac_channel\": \"Fac_Eye_Close\"",
            "\"fac_channel\": null",
        );
        let failures = validate_texts(PROFILE_RON, &record).expect_err("dropped channel must fail");
        assert!(
            failures.iter().any(|failure| failure.contains("まばたき")),
            "failure names the entry: {failures:?}"
        );
    }

    #[test]
    fn unmapped_without_reason_fails() {
        let record = record_json("\"Hips\"", 1).replace(
            "\"unmapped_reason\": \"no humanoid slot\"",
            "\"unmapped_reason\": null",
        );
        let profile = parse_profile(PROFILE_RON).expect("profile parses");
        let parsed = super::parse_record(&record).expect("record parses");
        let failures = validate(&profile, &parsed).expect_err("reasonless unmapped must fail");
        assert!(
            failures.iter().any(|failure| failure.contains("左手先")),
            "failure names the entry: {failures:?}"
        );
    }
}
