//! 120 fps presentation interpolation (fps arm D,
//! adopted D-610): the canonical evaluation stays at
//! 60 Hz (`scene::STEP_SECONDS`); the windowed shell
//! presents at the display rate by linearly interpolating
//! between the two bracketing *evaluated* frames — the
//! previous and the current `PreparedFrame` — at the
//! phase the fixed-step clock has reached inside the
//! current step interval.
//!
//! The contract, from the frame-rate trial
//! (torogao-fps-trial-20261102/TRIAL-REPORT.md):
//!
//! - Interpolation is presentation-only. An interpolated
//!   frame is derived data: it is never an evaluated
//!   state, it is never written back into the puppet, and
//!   it never enters a state hash, a save, or a take's
//!   governed frames. The take export and headless render
//!   paths do not pass through here at all.
//! - Continuous quantities blend: world-space vertex
//!   positions (where deformation and jiggle live), part
//!   tint (colour + effective opacity), screen tint, and
//!   the mask threshold.
//! - Discrete quantities snap, never blend: the part
//!   list and its order, node identity, UVs, indices,
//!   texture slot, blend mode, mask bindings, canvas,
//!   and the frame's skipped/notes. The earlier frame's
//!   discrete state holds for the whole interval; a
//!   change lands exactly at the step boundary (phase 1),
//!   never early.
//! - A part whose vertex count differs between the two
//!   frames (its mesh was swapped mid-interval) snaps as
//!   a whole: the earlier frame's part holds until the
//!   boundary.

use std::collections::BTreeMap;

use crate::render::{PreparedFrame, PreparedPart};
use crate::scene::STEP_SECONDS;

/// The presentation phase inside the current step
/// interval: 0 at the step boundary, approaching 1 as
/// the next step falls due. Clamped to [0, 1]; a
/// negative or non-finite elapsed time yields 0 (present
/// the earlier frame).
pub fn presentation_phase(elapsed_seconds: f64) -> f32 {
    if !elapsed_seconds.is_finite() || elapsed_seconds <= 0.0 {
        return 0.0;
    }
    (elapsed_seconds / STEP_SECONDS).clamp(0.0, 1.0) as f32
}

