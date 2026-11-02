//! The VMC frame model: the messages a VMC sender emits
//! per performance frame, assembled back into one frame
//! on receive. VMC (Virtual Motion Capture protocol) rides
//! OSC/UDP: blend values by name, bone poses by name, a
//! root pose, a time stamp, and `Blend/Apply`, which marks
//! the frame complete — the assembler emits a frame only
//! at Apply, so consumers never see a half-applied pose.
//!
//! Unknown addresses are tolerated (a VMC stream shares
//! the port with other OSC traffic); known addresses with
//! the wrong argument shape refuse typed.

use std::fmt;

use crate::osc::{OscArg, OscMessage};

/// Blend value: `(name, value)`.
pub const ADDRESS_BLEND_VAL: &str = "/VMC/Ext/Blend/Val";
/// Blend apply: the frame-complete marker (no arguments).
pub const ADDRESS_BLEND_APPLY: &str = "/VMC/Ext/Blend/Apply";
/// Bone pose: `(name, pos xyz, quaternion xyzw)`.
pub const ADDRESS_BONE_POS: &str = "/VMC/Ext/Bone/Pos";
/// Root pose: `(name, pos xyz, quaternion xyzw, …)` —
/// later fields (scale, offset) are tolerated, not read.
pub const ADDRESS_ROOT_POS: &str = "/VMC/Ext/Root/Pos";
/// Availability report `(loaded, calibration state, …)`;
/// understood, not consumed by the frame model.
pub const ADDRESS_OK: &str = "/VMC/Ext/OK";
/// Time stamp: `(seconds)` since the sender's start.
pub const ADDRESS_TIME: &str = "/VMC/Ext/T";
/// The VMC protocol's conventional receive port.
pub const VMC_PORT: u16 = 39539;

/// One bone's pose in sender space.
#[derive(Debug, Clone, PartialEq)]
pub struct BonePose {
    /// The bone's name (e.g. "Head", "Chest").
    pub name: String,
    /// Position (x, y, z).
    pub position: [f32; 3],
    /// Rotation quaternion (x, y, z, w).
    pub rotation: [f32; 4],
}

/// The root (whole-performer) pose.
#[derive(Debug, Clone, PartialEq)]
pub struct RootPose {
    /// Position (x, y, z).
    pub position: [f32; 3],
    /// Rotation quaternion (x, y, z, w).
    pub rotation: [f32; 4],
}

/// One performance frame: everything a sender emitted
/// between two Apply markers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VmcFrame {
    /// Blend values by wire name, in arrival order.
    pub blends: Vec<(String, f32)>,
    /// Bone poses, in arrival order.
    pub bones: Vec<BonePose>,
    /// The root pose, when one was sent.
    pub root: Option<RootPose>,
    /// The sender's time stamp in seconds, when sent.
    pub time_seconds: Option<f32>,
}

impl VmcFrame {
    /// One blend value by name (first occurrence).
    pub fn blend(&self, name: &str) -> Option<f32> {
        self.blends
            .iter()
            .find(|(blend_name, _)| blend_name == name)
            .map(|(_, value)| *value)
    }

    /// One bone pose by name (first occurrence).
    pub fn bone(&self, name: &str) -> Option<&BonePose> {
        self.bones.iter().find(|bone| bone.name == name)
    }

    /// The frame as wire messages, in VMC emission order
    /// (time, blends, bones, root, Apply last).
    pub fn messages(&self) -> Vec<OscMessage> {
        let mut out = Vec::with_capacity(self.blends.len() + self.bones.len() + 3);
        if let Some(time) = self.time_seconds {
            out.push(OscMessage {
                address: ADDRESS_TIME.to_string(),
                args: vec![OscArg::Float(time)],
            });
        }
        for (name, value) in &self.blends {
            out.push(OscMessage {
                address: ADDRESS_BLEND_VAL.to_string(),
                args: vec![OscArg::Str(name.clone()), OscArg::Float(*value)],
            });
        }
        for bone in &self.bones {
            out.push(OscMessage {
                address: ADDRESS_BONE_POS.to_string(),
                args: bone_args(&bone.name, &bone.position, &bone.rotation),
            });
        }
        if let Some(root) = &self.root {
            out.push(OscMessage {
                address: ADDRESS_ROOT_POS.to_string(),
                args: bone_args("Root", &root.position, &root.rotation),
            });
        }
        out.push(OscMessage {
            address: ADDRESS_BLEND_APPLY.to_string(),
            args: Vec::new(),
        });
        out
    }
}

fn bone_args(name: &str, position: &[f32; 3], rotation: &[f32; 4]) -> Vec<OscArg> {
    let mut args = Vec::with_capacity(8);
    args.push(OscArg::Str(name.to_string()));
    for value in position.iter().chain(rotation.iter()) {
        args.push(OscArg::Float(*value));
    }
    args
}

/// Why a message refused to join a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmcError {
    /// A known address with the wrong argument count.
    WrongArity {
        /// The message's address.
        address: String,
        /// The minimum argument count the address requires.
        expected: usize,
        /// The argument count the message carried.
        got: usize,
    },
    /// A known address with a wrongly-typed argument.
    WrongType {
        /// The message's address.
        address: String,
        /// The offending argument's index.
        index: usize,
    },
}

impl fmt::Display for VmcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmcError::WrongArity {
                address,
                expected,
                got,
            } => write!(
                f,
                "VMC message {address} carries {got} arguments; it requires at least {expected}"
            ),
            VmcError::WrongType { address, index } => {
                write!(
                    f,
                    "VMC message {address} argument {index} has the wrong type"
                )
            }
        }
    }
}

impl std::error::Error for VmcError {}

