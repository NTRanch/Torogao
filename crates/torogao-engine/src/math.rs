//! Vector and matrix math in f32, mirroring the reference
//! engine's (inmath's) semantics operation for operation:
//! componentwise vector arithmetic, column-major matrices
//! with column-vector multiplication, and the classic
//! cofactor matrix inverse. Evaluation-order fidelity here
//! is what makes simulated series agree bit for bit.

/// A 2-component f32 vector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    /// The one vector.
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };

    /// Builds a vector from its components.
    pub fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Distance to another point.
    pub fn distance(self, other: Vec2) -> f32 {
        (self - other).length()
    }

    /// The normalized vector (v / length), as the reference
    /// computes it — no zero-length guard.
    pub fn normalized(self) -> Vec2 {
        let len = self.length();
        Vec2::new(self.x / len, self.y / len)
    }

    /// Normalizes in place.
    pub fn normalize(&mut self) {
        let len = self.length();
        self.x /= len;
        self.y /= len;
    }

    /// Linear interpolation: `self * (1 - t) + other * t`,
    /// the reference `lerp` evaluation order.
    pub fn lerp(self, other: Vec2, t: f32) -> Vec2 {
        self * (1.0 - t) + other * t
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x + other.x, self.y + other.y)
    }
}

impl std::ops::AddAssign for Vec2 {
    fn add_assign(&mut self, other: Vec2) {
        *self = *self + other;
    }
}

impl std::ops::SubAssign for Vec2 {
    fn sub_assign(&mut self, other: Vec2) {
        *self = *self - other;
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x - other.x, self.y - other.y)
    }
}

impl std::ops::Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, scalar: f32) -> Vec2 {
        Vec2::new(self.x * scalar, self.y * scalar)
    }
}

impl std::ops::Mul<Vec2> for Vec2 {
    type Output = Vec2;
    fn mul(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x * other.x, self.y * other.y)
    }
}

impl std::ops::Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, scalar: f32) -> Vec2 {
        Vec2::new(self.x / scalar, self.y / scalar)
    }
}

/// A 3-component f32 vector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    /// The one vector.
    pub const ONE: Vec3 = Vec3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };

    /// Builds a vector from its components.
    pub fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// The XY components as a Vec2.
    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl std::ops::Mul<Vec3> for Vec3 {
    type Output = Vec3;
    fn mul(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }
}

/// A 4-component f32 vector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec4 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
    /// W component.
    pub w: f32,
}

impl Vec4 {
    /// Builds a vector from its components.
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Vec4 {
        Vec4 { x, y, z, w }
    }

    /// The XY components as a Vec2.
    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// A 4x4 matrix, column-major (`m[col * 4 + row]`),
/// multiplying column vectors — the reference layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    /// Column-major elements.
    pub m: [f32; 16],
}

impl Default for Mat4 {
    fn default() -> Mat4 {
        Mat4::ZERO
    }
}

impl Mat4 {
    /// The all-zero matrix (the reference's default struct
    /// state for uninitialized matrix fields).
    pub const ZERO: Mat4 = Mat4 { m: [0.0; 16] };

    /// The identity matrix.
    pub fn identity() -> Mat4 {
        let mut out = Mat4::ZERO;
        out.m[0] = 1.0;
        out.m[5] = 1.0;
        out.m[10] = 1.0;
        out.m[15] = 1.0;
        out
    }

    /// A translation matrix.
    pub fn translation(t: Vec3) -> Mat4 {
        let mut out = Mat4::identity();
        out.m[12] = t.x;
        out.m[13] = t.y;
        out.m[14] = t.z;
        out
    }

    /// A scaling matrix.
    pub fn scaling(x: f32, y: f32, z: f32) -> Mat4 {
        let mut out = Mat4::ZERO;
        out.m[0] = x;
        out.m[5] = y;
        out.m[10] = z;
        out.m[15] = 1.0;
        out
    }

