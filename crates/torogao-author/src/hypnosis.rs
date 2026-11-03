//! The Q1 hypnosis authoring verbs (D-634;
//! TOROGAO-HYPNO-CAPABILITY §b and §c): `audio_binding_set`
//! and `visual_binding_set`, through the same
//! `decide()` → apply → op-log path as every other family.
//!
//! `audio_binding_set` is the first verb that creates
//! `AudioBinding` rows at all — before Q1 the rows could
//! exist in a record but nothing authored them, which
//! blocked every subliminal-audio pattern. It authors both
//! the voice kind (the torori seam) and the hypnosis-track
//! kind (a session reference plus its layer map).
//! `visual_binding_set` authors the visual induction
//! channels: loop bindings and seeded overlay hooks.
//!
//! Design choices where the capability spec left the shape
//! open, recorded here: a binding verb *sets* its row (same
//! id + same record is a no-op; same id + different record
//! replaces, matching `audio_bind_set`'s posture); a layer's
//! empty edition mask means every edition; layers are stored
//! sorted by kind label so the record is canonical.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::decide::{Verdict, check_id, check_number};
use crate::limits;
use crate::model::{
    AudioBinding, AudioBindingKind, DeliveryRef, HypnosisLayer, HypnosisLayerKind,
    HypnosisTrackBinding, LoopSync, OverlayHookBinding, Project, VisualBinding, VisualBindingKind,
    VisualLoopBinding, VoiceBinding,
};
use crate::op::Op;
use crate::refusal::{Refusal, RefusalClass, refuse};

// ---- inputs ------------------------------------------------------

/// A torori delivery reference, in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryRefInput {
    /// The torori VoiceIdentity id.
    pub identity: String,
    /// The torori conditioning-v1 record reference.
    pub record: String,
}

/// One hypnosis layer, in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct HypnosisLayerInput {
    /// The layer kind label (voice/carrier/bed/subliminal/sfx).
    pub kind: String,
    /// The stem reference.
    pub stem: String,
    /// Gain offset in dB.
    #[serde(default)]
    pub gain_offset_db: f64,
    /// The named gain envelope, when bound.
    #[serde(default)]
    pub envelope: Option<String>,
    /// Named editions including this layer (empty = all).
    #[serde(default)]
    pub editions: Vec<String>,
    /// Whether the layer loops (the drone posture).
    #[serde(default)]
    pub looped: bool,
}

/// The binding payload, in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum AudioBindingKindInput {
    /// A voice binding (the torori seam).
    Voice {
        /// The speaking character's id.
        character: String,
        /// The torori delivery reference.
        delivery: DeliveryRefInput,
    },
    /// A hypnosis-track binding.
    HypnosisTrack {
        /// The session reference.
        session: String,
        /// The layer stack.
        layers: Vec<HypnosisLayerInput>,
    },
}

/// Inputs of `audio_binding_set`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioBindingSetInput {
    /// Stable id.
    pub id: String,
    /// The binding payload.
    pub binding: AudioBindingKindInput,
}

/// How a visual loop covers its session, in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "mode")]
pub enum LoopSyncInput {
    /// Repeat the loop this many times.
    Count {
        /// The repeat count.
        count: u32,
    },
    /// Stretch the loop to the session.
    Stretch,
}

/// The visual payload, in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum VisualBindingKindInput {
    /// A visual loop binding.
    VisualLoop {
        /// The loop asset reference.
        asset: String,
        /// The session reference.
        session: String,
        /// The sync mode.
        sync: LoopSyncInput,
        /// Declared loop duration in seconds.
        duration_seconds: f64,
        /// Declared frame rate.
        frame_rate: u32,
    },
    /// A procedural overlay hook binding.
    OverlayHook {
        /// The named hook.
        hook: String,
        /// The session reference.
        session: String,
        /// The parameter seed.
        seed: i64,
        /// Rotation rate in radians per second.
        rotation_rate: f64,
        /// The named opacity envelope.
        opacity_envelope: String,
    },
}

/// Inputs of `visual_binding_set`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisualBindingSetInput {
    /// Stable id.
    pub id: String,
    /// The binding payload.
    pub binding: VisualBindingKindInput,
}

