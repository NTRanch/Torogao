//! Counter-based seeded RNG: splitmix64 over a counter, with
//! Box-Muller gaussians. The stream is a pure function of
//! (seed, index) — identical on every machine, backend, and
//! run, which is the root of the determinism contract
//! (ARCH-EVAL section 3). The effect-timeline/1 crate in the
//! NTRanch content factory uses the same splitmix64 core.

/// One splitmix64 step: advances the state and returns the
/// mixed 64-bit value.
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A deterministic gaussian stream keyed by a seed.
pub struct GaussianStream {
    state: u64,
    spare: Option<f32>,
}

impl GaussianStream {
    /// A fresh stream for `seed`.
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed,
            spare: None,
        }
    }

    fn next_uniform(&mut self) -> f64 {
        // Top 53 bits mapped into (0, 1].
        let z = splitmix64(&mut self.state);
        let v = (z >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
        if v <= 0.0 { 1.0 } else { v }
    }

    /// The next standard-normal sample.
    pub fn next_gaussian(&mut self) -> f32 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let u1 = self.next_uniform();
        let u2 = self.next_uniform();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = 2.0 * std::f64::consts::PI * u2;
        self.spare = Some((r * theta.sin()) as f32);
        (r * theta.cos()) as f32
    }

    /// Fill `out` with standard-normal samples in stream order.
    pub fn fill_gaussian(&mut self, out: &mut [f32]) {
        for v in out.iter_mut() {
            *v = self.next_gaussian();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_seed_zero_first_value_is_pinned() {
        let mut s = 0u64;
        // Reference value computed from the defining
        // recurrence; pinning it pins the stream definition.
        assert_eq!(splitmix64(&mut s), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn same_seed_same_stream() {
        let mut a = GaussianStream::new(31001);
        let mut b = GaussianStream::new(31001);
        for _ in 0..1024 {
            assert_eq!(a.next_gaussian().to_bits(), b.next_gaussian().to_bits());
        }
    }

    #[test]
    fn different_seed_different_stream() {
        let mut a = GaussianStream::new(1);
        let mut b = GaussianStream::new(2);
        let mut differ = false;
        for _ in 0..16 {
            if a.next_gaussian().to_bits() != b.next_gaussian().to_bits() {
                differ = true;
            }
        }
        assert!(differ);
    }

    #[test]
    fn stream_moments_are_standard_normal() {
        let mut g = GaussianStream::new(20261020);
        let n = 200_000;
        let mut mean = 0.0f64;
        let mut m2 = 0.0f64;
        for _ in 0..n {
            let v = g.next_gaussian() as f64;
            mean += v;
            m2 += v * v;
        }
        mean /= n as f64;
        let var = m2 / n as f64 - mean * mean;
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((var - 1.0).abs() < 0.02, "var {var}");
    }
}
