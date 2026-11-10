//! Comparison (b): GPU skinning vs the CPU reference.
//!
//! THE BOUND, STATED BEFORE THE RUN: GPU positions must
//! agree with the f64 CPU reference (same packed inputs,
//! same equations) to within **1e-4 m** max absolute
//! component delta over every vertex of the body mesh, and
//! normals to within **1e-3** max absolute component
//! delta. Reasoning: f32 carries ~7 decimal digits; joint
//! matrices compose 4-5 levels of TRS products with
//! entries of order 1-2, and each vertex blends up to 4
//! joints plus up to ~8 sparse morph contributions —
//! accumulated rounding of order 1e-5 is expected; 1e-4
//! leaves a factor of 10 headroom while still catching
//! any structural error (wrong joint, wrong weight, wrong
//! delta) by orders of magnitude.
//!
//! Usage:
//!   render3d-crosscheck --inputs DIR --out FILE
//!                       [--frames 0,90,180,225,270,359]

use std::path::PathBuf;
use std::process::ExitCode;

use torogao_render3d::arc::load_arc_inputs;
use torogao_render3d::backend::RenderBackend;
use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::math::V3;
use torogao_render3d::morph::PackedMorphs;
use torogao_render3d::reference::skin_mesh;
use torogao_render3d::setscene::{PLATE_HEIGHT, PLATE_WIDTH};

/// Position agreement bound in metres (see header).
pub const POSITION_BOUND_M: f64 = 1e-4;
/// Normal agreement bound (unit-vector components).
pub const NORMAL_BOUND: f64 = 1e-3;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(inputs), Some(out)) = (arg_value(&args, "--inputs"), arg_value(&args, "--out"))
    else {
        eprintln!("usage: render3d-crosscheck --inputs DIR --out FILE [--frames a,b,c]");
        return ExitCode::FAILURE;
    };
    let frames: Vec<u32> = arg_value(&args, "--frames")
        .map(|spec| {
            spec.split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect()
        })
        .unwrap_or_else(|| vec![0, 90, 180, 225, 270, 359]);
    println!(
        "BOUND (stated before the run): positions <= {POSITION_BOUND_M} m, normals <= {NORMAL_BOUND}"
    );
    match run(&inputs, &out, &frames) {
        Ok(passed) => {
            if passed {
                println!("cross-check: WITHIN BOUND");
                ExitCode::SUCCESS
            } else {
                println!("cross-check: BOUND EXCEEDED");
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("render3d-crosscheck: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(inputs: &str, out: &str, frames: &[u32]) -> Result<bool, torogao_render3d::RenderError> {
    let arc = load_arc_inputs(&PathBuf::from(inputs))?;
    let bound = arc.bind()?;
    let packed_scene = arc.packed_scene();
    let mut backend = VulkanBackend::new()?;
    backend.upload_scene(&packed_scene, PLATE_WIDTH, PLATE_HEIGHT)?;

    let body_index = arc
        .model
        .meshes
        .iter()
        .position(|m| m.name == "Prima_Body")
        .ok_or(torogao_render3d::RenderError::Missing("Prima_Body mesh"))?;
    let packed_morphs = PackedMorphs::pack(&arc.model.meshes[body_index]);

    let mut rows = Vec::new();
    let mut all_within = true;
    for &frame in frames {
        let evaluated = arc.pose.evaluate_plate_frame(&bound, frame)?;
        let joint_matrices = arc.pose.joint_matrices(&arc.model, &evaluated);
        let joint_f32: Vec<[f32; 16]> = joint_matrices.iter().map(|m| m.to_f32_cols()).collect();
        let weights = arc.frame_weights(&bound, frame)?[body_index].clone();
        let (gpu_pos, gpu_nrm) = backend.skin_positions(body_index, &joint_f32, &weights)?;
        let (cpu_pos, cpu_nrm) = skin_mesh(
            &arc.model,
            body_index,
            &packed_morphs,
            &joint_matrices,
            &weights,
        );
        let mut max_pos = 0.0_f64;
        let mut sum_pos = 0.0_f64;
        let mut max_nrm = 0.0_f64;
        for v in 0..cpu_pos.len() {
            let d = V3::from_f32(gpu_pos[v]).minus(V3::from_f32(cpu_pos[v]));
            let delta = d.x.abs().max(d.y.abs()).max(d.z.abs());
            max_pos = max_pos.max(delta);
            sum_pos += d.length();
            let dn = V3::from_f32(gpu_nrm[v]).minus(V3::from_f32(cpu_nrm[v]));
            max_nrm = max_nrm.max(dn.x.abs().max(dn.y.abs()).max(dn.z.abs()));
        }
        let mean_pos = sum_pos / cpu_pos.len() as f64;
        let within = max_pos <= POSITION_BOUND_M && max_nrm <= NORMAL_BOUND;
        all_within = all_within && within;
        println!(
            "frame {frame}: max pos delta {max_pos:.3e} m, mean {mean_pos:.3e} m, max normal delta {max_nrm:.3e} — {}",
            if within {
                "within bound"
            } else {
                "BOUND EXCEEDED"
            }
        );
        rows.push(serde_json::json!({
            "frame": frame,
            "vertices": cpu_pos.len(),
            "max_position_delta_m": max_pos,
            "mean_position_delta_m": mean_pos,
            "max_normal_delta": max_nrm,
            "within_bound": within,
        }));
    }
    let report = serde_json::json!({
        "tool": "render3d-crosscheck",
        "device": backend.describe(),
        "bound": { "position_m": POSITION_BOUND_M, "normal": NORMAL_BOUND },
        "bound_stated_before_run": true,
        "mesh": "Prima_Body",
        "frames": rows,
        "all_within_bound": all_within,
    });
    std::fs::write(out, serde_json::to_string_pretty(&report).unwrap())?;
    Ok(all_within)
}
