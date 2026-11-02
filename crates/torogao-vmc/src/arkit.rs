//! The ARKit-to-governed mapping: which of the 52 ARKit
//! blendshape channels drive which governed parameters —
//! the same parameter names the editor, the companion, and
//! the session host use (Stage 3.3's naming law). The table
//! is data, not code paths: a field correction against a
//! live tracker is a table edit, and the census tests pin
//! the table's shape (52 channels, every one either mapped
//! or named unmapped with its reason — never dropped).
//!
//! Conventions the table encodes (each is a named,
//! single-place decision, field-verifiable against a real
//! tracker):
//! - Scalar-mapped channels are amounts in 0..=1 on both
//!   sides (the ARKit definition), applied direct.
//! - Blink channels are blink *amount* (0 = open,
//!   1 = closed), matching the governed `Blink` parameters.
//! - Eye look composes per eye: x = out − in for the left
//!   eye, in − out for the right (so +x is gaze toward the
//!   viewer's right with the performer facing the viewer),
//!   y = up − down. Both clamp to −1..=1.
//! - Mouth open (`jawOpen`) drives the `Mouth:: Shape`
//!   x axis — the same axis the viseme baker drives (its
//!   curve series are axis 0 by construction), so live
//!   tracking and baked lipsync agree on the parameter's
//!   meaning. Mouth form composes onto the y axis as
//!   mean(smile) − mean(frown), clamped to −1..=1.
//! - Head and body angles come from the `Head` and `Chest`
//!   bone quaternions (YXZ euler, normalized by π, clamped
//!   to −1..=1) onto the `Yaw-Pitch` (x = yaw, y = pitch)
//!   and `Roll` parameters.

use std::collections::BTreeMap;

use crate::osc::{OscArg, OscMessage};
use crate::vmc::{ADDRESS_BLEND_APPLY, ADDRESS_BLEND_VAL, BonePose, VmcFrame};

/// The 52 ARKit blendshape channel names, sorted.
pub const ARKIT_BLENDSHAPES: &[&str] = &[
    "browDownLeft",
    "browDownRight",
    "browInnerUp",
    "browOuterUpLeft",
    "browOuterUpRight",
    "cheekPuff",
    "cheekSquintLeft",
    "cheekSquintRight",
    "eyeBlinkLeft",
    "eyeBlinkRight",
    "eyeLookDownLeft",
    "eyeLookDownRight",
    "eyeLookInLeft",
    "eyeLookInRight",
    "eyeLookOutLeft",
    "eyeLookOutRight",
    "eyeLookUpLeft",
    "eyeLookUpRight",
    "eyeSquintLeft",
    "eyeSquintRight",
    "eyeWideLeft",
    "eyeWideRight",
    "jawForward",
    "jawLeft",
    "jawOpen",
    "jawRight",
    "mouthClose",
    "mouthDimpleLeft",
    "mouthDimpleRight",
    "mouthFrownLeft",
    "mouthFrownRight",
    "mouthFunnel",
    "mouthLeft",
    "mouthLowerDownLeft",
    "mouthLowerDownRight",
    "mouthPressLeft",
    "mouthPressRight",
    "mouthPucker",
    "mouthRight",
    "mouthRollLower",
    "mouthRollUpper",
    "mouthShrugLower",
    "mouthShrugUpper",
    "mouthSmileLeft",
    "mouthSmileRight",
    "mouthStretchLeft",
    "mouthStretchRight",
    "mouthUpperUpLeft",
    "mouthUpperUpRight",
    "noseSneerLeft",
    "noseSneerRight",
    "tongueOut",
];

/// The governed parameters tracking drives, sorted: the
/// cast models' own parameter names.
pub const GOVERNED_PARAMETERS: &[&str] = &[
    "Body:: Roll",
    "Body:: Yaw-Pitch",
    "Eye:: Left:: Blink",
    "Eye:: Left:: Move",
    "Eye:: Right:: Blink",
    "Eye:: Right:: Move",
    "Head:: Roll",
    "Head:: Yaw-Pitch",
    "Mouth:: Shape",
];

