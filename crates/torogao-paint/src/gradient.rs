//! Gradient fields and gradient fill (SPEC SS5 Phase 2
//! extension): the continuous-tonal term the flat
//! vocabulary lacked. A gradient is a pure function of
//! canvas position — multi-stop, linear or radial —
//! evaluated in exact integer arithmetic (fixed-point
//! parameter, round-half-up interpolation), so replay
//! derives the same colour at the same pixel on any
//! machine. Deposit follows `FillRect`'s per-pixel rule
//! with the field's colour in place of the flat colour.

use crate::color::{Rgba, composite_over, lerp_rgba};
use crate::document::intersect_rect;
use crate::layer::Layer;
use crate::ops::{GradientShape, GradientSpec, Rect};

/// Stops per gradient ceiling (the document validates).
pub const GRADIENT_STOPS_MAX: usize = 8;
/// Absolute bound on gradient endpoint coordinates:
/// keeps the parameter arithmetic deep inside i64.
pub const GRADIENT_COORD_ABS_MAX: i32 = 1_000_000;

/// The parameter's fixed-point full scale (t = 1.0).
const T_SCALE: i64 = 65_536;

/// Integer square root (floor), exact for the whole
/// non-negative i64 range. Newton iteration from above,
/// then a one-step correction in each direction.
fn isqrt_i64(value: i64) -> i64 {
    if value <= 0 {
        return 0;
    }
    let n = value as u64;
    let mut r = 1u64 << (64 - n.leading_zeros()).div_ceil(2);
    loop {
        let next = (r + n / r) / 2;
        if next >= r {
            break;
        }
        r = next;
    }
    while (r + 1) * (r + 1) <= n {
        r += 1;
    }
    while r * r > n {
        r -= 1;
    }
    r as i64
}

/// The gradient's fixed-point parameter at a canvas
/// pixel, 0..=T_SCALE. Degenerate endpoints (refused at
/// the document) read as parameter 0.
fn gradient_t(spec: &GradientSpec, x: i32, y: i32) -> u32 {
    let dx = i64::from(spec.to[0]) - i64::from(spec.from[0]);
    let dy = i64::from(spec.to[1]) - i64::from(spec.from[1]);
    let px = i64::from(x) - i64::from(spec.from[0]);
    let py = i64::from(y) - i64::from(spec.from[1]);
    match spec.shape {
        GradientShape::Linear => {
            let len2 = dx * dx + dy * dy;
            if len2 == 0 {
                return 0;
            }
            let num = (px * dx + py * dy) * T_SCALE;
            (num / len2).clamp(0, T_SCALE) as u32
        }
        GradientShape::Radial => {
            let radius = isqrt_i64(dx * dx + dy * dy);
            if radius == 0 {
                return 0;
            }
            let dist = isqrt_i64(px * px + py * py);
            (dist * T_SCALE / radius).min(T_SCALE) as u32
        }
    }
}

/// One channel's stop interpolation: the weighted mean
/// of the two stop colours at segment position `u` of
/// `span`, round-half-up. All terms non-negative.
fn lerp_channel(c0: u8, c1: u8, u: u32, span: u32) -> u8 {
    let span = u64::from(span);
    let numerator = u64::from(c0) * (span - u64::from(u)) + u64::from(c1) * u64::from(u);
    ((numerator + span / 2) / span) as u8
}

/// The gradient field's colour at a canvas pixel: the
/// stops interpolated at the pixel's parameter, clamped
/// to the end stops outside their range. An empty stop
/// list (refused at the document) reads as transparent.
pub fn gradient_color(spec: &GradientSpec, x: i32, y: i32) -> Rgba {
    let Some(first) = spec.stops.first() else {
        return crate::color::TRANSPARENT;
    };
    let t = gradient_t(spec, x, y);
    let position = |at: u8| -> u32 { u32::from(at) * (T_SCALE as u32) / 255 };
    if t <= position(first.at) {
        return first.color;
    }
    for pair in spec.stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (pa, pb) = (position(a.at), position(b.at));
        if t <= pb {
            let span = pb - pa;
            if span == 0 {
                return b.color;
            }
            let u = t - pa;
            return [
                lerp_channel(a.color[0], b.color[0], u, span),
                lerp_channel(a.color[1], b.color[1], u, span),
                lerp_channel(a.color[2], b.color[2], u, span),
                lerp_channel(a.color[3], b.color[3], u, span),
            ];
        }
    }
    spec.stops
        .last()
        .map_or(crate::color::TRANSPARENT, |stop| stop.color)
}

