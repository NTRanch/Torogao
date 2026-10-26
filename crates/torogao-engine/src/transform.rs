//! The node transform: translation / rotation / scale with
//! a cached matrix, ported operation-for-operation from the
//! reference `Transform` — including its composition quirk:
//! multiplying two transforms extracts the combined
//! translation as `matrix * (1, 1, 1, 1)`, not the matrix's
//! translation column.

use crate::math::{Mat4, Vec2, Vec3, Vec4};

/// A translation + Euler rotation + 2D scale, with its
/// composed matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Translation.
    pub translation: Vec3,
    /// Euler rotation in radians (x, y, z).
    pub rotation: Vec3,
    /// Scale (z is always 1 in the matrix).
    pub scale: Vec2,
    /// The composed matrix, refreshed by `update`.
    pub trs: Mat4,
}

impl Default for Transform {
    fn default() -> Transform {
        Transform {
            translation: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec2::ONE,
            trs: Mat4::identity(),
        }
    }
}

impl Transform {
    /// A transform with the given translation and identity
    /// rotation/scale (matrix updated).
    pub fn from_translation(translation: Vec3) -> Transform {
        let mut transform = Transform {
            translation,
            ..Transform::default()
        };
        transform.update();
        transform
    }

    /// Recomposes the matrix: T * R * S.
    pub fn update(&mut self) {
        self.trs = Mat4::translation(self.translation)
            * Mat4::euler_rotation(self.rotation.x, self.rotation.y, self.rotation.z)
            * Mat4::scaling(self.scale.x, self.scale.y, 1.0);
    }

    /// Resets to the identity transform values (the
    /// reference `clear`).
    pub fn clear(&mut self) {
        self.translation = Vec3::ZERO;
        self.rotation = Vec3::ZERO;
        self.scale = Vec2::ONE;
    }

    /// `self` offset by `other`: componentwise translation
    /// and rotation sums, componentwise scale product, matrix
    /// recomputed (the reference `calcOffset`).
    pub fn calc_offset(&self, other: &Transform) -> Transform {
        let mut out = Transform {
            translation: self.translation + other.translation,
            rotation: self.rotation + other.rotation,
            scale: self.scale * other.scale,
            trs: Mat4::identity(),
        };
        out.update();
        out
    }
}

impl std::ops::Mul for Transform {
    type Output = Transform;
    /// The reference composition: matrices multiply as
    /// `other.trs * self.trs`; the combined translation is
    /// read back as `matrix * (1, 1, 1, 1)` (sic — this is
    /// the reference behavior, preserved deliberately);
    /// rotations add, scales multiply.
    fn mul(self, other: Transform) -> Transform {
        let strs = other.trs * self.trs;
        let combined = strs * Vec4::new(1.0, 1.0, 1.0, 1.0);
        Transform {
            translation: Vec3::new(combined.x, combined.y, combined.z),
            rotation: self.rotation + other.rotation,
            scale: self.scale * other.scale,
            trs: strs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Transform;
    use crate::math::Vec3;

    #[test]
    fn identity_composition_shows_the_reference_extraction_quirk() {
        // The reference composition extracts the combined
        // translation as `matrix * (1, 1, 1, 1)`, so even
        // identity * identity reports translation (1, 1, 1)
        // in the field — while its matrix stays identity.
        // The field is never read for effect on the composed
        // transform (positions go through the matrix); the
        // quirk is pinned here so it is never "fixed" away.
        let a = Transform::default();
        let b = a * a;
        assert_eq!(b.translation, Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(b.trs, crate::math::Mat4::identity());
    }

    #[test]
    fn calc_offset_sums_components() {
        let a = Transform::from_translation(Vec3::new(1.0, 2.0, 0.0));
        let b = Transform::from_translation(Vec3::new(10.0, 20.0, 0.0));
        let c = a.calc_offset(&b);
        assert_eq!(c.translation, Vec3::new(11.0, 22.0, 0.0));
    }
}
