//! The bake: Rhubarb cues in, a 60 Hz morph-weight track
//! out. The bake is the shipping artifact's whole story —
//! the oracle (Rhubarb) runs at authoring time, and what
//! ships is this deterministic document.
//!
//! ## The bake law (normative for `torogao-viseme/1`)
//!
//! - Time is sampled at the estate's canonical 60 Hz
//!   (motion-clip spec §2.1). A time `t` in seconds lands
//!   on frame `round(t * 60)`, rounding half away from
//!   zero — `f64::round`, an exact IEEE operation, so the
//!   law is identical on every machine.
//! - A cue `[start, end)` covers frames
//!   `[frame_of(start), frame_of(end))`, clamped to the
//!   track's frame count. A cue whose clamped span is
//!   empty is dropped and **counted** in the bake report
//!   — never silently.
//! - Frames no cue covers take all-zero weights (rest).
//!   Rhubarb cue sheets are contiguous, so this arises
//!   only for a tail past the last cue when an explicit
//!   longer duration is given.
//! - The track's frame count is `frame_of(duration)`,
//!   where the duration is the caller's (the audio's true
//!   duration) or, absent one, the last cue's end.
//! - Each channel's weights are emitted as `stepped`
//!   scalar keys (motion-clip spec §4.3 shape): a key at
//!   frame 0 and a key wherever the weight changes. A
//!   channel that is zero on every frame gets no track —
//!   the spec holds untracked channels at their default.
//! - No frame map is emitted: the cue sheet is
//!   continuous-time data, not a frame sequence, so no
//!   frame retime happened (spec §6.2). The retime that
//!   did happen — continuous time sampled at 60 Hz — is
//!   this converter's named sampling law, stated here.

use crate::cue::{Cue, TIMELINE_SECONDS_MAX, validate_cues};
use crate::error::{VisemeError, VisemeResult};
use crate::map::{MOUTH_CHANNELS, VisemeMap};
use serde::Serialize;

/// The baked track document's format token.
pub const TRACK_FORMAT: &str = "torogao-viseme-track/1";

/// Most frames a bake may produce: the timeline limit at
/// 60 Hz (`TIMELINE_SECONDS_MAX` × 60).
pub const FRAMES_MAX: u32 = 2_160_000;

/// What a bake did, in numbers. Every count here is
/// reported by the CLI; a dropped cue is visible in
/// `cues_dropped_empty`, never absorbed silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BakeReport {
    /// Cues consumed from the input.
    pub cues_in: usize,
    /// Cues whose rounded frame span was empty (shorter
    /// than the sampling law can represent) and dropped.
    pub cues_dropped_empty: usize,
    /// Frames in the baked track.
    pub frames: u32,
    /// Channel tracks emitted (channels with at least one
    /// non-zero frame).
    pub tracks_emitted: usize,
}

/// A baked track: the canonical JSON document plus the
/// report of how it was made.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedTrack {
    /// Frames in the track (also stated in the document).
    pub frames: u32,
    /// The bake's counts.
    pub report: BakeReport,
    json: String,
}

impl BakedTrack {
    /// The canonical JSON document. Byte-identical for
    /// identical inputs: field order is fixed by the
    /// serialization structs below, tracks are emitted in
    /// `MOUTH_CHANNELS` (alphabetical) order, and float
    /// rendering is serde_json's deterministic shortest
    /// form.
    pub fn to_json(&self) -> &str {
        &self.json
    }
}

#[derive(Serialize)]
struct TrackDocument {
    format: &'static str,
    frames: u32,
    group: GroupDocument,
    provenance: ProvenanceDocument,
}

#[derive(Serialize)]
struct GroupDocument {
    kind: &'static str,
    tracks: Vec<ChannelTrack>,
}

#[derive(Serialize)]
struct ChannelTrack {
    channel: &'static str,
    keys: Vec<ScalarKey>,
}

#[derive(Serialize)]
struct ScalarKey {
    frame: u32,
    value: f64,
    interpolation: &'static str,
}

#[derive(Serialize)]
struct ProvenanceDocument {
    group: &'static str,
    source_class: &'static str,
    converter: &'static str,
    converter_version: &'static str,
    source_sha256: String,
}

fn frame_of(seconds: f64, frames: u32) -> u32 {
    debug_assert!(seconds.is_finite() && seconds >= 0.0);
    let rounded = (seconds * 60.0).round();
    if rounded <= 0.0 {
        0
    } else if rounded >= f64::from(frames) {
        frames
    } else {
        // `rounded` is integral and strictly inside
        // 1..frames (frames <= FRAMES_MAX), so the
        // conversion is exact.
        rounded as u32
    }
}

fn check_duration(duration_seconds: f64) -> VisemeResult<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err(VisemeError::new(
            "TV-DURATION-INVALID",
            format!("duration {duration_seconds}s is not a positive finite time"),
        ));
    }
    if duration_seconds > TIMELINE_SECONDS_MAX {
        return Err(VisemeError::new(
            "TV-DURATION-INVALID",
            format!(
                "duration {duration_seconds}s exceeds the timeline limit {TIMELINE_SECONDS_MAX}s"
            ),
        ));
    }
    Ok(())
}

