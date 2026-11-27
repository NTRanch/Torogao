//! Minimal f32 linear algebra for the on-device deform path.
//!
//! Column-major 4x4 matrices (`m[col * 4 + row]`), quaternions as
//! `[x, y, z, w]`. Only what the GLB loader, the pose composition,
//! and the preview renderer need — no external math crate, so the
//! trial build carries no dependency the estate has not already
//! vetted.

/// A 4x4 matrix, column-major.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [f32; 16]);

impl Mat4 {
    /// The identity matrix.
    pub fn identity() -> Self {
        Self([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// Uniform scale.
    pub fn scale(s: f32) -> Self {
        Self([
            s, 0.0, 0.0, 0.0, //
            0.0, s, 0.0, 0.0, //
            0.0, 0.0, s, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// Compose `T * R * S` from a translation, a quaternion
    /// `[x, y, z, w]`, and a scale.
    pub fn from_trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> Self {
        let [x, y, z, w] = q;
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let (wx, wy, wz) = (w * x, w * y, w * z);
        // Rotation columns, scaled per-axis.
        let c0 = [
            (1.0 - 2.0 * (yy + zz)) * s[0],
            2.0 * (xy + wz) * s[0],
            2.0 * (xz - wy) * s[0],
        ];
        let c1 = [
            2.0 * (xy - wz) * s[1],
            (1.0 - 2.0 * (xx + zz)) * s[1],
            2.0 * (yz + wx) * s[1],
        ];
        let c2 = [
            2.0 * (xz + wy) * s[2],
            2.0 * (yz - wx) * s[2],
            (1.0 - 2.0 * (xx + yy)) * s[2],
        ];
        Self([
            c0[0], c0[1], c0[2], 0.0, //
            c1[0], c1[1], c1[2], 0.0, //
            c2[0], c2[1], c2[2], 0.0, //
            t[0], t[1], t[2], 1.0,
        ])
    }

    /// Matrix product `self * rhs` (apply `rhs` first).
    pub fn mul(&self, rhs: &Mat4) -> Mat4 {
        let mut out = [0.0f32; 16];
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0f32;
                for k in 0..4 {
                    sum += self.0[k * 4 + row] * rhs.0[col * 4 + k];
                }
                out[col * 4 + row] = sum;
            }
        }
        Mat4(out)
    }

    /// Transform a point (w = 1, no perspective divide).
    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
        ]
    }

    /// Transform a direction by the upper 3x3 (no translation).
    /// Correct for the rigid-plus-uniform-scale transforms the
    /// skinning path produces; callers normalise.
    pub fn dir(&self, d: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        [
            m[0] * d[0] + m[4] * d[1] + m[8] * d[2],
            m[1] * d[0] + m[5] * d[1] + m[9] * d[2],
            m[2] * d[0] + m[6] * d[1] + m[10] * d[2],
        ]
    }

    /// Transform a point to clip space, returning `[x, y, z, w]`.
    pub fn clip(&self, p: [f32; 3]) -> [f32; 4] {
        let m = &self.0;
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
            m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15],
        ]
    }
}

/// Quaternion multiplication `a * b` (apply `b` first), `[x,y,z,w]`.
pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// Quaternion from Euler angles in degrees, applied as
/// `q = qx * qy * qz` (the trial's documented pose convention:
/// Z first, then Y, then X, in the joint's local space).
pub fn quat_from_euler_deg(e: [f32; 3]) -> [f32; 4] {
    let half = |deg: f32| (deg.to_radians() * 0.5).sin_cos();
    let (sx, cx) = half(e[0]);
    let (sy, cy) = half(e[1]);
    let (sz, cz) = half(e[2]);
    let qx = [sx, 0.0, 0.0, cx];
    let qy = [0.0, sy, 0.0, cy];
    let qz = [0.0, 0.0, sz, cz];
    quat_mul(quat_mul(qx, qy), qz)
}

/// A look-at view matrix (right-handed, camera looks down -Z in
/// view space).
pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = norm(sub(target, eye));
    let r = norm(cross(f, up));
    let u = cross(r, f);
    Mat4([
        r[0],
        u[0],
        -f[0],
        0.0, //
        r[1],
        u[1],
        -f[1],
        0.0, //
        r[2],
        u[2],
        -f[2],
        0.0, //
        -dot(r, eye),
        -dot(u, eye),
        dot(f, eye),
        1.0,
    ])
}

/// An OpenGL-style perspective projection (NDC z in [-1, 1]).
pub fn perspective(fov_y_deg: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let t = 1.0 / (fov_y_deg.to_radians() * 0.5).tan();
    let mut m = [0.0f32; 16];
    m[0] = t / aspect;
    m[5] = t;
    m[10] = (far + near) / (near - far);
    m[11] = -1.0;
    m[14] = 2.0 * far * near / (near - far);
    Mat4(m)
}

/// Vector subtraction.
pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Vector addition.
pub fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Scalar multiply.
pub fn mul_s(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// Dot product.
pub fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Cross product.
pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Normalise; a zero vector stays zero.
pub fn norm(a: [f32; 3]) -> [f32; 3] {
    let len = dot(a, a).sqrt();
    if len > 1e-12 { mul_s(a, 1.0 / len) } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_leaves_points_alone() {
        let m = Mat4::identity();
        assert_eq!(m.point([1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn trs_applies_scale_then_rotation_then_translation() {
        // 90 degrees about Z maps +X to +Y; scale 2 first: (1,0,0)
        // -> (2,0,0) -> (0,2,0) -> translated (1,2,0)... with
        // t=(1,0,0): (1,2,0).
        let q = quat_from_euler_deg([0.0, 0.0, 90.0]);
        let m = Mat4::from_trs([1.0, 0.0, 0.0], q, [2.0, 2.0, 2.0]);
        let p = m.point([1.0, 0.0, 0.0]);
        assert!((p[0] - 1.0).abs() < 1e-5, "{p:?}");
        assert!((p[1] - 2.0).abs() < 1e-5, "{p:?}");
        assert!(p[2].abs() < 1e-5, "{p:?}");
    }

    #[test]
    fn look_at_puts_target_on_negative_z() {
        let v = look_at([0.0, 1.0, 3.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]);
        let p = v.point([0.0, 1.0, 0.0]);
        assert!(p[0].abs() < 1e-5 && p[1].abs() < 1e-5, "{p:?}");
        assert!((p[2] + 3.0).abs() < 1e-5, "{p:?}");
    }
}
