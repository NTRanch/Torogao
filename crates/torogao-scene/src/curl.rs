//! Closed-form finger curl: lay a finger's segments on a curved
//! surface.
//!
//! Seen in the finger's own bend plane, the surface near the touch
//! is a circle of radius `radius_m` about the origin. The palm is a
//! straight line pressed `press_m` into it, at height
//! `-(radius_m - press_m)` (a negative press: the line stands clear
//! of the circle, as it does beside the palm's centre line); the palm
//! runs along +x from the touch point. The finger starts on the palm line, `root_m` along +x.
//! Each segment turns toward the palm's facing side by the least
//! angle at which it first touches the circle as it curls: either
//! its far end lands on the circle (two circles meet) or, from a
//! start outside the circle, its side comes to rest tangent to it.
//! Both are closed forms (one square root or one arcsine each, no
//! search). A segment that starts on the circle follows it: its far
//! end goes to the next point round the circle (a chord), even when
//! that means a turn back. A segment that cannot touch the circle by
//! curling forward stays straight. Joint angles are the turns
//! between one segment's direction and the next, the first against
//! the palm; positive turns toward the palm's facing side (+y).

use crate::SceneError;

/// Smallest and largest surface radius accepted, metres.
pub const RADIUS_MIN_M: f64 = 0.005;
pub const RADIUS_MAX_M: f64 = 10.0;
/// Longest chain accepted.
pub const SEGMENTS_MAX: usize = 8;
/// A point this close to the circle (share of the radius) is on it.
const ON_CIRCLE_SHARE: f64 = 1.0e-9;

/// One finger's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Curl {
    /// Turn at each joint, radians, segment by segment.
    pub joint_rad: Vec<f64>,
    /// Far end of each segment, bend-plane metres (circle centre at
    /// the origin).
    pub ends: Vec<(f64, f64)>,
    /// For each segment, whether its far end reached the circle.
    pub on_surface: Vec<bool>,
}

/// Curls one finger.
///
/// Checks, in order: every number finite; 1 to `SEGMENTS_MAX`
/// segments, each in the two-bone length range; radius in
/// `[RADIUS_MIN_M, RADIUS_MAX_M]`; press depth in `(-radius, radius)`.
///
/// # Errors
/// The first failed check, as a [`SceneError`].
pub fn wrap(
    radius_m: f64,
    press_m: f64,
    root_m: f64,
    segments_m: &[f64],
) -> Result<Curl, SceneError> {
    for (field, x) in [
        ("radius_m", radius_m),
        ("press_m", press_m),
        ("root_m", root_m),
    ] {
        if !x.is_finite() {
            return Err(SceneError::NonFinite { field });
        }
    }
    if segments_m.iter().any(|l| !l.is_finite()) {
        return Err(SceneError::NonFinite {
            field: "segments_m",
        });
    }
    if segments_m.is_empty() || segments_m.len() > SEGMENTS_MAX {
        return Err(SceneError::BadChain {
            segments: segments_m.len(),
        });
    }
    for &l in segments_m {
        if !(crate::two_bone::LENGTH_MIN_M..=crate::two_bone::LENGTH_MAX_M).contains(&l) {
            return Err(SceneError::LengthOutOfRange {
                field: "segments_m",
                length_m: l,
            });
        }
    }
    if !(RADIUS_MIN_M..=RADIUS_MAX_M).contains(&radius_m) {
        return Err(SceneError::RadiusOutOfRange { radius_m });
    }
    if press_m.abs() >= radius_m {
        return Err(SceneError::PressOutOfRange { press_m });
    }
    let mut p = (root_m, -(radius_m - press_m));
    let mut heading = 0.0_f64; // the palm runs along +x
    let mut out = Curl {
        joint_rad: Vec::new(),
        ends: Vec::new(),
        on_surface: Vec::new(),
    };
    for &l in segments_m {
        let on_circle = (p.0.hypot(p.1) - radius_m).abs() <= ON_CIRCLE_SHARE * radius_m;
        let turn = if on_circle {
            // the chord to the next point counter-clockwise round the circle
            meet(radius_m, p, l).map(|(q1, q2)| {
                let q = if p.0 * q1.1 - p.1 * q1.0 > 0.0 {
                    q1
                } else {
                    q2
                };
                wrap_pi((q.1 - p.1).atan2(q.0 - p.0) - heading)
            })
        } else {
            let mut cands: Vec<f64> = Vec::new();
            if let Some((q1, q2)) = meet(radius_m, p, l) {
                cands.extend([q1, q2].iter().map(|q| (q.1 - p.1).atan2(q.0 - p.0)));
            }
            cands.extend(tangent_headings(radius_m, p, l));
            cands
                .into_iter()
                .map(|h| wrap_pi(h - heading))
                .filter(|t| *t >= 0.0)
                .reduce(f64::min)
        };
        let (end, hit) = match turn {
            Some(t) => {
                let h = heading + t;
                ((p.0 + l * h.cos(), p.1 + l * h.sin()), true)
            }
            None => ((p.0 + l * heading.cos(), p.1 + l * heading.sin()), false),
        };
        let dir = (end.1 - p.1).atan2(end.0 - p.0);
        out.joint_rad.push(wrap_pi(dir - heading));
        out.ends.push(end);
        out.on_surface.push(hit);
        heading = dir;
        p = end;
    }
    Ok(out)
}

