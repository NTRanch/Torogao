//! Live VTuber mode (Stage 3.3): tracked frames drive
//! the puppet, performances are recorded as mimicry
//! takes, and viseme curves play over the puppet's mouth.
//!
//! The pieces, and who owns what:
//! - The transport and the ARKit-to-governed mapping live
//!   in `torogao-vmc`; the governed names are the model's
//!   own parameter names, the same names the editor, the
//!   companion, and the session host use.
//! - This module applies the tracking driver's governed
//!   state to the scene (one scene step per applied frame,
//!   ungoverned components holding their standing values),
//!   records the applied performance as a mimicry take —
//!   a replayable, hashable artifact in the take family —
//!   and plays viseme curve sets (the Stage 3.4 front
//!   door: audio analysis bakes curves, this driver applies
//!   them) with frame-exact timing.
//! - A take also preserves the raw 52-channel ARKit frame
//!   beside each governed frame (D-553): the governed
//!   view discards the 37 channels with no governed
//!   counterpart, the take does not — the raw frames are
//!   measurements only, privileged corpus data, never
//!   replayed and never a training set.
//! - Tracking loss degrades to hold: when no frame
//!   applies, the scene is not touched at all.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use torogao_cli::curves::CurveSet;
use torogao_engine::Vec2;
use torogao_vmc::tracker::{AppliedFrame, TrackingDriver};
use torogao_vmc::vmc::VmcFrame;

use crate::error::{AppError, AppResult};
use crate::scene::Scene;
use crate::state_hash::evaluated_state_hash;

/// The mimicry take format tag.
pub const MIMICRY_FORMAT: &str = "torogao-mimicry-take/1";
/// The largest mimicry take, in frames (600 s at 60 fps —
/// the take family's shared ceiling).
pub const MIMICRY_FRAMES_MAX: usize = 36_000;

/// A document refusal (bad take or curve document),
/// in the shell's `Format` voice.
fn invalid(message: impl Into<String>) -> AppError {
    AppError::Format(message.into())
}

/// An unknown-parameter refusal, in the same voice
/// `Scene::parameter_index` refuses with.
fn unknown_param(name: &str, code: &str) -> AppError {
    AppError::Param(format!("{code} refused: unknown parameter '{name}'"))
}

/// A recorded mimicry performance: per frame, the
/// governed parameters' full values as applied to the
/// puppet. Serializes as `torogao-mimicry-take/1` JSON —
/// it is a portable session artifact (Stage 3.3's export
/// leg for live performance).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MimicryTake {
    /// The format tag (`torogao-mimicry-take/1`).
    pub format: String,
    /// One map per frame: parameter name → (x, y) value.
    pub frames: Vec<BTreeMap<String, [f32; 2]>>,
    /// One raw ARKit frame per governed frame (D-553):
    /// all 52 blendshape channels as received, in
    /// `torogao_vmc::arkit::ARKIT_BLENDSHAPES` order —
    /// including the 37 channels with no governed
    /// counterpart, which the governed frames above drop.
    /// Measurements only: privileged corpus data,
    /// preserved for study, never replayed and never a
    /// training set. Takes recorded before D-553 carry
    /// none: the field defaults to empty on load, and a
    /// non-empty `raw_frames` must match `frames`
    /// one-for-one (`from_json` refuses otherwise).
    #[serde(
        default,
        serialize_with = "serialize_raw_frames",
        deserialize_with = "deserialize_raw_frames"
    )]
    pub raw_frames: Vec<[f32; 52]>,
}

/// Serializes the raw frames as one 52-number array per
/// frame (serde's built-in array support stops at 32
/// elements, so the 52-wide frames go through slices).
fn serialize_raw_frames<S>(frames: &[[f32; 52]], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(frames.len()))?;
    for frame in frames {
        seq.serialize_element(frame.as_slice())?;
    }
    seq.end()
}

/// Deserializes the raw frames, refusing any frame that
/// does not carry exactly 52 channels (a parse refusal
/// at the `from_json` seam).
fn deserialize_raw_frames<'de, D>(deserializer: D) -> Result<Vec<[f32; 52]>, D::Error>
where
    D: Deserializer<'de>,
{
    let frames = Vec::<Vec<f32>>::deserialize(deserializer)?;
    frames
        .into_iter()
        .map(|frame| {
            let count = frame.len();
            frame.try_into().map_err(|_| {
                serde::de::Error::custom(format!("a raw frame carries {count} channels, not 52"))
            })
        })
        .collect()
}

