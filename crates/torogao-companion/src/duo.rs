//! The duo stage (Stage 2.3): two companions on one
//! stage under one fixed-step clock — partner-directed
//! gaze, a turn-taking script, and the composed-frame
//! census the stage gate reads.
//!
//! Composition: partner A's parts draw first (all of
//! them), then partner B's, B placed to the right of A
//! with an authored gap between the rigs' rest spans.
//! The stage divider is the middle of that gap; the
//! stage frame is the union of the two rest spans.
//!
//! The census runs over every composed frame:
//!
//! - a **draw-order pop** is a pair of parts whose
//!   bounding boxes overlap in two consecutive frames
//!   while their draw relation flips between them —
//!   the composition layer must never reorder parts
//!   (no re-sort, no hash-order nondeterminism);
//! - a **clipping event** is a part whose bounding box
//!   crosses the stage divider or leaves the stage
//!   frame — each partner owns her half of the stage.
//!   The stage frame is the union of the two rest spans
//!   plus the authored stage margin: at rest a rig is
//!   not at the extremum of its own sway (measured
//!   physiological excursion past the rest bounds is
//!   up to ~4.2 units on these rigs, breathing and hair
//!   physics), so the frame the composition renders
//!   into carries breathing room; a part leaving THAT
//!   frame has escaped the stage. The divider carries
//!   no margin — the halves are exact.
//!
//! Partner gaze: each partner's pointer fractions are
//! computed to aim her gaze driver at the other
//! partner's face anchor (the midpoint of her eye
//! spans), through the same view geometry the core's
//! pointer conversion uses — so "looking at her
//! partner" is a measured direction, not a pose.

use torogao_app::render::PreparedFrame;

use crate::core::{CompanionCore, FACE_ANCHOR, VIEW_DISTANCE_HEIGHTS};
use crate::error::CompanionResult;
use crate::perf::chain_step;

/// The authored gap between the two rigs' rest spans,
/// in world units.
pub const STAGE_GAP: f64 = 40.0;

/// The stage frame's margin beyond the rest spans, in
/// world units (covers the measured physiological
/// excursion of ~4.2 with headroom).
pub const STAGE_MARGIN: f64 = 10.0;

/// One part of the composed frame.
#[derive(Debug, Clone)]
pub struct ComposedPart {
    /// Which partner owns the part (0 = A, 1 = B).
    pub owner: usize,
    /// The part's path within its own scene.
    pub path: String,
    /// The composed draw index (A's parts first).
    pub order: usize,
    /// Bounding box (min x, min y, max x, max y) in
    /// composed coordinates.
    pub bbox: (f64, f64, f64, f64),
}

/// The composed stage: both partners' parts in draw
/// order, plus the stage geometry the census reads.
#[derive(Debug, Clone)]
pub struct ComposedFrame {
    /// All parts, in draw order.
    pub parts: Vec<ComposedPart>,
    /// The divider's x (each partner owns her side).
    pub divider: f64,
    /// The stage frame (min x, min y, max x, max y).
    pub frame: (f64, f64, f64, f64),
}

/// The stage placement derived from the two rigs' rest
/// spans.
#[derive(Debug, Clone, Copy)]
pub struct StageLayout {
    /// B's x offset in composed coordinates.
    pub offset_b: f64,
    /// The divider's x.
    pub divider: f64,
    /// The stage frame.
    pub frame: (f64, f64, f64, f64),
    /// A's face anchor in composed coordinates.
    pub face_a: (f64, f64),
    /// B's face anchor in composed coordinates.
    pub face_b: (f64, f64),
}

/// A part-set span: (min x, min y, max x, max y).
fn span_of(frame: &PreparedFrame) -> (f64, f64, f64, f64) {
    let mut span = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for part in &frame.parts {
        for [x, y] in &part.positions {
            span.0 = span.0.min(f64::from(*x));
            span.1 = span.1.min(f64::from(*y));
            span.2 = span.2.max(f64::from(*x));
            span.3 = span.3.max(f64::from(*y));
        }
    }
    span
}

