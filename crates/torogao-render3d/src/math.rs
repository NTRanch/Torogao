//! Minimal f64 linear algebra for the CPU side: pose
//! composition and the morph+LBS reference. f64 throughout so
//! the reference arithmetic is the high-precision witness the
//! GPU's f32 path is measured against; matrices are
//! column-major (`m[col * 4 + row]`), matching glTF and GLSL.

/// A 3-vector in f64.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const ZERO: V3 = V3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub fn new(x: f64, y: f64, z: f64) -> V3 {
        V3 { x, y, z }
    }

    pub fn from_f32(v: [f32; 3]) -> V3 {
        V3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
    }

    pub fn plus(self, other: V3) -> V3 {
        V3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub fn minus(self, other: V3) -> V3 {
        V3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub fn scale(self, s: f64) -> V3 {
        V3::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn dot(self, other: V3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Unit-length copy; the zero vector maps to itself (the
    /// caller asserts on meaningful inputs).
    pub fn normalized(self) -> V3 {
        let len = self.length();
        if len == 0.0 {
            V3::ZERO
        } else {
            self.scale(1.0 / len)
        }
    }

    pub fn to_f32(self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
}

/// A quaternion `[x, y, z, w]` in f64.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Quat {
    pub const IDENTITY: Quat = Quat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    pub fn from_array(q: [f64; 4]) -> Quat {
        Quat {
            x: q[0],
            y: q[1],
            z: q[2],
            w: q[3],
        }
    }

    /// Quaternion for a rotation of `angle` radians about a
    /// unit `axis`.
    pub fn from_axis_angle(axis: V3, angle: f64) -> Quat {
        let half = angle * 0.5;
        let s = half.sin();
        Quat {
            x: axis.x * s,
            y: axis.y * s,
            z: axis.z * s,
            w: half.cos(),
        }
    }

    /// Hamilton product: `self * other` applies `other`
    /// first, then `self`.
    pub fn multiply(self, other: Quat) -> Quat {
        Quat {
            x: self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            y: self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            z: self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
            w: self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
        }
    }

    pub fn normalized(self) -> Quat {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if len == 0.0 {
            Quat::IDENTITY
        } else {
            Quat {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
                w: self.w / len,
            }
        }
    }

    /// Spherical linear interpolation along the short arc,
    /// `t` in `0..=1`. Inputs are evaluated clip rotations
    /// (unit quaternions).
    pub fn slerp(a: Quat, b: Quat, t: f64) -> Quat {
        let (a, b) = (a.normalized(), b.normalized());
        let mut dot = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
        let mut b = b;
        if dot < 0.0 {
            dot = -dot;
            b = Quat {
                x: -b.x,
                y: -b.y,
                z: -b.z,
                w: -b.w,
            };
        }
        if dot > 0.9995 {
            // Near-parallel: linear blend is indistinguishable
            // and avoids the sin(0) singularity.
            return Quat {
                x: a.x + t * (b.x - a.x),
                y: a.y + t * (b.y - a.y),
                z: a.z + t * (b.z - a.z),
                w: a.w + t * (b.w - a.w),
            }
            .normalized();
        }
        let theta = dot.clamp(-1.0, 1.0).acos();
        let sin_theta = theta.sin();
        let wa = ((1.0 - t) * theta).sin() / sin_theta;
        let wb = (t * theta).sin() / sin_theta;
        Quat {
            x: wa * a.x + wb * b.x,
            y: wa * a.y + wb * b.y,
            z: wa * a.z + wb * b.z,
            w: wa * a.w + wb * b.w,
        }
    }

    /// Rotate a vector by this (unit) quaternion.
    pub fn rotate(self, v: V3) -> V3 {
        // v' = v + 2 * cross(q.xyz, cross(q.xyz, v) + w * v)
        let qv = V3::new(self.x, self.y, self.z);
        let inner = V3::new(
            qv.y * v.z - qv.z * v.y,
            qv.z * v.x - qv.x * v.z,
            qv.x * v.y - qv.y * v.x,
        )
        .plus(v.scale(self.w));
        let outer = V3::new(
            qv.y * inner.z - qv.z * inner.y,
            qv.z * inner.x - qv.x * inner.z,
            qv.x * inner.y - qv.y * inner.x,
        );
        v.plus(outer.scale(2.0))
    }
}

/// A 4x4 matrix, column-major, f64.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    pub m: [f64; 16],
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    /// Compose `T * R * S` from a glTF-style TRS.
    pub fn from_trs(translation: [f64; 3], rotation: [f64; 4], scale: [f64; 3]) -> Mat4 {
        let q = Quat::from_array(rotation).normalized();
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let (wx, wy, wz) = (w * x, w * y, w * z);
        // Rotation rows (row-major view), then scale columns.
        let r = [
            [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
            [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
            [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
        ];
        let mut m = [0.0_f64; 16];
        for col in 0..3 {
            for row in 0..3 {
                m[col * 4 + row] = r[row][col] * scale[col];
            }
        }
        m[12] = translation[0];
        m[13] = translation[1];
        m[14] = translation[2];
        m[15] = 1.0;
        Mat4 { m }
    }

    /// Build from row-major rows (the layout Blender matrices
    /// arrive in through the probe dump).
    pub fn from_rows(rows: [[f64; 4]; 4]) -> Mat4 {
        let mut m = [0.0_f64; 16];
        for row in 0..4 {
            for col in 0..4 {
                m[col * 4 + row] = rows[row][col];
            }
        }
        Mat4 { m }
    }

    /// `self * other` (apply `other` first).
    pub fn multiply(self, other: Mat4) -> Mat4 {
        let mut out = [0.0_f64; 16];
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += self.m[k * 4 + row] * other.m[col * 4 + k];
                }
                out[col * 4 + row] = sum;
            }
        }
        Mat4 { m: out }
    }

    /// Transform a point (affine; the scenes in this crate
    /// never carry a projective w).
    pub fn transform_point(self, p: V3) -> V3 {
        V3::new(
            self.m[0] * p.x + self.m[4] * p.y + self.m[8] * p.z + self.m[12],
            self.m[1] * p.x + self.m[5] * p.y + self.m[9] * p.z + self.m[13],
            self.m[2] * p.x + self.m[6] * p.y + self.m[10] * p.z + self.m[14],
        )
    }

    /// Transform a direction by the upper 3x3 (no translation).
    pub fn transform_vector(self, v: V3) -> V3 {
        V3::new(
            self.m[0] * v.x + self.m[4] * v.y + self.m[8] * v.z,
            self.m[1] * v.x + self.m[5] * v.y + self.m[9] * v.z,
            self.m[2] * v.x + self.m[6] * v.y + self.m[10] * v.z,
        )
    }

    /// Full inverse (adjugate method). Asserts the matrix
    /// is non-singular — the callers invert camera world
    /// matrices, which are rigid transforms.
    pub fn inverse(self) -> Mat4 {
        let m = &self.m;
        // Cofactors of the 2x2 blocks (column-major storage).
        let b00 = m[0] * m[5] - m[1] * m[4];
        let b01 = m[0] * m[6] - m[2] * m[4];
        let b02 = m[0] * m[7] - m[3] * m[4];
        let b03 = m[1] * m[6] - m[2] * m[5];
        let b04 = m[1] * m[7] - m[3] * m[5];
        let b05 = m[2] * m[7] - m[3] * m[6];
        let b06 = m[8] * m[13] - m[9] * m[12];
        let b07 = m[8] * m[14] - m[10] * m[12];
        let b08 = m[8] * m[15] - m[11] * m[12];
        let b09 = m[9] * m[14] - m[10] * m[13];
        let b10 = m[9] * m[15] - m[11] * m[13];
        let b11 = m[10] * m[15] - m[11] * m[14];
        let det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
        assert!(det.abs() > 1e-15, "singular matrix has no inverse");
        let inv_det = 1.0 / det;
        let mut out = [0.0_f64; 16];
        out[0] = (m[5] * b11 - m[6] * b10 + m[7] * b09) * inv_det;
        out[1] = (m[2] * b10 - m[1] * b11 - m[3] * b09) * inv_det;
        out[2] = (m[13] * b05 - m[14] * b04 + m[15] * b03) * inv_det;
        out[3] = (m[10] * b04 - m[9] * b05 - m[11] * b03) * inv_det;
        out[4] = (m[6] * b08 - m[4] * b11 - m[7] * b07) * inv_det;
        out[5] = (m[0] * b11 - m[2] * b08 + m[3] * b07) * inv_det;
        out[6] = (m[14] * b02 - m[12] * b05 - m[15] * b01) * inv_det;
        out[7] = (m[8] * b05 - m[10] * b02 + m[11] * b01) * inv_det;
        out[8] = (m[4] * b10 - m[5] * b08 + m[7] * b06) * inv_det;
        out[9] = (m[1] * b08 - m[0] * b10 - m[3] * b06) * inv_det;
        out[10] = (m[12] * b04 - m[13] * b02 + m[15] * b00) * inv_det;
        out[11] = (m[9] * b02 - m[8] * b04 - m[11] * b00) * inv_det;
        out[12] = (m[5] * b07 - m[6] * b06 - m[4] * b09) * inv_det;
        out[13] = (m[0] * b09 - m[1] * b07 + m[2] * b06) * inv_det;
        out[14] = (m[13] * b01 - m[12] * b03 - m[14] * b00) * inv_det;
        out[15] = (m[8] * b03 - m[9] * b01 + m[10] * b00) * inv_det;
        Mat4 { m: out }
    }

    pub fn translation(self) -> V3 {
        V3::new(self.m[12], self.m[13], self.m[14])
    }

    /// Column-major f32 for GPU upload.
    pub fn to_f32_cols(self) -> [f32; 16] {
        let mut out = [0.0_f32; 16];
        for (dst, src) in out.iter_mut().zip(self.m.iter()) {
            *dst = *src as f32;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn trs_identity_is_identity() {
        let m = Mat4::from_trs([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]);
        assert_eq!(m, Mat4::IDENTITY);
    }

    #[test]
    fn trs_translation_moves_points_not_vectors() {
        let m = Mat4::from_trs([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]);
        assert_eq!(m.transform_point(V3::ZERO), V3::new(1.0, 2.0, 3.0));
        assert_eq!(
            m.transform_vector(V3::new(0.0, 0.0, 1.0)),
            V3::new(0.0, 0.0, 1.0)
        );
    }

    #[test]
    fn quat_rotate_quarter_turn_about_z() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let q = Quat::from_array([0.0, 0.0, s, s]);
        let v = q.rotate(V3::new(1.0, 0.0, 0.0));
        assert!(
            close(v.x, 0.0) && close(v.y, 1.0) && close(v.z, 0.0),
            "{v:?}"
        );
    }

    #[test]
    fn slerp_endpoints_and_midpoint() {
        let a = Quat::IDENTITY;
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let b = Quat::from_array([0.0, 0.0, s, s]); // 90 deg about Z
        assert_eq!(
            Quat::slerp(a, b, 0.0).rotate(V3::new(1.0, 0.0, 0.0)),
            V3::new(1.0, 0.0, 0.0)
        );
        let mid = Quat::slerp(a, b, 0.5).rotate(V3::new(1.0, 0.0, 0.0));
        assert!(close(mid.x, s) && close(mid.y, s), "{mid:?}");
        let end = Quat::slerp(a, b, 1.0).rotate(V3::new(1.0, 0.0, 0.0));
        assert!(close(end.x, 0.0) && close(end.y, 1.0), "{end:?}");
    }

    #[test]
    fn mat4_mul_applies_right_operand_first() {
        let translate = Mat4::from_trs([10.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]);
        let scale = Mat4::from_trs([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [2.0, 2.0, 2.0]);
        let out = translate
            .multiply(scale)
            .transform_point(V3::new(1.0, 0.0, 0.0));
        assert_eq!(out, V3::new(12.0, 0.0, 0.0));
    }

    // Adversarial: degenerate inputs must not produce NaNs.
    #[test]
    fn degenerate_inputs_stay_finite() {
        assert_eq!(V3::ZERO.normalized(), V3::ZERO);
        let q = Quat::slerp(Quat::IDENTITY, Quat::IDENTITY, 0.5);
        assert_eq!(q, Quat::IDENTITY);
        let zero_quat = Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        }
        .normalized();
        assert_eq!(zero_quat, Quat::IDENTITY);
    }
}