/// The ARKit channels the mapping consumes, sorted.
pub const ARKIT_MAPPED: &[&str] = &[
    "eyeBlinkLeft",
    "eyeBlinkRight",
    "eyeLookDownLeft",
    "eyeLookDownRight",
    "eyeLookInLeft",
    "eyeLookInRight",
    "eyeLookOutLeft",
    "eyeLookOutRight",
    "eyeLookUpLeft",
    "eyeLookUpRight",
    "jawOpen",
    "mouthFrownLeft",
    "mouthFrownRight",
    "mouthSmileLeft",
    "mouthSmileRight",
];

/// The ARKit channels with no governed counterpart, each
/// with the reason recorded — the census names them
/// rather than dropping them.
pub const ARKIT_UNMAPPED: &[(&str, &str)] = &[
    ("browDownLeft", "no brow parameters on the governed rigs"),
    ("browDownRight", "no brow parameters on the governed rigs"),
    ("browInnerUp", "no brow parameters on the governed rigs"),
    ("browOuterUpLeft", "no brow parameters on the governed rigs"),
    (
        "browOuterUpRight",
        "no brow parameters on the governed rigs",
    ),
    ("cheekPuff", "no cheek parameters on the governed rigs"),
    (
        "cheekSquintLeft",
        "no cheek parameters on the governed rigs",
    ),
    (
        "cheekSquintRight",
        "no cheek parameters on the governed rigs",
    ),
    (
        "eyeSquintLeft",
        "no squint parameters on the governed rigs; blink is the governed closure channel",
    ),
    (
        "eyeSquintRight",
        "no squint parameters on the governed rigs; blink is the governed closure channel",
    ),
    ("eyeWideLeft", "no wide-eye parameters on the governed rigs"),
    (
        "eyeWideRight",
        "no wide-eye parameters on the governed rigs",
    ),
    (
        "jawForward",
        "jawOpen is the governed jaw channel; a protruding jaw has no governed parameter",
    ),
    (
        "jawLeft",
        "jawOpen is the governed jaw channel; lateral jaw has no governed parameter",
    ),
    (
        "jawRight",
        "jawOpen is the governed jaw channel; lateral jaw has no governed parameter",
    ),
    (
        "mouthClose",
        "closure rides jawOpen's return on the governed rigs",
    ),
    (
        "mouthDimpleLeft",
        "no dimple parameters on the governed rigs",
    ),
    (
        "mouthDimpleRight",
        "no dimple parameters on the governed rigs",
    ),
    ("mouthFunnel", "no funnel parameter on the governed rigs"),
    (
        "mouthLeft",
        "no lateral mouth parameters on the governed rigs",
    ),
    (
        "mouthLowerDownLeft",
        "no lower-lip parameters on the governed rigs",
    ),
    (
        "mouthLowerDownRight",
        "no lower-lip parameters on the governed rigs",
    ),
    (
        "mouthPressLeft",
        "no lip-press parameters on the governed rigs",
    ),
    (
        "mouthPressRight",
        "no lip-press parameters on the governed rigs",
    ),
    ("mouthPucker", "no pucker parameter on the governed rigs"),
    (
        "mouthRight",
        "no lateral mouth parameters on the governed rigs",
    ),
    (
        "mouthRollLower",
        "no lip-roll parameters on the governed rigs",
    ),
    (
        "mouthRollUpper",
        "no lip-roll parameters on the governed rigs",
    ),
    (
        "mouthShrugLower",
        "no shrug parameters on the governed rigs",
    ),
    (
        "mouthShrugUpper",
        "no shrug parameters on the governed rigs",
    ),
    (
        "mouthStretchLeft",
        "no stretch parameters on the governed rigs",
    ),
    (
        "mouthStretchRight",
        "no stretch parameters on the governed rigs",
    ),
    (
        "mouthUpperUpLeft",
        "no upper-lip parameters on the governed rigs",
    ),
    (
        "mouthUpperUpRight",
        "no upper-lip parameters on the governed rigs",
    ),
    ("noseSneerLeft", "no nose parameters on the governed rigs"),
    ("noseSneerRight", "no nose parameters on the governed rigs"),
    ("tongueOut", "no tongue parameter on the governed rigs"),
];

/// Which component of a governed Vec2 parameter a value
/// addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// The x component.
    X,
    /// The y component.
    Y,
}

/// One governed parameter value: the components tracking
/// governs this frame. A component that is `None` is not
/// tracking-governed and holds its standing value.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GovernedValue {
    /// The x component, when governed.
    pub x: Option<f32>,
    /// The y component, when governed.
    pub y: Option<f32>,
}

