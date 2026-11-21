//! Surface anchors: a spot on a triangle, held a set distance off
//! the surface.
//!
//! A hand target given as a fixed point drifts off a body that
//! breathes or moves. Given as an anchor it is recomputed from the
//! triangle's current corners each frame and stays on the skin.

use crate::{SceneError, Vec3};

/// Barycentric weights must sum to one within this.
pub const BARYCENTRIC_SUM_TOLERANCE: f64 = 1.0e-9;
/// Triangles smaller than this have no usable normal, m^2.
pub const TRIANGLE_AREA_MIN_M2: f64 = 1.0e-12;
/// Largest distance an anchor may sit off its surface, metres.
pub const OFFSET_MAX_M: f64 = 0.1;

/// A spot on a triangle. Corners are the triangle's CURRENT
/// positions, counter-clockwise seen from outside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceAnchor {
    pub corners: [Vec3; 3],
    /// Weights of the three corners: each in `[0, 1]`, sum 1.
    pub barycentric: [f64; 3],
    /// Distance off the surface along the outward normal, metres.
    /// Negative presses in.
    pub offset_m: f64,
}

/// An anchor's place in space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorPoint {
    pub point: Vec3,
    /// Unit outward normal of the triangle.
    pub normal: Vec3,
}

impl SurfaceAnchor {
    /// Where the anchor is now.
    ///
    /// Checks, in order: every number finite; weights in `[0, 1]`
    /// summing to one; offset within `OFFSET_MAX_M`; triangle area
    /// at least `TRIANGLE_AREA_MIN_M2`. Constant work.
    ///
    /// # Errors
    /// The first failed check, as a [`SceneError`].
    pub fn resolve(&self) -> Result<AnchorPoint, SceneError> {
        if !self.corners.iter().all(|corner| corner.is_finite()) {
            return Err(SceneError::NonFinite { field: "corners" });
        }
        if !self.barycentric.iter().all(|weight| weight.is_finite()) {
            return Err(SceneError::NonFinite {
                field: "barycentric",
            });
        }
        if !self.offset_m.is_finite() {
            return Err(SceneError::NonFinite { field: "offset_m" });
        }
        let sum: f64 = self.barycentric.iter().sum();
        let in_range = self
            .barycentric
            .iter()
            .all(|weight| (0.0..=1.0).contains(weight));
        if !in_range || (sum - 1.0).abs() > BARYCENTRIC_SUM_TOLERANCE {
            return Err(SceneError::BadBarycentric { sum });
        }
        if self.offset_m.abs() > OFFSET_MAX_M {
            return Err(SceneError::OffsetOutOfRange {
                offset_m: self.offset_m,
            });
        }
        let [a, b, c] = self.corners;
        let cross = (b - a).cross(c - a);
        let area_m2 = cross.length() * 0.5;
        if area_m2 < TRIANGLE_AREA_MIN_M2 {
            return Err(SceneError::DegenerateTriangle { area_m2 });
        }
        let normal = cross * (1.0 / cross.length());
        let [wa, wb, wc] = self.barycentric;
        let on_surface = a * wa + b * wb + c * wc;
        assert!((normal.length() - 1.0).abs() < 1.0e-12);
        Ok(AnchorPoint {
            point: on_surface + normal * self.offset_m,
            normal,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor_anchor() -> SurfaceAnchor {
        SurfaceAnchor {
            corners: [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            barycentric: [0.5, 0.25, 0.25],
            offset_m: 0.003,
        }
    }

    // ---- validation ----

    #[test]
    fn anchor_sits_at_the_weighted_spot_offset_along_the_normal() {
        let got = floor_anchor().resolve().unwrap();
        assert_eq!(got.normal, Vec3::new(0.0, 0.0, 1.0));
        assert!((got.point - Vec3::new(0.25, 0.25, 0.003)).length() < 1.0e-15);
    }

    #[test]
    fn anchor_rides_the_triangle_when_it_moves() {
        let mut anchor = floor_anchor();
        let lift = Vec3::new(0.2, -0.1, 0.05);
        for corner in &mut anchor.corners {
            *corner = *corner + lift;
        }
        let moved = anchor.resolve().unwrap();
        let rest = floor_anchor().resolve().unwrap();
        assert!((moved.point - (rest.point + lift)).length() < 1.0e-15);
        assert_eq!(moved.normal, rest.normal);
    }

    #[test]
    fn negative_offset_presses_in_and_a_corner_weight_gives_the_corner() {
        let mut anchor = floor_anchor();
        anchor.barycentric = [0.0, 1.0, 0.0];
        anchor.offset_m = -0.008;
        let got = anchor.resolve().unwrap();
        assert!((got.point - Vec3::new(1.0, 0.0, -0.008)).length() < 1.0e-15);
    }

    // ---- adversarial ----

    #[test]
    fn weights_outside_the_triangle_or_not_summing_to_one_are_refused() {
        for bad in [[0.5, 0.5, 0.5], [1.2, -0.1, -0.1], [0.3, 0.3, 0.3]] {
            let mut anchor = floor_anchor();
            anchor.barycentric = bad;
            assert!(matches!(
                anchor.resolve(),
                Err(SceneError::BadBarycentric { .. })
            ));
        }
    }

    #[test]
    fn a_sliver_triangle_is_refused() {
        let mut anchor = floor_anchor();
        anchor.corners[2] = Vec3::new(2.0, 0.0, 0.0);
        assert!(matches!(
            anchor.resolve(),
            Err(SceneError::DegenerateTriangle { .. })
        ));
    }

    #[test]
    fn non_finite_numbers_and_wild_offsets_are_refused() {
        let mut anchor = floor_anchor();
        anchor.corners[1].x = f64::NAN;
        assert_eq!(
            anchor.resolve(),
            Err(SceneError::NonFinite { field: "corners" })
        );
        let mut anchor = floor_anchor();
        anchor.offset_m = 0.5;
        assert!(matches!(
            anchor.resolve(),
            Err(SceneError::OffsetOutOfRange { .. })
        ));
    }
}
