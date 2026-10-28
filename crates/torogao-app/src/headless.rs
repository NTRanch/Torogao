//! The headless verification mode: the same binary, run
//! with `--headless-eval`, loads and evaluates a model
//! without creating a window, prints the evaluated state
//! hash, and — when a GPU adapter is available — renders
//! offscreen to a PNG at the model's canvas size. This is
//! the surface the phase gate runs on machines without a
//! display.

use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::render::Renderer;
use crate::scene::{NamedAssignment, Scene};
use crate::state_hash::evaluated_state_hash;
use crate::textures::{RgbaImage, encode_png};

/// Largest number of distinct colours the statistics pass
/// counts exactly (sampling beyond this reports the cap).
const DISTINCT_COLOURS_CAP: usize = 1 << 16;

/// Options for one headless run, parsed by `main`.
#[derive(Debug, Default)]
pub struct HeadlessOptions {
    /// The model file to evaluate.
    pub model: PathBuf,
    /// Parameter assignments, in command-line order.
    pub params: Vec<NamedAssignment>,
    /// Where to write the offscreen render, when asked.
    pub render: Option<PathBuf>,
    /// Whether `--state-hash` was passed (the hash is
    /// printed either way; the flag exists for gate scripts
    /// that assert on its presence).
    pub state_hash: bool,
    /// Whether `--debug-physics` was passed: the render
    /// carries the physics debug overlay (G-10).
    pub debug_physics: bool,
}

/// Image statistics the gate reads: how much of the canvas
/// is not background, and how many distinct colours appear.
#[derive(Debug, Clone, Copy)]
pub struct ImageStats {
    /// Fraction of pixels that differ from transparent
    /// black (the render's background), 0.0..=1.0.
    pub non_background_fraction: f64,
    /// Distinct RGBA colours, capped at
    /// `DISTINCT_COLOURS_CAP`.
    pub distinct_colours: usize,
}

/// Computes the statistics over an RGBA image.
pub fn image_stats(image: &RgbaImage) -> ImageStats {
    let total = (image.width as usize) * (image.height as usize);
    let mut non_background = 0_usize;
    let mut seen = std::collections::HashSet::new();
    for pixel in image.pixels.chunks_exact(4) {
        if pixel != [0, 0, 0, 0] {
            non_background += 1;
        }
        if seen.len() < DISTINCT_COLOURS_CAP {
            seen.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
        }
    }
    ImageStats {
        non_background_fraction: if total == 0 {
            0.0
        } else {
            non_background as f64 / total as f64
        },
        distinct_colours: seen.len(),
    }
}

/// Runs one headless evaluation, printing the report lines
/// to stdout. Returns the process exit code.
pub fn run_headless(options: &HeadlessOptions) -> AppResult<i32> {
    let mut scene = Scene::load_file(&options.model)?;
    let assignments = scene.resolve_assignments(&options.params)?;
    scene.evaluate_fresh(&assignments)?;
    println!("model: {}", options.model.display());
    println!(
        "name: {} nodes: {} parameters: {} textures: {}",
        scene.model_name(),
        scene.puppet.nodes.nodes.len(),
        scene.puppet.params.len(),
        scene.textures.len(),
    );
    let hash = evaluated_state_hash(&mut scene.puppet);
    println!("state-hash: {hash:016x}");
    if options.state_hash {
        println!("state-hash-requested: yes");
    }
    if let Some(out_path) = &options.render {
        match Renderer::new() {
            Ok(mut renderer) => {
                let (image, report) = if options.debug_physics {
                    renderer.render_debug(&mut scene)?
                } else {
                    renderer.render(&mut scene)?
                };
                let stats = image_stats(&image);
                let encoded = encode_png(&image)?;
                std::fs::write(out_path, encoded).map_err(|err| AppError::Io {
                    context: "write render",
                    message: err.to_string(),
                })?;
                println!(
                    "render: {} {}x{} parts-drawn: {} parts-skipped: {}",
                    out_path.display(),
                    image.width,
                    image.height,
                    report.parts_drawn,
                    report.skipped.len(),
                );
                println!(
                    "render-stats: non-background-fraction: {:.6} distinct-colours: {}",
                    stats.non_background_fraction, stats.distinct_colours,
                );
                for reason in &report.skipped {
                    println!("skipped-part: {reason}");
                }
                for note in &report.notes {
                    println!("render-note: {note}");
                }
                for note in &report.blend_notes {
                    println!("blend-note: {note}");
                }
                for failure in &report.decode_failures {
                    println!("texture-note: {failure}");
                }
            }
            Err(err) => {
                println!("render: unavailable ({err})");
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::image_stats;
    use crate::textures::RgbaImage;

    #[test]
    fn stats_count_non_background_and_colours() {
        let mut pixels = vec![0_u8; 4 * 4 * 4];
        pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
        pixels[4..8].copy_from_slice(&[0, 255, 0, 255]);
        pixels[8..12].copy_from_slice(&[255, 0, 0, 255]);
        let stats = image_stats(&RgbaImage::new(4, 4, pixels));
        assert_eq!(stats.distinct_colours, 3);
        assert!((stats.non_background_fraction - 3.0 / 16.0).abs() < 1e-12);
    }
}