// ---- record builders ----------------------------------------------

fn layer_record(inputs: &HypnosisLayerInput) -> Result<HypnosisLayer, Refusal> {
    let Some(kind) = HypnosisLayerKind::from_label(&inputs.kind) else {
        return refuse(
            RefusalClass::UnknownKind,
            format!("hypnosis layer kind '{}' is not a stack layer", inputs.kind),
        );
    };
    Ok(HypnosisLayer {
        kind,
        stem: inputs.stem.clone(),
        gain_offset_db: inputs.gain_offset_db,
        envelope: inputs.envelope.clone(),
        editions: inputs.editions.clone(),
        looped: inputs.looped,
    })
}

/// The canonical binding record for these inputs (layers
/// sorted by kind label, editions sorted).
pub fn binding_record(inputs: &AudioBindingSetInput) -> Result<AudioBinding, Refusal> {
    let kind = match &inputs.binding {
        AudioBindingKindInput::Voice {
            character,
            delivery,
        } => AudioBindingKind::Voice(VoiceBinding {
            character: character.clone(),
            delivery: DeliveryRef {
                identity: delivery.identity.clone(),
                record: delivery.record.clone(),
            },
        }),
        AudioBindingKindInput::HypnosisTrack { session, layers } => {
            let mut records: Vec<HypnosisLayer> =
                layers.iter().map(layer_record).collect::<Result<_, _>>()?;
            records.sort_by(|a, b| a.kind.label().cmp(b.kind.label()));
            for layer in &mut records {
                layer.editions.sort();
                layer.editions.dedup();
            }
            AudioBindingKind::HypnosisTrack(HypnosisTrackBinding {
                session: session.clone(),
                layers: records,
            })
        }
    };
    Ok(AudioBinding {
        id: inputs.id.clone(),
        kind,
    })
}

fn sync_record(inputs: &LoopSyncInput) -> LoopSync {
    match inputs {
        LoopSyncInput::Count { count } => LoopSync::Count(*count),
        LoopSyncInput::Stretch => LoopSync::Stretch,
    }
}

/// The canonical visual-binding record for these inputs.
pub fn visual_record(inputs: &VisualBindingSetInput) -> VisualBinding {
    let kind = match &inputs.binding {
        VisualBindingKindInput::VisualLoop {
            asset,
            session,
            sync,
            duration_seconds,
            frame_rate,
        } => VisualBindingKind::VisualLoop(VisualLoopBinding {
            asset: asset.clone(),
            session: session.clone(),
            sync: sync_record(sync),
            duration_seconds: *duration_seconds,
            frame_rate: *frame_rate,
        }),
        VisualBindingKindInput::OverlayHook {
            hook,
            session,
            seed,
            rotation_rate,
            opacity_envelope,
        } => VisualBindingKind::OverlayHook(OverlayHookBinding {
            hook: hook.clone(),
            session: session.clone(),
            seed: *seed,
            rotation_rate: *rotation_rate,
            opacity_envelope: opacity_envelope.clone(),
        }),
    };
    VisualBinding {
        id: inputs.id.clone(),
        kind,
    }
}

// ---- decide --------------------------------------------------------

/// A reference string's facts: present, within the provenance
/// text bound. References are opaque namespace strings (a
/// torori record, a session id) — never id-checked here.
fn check_ref(value: &str, what: &str) -> Result<(), Refusal> {
    if value.is_empty() || value.len() > limits::PROVENANCE_TEXT_BYTES_MAX {
        return refuse(
            RefusalClass::Malformed,
            format!("{what} reference is empty or over the bound"),
        );
    }
    Ok(())
}

/// Decide one hypnosis op against the project.
pub fn decide_hypnosis(project: &Project, op: &Op) -> Result<Verdict, Refusal> {
    match op {
        Op::AudioBindingSet(inputs) => decide_audio_binding(project, inputs),
        Op::VisualBindingSet(inputs) => decide_visual_binding(project, inputs),
        other => unreachable!("decide_hypnosis called with {}", other.verb()),
    }
}

