//! Finger curl against a real surface section (P08 option 1).
//!
//! Seen in a finger's own bend plane, the surface is a set of
//! straight pieces (the mesh cut by that plane). The finger root is
//! at the origin and the straight finger runs along +x; a positive
//! turn curls toward +y (the palm's facing side). Each segment is a
//! capsule: its bone line plus `clearance_m` (the pad's thickness
//! under the bone, less the wanted press).
//!
//! Each joint turns, inside its limits, by the least angle at which
//! the capsule first touches any piece. Between a turning segment
//! and a straight piece the distance first reaches the clearance in
//! one of three ways, each a closed form (no search, no iteration):
//! the tip lands on the piece's offset line, the tip lands on the
//! circle round a piece end, or the segment's side comes within the
//! clearance of a piece end. The least valid turn over all pieces is
//! the first touch. A segment already within the clearance at its
//! lower limit is held there and says so; a segment that touches
//! nothing within its limits turns by zero (held into the limits)
//! and says so.

use crate::SceneError;

/// Most pieces accepted in one section.
pub const PIECES_MAX: usize = 200_000;
/// Shortest piece accepted, metres.
pub const PIECE_MIN_M: f64 = 1.0e-9;
/// Largest clearance accepted, metres.
pub const CLEARANCE_MAX_M: f64 = 0.05;
/// Joint limits must lie inside `[-LIMIT_ABS_MAX_RAD, LIMIT_ABS_MAX_RAD]`.
pub const LIMIT_ABS_MAX_RAD: f64 = 3.0;

/// How a segment ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    /// Turned until it first touched a piece.
    Touched,
    /// Already within the clearance at its lower limit: held there.
    InsideAtLimit,
    /// Touched nothing within its limits.
    Clear,
}

impl Touch {
    /// A short name for reports.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Touched => "touched",
            Self::InsideAtLimit => "inside_at_limit",
            Self::Clear => "clear",
        }
    }
}

/// One finger's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Follow {
    /// Turn at each joint, radians: the first against +x, the rest
    /// against the previous segment.
    pub joint_rad: Vec<f64>,
    /// Far end of each segment, bend-plane metres.
    pub ends: Vec<(f64, f64)>,
    /// How each segment ended up.
    pub touch: Vec<Touch>,
    /// Least distance from each final segment to the section, less
    /// the clearance (0 at a touch, negative inside).
    pub gap_m: Vec<f64>,
}

