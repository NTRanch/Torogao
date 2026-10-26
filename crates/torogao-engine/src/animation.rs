//! Animation data (the evaluation engine does not play
//! animations in the CLI profile; the census, the motion
//! event validator, and the exporters read their shape).

use serde_json::Value;
use torogao_format::value::{jget, jnum};

/// One keyframe of an animation lane.
#[derive(Debug, Clone, Copy)]
pub struct Keyframe {
    /// Time offset in steps.
    pub time_offset: f32,
    /// The keyed value.
    pub value: f32,
}

/// One animation lane (one parameter axis over time).
#[derive(Debug, Clone)]
pub struct AnimationLane {
    /// The animated parameter's uuid.
    pub param_uuid: u32,
    /// The animated axis.
    pub axis: i32,
    /// Keyframes.
    pub frames: Vec<Keyframe>,
}

/// One named animation.
#[derive(Debug, Clone)]
pub struct Animation {
    /// Whether the animation is additive.
    pub additive: bool,
    /// Lanes.
    pub lanes: Vec<AnimationLane>,
    /// Length in steps.
    pub length: i32,
    /// Seconds per step.
    pub timestep: f32,
}

impl Animation {
    /// Duration in seconds: length × timestep (the motion
    /// event validator's formula).
    pub fn duration(&self) -> f64 {
        self.length as f64 * self.timestep as f64
    }

    /// Total keyframes across all lanes.
    pub fn keyframe_count(&self) -> usize {
        self.lanes.iter().map(|lane| lane.frames.len()).sum()
    }

    /// Decodes an animation record.
    pub fn from_payload(raw: &Value) -> Animation {
        let lanes = match jget(raw, "lanes") {
            Value::Array(items) => items
                .iter()
                .map(|lane| {
                    let frames = match jget(lane, "keyframes") {
                        Value::Array(frames) => frames
                            .iter()
                            .map(|frame| Keyframe {
                                time_offset: jnum(frame, "time_offset", 0.0) as f32,
                                value: jnum(frame, "value", 0.0) as f32,
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    AnimationLane {
                        param_uuid: jnum(lane, "uuid", 0.0) as u32,
                        axis: jnum(lane, "target", 0.0) as i32,
                        frames,
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        Animation {
            additive: matches!(jget(raw, "additive"), Value::Bool(true)),
            lanes,
            length: jnum(raw, "length", 0.0) as i32,
            timestep: jnum(raw, "timestep", 0.0166) as f32,
        }
    }
}