impl MimicryTake {
    /// The document as pretty JSON (its exchange form).
    pub fn to_json(&self) -> AppResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|err| invalid(format!("TG-MIMICRY-SERIALIZE refused: {err}")))
    }

    /// Parses and validates a mimicry take: format tag
    /// and the frame bound refuse typed.
    pub fn from_json(json: &str) -> AppResult<MimicryTake> {
        let take: MimicryTake = serde_json::from_str(json)
            .map_err(|err| invalid(format!("TG-MIMICRY-PARSE refused: {err}")))?;
        if take.format != MIMICRY_FORMAT {
            return Err(invalid(format!(
                "TG-MIMICRY-FORMAT refused: format {:?} is not {MIMICRY_FORMAT}",
                take.format
            )));
        }
        if take.frames.len() > MIMICRY_FRAMES_MAX {
            return Err(invalid(format!(
                "TG-MIMICRY-BOUND refused: {} frames exceeds the {MIMICRY_FRAMES_MAX}-frame bound",
                take.frames.len()
            )));
        }
        if !take.raw_frames.is_empty() && take.raw_frames.len() != take.frames.len() {
            return Err(invalid(format!(
                "TG-MIMICRY-RAW-BOUND refused: {} raw frames do not match {} governed frames one-for-one",
                take.raw_frames.len(),
                take.frames.len()
            )));
        }
        Ok(take)
    }
}

/// Records applied performance frames into a mimicry
/// take.
#[derive(Debug, Default)]
pub struct MimicryRecorder {
    frames: Vec<BTreeMap<String, [f32; 2]>>,
    raw_frames: Vec<[f32; 52]>,
}

impl MimicryRecorder {
    /// A fresh recorder.
    pub fn new() -> MimicryRecorder {
        MimicryRecorder::default()
    }

    /// Records one applied frame: the governed parameters'
    /// post-application values on the puppet, alongside
    /// the raw 52-channel ARKit frame they were mapped
    /// from (D-553, in `ARKIT_BLENDSHAPES` order).
    pub fn record_frame(&mut self, frame: BTreeMap<String, [f32; 2]>, raw_frame: [f32; 52]) {
        self.frames.push(frame);
        self.raw_frames.push(raw_frame);
    }

    /// Frames recorded so far.
    pub fn frames(&self) -> usize {
        self.frames.len()
    }

    /// Finishes the recording as a take document.
    pub fn into_take(self) -> MimicryTake {
        MimicryTake {
            format: MIMICRY_FORMAT.to_string(),
            frames: self.frames,
            raw_frames: self.raw_frames,
        }
    }
}

/// The live tracking session against one scene: the
/// driver, the recorder, and the application rule.
#[derive(Debug, Default)]
pub struct LiveTracking {
    driver: TrackingDriver,
    recorder: MimicryRecorder,
}

impl LiveTracking {
    /// A fresh live session.
    pub fn new() -> LiveTracking {
        LiveTracking::default()
    }

    /// Queues one received frame under its stamp.
    pub fn ingest(&mut self, stamp: u64, frame: VmcFrame) {
        self.driver.ingest(stamp, frame);
    }

    /// The driver (queue, drop, and state accounting).
    pub fn driver(&self) -> &TrackingDriver {
        &self.driver
    }

    /// The performance recorded so far, as a take.
    pub fn take(&self) -> MimicryTake {
        MimicryTake {
            format: MIMICRY_FORMAT.to_string(),
            frames: self.recorder.frames.clone(),
            raw_frames: self.recorder.raw_frames.clone(),
        }
    }