impl GovernedValue {
    /// A value governing only x.
    pub fn x(value: f32) -> GovernedValue {
        GovernedValue {
            x: Some(value),
            y: None,
        }
    }

    /// A value governing only y.
    pub fn y(value: f32) -> GovernedValue {
        GovernedValue {
            x: None,
            y: Some(value),
        }
    }

    /// A value governing both components.
    pub fn xy(x: f32, y: f32) -> GovernedValue {
        GovernedValue {
            x: Some(x),
            y: Some(y),
        }
    }

    /// Overwrites this value's governed components with
    /// `other`'s (the tracking state's merge rule).
    pub fn merge(&mut self, other: GovernedValue) {
        if other.x.is_some() {
            self.x = other.x;
        }
        if other.y.is_some() {
            self.y = other.y;
        }
    }
}

/// The one-to-one scalar rows of the mapping table:
/// (ARKit channel, governed parameter, governed axis).
/// Composed channels (eye look, mouth form) and the bone
/// channels are functions below, not rows.
const SCALAR_MAP: &[(&str, &str, Axis)] = &[
    ("eyeBlinkLeft", "Eye:: Left:: Blink", Axis::X),
    ("eyeBlinkRight", "Eye:: Right:: Blink", Axis::X),
    ("jawOpen", "Mouth:: Shape", Axis::X),
];

/// The VMC bone whose angles drive the head parameters.
pub const HEAD_BONE: &str = "Head";
/// The VMC bone whose angles drive the body parameters.
pub const BODY_BONE: &str = "Chest";

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value.is_nan() {
        return 0.0;
    }
    value.clamp(min, max)
}

fn clamp01(value: f32) -> f32 {
    clamp(value, 0.0, 1.0)
}

fn clamp11(value: f32) -> f32 {
    clamp(value, -1.0, 1.0)
}

/// Joint angles extracted from a bone quaternion,
/// normalized so ±π maps to ±1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointAngles {
    /// Yaw (about Y), −1..=1.
    pub yaw: f32,
    /// Pitch (about X), −1..=1.
    pub pitch: f32,
    /// Roll (about Z), −1..=1.
    pub roll: f32,
}

/// Extracts YXZ euler angles from a bone pose's
/// quaternion (normalized first; a zero-length quaternion
/// reads as identity). The conversion is the standard
/// rotation-matrix route: for R = Ry·Rx·Rz,
/// pitch = asin(−R[1][2]), yaw = atan2(R[0][2], R[2][2]),
/// roll = atan2(R[1][0], R[1][1]).
pub fn joint_angles(pose: &BonePose) -> JointAngles {
    let [x, y, z, w] = pose.rotation;
    let length = (x * x + y * y + z * z + w * w).sqrt();
    if length <= 0.0 || !length.is_finite() {
        return JointAngles {
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        };
    }
    let (x, y, z, w) = (x / length, y / length, z / length, w / length);
    let r02 = 2.0 * (x * z + w * y);
    let r10 = 2.0 * (x * y + w * z);
    let r11 = 1.0 - 2.0 * (x * x + z * z);
    let r12 = 2.0 * (y * z - w * x);
    let r22 = 1.0 - 2.0 * (x * x + y * y);
    let to_normalized = |radians: f32| clamp11(radians / std::f32::consts::PI);
    JointAngles {
        yaw: to_normalized(r02.atan2(r22)),
        pitch: to_normalized((-r12).clamp(-1.0, 1.0).asin()),
        roll: to_normalized(r10.atan2(r11)),
    }
}

/// Composes one eye's look direction from its four ARKit
/// channels (`is_left` selects the mirroring convention in
/// the module note). Returns `None` when the frame carries
/// none of the four channels for this eye.
fn eye_look(frame: &VmcFrame, is_left: bool) -> Option<(f32, f32)> {
    let (down, inward, outward, up) = if is_left {
        (
            frame.blend("eyeLookDownLeft"),
            frame.blend("eyeLookInLeft"),
            frame.blend("eyeLookOutLeft"),
            frame.blend("eyeLookUpLeft"),
        )
    } else {
        (
            frame.blend("eyeLookDownRight"),
            frame.blend("eyeLookInRight"),
            frame.blend("eyeLookOutRight"),
            frame.blend("eyeLookUpRight"),
        )
    };
    if down.is_none() && inward.is_none() && outward.is_none() && up.is_none() {
        return None;
    }
    let inward = clamp01(inward.unwrap_or(0.0));
    let outward = clamp01(outward.unwrap_or(0.0));
    let x = if is_left {
        outward - inward
    } else {
        inward - outward
    };
    let y = clamp01(up.unwrap_or(0.0)) - clamp01(down.unwrap_or(0.0));
    Some((clamp11(x), clamp11(y)))
}

