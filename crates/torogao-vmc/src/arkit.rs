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

/// The reduction boundary of the deterministic
/// arctangent: tan(π/8) = √2 − 1. Past it, the π/4
/// reduction identity folds the argument back into the
/// reduced range (see `atan_det_unit`).
const ATAN_DET_REDUCE_AT: f64 = 0.41421356237309503;

/// Taylor coefficients of atan on the reduced range
/// |x| ≤ tan(π/8): atan(x) = x·(c₀ + x²·(c₁ + x²·(…))),
/// c_k = (−1)^k/(2k+1) for k = 0..=12 (through x²⁵),
/// written as f64 literals. Provenance (D-538): these
/// are the Maclaurin series' own rationals — no fitting
/// and no libm — and the first omitted term bounds the
/// truncation error at x²⁷/27 ≤ 1.8e-12 on the reduced
/// range (measured max error 1.5e-12 against a 50-digit
/// reference over the full atan2/asin domains at
/// implementation time; the dense-sweep test re-proves
/// ≤ 1 f32 ulp against this platform's libm on every
/// run).
const ATAN_DET_TAYLOR: [f64; 13] = [
    1.0,
    -0.3333333333333333,
    0.2,
    -0.14285714285714285,
    0.1111111111111111,
    -0.09090909090909091,
    0.07692307692307693,
    -0.06666666666666667,
    0.058823529411764705,
    -0.05263157894736842,
    0.047619047619047616,
    -0.043478260869565216,
    0.04,
];

/// atan(x) for |x| ≤ tan(π/8), by the `ATAN_DET_TAYLOR`
/// series in Horner form over x².
fn atan_det_reduced(x: f64) -> f64 {
    let z = x * x;
    let mut acc = ATAN_DET_TAYLOR[ATAN_DET_TAYLOR.len() - 1];
    for coefficient in ATAN_DET_TAYLOR[..ATAN_DET_TAYLOR.len() - 1].iter().rev() {
        acc = acc * z + coefficient;
    }
    x * acc
}

/// atan(t) for t in 0..=1: past tan(π/8), the identity
/// atan(t) = π/4 + atan((t−1)/(t+1)) folds the argument
/// into −tan(π/8)..=0 (tan(π/8) is the identity's fixed
/// point, so the folded argument never leaves the
/// reduced range).
fn atan_det_unit(t: f64) -> f64 {
    if t > ATAN_DET_REDUCE_AT {
        std::f64::consts::FRAC_PI_4 + atan_det_reduced((t - 1.0) / (t + 1.0))
    } else {
        atan_det_reduced(t)
    }
}

/// Deterministic atan2 (D-538): quadrant reduction to
/// the acute angle atan2(|y|, |x|) ∈ 0..=π/2 — by
/// reciprocal reduction into `atan_det_unit` — then the
/// quadrant and the sign of y (signed zeros included,
/// matching libm's edge behaviour exactly). Computed
/// entirely in f64 from pinned literals: no libm
/// transcendental is consulted, so the result is a pure
/// function of the input bits on every platform.
fn atan2_det(y: f64, x: f64) -> f64 {
    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let acute = if ax.is_infinite() || ay.is_infinite() {
        if ax.is_infinite() && ay.is_infinite() {
            std::f64::consts::FRAC_PI_4
        } else if ay.is_infinite() {
            std::f64::consts::FRAC_PI_2
        } else {
            0.0
        }
    } else if ay == 0.0 {
        0.0
    } else if ax == 0.0 {
        std::f64::consts::FRAC_PI_2
    } else if ay <= ax {
        atan_det_unit(ay / ax)
    } else {
        std::f64::consts::FRAC_PI_2 - atan_det_unit(ax / ay)
    };
    let magnitude = if x < 0.0 || (x == 0.0 && x.is_sign_negative()) {
        std::f64::consts::PI - acute
    } else {
        acute
    };
    if y.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    }
}