    /// The rotation matrix for Euler angles (x, y, z) in
    /// radians, composed as Rz * Ry * Rx (the reference
    /// quaternion's matrix form).
    pub fn euler_rotation(x: f32, y: f32, z: f32) -> Mat4 {
        let (cx, sx) = (x.cos(), x.sin());
        let (cy, sy) = (y.cos(), y.sin());
        let (cz, sz) = (z.cos(), z.sin());
        // R = Rz * Ry * Rx, written out element by element
        // (row form shown in comments; storage is column-major).
        // row 0: [cz*cy, cz*sy*sx - sz*cx, cz*sy*cx + sz*sx]
        // row 1: [sz*cy, sz*sy*sx + cz*cx, sz*sy*cx - cz*sx]
        // row 2: [-sy,   cy*sx,            cy*cx]
        let mut out = Mat4::identity();
        out.m[0] = cz * cy;
        out.m[1] = sz * cy;
        out.m[2] = -sy;
        out.m[4] = cz * sy * sx - sz * cx;
        out.m[5] = sz * sy * sx + cz * cx;
        out.m[6] = cy * sx;
        out.m[8] = cz * sy * cx + sz * sx;
        out.m[9] = sz * sy * cx - cz * sx;
        out.m[10] = cy * cx;
        out
    }

    /// Element access by (column, row).
    pub fn at(&self, col: usize, row: usize) -> f32 {
        self.m[col * 4 + row]
    }

    /// The matrix inverse, by the classic cofactor expansion
    /// (the algorithm the reference math library uses). For a
    /// singular matrix the reference produces infinities/NaNs
    /// by division; this mirrors the arithmetic without
    /// guarding it.
    /// The matrix inverse. The cofactor expressions and
    /// the determinant use the reference math library's
    /// exact term order, and every cofactor is scaled by
    /// the reciprocal of the determinant, so results
    /// agree with the reference bit-for-bit (long sums of
    /// large-magnitude products round differently under
    /// any other association).
    pub fn inverse(&self) -> Mat4 {
        let a = &self.m;
        let mut inv = [0.0f32; 16];
        inv[0] = a[5] * a[10] * a[15] + a[9] * a[14] * a[7] + a[13] * a[6] * a[11]
            - a[5] * a[14] * a[11]
            - a[9] * a[6] * a[15]
            - a[13] * a[10] * a[7];
        inv[4] = a[4] * a[14] * a[11] + a[8] * a[6] * a[15] + a[12] * a[10] * a[7]
            - a[4] * a[10] * a[15]
            - a[8] * a[14] * a[7]
            - a[12] * a[6] * a[11];
        inv[8] = a[4] * a[9] * a[15] + a[8] * a[13] * a[7] + a[12] * a[5] * a[11]
            - a[4] * a[13] * a[11]
            - a[8] * a[5] * a[15]
            - a[12] * a[9] * a[7];
        inv[12] = a[4] * a[13] * a[10] + a[8] * a[5] * a[14] + a[12] * a[9] * a[6]
            - a[4] * a[9] * a[14]
            - a[8] * a[13] * a[6]
            - a[12] * a[5] * a[10];
        inv[1] = a[1] * a[14] * a[11] + a[9] * a[2] * a[15] + a[13] * a[10] * a[3]
            - a[1] * a[10] * a[15]
            - a[9] * a[14] * a[3]
            - a[13] * a[2] * a[11];
        inv[5] = a[0] * a[10] * a[15] + a[8] * a[14] * a[3] + a[12] * a[2] * a[11]
            - a[0] * a[14] * a[11]
            - a[8] * a[2] * a[15]
            - a[12] * a[10] * a[3];
        inv[9] = a[0] * a[13] * a[11] + a[8] * a[1] * a[15] + a[12] * a[9] * a[3]
            - a[0] * a[9] * a[15]
            - a[8] * a[13] * a[3]
            - a[12] * a[1] * a[11];
        inv[13] = a[0] * a[9] * a[14] + a[8] * a[13] * a[2] + a[12] * a[1] * a[10]
            - a[0] * a[13] * a[10]
            - a[8] * a[1] * a[14]
            - a[12] * a[9] * a[2];
        inv[2] = a[1] * a[6] * a[15] + a[5] * a[14] * a[3] + a[13] * a[2] * a[7]
            - a[1] * a[14] * a[7]
            - a[5] * a[2] * a[15]
            - a[13] * a[6] * a[3];
        inv[6] = a[0] * a[14] * a[7] + a[4] * a[2] * a[15] + a[12] * a[6] * a[3]
            - a[0] * a[6] * a[15]
            - a[4] * a[14] * a[3]
            - a[12] * a[2] * a[7];
        inv[10] = a[0] * a[5] * a[15] + a[4] * a[13] * a[3] + a[12] * a[1] * a[7]
            - a[0] * a[13] * a[7]
            - a[4] * a[1] * a[15]
            - a[12] * a[5] * a[3];
        inv[14] = a[0] * a[13] * a[6] + a[4] * a[1] * a[14] + a[12] * a[5] * a[2]
            - a[0] * a[5] * a[14]
            - a[4] * a[13] * a[2]
            - a[12] * a[1] * a[6];
        inv[3] = a[1] * a[10] * a[7] + a[5] * a[2] * a[11] + a[9] * a[6] * a[3]
            - a[1] * a[6] * a[11]
            - a[5] * a[10] * a[3]
            - a[9] * a[2] * a[7];
        inv[7] = a[0] * a[6] * a[11] + a[4] * a[10] * a[3] + a[8] * a[2] * a[7]
            - a[0] * a[10] * a[7]
            - a[4] * a[2] * a[11]
            - a[8] * a[6] * a[3];
        inv[11] = a[0] * a[9] * a[7] + a[4] * a[1] * a[11] + a[8] * a[5] * a[3]
            - a[0] * a[5] * a[11]
            - a[4] * a[9] * a[3]
            - a[8] * a[1] * a[7];
        inv[15] = a[0] * a[5] * a[10] + a[4] * a[9] * a[2] + a[8] * a[1] * a[6]
            - a[0] * a[9] * a[6]
            - a[4] * a[1] * a[10]
            - a[8] * a[5] * a[2];
        let det =
            a[12] * a[9] * a[6] * a[3] - a[8] * a[13] * a[6] * a[3] - a[12] * a[5] * a[10] * a[3]
                + a[4] * a[13] * a[10] * a[3]
                + a[8] * a[5] * a[14] * a[3]
                - a[4] * a[9] * a[14] * a[3]
                - a[12] * a[9] * a[2] * a[7]
                + a[8] * a[13] * a[2] * a[7]
                + a[12] * a[1] * a[10] * a[7]
                - a[0] * a[13] * a[10] * a[7]
                - a[8] * a[1] * a[14] * a[7]
                + a[0] * a[9] * a[14] * a[7]
                + a[12] * a[5] * a[2] * a[11]
                - a[4] * a[13] * a[2] * a[11]
                - a[12] * a[1] * a[6] * a[11]
                + a[0] * a[13] * a[6] * a[11]
                + a[4] * a[1] * a[14] * a[11]
                - a[0] * a[5] * a[14] * a[11]
                - a[8] * a[5] * a[2] * a[15]
                + a[4] * a[9] * a[2] * a[15]
                + a[8] * a[1] * a[6] * a[15]
                - a[0] * a[9] * a[6] * a[15]
                - a[4] * a[1] * a[10] * a[15]
                + a[0] * a[5] * a[10] * a[15];
        let inv_det = 1.0 / det;
        let mut out = Mat4::ZERO;
        for (i, cell) in inv.iter().enumerate() {
            out.m[i] = cell * inv_det;
        }
        out
    }
}

