//! Rigid rotation of splat layers (splat programme
//! Rung 3): one rotation R applied consistently to
//! every part of a Gaussian —
//!
//! - centre: `x' = R * x`;
//! - covariance: `Sigma' = R * Sigma * R^T`, carried
//!   by the quaternion product `q' = q_R * q_splat`
//!   (the splat's local frame rides the rotation; its
//!   scales are untouched);
//! - colour: the real-SH field `f(d)` becomes
//!   `f'(d) = f(R^T d)`, i.e. `f'(R d) = f(d)`. Band 0
//!   is invariant (left bit-exact); bands 1–3 are
//!   multiplied by the real Wigner-D matrices of R.
//!
//! The Wigner-D matrices are built from R by the
//! Ivanic–Ruedenberg recurrence (J. Ivanic, K.
//! Ruedenberg, "Rotation Matrices for Real Spherical
//! Harmonics. Direct Determination by Recursion",
//! J. Phys. Chem. 100(15):6342–6347, 1996; erratum
//! J. Phys. Chem. A 102(45):9099, 1998): band 1 is a
//! re-indexing of R itself, band l is built from band
//! l−1 and band 1. That recurrence is stated for the
//! real basis without the Condon–Shortley phase; the
//! 3DGS reference basis (`splat::sh_colour`) carries
//! the phase, i.e. its basis function (l, m) is
//! `(-1)^m` times the phase-free one. The matrix in
//! the 3DGS basis is therefore
//! `D_3dgs(m, n) = (-1)^(m+n) * D(m, n)` — a pure sign
//! change, exact in floating point.
//!
//! Everything is f64 and a pure function of R: no
//! trigonometry (R enters only through its entries),
//! no iteration-order dependence. Type-free on purpose
//! (plain arrays in, plain arrays out) so the module's
//! unit gates stand without the renderer around them.

/// Coefficients per channel above band 0 (bands 1–3:
/// 3 + 5 + 7). Mirrors `splat::SH_REST_PER_CHANNEL`.
pub const REST_PER_CHANNEL: usize = 15;
/// Highest SH band rotated.
const MAX_BAND: usize = 3;
/// Padded band-matrix side (2 * MAX_BAND + 1).
const SIDE: usize = 2 * MAX_BAND + 1;
/// Offset of band l's first coefficient within a
/// channel's 15 (band 1 at 0, band 2 at 3, band 3 at 8).
const BAND_OFFSET: [usize; MAX_BAND + 1] = [0, 0, 3, 8];

/// The identity rotation, `(w, x, y, z)`.
pub const IDENTITY_QUAT: [f64; 4] = [1.0, 0.0, 0.0, 0.0];

/// Hamilton product `a * b`, both `(w, x, y, z)`.
pub fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [aw, ax, ay, az] = a;
    let [w, x, y, z] = b;
    [
        aw * w - ax * x - ay * y - az * z,
        aw * x + ax * w + ay * z - az * y,
        aw * y - ax * z + ay * w + az * x,
        aw * z + ax * y - ay * x + az * w,
    ]
}

/// Rotation matrix (row-major, `r[row][col]`) of a
/// UNIT quaternion `(w, x, y, z)` — the same closed
/// form the splat covariance uses.
pub fn quat_to_matrix(q: [f64; 4]) -> [[f64; 3]; 3] {
    let [w, x, y, z] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// True when `q` is exactly the identity rotation
/// (either double-cover representative).
pub fn is_identity_quat(q: [f64; 4]) -> bool {
    q[1] == 0.0 && q[2] == 0.0 && q[3] == 0.0 && (q[0] == 1.0 || q[0] == -1.0)
}

/// A rotation in both forms the splat path needs: the
/// unit quaternion (for splat frames) and the matrix
/// (for centres and the Wigner-D construction).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    /// Unit quaternion `(w, x, y, z)`.
    pub quat: [f64; 4],
    /// Row-major rotation matrix of `quat`.
    pub matrix: [[f64; 3]; 3],
}

