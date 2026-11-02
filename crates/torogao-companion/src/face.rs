//! Face geometry reads (Stage 2.2): the aperture,
//! visible-iris fraction, and sclera field of each eye,
//! read from a prepared frame's part meshes — the
//! engineering reads behind the stage's coupling gates.
//!
//! Definitions (the plate frame; all spans in world
//! units, y down):
//!
//! - The eye frame is the `EyeL` / `EyeR` part's own
//!   span (the eye white plate): top, bottom, height H.
//! - The lid edge is the `UpperLidL` / `UpperLidR`
//!   part's lowest point — where the lid line cuts the
//!   eye.
//! - aperture = (eye bottom − lid edge) / H: the
//!   fraction of the eye's height open below the lid.
//! - visible-iris fraction = the iris mesh's area
//!   inside the opening band [lid edge, eye bottom],
//!   over the iris mesh's full area (exact triangle
//!   clipping, no disc approximation).
//! - sclera field = (opening area − visible iris
//!   area) / opening area, where the opening area is
//!   the eye mesh's area inside the same band.
//!
//! These are polygon reads on the evaluated pose. The
//! absolute bands the PROGRAM sets for aperture / iris
//! / sclera are renderer reads (the cast-rigs battery
//! routes them to its FR-08 lane as OPEN); the gate
//! test reports these reads against the bands and pins
//! the measured frame deviation rather than redefining
//! the read to fit.

use torogao_app::render::{PreparedFrame, PreparedPart};

/// One eye's geometry read.
#[derive(Debug, Clone, Copy)]
pub struct EyeRead {
    /// Open fraction of the eye's height below the lid.
    pub aperture: f64,
    /// Visible iris area over full iris area.
    pub iris_fraction: f64,
    /// White fraction of the opening.
    pub sclera_field: f64,
}

/// Both eyes' reads from one prepared frame.
#[derive(Debug, Clone, Copy)]
pub struct FaceRead {
    /// The left eye (her left, `UpperLidL` side).
    pub left: EyeRead,
    /// The right eye.
    pub right: EyeRead,
}

/// Reads both eyes from a prepared frame; `None` when
/// any of the six face parts is absent (a rig without
/// the governed face parts has nothing to read — the
/// caller reports the gap).
pub fn read_face(frame: &PreparedFrame) -> Option<FaceRead> {
    Some(FaceRead {
        left: read_eye(frame, "EyeL", "UpperLidL", "IrisL")?,
        right: read_eye(frame, "EyeR", "UpperLidR", "IrisR")?,
    })
}

/// The iris's upward travel from its rest top, as a
/// fraction of the eye height — the coupling law's
/// second dial (the template band is 0.50–0.65 at the
/// peak). `rest_iris_top` is read at R = 0 by the
/// caller.
pub fn iris_travel(frame: &PreparedFrame, side: &str, rest_iris_top: f64) -> Option<f64> {
    let eye = part(frame, if side == "L" { "EyeL" } else { "EyeR" })?;
    let iris = part(frame, if side == "L" { "IrisL" } else { "IrisR" })?;
    let (eye_top, eye_bottom) = span_y(eye);
    let height = eye_bottom - eye_top;
    if height <= 0.0 {
        return None;
    }
    let (iris_top, _) = span_y(iris);
    Some((rest_iris_top - iris_top) / height)
}

/// One eye's read from its three parts.
fn read_eye(frame: &PreparedFrame, eye: &str, lid: &str, iris: &str) -> Option<EyeRead> {
    let eye_part = part(frame, eye)?;
    let lid_part = part(frame, lid)?;
    let iris_part = part(frame, iris)?;
    let (eye_top, eye_bottom) = span_y(eye_part);
    let height = eye_bottom - eye_top;
    if height <= 0.0 {
        return None;
    }
    let (_, lid_edge) = span_y(lid_part);
    let aperture = ((eye_bottom - lid_edge) / height).clamp(0.0, 1.0);
    let iris_area = mesh_area(iris_part);
    let visible_iris = clipped_area(iris_part, lid_edge, eye_bottom);
    let opening = clipped_area(eye_part, lid_edge, f64::MAX);
    let iris_fraction = if iris_area > 0.0 {
        (visible_iris / iris_area).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let sclera_field = if opening > 0.0 {
        ((opening - visible_iris) / opening).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(EyeRead {
        aperture,
        iris_fraction,
        sclera_field,
    })
}

/// Finds a part by its leaf name (the last path
/// segment).
fn part<'a>(frame: &'a PreparedFrame, leaf: &str) -> Option<&'a PreparedPart> {
    frame
        .parts
        .iter()
        .find(|part| part.path.rsplit('/').next() == Some(leaf))
}

/// The (top, bottom) y span of a part's vertices.
fn span_y(part: &PreparedPart) -> (f64, f64) {
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    for [_, y] in &part.positions {
        min = min.min(f64::from(*y));
        max = max.max(f64::from(*y));
    }
    (min, max)
}

/// A part's full mesh area (sum of triangle areas).
fn mesh_area(part: &PreparedPart) -> f64 {
    part.indices
        .chunks_exact(3)
        .map(|tri| {
            let (a, b, c) = (
                vertex(part, tri[0]),
                vertex(part, tri[1]),
                vertex(part, tri[2]),
            );
            triangle_area(a, b, c)
        })
        .sum()
}

