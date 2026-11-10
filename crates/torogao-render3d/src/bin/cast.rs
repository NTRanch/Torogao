//! Cast-generic M1 gates for any cast girl, character-only
//! under the plate camera and rig.
//!
//! Usage:
//!   render3d-cast info       --inputs DIR --character SLUG --outfit-plan P
//!                            [--bust-weight W] --out FILE
//!   render3d-cast crosscheck (same) --out FILE [--frames a,b,c]
//!   render3d-cast render     (same) --out DIR
//!
//! `info` is CPU-only: decoded census, clip replay hash,
//! input hashes. `crosscheck` and `render` need a Vulkan
//! device.
//!
//! THE BOUND, STATED BEFORE THE RUN (crosscheck; the rung-1
//! bound, unchanged): GPU positions within **1e-4 m** and
//! normals within **1e-3** max absolute component delta of
//! the f64 CPU reference over every vertex of the body mesh.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use torogao_render3d::RenderError;
use torogao_render3d::backend::RenderBackend;
use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::cast::{CastInputs, CastOptions, OutfitPlan, load_cast_inputs};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::error::write_file;
use torogao_render3d::math::V3;
use torogao_render3d::morph::PackedMorphs;
use torogao_render3d::reference::skin_mesh;
use torogao_render3d::setscene::{PLATE_FRAMES, PLATE_HEIGHT, PLATE_WIDTH};

/// Position agreement bound in metres (see header).
const POSITION_BOUND_M: f64 = 1e-4;
/// Normal agreement bound (unit-vector components).
const NORMAL_BOUND: f64 = 1e-3;
/// Most frames one crosscheck run accepts.
const CROSSCHECK_FRAMES_MAX: usize = 64;
/// The crosscheck frames of record.
const CROSSCHECK_FRAMES_DEFAULT: [u32; 6] = [0, 90, 180, 225, 270, 359];

const USAGE: &str = "usage: render3d-cast <info|crosscheck|render> --inputs DIR \
--character SLUG --outfit-plan START:INDEX[,..] [--bust-weight W] --out PATH [--frames a,b,c]";

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn usage_error(detail: &str) -> RenderError {
    RenderError::Document(format!("{detail}\n{USAGE}"))
}

fn required(args: &[String], name: &str) -> Result<String, RenderError> {
    arg_value(args, name).ok_or_else(|| usage_error(&format!("missing {name}")))
}

fn load(args: &[String]) -> Result<CastInputs, RenderError> {
    let bust_weight = match arg_value(args, "--bust-weight") {
        Some(text) => Some(
            text.parse::<f32>()
                .map_err(|_| usage_error("--bust-weight is not a number"))?,
        ),
        None => None,
    };
    let options = CastOptions {
        slug: required(args, "--character")?,
        outfit_plan: OutfitPlan::parse(&required(args, "--outfit-plan")?)?,
        bust_weight,
    };
    load_cast_inputs(Path::new(&required(args, "--inputs")?), &options)
}

fn parse_frames(args: &[String]) -> Result<Vec<u32>, RenderError> {
    let Some(spec) = arg_value(args, "--frames") else {
        return Ok(CROSSCHECK_FRAMES_DEFAULT.to_vec());
    };
    let mut frames = Vec::new();
    for part in spec.split(',') {
        if frames.len() == CROSSCHECK_FRAMES_MAX {
            return Err(usage_error("too many --frames"));
        }
        let frame: u32 = part
            .parse()
            .map_err(|_| usage_error("--frames entry is not a frame"))?;
        frames.push(frame);
    }
    Ok(frames)
}

/// The identity block every artifact carries: who was
/// loaded, from what bytes, under which render-time choices.
fn identity(cast: &CastInputs) -> serde_json::Value {
    serde_json::json!({
        "character": cast.slug,
        "outfit_plan": cast.outfit_plan.describe(),
        "bust_weight": match cast.bust_weight {
            Some(weight) => serde_json::json!(weight),
            None => serde_json::json!("as saved in the GLB"),
        },
        "inputs": cast.input_hashes.iter()
            .map(|(name, hash)| serde_json::json!({"file": name, "sha256": hash}))
            .collect::<Vec<_>>(),
    })
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), RenderError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| RenderError::Document(format!("report encode: {e}")))?;
    write_file(path, text.as_bytes())
}

fn run_info(args: &[String]) -> Result<bool, RenderError> {
    let cast = load(args)?;
    let bound = cast.bind()?;
    let census = cast.census();
    let report = serde_json::json!({
        "tool": "render3d-cast info",
        "identity": identity(&cast),
        "clip_frames": cast.clip.frames,
        "replay_hash": bound.replay_hash(),
        "census": {
            "joints": census.joints,
            "nodes": census.nodes,
            "meshes": census.meshes.iter()
                .map(|(name, vertices, targets)| serde_json::json!({
                    "name": name, "vertices": vertices, "morph_targets": targets,
                }))
                .collect::<Vec<_>>(),
        },
    });
    write_json(Path::new(&required(args, "--out")?), &report)?;
    println!("{}: replay hash {}", cast.slug, bound.replay_hash());
    Ok(true)
}

