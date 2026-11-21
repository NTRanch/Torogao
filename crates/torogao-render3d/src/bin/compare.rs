//! Comparison (a) tooling + D-221 verdict-pack
//! composition.
//!
//! Stats mode:
//!   render3d-compare --mine DIR --plates DIR [--char-only DIR]
//!                    --out stats.json [--frames 0,6,12,...]
//! Computes, per sampled frame, full-frame statistics and
//! (with --char-only) character-masked and set-region
//! splits of the same statistics.
//!
//! Pack mode (adds image composition for the Owner's eye):
//!   ... --pack DIR
//! For the pack frames (stage boundaries + the PEAK
//! exemplar) it writes plate|mine side-by-sides at 2x, a
//! face-region crop pair at 3x, and an amplified diff
//! image; plus a 12-frame contact sheet of the full arc.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use torogao_render3d::compare::{
    FrameStats, Image, character_mask, crop, diff_heat, frame_stats, load_png, masked_stats,
    side_by_side, stack, upscale, write_png,
};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

const PACK_FRAMES: [u32; 6] = [0, 90, 180, 225, 270, 359];
const BG: [u8; 4] = [24, 24, 28, 255];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(mine), Some(plates), Some(out)) = (
        arg_value(&args, "--mine"),
        arg_value(&args, "--plates"),
        arg_value(&args, "--out"),
    ) else {
        eprintln!(
            "usage: render3d-compare --mine DIR --plates DIR [--char-only DIR] --out stats.json [--pack DIR] [--frames a,b,c]"
        );
        return ExitCode::FAILURE;
    };
    let frames: Vec<u32> = arg_value(&args, "--frames")
        .map(|spec| {
            spec.split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect()
        })
        .unwrap_or_else(|| (0..360).step_by(6).collect());
    let char_only = arg_value(&args, "--char-only");
    let pack = arg_value(&args, "--pack");
    match run(
        &PathBuf::from(mine),
        &PathBuf::from(plates),
        char_only.as_deref(),
        &PathBuf::from(out),
        &frames,
        pack.as_deref(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("render3d-compare: {error}");
            ExitCode::FAILURE
        }
    }
}

fn load(dir: &Path, frame: u32) -> Result<Image, torogao_render3d::RenderError> {
    load_png(&dir.join(format!("f_{frame:04}.png")))
}