fn string_arg(message: &OscMessage, index: usize) -> Result<&str, VmcError> {
    match message.args.get(index) {
        Some(OscArg::Str(text)) => Ok(text),
        _ => Err(VmcError::WrongType {
            address: message.address.clone(),
            index,
        }),
    }
}

fn float_arg(message: &OscMessage, index: usize) -> Result<f32, VmcError> {
    match message.args.get(index) {
        Some(OscArg::Float(value)) => Ok(*value),
        _ => Err(VmcError::WrongType {
            address: message.address.clone(),
            index,
        }),
    }
}

fn require_arity(message: &OscMessage, expected: usize) -> Result<(), VmcError> {
    if message.args.len() < expected {
        return Err(VmcError::WrongArity {
            address: message.address.clone(),
            expected,
            got: message.args.len(),
        });
    }
    Ok(())
}

/// Collects messages into frames: `push` returns the
/// completed frame at each Apply marker, `None` while a
/// frame is still assembling. Bone poses replace by name
/// within a frame (a sender repeating a bone updates it);
/// blend values accumulate in arrival order.
#[derive(Debug, Default)]
pub struct FrameAssembler {
    pending: VmcFrame,
}

impl FrameAssembler {
    /// A fresh assembler with no partial frame.
    pub fn new() -> FrameAssembler {
        FrameAssembler::default()
    }

    /// Feeds one message. Foreign addresses (anything the
    /// frame model does not consume) are ignored by design.
    pub fn push(&mut self, message: &OscMessage) -> Result<Option<VmcFrame>, VmcError> {
        match message.address.as_str() {
            ADDRESS_BLEND_APPLY => Ok(Some(std::mem::take(&mut self.pending))),
            ADDRESS_BLEND_VAL => {
                require_arity(message, 2)?;
                let name = string_arg(message, 0)?.to_string();
                let value = float_arg(message, 1)?;
                self.pending.blends.push((name, value));
                Ok(None)
            }
            ADDRESS_BONE_POS => {
                require_arity(message, 8)?;
                let pose = BonePose {
                    name: string_arg(message, 0)?.to_string(),
                    position: [
                        float_arg(message, 1)?,
                        float_arg(message, 2)?,
                        float_arg(message, 3)?,
                    ],
                    rotation: [
                        float_arg(message, 4)?,
                        float_arg(message, 5)?,
                        float_arg(message, 6)?,
                        float_arg(message, 7)?,
                    ],
                };
                match self
                    .pending
                    .bones
                    .iter_mut()
                    .find(|bone| bone.name == pose.name)
                {
                    Some(existing) => *existing = pose,
                    None => self.pending.bones.push(pose),
                }
                Ok(None)
            }
            ADDRESS_ROOT_POS => {
                require_arity(message, 8)?;
                let _name = string_arg(message, 0)?;
                self.pending.root = Some(RootPose {
                    position: [
                        float_arg(message, 1)?,
                        float_arg(message, 2)?,
                        float_arg(message, 3)?,
                    ],
                    rotation: [
                        float_arg(message, 4)?,
                        float_arg(message, 5)?,
                        float_arg(message, 6)?,
                        float_arg(message, 7)?,
                    ],
                });
                Ok(None)
            }
            ADDRESS_TIME => {
                require_arity(message, 1)?;
                self.pending.time_seconds = Some(float_arg(message, 0)?);
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameAssembler, VmcFrame};
    use crate::osc::{OscArg, OscMessage};

    fn blend(name: &str, value: f32) -> OscMessage {
        OscMessage {
            address: super::ADDRESS_BLEND_VAL.to_string(),
            args: vec![OscArg::Str(name.to_string()), OscArg::Float(value)],
        }
    }

    #[test]
    fn assembler_emits_one_frame_per_apply() {
        let mut assembler = FrameAssembler::new();
        assert!(
            assembler
                .push(&blend("eyeBlinkLeft", 0.5))
                .expect("push")
                .is_none()
        );
        let frame = assembler
            .push(&OscMessage {
                address: super::ADDRESS_BLEND_APPLY.to_string(),
                args: Vec::new(),
            })
            .expect("apply")
            .expect("frame completes");
        assert_eq!(frame.blend("eyeBlinkLeft"), Some(0.5));
        // The next frame starts empty.
        let frame = assembler
            .push(&OscMessage {
                address: super::ADDRESS_BLEND_APPLY.to_string(),
                args: Vec::new(),
            })
            .expect("apply")
            .expect("frame completes");
        assert_eq!(frame, VmcFrame::default());
    }

    #[test]
    fn frame_messages_round_trip_through_the_assembler() {
        let mut frame = VmcFrame::default();
        frame.blends.push(("jawOpen".to_string(), 0.25));
        frame.time_seconds = Some(1.5);
        frame.bones.push(super::BonePose {
            name: "Head".to_string(),
            position: [0.0, 1.6, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
        let mut assembler = FrameAssembler::new();
        let mut completed = None;
        for message in frame.messages() {
            completed = assembler.push(&message).expect("push").or(completed);
        }
        assert_eq!(completed.expect("frame completes"), frame);
    }

    #[test]
    fn assembler_refuses_wrong_shapes_and_tolerates_foreign_traffic() {
        let mut assembler = FrameAssembler::new();
        let foreign = OscMessage {
            address: "/some/other/protocol".to_string(),
            args: Vec::new(),
        };
        assert!(assembler.push(&foreign).expect("tolerated").is_none());
        let bad = OscMessage {
            address: super::ADDRESS_BLEND_VAL.to_string(),
            args: vec![OscArg::Float(1.0)],
        };
        assert!(assembler.push(&bad).is_err());
    }
}