/// The two points of the circle |X| = r at distance `l` from `p`,
/// if the two circles meet.
fn meet(r: f64, p: (f64, f64), l: f64) -> Option<((f64, f64), (f64, f64))> {
    let d = p.0.hypot(p.1);
    if d < 1.0e-12 || l > r + d || l < (r - d).abs() {
        return None;
    }
    let a = (r * r - l * l + d * d) / (2.0 * d); // along O->P from O
    let h = (r * r - a * a).max(0.0).sqrt();
    let (ux, uy) = (p.0 / d, p.1 / d);
    Some((
        (ux * a - uy * h, uy * a + ux * h),
        (ux * a + uy * h, uy * a - ux * h),
    ))
}

/// Headings from `p` (outside the circle) at which a segment of
/// length `l` lies tangent to the circle with the touching point on
/// the segment.
fn tangent_headings(r: f64, p: (f64, f64), l: f64) -> Vec<f64> {
    let d = p.0.hypot(p.1);
    if d <= r * (1.0 + ON_CIRCLE_SHARE) {
        return Vec::new(); // on or inside the circle: the touch is the far end landing
    }
    let phi = p.1.atan2(p.0);
    let mut out = Vec::new();
    for s in [-1.0_f64, 1.0] {
        let a = (s * r / d).asin();
        for h in [phi + a, phi + std::f64::consts::PI - a] {
            let foot = -(p.0 * h.cos() + p.1 * h.sin()); // where the perpendicular from O lands
            if foot > 1.0e-12 && foot <= l {
                out.push(h);
            }
        }
    }
    out
}

