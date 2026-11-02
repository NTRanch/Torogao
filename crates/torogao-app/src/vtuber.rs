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
//! - Tracking loss degrades to hold: when no frame
//!   applies, the scene is not touched at all.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
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
        Ok(take)
    }
}

/// Records applied performance frames into a mimicry
/// take.
#[derive(Debug, Default)]
pub struct MimicryRecorder {
    frames: Vec<BTreeMap<String, [f32; 2]>>,
}

impl MimicryRecorder {
    /// A fresh recorder.
    pub fn new() -> MimicryRecorder {
        MimicryRecorder::default()
    }

    /// Records one applied frame (the governed parameters'
    /// post-application values on the puppet).
    pub fn record_frame(&mut self, frame: BTreeMap<String, [f32; 2]>) {
        self.frames.push(frame);
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
        self.recorder.record_frame(recorded);
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