/// Curls one finger against a section.
///
/// Checks, in order: every number finite; 1 to
/// `curl::SEGMENTS_MAX` segments, each in the two-bone length range;
/// one `(lo, hi)` limit per segment with `lo < hi` inside
/// `±LIMIT_ABS_MAX_RAD`; clearance in `(0, CLEARANCE_MAX_M]`; at most
/// `PIECES_MAX` pieces, none shorter than `PIECE_MIN_M`. An empty
/// section is allowed (every segment is then clear).
///
/// # Errors
/// The first failed check, as a [`SceneError`].
pub fn follow(
    pieces: &[[f64; 4]],
    segments_m: &[f64],
    limits_rad: &[(f64, f64)],
    clearance_m: f64,
) -> Result<Follow, SceneError> {
    if !clearance_m.is_finite() {
        return Err(SceneError::NonFinite {
            field: "clearance_m",
        });
    }
    if segments_m.iter().any(|l| !l.is_finite()) {
        return Err(SceneError::NonFinite {
            field: "segments_m",
        });
    }
    if limits_rad
        .iter()
        .any(|(a, b)| !a.is_finite() || !b.is_finite())
    {
        return Err(SceneError::NonFinite {
            field: "limits_rad",
        });
    }
    if pieces.iter().flatten().any(|x| !x.is_finite()) {
        return Err(SceneError::NonFinite { field: "pieces" });
    }
    if segments_m.is_empty() || segments_m.len() > crate::curl::SEGMENTS_MAX {
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
    if limits_rad.len() != segments_m.len() {
        return Err(SceneError::BadLimits {
            joint: limits_rad.len().min(segments_m.len()),
        });
    }
    for (j, &(lo, hi)) in limits_rad.iter().enumerate() {
        if lo >= hi || lo < -LIMIT_ABS_MAX_RAD || hi > LIMIT_ABS_MAX_RAD {
            return Err(SceneError::BadLimits { joint: j });
        }
    }
    if !(clearance_m > 0.0 && clearance_m <= CLEARANCE_MAX_M) {
        return Err(SceneError::ClearanceOutOfRange { clearance_m });
    }
    if pieces.len() > PIECES_MAX {
        return Err(SceneError::BadSection {
            piece: pieces.len(),
        });
    }
    for (k, pc) in pieces.iter().enumerate() {
        if (pc[2] - pc[0]).hypot(pc[3] - pc[1]) < PIECE_MIN_M {
            return Err(SceneError::BadSection { piece: k });
        }
    }

    let t = clearance_m;
    let mut p = (0.0_f64, 0.0_f64);
    let mut heading = 0.0_f64;
    let mut out = Follow {
        joint_rad: Vec::new(),
        ends: Vec::new(),
        touch: Vec::new(),
        gap_m: Vec::new(),
    };
    for (&l, &(lo, hi)) in segments_m.iter().zip(limits_rad) {
        let at = |turn: f64| {
            let h = heading + turn;
            (p.0 + l * h.cos(), p.1 + l * h.sin())
        };
        let (turn, touch) = if section_gap(pieces, p, at(lo), t) < 0.0 {
            (lo, Touch::InsideAtLimit)
        } else {
            let first = pieces
                .iter()
                .flat_map(|pc| events(pc, p, l, t))
                .map(|phi| wrap_pi(phi - heading))
                .filter(|&x| x > lo && x <= hi)
                .reduce(f64::min);
            match first {
                Some(x) => (x, Touch::Touched),
                None => (0.0_f64.clamp(lo, hi), Touch::Clear),
            }
        };
        let end = at(turn);
        out.joint_rad.push(turn);
        out.gap_m.push(section_gap(pieces, p, end, t));
        out.ends.push(end);
        out.touch.push(touch);
        heading += turn;
        p = end;
    }
    Ok(out)
}

/// Absolute directions from `p` at which a segment of length `l`
/// comes exactly `t` from the piece `pc` by one of the three ways.
fn events(pc: &[f64; 4], p: (f64, f64), l: f64, t: f64) -> Vec<f64> {
    let (a, b) = ((pc[0], pc[1]), (pc[2], pc[3]));
    let len = (b.0 - a.0).hypot(b.1 - a.1);
    let e = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let n = (-e.1, e.0);
    let mut out = Vec::new();
    // 1. tip on an offset line, foot inside the piece
    for s in [-1.0_f64, 1.0] {
        let w = (a.0 + s * t * n.0 - p.0, a.1 + s * t * n.1 - p.1);
        let half_b = w.0 * e.0 + w.1 * e.1;
        let c = w.0 * w.0 + w.1 * w.1 - l * l;
        let disc = half_b * half_b - c;
        if disc < 0.0 {
            continue;
        }
        let r = disc.sqrt();
        for lam in [-half_b - r, -half_b + r] {
            if (0.0..=len).contains(&lam) {
                out.push((w.1 + lam * e.1).atan2(w.0 + lam * e.0));
            }
        }
    }
    for q in [a, b] {
        // 2. tip on the circle of radius t round a piece end
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let d = dx.hypot(dy);
        if d > 1.0e-15 && d <= l + t && d >= (l - t).abs() {
            let along = (l * l - t * t + d * d) / (2.0 * d);
            let h = (l * l - along * along).max(0.0).sqrt();
            let base = dy.atan2(dx);
            let off = h.atan2(along);
            out.extend([base + off, base - off]);
        }
        // 3. the side passes within t of a piece end, the foot on the segment
        if d > t {
            let foot = (d * d - t * t).sqrt();
            if foot > 0.0 && foot < l {
                let base = dy.atan2(dx);
                let off = (t / d).asin();
                out.extend([base + off, base - off]);
            }
        }
    }
    out
}

/// Least distance from segment `p`-`q` to the section, less `t`.
fn section_gap(pieces: &[[f64; 4]], p: (f64, f64), q: (f64, f64), t: f64) -> f64 {
    pieces
        .iter()
        .map(|pc| seg_seg(p, q, (pc[0], pc[1]), (pc[2], pc[3])))
        .fold(f64::INFINITY, f64::min)
        - t
}

fn point_seg(x: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let dd = dx * dx + dy * dy;
    let s = if dd > 0.0 {
        (((x.0 - a.0) * dx + (x.1 - a.1) * dy) / dd).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a.0 + s * dx - x.0).hypot(a.1 + s * dy - x.1)
}

fn cross(o: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

/// Distance between two segments (0 when they cross).
fn seg_seg(p: (f64, f64), q: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (d1, d2) = (cross(a, b, p), cross(a, b, q));
    let (d3, d4) = (cross(p, q, a), cross(p, q, b));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return 0.0;
    }
    point_seg(p, a, b)
        .min(point_seg(q, a, b))
        .min(point_seg(a, p, q))
        .min(point_seg(b, p, q))
}

fn wrap_pi(a: f64) -> f64 {
    let pi = std::f64::consts::PI;
    let x = (a + pi).rem_euclid(std::f64::consts::TAU) - pi;
    if x == -pi { pi } else { x }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEG: f64 = std::f64::consts::PI / 180.0;
    const FINGER: [(f64, f64); 3] = [
        (-20.0 * DEG, 90.0 * DEG),
        (0.0, 100.0 * DEG),
        (0.0, 80.0 * DEG),
    ];

    /// A ball of radius r centred at (cx, cy), as n straight pieces.
    fn ball(cx: f64, cy: f64, r: f64, n: usize) -> Vec<[f64; 4]> {
        (0..n)
            .map(|k| {
                let (a0, a1) = (
                    std::f64::consts::TAU * k as f64 / n as f64,
                    std::f64::consts::TAU * (k + 1) as f64 / n as f64,
                );
                [
                    cx + r * a0.cos(),
                    cy + r * a0.sin(),
                    cx + r * a1.cos(),
                    cy + r * a1.sin(),
                ]
            })
            .collect()
    }

    // ---- validation ----
    #[test]
    fn a_floor_is_met_by_the_tip_at_the_closed_form_angle() {
        // floor y = c; a capsule of radius t touches it when its tip is c - t high
        let (c, t, l) = (0.03, 0.004, 0.05);
        let f = follow(&[[-1.0, c, 1.0, c]], &[l], &[(0.0, 1.5)], t).unwrap();
        let want = ((c - t) / l).asin();
        assert!(
            (f.joint_rad[0] - want).abs() < 1e-12,
            "{} vs {want}",
            f.joint_rad[0]
        );
        assert_eq!(f.touch[0], Touch::Touched);
        assert!(f.gap_m[0].abs() < 1e-12);
    }

    #[test]
    fn a_piece_end_is_met_by_the_side_at_the_closed_form_angle() {
        // a wall from (0.02, 0.01) upward: the side meets its lower end first
        let (t, a) = (0.002, (0.02_f64, 0.01_f64));
        let f = follow(&[[a.0, a.1, a.0, 0.05]], &[0.04], &[(0.0, 1.5)], t).unwrap();
        let d = a.0.hypot(a.1);
        let want = a.1.atan2(a.0) - (t / d).asin();
        assert!(
            (f.joint_rad[0] - want).abs() < 1e-12,
            "{} vs {want}",
            f.joint_rad[0]
        );
        assert!(f.gap_m[0].abs() < 1e-12);
    }

    #[test]
    fn on_a_ball_every_segment_touches_and_none_goes_in() {
        // the maren-v2d shape: a 44 mm ball under a straight finger
        let pieces = ball(0.045, 0.050, 0.044, 720);
        let f = follow(&pieces, &[0.042, 0.028, 0.021], &FINGER, 0.006).unwrap();
        for (k, g) in f.gap_m.iter().enumerate() {
            match f.touch[k] {
                Touch::Touched => assert!(*g >= -1e-9, "segment {k} goes {g} m in"),
                // the tip landed heading inward and the next joint cannot bend back: held at
                // its limit, and reported; on this ball that costs about 0.1 mm
                Touch::InsideAtLimit => assert!(*g > -0.0005, "segment {k} goes {g} m in"),
                Touch::Clear => panic!("segment {k} touched nothing"),
            }
        }
        assert_eq!(f.touch[0], Touch::Touched);
        assert!(f.joint_rad[0] > 0.0);
    }

    #[test]
    fn the_same_input_gives_the_same_bits() {
        let pieces = ball(0.04, 0.05, 0.045, 360);
        let a = follow(&pieces, &[0.042, 0.028, 0.021], &FINGER, 0.005).unwrap();
        let b = follow(&pieces, &[0.042, 0.028, 0.021], &FINGER, 0.005).unwrap();
        assert_eq!(a, b);
    }

    // ---- adversarial ----
    #[test]
    fn over_many_balls_no_touched_segment_ever_goes_in() {
        for cx in [0.0, 0.02, 0.05, 0.08] {
            for cy in [0.03, 0.05, 0.08] {
                for r in [0.02, 0.044, 0.09] {
                    let pieces = ball(cx, cy, r, 360);
                    let f = follow(&pieces, &[0.042, 0.028, 0.021], &FINGER, 0.006).unwrap();
                    for k in 0..3 {
                        if f.touch[k] != Touch::InsideAtLimit {
                            assert!(
                                f.gap_m[k] >= -1e-9,
                                "ball ({cx},{cy},{r}) seg {k}: {}",
                                f.gap_m[k]
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_surface_behind_the_finger_is_never_touched() {
        let f = follow(
            &[[-1.0, -0.02, 1.0, -0.02]],
            &[0.04, 0.03],
            &FINGER[..2],
            0.005,
        )
        .unwrap();
        assert_eq!(f.touch, vec![Touch::Clear, Touch::Clear]);
        assert_eq!(f.joint_rad, vec![0.0, 0.0]);
    }

    #[test]
    fn a_surface_out_of_reach_leaves_the_finger_straight_and_clear() {
        let f = follow(&[[-1.0, 0.5, 1.0, 0.5]], &[0.04], &[(0.0, 1.5)], 0.005).unwrap();
        assert_eq!((f.touch[0], f.joint_rad[0]), (Touch::Clear, 0.0));
        let f = follow(&[], &[0.04], &[(0.1, 1.5)], 0.005).unwrap();
        assert_eq!(
            (f.touch[0], f.joint_rad[0]),
            (Touch::Clear, 0.1),
            "held into the limits"
        );
    }

    #[test]
    fn a_segment_inside_at_its_lower_limit_is_held_there_and_says_so() {
        let f = follow(&[[-1.0, 0.001, 1.0, 0.001]], &[0.04], &[(-0.1, 1.5)], 0.005).unwrap();
        assert_eq!((f.touch[0], f.joint_rad[0]), (Touch::InsideAtLimit, -0.1));
        assert!(f.gap_m[0] < 0.0);
    }

    #[test]
    fn a_touch_past_the_upper_limit_is_not_taken() {
        // the floor is met at about 34 degrees; the joint stops at 20
        let f = follow(
            &[[-1.0, 0.03, 1.0, 0.03]],
            &[0.05],
            &[(0.0, 20.0 * DEG)],
            0.002,
        )
        .unwrap();
        assert_eq!(f.touch[0], Touch::Clear);
    }

    #[test]
    fn bad_input_is_refused_by_name() {
        let s = [[0.0, 0.01, 0.1, 0.01]];
        assert_eq!(
            follow(&s, &[0.04], &[(0.0, 1.0)], f64::NAN),
            Err(SceneError::NonFinite {
                field: "clearance_m"
            })
        );
        assert_eq!(
            follow(&[[0.0, f64::NAN, 0.1, 0.0]], &[0.04], &[(0.0, 1.0)], 0.005),
            Err(SceneError::NonFinite { field: "pieces" })
        );
        assert_eq!(
            follow(&[[0.0, 0.01, 0.0, 0.01]], &[0.04], &[(0.0, 1.0)], 0.005),
            Err(SceneError::BadSection { piece: 0 })
        );
        assert_eq!(
            follow(&s, &[0.04], &[], 0.005),
            Err(SceneError::BadLimits { joint: 0 })
        );
        assert_eq!(
            follow(&s, &[0.04, 0.03], &[(0.0, 1.0), (1.0, 0.5)], 0.005),
            Err(SceneError::BadLimits { joint: 1 })
        );
        assert_eq!(
            follow(&s, &[0.04], &[(0.0, 1.0)], 0.0),
            Err(SceneError::ClearanceOutOfRange { clearance_m: 0.0 })
        );
        assert_eq!(
            follow(&s, &[], &[], 0.005),
            Err(SceneError::BadChain { segments: 0 })
        );
    }
}