/// A part's mesh area inside the horizontal band
/// `y_min <= y <= y_max` (exact per-triangle clipping).
fn clipped_area(part: &PreparedPart, y_min: f64, y_max: f64) -> f64 {
    part.indices
        .chunks_exact(3)
        .map(|tri| {
            let poly = vec![
                vertex(part, tri[0]),
                vertex(part, tri[1]),
                vertex(part, tri[2]),
            ];
            let poly = clip_half_plane(poly, y_min, true);
            let poly = clip_half_plane(poly, y_max, false);
            polygon_area(&poly)
        })
        .sum()
}

/// One vertex as an f64 point.
fn vertex(part: &PreparedPart, index: u32) -> (f64, f64) {
    let [x, y] = part.positions[index as usize];
    (f64::from(x), f64::from(y))
}

/// Sutherland–Hodgman clip of a polygon against the
/// half-plane `y >= bound` (keep_above) or `y <= bound`.
fn clip_half_plane(poly: Vec<(f64, f64)>, bound: f64, keep_above: bool) -> Vec<(f64, f64)> {
    if poly.is_empty() {
        return poly;
    }
    let inside = |p: (f64, f64)| {
        if keep_above {
            p.1 >= bound
        } else {
            p.1 <= bound
        }
    };
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut current = poly[poly.len() - 1];
    let mut current_in = inside(current);
    for next in poly {
        let next_in = inside(next);
        if next_in {
            if !current_in {
                out.push(intersect_y(current, next, bound));
            }
            out.push(next);
        } else if current_in {
            out.push(intersect_y(current, next, bound));
        }
        current = next;
        current_in = next_in;
    }
    out
}

/// The point where segment a→b crosses `y = bound`.
fn intersect_y(a: (f64, f64), b: (f64, f64), bound: f64) -> (f64, f64) {
    let span = b.1 - a.1;
    if span.abs() < f64::EPSILON {
        return a;
    }
    let t = (bound - a.1) / span;
    (a.0 + (b.0 - a.0) * t, bound)
}

/// A polygon's area (shoelace).
fn polygon_area(poly: &[(f64, f64)]) -> f64 {
    if poly.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..poly.len() {
        let (x1, y1) = poly[i];
        let (x2, y2) = poly[(i + 1) % poly.len()];
        sum += x1 * y2 - x2 * y1;
    }
    (sum / 2.0).abs()
}

/// A triangle's area.
fn triangle_area(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    polygon_area(&[a, b, c])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad_part(path: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> PreparedPart {
        PreparedPart {
            path: path.to_string(),
            positions: vec![[x0, y0], [x1, y0], [x0, y1], [x1, y1]],
            uvs: vec![[0.0, 0.0]; 4],
            indices: vec![0, 1, 2, 2, 1, 3],
            texture_slot: None,
            blend_slug: "normal",
            blend_mode: "Normal".to_string(),
            blend_note: None,
            tint: [1.0, 1.0, 1.0, 1.0],
            screen: [0.0, 0.0, 0.0],
            node: 0,
            masks: Vec::new(),
            mask_threshold: 0.0,
        }
    }

    fn frame_with(lid_bottom: f32, iris_top: f32) -> PreparedFrame {
        // Eye white: y in [0, 10]; lid band: [-5.5, lid_bottom];
        // iris: [iris_top, iris_top + 11].
        PreparedFrame {
            canvas: torogao_app::render::Canvas {
                width: 16,
                height: 10,
                view: [0.0; 16],
            },
            parts: vec![
                quad_part("/Root/EyeL", -8.0, 0.0, 8.0, 10.0),
                quad_part("/Root/UpperLidL", -9.0, -5.5, 9.0, lid_bottom),
                quad_part("/Root/IrisL", -5.0, iris_top, 5.0, iris_top + 11.0),
                quad_part("/Root/EyeR", 12.0, 0.0, 28.0, 10.0),
                quad_part("/Root/UpperLidR", 11.0, -5.5, 29.0, lid_bottom),
                quad_part("/Root/IrisR", 15.0, iris_top, 25.0, iris_top + 11.0),
            ],
            mask_sources: Vec::new(),
            skipped: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn the_reads_follow_the_geometry() {
        // Lid edge at 3.6 (aperture 0.64), iris at rest.
        let read = read_face(&frame_with(3.6, -0.5)).expect("read");
        assert!((read.left.aperture - 0.64).abs() < 0.01);
        // Iris spans [-0.5, 10.5]; visible band [3.6, 10]:
        // 6.4 of 11 in height over a quad: area fraction
        // 6.4/11.
        assert!((read.left.iris_fraction - 6.4 / 11.0).abs() < 0.01);
        // Opening: eye band [3.6, 10] over width 16 = 102.4;
        // visible iris: band over width 10 = 64; sclera =
        // (102.4 - 64)/102.4 = 0.375.
        assert!((read.left.sclera_field - 0.375).abs() < 0.01);
        assert!((read.left.aperture - read.right.aperture).abs() < 1e-9);
    }

    #[test]
    fn a_fully_rolled_iris_reads_zero_visible() {
        // Iris translated up so its bottom (lid_edge - 2)
        // is above the lid edge: nothing visible.
        let read = read_face(&frame_with(3.6, -12.6)).expect("read");
        assert_eq!(read.left.iris_fraction, 0.0);
        assert_eq!(read.left.sclera_field, 1.0);
    }

    #[test]
    fn clipped_area_of_a_triangle_is_exact() {
        let part = quad_part("/Root/X", 0.0, 0.0, 10.0, 10.0);
        assert!((mesh_area(&part) - 100.0).abs() < 1e-9);
        assert!((clipped_area(&part, 5.0, f64::MAX) - 50.0).abs() < 1e-9);
        assert!((clipped_area(&part, 2.5, 7.5) - 50.0).abs() < 1e-9);
    }
}
