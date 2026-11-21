//! The crate's one error type.

use std::fmt;

/// Why a solve was refused. Each variant names the first input
/// that failed, in the order the function documents its checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SceneError {
    /// An input number was NaN or infinite.
    NonFinite { field: &'static str },
    /// A bone length was outside `[LENGTH_MIN_M, LENGTH_MAX_M]`.
    LengthOutOfRange { field: &'static str, length_m: f64 },
    /// The target sits on the limb's root: no reach direction.
    TargetOnRoot { distance_m: f64 },
    /// The pole points along the reach axis: the bend plane is
    /// undefined.
    PoleAlongAxis { off_axis: f64 },
    /// A barycentric weight was outside `[0, 1]`, or the three did
    /// not sum to one.
    BadBarycentric { sum: f64 },
    /// The anchor triangle has (almost) no area: no normal.
    DegenerateTriangle { area_m2: f64 },
    /// The anchor offset was larger than `OFFSET_MAX_M`.
    OffsetOutOfRange { offset_m: f64 },
    /// A curl was asked for with no finger segments, or too many.
    BadChain { segments: usize },
    /// The surface radius was below `RADIUS_MIN_M` or above
    /// `RADIUS_MAX_M`, or not larger than the press depth.
    RadiusOutOfRange { radius_m: f64 },
    /// The press depth (either way) was not less than the radius.
    PressOutOfRange { press_m: f64 },
    /// The key's baked depth was zero or negative.
    BakedDepthNotPositive { baked_m: f64 },
    /// A section piece was too short, or there were too many (the
    /// index of the first bad piece, or the count).
    BadSection { piece: usize },
    /// Joint limits missing, or `lo >= hi`, or out of range (the
    /// first bad joint).
    BadLimits { joint: usize },
    /// The capsule clearance was not in `(0, CLEARANCE_MAX_M]`.
    ClearanceOutOfRange { clearance_m: f64 },
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NonFinite { field } => write!(f, "{field} is not a finite number"),
            Self::LengthOutOfRange { field, length_m } => {
                write!(
                    f,
                    "{field} = {length_m} m is outside the allowed bone length range"
                )
            }
            Self::TargetOnRoot { distance_m } => {
                write!(
                    f,
                    "target is {distance_m} m from the root: no reach direction"
                )
            }
            Self::PoleAlongAxis { off_axis } => {
                write!(f, "pole is along the reach axis (off-axis part {off_axis})")
            }
            Self::BadBarycentric { sum } => {
                write!(
                    f,
                    "barycentric weights are outside [0, 1] or sum to {sum}, not 1"
                )
            }
            Self::DegenerateTriangle { area_m2 } => {
                write!(
                    f,
                    "anchor triangle area {area_m2} m^2 is too small for a normal"
                )
            }
            Self::OffsetOutOfRange { offset_m } => {
                write!(f, "anchor offset {offset_m} m is outside the allowed range")
            }
            Self::BadChain { segments } => {
                write!(f, "a finger chain needs 1 to 8 segments, got {segments}")
            }
            Self::RadiusOutOfRange { radius_m } => {
                write!(
                    f,
                    "surface radius {radius_m} m is outside the allowed range"
                )
            }
            Self::PressOutOfRange { press_m } => {
                write!(
                    f,
                    "press depth {press_m} m is negative or not under the radius"
                )
            }
            Self::BakedDepthNotPositive { baked_m } => {
                write!(f, "baked press depth {baked_m} m must be above zero")
            }
            Self::BadSection { piece } => {
                write!(
                    f,
                    "section piece {piece} is too short, or there are too many"
                )
            }
            Self::BadLimits { joint } => {
                write!(f, "joint {joint} has missing or bad limits")
            }
            Self::ClearanceOutOfRange { clearance_m } => {
                write!(f, "clearance {clearance_m} m is outside the allowed range")
            }
        }
    }
}

impl std::error::Error for SceneError {}