impl std::ops::Mul for Mat4 {
    type Output = Mat4;
    /// Standard product: `(self * other) * v == self * (other * v)`.
    fn mul(self, other: Mat4) -> Mat4 {
        let mut out = Mat4::ZERO;
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0f32;
                for k in 0..4 {
                    sum += self.m[k * 4 + row] * other.m[col * 4 + k];
                }
                out.m[col * 4 + row] = sum;
            }
        }
        out
    }
}

impl std::ops::Mul<Vec4> for Mat4 {
    type Output = Vec4;
    fn mul(self, v: Vec4) -> Vec4 {
        Vec4::new(
            self.m[0] * v.x + self.m[4] * v.y + self.m[8] * v.z + self.m[12] * v.w,
            self.m[1] * v.x + self.m[5] * v.y + self.m[9] * v.z + self.m[13] * v.w,
            self.m[2] * v.x + self.m[6] * v.y + self.m[10] * v.z + self.m[14] * v.w,
            self.m[3] * v.x + self.m[7] * v.y + self.m[11] * v.z + self.m[15] * v.w,
        )
    }
}

/// A 3x3 matrix, column-major, used by the mesh-group warp.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mat3 {
    /// Column-major elements.
    pub m: [f32; 9],
}

impl Mat3 {
    /// Builds a matrix from nine elements in the reference
    /// constructor's array order. The reference constructor
    /// fills its matrix row by row, while this storage is
    /// column-major, so the elements are transposed on
    /// store: `values[row * 3 + col]` lands at
    /// `m[col * 3 + row]`.
    pub fn from_array(values: [f32; 9]) -> Mat3 {
        Mat3 {
            m: [
                values[0], values[3], values[6], values[1], values[4], values[7], values[2],
                values[5], values[8],
            ],
        }
    }
}