fn decide_audio_binding(
    project: &Project,
    inputs: &AudioBindingSetInput,
) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "id")?;
    match &inputs.binding {
        AudioBindingKindInput::Voice {
            character,
            delivery,
        } => {
            check_id(character, "character")?;
            check_ref(&delivery.identity, "delivery identity")?;
            check_ref(&delivery.record, "delivery record")?;
        }
        AudioBindingKindInput::HypnosisTrack { session, layers } => {
            check_ref(session, "session")?;
            if layers.is_empty() {
                return refuse(
                    RefusalClass::Malformed,
                    "a hypnosis track names at least one layer: a track is its stack",
                );
            }
            if layers.len() > limits::HYPNOSIS_LAYERS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "track declares {} layers, over the bound of {}",
                        layers.len(),
                        limits::HYPNOSIS_LAYERS_MAX
                    ),
                );
            }
            let mut kinds = BTreeSet::new();
            for layer in layers {
                let record = layer_record(layer)?;
                if !kinds.insert(record.kind.label()) {
                    return refuse(
                        RefusalClass::Malformed,
                        format!("track names layer kind '{}' twice", record.kind.label()),
                    );
                }
                check_ref(&layer.stem, "layer stem")?;
                check_number(layer.gain_offset_db, "gain_offset_db")?;
                if layer.gain_offset_db.abs() > limits::HYPNOSIS_GAIN_OFFSET_DB_MAX {
                    return refuse(
                        RefusalClass::Range,
                        format!(
                            "layer gain offset {} dB exceeds the bound {}",
                            layer.gain_offset_db,
                            limits::HYPNOSIS_GAIN_OFFSET_DB_MAX
                        ),
                    );
                }
                if let Some(envelope) = &layer.envelope {
                    check_ref(envelope, "layer envelope")?;
                }
                let mut editions = BTreeSet::new();
                for edition in &layer.editions {
                    check_id(edition, "edition")?;
                    if !editions.insert(edition.as_str()) {
                        return refuse(
                            RefusalClass::Malformed,
                            format!("layer names edition '{edition}' twice"),
                        );
                    }
                }
            }
        }
    }
    let record = binding_record(inputs)?;
    match project.audio_binding(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => Ok(Verdict::Apply),
        None => {
            if project.audio_bindings.len() + 1 > limits::AUDIO_BINDINGS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "project already holds {} audio bindings, the bound",
                        limits::AUDIO_BINDINGS_MAX
                    ),
                );
            }
            Ok(Verdict::Apply)
        }
    }
}

fn decide_visual_binding(
    project: &Project,
    inputs: &VisualBindingSetInput,
) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "id")?;
    match &inputs.binding {
        VisualBindingKindInput::VisualLoop {
            asset,
            session,
            sync,
            duration_seconds,
            frame_rate,
        } => {
            check_ref(asset, "loop asset")?;
            check_ref(session, "session")?;
            if let LoopSyncInput::Count { count } = sync {
                if *count == 0 {
                    return refuse(
                        RefusalClass::Range,
                        "a counted visual loop repeats at least once",
                    );
                }
            }
            check_number(*duration_seconds, "duration_seconds")?;
            if *duration_seconds <= 0.0 || *duration_seconds > limits::VISUAL_LOOP_SECONDS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "loop duration {duration_seconds} s outside (0, {}]",
                        limits::VISUAL_LOOP_SECONDS_MAX
                    ),
                );
            }
            if *frame_rate == 0 || *frame_rate > 240 {
                return refuse(
                    RefusalClass::Range,
                    format!("loop frame rate {frame_rate} outside [1, 240]"),
                );
            }
        }
        VisualBindingKindInput::OverlayHook {
            hook,
            session,
            seed: _,
            rotation_rate,
            opacity_envelope,
        } => {
            check_ref(hook, "overlay hook")?;
            check_ref(session, "session")?;
            check_ref(opacity_envelope, "opacity envelope")?;
            check_number(*rotation_rate, "rotation_rate")?;
            if rotation_rate.abs() > 100.0 {
                return refuse(
                    RefusalClass::Range,
                    format!("rotation rate {rotation_rate} rad/s exceeds the bound 100"),
                );
            }
        }
    }
    let record = visual_record(inputs);
    match project.visual_binding(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => Ok(Verdict::Apply),
        None => {
            if project.visual_bindings.len() + 1 > limits::VISUAL_BINDINGS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "project already holds {} visual bindings, the bound",
                        limits::VISUAL_BINDINGS_MAX
                    ),
                );
            }
            Ok(Verdict::Apply)
        }
    }
}