/// Deterministic asin (D-538): asin(x) =
/// atan2(x, √(1−x²)). The f64 sqrt is IEEE-exact
/// (correctly rounded on every platform), so the frozen
/// atan2 carries the whole mapping.
fn asin_det(x: f64) -> f64 {
    atan2_det(x, (1.0 - x * x).sqrt())
}

/// The frame's raw ARKit blendshape values as one
/// 52-wide frame, in `ARKIT_BLENDSHAPES` order (D-553):
/// every channel the sender carried, mapped or not, at
/// its first-occurrence value (the `VmcFrame::blend`
/// rule); a channel the frame did not carry reads 0.0.
/// This is the measurements view the mimicry recorder
/// preserves alongside the governed values.
pub fn raw_blendshape_frame(frame: &VmcFrame) -> [f32; 52] {
    let mut raw = [0.0_f32; 52];
    for (slot, name) in raw.iter_mut().zip(ARKIT_BLENDSHAPES.iter()) {
        *slot = frame.blend(name).unwrap_or(0.0);
    }
    raw
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
///
/// The mapping is frozen in-crate (D-538): the
/// quaternion products are promoted to f64 and the
/// angles come from the deterministic `atan2_det` /
/// `asin_det`, never from libm — glibc's `atan2f`/`asinf`
/// differ by 1 ulp between versions (the D-521
/// diagnosis), which made a recorded take depend on the
/// recording machine. Here a recorded angle is a pure
/// function of the input bits on every platform.
pub fn joint_angles(pose: &BonePose) -> JointAngles {
    let [x, y, z, w] = pose.rotation;
    let (x, y, z, w) = (f64::from(x), f64::from(y), f64::from(z), f64::from(w));
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
    let to_normalized = |radians: f64| clamp11((radians / std::f64::consts::PI) as f32);
    JointAngles {
        yaw: to_normalized(atan2_det(r02, r22)),
        pitch: to_normalized(asin_det((-r12).clamp(-1.0, 1.0))),
        roll: to_normalized(atan2_det(r10, r11)),
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
        ARKIT_BLENDSHAPES, ARKIT_MAPPED, ARKIT_UNMAPPED, GovernedValue, asin_det, atan2_det,
        governed_from_blends, governed_messages, governed_values, joint_angles,
        raw_blendshape_frame,
    };
    use crate::vmc::{BonePose, FrameAssembler, VmcFrame};
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

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

    /// Monotone key for ulp distances between f32
    /// values (signed zero keys equal +0.0's).
    fn ulp_key(value: f32) -> i64 {
        let bits = value.to_bits() as i32;
        i64::from(if bits < 0 { i32::MIN - bits } else { bits })
    }

    fn ulp_diff(a: f32, b: f32) -> u64 {
        (ulp_key(a) - ulp_key(b)).unsigned_abs()
    }

    /// D-538 gate, part 1: the frozen f64 atan2/asin,
    /// cast to f32, stay within 1 ulp of this platform's
    /// libm across a dense sweep of both domains —
    /// ≥200k points, the asin endpoints and near-endpoints,
    /// and atan2's axis-aligned, near-axis, and
    /// signed-zero cases included.
    #[test]
    fn det_trig_dense_sweep_stays_within_one_ulp_of_libm() {
        let mut points = 0_u64;
        let mut max_asin_ulp = 0_u64;
        for i in 0..=100_000_u32 {
            let x = (-1.0 + 2.0 * f64::from(i) / 100_000.0) as f32;
            let diff = ulp_diff(asin_det(f64::from(x)) as f32, x.asin());
            max_asin_ulp = max_asin_ulp.max(diff);
            points += 1;
        }
        for x in [-1.0_f32, -0.99999994, -0.5, -0.0, 0.0, 0.5, 0.99999994, 1.0] {
            let diff = ulp_diff(asin_det(f64::from(x)) as f32, x.asin());
            max_asin_ulp = max_asin_ulp.max(diff);
            points += 1;
        }
        // Both sides see the SAME inputs: f32 values,
        // promoted to f64 for the frozen functions (the
        // exact seam `joint_angles` uses). Feeding the
        // f64 side unrounded grid values instead would
        // measure input rounding, not the mapping.
        let mut max_atan2_ulp = 0_u64;
        for iy in -160..=160_i32 {
            for ix in -160..=160_i32 {
                let (y, x) = (iy as f32 / 160.0, ix as f32 / 160.0);
                let diff = ulp_diff(atan2_det(f64::from(y), f64::from(x)) as f32, y.atan2(x));
                max_atan2_ulp = max_atan2_ulp.max(diff);
                points += 1;
            }
        }
        // Axis-aligned, near-axis, signed-zero, subnormal,
        // and infinite cases.
        for &(y, x) in &[
            (1.0_f32, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (1.0, -0.0),
            (-1.0, -0.0),
            (0.0, 0.0),
            (-0.0, 0.0),
            (0.0, -0.0),
            (-0.0, -0.0),
            (f32::MIN_POSITIVE, 1.0),
            (-f32::MIN_POSITIVE, -1.0),
            (1.0, f32::MIN_POSITIVE),
            (f32::from_bits(1), -1.0),
            (1.0, f32::from_bits(1)),
            (f32::INFINITY, 1.0),
            (1.0, f32::INFINITY),
            (f32::INFINITY, f32::INFINITY),
            (f32::INFINITY, -0.0),
            (-f32::INFINITY, -f32::INFINITY),
        ] {
            let diff = ulp_diff(atan2_det(f64::from(y), f64::from(x)) as f32, y.atan2(x));
            max_atan2_ulp = max_atan2_ulp.max(diff);
            points += 1;
        }
        eprintln!(
            "D-538 sweep: {points} points, atan2 max {max_atan2_ulp} ulp, asin max {max_asin_ulp} ulp"
        );
        assert!(
            points >= 200_000,
            "the sweep covers at least 200k points ({points})"
        );
        assert!(
            max_atan2_ulp <= 1,
            "atan2_det stays within 1 f32 ulp of libm (max {max_atan2_ulp})"
        );
        assert!(
            max_asin_ulp <= 1,
            "asin_det stays within 1 f32 ulp of libm (max {max_asin_ulp})"
        );
    }

    /// The 120 head quaternions of the T3 diagnosis
    /// fixture (D-521), as f32 bit patterns (x, y, z, w).
    /// Embedded from the diagnosis directory's
    /// `quats-sandbox.txt` so the pinned-hash gate below
    /// runs anywhere; the env-gated test after it
    /// re-reads the on-disk fixture and both platforms'
    /// recordings.
    const FIXTURE_QUAT_BITS: [[u32; 4]; 120] = [
        [0x00000000, 0x00000000, 0x00000000, 0x3f800000],
        [0x00000000, 0x3cd6710b, 0x00000000, 0x3f7fe98b],
        [0x00000000, 0x3d565e3b, 0x00000000, 0x3f7fa62f],
        [0x00000000, 0x3da0af2a, 0x00000000, 0x3f7f35f9],
        [0x00000000, 0x3dd61305, 0x00000000, 0x3f7e98fd],
        [0x00000000, 0x3e05a8a9, 0x00000000, 0x3f7dcf55],
        [0x00000000, 0x3e20305c, 0x00000000, 0x3f7cd925],
        [0x00000000, 0x3e3a9bf3, 0x00000000, 0x3f7bb698],
        [0x00000000, 0x3e54e6ce, 0x00000000, 0x3f7a67e2],
        [0x00000000, 0x3e6f0c4d, 0x00000000, 0x3f78ed3c],
        [0x00000000, 0x3e8483ee, 0x00000000, 0x3f7746ea],
        [0x00000000, 0x3e916a76, 0x00000000, 0x3f757536],
        [0x00000000, 0x3e9e377a, 0x00000000, 0x3f737871],
        [0x00000000, 0x3eaae8bc, 0x00000000, 0x3f7150f4],
        [0x00000000, 0x3eb77c01, 0x00000000, 0x3f6eff20],
        [0x00000000, 0x3ec3ef16, 0x00000000, 0x3f6c835e],
        [0x00000000, 0x3ed03fc9, 0x00000000, 0x3f69de1d],
        [0x00000000, 0x3edc6bf3, 0x00000000, 0x3f670fd4],
        [0x00000000, 0x3ee87171, 0x00000000, 0x3f641901],
        [0x00000000, 0x3ef44e27, 0x00000000, 0x3f60fa29],
        [0x00000000, 0x3f000000, 0x00000000, 0x3f5db3d7],
        [0x00000000, 0x3f05c277, 0x00000000, 0x3f5a46a0],
        [0x00000000, 0x3f0b6d77, 0x00000000, 0x3f56b31d],
        [0x00000000, 0x3f110000, 0x00000000, 0x3f52f9ef],
        [0x00000000, 0x3f167918, 0x00000000, 0x3f4f1bbd],
        [0x00000000, 0x3f1bd7ca, 0x00000000, 0x3f4b1934],
        [0x00000000, 0x3f211b24, 0x00000000, 0x3f46f309],
        [0x00000000, 0x3f26423a, 0x00000000, 0x3f42a9f7],
        [0x00000000, 0x3f2b4c25, 0x00000000, 0x3f3e3ebd],
        [0x00000000, 0x3f303801, 0x00000000, 0x3f39b223],
        [0x00000000, 0x3f3504f3, 0x00000000, 0x3f3504f3],
        [0x00000000, 0x3f39b222, 0x00000000, 0x3f303802],
        [0x00000000, 0x3f3e3ebe, 0x00000000, 0x3f2b4c24],
        [0x00000000, 0x3f42a9f7, 0x00000000, 0x3f26423a],
        [0x00000000, 0x3f46f309, 0x00000000, 0x3f211b25],
        [0x00000000, 0x3f4b1934, 0x00000000, 0x3f1bd7ca],
        [0x00000000, 0x3f4f1bbd, 0x00000000, 0x3f167919],
        [0x00000000, 0x3f52f9ef, 0x00000000, 0x3f110000],
        [0x00000000, 0x3f56b31d, 0x00000000, 0x3f0b6d77],
        [0x00000000, 0x3f5a46a0, 0x00000000, 0x3f05c278],
        [0x00000000, 0x3f5db3d8, 0x00000000, 0x3effffff],
        [0x00000000, 0x3f60fa29, 0x00000000, 0x3ef44e27],
        [0x00000000, 0x3f641901, 0x00000000, 0x3ee87172],
        [0x00000000, 0x3f670fd4, 0x00000000, 0x3edc6bf5],
        [0x00000000, 0x3f69de1e, 0x00000000, 0x3ed03fc8],
        [0x00000000, 0x3f6c835e, 0x00000000, 0x3ec3ef15],
        [0x00000000, 0x3f6eff20, 0x00000000, 0x3eb77c02],
        [0x00000000, 0x3f7150f4, 0x00000000, 0x3eaae8bd],
        [0x00000000, 0x3f737871, 0x00000000, 0x3e9e3779],
        [0x00000000, 0x3f757536, 0x00000000, 0x3e916a75],
        [0x00000000, 0x3f7746ea, 0x00000000, 0x3e8483ef],
        [0x00000000, 0x3f78ed3c, 0x00000000, 0x3e6f0c51],
        [0x00000000, 0x3f7a67e2, 0x00000000, 0x3e54e6cb],
        [0x00000000, 0x3f7bb698, 0x00000000, 0x3e3a9bf3],
        [0x00000000, 0x3f7cd925, 0x00000000, 0x3e20305d],
        [0x00000000, 0x3f7dcf54, 0x00000000, 0x3e05a8ac],
        [0x00000000, 0x3f7e98fd, 0x00000000, 0x3dd612ff],
        [0x00000000, 0x3f7f35f9, 0x00000000, 0x3da0af28],
        [0x00000000, 0x3f7fa62f, 0x00000000, 0x3d565e3f],
        [0x00000000, 0x3f7fe98b, 0x00000000, 0x3cd67123],
        [0x00000000, 0x3f800000, 0x00000000, 0xb33bbd2e],
        [0x00000000, 0x3f7fe98b, 0x00000000, 0xbcd67112],
        [0x00000000, 0x3f7fa62f, 0x00000000, 0xbd565e36],
        [0x00000000, 0x3f7f35f9, 0x00000000, 0xbda0af24],
        [0x00000000, 0x3f7e98fd, 0x00000000, 0xbdd6130b],
        [0x00000000, 0x3f7dcf55, 0x00000000, 0xbe05a8aa],
        [0x00000000, 0x3f7cd925, 0x00000000, 0xbe20305a],
        [0x00000000, 0x3f7bb698, 0x00000000, 0xbe3a9bf0],
        [0x00000000, 0x3f7a67e2, 0x00000000, 0xbe54e6c9],
        [0x00000000, 0x3f78ed3c, 0x00000000, 0xbe6f0c4f],
        [0x00000000, 0x3f7746ea, 0x00000000, 0xbe8483ee],
        [0x00000000, 0x3f757536, 0x00000000, 0xbe916a74],
        [0x00000000, 0x3f737871, 0x00000000, 0xbe9e3778],
        [0x00000000, 0x3f7150f4, 0x00000000, 0xbeaae8bc],
        [0x00000000, 0x3f6eff21, 0x00000000, 0xbeb77c01],
        [0x00000000, 0x3f6c835f, 0x00000000, 0xbec3ef14],
        [0x00000000, 0x3f69de1e, 0x00000000, 0xbed03fc7],
        [0x00000000, 0x3f670fd4, 0x00000000, 0xbedc6bf4],
        [0x00000000, 0x3f641902, 0x00000000, 0xbee8716e],
        [0x00000000, 0x3f60fa29, 0x00000000, 0xbef44e26],
        [0x00000000, 0x3f5db3d7, 0x00000000, 0xbf000001],
        [0x00000000, 0x3f5a46a1, 0x00000000, 0xbf05c276],
        [0x00000000, 0x3f56b31d, 0x00000000, 0xbf0b6d77],
        [0x00000000, 0x3f52f9ee, 0x00000000, 0xbf110001],
        [0x00000000, 0x3f4f1bbd, 0x00000000, 0xbf167917],
        [0x00000000, 0x3f4b1934, 0x00000000, 0xbf1bd7ca],
        [0x00000000, 0x3f46f30b, 0x00000000, 0xbf211b23],
        [0x00000000, 0x3f42a9f7, 0x00000000, 0xbf26423a],
        [0x00000000, 0x3f3e3ebd, 0x00000000, 0xbf2b4c26],
        [0x00000000, 0x3f39b223, 0x00000000, 0xbf303800],
        [0x00000000, 0x3f3504f3, 0x00000000, 0xbf3504f3],
        [0x00000000, 0x3f303800, 0x00000000, 0xbf39b224],
        [0x00000000, 0x3f2b4c25, 0x00000000, 0xbf3e3ebd],
        [0x00000000, 0x3f26423a, 0x00000000, 0xbf42a9f8],
        [0x00000000, 0x3f211b26, 0x00000000, 0xbf46f308],
        [0x00000000, 0x3f1bd7ca, 0x00000000, 0xbf4b1934],
        [0x00000000, 0x3f167917, 0x00000000, 0xbf4f1bbe],
        [0x00000000, 0x3f110001, 0x00000000, 0xbf52f9ee],
        [0x00000000, 0x3f0b6d77, 0x00000000, 0xbf56b31d],
        [0x00000000, 0x3f05c279, 0x00000000, 0xbf5a469f],
        [0x00000000, 0x3f000001, 0x00000000, 0xbf5db3d7],
        [0x00000000, 0x3ef44e26, 0x00000000, 0xbf60fa29],
        [0x00000000, 0x3ee87174, 0x00000000, 0xbf641900],
        [0x00000000, 0x3edc6bf4, 0x00000000, 0xbf670fd4],
        [0x00000000, 0x3ed03fc7, 0x00000000, 0xbf69de1e],
        [0x00000000, 0x3ec3ef17, 0x00000000, 0xbf6c835e],
        [0x00000000, 0x3eb77c00, 0x00000000, 0xbf6eff21],
        [0x00000000, 0x3eaae8c0, 0x00000000, 0xbf7150f3],
        [0x00000000, 0x3e9e377b, 0x00000000, 0xbf737870],
        [0x00000000, 0x3e916a74, 0x00000000, 0xbf757536],
        [0x00000000, 0x3e8483f1, 0x00000000, 0xbf7746ea],
        [0x00000000, 0x3e6f0c4e, 0x00000000, 0xbf78ed3c],
        [0x00000000, 0x3e54e6c8, 0x00000000, 0xbf7a67e2],
        [0x00000000, 0x3e3a9bf8, 0x00000000, 0xbf7bb698],
        [0x00000000, 0x3e20305a, 0x00000000, 0xbf7cd925],
        [0x00000000, 0x3e05a8b1, 0x00000000, 0xbf7dcf54],
        [0x00000000, 0x3dd61309, 0x00000000, 0xbf7e98fd],
        [0x00000000, 0x3da0af22, 0x00000000, 0xbf7f35fa],
        [0x00000000, 0x3d565e53, 0x00000000, 0xbf7fa62f],
        [0x00000000, 0x3cd6710c, 0x00000000, 0xbf7fe98b],
    ];

    fn fixture_pose(bits: &[u32; 4]) -> BonePose {
        BonePose {
            name: "Head".to_string(),
            position: [0.0; 3],
            rotation: [
                f32::from_bits(bits[0]),
                f32::from_bits(bits[1]),
                f32::from_bits(bits[2]),
                f32::from_bits(bits[3]),
            ],
        }
    }

    /// D-538 gate, part 2: the pinned SHA-256 of the
    /// canonical serialization (per frame, in order:
    /// yaw, pitch, roll as little-endian f32 bit
    /// patterns) of `joint_angles` over the fixture
    /// quaternions. This hash is the cross-machine
    /// recording gate: any platform whose libm differs
    /// must still produce it, because libm is no longer
    /// consulted.
    #[test]
    fn joint_angles_fixture_hash_is_pinned() {
        let mut hasher = Sha256::new();
        for bits in &FIXTURE_QUAT_BITS {
            let angles = joint_angles(&fixture_pose(bits));
            for value in [angles.yaw, angles.pitch, angles.roll] {
                hasher.update(value.to_bits().to_le_bytes());
            }
        }
        assert_eq!(
            format!("{:x}", hasher.finalize()),
            "439118c1d3d0ecc0ad09974a5f0855cba8837680a8cf0e1cbee714061e12b015",
            "the frozen mapping's fixture hash (D-538)"
        );
    }

    /// D-538 gate, part 3 (env-gated on
    /// `TOROGAO_T3_DIAG_DIR`, the diagnosis directory):
    /// the frozen mapping lands within 1 f32 ulp of BOTH
    /// platforms' recorded `joint_angles` outputs (the
    /// `map-sandbox.txt` / `map-primo.txt` dumps — the
    /// production mapping's own recordings, parsed
    /// instead of the take JSONs, whose head eulers are
    /// the same values one indirection further away),
    /// on 119 of the 120 fixture frames per platform —
    /// with exactly one named exception per platform,
    /// where the platform's own recording is 2 ulp from
    /// the true value and the frozen value is the
    /// correctly rounded one (both verified against a
    /// 50-digit reference at implementation time):
    /// frame 6 yaw, sandbox recorded 0x3dcccccf against
    /// the true 0x3dcccccd (primo: 0x3dccccce), and
    /// frame 108 yaw, primo recorded 0xbe4cccd1 against
    /// the true 0xbe4ccccf (sandbox: exact). No frozen
    /// mapping can sit within 1 ulp of both recordings
    /// on both frames; the exceptions are the recordings'
    /// own f32-chain error, not the frozen mapping's.
    #[test]
    fn joint_angles_match_both_platform_recordings_within_one_ulp() {
        let Some(dir) = std::env::var_os("TOROGAO_T3_DIAG_DIR") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let quats = std::fs::read_to_string(dir.join("quats-sandbox.txt"))
            .expect("the diagnosis quaternions read");
        for (line, bits) in quats.lines().zip(FIXTURE_QUAT_BITS.iter()) {
            let parsed: Vec<u32> = line
                .split_whitespace()
                .map(|field| u32::from_str_radix(field, 16).expect("hex bit pattern"))
                .collect();
            assert_eq!(
                parsed.as_slice(),
                bits.as_slice(),
                "the embedded table matches the on-disk fixture"
            );
        }
        // The one named exception per platform (frame
        // index, axis index 0 = yaw, ulp diff): see the
        // test's doc comment.
        for (file, exception) in [
            ("map-sandbox.txt", (6_usize, 0_usize, 2_u64)),
            ("map-primo.txt", (108, 0, 2)),
        ] {
            let text = std::fs::read_to_string(dir.join(file)).expect("the map dump reads");
            let mut lines = text.lines();
            let mut within = 0_usize;
            let mut exceptions: Vec<(usize, usize, u64)> = Vec::new();
            for (index, bits) in FIXTURE_QUAT_BITS.iter().enumerate() {
                let line = lines.next().expect("one map line per fixture quaternion");
                let fields: Vec<&str> = line.split_whitespace().collect();
                assert_eq!(fields.len(), 8, "map line shape: {line}");
                let angles = joint_angles(&fixture_pose(bits));
                let mut frame_within = true;
                for (axis, (actual, field)) in [angles.yaw, angles.pitch, angles.roll]
                    .into_iter()
                    .zip([3, 5, 7])
                    .enumerate()
                {
                    let recorded =
                        f32::from_bits(u32::from_str_radix(fields[field], 16).expect("hex"));
                    let diff = ulp_diff(actual, recorded);
                    if diff > 1 {
                        exceptions.push((index, axis, diff));
                        frame_within = false;
                    }
                }
                if frame_within {
                    within += 1;
                }
            }
            eprintln!("D-538 fixture vs {file}: {within}/120 frames within 1 ulp");
            assert_eq!(within, 119, "119 of 120 frames within 1 ulp of {file}");
            assert_eq!(
                exceptions,
                [exception],
                "the only exception against {file} is the named one"
            );
        }
    }

    #[test]
    fn raw_blendshape_frame_uses_the_channel_table_order() {
        let mut frame = VmcFrame::default();
        frame.blends.push(("cheekSquintLeft".to_string(), 0.73));
        frame.blends.push(("eyeBlinkLeft".to_string(), 0.25));
        frame.blends.push(("tongueOut".to_string(), 0.5));
        let raw = raw_blendshape_frame(&frame);
        let slot = |name: &str| {
            ARKIT_BLENDSHAPES
                .iter()
                .position(|channel| *channel == name)
                .expect("channel is in the table")
        };
        assert_eq!(raw[slot("cheekSquintLeft")], 0.73);
        assert_eq!(raw[slot("eyeBlinkLeft")], 0.25);
        assert_eq!(raw[slot("tongueOut")], 0.5);
        assert_eq!(raw[slot("jawOpen")], 0.0, "an uncarried channel reads 0.0");
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
