//! NTRanch admission and immutable scene identity. No label rewriting or canonization.
use crate::schema::{self, CharacterRecord, ContentLabel, Provenance};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const STYLE_IDS: [&str; 4] = [
    "plate-3d",
    "sheet-illustrated",
    "performance-3d",
    "doujin-graphic",
];
pub const CAST_IDS: [&str; 6] = ["prima", "alma", "lux", "piper", "sloane", "yui"];

/// Missing assertions are distinct from explicit refusal; both fail closed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consent {
    pub adult: Option<bool>,
    pub consenting: Option<bool>,
    pub wants_the_assigned_scene: Option<bool>,
}
impl Consent {
    fn validate(&self, subject: &str) -> Result<(), String> {
        for (field, value) in [
            ("adult", self.adult),
            ("consenting", self.consenting),
            ("wants_the_assigned_scene", self.wants_the_assigned_scene),
        ] {
            if value != Some(true) {
                return Err(format!(
                    "{subject}: {field} must be explicitly true (got {value:?})"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastInput {
    pub canon_id: String,
    pub provenance: Provenance,
    pub consent: Consent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub provenance: Provenance,
    pub consent: Consent,
    pub cast: Vec<CastInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StyleRecord {
    id: String,
    status: &'static str,
    provenance: Provenance,
}
impl StyleRecord {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn status(&self) -> &str {
        self.status
    }
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

fn admission(p: &Provenance, subject: &str) -> Result<(), String> {
    if p.content_label != ContentLabel::Nsfw {
        return Err(format!(
            "{subject}: SFW ingress refused; NSFW label required"
        ));
    }
    if p.origin_lane != "ntranch" {
        return Err(format!("{subject}: origin_lane must be ntranch"));
    }
    if p.evidence.is_empty() || p.evidence.iter().any(|e| e.trim().is_empty()) {
        return Err(format!("{subject}: nonempty evidence required"));
    }
    p.validate().map_err(|e| format!("{subject}: {e}"))
}

/// Four candidate records, born locally with explicit provenance.
pub fn styles() -> [StyleRecord; 4] {
    STYLE_IDS.map(|id| StyleRecord {
        id: id.into(),
        status: "candidate",
        provenance: Provenance {
            origin_lane: "ntranch".into(),
            content_label: ContentLabel::Nsfw,
            evidence: vec!["Lane B Stage 2 commission: candidate styles, never canonized".into()],
        },
    })
}

/// An admitted canon record always travels with explicit consent metadata.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedCast {
    record: CharacterRecord,
    consent: Consent,
}
impl ResolvedCast {
    pub fn record(&self) -> &CharacterRecord {
        &self.record
    }
    pub fn consent(&self) -> &Consent {
        &self.consent
    }
}

/// Compile-time snapshots of the six authoritative local records: no path ingress.
pub fn resolve_cast(input: &CastInput) -> Result<ResolvedCast, String> {
    schema::canonical(input)?;
    let record = match input.canon_id.as_str() {
        "prima" => include_str!("../../../docs/cast-records/prima.md"),
        "alma" => include_str!("../../../docs/cast-records/alma.md"),
        "lux" => include_str!("../../../docs/cast-records/lux.md"),
        "piper" => include_str!("../../../docs/cast-records/piper.md"),
        "sloane" => include_str!("../../../docs/cast-records/sloane.md"),
        "yui" => include_str!("../../../docs/cast-records/yui.md"),
        id => return Err(format!("unknown cast ID: {id}")),
    };
    admission(&input.provenance, &input.canon_id)?;
    input.consent.validate(&input.canon_id)?;
    let mut provenance = input.provenance.clone();
    provenance
        .evidence
        .push(format!("docs/cast-records/{}.md", input.canon_id));
    Ok(ResolvedCast {
        record: CharacterRecord {
            provenance,
            canon_id: input.canon_id.clone(),
            canon_record_sha256: schema::sha256(record.as_bytes()),
        },
        consent: input.consent.clone(),
    })
}

/// Private owned fields and read-only accessors prevent post-admission mutation.
/// This is an integrity identity, not a signature or proof of human authorization.
#[derive(Debug, Clone)]
pub struct Instance {
    frozen: schema::Frozen,
    metadata: Metadata,
    style: StyleRecord,
    bytes: Vec<u8>,
    hash: String,
}
#[derive(Serialize)]
struct Envelope<'a> {
    envelope_version: u32,
    frozen: &'a schema::Frozen,
    metadata: &'a Metadata,
    style: &'a StyleRecord,
}
impl Instance {
    pub fn seal(
        script: &schema::SceneScript,
        channels: BTreeMap<String, schema::ChannelRange>,
        metadata: Metadata,
    ) -> Result<Self, String> {
        // Shared canonical limits bound work before local collection traversal.
        schema::canonical(&metadata)?;
        schema::canonical(script)?;
        schema::canonical(&channels)?;
        admission(&script.provenance, "scene script")?;
        admission(&metadata.provenance, "scene metadata")?;
        metadata.consent.validate("scene")?;
        let style = styles()
            .into_iter()
            .find(|s| s.id == script.style_category)
            .ok_or_else(|| format!("unknown style: {}", script.style_category))?;
        if metadata.cast.is_empty() || metadata.cast.len() > CAST_IDS.len() {
            return Err("cast metadata bounds: expected 1..=6".into());
        }
        let mut ids = BTreeSet::new();
        let mut characters = Vec::new();
        for cast in &metadata.cast {
            if !ids.insert(cast.canon_id.as_str()) {
                return Err(format!("duplicate cast metadata: {}", cast.canon_id));
            }
            characters.push(resolve_cast(cast)?.record);
        }
        let mut assigned = BTreeSet::new();
        for id in &script.cast {
            if !assigned.insert(id.as_str()) {
                return Err(format!("duplicate scene cast: {id}"));
            }
            if !ids.contains(id.as_str()) {
                return Err(format!("unresolved scene cast: {id}"));
            }
        }
        if assigned != ids {
            return Err("scene cast and consent metadata must match exactly".into());
        }
        let registry = schema::Registry {
            provenance: script.provenance.clone(),
            characters,
            channels,
        };
        let frozen = schema::freeze(script, &registry)?;
        let bytes = schema::canonical(&Envelope {
            envelope_version: 1,
            frozen: &frozen,
            metadata: &metadata,
            style: &style,
        })?;
        let hash = schema::sha256(&bytes);
        Ok(Self {
            frozen,
            metadata,
            style,
            bytes,
            hash,
        })
    }
    pub fn hash(&self) -> &str {
        &self.hash
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn frozen(&self) -> &schema::Frozen {
        &self.frozen
    }
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    pub fn style(&self) -> &StyleRecord {
        &self.style
    }
    pub fn verify(&self) -> Result<(), String> {
        let bytes = schema::canonical(&Envelope {
            envelope_version: 1,
            frozen: &self.frozen,
            metadata: &self.metadata,
            style: &self.style,
        })?;
        if bytes != self.bytes
            || schema::sha256(&bytes) != self.hash
            || schema::sha256(self.frozen.bytes()) != self.frozen.hash()
        {
            return Err("instance seal integrity mismatch".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provenance() -> Provenance {
        Provenance {
            origin_lane: "ntranch".into(),
            content_label: ContentLabel::Nsfw,
            evidence: vec!["explicit test fixture authorization".into()],
        }
    }
    fn consent() -> Consent {
        Consent {
            adult: Some(true),
            consenting: Some(true),
            wants_the_assigned_scene: Some(true),
        }
    }
    fn fixture() -> (
        schema::SceneScript,
        BTreeMap<String, schema::ChannelRange>,
        Metadata,
    ) {
        let script = schema::SceneScript {
            schema_version: schema::SCHEMA_VERSION,
            provenance: provenance(),
            style_category: "plate-3d".into(),
            fps: 24,
            length_frames: 24,
            cast: vec!["prima".into()],
            shots: vec![schema::Shot {
                duration_frames: 24,
                camera: vec![schema::CameraKey {
                    frame: 0,
                    position: [0.0, 0.0, 1.0],
                    target: [0.0; 3],
                    focal_length_mm: 50.0,
                }],
            }],
            performance: vec![],
            sound: vec![],
        };
        let channels = BTreeMap::from([(
            "staging".into(),
            schema::ChannelRange {
                family: schema::PerformanceChannel::Staging,
                min: 0.0,
                max: 1.0,
            },
        )]);
        let metadata = Metadata {
            provenance: provenance(),
            consent: consent(),
            cast: vec![CastInput {
                canon_id: "prima".into(),
                provenance: provenance(),
                consent: consent(),
            }],
        };
        (script, channels, metadata)
    }
    #[test]
    fn valid_instance_seals_and_verifies() {
        let (s, c, m) = fixture();
        let instance = Instance::seal(&s, c, m).unwrap();
        instance.verify().unwrap();
        assert_eq!(instance.hash(), schema::sha256(instance.bytes()));
        assert_eq!(instance.metadata().consent.adult, Some(true));
    }
    #[test]
    fn all_styles_are_candidates_with_nsfw_provenance() {
        assert_eq!(styles().map(|s| s.id), STYLE_IDS.map(String::from));
        for style in styles() {
            assert_eq!(style.status(), "candidate");
            admission(style.provenance(), style.id()).unwrap();
        }
    }
    #[test]
    fn all_six_canon_records_resolve_with_path_evidence() {
        for id in CAST_IDS {
            let record = resolve_cast(&CastInput {
                canon_id: id.into(),
                provenance: provenance(),
                consent: consent(),
            })
            .unwrap();
            assert_eq!(record.consent().adult, Some(true));
            let record = record.record();
            assert_eq!(record.canon_id, id);
            assert_eq!(record.canon_record_sha256.len(), 64);
            assert!(
                record
                    .provenance
                    .evidence
                    .contains(&format!("docs/cast-records/{id}.md"))
            );
        }
    }
    #[test]
    fn metadata_mutation_changes_identity_with_same_frozen_scene() {
        let (s, c, m) = fixture();
        let first = Instance::seal(&s, c.clone(), m.clone()).unwrap();
        let mut changed = m;
        changed
            .provenance
            .evidence
            .push("second scene authorization".into());
        let second = Instance::seal(&s, c, changed).unwrap();
        assert_eq!(first.frozen().bytes(), second.frozen().bytes());
        assert_ne!(first.bytes(), second.bytes());
        assert_ne!(first.hash(), second.hash());
        first.verify().unwrap();
        second.verify().unwrap();
    }
    #[test]
    fn repeat_sealing_is_deterministic() {
        let (s, c, m) = fixture();
        assert_eq!(
            Instance::seal(&s, c.clone(), m.clone()).unwrap().bytes(),
            Instance::seal(&s, c, m).unwrap().bytes()
        );
    }
    #[test]
    fn every_candidate_style_can_seal() {
        let (mut s, c, m) = fixture();
        for style in STYLE_IDS {
            s.style_category = style.into();
            Instance::seal(&s, c.clone(), m.clone())
                .unwrap()
                .verify()
                .unwrap();
        }
    }
    #[test]
    fn all_six_cast_can_share_a_scene() {
        let (mut s, c, mut m) = fixture();
        s.cast = CAST_IDS.map(String::from).to_vec();
        m.cast = CAST_IDS
            .map(|id| CastInput {
                canon_id: id.into(),
                provenance: provenance(),
                consent: consent(),
            })
            .to_vec();
        Instance::seal(&s, c, m).unwrap().verify().unwrap();
    }
    #[test]
    fn frozen_scene_uses_shared_freeze() {
        let (s, c, m) = fixture();
        let registry = schema::Registry {
            provenance: m.provenance.clone(),
            characters: m
                .cast
                .iter()
                .map(|v| resolve_cast(v).unwrap().record)
                .collect(),
            channels: c.clone(),
        };
        assert_eq!(
            Instance::seal(&s, c, m).unwrap().frozen().bytes(),
            schema::freeze(&s, &registry).unwrap().bytes()
        );
    }
    #[test]
    fn unknown_style_refused_by_name() {
        let (mut s, c, m) = fixture();
        s.style_category = "invented".into();
        assert!(
            Instance::seal(&s, c, m)
                .unwrap_err()
                .contains("unknown style: invented")
        );
    }
    #[test]
    fn unknown_cast_refused_by_name() {
        let (_, _, mut m) = fixture();
        m.cast[0].canon_id = "intruder".into();
        assert!(
            resolve_cast(&m.cast[0])
                .unwrap_err()
                .contains("unknown cast ID: intruder")
        );
    }
    #[test]
    fn missing_and_false_consent_refused_for_scene_and_cast() {
        for subject in 0..2 {
            for field in 0..3 {
                for value in [None, Some(false)] {
                    let (s, c, mut m) = fixture();
                    let consent = if subject == 0 {
                        &mut m.consent
                    } else {
                        &mut m.cast[0].consent
                    };
                    match field {
                        0 => consent.adult = value,
                        1 => consent.consenting = value,
                        _ => consent.wants_the_assigned_scene = value,
                    }
                    let error = Instance::seal(&s, c, m).unwrap_err();
                    assert!(
                        error.contains(["adult", "consenting", "wants_the_assigned_scene"][field])
                    );
                    assert!(error.contains(if subject == 0 { "scene:" } else { "prima:" }));
                }
            }
        }
    }
    #[test]
    fn sfw_and_wrong_origin_ingress_refused_everywhere() {
        for subject in 0..3 {
            for sfw in [true, false] {
                let (mut s, c, mut m) = fixture();
                let p = match subject {
                    0 => &mut s.provenance,
                    1 => &mut m.provenance,
                    _ => &mut m.cast[0].provenance,
                };
                if sfw {
                    p.content_label = ContentLabel::Sfw;
                } else {
                    p.origin_lane = "other".into();
                }
                assert!(Instance::seal(&s, c, m).unwrap_err().contains(if sfw {
                    "SFW ingress"
                } else {
                    "origin_lane"
                }));
            }
        }
    }
    #[test]
    fn missing_label_and_evidence_refused() {
        for field in ["content_label", "evidence"] {
            let mut value = serde_json::to_value(provenance()).unwrap();
            value.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<Provenance>(value)
                    .unwrap_err()
                    .to_string()
                    .contains(field)
            );
        }
        for subject in 0..3 {
            for evidence in [vec![], vec![" ".into()]] {
                let (mut s, c, mut m) = fixture();
                let p = match subject {
                    0 => &mut s.provenance,
                    1 => &mut m.provenance,
                    _ => &mut m.cast[0].provenance,
                };
                p.evidence = evidence;
                assert!(
                    Instance::seal(&s, c, m)
                        .unwrap_err()
                        .contains("nonempty evidence")
                );
            }
        }
    }
    #[test]
    fn duplicates_refused_by_name() {
        let (s, c, mut m) = fixture();
        m.cast.push(m.cast[0].clone());
        assert!(
            Instance::seal(&s, c, m)
                .unwrap_err()
                .contains("duplicate cast metadata: prima")
        );
        let (mut s, c, m) = fixture();
        s.cast.push("prima".into());
        assert!(
            Instance::seal(&s, c, m)
                .unwrap_err()
                .contains("duplicate scene cast: prima")
        );
    }
    #[test]
    fn missing_or_extra_cast_metadata_refused() {
        let (mut s, c, m) = fixture();
        s.cast.push("alma".into());
        assert!(
            Instance::seal(&s, c, m)
                .unwrap_err()
                .contains("unresolved scene cast: alma")
        );
        let (s, c, mut m) = fixture();
        m.cast.push(CastInput {
            canon_id: "alma".into(),
            provenance: provenance(),
            consent: consent(),
        });
        assert!(
            Instance::seal(&s, c, m)
                .unwrap_err()
                .contains("must match exactly")
        );
    }
    #[test]
    fn tampering_and_invalid_shared_scene_refused() {
        let (s, c, m) = fixture();
        let mut instance = Instance::seal(&s, c, m).unwrap();
        instance.metadata.consent.adult = Some(false);
        assert!(
            instance
                .verify()
                .unwrap_err()
                .contains("integrity mismatch")
        );
        let (mut s, c, m) = fixture();
        s.fps = 0;
        assert!(Instance::seal(&s, c, m).unwrap_err().contains("fps"));
    }
}
