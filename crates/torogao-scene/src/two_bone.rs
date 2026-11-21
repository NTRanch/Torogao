//! Closed-form two-bone reach (shoulder, elbow, wrist).
//!
//! The elbow lies on the circle where two spheres meet: one of
//! radius `upper_length_m` about the root, one of radius
//! `fore_length_m` about the (clamped) target. The pole picks the
//! point of that circle. Same construction as the estate's Python
//! `ik` (maren-milking-scene `build_keyframes.py`), with the
//! refusals that function does not make.

use crate::{SceneError, Vec3};

/// Shortest and longest bone this solve accepts, metres.
pub const LENGTH_MIN_M: f64 = 1.0e-4;
pub const LENGTH_MAX_M: f64 = 10.0;
/// A target further than this share of the full limb length is
/// pulled in to it, so the elbow never locks dead straight.
pub const REACH_MAX_SHARE: f64 = 0.999;
/// A target closer than `|upper - fore|` times this is pushed out
/// to it: nearer than that the limb cannot fold.
pub const REACH_MIN_SHARE: f64 = 1.001;
/// Below this the target is on the root, metres.
pub const TARGET_DISTANCE_MIN_M: f64 = 1.0e-6;
/// Below this (as a share of the pole's length) the pole is along
/// the reach axis.
pub const POLE_OFF_AXIS_MIN: f64 = 1.0e-6;
/// Bone lengths of the answer are asserted to this, metres.
const LENGTH_TOLERANCE_M: f64 = 1.0e-9;

/// What to solve. Positions in metres, one common space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoBoneInput {
    /// The fixed joint (shoulder).
    pub root: Vec3,
    pub upper_length_m: f64,
    pub fore_length_m: f64,
    /// Where the end joint (wrist) should go.
    pub target: Vec3,
    /// A direction: the middle joint bends toward it.
    pub pole: Vec3,
}

/// Where the joints go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoBoneSolution {
    /// The middle joint (elbow).
    pub mid: Vec3,
    /// The end joint (wrist): the target, unless clamped.
    pub end: Vec3,
    /// True when the target was out of reach and was clamped.
    pub reach_clamped: bool,
}

/// Solves the limb.
///
/// Checks, in order: every number finite; both lengths in
/// `[LENGTH_MIN_M, LENGTH_MAX_M]`; target at least
/// `TARGET_DISTANCE_MIN_M` from the root; pole not along the reach
/// axis. Work is constant: one square root, no loop.
///
/// # Errors
/// The first failed check, as a [`SceneError`].
pub fn solve(input: &TwoBoneInput) -> Result<TwoBoneSolution, SceneError> {
    check_finite(input)?;
    check_length("upper_length_m", input.upper_length_m)?;
    check_length("fore_length_m", input.fore_length_m)?;
    let reach = input.target - input.root;
    let distance_m = reach.length();
    if distance_m < TARGET_DISTANCE_MIN_M {
        return Err(SceneError::TargetOnRoot { distance_m });
    }
    let axis = reach * (1.0 / distance_m);
    let across = input.pole - axis * input.pole.dot(axis);
    let pole_length = input.pole.length();
    if pole_length <= 0.0 || across.length() / pole_length < POLE_OFF_AXIS_MIN {
        let off_axis = if pole_length > 0.0 {
            across.length() / pole_length
        } else {
            0.0
        };
        return Err(SceneError::PoleAlongAxis { off_axis });
    }
    let bend = across * (1.0 / across.length());

    let (upper, fore) = (input.upper_length_m, input.fore_length_m);
    let far = (upper + fore) * REACH_MAX_SHARE;
    let near = (upper - fore).abs() * REACH_MIN_SHARE;
    let clamped_m = distance_m.clamp(near.min(far), far);
    let along_m = (upper * upper - fore * fore + clamped_m * clamped_m) / (2.0 * clamped_m);
    let rise_m = (upper * upper - along_m * along_m).max(0.0).sqrt();

    let mid = input.root + axis * along_m + bend * rise_m;
    let end = input.root + axis * clamped_m;
    assert!(((mid - input.root).length() - upper).abs() < LENGTH_TOLERANCE_M);
    assert!(((end - mid).length() - fore).abs() < LENGTH_TOLERANCE_M);
    Ok(TwoBoneSolution {
        mid,
        end,
        reach_clamped: clamped_m != distance_m,
    })
}

fn check_finite(input: &TwoBoneInput) -> Result<(), SceneError> {
    let fields = [
        ("root", input.root.is_finite()),
        ("upper_length_m", input.upper_length_m.is_finite()),
        ("fore_length_m", input.fore_length_m.is_finite()),
        ("target", input.target.is_finite()),
        ("pole", input.pole.is_finite()),
    ];
    for (field, finite) in fields {
        if !finite {
            return Err(SceneError::NonFinite { field });
        }
    }
    Ok(())
}

