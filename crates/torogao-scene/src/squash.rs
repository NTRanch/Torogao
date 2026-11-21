//! The contact channel's drive of the dedicated squash key
//! (Owner ruling D-1415: a baked shape key, driven by contact).
//!
//! The key is baked at one press depth. The channel is how deep the
//! hand presses now; the key's weight is that depth over the baked
//! depth, held to `[0, 1]`. Linear on purpose: the harness can read
//! it back from the model.

use crate::SceneError;

/// Key weight for a press.
///
/// # Errors
/// Non-finite input, or a baked depth that is not above zero.
pub fn weight(press_m: f64, baked_m: f64) -> Result<f64, SceneError> {
    if !press_m.is_finite() {
        return Err(SceneError::NonFinite { field: "press_m" });
    }
    if !baked_m.is_finite() {
        return Err(SceneError::NonFinite { field: "baked_m" });
    }
    if baked_m <= 0.0 {
        return Err(SceneError::BakedDepthNotPositive { baked_m });
    }
    Ok((press_m / baked_m).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- validation ----
    #[test]
    fn weight_is_press_over_baked_depth() {
        assert_eq!(weight(0.006, 0.006), Ok(1.0));
        assert!((weight(0.003, 0.006).unwrap() - 0.5).abs() < 1e-15);
        assert_eq!(weight(0.0, 0.006), Ok(0.0));
    }

    // ---- adversarial ----
    #[test]
    fn no_contact_and_overpress_are_held_to_the_key_range() {
        assert_eq!(weight(-0.01, 0.006), Ok(0.0));
        assert_eq!(weight(0.05, 0.006), Ok(1.0));
    }

    #[test]
    fn bad_input_is_refused() {
        assert_eq!(
            weight(f64::NAN, 0.006),
            Err(SceneError::NonFinite { field: "press_m" })
        );
        assert_eq!(
            weight(0.001, 0.0),
            Err(SceneError::BakedDepthNotPositive { baked_m: 0.0 })
        );
        assert_eq!(
            weight(0.001, f64::INFINITY),
            Err(SceneError::NonFinite { field: "baked_m" })
        );
    }
}