    /// One 60 fps tick: applies the oldest queued frame's
    /// governed state to the scene and records it. Returns
    /// the applied frame's accounting, or `None` when the
    /// feed is quiet — in which case the scene is left
    /// exactly as it stands (the hold rule).
    ///
    /// A governed parameter the scene does not carry is
    /// skipped: the mapping is defined over the cast rigs
    /// (the census gate proves presence there), and a
    /// foreign rig degrades to partial tracking rather
    /// than failing the live loop.
    pub fn tick(&mut self, scene: &mut Scene, now: u64) -> AppResult<Option<AppliedFrame>> {
        let Some(applied) = self.driver.tick(now) else {
            return Ok(None);
        };
        let mut assignments: Vec<(usize, Vec2)> = Vec::new();
        let mut governed_names: Vec<String> = Vec::new();
        for (name, value) in self.driver.state() {
            let Ok(index) = scene.parameter_index(name) else {
                continue;
            };
            let current = scene.puppet.params[index].value;
            assignments.push((
                index,
                Vec2::new(value.x.unwrap_or(current.x), value.y.unwrap_or(current.y)),
            ));
            governed_names.push(name.clone());
        }
        scene.assign_and_step(&assignments)?;
        let mut recorded: BTreeMap<String, [f32; 2]> = BTreeMap::new();
        for name in governed_names {
            if let Ok(index) = scene.parameter_index(&name) {
                let value = scene.puppet.params[index].value;
                recorded.insert(name, [value.x, value.y]);
            }
        }
        self.recorder.record_frame(recorded, applied.raw_frame);
        Ok(Some(applied))
    }
}

/// Replays a mimicry take into a scene: each recorded
/// frame's values are assigned and the scene steps once
/// per frame. Returns the SHA-256 of the evaluated-state
/// hash sequence — the same fold a recorded take's replay
/// proves itself against, so two replays of one
/// performance are bit-comparable. A parameter the scene
/// does not carry refuses: a take belongs to its rig.
pub fn replay_mimicry(take: &MimicryTake, scene: &mut Scene) -> AppResult<String> {
    if take.format != MIMICRY_FORMAT {
        return Err(invalid(format!(
            "TG-MIMICRY-FORMAT refused: format {:?} is not {MIMICRY_FORMAT}",
            take.format
        )));
    }
    if take.frames.len() > MIMICRY_FRAMES_MAX {
        return Err(invalid(format!(
            "TG-MIMICRY-BOUND refused: {} frames exceeds the {MIMICRY_FRAMES_MAX}-frame bound",
            take.frames.len()
        )));
    }
    let mut sequence = Sha256::new();
    for frame in &take.frames {
        let mut assignments: Vec<(usize, Vec2)> = Vec::with_capacity(frame.len());
        for (name, value) in frame {
            let index = scene
                .parameter_index(name)
                .map_err(|_| unknown_param(name, "TG-MIMICRY-PARAM"))?;
            assignments.push((index, Vec2::new(value[0], value[1])));
        }
        scene.assign_and_step(&assignments)?;
        sequence.update(evaluated_state_hash(&mut scene.puppet).to_le_bytes());
    }
    Ok(format!("{:x}", sequence.finalize()))
}

/// Applies a baked viseme curve set to the scene, one
/// curve step per tick — the playback half of the
/// lipsync pipeline (`analyze` → track → `bake_visemes` →
/// this driver).
pub struct VisemeDriver {
    curves: CurveSet,
    series: Vec<(usize, usize, usize)>,
    step: usize,
}

impl VisemeDriver {
    /// Resolves a curve set against a scene: every series'
    /// parameter must exist on the rig and address the x
    /// or y axis, refused typed otherwise.
    pub fn new(scene: &Scene, curves: CurveSet) -> AppResult<VisemeDriver> {
        let mut series = Vec::with_capacity(curves.series.len());
        for (offset, entry) in curves.series.iter().enumerate() {
            let Some(index) = scene.puppet.param_index(&entry.parameter) else {
                return Err(unknown_param(&entry.parameter, "TG-VISEME-DRIVER-PARAM"));
            };
            if !(0..=1).contains(&entry.axis) {
                return Err(invalid(format!(
                    "TG-VISEME-DRIVER-AXIS refused: axis {} for {:?} is not 0 (x) or 1 (y)",
                    entry.axis, entry.parameter
                )));
            }
            series.push((index, entry.axis as usize, offset));
        }
        Ok(VisemeDriver {
            curves,
            series,
            step: 0,
        })
    }

    /// Curve steps not yet applied.
    pub fn remaining(&self) -> usize {
        self.curves.steps().saturating_sub(self.step)
    }

