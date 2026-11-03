//! Arm D determinism evidence (fps trial adoption,
//! D-610): presentation interpolation must leave the
//! canonical simulation untouched.
//!
//! The script below drives the Aka fixture through the
//! app's own canonical protocol — a fresh evaluation,
//! then twelve steps, each assigning the head yaw-pitch
//! and the left blink (one governed parameter walking
//! the head across its range, one deforming the eyelid)
//! before advancing exactly one 60 Hz step — twice: once
//! with presentation computed between steps exactly as
//! the shell computes it (interpolation on), once with
//! presentation selecting the canonical frame
//! (interpolation off). The evaluated-state hash chains
//! must be byte-identical, the off-run's presented
//! frames must be the canonical frames themselves, and
//! the on-run's half-phase presented frames must be the
//! exact midpoint blend of the bracketing evaluated
//! frames.

use std::path::PathBuf;

use torogao_app::present::presented_frame;
use torogao_app::render::{PreparedFrame, prepare_frame};
use torogao_app::scene::Scene;
use torogao_app::state_hash::evaluated_state_hash;
use torogao_engine::Vec2;

/// One scripted run: the evaluated-state hash after the
/// fresh evaluation and after each step, the evaluated
/// frames themselves, and the frame presented after each
/// step at phase 0.5.
struct ScriptRun {
    hashes: Vec<u64>,
    frames: Vec<PreparedFrame>,
    presented: Vec<PreparedFrame>,
}

fn run_script(interpolate: bool) -> Option<ScriptRun> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/example-models/aka/Aka.inx");
    if !path.exists() {
        eprintln!("Aka fixture not present; arm D determinism test skipped");
        return None;
    }
    let mut scene = Scene::load_file(&path).expect("fixture loads");
    let param_index = |name: &str| {
        scene
            .puppet
            .params
            .iter()
            .position(|param| param.name == name)
            .unwrap_or_else(|| panic!("fixture carries the {name} parameter"))
    };
    let head = param_index("Head:: Yaw-Pitch");
    let blink = param_index("Eye:: Left:: Blink");
    scene
        .evaluate_fresh(&[(head, Vec2::new(-0.6, 0.0)), (blink, Vec2::new(0.0, 0.0))])
        .expect("fresh evaluation");
    let mut hashes = vec![evaluated_state_hash(&mut scene.puppet)];
    let mut frames = vec![prepare_frame(&mut scene).expect("initial frame prepares")];
    let mut presented = Vec::new();
    for step in 1..=12u32 {
        // One canonical step per assignment batch, the
        // app's named-state application path.
        scene
            .assign_and_step(&[
                (head, Vec2::new(-0.6 + 0.1 * step as f32, 0.0)),
                (blink, Vec2::new(0.08 * step as f32, 0.0)),
            ])
            .expect("parameter step evaluates");
        hashes.push(evaluated_state_hash(&mut scene.puppet));
        let frame = prepare_frame(&mut scene).expect("step frame prepares");
        // Presentation is computed between steps, from
        // the two bracketing evaluated frames only — the
        // shell's order. It takes shared references; the
        // hash chain below proves it changed nothing.
        let prev = frames.last().expect("an earlier bracket exists");
        presented.push(presented_frame(Some(prev), &frame, 0.5, interpolate));
        // Other phases too, still derived data only.
        let _ = presented_frame(Some(prev), &frame, 0.25, interpolate);
        let _ = presented_frame(Some(prev), &frame, 0.75, interpolate);
        frames.push(frame);
    }
    Some(ScriptRun {
        hashes,
        frames,
        presented,
    })
}

fn positions_of(frame: &PreparedFrame) -> Vec<Vec<[f32; 2]>> {
    frame
        .parts
        .iter()
        .map(|part| part.positions.clone())
        .collect()
}

#[test]
fn interpolation_on_and_off_hash_chains_are_byte_identical() {
    let (Some(on), Some(off)) = (run_script(true), run_script(false)) else {
        return;
    };
    // The script must actually move the state, or the
    // comparison proves nothing.
    assert!(
        on.hashes.windows(2).any(|pair| pair[0] != pair[1]),
        "scripted steps must change the evaluated state"
    );
    assert_eq!(on.hashes.len(), 13);
    assert_eq!(
        on.hashes, off.hashes,
        "presentation interpolation altered the simulation hash chain"
    );
}

#[test]
fn interpolation_off_presents_the_canonical_frames() {
    let Some(off) = run_script(false) else {
        return;
    };
    assert_eq!(off.presented.len(), 12);
    for (index, presented) in off.presented.iter().enumerate() {
        let canonical = &off.frames[index + 1];
        assert_eq!(
            positions_of(presented),
            positions_of(canonical),
            "step {} presented frame differs from the canonical frame",
            index + 1
        );
        for (presented_part, canonical_part) in presented.parts.iter().zip(canonical.parts.iter()) {
            assert_eq!(presented_part.tint, canonical_part.tint);
            assert_eq!(presented_part.node, canonical_part.node);
        }
    }
}

#[test]
fn interpolation_on_presents_the_exact_midpoint_blend() {
    let Some(on) = run_script(true) else {
        return;
    };
    let mut blended_parts = 0usize;
    for (index, presented) in on.presented.iter().enumerate() {
        let prev = &on.frames[index];
        let curr = &on.frames[index + 1];
        assert_eq!(
            presented.parts.len(),
            prev.parts.len(),
            "presented structure must be the earlier frame's"
        );
        for (part_index, presented_part) in presented.parts.iter().enumerate() {
            let prev_part = &prev.parts[part_index];
            let curr_part = curr
                .parts
                .iter()
                .find(|part| part.node == prev_part.node)
                .expect("bracketing part exists");
            if prev_part.positions.len() != curr_part.positions.len() {
                // A mesh swap snaps: the earlier part
                // holds whole.
                assert_eq!(presented_part.positions, prev_part.positions);
                continue;
            }
            let mut moved = false;
            for (vertex, (from, to)) in presented_part
                .positions
                .iter()
                .zip(prev_part.positions.iter().zip(curr_part.positions.iter()))
            {
                assert_eq!(
                    *vertex,
                    [
                        from[0] + (to[0] - from[0]) * 0.5,
                        from[1] + (to[1] - from[1]) * 0.5
                    ]
                );
                moved |= from != to;
            }
            if moved {
                blended_parts += 1;
            }
        }
    }
    assert!(
        blended_parts > 0,
        "the script must actually exercise blended geometry"
    );
}