impl Rotation {
    /// From any finite, non-zero quaternion
    /// `(w, x, y, z)`; normalised here. `None` for a
    /// zero or non-finite input (no fallback rotation
    /// is ever invented).
    pub fn from_quaternion(q: [f64; 4]) -> Option<Rotation> {
        let norm = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        if !(norm.is_finite() && norm > 0.0) {
            return None;
        }
        let quat = [q[0] / norm, q[1] / norm, q[2] / norm, q[3] / norm];
        Some(Rotation {
            quat,
            matrix: quat_to_matrix(quat),
        })
    }

    /// A rotation whose matrix is supplied exactly —
    /// the signed-permutation axis maps, whose entries
    /// are exactly 0 / ±1 but whose quaternions (e.g.
    /// ±1/√2 components) do not reproduce those entries
    /// exactly through [`quat_to_matrix`]. The caller
    /// guarantees the pair agree; the unit gates check
    /// every such pair to 1e-15.
    pub const fn exact(quat: [f64; 4], matrix: [[f64; 3]; 3]) -> Rotation {
        Rotation { quat, matrix }
    }

    /// Apply to a point: `R * p`.
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let r = &self.matrix;
        [
            r[0][0] * p[0] + r[0][1] * p[1] + r[0][2] * p[2],
            r[1][0] * p[0] + r[1][1] * p[1] + r[1][2] * p[2],
            r[2][0] * p[0] + r[2][1] * p[1] + r[2][2] * p[2],
        ]
    }
}

/// The real Wigner-D matrices of one rotation for SH
/// bands 1–3, in the 3DGS reference basis. Band l's
/// matrix lives in `bands[l]`, element (m, n) at
/// `[m + l][n + l]`; unused padding is zero.
#[derive(Debug, Clone, PartialEq)]
pub struct ShRotation {
    bands: [[[f64; SIDE]; SIDE]; MAX_BAND + 1],
}

/// Band l−1 and band 1 (phase-free basis), indexed by
/// signed (m, n).
struct Recurrence<'a> {
    band1: &'a [[f64; SIDE]; SIDE],
    prev: &'a [[f64; SIDE]; SIDE],
}