/// A phase clamped to [0, 1]; NaN yields 0 (the earlier
/// frame), matching the render module's NaN discipline.
fn clamp_phase(t: f32) -> f32 {
    if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Blends one part pair. `curr` is the same node's part
/// in the later frame, when it exists there. The result
/// always carries `prev`'s discrete state; the
/// continuous fields blend only when both sides carry
/// the same vertex count — a mesh swap snaps the whole
/// part.
fn interpolate_part(prev: &PreparedPart, curr: Option<&PreparedPart>, t: f32) -> PreparedPart {
    let mut out = prev.clone();
    let Some(curr) = curr else {
        return out;
    };
    if prev.positions.len() != curr.positions.len() {
        return out;
    }
    for (point, (from, to)) in out
        .positions
        .iter_mut()
        .zip(prev.positions.iter().zip(curr.positions.iter()))
    {
        *point = [lerp(from[0], to[0], t), lerp(from[1], to[1], t)];
    }
    for channel in 0..4 {
        out.tint[channel] = lerp(prev.tint[channel], curr.tint[channel], t);
    }
    for channel in 0..3 {
        out.screen[channel] = lerp(prev.screen[channel], curr.screen[channel], t);
    }
    out.mask_threshold = lerp(prev.mask_threshold, curr.mask_threshold, t);
    out
}

/// Blends one part list against the later frame's list,
/// matching parts by arena node. Parts only in the
/// earlier list hold; parts only in the later list are
/// absent (they land at the boundary, with the later
/// frame's structure).
fn interpolate_parts(prev: &[PreparedPart], curr: &[PreparedPart], t: f32) -> Vec<PreparedPart> {
    let curr_by_node: BTreeMap<usize, &PreparedPart> =
        curr.iter().map(|part| (part.node, part)).collect();
    prev.iter()
        .map(|part| interpolate_part(part, curr_by_node.get(&part.node).copied(), t))
        .collect()
}

/// Builds the presented frame at phase `t` between the
/// evaluated frames `prev` (earlier) and `curr` (later).
/// At `t <= 0` the result is `prev` exactly; at `t >= 1`
/// it is `curr` exactly, structure included. Between,
/// the structure is `prev`'s and the continuous
/// quantities are the linear blend — the trial's
/// half-step interpolation at `t = 0.5` on a 120 Hz
/// display.
pub fn interpolate_frame(prev: &PreparedFrame, curr: &PreparedFrame, t: f32) -> PreparedFrame {
    let t = clamp_phase(t);
    if t <= 0.0 {
        return prev.clone();
    }
    if t >= 1.0 {
        return curr.clone();
    }
    PreparedFrame {
        canvas: prev.canvas,
        parts: interpolate_parts(&prev.parts, &curr.parts, t),
        mask_sources: interpolate_parts(&prev.mask_sources, &curr.mask_sources, t),
        skipped: prev.skipped.clone(),
        notes: prev.notes.clone(),
    }
}

/// Selects the frame to present. Interpolation applies
/// only when `enabled` and an earlier bracket exists;
/// in every other case the presented frame is the
/// current evaluated frame itself — presentation with
/// interpolation off is the canonical frame, unchanged.
pub fn presented_frame(
    prev: Option<&PreparedFrame>,
    curr: &PreparedFrame,
    t: f32,
    enabled: bool,
) -> PreparedFrame {
    match (enabled, prev) {
        (true, Some(prev)) => interpolate_frame(prev, curr, t),
        _ => curr.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Canvas;

    fn part(node: usize, positions: &[[f32; 2]], tint: [f32; 4]) -> PreparedPart {
        PreparedPart {
            path: format!("node{node}"),
            positions: positions.to_vec(),
            uvs: positions
                .iter()
                .map(|point| [point[0], -point[1]])
                .collect(),
            indices: vec![0, 1, 2],
            texture_slot: Some(node),
            blend_slug: "normal",
            blend_mode: "Normal".to_string(),
            blend_note: None,
            tint,
            screen: [0.0, 0.0, 0.0],
            node,
            masks: Vec::new(),
            mask_threshold: 0.25,
        }
    }

    fn frame(parts: Vec<PreparedPart>) -> PreparedFrame {
        PreparedFrame {
            canvas: Canvas {
                width: 64,
                height: 64,
                view: [0.0; 16],
            },
            parts,
            mask_sources: Vec::new(),
            skipped: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn positions(frame: &PreparedFrame, part: usize) -> Vec<[f32; 2]> {
        frame.parts[part].positions.clone()
    }

    #[test]
    fn phase_zero_and_one_return_the_bracketing_frames_exactly() {
        let prev = frame(vec![part(
            1,
            &[[0.0, 0.0], [10.0, 0.0]],
            [1.0, 0.0, 0.0, 1.0],
        )]);
        let curr = frame(vec![part(
            1,
            &[[4.0, 8.0], [2.0, -4.0]],
            [0.0, 1.0, 0.0, 0.5],
        )]);
        let at_zero = interpolate_frame(&prev, &curr, 0.0);
        assert_eq!(positions(&at_zero, 0), vec![[0.0, 0.0], [10.0, 0.0]]);
        assert_eq!(at_zero.parts[0].tint, [1.0, 0.0, 0.0, 1.0]);
        let at_one = interpolate_frame(&prev, &curr, 1.0);
        assert_eq!(positions(&at_one, 0), vec![[4.0, 8.0], [2.0, -4.0]]);
        assert_eq!(at_one.parts[0].tint, [0.0, 1.0, 0.0, 0.5]);
    }

    #[test]
    fn midpoint_blends_the_continuous_quantities() {
        let prev = frame(vec![part(
            1,
            &[[0.0, 0.0], [10.0, 20.0]],
            [1.0, 0.0, 0.0, 1.0],
        )]);
        let mut curr_part = part(1, &[[4.0, 8.0], [2.0, -4.0]], [0.0, 1.0, 0.0, 0.5]);
        curr_part.screen = [0.2, 0.4, 0.6];
        curr_part.mask_threshold = 0.75;
        let curr = frame(vec![curr_part]);
        let mid = interpolate_frame(&prev, &curr, 0.5);
        assert_eq!(positions(&mid, 0), vec![[2.0, 4.0], [6.0, 8.0]]);
        assert_eq!(mid.parts[0].tint, [0.5, 0.5, 0.0, 0.75]);
        assert_eq!(mid.parts[0].screen, [0.1, 0.2, 0.3]);
        assert_eq!(mid.parts[0].mask_threshold, 0.5);
    }

    #[test]
    fn discrete_state_snaps_until_the_boundary() {
        let prev = frame(vec![part(1, &[[0.0, 0.0]], [1.0, 1.0, 1.0, 1.0])]);
        let mut curr_part = part(1, &[[8.0, 8.0]], [1.0, 1.0, 1.0, 1.0]);
        curr_part.texture_slot = Some(99);
        curr_part.uvs = vec![[7.0, 7.0]];
        curr_part.indices = vec![2, 1, 0];
        curr_part.blend_mode = "Multiply".to_string();
        let mut curr = frame(vec![curr_part]);
        // A part that appears only in the later frame.
        curr.parts
            .push(part(2, &[[1.0, 1.0]], [0.0, 0.0, 0.0, 1.0]));
        for t in [0.25, 0.5, 0.999] {
            let out = interpolate_frame(&prev, &curr, t);
            assert_eq!(
                out.parts.len(),
                1,
                "appearing part must wait for the boundary"
            );
            assert_eq!(out.parts[0].texture_slot, Some(1));
            assert_eq!(out.parts[0].uvs, vec![[0.0, 0.0]]);
            assert_eq!(out.parts[0].indices, vec![0, 1, 2]);
            assert_eq!(out.parts[0].blend_mode, "Normal");
        }
        let at_one = interpolate_frame(&prev, &curr, 1.0);
        assert_eq!(at_one.parts.len(), 2);
        assert_eq!(at_one.parts[0].texture_slot, Some(99));
        assert_eq!(at_one.parts[0].blend_mode, "Multiply");
    }

    #[test]
    fn vanished_part_holds_until_the_boundary() {
        let prev = frame(vec![
            part(1, &[[0.0, 0.0]], [1.0, 1.0, 1.0, 1.0]),
            part(2, &[[3.0, 3.0]], [1.0, 1.0, 1.0, 1.0]),
        ]);
        let curr = frame(vec![part(1, &[[6.0, 6.0]], [1.0, 1.0, 1.0, 1.0])]);
        let mid = interpolate_frame(&prev, &curr, 0.5);
        assert_eq!(mid.parts.len(), 2);
        assert_eq!(positions(&mid, 0), vec![[3.0, 3.0]]);
        assert_eq!(positions(&mid, 1), vec![[3.0, 3.0]]);
        assert_eq!(interpolate_frame(&prev, &curr, 1.0).parts.len(), 1);
    }

    #[test]
    fn vertex_count_mismatch_snaps_the_whole_part() {
        let prev = frame(vec![part(
            1,
            &[[0.0, 0.0], [10.0, 0.0]],
            [1.0, 0.0, 0.0, 1.0],
        )]);
        let curr = frame(vec![part(
            1,
            &[[4.0, 8.0], [2.0, -4.0], [9.0, 9.0]],
            [0.0, 1.0, 0.0, 0.5],
        )]);
        let mid = interpolate_frame(&prev, &curr, 0.5);
        assert_eq!(positions(&mid, 0), vec![[0.0, 0.0], [10.0, 0.0]]);
        assert_eq!(mid.parts[0].tint, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn mask_sources_blend_and_snap_like_parts() {
        let mut prev = frame(vec![part(1, &[[0.0, 0.0]], [1.0, 1.0, 1.0, 1.0])]);
        prev.mask_sources = vec![part(7, &[[0.0, 0.0]], [1.0, 1.0, 1.0, 1.0])];
        let mut curr = frame(vec![part(1, &[[2.0, 2.0]], [1.0, 1.0, 1.0, 1.0])]);
        curr.mask_sources = vec![
            part(7, &[[10.0, 0.0]], [1.0, 1.0, 1.0, 1.0]),
            part(8, &[[5.0, 5.0]], [1.0, 1.0, 1.0, 1.0]),
        ];
        let mid = interpolate_frame(&prev, &curr, 0.5);
        assert_eq!(mid.mask_sources.len(), 1);
        assert_eq!(mid.mask_sources[0].positions, vec![[5.0, 0.0]]);
        assert_eq!(interpolate_frame(&prev, &curr, 1.0).mask_sources.len(), 2);
    }

    #[test]
    fn phase_clamps_and_rejects_non_finite_time() {
        assert_eq!(presentation_phase(-1.0), 0.0);
        assert_eq!(presentation_phase(0.0), 0.0);
        assert_eq!(presentation_phase(STEP_SECONDS / 2.0), 0.5);
        assert_eq!(presentation_phase(STEP_SECONDS), 1.0);
        assert_eq!(presentation_phase(STEP_SECONDS * 4.0), 1.0);
        assert_eq!(presentation_phase(f64::NAN), 0.0);
        assert_eq!(presentation_phase(f64::INFINITY), 0.0);
    }

    #[test]
    fn out_of_range_phase_snaps_to_the_brackets() {
        let prev = frame(vec![part(1, &[[0.0, 0.0]], [1.0, 0.0, 0.0, 1.0])]);
        let curr = frame(vec![part(1, &[[4.0, 8.0]], [0.0, 1.0, 0.0, 0.5])]);
        assert_eq!(
            positions(&interpolate_frame(&prev, &curr, -0.5), 0),
            vec![[0.0, 0.0]]
        );
        assert_eq!(
            positions(&interpolate_frame(&prev, &curr, 2.0), 0),
            vec![[4.0, 8.0]]
        );
        assert_eq!(
            positions(&interpolate_frame(&prev, &curr, f32::NAN), 0),
            vec![[0.0, 0.0]]
        );
    }

    #[test]
    fn presented_frame_is_the_canonical_frame_when_disabled_or_unpaired() {
        let prev = frame(vec![part(1, &[[0.0, 0.0]], [1.0, 0.0, 0.0, 1.0])]);
        let curr = frame(vec![part(1, &[[4.0, 8.0]], [0.0, 1.0, 0.0, 0.5])]);
        for t in [0.0, 0.5, 0.999] {
            let off = presented_frame(Some(&prev), &curr, t, false);
            assert_eq!(positions(&off, 0), vec![[4.0, 8.0]]);
            let unpaired = presented_frame(None, &curr, t, true);
            assert_eq!(positions(&unpaired, 0), vec![[4.0, 8.0]]);
        }
        let on = presented_frame(Some(&prev), &curr, 0.5, true);
        assert_eq!(positions(&on, 0), vec![[2.0, 4.0]]);
    }
}