impl std::ops::Mul for Mat3 {
    type Output = Mat3;
    fn mul(self, other: Mat3) -> Mat3 {
        let mut out = Mat3::default();
        for col in 0..3 {
            for row in 0..3 {
                let mut sum = 0.0f32;
                for k in 0..3 {
                    sum += self.m[k * 3 + row] * other.m[col * 3 + k];
                }
                out.m[col * 3 + row] = sum;
            }
        }
        out
    }
}

impl std::ops::Mul<Vec3> for Mat3 {
    type Output = Vec3;
    fn mul(self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.m[0] * v.x + self.m[3] * v.y + self.m[6] * v.z,
            self.m[1] * v.x + self.m[4] * v.y + self.m[7] * v.z,
            self.m[2] * v.x + self.m[5] * v.y + self.m[8] * v.z,
        )
    }
}

/// Clamps a value into `[lo, hi]` (f32, the reference
/// `clamp` for floats).
pub fn clamp_f32(value: f32, lo: f32, hi: f32) -> f32 {
    if value < lo {
        lo
    } else if value > hi {
        hi
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::{Mat4, Vec2, Vec3, Vec4};

    #[test]
    fn mat3_from_array_uses_reference_row_major_order() {
        // The reference constructor fills row by row, so
        // [1, 2, 3, 4, 5, 6, 0, 0, 1] is the matrix with
        // rows (1, 2, 3) and (4, 5, 6): applying it to
        // (1, 1, 1) must give (1 + 2 + 3, 4 + 5 + 6, 1).
        // Storing the array verbatim in column-major order
        // transposed every literal-built warp matrix.
        let m = super::Mat3::from_array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.0, 0.0, 1.0]);
        let p = m * Vec3::new(1.0, 1.0, 1.0);
        assert_eq!(p, Vec3::new(6.0, 15.0, 1.0));
    }

    #[test]
    fn translation_matrix_moves_points() {
        let m = Mat4::translation(Vec3::new(10.0, 20.0, 0.0));
        let p = m * Vec4::new(1.0, 2.0, 0.0, 1.0);
        assert_eq!(p.xy(), Vec2::new(11.0, 22.0));
    }

    #[test]
    fn inverse_undoes_a_transform() {
        let m = Mat4::translation(Vec3::new(3.0, -4.0, 0.0)) * Mat4::scaling(2.0, 4.0, 1.0);
        let inv = m.inverse();
        let p = inv * (m * Vec4::new(1.0, 1.0, 0.0, 1.0));
        assert!(
            (p.x - 1.0).abs() < 1e-6
                && (p.y - 1.0).abs() < 1e-6
                && p.z.abs() < 1e-6
                && (p.w - 1.0).abs() < 1e-6
        );
    }

    #[test]
    fn inverse_of_identity_is_identity() {
        let inv = Mat4::identity().inverse();
        for (i, cell) in inv.m.iter().enumerate() {
            let want = if i % 5 == 0 { 1.0 } else { 0.0 };
            assert_eq!(*cell, want, "element {i}");
        }
    }

    #[test]
    fn inverse_multiplies_back_to_identity_on_all_elements() {
        let m = Mat4::translation(Vec3::new(-7.0, 11.0, 3.0))
            * Mat4::euler_rotation(0.3, -0.2, 0.9)
            * Mat4::scaling(1.5, 0.75, 2.0);
        let product = m * m.inverse();
        for (i, cell) in product.m.iter().enumerate() {
            let want = if i % 5 == 0 { 1.0 } else { 0.0 };
            assert!((cell - want).abs() < 1e-5, "element {i}: {cell}");
        }
    }

    #[test]
    fn euler_z_rotation_is_the_elementary_rotation() {
        let m = Mat4::euler_rotation(0.0, 0.0, std::f32::consts::FRAC_PI_2);
        let p = m * Vec4::new(1.0, 0.0, 0.0, 1.0);
        assert!(p.x.abs() < 1e-6 && (p.y - 1.0).abs() < 1e-6);
    }
}