fn run_crosscheck(args: &[String]) -> Result<bool, RenderError> {
    let cast = load(args)?;
    let frames = parse_frames(args)?;
    let bound = cast.bind()?;
    let body_index = cast.body_index()?;
    let packed_morphs = PackedMorphs::pack(&cast.model.meshes[body_index]);
    println!(
        "BOUND (stated before the run): positions <= {POSITION_BOUND_M} m, \
         normals <= {NORMAL_BOUND}"
    );
    let mut backend = VulkanBackend::new()?;
    backend.upload_scene(&cast.packed_scene(), PLATE_WIDTH, PLATE_HEIGHT)?;

    let mut rows = Vec::with_capacity(frames.len());
    let mut all_within = true;
    for &frame in &frames {
        let evaluated = cast.pose.evaluate_plate_frame(&bound, frame)?;
        let joints = cast.pose.joint_matrices(&cast.model, &evaluated);
        let joints_f32: Vec<[f32; 16]> = joints.iter().map(|m| m.to_f32_cols()).collect();
        let weights = cast.frame_weights(&bound, frame)?[body_index].clone();
        let (gpu_pos, gpu_nrm) = backend.skin_positions(body_index, &joints_f32, &weights)?;
        let (cpu_pos, cpu_nrm) =
            skin_mesh(&cast.model, body_index, &packed_morphs, &joints, &weights);
        if gpu_pos.len() != cpu_pos.len() || gpu_nrm.len() != cpu_nrm.len() {
            return Err(RenderError::Backend(
                "skin readback length differs from the CPU reference".to_string(),
            ));
        }
        let max_delta = |a: &[[f32; 3]], b: &[[f32; 3]]| -> f64 {
            a.iter().zip(b).fold(0.0_f64, |worst, (x, y)| {
                let d = V3::from_f32(*x).minus(V3::from_f32(*y));
                worst.max(d.x.abs()).max(d.y.abs()).max(d.z.abs())
            })
        };
        let max_pos = max_delta(&gpu_pos, &cpu_pos);
        let max_nrm = max_delta(&gpu_nrm, &cpu_nrm);
        let within = max_pos <= POSITION_BOUND_M && max_nrm <= NORMAL_BOUND;
        all_within = all_within && within;
        println!("frame {frame}: max pos delta {max_pos:.3e} m, max normal delta {max_nrm:.3e}");
        rows.push(serde_json::json!({
            "frame": frame, "vertices": cpu_pos.len(),
            "max_position_delta_m": max_pos, "max_normal_delta": max_nrm,
            "within_bound": within,
        }));
    }
    let report = serde_json::json!({
        "tool": "render3d-cast crosscheck",
        "identity": identity(&cast),
        "device": backend.describe(),
        "bound": { "position_m": POSITION_BOUND_M, "normal": NORMAL_BOUND },
        "bound_stated_before_run": true,
        "mesh": cast.model.meshes[body_index].name,
        "frames": rows,
        "all_within_bound": all_within,
    });
    write_json(Path::new(&required(args, "--out")?), &report)?;
    Ok(all_within)
}

fn run_render(args: &[String]) -> Result<bool, RenderError> {
    let cast = load(args)?;
    let out_dir = PathBuf::from(required(args, "--out")?);
    std::fs::create_dir_all(&out_dir).map_err(|error| RenderError::Io {
        path: out_dir.display().to_string(),
        detail: error.to_string(),
    })?;
    let bound = cast.bind()?;
    let mut backend = VulkanBackend::new()?;
    backend.upload_scene(&cast.packed_scene(), PLATE_WIDTH, PLATE_HEIGHT)?;
    println!("device: {}", backend.describe());

    let mut hashes = Vec::with_capacity(PLATE_FRAMES as usize);
    for frame in 0..PLATE_FRAMES {
        let rendered = backend.render_frame(&cast.draw_frame(&bound, frame)?)?;
        hashes.push(serde_json::json!({"frame": frame, "sha256": sha256_hex(&rendered.rgba)}));
        let image = Image {
            width: rendered.width,
            height: rendered.height,
            rgba: rendered.rgba,
        };
        write_png(&out_dir.join(format!("f_{frame:04}.png")), &image)?;
    }
    let manifest = serde_json::json!({
        "tool": "render3d-cast render",
        "identity": identity(&cast),
        "device": backend.describe(),
        "plate_resolution": [PLATE_WIDTH, PLATE_HEIGHT],
        "frames": { "first": 0, "last": PLATE_FRAMES - 1 },
        "include_set": false,
        "replay_hash": bound.replay_hash(),
        "frame_hashes_rgba_sha256": hashes,
    });
    write_json(&out_dir.join("manifest.json"), &manifest)?;
    println!("wrote {PLATE_FRAMES} frames to {}", out_dir.display());
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let outcome = match args.get(1).map(String::as_str) {
        Some("info") => run_info(&args),
        Some("crosscheck") => run_crosscheck(&args),
        Some("render") => run_render(&args),
        _ => Err(usage_error("unknown or missing subcommand")),
    };
    match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            println!("render3d-cast: BOUND EXCEEDED");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("render3d-cast: {error}");
            ExitCode::FAILURE
        }
    }
}