/// A frame's face anchor: the midpoint of the two eye
/// parts' bounding-box centres.
fn face_anchor_of(frame: &PreparedFrame) -> (f64, f64) {
    let mut sum = (0.0, 0.0);
    let mut count = 0.0;
    for leaf in ["EyeL", "EyeR"] {
        if let Some(part) = frame
            .parts
            .iter()
            .find(|part| part.path.rsplit('/').next() == Some(leaf))
        {
            let (mut min_x, mut min_y, mut max_x, mut max_y) =
                (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for [x, y] in &part.positions {
                min_x = min_x.min(f64::from(*x));
                min_y = min_y.min(f64::from(*y));
                max_x = max_x.max(f64::from(*x));
                max_y = max_y.max(f64::from(*y));
            }
            sum.0 += (min_x + max_x) / 2.0;
            sum.1 += (min_y + max_y) / 2.0;
            count += 1.0;
        }
    }
    if count > 0.0 {
        (sum.0 / count, sum.1 / count)
    } else {
        (0.0, 0.0)
    }
}

/// Derives the stage layout from the two rest frames.
pub fn layout(frame_a: &PreparedFrame, frame_b: &PreparedFrame) -> StageLayout {
    let span_a = span_of(frame_a);
    let span_b = span_of(frame_b);
    let offset_b = span_a.2 - span_b.0 + STAGE_GAP;
    let divider = span_a.2 + STAGE_GAP / 2.0;
    let frame = (
        span_a.0.min(span_b.0 + offset_b) - STAGE_MARGIN,
        span_a.1.min(span_b.1) - STAGE_MARGIN,
        span_a.2.max(span_b.2 + offset_b) + STAGE_MARGIN,
        span_a.3.max(span_b.3) + STAGE_MARGIN,
    );
    let face_a = face_anchor_of(frame_a);
    let face_b_local = face_anchor_of(frame_b);
    StageLayout {
        offset_b,
        divider,
        frame,
        face_a,
        face_b: (face_b_local.0 + offset_b, face_b_local.1),
    }
}

/// Composes one frame from the two partners' prepared
/// frames under a layout.
pub fn compose(
    frame_a: &PreparedFrame,
    frame_b: &PreparedFrame,
    layout: &StageLayout,
) -> ComposedFrame {
    let mut parts = Vec::with_capacity(frame_a.parts.len() + frame_b.parts.len());
    let push = |owner: usize, frame: &PreparedFrame, offset: f64, parts: &mut Vec<ComposedPart>| {
        for part in &frame.parts {
            let (mut min_x, mut min_y, mut max_x, mut max_y) =
                (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for [x, y] in &part.positions {
                min_x = min_x.min(f64::from(*x) + offset);
                min_y = min_y.min(f64::from(*y));
                max_x = max_x.max(f64::from(*x) + offset);
                max_y = max_y.max(f64::from(*y));
            }
            parts.push(ComposedPart {
                owner,
                path: part.path.clone(),
                order: parts.len(),
                bbox: (min_x, min_y, max_x, max_y),
            });
        }
    };
    push(0, frame_a, 0.0, &mut parts);
    push(1, frame_b, layout.offset_b, &mut parts);
    ComposedFrame {
        parts,
        divider: layout.divider,
        frame: layout.frame,
    }
}

/// Whether two bounding boxes overlap (with area).
fn overlaps(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// The frame-to-frame census: draw-order pops and
/// clipping events between the previous composed frame
/// (if any) and this one. Returns (pops, clipping).
pub fn census(previous: Option<&ComposedFrame>, current: &ComposedFrame) -> (usize, usize) {
    let mut clipping = 0;
    for part in &current.parts {
        let (min_x, min_y, max_x, max_y) = part.bbox;
        let crosses_divider = if part.owner == 0 {
            max_x > current.divider
        } else {
            min_x < current.divider
        };
        let outside_frame = min_x < current.frame.0
            || max_x > current.frame.2
            || min_y < current.frame.1
            || max_y > current.frame.3;
        if crosses_divider || outside_frame {
            clipping += 1;
        }
    }
    let mut pops = 0;
    if let Some(previous) = previous {
        // Part identity is (owner, path); both frames
        // carry the same part sets.
        for i in 0..current.parts.len() {
            for j in (i + 1)..current.parts.len() {
                let (a_now, b_now) = (&current.parts[i], &current.parts[j]);
                if !overlaps(a_now.bbox, b_now.bbox) {
                    continue;
                }
                let Some(a_prev) = previous
                    .parts
                    .iter()
                    .find(|part| part.owner == a_now.owner && part.path == a_now.path)
                else {
                    continue;
                };
                let Some(b_prev) = previous
                    .parts
                    .iter()
                    .find(|part| part.owner == b_now.owner && part.path == b_now.path)
                else {
                    continue;
                };
                if overlaps(a_prev.bbox, b_prev.bbox)
                    && (a_prev.order < b_prev.order) != (a_now.order < b_now.order)
                {
                    pops += 1;
                }
            }
        }
    }
    (pops, clipping)
}

/// The pointer fractions aiming a partner's gaze at a
/// target point in composed coordinates, through the
/// core's view geometry. `own_face` is the partner's
/// own face anchor (composed), `canvas_height` her
/// scene's canvas height in world units, `aspect` the
/// surface aspect her core converts with.
pub fn partner_pointer(
    own_face: (f64, f64),
    target: (f64, f64),
    canvas_height: f64,
    aspect: f64,
) -> (f64, f64) {
    let distance = VIEW_DISTANCE_HEIGHTS * canvas_height;
    let yaw = (target.0 - own_face.0).atan2(distance);
    let pitch = (target.1 - own_face.1).atan2(distance);
    (
        FACE_ANCHOR.0 + yaw.tan() * VIEW_DISTANCE_HEIGHTS / aspect,
        FACE_ANCHOR.1 + pitch.tan() * VIEW_DISTANCE_HEIGHTS,
    )
}

/// Two companion cores under one fixed-step clock.
pub struct DuoCore {
    /// Partner A (stage left).
    pub a: CompanionCore,
    /// Partner B (stage right).
    pub b: CompanionCore,
    /// Steps taken on the shared clock.
    pub step: u64,
}

impl DuoCore {
    /// Loads both partners (the genesis scene + ambient
    /// loop each core starts with).
    pub fn load(
        model_a: &std::path::Path,
        model_b: &std::path::Path,
        identity: &str,
    ) -> CompanionResult<DuoCore> {
        Ok(DuoCore {
            a: CompanionCore::load(model_a, None, None, identity)?,
            b: CompanionCore::load(model_b, None, None, identity)?,
            step: 0,
        })
    }

    /// One shared-clock step: both partners tick once;
    /// the combined hash chains both evaluated states.
    pub fn tick(&mut self) -> CompanionResult<u64> {
        let hash_a = self.a.tick()?;
        let hash_b = self.b.tick()?;
        self.step += 1;
        Ok(chain_step(chain_step(0, hash_a), hash_b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torogao_app::render::{Canvas, PreparedPart};

    fn part(path: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> PreparedPart {
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

    fn frame(parts: Vec<PreparedPart>) -> PreparedFrame {
        PreparedFrame {
            canvas: Canvas {
                width: 16,
                height: 10,
                view: [0.0; 16],
            },
            parts,
            mask_sources: Vec::new(),
            skipped: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn the_layout_places_b_right_of_a_with_the_gap() {
        let a = frame(vec![
            part("/Root/EyeL", -4.0, -5.0, -1.0, -3.0),
            part("/Root/EyeR", 1.0, -5.0, 4.0, -3.0),
            part("/Root/Body", -10.0, 0.0, 10.0, 20.0),
        ]);
        let b = frame(vec![
            part("/Root/EyeL", -4.0, -5.0, -1.0, -3.0),
            part("/Root/EyeR", 1.0, -5.0, 4.0, -3.0),
            part("/Root/Body", -12.0, 0.0, 12.0, 22.0),
        ]);
        let layout = layout(&a, &b);
        // B offset: A max x (10) - B min x (-12) + 40 = 62.
        assert_eq!(layout.offset_b, 62.0);
        assert_eq!(layout.divider, 30.0);
        assert_eq!(layout.face_a, (0.0, -4.0));
        assert_eq!(layout.face_b, (62.0, -4.0));
        let composed = compose(&a, &b, &layout);
        let (pops, clipping) = census(None, &composed);
        assert_eq!(pops, 0);
        assert_eq!(clipping, 0);
        // A second identical frame: still no pops.
        let (pops, _) = census(Some(&composed), &composed);
        assert_eq!(pops, 0);
    }

    #[test]
    fn a_part_crossing_the_divider_is_a_clipping_event() {
        // The layout comes from the rest frames (A spans
        // to x=10, so the divider sits at x=30); a posed
        // frame whose body reaches x=35 crosses it.
        let rest_a = frame(vec![part("/Root/Body", -10.0, 0.0, 10.0, 20.0)]);
        let rest_b = frame(vec![part("/Root/Body", -12.0, 0.0, 12.0, 22.0)]);
        let layout = layout(&rest_a, &rest_b);
        assert_eq!(layout.divider, 30.0);
        let posed_a = frame(vec![part("/Root/Body", -10.0, 0.0, 35.0, 20.0)]);
        let composed = compose(&posed_a, &rest_b, &layout);
        let (_, clipping) = census(None, &composed);
        assert_eq!(clipping, 1, "A's posed body crosses the divider");
    }

    #[test]
    fn partner_pointer_aims_right_for_a_right_partner() {
        let fractions = partner_pointer((0.0, -4.0), (62.0, -4.0), 640.0, 0.5);
        // Yaw = atan(62 / (1.35*640)) ~ 4.1 deg; the
        // fractions must sit right of the anchor, at the
        // anchor height.
        assert!(fractions.0 > FACE_ANCHOR.0);
        assert!((fractions.1 - FACE_ANCHOR.1).abs() < 1e-9);
    }
}
