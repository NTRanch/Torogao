//! Deterministic transcendental math for the simulation
//! path (ANDR-REQ-12; Owner ruling D-231, 2026-10-28).
//!
//! The determinism law hashes evaluated state bit-exactly,
//! so every transcendental the evaluation calls must
//! return identical bits on every platform. The platform
//! libm does not promise that: measured on the A-1 grids
//! (388,652 calls, glibc vs bionic x86_64), f64 sin/cos/tan
//! diverged on ~3% of inputs and f32 atan2 on 13.3% —
//! every divergence exactly 1 ulp (GATE-A1-CURE.md). Those
//! 1-ulp splits are what diverged the Android build's
//! evaluated state from the desktop's at tick 0 (STOP-4).
//!
//! The functions here are the `libm` crate's: pure-Rust
//! ports of the MUSL algorithms, integer and IEEE
//! arithmetic only, no platform libm calls. Rust never
//! contracts a multiply-add unless `mul_add` is named, so
//! the evaluation order — and therefore the result bits —
//! are the same on every target the estate builds for.
//! Provenance: crate `libm` 0.2 (MIT OR Apache-2.0), the
//! Rust project's portable libm, already in this
//! workspace's lockfile as a transitive dependency before
//! this module made it a direct one.
//!
//! Contract:
//! - Simulation code (evaluation, automations, physics,
//!   output mapping) calls ONLY these wrappers for sin,
//!   cos, tan, and atan2. Rendering and tooling may keep
//!   the platform libm; nothing they compute is hashed.
//! - Accuracy is a measured property, not a promise: the
//!   cure gate (GATE-A1-CURE.md) records each wrapper's
//!   divergence rate and maximum ulp distance against
//!   glibc over the A-1 grids. Determinism — the property
//!   the law needs — holds by construction.
//! - `sqrt` is deliberately absent: IEEE 754 requires
//!   correct rounding and both libms lower it to the
//!   hardware instruction; it has never diverged.

/// Deterministic f32 sine (simulation path only).
pub fn sin_f32(x: f32) -> f32 {
    libm::sinf(x)
}

/// Deterministic f32 cosine (simulation path only).
pub fn cos_f32(x: f32) -> f32 {
    libm::cosf(x)
}

/// Deterministic f32 two-argument arctangent
/// (simulation path only).
pub fn atan2_f32(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

/// Deterministic f64 sine (simulation path only).
pub fn sin_f64(x: f64) -> f64 {
    libm::sin(x)
}

/// Deterministic f64 cosine (simulation path only).
pub fn cos_f64(x: f64) -> f64 {
    libm::cos(x)
}

/// Deterministic f64 tangent (simulation path only).
pub fn tan_f64(x: f64) -> f64 {
    libm::tan(x)
}

#[cfg(test)]
mod tests {
    use super::{atan2_f32, cos_f32, cos_f64, sin_f32, sin_f64, tan_f64};

    #[test]
    fn exact_special_values() {
        assert_eq!(sin_f32(0.0).to_bits(), 0.0f32.to_bits());
        assert_eq!(cos_f32(0.0), 1.0);
        assert_eq!(sin_f64(0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(cos_f64(0.0), 1.0);
        assert_eq!(tan_f64(0.0).to_bits(), 0.0f64.to_bits());
        // atan2 axes and signed zeros (the pendulum's
        // rest pose derives its angle through these).
        assert_eq!(atan2_f32(0.0, 1.0), 0.0);
        assert_eq!(atan2_f32(0.0, -1.0), std::f32::consts::PI);
        assert_eq!(atan2_f32(1.0, 0.0), std::f32::consts::FRAC_PI_2);
        assert_eq!(atan2_f32(-1.0, 0.0), -std::f32::consts::FRAC_PI_2);
        assert!(atan2_f32(-0.0, 1.0).is_sign_negative());
    }

    #[test]
    fn pinned_bit_patterns() {
        // Regression pins: these bit patterns are the
        // cross-platform contract. A libm upgrade that
        // moves any of them is a re-baseline event, not a
        // silent drift (values measured at the D-231 cure;
        // see GATE-A1-CURE.md).
        assert_eq!(sin_f32(1.0).to_bits(), 0x3f57_6aa4);
        assert_eq!(cos_f32(1.0).to_bits(), 0x3f0a_5140);
        assert_eq!(atan2_f32(0.6, -0.8).to_bits(), 0x401f_e0bc);
        assert_eq!(sin_f64(1.0).to_bits(), 0x3fea_ed54_8f09_0cee);
        assert_eq!(cos_f64(1.0).to_bits(), 0x3fe1_4a28_0fb5_068c);
        assert_eq!(tan_f64(1.0).to_bits(), 0x3ff8_eb24_5cbe_e3a6);
    }

    #[test]
    fn identities_hold_to_rounding() {
        // sin^2 + cos^2 == 1 within a few ulp across the
        // automation's working range; a broken range
        // reduction would blow far past this.
        for k in 0..1000 {
            let x = (k as f64) * 0.01;
            let s = sin_f64(x);
            let c = cos_f64(x);
            assert!((s * s + c * c - 1.0).abs() < 1e-14, "x={x}");
        }
    }
}