impl Recurrence<'_> {
    fn r1(&self, i: i32, j: i32) -> f64 {
        self.band1[(i + 1) as usize][(j + 1) as usize]
    }

    /// Band l−1 element (a, b); |a|, |b| <= l−1.
    fn rp(&self, l: i32, a: i32, b: i32) -> f64 {
        let lp = l - 1;
        debug_assert!(a.abs() <= lp && b.abs() <= lp);
        self.prev[(a + lp) as usize][(b + lp) as usize]
    }

    /// The recurrence's P function (Ivanic–Ruedenberg
    /// Table 2), i ∈ {−1, 0, 1}, |a| <= l−1, |b| <= l.
    fn p(&self, i: i32, l: i32, a: i32, b: i32) -> f64 {
        if b == l {
            self.r1(i, 1) * self.rp(l, a, l - 1) - self.r1(i, -1) * self.rp(l, a, -l + 1)
        } else if b == -l {
            self.r1(i, 1) * self.rp(l, a, -l + 1) + self.r1(i, -1) * self.rp(l, a, l - 1)
        } else {
            self.r1(i, 0) * self.rp(l, a, b)
        }
    }

    fn u(&self, l: i32, m: i32, n: i32) -> f64 {
        self.p(0, l, m, n)
    }

    fn v(&self, l: i32, m: i32, n: i32) -> f64 {
        if m == 0 {
            self.p(1, l, 1, n) + self.p(-1, l, -1, n)
        } else if m > 0 {
            let d: f64 = if m == 1 { 1.0 } else { 0.0 };
            let first = self.p(1, l, m - 1, n) * (1.0 + d).sqrt();
            // (1 - d) is zero at m == 1: skip the term
            // rather than index outside band l−1.
            if m == 1 {
                first
            } else {
                first - self.p(-1, l, -m + 1, n)
            }
        } else {
            let d: f64 = if m == -1 { 1.0 } else { 0.0 };
            let second = self.p(-1, l, -m - 1, n) * (1.0 + d).sqrt();
            if m == -1 {
                second
            } else {
                self.p(1, l, m + 1, n) + second
            }
        }
    }

    fn w(&self, l: i32, m: i32, n: i32) -> f64 {
        debug_assert!(m != 0);
        if m > 0 {
            self.p(1, l, m + 1, n) + self.p(-1, l, -m - 1, n)
        } else {
            self.p(1, l, m - 1, n) - self.p(-1, l, -m + 1, n)
        }
    }

    /// Element (m, n) of band l (phase-free basis).
    fn element(&self, l: i32, m: i32, n: i32) -> f64 {
        let d = if m == 0 { 1.0 } else { 0.0 };
        let am = f64::from(m.abs());
        let lf = f64::from(l);
        let denom = if n.abs() == l {
            (2.0 * lf) * (2.0 * lf - 1.0)
        } else {
            (lf + f64::from(n)) * (lf - f64::from(n))
        };
        let u = ((lf + f64::from(m)) * (lf - f64::from(m)) / denom).sqrt();
        let v = 0.5 * ((1.0 + d) * (lf + am - 1.0) * (lf + am) / denom).sqrt() * (1.0 - 2.0 * d);
        let w = -0.5 * ((lf - am - 1.0) * (lf - am) / denom).sqrt() * (1.0 - d);
        // Each term is evaluated only where its
        // coefficient is non-zero: the zero-coefficient
        // terms index outside band l−1 by construction.
        let mut out = 0.0;
        if u != 0.0 {
            out += u * self.u(l, m, n);
        }
        if v != 0.0 {
            out += v * self.v(l, m, n);
        }
        if w != 0.0 {
            out += w * self.w(l, m, n);
        }
        out
    }
}

impl ShRotation {
    /// Build bands 1–3 for rotation matrix `r`
    /// (row-major; must be a proper rotation — the
    /// recurrence assumes orthogonality, det +1).
    pub fn new(r: &[[f64; 3]; 3]) -> ShRotation {
        let mut phase_free = [[[0.0_f64; SIDE]; SIDE]; MAX_BAND + 1];
        // Band 0 is the scalar 1 (unused by the rest
        // coefficients; kept for completeness).
        phase_free[0][0][0] = 1.0;
        // Band 1, basis order (m = −1, 0, 1) = (y, z, x):
        // D1(m, n) = R[axis(m)][axis(n)].
        let axis = |m: usize| [1_usize, 2, 0][m];
        for m in 0..3 {
            for n in 0..3 {
                phase_free[1][m][n] = r[axis(m)][axis(n)];
            }
        }
        for l in 2..=MAX_BAND {
            let (lower, upper) = phase_free.split_at_mut(l);
            let rec = Recurrence {
                band1: &lower[1],
                prev: &lower[l - 1],
            };
            let li = l as i32;
            for m in -li..=li {
                for n in -li..=li {
                    upper[0][(m + li) as usize][(n + li) as usize] = rec.element(li, m, n);
                }
            }
        }
        // Into the 3DGS basis: D(m, n) * (-1)^(m + n).
        let mut bands = phase_free;
        for (l, band) in bands.iter_mut().enumerate() {
            let li = l as i32;
            for m in -li..=li {
                for n in -li..=li {
                    if (m + n).rem_euclid(2) == 1 {
                        let e = &mut band[(m + li) as usize][(n + li) as usize];
                        *e = -*e;
                    }
                }
            }
        }
        ShRotation { bands }
    }

    /// Band l's matrix element (m, n), 3DGS basis.
    pub fn element(&self, l: usize, m: i32, n: i32) -> f64 {
        assert!((1..=MAX_BAND).contains(&l), "band {l} out of range");
        let li = l as i32;
        assert!(m.abs() <= li && n.abs() <= li, "index out of band {l}");
        self.bands[l][(m + li) as usize][(n + li) as usize]
    }

