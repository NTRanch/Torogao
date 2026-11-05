//! Pixel arithmetic: the determinism contract's integer core
//! (SPEC SS3.2). Every blend and composite formula here is
//! exact integer arithmetic on 8-bit channels with u32
//! intermediates and round-half-up at the named division.
//! No float ever touches a pixel.

/// One RGBA8 pixel, channels in memory order.
pub type Rgba = [u8; 4];

/// The fully transparent pixel.
pub const TRANSPARENT: Rgba = [0, 0, 0, 0];

/// `(a * b) / 255` with round-half-up, for `a, b <= 255`.
/// Exact over the whole input domain (proven against the
/// rational definition in the unit tests).
pub fn mul255(a: u32, b: u32) -> u32 {
    debug_assert!(a <= 255 && b <= 255);
    (a * b + 127) / 255
}

/// `(numerator / 255)` with round-half-up, for a numerator
/// already known to be a product bounded by `255 * 255`.
fn div255_round(numerator: u32) -> u32 {
    debug_assert!(numerator <= 255 * 255);
    (numerator + 127) / 255
}

/// The per-channel blend functions (SPEC SS3.1): what colour
/// a source channel contributes over a destination channel
/// before alpha compositing. Each is the textbook definition
/// evaluated in integer arithmetic.
pub fn blend_channel(mode: crate::layer::BlendMode, source: u8, dest: u8) -> u8 {
    let s = u32::from(source);
    let d = u32::from(dest);
    match mode {
        crate::layer::BlendMode::Normal => source,
        crate::layer::BlendMode::Multiply => div255_round(s * d) as u8,
        crate::layer::BlendMode::Overlay => {
            if d < 128 {
                // 2*s*d/255; bounded: d <= 127 keeps the
                // product inside div255_round's domain.
                div255_round(2 * s * d) as u8
            } else {
                255 - div255_round(2 * (255 - s) * (255 - d)) as u8
            }
        }
    }
}

/// Source-over compositing: `source` painted over `dest`.
/// The exported pixel array is always built from this and
/// `composite_blend` on the CPU (SPEC SS3.2.1).
pub fn composite_over(source: Rgba, dest: Rgba) -> Rgba {
    let sa = u32::from(source[3]);
    let da = u32::from(dest[3]);
    let out_a = sa + mul255(da, 255 - sa);
    if out_a == 0 {
        return TRANSPARENT;
    }
    let mut out = [0u8; 4];
    for channel in 0..3 {
        let sc = u32::from(source[channel]);
        let dc = u32::from(dest[channel]);
        // (sc*sa*255 + dc*da*(255-sa)) / (255*out_a), round-half-up.
        let numerator = sc * sa * 255 + dc * da * (255 - sa);
        let denominator = 255 * out_a;
        out[channel] = ((numerator + denominator / 2) / denominator) as u8;
    }
    out[3] = out_a as u8;
    out
}

/// Blend-mode compositing: the source colour is first bent
/// by the layer's blend mode against the destination, then
/// composited source-over at the source's alpha.
pub fn composite_blend(mode: crate::layer::BlendMode, source: Rgba, dest: Rgba) -> Rgba {
    let bent = [
        blend_channel(mode, source[0], dest[0]),
        blend_channel(mode, source[1], dest[1]),
        blend_channel(mode, source[2], dest[2]),
        source[3],
    ];
    composite_over(bent, dest)
}

/// Destination-out: reduce `dest`'s alpha by `strength`
/// (0 keeps the pixel, 255 clears it). Colour channels are
/// preserved while any alpha remains; a fully cleared pixel
/// is transparent black, so the document has one canonical
/// representation of "nothing here".
pub fn erase_strength(dest: Rgba, strength: u8) -> Rgba {
    let da = u32::from(dest[3]);
    let out_a = div255_round(da * (255 - u32::from(strength)));
    if out_a == 0 {
        return TRANSPARENT;
    }
    [dest[0], dest[1], dest[2], out_a as u8]
}