    /// Applies the next curve step: each series sets its
    /// parameter's addressed component (curve values are
    /// f64 authoring values; the rig is f32, and the cast
    /// is the documented seam), other components holding
    /// their standing values, and the scene steps once.
    /// Returns false when the curve set is exhausted (the
    /// scene is then left untouched).
    pub fn tick(&mut self, scene: &mut Scene) -> AppResult<bool> {
        if self.step >= self.curves.steps() {
            return Ok(false);
        }
        let step = self.step;
        let mut targets: BTreeMap<usize, Vec2> = BTreeMap::new();
        for (index, axis, offset) in &self.series {
            let entry = &self.curves.series[*offset];
            let Some(value) = entry.values.get(step) else {
                return Err(invalid(format!(
                    "TG-VISEME-DRIVER-BOUND refused: series {:?} has no step {step}",
                    entry.parameter
                )));
            };
            let target = targets
                .entry(*index)
                .or_insert_with(|| scene.puppet.params[*index].value);
            if *axis == 0 {
                target.x = *value as f32;
            } else {
                target.y = *value as f32;
            }
        }
        let assignments: Vec<(usize, Vec2)> = targets.into_iter().collect();
        scene.assign_and_step(&assignments)?;
        self.step += 1;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{MIMICRY_FORMAT, MimicryRecorder, MimicryTake};
    use std::collections::BTreeMap;

    fn governed_frame() -> BTreeMap<String, [f32; 2]> {
        BTreeMap::from([("Eye:: Left:: Blink".to_string(), [0.25, 0.0])])
    }

    /// A raw frame whose unmapped channels carry the
    /// distinctive values: cheekSquintLeft (index 6 in
    /// `ARKIT_BLENDSHAPES` order) at 0.73, eyeSquintLeft
    /// (index 18) at 0.41, the mapped eyeBlinkLeft
    /// (index 8) at 0.25.
    fn raw_frame() -> [f32; 52] {
        let mut raw = [0.0_f32; 52];
        raw[6] = 0.73;
        raw[8] = 0.25;
        raw[18] = 0.41;
        raw[51] = -0.0;
        raw
    }

    #[test]
    fn recorder_preserves_raw_frames_alongside_governed() {
        let mut recorder = MimicryRecorder::new();
        recorder.record_frame(governed_frame(), raw_frame());
        let take = recorder.into_take();
        assert_eq!(take.raw_frames.len(), 1);
        for (actual, expected) in take.raw_frames[0].iter().zip(raw_frame()) {
            assert_eq!(actual.to_bits(), expected.to_bits(), "raw values are exact");
        }
        assert!(
            !take.frames[0].contains_key("cheekSquintLeft"),
            "the governed frame does not contain the unmapped channel"
        );
        assert_eq!(take.frames[0]["Eye:: Left:: Blink"], [0.25, 0.0]);
    }

    #[test]
    fn raw_frames_survive_the_json_round_trip_bit_exactly() {
        let take = MimicryTake {
            format: MIMICRY_FORMAT.to_string(),
            frames: vec![governed_frame(), governed_frame()],
            raw_frames: vec![raw_frame(), raw_frame()],
        };
        let parsed =
            MimicryTake::from_json(&take.to_json().expect("take serializes")).expect("take parses");
        assert_eq!(parsed.frames.len(), take.frames.len());
        for (actual_frames, expected_frames) in parsed.raw_frames.iter().zip(&take.raw_frames) {
            for (actual, expected) in actual_frames.iter().zip(expected_frames) {
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
        }
    }

    #[test]
    fn from_json_refuses_raw_frames_that_do_not_match_governed() {
        let take = MimicryTake {
            format: MIMICRY_FORMAT.to_string(),
            frames: vec![governed_frame()],
            raw_frames: vec![raw_frame(), raw_frame()],
        };
        let error = MimicryTake::from_json(&take.to_json().expect("take serializes"))
            .expect_err("a length-mismatched document refuses");
        assert!(
            error.to_string().contains("TG-MIMICRY-RAW-BOUND"),
            "the refusal is typed: {error}"
        );
    }

    #[test]
    fn takes_recorded_before_raw_frames_still_load() {
        let parsed =
            MimicryTake::from_json("{\"format\": \"torogao-mimicry-take/1\", \"frames\": []}")
                .expect("a pre-D-553 take loads");
        assert!(parsed.raw_frames.is_empty(), "raw frames default to empty");
    }
}