fn summarize(rows: &[FrameStats]) -> serde_json::Value {
    let n = rows.len().max(1) as f64;
    let mean = |f: &dyn Fn(&FrameStats) -> f64| rows.iter().map(f).sum::<f64>() / n;
    serde_json::json!({
        "frames": rows.len(),
        "changed_fraction_gate_mean": mean(&|r| r.changed_fraction_gate),
        "changed_fraction_gate_max": rows.iter().map(|r| r.changed_fraction_gate).fold(0.0, f64::max),
        "changed_fraction_strict_mean": mean(&|r| r.changed_fraction_strict),
        "mean_abs_delta_mean": [
            mean(&|r| r.mean_abs_delta[0]),
            mean(&|r| r.mean_abs_delta[1]),
            mean(&|r| r.mean_abs_delta[2]),
        ],
        "mean_signed_delta_mean": [
            mean(&|r| r.mean_signed_delta[0]),
            mean(&|r| r.mean_signed_delta[1]),
            mean(&|r| r.mean_signed_delta[2]),
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn run(
    mine_dir: &Path,
    plates_dir: &Path,
    char_only_dir: Option<&str>,
    out: &Path,
    frames: &[u32],
    pack_dir: Option<&str>,
) -> Result<(), torogao_render3d::RenderError> {
    let mut full_rows = Vec::new();
    let mut char_rows = Vec::new();
    let mut set_rows = Vec::new();
    for &frame in frames {
        let mine = load(mine_dir, frame)?;
        let plate = load(plates_dir, frame)?;
        full_rows.push(frame_stats(frame, &mine, &plate)?);
        if let Some(dir) = char_only_dir {
            let char_only = load(&PathBuf::from(dir), frame)?;
            // The clear colour of a char-only render is the
            // display-transformed world colour; recover it
            // from the corner pixel (never covered by the
            // character at plate framing).
            let clear = char_only.pixel(0, 0);
            let mask = character_mask(&char_only, clear);
            char_rows.push(masked_stats(frame, &mine, &plate, &mask)?);
            let inverse: Vec<bool> = mask.iter().map(|m| !m).collect();
            set_rows.push(masked_stats(frame, &mine, &plate, &inverse)?);
        }
    }
    println!(
        "{:>6} {:>10} {:>10} {:>22}",
        "frame", "chg_gate", "chg_strict", "mean_abs_delta (rgb)"
    );
    for row in &full_rows {
        println!(
            "{:>6} {:>10.4} {:>10.4} {:>7.2} {:>7.2} {:>7.2}",
            row.frame,
            row.changed_fraction_gate,
            row.changed_fraction_strict,
            row.mean_abs_delta[0],
            row.mean_abs_delta[1],
            row.mean_abs_delta[2]
        );
    }
    let report = serde_json::json!({
        "tool": "render3d-compare",
        "definitions": {
            "changed_fraction_gate": "fraction of pixels with summed abs RGB delta > 0.08 (float [0,1] scale) — the estate gate idiom",
            "changed_fraction_strict": "fraction of pixels with any channel abs delta > 16/255",
            "mean_abs_delta": "per-channel mean absolute delta, 0..255 units",
        },
        "full_frame": { "summary": summarize(&full_rows), "frames": full_rows },
        "character_masked": { "summary": summarize(&char_rows), "frames": char_rows },
        "set_region": { "summary": summarize(&set_rows), "frames": set_rows },
    });
    std::fs::write(out, serde_json::to_string_pretty(&report).unwrap())?;
    println!("wrote {}", out.display());

    if let Some(pack) = pack_dir {
        compose_pack(mine_dir, plates_dir, char_only_dir, &PathBuf::from(pack))?;
    }
    Ok(())
}

/// Face/decision region from the character mask's bounding
/// box: the top 38% of the box (head + shoulders at plate
/// framing), padded by 12 px. Data-driven so the crop
/// follows the actual silhouette, not a guess.
fn face_crop_rect(mask_image: &Image, clear: [u8; 4]) -> (u32, u32, u32, u32) {
    let (w, h) = (mask_image.width, mask_image.height);
    let (mut minx, mut miny, mut maxx, mut maxy) = (w, h, 0_u32, 0_u32);
    for y in 0..h {
        for x in 0..w {
            if mask_image.pixel(x, y) != clear {
                minx = minx.min(x);
                miny = miny.min(y);
                maxx = maxx.max(x);
                maxy = maxy.max(y);
            }
        }
    }
    if maxx < minx {
        return (0, 0, w, h / 2);
    }
    let head_h = ((maxy - miny) as f64 * 0.38) as u32;
    let pad = 12;
    let x0 = minx.saturating_sub(pad);
    let y0 = miny.saturating_sub(pad);
    let x1 = (maxx + pad).min(w);
    let y1 = (miny + head_h + pad).min(h);
    (x0, y0, x1 - x0, y1 - y0)
}

fn compose_pack(
    mine_dir: &Path,
    plates_dir: &Path,
    char_only_dir: Option<&str>,
    pack_dir: &Path,
) -> Result<(), torogao_render3d::RenderError> {
    std::fs::create_dir_all(pack_dir)?;
    for &frame in &PACK_FRAMES {
        let mine = load(mine_dir, frame)?;
        let plate = load(plates_dir, frame)?;
        let pair = side_by_side(&[&upscale(&plate, 2), &upscale(&mine, 2)], 8, BG);
        write_png(&pack_dir.join(format!("pair_f{frame:04}.png")), &pair)?;
        let heat = diff_heat(&mine, &plate, 4);
        let heat_pair = side_by_side(&[&upscale(&plate, 2), &upscale(&heat, 2)], 8, BG);
        write_png(&pack_dir.join(format!("diff_f{frame:04}.png")), &heat_pair)?;
        if let Some(dir) = char_only_dir {
            let char_only = load(&PathBuf::from(dir), frame)?;
            let clear = char_only.pixel(0, 0);
            let (x, y, cw, ch) = face_crop_rect(&char_only, clear);
            let plate_crop = upscale(&crop(&plate, x, y, cw, ch), 3);
            let mine_crop = upscale(&crop(&mine, x, y, cw, ch), 3);
            let pair = side_by_side(&[&plate_crop, &mine_crop], 8, BG);
            write_png(&pack_dir.join(format!("face_f{frame:04}.png")), &pair)?;
        }
        println!("pack frame {frame} composed");
    }
    // Contact sheet: every 30th frame, plate row over
    // mine row, at half scale (nearest x1 of a 256x320
    // downscale via crop-free 2x block pick: use upscale
    // machinery in reverse — simply compose at full size
    // in two rows of six, scaled by cropping is wrong;
    // instead stack pairs at 1x and let the viewer zoom).
    let mut rows = Vec::new();
    for &frame in &[0_u32, 30, 60, 90, 120, 150, 180, 210, 240, 270, 300, 330] {
        let mine = load(mine_dir, frame)?;
        let plate = load(plates_dir, frame)?;
        rows.push(side_by_side(&[&plate, &mine], 4, BG));
    }
    let refs: Vec<&Image> = rows.iter().collect();
    let sheet = stack(&refs, 4, BG);
    write_png(&pack_dir.join("contact_sheet.png"), &sheet)?;
    println!("pack written to {}", pack_dir.display());
    Ok(())
}