/// Converts one tracked frame into governed parameter
/// values: only the parameters this frame actually
/// governs appear in the result.
pub fn governed_values(frame: &VmcFrame) -> BTreeMap<String, GovernedValue> {
    let mut out: BTreeMap<String, GovernedValue> = BTreeMap::new();
    for (arkit, governed, axis) in SCALAR_MAP {
        if let Some(value) = frame.blend(arkit) {
            let value = clamp01(value);
            let entry = out.entry(governed.to_string()).or_default();
            match axis {
                Axis::X => entry.x = Some(value),
                Axis::Y => entry.y = Some(value),
            }
        }
    }
    for (is_left, governed) in [(true, "Eye:: Left:: Move"), (false, "Eye:: Right:: Move")] {
        if let Some((x, y)) = eye_look(frame, is_left) {
            out.insert(governed.to_string(), GovernedValue::xy(x, y));
        }
    }
    let smile = match (
        frame.blend("mouthSmileLeft"),
        frame.blend("mouthSmileRight"),
    ) {
        (None, None) => None,
        (left, right) => Some((clamp01(left.unwrap_or(0.0)) + clamp01(right.unwrap_or(0.0))) / 2.0),
    };
    let frown = match (
        frame.blend("mouthFrownLeft"),
        frame.blend("mouthFrownRight"),
    ) {
        (None, None) => None,
        (left, right) => Some((clamp01(left.unwrap_or(0.0)) + clamp01(right.unwrap_or(0.0))) / 2.0),
    };
    if smile.is_some() || frown.is_some() {
        let form = clamp11(smile.unwrap_or(0.0) - frown.unwrap_or(0.0));
        out.entry("Mouth:: Shape".to_string()).or_default().y = Some(form);
    }
    for (bone_name, yaw_pitch, roll) in [
        (HEAD_BONE, "Head:: Yaw-Pitch", "Head:: Roll"),
        (BODY_BONE, "Body:: Yaw-Pitch", "Body:: Roll"),
    ] {
        if let Some(pose) = frame.bone(bone_name) {
            let angles = joint_angles(pose);
            out.insert(
                yaw_pitch.to_string(),
                GovernedValue::xy(angles.yaw, angles.pitch),
            );
            out.insert(roll.to_string(), GovernedValue::x(angles.roll));
        }
    }
    out
}

/// The governed state as outbound VMC messages (Torogao
/// sending to OBS-side consumers): each governed component
/// is one blend value — x under the parameter's own name,
/// y under `<name>/y` — closed by an Apply, so the same
/// names resolve on the wire as in the editor.
pub fn governed_messages(values: &BTreeMap<String, GovernedValue>) -> Vec<OscMessage> {
    let mut out = Vec::with_capacity(values.len() * 2 + 1);
    for (name, value) in values {
        if let Some(x) = value.x {
            out.push(OscMessage {
                address: ADDRESS_BLEND_VAL.to_string(),
                args: vec![OscArg::Str(name.clone()), OscArg::Float(x)],
            });
        }
        if let Some(y) = value.y {
            out.push(OscMessage {
                address: ADDRESS_BLEND_VAL.to_string(),
                args: vec![OscArg::Str(format!("{name}/y")), OscArg::Float(y)],
            });
        }
    }
    out.push(OscMessage {
        address: ADDRESS_BLEND_APPLY.to_string(),
        args: Vec::new(),
    });
    out
}

