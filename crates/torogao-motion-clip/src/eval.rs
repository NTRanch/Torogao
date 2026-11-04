//! Evaluation: the layer replay evaluator (SPEC.md
//! section 7), exact as written.
//!
//! An evaluated frame is a pure function of the clip
//! document, the SkeletonDef, the rig's channel
//! declarations, and the frame index (section 8).
//! Provenance is never read here (section 6.1). All
//! arithmetic is f64; the only transcendental operation is
//! `sqrt`. Composition implements section 7.2's formulas
//! literally — in particular section 7.3's weight-0
//! identity: no shortcut skips a zero-weight layer, no
//! composed rotation is renormalized, so a clip with all
//! weights at 0 evaluates bit-identically to the same clip
//! with its layers removed.

use std::fmt;

use sha2::Digest;

use crate::bind::BoundClip;
use crate::model::{
    DeltaQuatKey, DeltaScalarKey, DeltaVecKey, FrameMap, Interpolation, LayerTrack, QuatKey,
    ScalarKey, VecKey,
};

/// A bone's evaluated local transform at one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BonePose {
    /// Local translation, in metres.
    pub translation: [f64; 3],
    /// Local rotation, a unit quaternion `[x, y, z, w]`.
    pub rotation: [f64; 4],
    /// Local scale.
    pub scale: [f64; 3],
}

/// One evaluated frame: a local TRS for every bone of the
/// SkeletonDef (in def order), and a value for every scalar
/// channel that appears in any track or layer scope in the
/// clip (sorted by channel name). Channel values carry the
/// one sanctioned clamp (SPEC.md section 10.3): the
/// composed value clamped to the channel's declared range
/// as it is written out.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedFrame {
    /// `(bone name, pose)` pairs in SkeletonDef order.
    pub bones: Vec<(String, BonePose)>,
    /// `(channel name, value)` pairs sorted by name.
    pub channels: Vec<(String, f64)>,
}

/// Evaluation was asked for a frame outside `0..frames`.
/// Evaluation is defined only for in-range indices (SPEC.md
/// section 7.4); wrapping is the host's policy, applied by
/// the host before calling.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    /// The requested frame is not in `0..frames`.
    FrameOutOfRange {
        /// The frame requested.
        frame: u32,
        /// The clip's frame count.
        frames: u32,
    },
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::FrameOutOfRange { frame, frames } => {
                write!(f, "frame {frame} is outside 0..{frames}")
            }
        }
    }
}

impl std::error::Error for EvalError {}