/// Fill a rectangle with the gradient field: source-over
/// deposit, or alpha-preserving under lock-alpha — per
/// pixel exactly `FillRect`'s rule with the field's
/// colour at that pixel.
pub fn apply_gradient_fill(
    layer: &mut Layer,
    rect: &Rect,
    spec: &GradientSpec,
    width: u32,
    height: u32,
) {
    let (x0, y0, x1, y1) = intersect_rect(rect, width, height).expect("validated intersection");
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let color = gradient_color(spec, gx as i32, gy as i32);
            let dest = layer.pixel(gx, gy);
            let out = if layer.lock_alpha {
                if dest[3] == 0 {
                    continue;
                }
                let mut mixed = lerp_rgba(color, dest, color[3]);
                mixed[3] = dest[3];
                mixed
            } else {
                composite_over(color, dest)
            };
            if out != dest {
                layer.set_pixel(gx, gy, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerId;
    use crate::ops::GradientStop;

    fn linear_spec() -> GradientSpec {
        GradientSpec {
            shape: GradientShape::Linear,
            from: [0, 0],
            to: [100, 0],
            stops: vec![
                GradientStop {
                    at: 0,
                    color: [0, 0, 0, 255],
                },
                GradientStop {
                    at: 255,
                    color: [255, 255, 255, 255],
                },
            ],
        }
    }

    #[test]
    fn linear_endpoints_midpoint_and_clamp() {
        let spec = linear_spec();
        assert_eq!(gradient_color(&spec, 0, 0), [0, 0, 0, 255]);
        assert_eq!(gradient_color(&spec, 100, 0), [255, 255, 255, 255]);
        // Midpoint: (255 * span/2 + span/2) / span = 128.
        assert_eq!(gradient_color(&spec, 50, 0), [128, 128, 128, 255]);
        // Past the end clamps to the last stop; the axis
        // is horizontal, so y does not move the parameter.
        assert_eq!(gradient_color(&spec, 400, 0), [255, 255, 255, 255]);
        assert_eq!(gradient_color(&spec, 50, 77), [128, 128, 128, 255]);
        // Behind the start clamps to the first stop.
        assert_eq!(gradient_color(&spec, -30, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn radial_centre_rim_and_beyond() {
        let spec = GradientSpec {
            shape: GradientShape::Radial,
            from: [50, 50],
            to: [150, 50],
            stops: vec![
                GradientStop {
                    at: 0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    at: 255,
                    color: [0, 0, 255, 255],
                },
            ],
        };
        assert_eq!(gradient_color(&spec, 50, 50), [255, 0, 0, 255]);
        assert_eq!(gradient_color(&spec, 150, 50), [0, 0, 255, 255]);
        assert_eq!(gradient_color(&spec, 50, 150), [0, 0, 255, 255]);
        assert_eq!(gradient_color(&spec, 500, 50), [0, 0, 255, 255]);
        // Half the radius: both channels at the midpoint.
        assert_eq!(gradient_color(&spec, 100, 50), [128, 0, 128, 255]);
    }

    #[test]
    fn multi_stop_and_alpha_interpolate() {
        let spec = GradientSpec {
            shape: GradientShape::Linear,
            from: [0, 0],
            to: [255, 0],
            stops: vec![
                GradientStop {
                    at: 0,
                    color: [0, 0, 0, 0],
                },
                GradientStop {
                    at: 128,
                    color: [100, 0, 0, 200],
                },
                GradientStop {
                    at: 255,
                    color: [100, 100, 100, 255],
                },
            ],
        };
        // x=128 lands inside the first segment, near its
        // end: red close to 100, alpha close to 200.
        let mid = gradient_color(&spec, 128, 0);
        assert!((95..=100).contains(&mid[0]), "red {mid:?}");
        assert!((195..=200).contains(&mid[3]), "alpha {mid:?}");
        assert_eq!(gradient_color(&spec, 255, 0), [100, 100, 100, 255]);
    }

    #[test]
    fn isqrt_is_exact_at_squares_and_neighbours() {
        for n in [0i64, 1, 2, 3, 4, 15, 16, 17, 10_000, 8_000_000_000_000] {
            let r = isqrt_i64(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "isqrt({n}) = {r}");
        }
        assert_eq!(isqrt_i64(144), 12);
        assert_eq!(isqrt_i64(143), 11);
    }

    #[test]
    fn fill_deposits_the_field_and_respects_lock_alpha() {
        let spec = linear_spec();
        let rect = Rect {
            x: 0,
            y: 0,
            width: 101,
            height: 4,
        };
        let mut layer = Layer::new(LayerId(1), "shade".to_string());
        apply_gradient_fill(&mut layer, &rect, &spec, 200, 200);
        assert_eq!(layer.pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(layer.pixel(100, 0), [255, 255, 255, 255]);
        assert_eq!(layer.pixel(50, 3), [128, 128, 128, 255]);

        // Lock-alpha: nothing lands on transparency, and
        // landed colour keeps the destination alpha.
        let mut locked = Layer::new(LayerId(2), "locked".to_string());
        locked.lock_alpha = true;
        apply_gradient_fill(&mut locked, &rect, &spec, 200, 200);
        assert_eq!(locked.tile_count(), 0);
    }
}
