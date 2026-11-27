//! Render Prima's 360-frame arc offscreen through the
//! Vulkan backend and write the frames + per-frame hashes
//! to an evidence directory.
//!
//! Usage:
//!   render3d-arc --inputs DIR --out DIR [--frames A-B]
//!                [--no-set] [--char-only]
//!
//! --no-set / --char-only are the same switch (kept under
//! both names for the comparison tooling): render only the
//! character, for the character-mask statistics.

use std::path::PathBuf;
use std::process::ExitCode;

use torogao_render3d::arc::load_arc_inputs;
use torogao_render3d::backend::RenderBackend;
use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::setscene::{PLATE_FRAMES, PLATE_HEIGHT, PLATE_WIDTH};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(inputs) = arg_value(&args, "--inputs") else {
        eprintln!("usage: render3d-arc --inputs DIR --out DIR [--frames A-B] [--char-only]");
        return ExitCode::FAILURE;
    };
    let Some(out) = arg_value(&args, "--out") else {
        eprintln!("missing --out DIR");
        return ExitCode::FAILURE;
    };
    let include_set = !args.iter().any(|a| a == "--char-only" || a == "--no-set");
    let (first, last) = match arg_value(&args, "--frames") {
        Some(spec) => {
            let mut parts = spec.split('-');
            let a: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let b: u32 = parts
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(PLATE_FRAMES - 1);
            (a, b.min(PLATE_FRAMES - 1))
        }
        None => (0, PLATE_FRAMES - 1),
    };
    if let Err(error) = run(&inputs, &out, first, last, include_set) {
        eprintln!("render3d-arc: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run(
    inputs: &str,
    out: &str,
    first: u32,
    last: u32,
    include_set: bool,
) -> Result<(), torogao_render3d::RenderError> {
    let out_dir = PathBuf::from(out);
    std::fs::create_dir_all(&out_dir)?;
    let arc = load_arc_inputs(&PathBuf::from(inputs))?;
    let bound = arc.bind()?;
    let packed = arc.packed_scene();
    let mut backend = VulkanBackend::new()?;
    backend.upload_scene(&packed, PLATE_WIDTH, PLATE_HEIGHT)?;
    println!("device: {}", backend.describe());

    let mut hashes: Vec<(u32, String)> = Vec::new();
    for frame in first..=last {
        let draw = arc.draw_frame(&bound, frame, include_set)?;
        let rendered = backend.render_frame(&draw)?;
        let hash = sha256_hex(&rendered.rgba);
        let image = Image {
            width: rendered.width,
            height: rendered.height,
            rgba: rendered.rgba,
        };
        write_png(&out_dir.join(format!("f_{frame:04}.png")), &image)?;
        hashes.push((frame, hash));
        if frame % 60 == 0 || frame == last {
            println!("frame {frame} done");
        }
    }
    let manifest = serde_json::json!({
        "tool": "render3d-arc",
        "device": backend.describe(),
        "plate_resolution": [PLATE_WIDTH, PLATE_HEIGHT],
        "frames": { "first": first, "last": last },
        "include_set": include_set,
        "inputs": arc.input_hashes.iter().map(|(n, h)| serde_json::json!({"file": n, "sha256": h})).collect::<Vec<_>>(),
        "frame_hashes_rgba_sha256": hashes.iter().map(|(f, h)| serde_json::json!({"frame": f, "sha256": h})).collect::<Vec<_>>(),
    });
    std::fs::write(
        out_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )?;
    println!("wrote {} frames to {out}", hashes.len());
    Ok(())
}