impl BoundClip<'_> {
    /// Evaluate one frame (SPEC.md section 7).
    pub fn evaluate_frame(&self, frame: u32) -> Result<EvaluatedFrame, EvalError> {
        if frame >= self.clip.frames {
            return Err(EvalError::FrameOutOfRange {
                frame,
                frames: self.clip.frames,
            });
        }
        let mut bones = Vec::with_capacity(self.skeleton.bones().len());
        for bone in self.skeleton.bones() {
            bones.push((bone.name.clone(), self.evaluate_bone(bone, frame)));
        }
        let mut channels = Vec::with_capacity(self.channel_names.len());
        for name in &self.channel_names {
            channels.push(((*name).to_string(), self.evaluate_channel(name, frame)));
        }
        Ok(EvaluatedFrame { bones, channels })
    }

    /// Evaluate every frame, `0..frames`, in order.
    pub fn evaluate_all(&self) -> Vec<EvaluatedFrame> {
        (0..self.clip.frames)
            .map(|frame| {
                self.evaluate_frame(frame)
                    .expect("frames generated from 0..frames are in range")
            })
            .collect()
    }

    /// The replay hash: SHA-256 over the evaluated-frame
    /// sequence, encoded canonically — the frame count as
    /// u32 LE, then per frame, per bone in def order, the
    /// f64 bit patterns (LE) of translation, rotation,
    /// scale; then per channel in sorted order, the value's
    /// bit pattern. Bit patterns, not decimal text, so the
    /// hash is a statement about the exact evaluated values
    /// (the D-538 cross-machine pattern, SPEC.md section 13
    /// PT-1/PT-7).
    pub fn replay_hash(&self) -> String {
        let mut hasher = sha2::Sha256::new();
        hasher.update(self.clip.frames.to_le_bytes());
        for frame in self.evaluate_all() {
            for (_, pose) in &frame.bones {
                for value in pose
                    .translation
                    .iter()
                    .chain(pose.rotation.iter())
                    .chain(pose.scale.iter())
                {
                    hasher.update(value.to_bits().to_le_bytes());
                }
            }
            for (_, value) in &frame.channels {
                hasher.update(value.to_bits().to_le_bytes());
            }
        }
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(64);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }

    fn evaluate_bone(&self, bone: &crate::bind::BoneDef, frame: u32) -> BonePose {
        let name = bone.name.as_str();
        let mut pose = BonePose {
            translation: bone.rest_translation,
            rotation: bone.rest_rotation,
            scale: bone.rest_scale,
        };
        if let Some(track) = self.body_tracks.get(name) {
            if let Some(keys) = &track.translation {
                pose.translation = eval_vec_keys(keys, frame);
            }
            if let Some(keys) = &track.rotation {
                pose.rotation = eval_quat_keys(keys, frame);
            }
            if let Some(keys) = &track.scale {
                pose.scale = eval_vec_keys(keys, frame);
            }
        }
        for layer in &self.clip.layers {
            let Some(LayerTrack::Bone(track)) = layer
                .tracks
                .iter()
                .find(|track| matches!(track, LayerTrack::Bone(track) if track.bone == name))
            else {
                continue;
            };
            let weight = layer.weight;
            if let Some(keys) = &track.translation {
                let delta = eval_delta_vec_keys(keys, frame);
                for (component, delta_component) in pose.translation.iter_mut().zip(delta.iter()) {
                    *component += weight * delta_component;
                }
            }
            if let Some(keys) = &track.scale {
                let delta = eval_delta_vec_keys(keys, frame);
                for (component, delta_component) in pose.scale.iter_mut().zip(delta.iter()) {
                    *component *= 1.0 + weight * (delta_component - 1.0);
                }
            }
            if let Some(keys) = &track.rotation {
                let delta = eval_delta_quat_keys(keys, frame);
                pose.rotation = quat_mul(pose.rotation, weighted_delta(weight, delta));
            }
        }
        pose
    }

    fn evaluate_channel(&self, name: &str, frame: u32) -> f64 {
        // Base value: the channel's track, or 0 for a
        // channel that appears only in layer scopes
        // (SPEC.md section 7.1).
        let mut value = match self.face_tracks.get(name) {
            Some(track) => eval_scalar_keys(&track.keys, frame),
            None => 0.0,
        };
        for layer in &self.clip.layers {
            let Some(LayerTrack::Channel(track)) = layer
                .tracks
                .iter()
                .find(|track| matches!(track, LayerTrack::Channel(track) if track.channel == name))
            else {
                continue;
            };
            let delta = eval_delta_scalar_keys(&track.keys, frame);
            value += layer.weight * delta;
        }
        // The one sanctioned clamp (section 10.3): the
        // composed value clamps to the declared range as it
        // is written into the governed parameter.
        let declaration = self
            .channels
            .channel(name)
            .expect("binding validation resolved every evaluated channel");
        value.clamp(declaration.min, declaration.max)
    }
}

/// The source frame a frame map assigns to a take frame,
/// if the take frame is in range. The map is audit data:
/// evaluation never reads it (SPEC.md section 6.1); these
/// helpers are how a consumer audits a retime (PT-4).
pub fn frame_map_source_at(map: &FrameMap, take_frame: u32) -> Option<u32> {
    map.entries.get(take_frame as usize).copied()
}

/// Every take frame a frame map derived from one source
/// frame, in increasing order.
pub fn frame_map_take_frames_for_source(map: &FrameMap, source_frame: u32) -> Vec<u32> {
    map.entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| **entry == source_frame)
        .map(|(index, _)| index as u32)
        .collect()
}