/// Reads governed values back from blend pairs (the
/// inverse of `governed_messages`' naming): `<name>/y`
/// addresses the y component, a bare name the x.
pub fn governed_from_blends(blends: &[(String, f32)]) -> BTreeMap<String, GovernedValue> {
    let mut out: BTreeMap<String, GovernedValue> = BTreeMap::new();
    for (name, value) in blends {
        if let Some(base) = name.strip_suffix("/y") {
            out.entry(base.to_string()).or_default().y = Some(*value);
        } else {
            out.entry(name.clone()).or_default().x = Some(*value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        ARKIT_BLENDSHAPES, ARKIT_MAPPED, ARKIT_UNMAPPED, GovernedValue, governed_from_blends,
        governed_messages, governed_values, joint_angles,
    };
    use crate::vmc::{BonePose, FrameAssembler, VmcFrame};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn arkit_census_is_complete_and_disjoint() {
        assert_eq!(ARKIT_BLENDSHAPES.len(), 52);
        let all: BTreeSet<&str> = ARKIT_BLENDSHAPES.iter().copied().collect();
        assert_eq!(all.len(), 52, "channel names are unique");
        let mapped: BTreeSet<&str> = ARKIT_MAPPED.iter().copied().collect();
        let unmapped: BTreeSet<&str> = ARKIT_UNMAPPED.iter().map(|(name, _)| *name).collect();
        assert!(mapped.is_disjoint(&unmapped), "no channel is both");
        assert_eq!(
            mapped.union(&unmapped).copied().collect::<BTreeSet<_>>(),
            all
        );
        for (_, reason) in ARKIT_UNMAPPED {
            assert!(
                !reason.is_empty(),
                "every unmapped channel names its reason"
            );
        }
    }

    fn approx(actual: Option<f32>, expected: f32) {
        let actual = actual.expect("value present");
        assert!(
            (actual - expected).abs() <= 1e-3,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn governed_values_map_the_whole_table() {
        let mut frame = VmcFrame {
            blends: vec![
                ("eyeBlinkLeft".to_string(), 0.7),
                ("eyeBlinkRight".to_string(), 0.2),
                ("eyeLookInLeft".to_string(), 1.0),
                ("eyeLookUpRight".to_string(), 0.5),
                ("jawOpen".to_string(), 0.4),
                ("mouthSmileLeft".to_string(), 0.6),
                ("mouthSmileRight".to_string(), 1.0),
                ("mouthFrownLeft".to_string(), 0.2),
            ],
            ..VmcFrame::default()
        };
        frame.bones.push(BonePose {
            name: "Head".to_string(),
            position: [0.0; 3],
            rotation: [
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
        });
        let values = governed_values(&frame);
        assert_eq!(values["Eye:: Left:: Blink"].x, Some(0.7));
        assert_eq!(values["Eye:: Right:: Blink"].x, Some(0.2));
        // Left eye looking in (toward the nose) is viewer-left: x = -1.
        assert_eq!(values["Eye:: Left:: Move"].x, Some(-1.0));
        assert_eq!(values["Eye:: Right:: Move"].y, Some(0.5));
        assert_eq!(values["Mouth:: Shape"].x, Some(0.4));
        // Form: mean smile 0.8 - mean frown 0.1 = 0.7.
        approx(values["Mouth:: Shape"].y, 0.7);
        // A +90-degree yaw is +0.5 normalized.
        approx(values["Head:: Yaw-Pitch"].x, 0.5);
        approx(values["Head:: Yaw-Pitch"].y, 0.0);
    }

    #[test]
    fn joint_angles_read_identity_and_pitch() {
        let identity = BonePose {
            name: "Head".to_string(),
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        let angles = joint_angles(&identity);
        assert_eq!((angles.yaw, angles.pitch, angles.roll), (0.0, 0.0, 0.0));
        let pitched = BonePose {
            rotation: [
                std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            ..identity
        };
        approx(Some(joint_angles(&pitched).pitch), 0.5);
    }

    #[test]
    fn governed_values_round_trip_over_the_wire() {
        let mut values = BTreeMap::new();
        values.insert("Eye:: Left:: Blink".to_string(), GovernedValue::x(0.25));
        values.insert(
            "Head:: Yaw-Pitch".to_string(),
            GovernedValue::xy(0.5, -0.25),
        );
        values.insert("Mouth:: Shape".to_string(), GovernedValue::xy(0.1, 0.9));
        let mut assembler = FrameAssembler::new();
        let mut completed = None;
        for message in governed_messages(&values) {
            completed = assembler.push(&message).expect("push").or(completed);
        }
        let frame = completed.expect("the Apply completes a frame");
        assert_eq!(governed_from_blends(&frame.blends), values);
    }
}
