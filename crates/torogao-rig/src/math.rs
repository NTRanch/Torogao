//! Minimal f32 vector/matrix math for puppet evaluation.
//!
//! `Mat4` is column-major (`cols[col][row]`), matching the GL
//! convention of the INP1 reference route: a node's relative matrix
//! is `T * R * S` (translation, then rotation, then scale), and a
//! child's absolute matrix is `parent_absolute * relative`.
//! All arithmetic is plain f32 in a fixed operation order so results
//! are reproducible across processes on one machine.

use std::ops::{Add, Mul, Sub};

/// A 2D vector / point in model units.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    /// Construct from components.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Component-wise multiplication.
    pub fn mul_vec(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x * other.x, self.y * other.y)
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Distance to another point.
    pub fn distance(self, other: Vec2) -> f32 {
        self.sub(other).length()
    }

    /// Unit vector in the same direction. A zero vector yields NaN
    /// components, exactly as the reference route's normalize does;
    /// the physics integrator's non-finite revert contains that case.
    pub fn normalize(self) -> Vec2 {
        let len = self.length();
        Vec2::new(self.x / len, self.y / len)
    }
}

impl Add for Vec2 {
    type Output = Vec2;

    /// Component-wise addition.
    fn add(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;

    /// Component-wise subtraction.
    fn sub(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;

    /// Scalar multiplication.
    fn mul(self, scalar: f32) -> Vec2 {
        Vec2::new(self.x * scalar, self.y * scalar)
    }
}

/// A 3D vector, used for node translations and Euler rotations.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// Construct from components.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// The XY components as a `Vec2`.
    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// A 4x4 matrix in column-major order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    /// `cols[col][row]`.
    pub cols: [[f32; 4]; 4],
}

impl Mat4 {
    /// The identity matrix.
    pub const IDENTITY: Mat4 = Mat4 {
        cols: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// Translation matrix.
    pub fn from_translation(t: Vec3) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.cols[3] = [t.x, t.y, t.z, 1.0];
        m
    }

    /// Non-uniform scale matrix (z scale 1).
    pub fn from_scale_xy(s: Vec2) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.cols[0][0] = s.x;
        m.cols[1][1] = s.y;
        m
    }

    /// Rotation matrix from Euler angles in radians, XYZ order:
    /// the composed rotation applies Z first, then Y, then X to a
    /// column vector (the reference route's `EulerRot::XYZ`).
    pub fn from_euler_xyz(rot: Vec3) -> Mat4 {
        let q = quat_from_euler_xyz(rot);
        mat4_from_quat(q)
    }

    /// Matrix product `self * rhs` (rhs applies first to vectors).
    pub fn mul_mat(self, rhs: Mat4) -> Mat4 {
        let mut out = [[0.0f32; 4]; 4];
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0f32;
                for k in 0..4 {
                    sum += self.cols[k][row] * rhs.cols[col][k];
                }
                out[col][row] = sum;
            }
        }
        Mat4 { cols: out }
    }

    /// Transform a 2D point (z = 0, w = 1); returns the XY result.
    /// Puppet matrices are affine, so no perspective divide applies.
    pub fn transform_point2(self, p: Vec2) -> Vec2 {
        let c = &self.cols;
        Vec2::new(
            c[0][0] * p.x + c[1][0] * p.y + c[3][0],
            c[0][1] * p.x + c[1][1] * p.y + c[3][1],
        )
    }

    /// Transform a full homogeneous vector; used by the physics
    /// output mapping, which inverts the absolute matrix.
    pub fn transform_vec4(self, v: [f32; 4]) -> [f32; 4] {
        let c = &self.cols;
        let mut out = [0.0f32; 4];
        for row in 0..4 {
            out[row] = c[0][row] * v[0] + c[1][row] * v[1] + c[2][row] * v[2] + c[3][row] * v[3];
        }
        out
    }

    /// The translation column as a 2D point.
    pub fn translation2(self) -> Vec2 {
        Vec2::new(self.cols[3][0], self.cols[3][1])
    }

    /// Inverse of an affine matrix (the only kind this evaluator
    /// builds: T * R * S products, bottom row [0, 0, 0, 1]).
    /// Computed directly in column-major form: for the 3x3 part
    /// with columns c0, c1, c2, the inverse's rows are the cross
    /// products (c1 x c2), (c2 x c0), (c0 x c1) over the
    /// determinant, and the translation maps to -A^-1 * t.
    /// Returns `None` for a singular 3x3 part.
    pub fn inverse(self) -> Option<Mat4> {
        let a = &self.cols;
        let c0 = [a[0][0], a[0][1], a[0][2]];
        let c1 = [a[1][0], a[1][1], a[1][2]];
        let c2 = [a[2][0], a[2][1], a[2][2]];
        let t = [a[3][0], a[3][1], a[3][2]];
        let cross = |u: [f32; 3], v: [f32; 3]| {
            [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
        };
        let r0 = cross(c1, c2);
        let r1 = cross(c2, c0);
        let r2 = cross(c0, c1);
        let det = r0[0] * c0[0] + r0[1] * c0[1] + r0[2] * c0[2];
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let inv_det = 1.0 / det;
        // Inverse 3x3 columns: column j gathers component j of
        // each cross-product row.
        let inv_cols = [
            [r0[0] * inv_det, r1[0] * inv_det, r2[0] * inv_det],
            [r0[1] * inv_det, r1[1] * inv_det, r2[1] * inv_det],
            [r0[2] * inv_det, r1[2] * inv_det, r2[2] * inv_det],
        ];
        let mut t_inv = [0.0f32; 3];
        for row in 0..3 {
            t_inv[row] =
                -(inv_cols[0][row] * t[0] + inv_cols[1][row] * t[1] + inv_cols[2][row] * t[2]);
        }
        Some(Mat4 {
            cols: [
                [inv_cols[0][0], inv_cols[0][1], inv_cols[0][2], 0.0],
                [inv_cols[1][0], inv_cols[1][1], inv_cols[1][2], 0.0],
                [inv_cols[2][0], inv_cols[2][1], inv_cols[2][2], 0.0],
                [t_inv[0], t_inv[1], t_inv[2], 1.0],
            ],
        })
    }
}

