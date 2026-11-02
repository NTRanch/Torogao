//! The A3 audio-reactive authoring verbs (BRIEF-AUTHORING,
//! audio-reactive hooks; landed by the G-10 completion):
//! `audio_track_define` and `audio_bind_set`, through the same
//! `decide()` → apply → op-log path as every other family.
//!
//! Authoring stores the reaction system's sync contract as
//! project data — tracks of event markers in ticks, and binding
//! rows from markers to parameter offsets or expression
//! triggers with their envelopes as data. It synthesizes no
//! audio (atelier's lane) and plays none (the engine's).

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::decide::{Verdict, check_id, check_number};
use crate::limits;
use crate::model::{AudioBind, AudioBindTarget, AudioMarker, AudioTrack, Project};
use crate::op::Op;
use crate::refusal::{Refusal, RefusalClass, refuse};

// ---- inputs ------------------------------------------------------

/// One event marker in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioMarkerInput {
    /// Stable id (unique within its track).
    pub id: String,
    /// The marker's tick.
    pub tick: u32,
}

/// Inputs of `audio_track_define`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioTrackDefineInput {
    /// Stable id.
    pub id: String,
    /// Event markers.
    pub markers: Vec<AudioMarkerInput>,
}

/// What an audio marker drives, in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum AudioBindTargetInput {
    /// Offset a parameter's curve by a fixed amount.
    ParameterOffset {
        /// The parameter.
        parameter: String,
        /// The offset added while the envelope is hot.
        offset: f64,
    },
    /// Fire a named expression.
    ExpressionTrigger {
        /// The expression.
        expression: String,
    },
}

/// Inputs of `audio_bind_set` (one binding row).
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioBindSetInput {
    /// The track id.
    pub track: String,
    /// The marker id within the track.
    pub marker: String,
    /// What the marker drives.
    pub target: AudioBindTargetInput,
    /// Sync offset in ticks (may be negative).
    #[serde(default)]
    pub sync_offset_ticks: i64,
    /// Ticks from the (offset) event to the envelope's peak.
    pub peak_tick: u32,
    /// The envelope's trail length in ticks.
    pub trail_ticks: u32,
}

// ---- record builders ----------------------------------------------

fn marker_record(inputs: &AudioMarkerInput) -> AudioMarker {
    AudioMarker {
        id: inputs.id.clone(),
        tick: inputs.tick,
    }
}

fn marker_inputs(record: &AudioMarker) -> AudioMarkerInput {
    AudioMarkerInput {
        id: record.id.clone(),
        tick: record.tick,
    }
}

/// The canonical track record for these inputs (markers sorted
/// by tick, then id).
pub fn track_record(inputs: &AudioTrackDefineInput) -> AudioTrack {
    let mut markers: Vec<AudioMarker> = inputs.markers.iter().map(marker_record).collect();
    markers.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.id.cmp(&b.id)));
    AudioTrack {
        id: inputs.id.clone(),
        markers,
    }
}

/// The inputs that rebuild this track record (validation sweep).
pub fn track_inputs(record: &AudioTrack) -> AudioTrackDefineInput {
    AudioTrackDefineInput {
        id: record.id.clone(),
        markers: record.markers.iter().map(marker_inputs).collect(),
    }
}

fn target_record(inputs: &AudioBindTargetInput) -> AudioBindTarget {
    match inputs {
        AudioBindTargetInput::ParameterOffset { parameter, offset } => {
            AudioBindTarget::ParameterOffset {
                parameter: parameter.clone(),
                offset: *offset,
            }
        }
        AudioBindTargetInput::ExpressionTrigger { expression } => {
            AudioBindTarget::ExpressionTrigger {
                expression: expression.clone(),
            }
        }
    }
}

fn target_inputs(record: &AudioBindTarget) -> AudioBindTargetInput {
    match record {
        AudioBindTarget::ParameterOffset { parameter, offset } => {
            AudioBindTargetInput::ParameterOffset {
                parameter: parameter.clone(),
                offset: *offset,
            }
        }
        AudioBindTarget::ExpressionTrigger { expression } => {
            AudioBindTargetInput::ExpressionTrigger {
                expression: expression.clone(),
            }
        }
    }
}

/// The canonical binding record for these inputs.
pub fn bind_record(inputs: &AudioBindSetInput) -> AudioBind {
    AudioBind {
        track: inputs.track.clone(),
        marker: inputs.marker.clone(),
        target: target_record(&inputs.target),
        sync_offset_ticks: inputs.sync_offset_ticks,
        peak_tick: inputs.peak_tick,
        trail_ticks: inputs.trail_ticks,
    }
}

/// The inputs that rebuild this binding record (sweep).
pub fn bind_inputs(record: &AudioBind) -> AudioBindSetInput {
    AudioBindSetInput {
        track: record.track.clone(),
        marker: record.marker.clone(),
        target: target_inputs(&record.target),
        sync_offset_ticks: record.sync_offset_ticks,
        peak_tick: record.peak_tick,
        trail_ticks: record.trail_ticks,
    }
}

