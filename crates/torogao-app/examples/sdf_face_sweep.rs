//! Renders the SDF face material's CPU reference as a four-panel
//! calibration sweep (T-11, D-740): one synthetic painted-
//! terminator face under three light azimuths, plus the
//! range-debug view under a deliberately imperfect calibration.
//!
//! This is **evidence for the Owner's eye**, not a pass: the
//! synthetic SDF texture is a stand-in (a linear terminator with
//! a centre ridge), painted by formula, not by an artist. What
//! the sweep demonstrates is the material's behaviour — the
//! terminator sweeping with azimuth, the two cel bands, and the
//! debug view's red/green/blue calibration read.
//!
//! Usage: `cargo run --example sdf_face_sweep -- <output.png>`

use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::process::ExitCode;

use torogao_app::sdf_face::{SdfFaceCalibration, light_side, mirrored_u, shade_face};

const PANEL: usize = 256;
const PANELS: usize = 4;
const SKIN: [f32; 3] = [0.96, 0.80, 0.68];
const BACKGROUND: [f32; 3] = [0.12, 0.12, 0.14];

/// The synthetic painted SDF, in *sampled* space (after the
/// light-side mirror): low at the sampled-space left edge — the
/// side away from the light, which shadows first — rising
/// across the face, with a raised ridge down the centre (the
/// nose's stand-in, in face space). Painted in face-UV space
/// (v up).
fn painted_sdf(u: f32, v: f32) -> f32 {
    (0.15 + u * 0.85).clamp(0.0, 1.0) * (0.92 + 0.08 * v)
}

fn nose_ridge(u: f32, v: f32) -> f32 {
    0.08 * (-(u - 0.5).powi(2) / 0.004).exp() * (0.3 + 0.7 * v)
}

fn inside_face(u: f32, v: f32) -> bool {
    let du = (u - 0.5) / 0.42;
    let dv = (v - 0.52) / 0.46;
    du * du + dv * dv <= 1.0
}

fn to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: sdf_face_sweep <output.png>");
        return ExitCode::FAILURE;
    };
    let production = SdfFaceCalibration::identity();
    // Deliberately imperfect: the painted range is wider than
    // this bracket, so the debug panel shows red and blue, and
    // the green mid line sits at the face centre as placed.
    let imperfect = SdfFaceCalibration::new(0.25, 0.5, 0.75).expect("static triple is valid");
    let width = PANEL * PANELS;
    let mut pixels = vec![0u8; width * PANEL * 3];
    for row in 0..PANEL {
        let v = 1.0 - (row as f32 + 0.5) / PANEL as f32;
        for panel in 0..PANELS {
            for column in 0..PANEL {
                let u = (column as f32 + 0.5) / PANEL as f32;
                // Production panels sample the SDF mirrored by
                // light side, then add the face-space nose ridge;
                // the debug panel inspects the authored texture
                // (sampled-space ramp plus ridge), unmirrored.
                let ridge = nose_ridge(u, v);
                let colour = if !inside_face(u, v) {
                    BACKGROUND
                } else {
                    match panel {
                        0 => {
                            let side = light_side((-60.0f32).to_radians(), 0.0);
                            let painted =
                                (painted_sdf(mirrored_u(u, side), v) + ridge).clamp(0.0, 1.0);
                            shade_face(SKIN, painted, side, production, false)
                        }
                        1 => {
                            let painted = (painted_sdf(u, v) + ridge).clamp(0.0, 1.0);
                            shade_face(SKIN, painted, light_side(0.0, 0.0), production, false)
                        }
                        2 => {
                            let side = light_side(60.0f32.to_radians(), 0.0);
                            let painted =
                                (painted_sdf(mirrored_u(u, side), v) + ridge).clamp(0.0, 1.0);
                            shade_face(SKIN, painted, side, production, false)
                        }
                        _ => {
                            let painted = (painted_sdf(u, v) + ridge).clamp(0.0, 1.0);
                            shade_face(SKIN, painted, 0.0, imperfect, true)
                        }
                    }
                };
                let x = panel * PANEL + column;
                let at = (row * width + x) * 3;
                pixels[at] = to_u8(colour[0]);
                pixels[at + 1] = to_u8(colour[1]);
                pixels[at + 2] = to_u8(colour[2]);
            }
        }
    }
    let file = File::create(&path).expect("create output png");
    let mut encoder = png::Encoder::new(BufWriter::new(file), width as u32, PANEL as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(&pixels).expect("png data");
    writer.finish().expect("png finish");
    println!("wrote {path} ({width}x{PANEL}, panels: azimuth -60/0/+60, range-debug)");
    ExitCode::SUCCESS
}