    /// Rotate one channel's band 1–3 coefficients
    /// (3DGS layout: band 1 at 0..3, band 2 at 3..8,
    /// band 3 at 8..15, each in m = −l..=l order):
    /// `c'[m] = sum_n D(m, n) * c[n]`, so the rotated
    /// field evaluated at `R d` equals the original
    /// at `d`.
    /// The band matrices flattened for the GPU
    /// projection's parameter block: band 1's 3x3 at
    /// `[0..9]`, band 2's 5x5 at `[9..34]`, band 3's
    /// 7x7 at `[34..83]`, each row-major in the
    /// (m, n) indexing `rotate_channel` applies.
    pub fn matrices_flat(&self) -> [f64; 83] {
        let mut out = [0.0_f64; 83];
        for (band, side, offset) in [(1_usize, 3_usize, 0_usize), (2, 5, 9), (3, 7, 34)] {
            for m in 0..side {
                for n in 0..side {
                    out[offset + m * side + n] = self.bands[band][m][n];
                }
            }
        }
        out
    }

    pub fn rotate_channel(&self, c: &[f64; REST_PER_CHANNEL]) -> [f64; REST_PER_CHANNEL] {
        let mut out = [0.0_f64; REST_PER_CHANNEL];
        for l in 1..=MAX_BAND {
            let size = 2 * l + 1;
            let base = BAND_OFFSET[l];
            for m in 0..size {
                let mut acc = 0.0;
                for n in 0..size {
                    acc += self.bands[l][m][n] * c[base + n];
                }
                out[base + m] = acc;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The 3DGS reference basis, transcribed from
    // `splat::sh_colour` (same constants, same order).
    const C1: f64 = 0.488_602_511_902_919_9;
    const C2: [f64; 5] = [
        1.092_548_430_592_079_2,
        -1.092_548_430_592_079_2,
        0.315_391_565_252_52,
        -1.092_548_430_592_079_2,
        0.546_274_215_296_039_6,
    ];
    const C3: [f64; 7] = [
        -0.590_043_589_926_643_5,
        2.890_611_442_640_554,
        -0.457_045_799_464_465_8,
        0.373_176_332_590_115_4,
        -0.457_045_799_464_465_8,
        1.445_305_721_320_277,
        -0.590_043_589_926_643_5,
    ];

    /// Basis values for bands 1–3 at unit direction d.
    fn basis(d: [f64; 3]) -> [f64; REST_PER_CHANNEL] {
        let (x, y, z) = (d[0], d[1], d[2]);
        let (xx, yy, zz) = (x * x, y * y, z * z);
        [
            -C1 * y,
            C1 * z,
            -C1 * x,
            C2[0] * x * y,
            C2[1] * y * z,
            C2[2] * (2.0 * zz - xx - yy),
            C2[3] * x * z,
            C2[4] * (xx - yy),
            C3[0] * y * (3.0 * xx - yy),
            C3[1] * x * y * z,
            C3[2] * y * (4.0 * zz - xx - yy),
            C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy),
            C3[4] * x * (4.0 * zz - xx - yy),
            C3[5] * z * (xx - yy),
            C3[6] * x * (xx - 3.0 * yy),
        ]
    }

    fn eval(c: &[f64; REST_PER_CHANNEL], d: [f64; 3]) -> f64 {
        basis(d).iter().zip(c).map(|(b, k)| b * k).sum()
    }

    /// Band-l slice of the field only.
    fn eval_band(c: &[f64; REST_PER_CHANNEL], d: [f64; 3], l: usize) -> f64 {
        let b = basis(d);
        let base = BAND_OFFSET[l];
        (base..base + 2 * l + 1).map(|i| b[i] * c[i]).sum()
    }

    /// splitmix64: deterministic test inputs.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        }
        fn unit(&mut self) -> [f64; 3] {
            loop {
                let v = [self.next(), self.next(), self.next()];
                let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if n > 0.1 && n <= 1.0 {
                    return [v[0] / n, v[1] / n, v[2] / n];
                }
            }
        }
        fn rotation(&mut self) -> Rotation {
            Rotation::from_quaternion([self.next(), self.next(), self.next(), self.next()])
                .expect("non-zero")
        }
        fn coeffs(&mut self) -> [f64; REST_PER_CHANNEL] {
            let mut c = [0.0; REST_PER_CHANNEL];
            for k in &mut c {
                *k = self.next();
            }
            c
        }
    }

    /// The bound every "f64 tolerance" gate below uses,
    /// stated before the runs: 1e-12 absolute on
    /// fields with O(1) coefficients (≈ 4500 ulps of
    /// 1.0 — room for the recurrence's accumulated
    /// rounding over three bands, tight enough that any
    /// sign or index slip, which errs at O(0.1), fails).
    const F64_TOL: f64 = 1e-12;

    fn axis_angle(axis: [f64; 3], angle: f64) -> Rotation {
        let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        let s = (angle / 2.0).sin() / n;
        Rotation::from_quaternion([(angle / 2.0).cos(), axis[0] * s, axis[1] * s, axis[2] * s])
            .expect("non-zero")
    }

    // -- Gate 2: known-answer SH rotation ---------------

    #[test]
    fn band1_known_answer_rotation() {
        // 90° about +Z maps x -> y. A band-1 field that
        // is "C1 * x" (coefficient −1 on the third basis
        // function, −C1 * x) rotated by it must become
        // "C1 * y": coefficients (−1, 0, 0) on (−C1 y).
        let r = axis_angle([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2);
        let sh = ShRotation::new(&r.matrix);
        let mut c = [0.0; REST_PER_CHANNEL];
        c[2] = -1.0; // field = C1 * x
        let out = sh.rotate_channel(&c);
        let want = [-1.0, 0.0, 0.0];
        for i in 0..3 {
            assert!((out[i] - want[i]).abs() < F64_TOL, "{out:?}");
        }
        // And the defining property at sample points.
        let mut rng = Rng(7);
        for _ in 0..64 {
            let d = rng.unit();
            let rd = r.apply(d);
            assert!((eval(&out, rd) - eval(&c, d)).abs() < F64_TOL);
        }
    }

    #[test]
    fn rotated_field_at_rd_equals_original_at_d_bands_1_to_3() {
        // The gate's statement, per band and for the
        // full field: f'(R d) == f(d) to f64 tolerance,
        // over random rotations, coefficients and
        // directions (seeded: same inputs every run).
        let mut rng = Rng(0x5EED_0003);
        let mut worst = 0.0_f64;
        for _ in 0..200 {
            let r = rng.rotation();
            let sh = ShRotation::new(&r.matrix);
            let c = rng.coeffs();
            let out = sh.rotate_channel(&c);
            for _ in 0..16 {
                let d = rng.unit();
                let rd = r.apply(d);
                for l in 1..=3 {
                    let e = (eval_band(&out, rd, l) - eval_band(&c, d, l)).abs();
                    worst = worst.max(e);
                    assert!(e < F64_TOL, "band {l}: residual {e:e}");
                }
                let e = (eval(&out, rd) - eval(&c, d)).abs();
                worst = worst.max(e);
                assert!(e < F64_TOL, "full field: residual {e:e}");
            }
        }
        println!("gate2 worst |f'(Rd) - f(d)| over 200 rotations x 16 dirs: {worst:e}");
    }

    #[test]
    fn known_axis_rotations_match_closed_forms() {
        // Hand-derived: 180° about Z sends (x, y, z) ->
        // (−x, −y, z); every 3DGS basis function is
        // then ±itself, sign = parity of its x/y degree.
        let r = axis_angle([0.0, 0.0, 1.0], std::f64::consts::PI);
        let sh = ShRotation::new(&r.matrix);
        // Band 1 (−C1 y, C1 z, −C1 x): (−, +, −).
        // Band 2 (xy, yz, z², xz, x²−y²): (+, −, +, −, +).
        // Band 3 (y(3x²−y²), xyz, y(..), z(..), x(..),
        //         z(x²−y²), x(x²−3y²)): (−, +, −, +, −, +, −).
        let signs: [f64; 15] = [
            -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0,
        ];
        for (l, base) in [(1_usize, 0_usize), (2, 3), (3, 8)] {
            let li = l as i32;
            for m in -li..=li {
                for n in -li..=li {
                    let want = if m == n {
                        signs[base + (m + li) as usize]
                    } else {
                        0.0
                    };
                    let got = sh.element(l, m, n);
                    assert!((got - want).abs() < F64_TOL, "l={l} ({m},{n}) {got}");
                }
            }
        }
    }

    #[test]
    fn wigner_matrices_are_orthogonal_and_compose() {
        // Representation properties: D(R) orthogonal,
        // D(R2 R1) = D(R2) D(R1). A wrong sign or index
        // in the recurrence breaks one or the other.
        let mut rng = Rng(99);
        for _ in 0..50 {
            let r1 = rng.rotation();
            let r2 = rng.rotation();
            let r21 = Rotation::from_quaternion(quat_mul(r2.quat, r1.quat)).expect("unit");
            let (d1, d2, d21) = (
                ShRotation::new(&r1.matrix),
                ShRotation::new(&r2.matrix),
                ShRotation::new(&r21.matrix),
            );
            for l in 1..=3_usize {
                let li = l as i32;
                for m in -li..=li {
                    for n in -li..=li {
                        let dot: f64 = (-li..=li)
                            .map(|k| d1.element(l, k, m) * d1.element(l, k, n))
                            .sum();
                        let want = if m == n { 1.0 } else { 0.0 };
                        assert!((dot - want).abs() < F64_TOL, "orthogonality l={l}");
                        let prod: f64 = (-li..=li)
                            .map(|k| d2.element(l, m, k) * d1.element(l, k, n))
                            .sum();
                        assert!(
                            (prod - d21.element(l, m, n)).abs() < F64_TOL,
                            "composition l={l} ({m},{n})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn independent_least_squares_fit_agrees() {
        // Adversarial cross-derivation that shares no
        // code with the recurrence: for each basis
        // function b_n, sample g(d) = b_n(R^T d) at 32
        // directions and solve the normal equations for
        // its expansion in the basis. Column n of that
        // fit is column n of D. (f'(d) = f(R^T d).)
        let mut rng = Rng(4242);
        let r = rng.rotation();
        let rt = [
            [r.matrix[0][0], r.matrix[1][0], r.matrix[2][0]],
            [r.matrix[0][1], r.matrix[1][1], r.matrix[2][1]],
            [r.matrix[0][2], r.matrix[1][2], r.matrix[2][2]],
        ];
        let rtd = |d: [f64; 3]| Rotation::exact([1.0, 0.0, 0.0, 0.0], rt).apply(d);
        let sh = ShRotation::new(&r.matrix);
        let dirs: Vec<[f64; 3]> = (0..32).map(|_| rng.unit()).collect();
        for (l, base) in [(1_usize, 0_usize), (2, 3), (3, 8)] {
            let size = 2 * l + 1;
            // Normal matrix A^T A (size x size).
            let mut ata = vec![vec![0.0; size]; size];
            for d in &dirs {
                let b = basis(*d);
                for i in 0..size {
                    for j in 0..size {
                        ata[i][j] += b[base + i] * b[base + j];
                    }
                }
            }
            for n in 0..size {
                let mut atg = vec![0.0; size];
                for d in &dirs {
                    let b = basis(*d);
                    let g = basis(rtd(*d))[base + n];
                    for i in 0..size {
                        atg[i] += b[base + i] * g;
                    }
                }
                let x = solve(ata.clone(), atg);
                for (m, xm) in x.iter().enumerate() {
                    let want = sh.element(l, m as i32 - l as i32, n as i32 - l as i32);
                    assert!(
                        (xm - want).abs() < 1e-10,
                        "fit l={l} ({m},{n}) {xm} vs {want}"
                    );
                }
            }
        }
    }

    /// Gaussian elimination with partial pivoting
    /// (test-only; small dense systems).
    fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
        let n = b.len();
        for col in 0..n {
            let pivot = (col..n)
                .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))
                .expect("rows");
            a.swap(col, pivot);
            b.swap(col, pivot);
            for row in col + 1..n {
                let f = a[row][col] / a[col][col];
                for k in col..n {
                    a[row][k] -= f * a[col][k];
                }
                b[row] -= f * b[col];
            }
        }
        let mut x = vec![0.0; n];
        for row in (0..n).rev() {
            let s: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
            x[row] = (b[row] - s) / a[row][row];
        }
        x
    }

    #[test]
    fn identity_rotation_matrices_are_exactly_identity() {
        // Gate 1's foundation: at R = I every Wigner
        // matrix is the identity BIT-exactly, so the
        // general path cannot perturb a coefficient.
        let sh = ShRotation::new(&quat_to_matrix(IDENTITY_QUAT));
        for l in 1..=3_usize {
            let li = l as i32;
            for m in -li..=li {
                for n in -li..=li {
                    let want = if m == n { 1.0_f64 } else { 0.0 };
                    let got = sh.element(l, m, n);
                    assert_eq!(
                        got.abs().to_bits(),
                        want.to_bits(),
                        "l={l} ({m},{n}) {got:e}"
                    );
                }
            }
        }
    }

    #[test]
    fn signed_permutation_axis_maps_are_consistent() {
        // The axis maps' exact matrices agree with their
        // quaternions; band 1 is then exactly a signed
        // permutation, and the defining property holds.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let maps = [
            Rotation::exact(
                [0.0, 1.0, 0.0, 0.0],
                [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
            ),
            Rotation::exact(
                [h, -h, 0.0, 0.0],
                [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]],
            ),
        ];
        let mut rng = Rng(11);
        for map in maps {
            let from_q = quat_to_matrix(map.quat);
            for i in 0..3 {
                for j in 0..3 {
                    assert!((from_q[i][j] - map.matrix[i][j]).abs() < 1e-15);
                }
            }
            let sh = ShRotation::new(&map.matrix);
            for m in -1..=1 {
                for n in -1..=1 {
                    let e = sh.element(1, m, n);
                    assert!(e == 0.0 || e == 1.0 || e == -1.0, "band 1 exact: {e}");
                }
            }
            let c = rng.coeffs();
            let out = sh.rotate_channel(&c);
            for _ in 0..16 {
                let d = rng.unit();
                assert!((eval(&out, map.apply(d)) - eval(&c, d)).abs() < F64_TOL);
            }
        }
    }

    #[test]
    fn quaternion_double_cover_gives_the_same_matrices() {
        let mut rng = Rng(5);
        let r = rng.rotation();
        let neg = Rotation::from_quaternion(r.quat.map(|v| -v)).expect("non-zero");
        let (a, b) = (ShRotation::new(&r.matrix), ShRotation::new(&neg.matrix));
        for l in 1..=3_usize {
            let li = l as i32;
            for m in -li..=li {
                for n in -li..=li {
                    assert!((a.element(l, m, n) - b.element(l, m, n)).abs() < F64_TOL);
                }
            }
        }
    }

    #[test]
    fn degenerate_quaternions_are_refused() {
        assert!(Rotation::from_quaternion([0.0; 4]).is_none());
        assert!(Rotation::from_quaternion([f64::NAN, 0.0, 0.0, 1.0]).is_none());
        assert!(Rotation::from_quaternion([f64::INFINITY, 0.0, 0.0, 0.0]).is_none());
        // Non-unit input is normalised, not refused.
        let r = Rotation::from_quaternion([2.0, 0.0, 0.0, 0.0]).expect("scaled identity");
        assert_eq!(r.quat, IDENTITY_QUAT);
    }
}