/// Bakes a validated cue list into a 60 Hz track
/// document. `source_sha256` is the hex SHA-256 of the
/// cue file the cues came from; it is carried into the
/// track's provenance (motion-clip spec §6) so the bake
/// is auditable back to its exact input. `duration_seconds`
/// is the source audio's true duration when the caller
/// knows it; absent one, the last cue's end is the
/// duration.
pub fn bake(
    cues: &[Cue],
    map: &VisemeMap,
    source_sha256: &str,
    duration_seconds: Option<f64>,
) -> VisemeResult<BakedTrack> {
    validate_cues(cues)?;
    let duration = duration_seconds.unwrap_or_else(|| cues[cues.len() - 1].end_seconds);
    check_duration(duration)?;
    let frames = frame_of(duration, FRAMES_MAX);
    if frames == 0 {
        return Err(VisemeError::new(
            "TV-DURATION-INVALID",
            format!("duration {duration}s rounds to zero frames at 60 Hz"),
        ));
    }

    // Resolve each cue to its frame span once; the walk
    // below then reads spans, never raw times.
    let mut frame_shapes: Vec<Option<crate::cue::Shape>> = vec![None; frames as usize];
    let mut cues_dropped_empty = 0usize;
    for cue in cues {
        let first = frame_of(cue.start_seconds, frames);
        let last = frame_of(cue.end_seconds, frames);
        if last <= first {
            cues_dropped_empty += 1;
            continue;
        }
        for slot in &mut frame_shapes[first as usize..last as usize] {
            *slot = Some(cue.shape);
        }
    }

    let mut tracks: Vec<ChannelTrack> = Vec::new();
    for (channel_index, channel) in MOUTH_CHANNELS.iter().enumerate() {
        let mut keys: Vec<ScalarKey> = Vec::new();
        let mut previous: Option<f64> = None;
        for (frame, shape) in frame_shapes.iter().enumerate() {
            let value = shape.map_or(0.0, |shape| map.weights(shape)[channel_index]);
            if previous != Some(value) {
                keys.push(ScalarKey {
                    frame: frame as u32,
                    value,
                    interpolation: "stepped",
                });
                previous = Some(value);
            }
        }
        // A channel silent for the whole track holds its
        // default by the spec's rule; emit no track.
        if keys.iter().any(|key| key.value != 0.0) {
            tracks.push(ChannelTrack { channel, keys });
        }
    }

    let report = BakeReport {
        cues_in: cues.len(),
        cues_dropped_empty,
        frames,
        tracks_emitted: tracks.len(),
    };
    let document = TrackDocument {
        format: TRACK_FORMAT,
        frames,
        group: GroupDocument {
            kind: "face",
            tracks,
        },
        provenance: ProvenanceDocument {
            group: "face",
            source_class: "converted",
            converter: crate::CONVERTER_ID,
            converter_version: env!("CARGO_PKG_VERSION"),
            source_sha256: source_sha256.to_string(),
        },
    };
    let mut json = serde_json::to_string_pretty(&document).map_err(|err| {
        VisemeError::new(
            "TV-BAKE-FAILED",
            format!("track serialization failed: {err}"),
        )
    })?;
    json.push('\n');
    Ok(BakedTrack {
        frames,
        report,
        json,
    })
}

#[cfg(test)]
mod tests {
    use super::{FRAMES_MAX, bake, frame_of};
    use crate::cue::{Cue, Shape};
    use crate::map::VisemeMap;

    fn cue(start: f64, end: f64, shape: Shape) -> Cue {
        Cue {
            start_seconds: start,
            end_seconds: end,
            shape,
        }
    }

    fn map() -> VisemeMap {
        VisemeMap::load().expect("shipped map")
    }

    #[test]
    fn frame_of_rounds_half_away_from_zero() {
        assert_eq!(frame_of(0.0, 100), 0);
        assert_eq!(frame_of(0.025, 100), 2); // 1.5 frames -> 2
        assert_eq!(frame_of(5.57, FRAMES_MAX), 334);
        assert_eq!(frame_of(5.58, FRAMES_MAX), 335);
        assert_eq!(frame_of(99.0, 100), 100); // clamped
    }

    #[test]
    fn bake_is_byte_identical_across_runs() {
        let cues = [
            cue(0.0, 0.5, Shape::X),
            cue(0.5, 1.0, Shape::D),
            cue(1.0, 1.5, Shape::F),
        ];
        let first = bake(&cues, &map(), "ab12", None).expect("bake");
        let second = bake(&cues, &map(), "ab12", None).expect("bake");
        assert_eq!(first.to_json(), second.to_json());
        assert_eq!(first.frames, 90);
        assert_eq!(first.report.cues_dropped_empty, 0);
    }

    #[test]
    fn explicit_duration_extends_the_tail_as_rest() {
        let cues = [cue(0.0, 0.5, Shape::D)];
        let track = bake(&cues, &map(), "ab12", Some(1.0)).expect("bake");
        assert_eq!(track.frames, 60);
        let parsed: serde_json::Value =
            serde_json::from_str(track.to_json()).expect("track parses");
        let aa = &parsed["group"]["tracks"][0];
        assert_eq!(aa["channel"], "Fac_Mth_Aa");
        // Aa is 1.0 over [0, 30) and returns to 0 at 30.
        assert_eq!(aa["keys"][1]["frame"], 30);
        assert_eq!(aa["keys"][1]["value"], 0.0);
    }

    #[test]
    fn sub_frame_cue_is_dropped_and_counted() {
        // A 1 ms cue inside one frame rounds to an empty
        // span at 60 Hz; it is dropped, and the report
        // says so.
        let cues = [
            cue(0.0, 0.5, Shape::X),
            cue(0.501, 0.502, Shape::D),
            cue(0.502, 1.0, Shape::X),
        ];
        let track = bake(&cues, &map(), "ab12", None).expect("bake");
        assert_eq!(track.report.cues_dropped_empty, 1);
        assert_eq!(track.report.tracks_emitted, 0);
    }

    #[test]
    fn zero_duration_is_a_loud_error() {
        let cues = [cue(0.0, 0.5, Shape::X)];
        let err = bake(&cues, &map(), "ab12", Some(0.0)).expect_err("zero duration");
        assert_eq!(err.code(), "TV-DURATION-INVALID");
    }
}