fn check_length(field: &'static str, length_m: f64) -> Result<(), SceneError> {
    if (LENGTH_MIN_M..=LENGTH_MAX_M).contains(&length_m) {
        Ok(())
    } else {
        Err(SceneError::LengthOutOfRange { field, length_m })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(target: Vec3) -> TwoBoneInput {
        TwoBoneInput {
            root: Vec3::new(0.316, 0.0, 1.415),
            upper_length_m: 0.283,
            fore_length_m: 0.255,
            target,
            pole: Vec3::new(0.25, 0.15, -0.6),
        }
    }

    // ---- validation ----

    #[test]
    fn right_angle_limb_has_the_textbook_elbow() {
        // 3-4-5: bones 3 and 4, target 5 away, elbow at a right angle.
        let input = TwoBoneInput {
            root: Vec3::new(0.0, 0.0, 0.0),
            upper_length_m: 3.0,
            fore_length_m: 4.0,
            target: Vec3::new(5.0, 0.0, 0.0),
            pole: Vec3::new(0.0, 0.0, 1.0),
        };
        let got = solve(&input).unwrap();
        assert!((got.mid - Vec3::new(1.8, 0.0, 2.4)).length() < 1.0e-12);
        assert_eq!(got.end, Vec3::new(5.0, 0.0, 0.0));
        assert!(!got.reach_clamped);
    }

    #[test]
    fn reachable_target_is_hit_and_bone_lengths_hold() {
        let target = Vec3::new(0.14, -0.17, 1.15);
        let got = solve(&arm(target)).unwrap();
        assert!((got.end - target).length() < 1.0e-12);
        assert!(((got.mid - arm(target).root).length() - 0.283).abs() < 1.0e-9);
        assert!(((got.end - got.mid).length() - 0.255).abs() < 1.0e-9);
    }

    #[test]
    fn elbow_bends_toward_the_pole_and_the_answer_repeats_bit_for_bit() {
        let input = arm(Vec3::new(0.14, -0.17, 1.15));
        let first = solve(&input).unwrap();
        let second = solve(&input).unwrap();
        assert_eq!(first, second);
        let axis = input.target - input.root;
        let off =
            (first.mid - input.root) - axis * ((first.mid - input.root).dot(axis) / axis.dot(axis));
        assert!(off.dot(input.pole) > 0.0);
    }

    #[test]
    fn out_of_reach_target_is_clamped_along_the_same_line() {
        let got = solve(&arm(Vec3::new(3.316, 0.0, 1.415))).unwrap();
        assert!(got.reach_clamped);
        let reach = (got.end - Vec3::new(0.316, 0.0, 1.415)).length();
        assert!((reach - (0.283 + 0.255) * REACH_MAX_SHARE).abs() < 1.0e-12);
        assert!((got.end.z - 1.415).abs() < 1.0e-12 && got.end.y.abs() < 1.0e-12);
    }

    // ---- adversarial ----

    #[test]
    fn matches_the_harness_reach_on_maren_v2d() {
        // Shoulder, bone lengths, wrist target and pole as the P08 build used them, and the elbow
        // and wrist Blender reached with the harness's own reach (chars/maren-v2d/build_record.json,
        // contact.twobone_crosscheck). Blender works in 32-bit floats, hence 1e-6 m.
        let s = solve(&TwoBoneInput {
            root: Vec3::new(0.315_999_985, 0.0, 1.414_636_254),
            upper_length_m: 0.282_082_19,
            fore_length_m: 0.254_765_333,
            target: Vec3::new(0.079_594_672, -0.083_992_884, 1.192_894_697),
            pole: Vec3::new(1.0, 0.3, -0.4),
        })
        .unwrap();
        assert!(!s.reach_clamped);
        let elbow = Vec3::new(0.316_330_582, -0.011_542_715, 1.132_790_565);
        let wrist = Vec3::new(0.079_594_873, -0.083_992_854, 1.192_894_816);
        assert!(
            (s.mid - elbow).length() < 1e-6,
            "elbow off by {} m",
            (s.mid - elbow).length()
        );
        assert!(
            (s.end - wrist).length() < 1e-6,
            "wrist off by {} m",
            (s.end - wrist).length()
        );
    }

    #[test]
    fn non_finite_input_is_refused_by_name() {
        let mut input = arm(Vec3::new(0.14, -0.17, 1.15));
        input.target.y = f64::NAN;
        assert_eq!(
            solve(&input),
            Err(SceneError::NonFinite { field: "target" })
        );
        let mut input = arm(Vec3::new(0.14, -0.17, 1.15));
        input.fore_length_m = f64::INFINITY;
        assert_eq!(
            solve(&input),
            Err(SceneError::NonFinite {
                field: "fore_length_m"
            })
        );
    }

    #[test]
    fn zero_negative_and_huge_bones_are_refused() {
        for bad in [0.0, -0.3, 1.0e-5, 11.0] {
            let mut input = arm(Vec3::new(0.14, -0.17, 1.15));
            input.upper_length_m = bad;
            assert!(matches!(
                solve(&input),
                Err(SceneError::LengthOutOfRange {
                    field: "upper_length_m",
                    ..
                })
            ));
        }
    }

    #[test]
    fn target_on_the_root_is_refused() {
        let got = solve(&arm(Vec3::new(0.316, 0.0, 1.415)));
        assert!(matches!(got, Err(SceneError::TargetOnRoot { .. })));
    }

    #[test]
    fn pole_along_the_axis_or_zero_is_refused() {
        let mut input = arm(Vec3::new(0.316, 0.0, 1.0));
        input.pole = Vec3::new(0.0, 0.0, -2.0);
        assert!(matches!(
            solve(&input),
            Err(SceneError::PoleAlongAxis { .. })
        ));
        input.pole = Vec3::new(0.0, 0.0, 0.0);
        assert!(matches!(
            solve(&input),
            Err(SceneError::PoleAlongAxis { .. })
        ));
    }

    #[test]
    fn target_inside_the_fold_limit_is_pushed_out_not_broken() {
        // bones 0.283 and 0.255 cannot bring the wrist within 0.028 m of the shoulder
        let got = solve(&arm(Vec3::new(0.316, 0.0, 1.405))).unwrap();
        assert!(got.reach_clamped);
        assert!(((got.end - got.mid).length() - 0.255).abs() < 1.0e-9);
    }
}