/// Linear interpolation between two colours at strength
/// `s` (0 = dest, 255 = source), per channel including
/// alpha. Used by lock-alpha deposit, where the destination
/// alpha is preserved by construction.
pub fn lerp_rgba(source: Rgba, dest: Rgba, strength: u8) -> Rgba {
    let s = u32::from(strength);
    let mut out = [0u8; 4];
    for channel in 0..4 {
        let sc = u32::from(source[channel]);
        let dc = u32::from(dest[channel]);
        out[channel] = ((sc * s + dc * (255 - s) + 127) / 255) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::BlendMode;

    #[test]
    fn mul255_matches_rational_round_half_up() {
        for a in 0..=255u32 {
            for b in 0..=255u32 {
                let rational = (2 * a * b + 255) / 510;
                assert_eq!(mul255(a, b), rational, "mul255({a}, {b})");
            }
        }
    }

    #[test]
    fn composite_over_known_values() {
        // Opaque source wins completely.
        assert_eq!(
            composite_over([10, 20, 30, 255], [200, 100, 50, 255]),
            [10, 20, 30, 255]
        );
        // Transparent source changes nothing.
        assert_eq!(
            composite_over([10, 20, 30, 0], [200, 100, 50, 255]),
            [200, 100, 50, 255]
        );
        // Half red over opaque blue: exact midpoint, alpha 255.
        assert_eq!(
            composite_over([255, 0, 0, 128], [0, 0, 255, 255]),
            [128, 0, 127, 255]
        );
        // Anything over nothing is itself.
        assert_eq!(composite_over([9, 8, 7, 200], TRANSPARENT), [9, 8, 7, 200]);
        // Nothing over nothing stays the canonical transparent.
        assert_eq!(composite_over(TRANSPARENT, TRANSPARENT), TRANSPARENT);
    }

    #[test]
    fn blend_modes_known_values() {
        // Multiply by white is identity; by black is black.
        assert_eq!(blend_channel(BlendMode::Multiply, 200, 255), 200);
        assert_eq!(blend_channel(BlendMode::Multiply, 200, 0), 0);
        assert_eq!(blend_channel(BlendMode::Multiply, 128, 128), 64);
        // Overlay is identity on the source at the branch
        // boundary from below (dest 127, low branch); dest 128
        // takes the high branch, whose named rounding (round
        // the inner product half-up, then subtract) gives 78.
        assert_eq!(blend_channel(BlendMode::Overlay, 77, 127), 77);
        assert_eq!(blend_channel(BlendMode::Overlay, 77, 128), 78);
        // Overlay extremes.
        assert_eq!(blend_channel(BlendMode::Overlay, 255, 255), 255);
        assert_eq!(blend_channel(BlendMode::Overlay, 0, 0), 0);
        // Normal is the source, always.
        assert_eq!(blend_channel(BlendMode::Normal, 33, 222), 33);
    }

    #[test]
    fn erase_and_lerp_known_values() {
        assert_eq!(erase_strength([10, 20, 30, 200], 0), [10, 20, 30, 200]);
        assert_eq!(erase_strength([10, 20, 30, 200], 255), TRANSPARENT);
        assert_eq!(erase_strength([10, 20, 30, 128], 128), [10, 20, 30, 64]);
        assert_eq!(
            lerp_rgba([255, 0, 0, 255], [0, 0, 255, 255], 255),
            [255, 0, 0, 255]
        );
        assert_eq!(
            lerp_rgba([255, 0, 0, 255], [0, 0, 255, 255], 0),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn adversarial_composite_extremes_never_overflow() {
        // Every alpha pairing at channel extremes: the u32
        // intermediates are sized for exactly this corner.
        for sa in [0u8, 1, 127, 128, 254, 255] {
            for da in [0u8, 1, 127, 128, 254, 255] {
                let out = composite_over([255, 255, 255, sa], [255, 255, 255, da]);
                assert!(out[3] >= sa.max(da) || out[3] == 255 || sa == 0 && out[3] == da);
                let _ = composite_blend(BlendMode::Overlay, [255, 0, 255, sa], [0, 255, 0, da]);
                let _ = erase_strength([255, 255, 255, da], sa);
            }
        }
    }
}