// ---- decide --------------------------------------------------------

/// Decide one audio op against the project.
pub fn decide_audio(project: &Project, op: &Op) -> Result<Verdict, Refusal> {
    match op {
        Op::AudioTrackDefine(inputs) => decide_track_define(project, inputs),
        Op::AudioBindSet(inputs) => decide_bind_set(project, inputs),
        other => unreachable!("decide_audio called with {}", other.verb()),
    }
}

fn decide_track_define(
    project: &Project,
    inputs: &AudioTrackDefineInput,
) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "id")?;
    if inputs.markers.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            "an audio track needs at least one marker: a track exists to carry markers",
        );
    }
    if inputs.markers.len() > limits::AUDIO_MARKERS_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "track declares {} markers, over the bound of {}",
                inputs.markers.len(),
                limits::AUDIO_MARKERS_MAX
            ),
        );
    }
    let mut seen = BTreeSet::new();
    for marker in &inputs.markers {
        check_id(&marker.id, "marker")?;
        if !seen.insert(marker.id.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("track names marker '{}' twice", marker.id),
            );
        }
        if marker.tick > limits::TICKS_MAX {
            return refuse(
                RefusalClass::Range,
                format!(
                    "marker '{}' tick {} exceeds the bound {}",
                    marker.id,
                    marker.tick,
                    limits::TICKS_MAX
                ),
            );
        }
    }
    let record = track_record(inputs);
    match project.audio_track(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => refuse(
            RefusalClass::Conflict,
            format!(
                "audio track '{}' already exists with different markers",
                inputs.id
            ),
        ),
        None => {
            if project.audio_tracks.len() + 1 > limits::AUDIO_TRACKS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "project already holds {} audio tracks, the bound",
                        limits::AUDIO_TRACKS_MAX
                    ),
                );
            }
            Ok(Verdict::Apply)
        }
    }
}

fn decide_bind_set(project: &Project, inputs: &AudioBindSetInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.track, "track")?;
    check_id(&inputs.marker, "marker")?;
    let Some(track) = project.audio_track(&inputs.track) else {
        return refuse(
            RefusalClass::UnknownTarget,
            format!("audio track '{}' does not exist", inputs.track),
        );
    };
    if !track
        .markers
        .iter()
        .any(|marker| marker.id == inputs.marker)
    {
        return refuse(
            RefusalClass::Dangling,
            format!("track '{}' has no marker '{}'", inputs.track, inputs.marker),
        );
    }
    match &inputs.target {
        AudioBindTargetInput::ParameterOffset { parameter, offset } => {
            check_id(parameter, "parameter")?;
            check_number(*offset, "offset")?;
            if project.parameter(parameter).is_none() {
                return refuse(
                    RefusalClass::Dangling,
                    format!("parameter '{parameter}' is not defined"),
                );
            }
        }
        AudioBindTargetInput::ExpressionTrigger { expression } => {
            check_id(expression, "expression")?;
            if project.expression(expression).is_none() {
                return refuse(
                    RefusalClass::Dangling,
                    format!("expression '{expression}' is not defined"),
                );
            }
        }
    }
    if inputs.peak_tick > limits::TICKS_MAX || inputs.trail_ticks > limits::TICKS_MAX {
        return refuse(
            RefusalClass::Range,
            format!("envelope ticks exceed the bound {}", limits::TICKS_MAX),
        );
    }
    let record = bind_record(inputs);
    match project.audio_bind(&inputs.track, &inputs.marker) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => Ok(Verdict::Apply),
        None => {
            if project.audio_binds.len() + 1 > limits::AUDIO_BINDS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "project already holds {} audio bindings, the bound",
                        limits::AUDIO_BINDS_MAX
                    ),
                );
            }
            Ok(Verdict::Apply)
        }
    }
}

// ---- apply -----------------------------------------------------------

/// Apply one decided audio op to the project.
pub fn apply_audio(project: &mut Project, op: &Op) {
    match op {
        Op::AudioTrackDefine(inputs) => {
            let record = track_record(inputs);
            let at = project
                .audio_tracks
                .partition_point(|track| track.id < record.id);
            if project.audio_tracks.get(at).map(|t| &t.id) == Some(&record.id) {
                project.audio_tracks[at] = record;
            } else {
                project.audio_tracks.insert(at, record);
            }
        }
        Op::AudioBindSet(inputs) => {
            let record = bind_record(inputs);
            let key = |bind: &AudioBind| (bind.track.clone(), bind.marker.clone());
            let at = project
                .audio_binds
                .partition_point(|bind| key(bind) < key(&record));
            if project.audio_binds.get(at).map(key) == Some(key(&record)) {
                project.audio_binds[at] = record;
            } else {
                project.audio_binds.insert(at, record);
            }
        }
        other => unreachable!("apply_audio called with {}", other.verb()),
    }
}