/// Where frame `f` falls in a key list: an endpoint hold,
/// or a segment with its parameter `u` (SPEC.md §7.1).
enum Segment {
    /// Hold the key at this index.
    Hold(usize),
    /// Interpolate segment `index` → `index + 1` at `u`.
    Between { index: usize, u: f64 },
}

/// Locate frame `f` among sorted key frames. Key lists are
/// non-empty with strictly increasing frames (validated).
fn locate(frames: &[u32], f: u32) -> Segment {
    assert!(!frames.is_empty(), "key lists are non-empty by validation");
    if f <= frames[0] {
        return Segment::Hold(0);
    }
    let last = frames.len() - 1;
    if f >= frames[last] {
        return Segment::Hold(last);
    }
    // The count of key frames ≤ f, minus one, is the
    // segment index: frames[index] ≤ f < frames[index + 1].
    let index = frames.partition_point(|key_frame| *key_frame <= f) - 1;
    let u = (f - frames[index]) as f64 / (frames[index + 1] - frames[index]) as f64;
    Segment::Between { index, u }
}

fn frames_of_scalar(keys: &[ScalarKey]) -> Vec<u32> {
    keys.iter().map(|key| key.frame).collect()
}

fn frames_of_vec(keys: &[VecKey]) -> Vec<u32> {
    keys.iter().map(|key| key.frame).collect()
}

fn frames_of_quat(keys: &[QuatKey]) -> Vec<u32> {
    keys.iter().map(|key| key.frame).collect()
}

fn frames_of_delta_scalar(keys: &[DeltaScalarKey]) -> Vec<u32> {
    keys.iter().map(|key| key.frame).collect()
}

fn frames_of_delta_vec(keys: &[DeltaVecKey]) -> Vec<u32> {
    keys.iter().map(|key| key.frame).collect()
}

fn frames_of_delta_quat(keys: &[DeltaQuatKey]) -> Vec<u32> {
    keys.iter().map(|key| key.frame).collect()
}

/// Evaluate a base scalar key list at frame `f` (§7.1).
fn eval_scalar_keys(keys: &[ScalarKey], f: u32) -> f64 {
    match locate(&frames_of_scalar(keys), f) {
        Segment::Hold(index) => keys[index].value,
        Segment::Between { index, u } => {
            let (a, b) = (keys[index], keys[index + 1]);
            match a.interpolation {
                Interpolation::Stepped => a.value,
                Interpolation::Linear => a.value + (b.value - a.value) * u,
            }
        }
    }
}

/// Evaluate a base vector key list at frame `f` (§7.1).
fn eval_vec_keys(keys: &[VecKey], f: u32) -> [f64; 3] {
    match locate(&frames_of_vec(keys), f) {
        Segment::Hold(index) => keys[index].value,
        Segment::Between { index, u } => {
            let (a, b) = (keys[index], keys[index + 1]);
            match a.interpolation {
                Interpolation::Stepped => a.value,
                Interpolation::Linear => [
                    a.value[0] + (b.value[0] - a.value[0]) * u,
                    a.value[1] + (b.value[1] - a.value[1]) * u,
                    a.value[2] + (b.value[2] - a.value[2]) * u,
                ],
            }
        }
    }
}

/// Evaluate a base quaternion key list at frame `f`:
/// nlerp, per section 7.1.
fn eval_quat_keys(keys: &[QuatKey], f: u32) -> [f64; 4] {
    match locate(&frames_of_quat(keys), f) {
        Segment::Hold(index) => keys[index].value,
        Segment::Between { index, u } => {
            let (a, b) = (keys[index], keys[index + 1]);
            match a.interpolation {
                Interpolation::Stepped => a.value,
                Interpolation::Linear => nlerp(a.value, b.value, u),
            }
        }
    }
}

/// Evaluate a scalar delta key list at frame `f`: the
/// section 7.1 segment law, over delta keys (section 7.2).
fn eval_delta_scalar_keys(keys: &[DeltaScalarKey], f: u32) -> f64 {
    match locate(&frames_of_delta_scalar(keys), f) {
        Segment::Hold(index) => keys[index].value,
        Segment::Between { index, u } => {
            let (a, b) = (keys[index], keys[index + 1]);
            match a.interpolation {
                Interpolation::Stepped => a.value,
                Interpolation::Linear => a.value + (b.value - a.value) * u,
            }
        }
    }
}