// ---- apply -----------------------------------------------------------

/// Apply one decided hypnosis op to the project.
pub fn apply_hypnosis(project: &mut Project, op: &Op) {
    match op {
        Op::AudioBindingSet(inputs) => {
            let record = binding_record(inputs).expect("decided inputs build a record");
            let at = project
                .audio_bindings
                .partition_point(|binding| binding.id < record.id);
            if project.audio_bindings.get(at).map(|b| &b.id) == Some(&record.id) {
                project.audio_bindings[at] = record;
            } else {
                project.audio_bindings.insert(at, record);
            }
        }
        Op::VisualBindingSet(inputs) => {
            let record = visual_record(inputs);
            let at = project
                .visual_bindings
                .partition_point(|binding| binding.id < record.id);
            if project.visual_bindings.get(at).map(|b| &b.id) == Some(&record.id) {
                project.visual_bindings[at] = record;
            } else {
                project.visual_bindings.insert(at, record);
            }
        }
        other => unreachable!("apply_hypnosis called with {}", other.verb()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;

    fn track_input() -> AudioBindingSetInput {
        serde_json::from_value(serde_json::json!({
            "id": "vivienne-hypno",
            "binding": {
            "kind": "hypnosis_track",
            "session": "session.vivienne.patterns",
            "layers": [
                {"kind": "voice", "stem": "narration", "looped": true},
                {"kind": "bed", "stem": "bed-noise"},
                {"kind": "carrier", "stem": "carrier-binaural"},
                {"kind": "subliminal", "stem": "affirmations-encoded", "gain_offset_db": -3.0,
                 "envelope": "env.subliminal-fade", "editions": ["full"]},
                {"kind": "sfx", "stem": "bell", "editions": ["full", "lite"]}
            ]
            }
        }))
        .expect("input parses")
    }

    #[test]
    fn edition_masks_filter_the_stack() {
        let project = Project::default();
        let inputs = track_input();
        assert_eq!(
            decide_audio_binding(&project, &inputs).unwrap(),
            Verdict::Apply
        );
        let record = binding_record(&inputs).unwrap();
        let AudioBindingKind::HypnosisTrack(track) = &record.kind else {
            panic!("track kind");
        };
        // Full: every layer. Lite: only the sfx layer names it;
        // unmasked layers ride every edition.
        let full: Vec<&str> = track
            .layers_for_edition("full")
            .iter()
            .map(|l| l.kind.label())
            .collect();
        assert_eq!(full, vec!["bed", "carrier", "sfx", "subliminal", "voice"]);
        let lite: Vec<&str> = track
            .layers_for_edition("lite")
            .iter()
            .map(|l| l.kind.label())
            .collect();
        assert_eq!(lite, vec!["bed", "carrier", "sfx", "voice"]);
    }

    #[test]
    fn duplicate_layer_kind_refuses() {
        let mut inputs = track_input();
        let AudioBindingKindInput::HypnosisTrack { layers, .. } = &mut inputs.binding else {
            panic!("track input");
        };
        layers.push(layers[0].clone());
        let err = decide_audio_binding(&Project::default(), &inputs).expect_err("refused");
        assert!(err.message.contains("twice"), "{}", err.message);
    }

    #[test]
    fn overlay_phase_is_deterministic() {
        let binding = OverlayHookBinding {
            hook: "spiral".to_string(),
            session: "session.vivienne.patterns".to_string(),
            seed: 7,
            rotation_rate: 0.5,
            opacity_envelope: "env.phase-opacity".to_string(),
        };
        assert_eq!(binding.phase_at(48_000), binding.phase_at(48_000));
        assert_ne!(binding.phase_at(0), binding.phase_at(48_000));
        let phase = binding.phase_at(123_456);
        assert!((0.0..std::f64::consts::TAU).contains(&phase));
    }
}
