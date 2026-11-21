//! Render Prima's 360-frame arc offscreen through the
//! Vulkan backend and write the frames + per-frame hashes
//! to an evidence directory.
//!
//! Usage:
//!   render3d-arc --inputs DIR --out DIR [--frames A-B]
//!                [--no-set] [--char-only]
//!                [--size WxH] [--count N] [--hash-only]
//!
//! --size WxH renders at another frame size (default: the
//! plate's 512x640). The vertical field of view is kept and
//! the horizontal one follows the new aspect, so pixels
//! stay square. --count N renders N frames, looping the
//! 360-frame arc (output frame i draws arc frame i mod
//! 360); it cannot be combined with --frames. --hash-only
//! hashes every frame but writes no PNG (cost runs).
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

/// Largest accepted frame edge, in pixels.
const SIZE_EDGE_MAX_PX: u32 = 8192;
/// Largest accepted `--count` (one hour at 60 fps), in frames.
const COUNT_MAX_FRAMES: u32 = 216_000;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

/// Parse `WxH`: both edges in 1..=SIZE_EDGE_MAX_PX, nothing else accepted.
fn parse_size(spec: &str) -> Result<(u32, u32), String> {
    let Some((width, height)) = spec.split_once('x') else {
        return Err(format!("--size wants WxH, got {spec:?}"));
    };
    let edge = |text: &str| -> Result<u32, String> {
        match text.parse::<u32>() {
            Ok(px) if (1..=SIZE_EDGE_MAX_PX).contains(&px) => Ok(px),
            _ => Err(format!(
                "--size edge {text:?} is not in 1..={SIZE_EDGE_MAX_PX}"
            )),
        }
    };
    Ok((edge(width)?, edge(height)?))
}

/// Parse `--count`: 1..=COUNT_MAX_FRAMES.
fn parse_count(spec: &str) -> Result<u32, String> {
    match spec.parse::<u32>() {
        Ok(frames) if (1..=COUNT_MAX_FRAMES).contains(&frames) => Ok(frames),
        _ => Err(format!("--count {spec:?} is not in 1..={COUNT_MAX_FRAMES}")),
    }
}

/// What one run renders.
struct RunPlan {
    width: u32,
    height: u32,
    /// Output frame indices `first..=last`; each draws arc frame `index % PLATE_FRAMES`.
    first: u32,
    last: u32,
    include_set: bool,
    write_png: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(inputs) = arg_value(&args, "--inputs") else {
        eprintln!(
            "usage: render3d-arc --inputs DIR --out DIR [--frames A-B] [--char-only] \
             [--size WxH] [--count N] [--hash-only]"
        );
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
    let (width, height) = match arg_value(&args, "--size").map(|spec| parse_size(&spec)) {
        Some(Ok(size)) => size,
        Some(Err(message)) => {
            eprintln!("render3d-arc: {message}");
            return ExitCode::FAILURE;
        }
        None => (PLATE_WIDTH, PLATE_HEIGHT),
    };
    let (first, last) = match arg_value(&args, "--count").map(|spec| parse_count(&spec)) {
        Some(Ok(_)) if args.iter().any(|a| a == "--frames") => {
            eprintln!("render3d-arc: --count and --frames cannot be combined");
            return ExitCode::FAILURE;
        }
        Some(Ok(frames)) => (0, frames - 1),
        Some(Err(message)) => {
            eprintln!("render3d-arc: {message}");
            return ExitCode::FAILURE;
        }
        None => (first, last),
    };
    let plan = RunPlan {
        width,
        height,
        first,
        last,
        include_set,
        write_png: !args.iter().any(|a| a == "--hash-only"),
    };
    if let Err(error) = run(&inputs, &out, &plan) {
        eprintln!("render3d-arc: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run(inputs: &str, out: &str, plan: &RunPlan) -> Result<(), torogao_render3d::RenderError> {
    let (first, last, include_set) = (plan.first, plan.last, plan.include_set);
    let out_dir = PathBuf::from(out);
    std::fs::create_dir_all(&out_dir)?;
    let mut arc = load_arc_inputs(&PathBuf::from(inputs))?;
    if (plan.width, plan.height) != (PLATE_WIDTH, PLATE_HEIGHT) {
        // Keep the plate's vertical field of view; the horizontal one follows
        // the new aspect so pixels stay square. The plate size is left exactly
        // as measured (no arithmetic touches it).
        arc.scene.tan_half_x =
            arc.scene.tan_half_y * f64::from(plan.width) / f64::from(plan.height);
    }
    let bound = arc.bind()?;
    let packed = arc.packed_scene();
    let mut backend = VulkanBackend::new()?;
    backend.upload_scene(&packed, plan.width, plan.height)?;
    println!("device: {}", backend.describe());

    let mut hashes: Vec<(u32, String)> = Vec::new();
    for frame in first..=last {
        let draw = arc.draw_frame(&bound, frame % PLATE_FRAMES, include_set)?;
        let rendered = backend.render_frame(&draw)?;
        let hash = sha256_hex(&rendered.rgba);
        if plan.write_png {
            let image = Image {
                width: rendered.width,
                height: rendered.height,
                rgba: rendered.rgba,
            };
            write_png(&out_dir.join(format!("f_{frame:04}.png")), &image)?;
        }
        hashes.push((frame, hash));
        if frame % 60 == 0 || frame == last {
            println!("frame {frame} done");
        }
    }
    let manifest = serde_json::json!({
        "tool": "render3d-arc",
        "device": backend.describe(),
        "plate_resolution": [plan.width, plan.height],
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

#[cfg(test)]
mod tests {
    use super::{COUNT_MAX_FRAMES, SIZE_EDGE_MAX_PX, parse_count, parse_size};

    // validation
    #[test]
    fn size_accepts_1080p() {
        assert_eq!(parse_size("1920x1080"), Ok((1920, 1080)));
    }

    #[test]
    fn size_accepts_the_bounds() {
        assert_eq!(parse_size("1x1"), Ok((1, 1)));
        let edge = SIZE_EDGE_MAX_PX;
        assert_eq!(parse_size(&format!("{edge}x{edge}")), Ok((edge, edge)));
    }

    #[test]
    fn count_accepts_the_bounds() {
        assert_eq!(parse_count("1"), Ok(1));
        assert_eq!(parse_count("3600"), Ok(3600));
        assert_eq!(
            parse_count(&COUNT_MAX_FRAMES.to_string()),
            Ok(COUNT_MAX_FRAMES)
        );
    }

    // adversarial
    #[test]
    fn size_rejects_zero_and_oversize() {
        assert!(parse_size("0x1080").is_err());
        assert!(parse_size("1920x0").is_err());
        assert!(parse_size(&format!("{}x8", SIZE_EDGE_MAX_PX + 1)).is_err());
        assert!(parse_size("4294967296x8").is_err());
    }

    #[test]
    fn size_rejects_malformed() {
        for spec in [
            "",
            "1920",
            "1920x",
            "x1080",
            "1920X1080",
            "1920x1080x3",
            "-1x8",
            " 8x8",
            "8.0x8",
        ] {
            assert!(parse_size(spec).is_err(), "{spec:?} was accepted");
        }
    }

    #[test]
    fn count_rejects_zero_oversize_and_malformed() {
        for spec in ["0", "-1", "", "abc", "1.5", "4294967296"] {
            assert!(parse_count(spec).is_err(), "{spec:?} was accepted");
        }
        assert!(parse_count(&(COUNT_MAX_FRAMES + 1).to_string()).is_err());
    }
}