/// Evaluate a vector delta key list at frame `f`.
fn eval_delta_vec_keys(keys: &[DeltaVecKey], f: u32) -> [f64; 3] {
    match locate(&frames_of_delta_vec(keys), f) {
        Segment::Hold(index) => keys[index].value,
        Segment::Between { index, u } => {
            let (a, b) = (keys[index], keys[index + 1]);
            match a.interpolation {
                Interpolation::Stepped => a.value,
                Interpolation::Linear => [
                    a.value[0] + (b.value[0] - a.value[0]) * u,
                    a.value[1] + (b.value[1] - a.value[1]) * u,
                    a.value[2] + (b.value[2] - a.value[2]) * u,
                ],
            }
        }
    }
}

/// Evaluate a quaternion delta key list at frame `f`.
fn eval_delta_quat_keys(keys: &[DeltaQuatKey], f: u32) -> [f64; 4] {
    match locate(&frames_of_delta_quat(keys), f) {
        Segment::Hold(index) => keys[index].value,
        Segment::Between { index, u } => {
            let (a, b) = (keys[index], keys[index + 1]);
            match a.interpolation {
                Interpolation::Stepped => a.value,
                Interpolation::Linear => nlerp(a.value, b.value, u),
            }
        }
    }
}

/// Normalized linear interpolation between unit
/// quaternions (SPEC.md section 7.1): flip `b` to the short
/// way around when the dot product is negative, lerp, then
/// normalize. The norm of the lerp is bounded below (the
/// flipped dot product is ≥ 0, so the lerp of two unit
/// quaternions has norm² ≥ (1−u)² + u² ≥ ½); the assertion
/// states that invariant.
fn nlerp(a: [f64; 4], b: [f64; 4], u: f64) -> [f64; 4] {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let b = if dot < 0.0 {
        [-b[0], -b[1], -b[2], -b[3]]
    } else {
        b
    };
    let c = [
        a[0] + (b[0] - a[0]) * u,
        a[1] + (b[1] - a[1]) * u,
        a[2] + (b[2] - a[2]) * u,
        a[3] + (b[3] - a[3]) * u,
    ];
    let norm = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2] + c[3] * c[3]).sqrt();
    assert!(
        norm > 0.0,
        "nlerp midpoint of unit quaternions is never zero"
    );
    [c[0] / norm, c[1] / norm, c[2] / norm, c[3] / norm]
}

/// Quaternion multiplication `a ⊗ b`: the rotation that
/// applies `b` first, then `a` (SPEC.md section 2.2),
/// componentwise in f64, exactly as the spec writes it.
fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The weighted rotation delta `Δ(w, d)` of section 7.2:
/// canonicalize `d` (negate when `d.w < 0`, the short way
/// from identity), then nlerp from the identity by `w`.
/// The formula is written out in full — including the
/// identity's zero components — because section 7.3's
/// bit-exact weight-0 identity depends on the exact
/// arithmetic, signed zeros included: folding
/// `(1 − w) · 0` away would change the bits at `w = 0`
/// for deltas with negative components.
#[allow(clippy::identity_op)]
fn weighted_delta(w: f64, d: [f64; 4]) -> [f64; 4] {
    let d = if d[3] < 0.0 {
        [-d[0], -d[1], -d[2], -d[3]]
    } else {
        d
    };
    let one_minus_w = 1.0 - w;
    let raw = [
        one_minus_w * 0.0 + w * d[0],
        one_minus_w * 0.0 + w * d[1],
        one_minus_w * 0.0 + w * d[2],
        one_minus_w * 1.0 + w * d[3],
    ];
    let norm = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2] + raw[3] * raw[3]).sqrt();
    assert!(norm > 0.0, "weighted delta from identity is never zero");
    [raw[0] / norm, raw[1] / norm, raw[2] / norm, raw[3] / norm]
}