fn wrap_pi(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    let x = (a + std::f64::consts::PI).rem_euclid(t) - std::f64::consts::PI;
    if x == -std::f64::consts::PI {
        std::f64::consts::PI
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- validation ----
    /// Least distance from the circle centre to the segment p-q.
    fn closest(p: (f64, f64), q: (f64, f64)) -> f64 {
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let t = (-(p.0 * dx + p.1 * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        (p.0 + dx * t).hypot(p.1 + dy * t)
    }

    #[test]
    fn every_segment_touches_the_circle_keeps_its_length_and_curls_toward_the_palm() {
        let segs = [0.042, 0.028, 0.028];
        let c = wrap(0.060, 0.006, 0.040, &segs).unwrap();
        let mut p = (0.040, -(0.060 - 0.006));
        for (i, &(x, y)) in c.ends.iter().enumerate() {
            assert!(c.on_surface[i]);
            assert!(
                (closest(p, (x, y)) - 0.060).abs() < 1e-9,
                "segment {i} does not touch"
            );
            assert!(
                ((x - p.0).hypot(y - p.1) - segs[i]).abs() < 1e-12,
                "segment {i} length"
            );
            p = (x, y);
        }
        assert!(
            c.joint_rad[0] > 0.0,
            "the first joint turns toward the palm"
        );
    }

    #[test]
    fn from_a_root_on_the_circle_later_joints_follow_the_chord_rule() {
        // root where the pressed palm line crosses the circle; then every segment is a chord, and
        // between chords of lengths a and b on a circle of radius r the turn is asin(a/2r) + asin(b/2r)
        let (r, d, segs): (f64, f64, [f64; 3]) = (0.05, 0.004, [0.03, 0.02, 0.02]);
        let root = (r * r - (r - d) * (r - d)).sqrt();
        let c = wrap(r, d, root, &segs).unwrap();
        for (k, &(x, y)) in c.ends.iter().enumerate() {
            assert!((x.hypot(y) - r).abs() < 1e-12, "end {k} off the circle");
        }
        for k in 1..segs.len() {
            let want = (segs[k - 1] / (2.0 * r)).asin() + (segs[k] / (2.0 * r)).asin();
            assert!((c.joint_rad[k] - want).abs() < 1e-12, "joint {k}");
        }
    }

    #[test]
    fn a_nearly_flat_surface_leaves_the_finger_nearly_straight() {
        let c = wrap(10.0, 0.0, 0.04, &[0.04, 0.03, 0.03]).unwrap();
        assert!(
            c.joint_rad.iter().all(|a| a.abs() < 0.02),
            "{:?}",
            c.joint_rad
        );
    }

    #[test]
    fn the_same_input_gives_the_same_bits() {
        let a = wrap(0.061, 0.006, 0.041, &[0.042, 0.028, 0.028]).unwrap();
        let b = wrap(0.061, 0.006, 0.041, &[0.042, 0.028, 0.028]).unwrap();
        assert_eq!(a, b);
    }

    // ---- adversarial ----
    #[test]
    fn a_root_outside_the_curve_never_cuts_deeper_than_a_chord() {
        // the case met on maren-v2d: the palm is longer than the curve is wide. Before the
        // first-touch rule a finger dove through the curve (27-30 mm on the ring and little
        // fingers); now a segment goes in no deeper than a chord's sagitta, r - sqrt(r^2 - l^2/4).
        let segs = [0.042, 0.028, 0.021];
        for &(r, root) in &[(0.045, 0.046), (0.0294, 0.032), (0.06, 0.08)] {
            let c = wrap(r, 0.006, root, &segs).unwrap();
            let mut p = (root, -(r - 0.006));
            for (i, &(x, y)) in c.ends.iter().enumerate() {
                let sagitta = r - (r * r - segs[i] * segs[i] / 4.0).sqrt();
                assert!(
                    closest(p, (x, y)) >= r - sagitta - 1e-9,
                    "segment {i} cuts {} m into the curve (r {r}, sagitta {sagitta})",
                    r - closest(p, (x, y))
                );
                p = (x, y);
            }
        }
    }

    #[test]
    fn a_segment_that_cannot_reach_the_surface_stays_straight_and_says_so() {
        // root deep inside a large circle, short segment: no meeting point
        let c = wrap(1.0, 0.5, 0.0, &[0.01]).unwrap();
        assert!(!c.on_surface[0]);
        assert_eq!(c.joint_rad[0], 0.0);
    }

    #[test]
    fn bad_numbers_and_chains_are_refused_by_name() {
        assert_eq!(
            wrap(f64::NAN, 0.0, 0.0, &[0.02]),
            Err(SceneError::NonFinite { field: "radius_m" })
        );
        assert_eq!(
            wrap(0.05, 0.0, 0.0, &[f64::INFINITY]),
            Err(SceneError::NonFinite {
                field: "segments_m"
            })
        );
        assert_eq!(
            wrap(0.05, 0.0, 0.0, &[]),
            Err(SceneError::BadChain { segments: 0 })
        );
        assert_eq!(
            wrap(0.05, 0.0, 0.0, &[0.01; 9]),
            Err(SceneError::BadChain { segments: 9 })
        );
        assert!(matches!(
            wrap(0.05, 0.0, 0.0, &[0.0]),
            Err(SceneError::LengthOutOfRange { .. })
        ));
    }

    #[test]
    fn radius_and_press_outside_their_range_are_refused() {
        assert_eq!(
            wrap(0.001, 0.0, 0.0, &[0.02]),
            Err(SceneError::RadiusOutOfRange { radius_m: 0.001 })
        );
        assert_eq!(
            wrap(0.05, -0.05, 0.0, &[0.02]),
            Err(SceneError::PressOutOfRange { press_m: -0.05 })
        );
        assert_eq!(
            wrap(0.05, 0.05, 0.0, &[0.02]),
            Err(SceneError::PressOutOfRange { press_m: 0.05 })
        );
    }
}