/// Quaternion (x, y, z, w) for Euler angles in XYZ order:
/// `q = qx * qy * qz` under the Hamilton product, so a column vector
/// is rotated by Z, then Y, then X.
fn quat_from_euler_xyz(rot: Vec3) -> [f32; 4] {
    let qx = quat_axis_angle(1.0, 0.0, 0.0, rot.x);
    let qy = quat_axis_angle(0.0, 1.0, 0.0, rot.y);
    let qz = quat_axis_angle(0.0, 0.0, 1.0, rot.z);
    quat_mul(quat_mul(qx, qy), qz)
}

/// Quaternion for a rotation of `angle` radians about a unit axis.
fn quat_axis_angle(ax: f32, ay: f32, az: f32, angle: f32) -> [f32; 4] {
    let half = angle * 0.5;
    let s = half.sin();
    [ax * s, ay * s, az * s, half.cos()]
}

/// Hamilton product `a * b` (b applies first to column vectors).
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// Rotation matrix for a quaternion (x, y, z, w), column-major.
fn mat4_from_quat(q: [f32; 4]) -> Mat4 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, xy, xz) = (x * x2, x * y2, x * z2);
    let (yy, yz, zz) = (y * y2, y * z2, z * z2);
    let (wx, wy, wz) = (w * x2, w * y2, w * z2);
    Mat4 {
        cols: [
            [1.0 - (yy + zz), xy + wz, xz - wy, 0.0],
            [xy - wz, 1.0 - (xx + zz), yz + wx, 0.0],
            [xz + wy, yz - wx, 1.0 - (xx + yy), 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-6
    }

    #[test]
    fn rotation_z_quarter_turn_maps_x_to_y() {
        let m = Mat4::from_euler_xyz(Vec3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2));
        let p = m.transform_point2(Vec2::new(1.0, 0.0));
        assert!(close(p.x, 0.0) && close(p.y, 1.0), "got {p:?}");
    }

    #[test]
    fn rotation_x_quarter_turn_maps_y_to_z() {
        let m = Mat4::from_euler_xyz(Vec3::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0));
        let out = m.transform_vec4([0.0, 1.0, 0.0, 1.0]);
        assert!(close(out[0], 0.0) && close(out[1], 0.0) && close(out[2], 1.0));
    }

    #[test]
    fn trs_composition_and_inverse_round_trip() {
        // The round trip runs through the homogeneous transform:
        // an Euler rotation about X moves the point off the z = 0
        // plane, so the 2D point helper (which reads z as 0)
        // cannot express the return leg.
        let m = Mat4::from_translation(Vec3::new(10.0, -4.0, 2.0))
            .mul_mat(Mat4::from_euler_xyz(Vec3::new(0.1, -0.2, 0.3)))
            .mul_mat(Mat4::from_scale_xy(Vec2::new(2.0, 3.0)));
        let world = m.transform_vec4([5.0, 7.0, 0.0, 1.0]);
        let inv = m.inverse().expect("affine matrix is invertible");
        let back = inv.transform_vec4(world);
        assert!(
            close(back[0], 5.0)
                && close(back[1], 7.0)
                && close(back[2], 0.0)
                && close(back[3], 1.0),
            "got {back:?}"
        );
    }

    #[test]
    fn planar_inverse_round_trip_via_point2() {
        // For a z-planar TRS (rotation about Z only) the 2D point
        // helper round-trips exactly.
        let m = Mat4::from_translation(Vec3::new(10.0, -4.0, 0.0))
            .mul_mat(Mat4::from_euler_xyz(Vec3::new(0.0, 0.0, 0.3)))
            .mul_mat(Mat4::from_scale_xy(Vec2::new(2.0, 3.0)));
        let p = Vec2::new(5.0, 7.0);
        let world = m.transform_point2(p);
        let inv = m.inverse().expect("affine matrix is invertible");
        let back = inv.transform_point2(world);
        assert!(close(back.x, p.x) && close(back.y, p.y), "got {back:?}");
    }
}
