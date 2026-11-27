//! `hero-key` — the drawn-spike Half-2 harness: turns a
//! design-ops JSON file (generated from pass 2's
//! `p2_design.py` geometry + locked palette) into a
//! torogao-paint `SessionScript`, saves it as RON (the op
//! list as data), reloads it, replays it twice, and
//! exports the rendered frame. The twin replay must be
//! byte-identical (equal composite hashes) or the harness
//! fails — determinism is the gate, not a hope.
//!
//! Usage: hero-key <ops.json> <script.ron> <frame.png>

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use serde::Deserialize;
use torogao_paint::color::Rgba;
use torogao_paint::layer::LayerId;
use torogao_paint::ops::{GradientShape, GradientSpec, GradientStop, Op, Rect};
use torogao_paint::script::SessionScript;
use torogao_paint::stroke::{
    GrainSpec, PressureMap, Stamp, StampMode, StrokeParams, StrokeSample, build_stamp_run,
};

/// The grain field of a textured op, JSON-side.
#[derive(Deserialize)]
struct GrainJson {
    seed: u64,
    cell_px: u32,
    strength: u8,
    octaves: u8,
}

impl From<&GrainJson> for GrainSpec {
    fn from(json: &GrainJson) -> GrainSpec {
        GrainSpec {
            seed: json.seed,
            cell_px: json.cell_px,
            strength: json.strength,
            octaves: json.octaves,
        }
    }
}

/// Stroke samples, JSON-side: `[x, y]` or
/// `[x, y, pressure]` per sample (pressure defaults 1).
fn parse_samples(raw: &[Vec<f64>]) -> Result<Vec<StrokeSample>, String> {
    raw.iter()
        .map(|sample| match sample.len() {
            2 => Ok(StrokeSample {
                x_px: sample[0],
                y_px: sample[1],
                pressure: 1.0,
            }),
            3 => Ok(StrokeSample {
                x_px: sample[0],
                y_px: sample[1],
                pressure: sample[2],
            }),
            n => Err(format!("stroke sample has {n} components, want 2 or 3")),
        })
        .collect()
}

/// Build the stamp placements of one JSON stroke.
fn build_stroke(
    samples: &[Vec<f64>],
    color: Rgba,
    opacity: u8,
    flow: u8,
    diameter: u32,
    pressure_size: bool,
    pressure_alpha: bool,
) -> Result<Vec<Stamp>, String> {
    let samples = parse_samples(samples)?;
    let params = StrokeParams {
        color,
        diameter_px: diameter,
        opacity,
        flow,
        spacing_fraction: 0.3,
        stabilizer_alpha: 1.0,
        pressure_map: PressureMap {
            size: pressure_size,
            alpha: pressure_alpha,
        },
    };
    build_stamp_run(&samples, &params).map_err(|error| format!("stroke build: {error}"))
}

#[derive(Deserialize)]
struct OpsFile {
    width: u32,
    height: u32,
    layers: Vec<String>,
    ops: Vec<OpSpec>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OpSpec {
    FillRect {
        layer: String,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        color: Rgba,
    },
    Stroke {
        layer: String,
        color: Rgba,
        opacity: u8,
        flow: u8,
        diameter: u32,
        /// `[x, y]` or `[x, y, pressure]` per sample.
        samples: Vec<Vec<f64>>,
        /// Pressure drives stamp radius (default false:
        /// the spike's pinned-pressure behaviour).
        #[serde(default)]
        pressure_size: bool,
        /// Pressure drives stamp alpha (default false).
        #[serde(default)]
        pressure_alpha: bool,
    },
    Dabs {
        layer: String,
        color: Rgba,
        opacity: u8,
        flow: u8,
        /// `[x, y, radius_px, alpha]` per stamp.
        stamps: Vec<[f64; 4]>,
    },
    GradientFill {
        layer: String,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        /// "linear" or "radial".
        shape: String,
        from: [f64; 2],
        to: [f64; 2],
        /// `[at, [r, g, b, a]]` per stop, increasing `at`.
        stops: Vec<(u8, Rgba)>,
    },
    TexturedDabs {
        layer: String,
        color: Rgba,
        opacity: u8,
        flow: u8,
        /// `[x, y, radius_px, alpha]` per stamp.
        stamps: Vec<[f64; 4]>,
        grain: GrainJson,
    },
    TexturedStroke {
        layer: String,
        color: Rgba,
        opacity: u8,
        flow: u8,
        diameter: u32,
        /// `[x, y]` or `[x, y, pressure]` per sample.
        samples: Vec<Vec<f64>>,
        #[serde(default)]
        pressure_size: bool,
        #[serde(default)]
        pressure_alpha: bool,
        grain: GrainJson,
    },
}

fn fail(message: String) -> ExitCode {
    eprintln!("hero-key: {message}");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("usage: hero-key <ops.json> <script.ron> <frame.png>");
        return ExitCode::from(2);
    }
    let (ops_path, ron_path, png_path) = (&args[0], &args[1], &args[2]);

    let text = match std::fs::read_to_string(ops_path) {
        Ok(text) => text,
        Err(error) => return fail(format!("read {ops_path}: {error}")),
    };
    let spec: OpsFile = match serde_json::from_str(&text) {
        Ok(spec) => spec,
        Err(error) => return fail(format!("parse {ops_path}: {error}")),
    };

    // Layer ids in stack order (bottom first), issued
    // before any paint op.
    let mut layer_ids: BTreeMap<String, LayerId> = BTreeMap::new();
    let mut ops: Vec<Op> = Vec::new();
    for (index, name) in spec.layers.iter().enumerate() {
        let id = LayerId((index + 1) as u32);
        layer_ids.insert(name.clone(), id);
        ops.push(Op::LayerAdd {
            id,
            name: name.clone(),
            after: None,
        });
    }
    let layer_of = |name: &str| -> Result<LayerId, String> {
        layer_ids
            .get(name)
            .copied()
            .ok_or_else(|| format!("op names undeclared layer {name:?}"))
    };

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for op in &spec.ops {
        match op {
            OpSpec::FillRect {
                layer,
                x,
                y,
                w,
                h,
                color,
            } => {
                let id = match layer_of(layer) {
                    Ok(id) => id,
                    Err(message) => return fail(message),
                };
                ops.push(Op::FillRect {
                    layer: id,
                    rect: Rect {
                        x: *x,
                        y: *y,
                        width: *w,
                        height: *h,
                    },
                    color: *color,
                });
                *counts.entry("fill_rect").or_default() += 1;
            }
            OpSpec::Stroke {
                layer,
                color,
                opacity,
                flow,
                diameter,
                samples,
                pressure_size,
                pressure_alpha,
            } => {
                let id = match layer_of(layer) {
                    Ok(id) => id,
                    Err(message) => return fail(message),
                };
                let stamps = match build_stroke(
                    samples,
                    *color,
                    *opacity,
                    *flow,
                    *diameter,
                    *pressure_size,
                    *pressure_alpha,
                ) {
                    Ok(stamps) => stamps,
                    Err(error) => return fail(format!("stroke on {layer}: {error}")),
                };
                ops.push(Op::StampRun {
                    layer: id,
                    mode: StampMode::Paint,
                    color: *color,
                    opacity: *opacity,
                    flow: *flow,
                    stamps,
                });
                *counts.entry("stroke").or_default() += 1;
            }
            OpSpec::Dabs {
                layer,
                color,
                opacity,
                flow,
                stamps,
            } => {
                let id = match layer_of(layer) {
                    Ok(id) => id,
                    Err(message) => return fail(message),
                };
                let stamps: Vec<Stamp> = stamps
                    .iter()
                    .map(|[x, y, radius, alpha]| Stamp {
                        x_fixed: (x * 64.0).round() as i64,
                        y_fixed: (y * 64.0).round() as i64,
                        radius_px: (radius.round() as u32).clamp(1, 512),
                        alpha: alpha.round().clamp(0.0, 255.0) as u8,
                    })
                    .collect();
                ops.push(Op::StampRun {
                    layer: id,
                    mode: StampMode::Paint,
                    color: *color,
                    opacity: *opacity,
                    flow: *flow,
                    stamps,
                });
                *counts.entry("dabs").or_default() += 1;
            }
            OpSpec::GradientFill {
                layer,
                x,
                y,
                w,
                h,
                shape,
                from,
                to,
                stops,
            } => {
                let id = match layer_of(layer) {
                    Ok(id) => id,
                    Err(message) => return fail(message),
                };
                let shape = match shape.as_str() {
                    "linear" => GradientShape::Linear,
                    "radial" => GradientShape::Radial,
                    other => return fail(format!("unknown gradient shape {other:?}")),
                };
                let point = |p: [f64; 2]| [p[0].round() as i32, p[1].round() as i32];
                ops.push(Op::GradientFill {
                    layer: id,
                    rect: Rect {
                        x: *x,
                        y: *y,
                        width: *w,
                        height: *h,
                    },
                    gradient: GradientSpec {
                        shape,
                        from: point(*from),
                        to: point(*to),
                        stops: stops
                            .iter()
                            .map(|(at, color)| GradientStop {
                                at: *at,
                                color: *color,
                            })
                            .collect(),
                    },
                });
                *counts.entry("gradient_fill").or_default() += 1;
            }
            OpSpec::TexturedDabs {
                layer,
                color,
                opacity,
                flow,
                stamps,
                grain,
            } => {
                let id = match layer_of(layer) {
                    Ok(id) => id,
                    Err(message) => return fail(message),
                };
                let stamps: Vec<Stamp> = stamps
                    .iter()
                    .map(|[x, y, radius, alpha]| Stamp {
                        x_fixed: (x * 64.0).round() as i64,
                        y_fixed: (y * 64.0).round() as i64,
                        radius_px: (radius.round() as u32).clamp(1, 512),
                        alpha: alpha.round().clamp(0.0, 255.0) as u8,
                    })
                    .collect();
                ops.push(Op::TexturedStampRun {
                    layer: id,
                    mode: StampMode::Paint,
                    color: *color,
                    opacity: *opacity,
                    flow: *flow,
                    stamps,
                    grain: GrainSpec::from(grain),
                });
                *counts.entry("textured_dabs").or_default() += 1;
            }
            OpSpec::TexturedStroke {
                layer,
                color,
                opacity,
                flow,
                diameter,
                samples,
                pressure_size,
                pressure_alpha,
                grain,
            } => {
                let id = match layer_of(layer) {
                    Ok(id) => id,
                    Err(message) => return fail(message),
                };
                let stamps = match build_stroke(
                    samples,
                    *color,
                    *opacity,
                    *flow,
                    *diameter,
                    *pressure_size,
                    *pressure_alpha,
                ) {
                    Ok(stamps) => stamps,
                    Err(error) => return fail(format!("textured stroke on {layer}: {error}")),
                };
                ops.push(Op::TexturedStampRun {
                    layer: id,
                    mode: StampMode::Paint,
                    color: *color,
                    opacity: *opacity,
                    flow: *flow,
                    stamps,
                    grain: GrainSpec::from(grain),
                });
                *counts.entry("textured_stroke").or_default() += 1;
            }
        }
    }

    let script = SessionScript {
        width: spec.width,
        height: spec.height,
        base_png: None,
        ops,
    };
    let ron_text = match ron::to_string(&script) {
        Ok(text) => text,
        Err(error) => return fail(format!("serialize script: {error}")),
    };
    if let Err(error) = std::fs::write(ron_path, &ron_text) {
        return fail(format!("write {ron_path}: {error}"));
    }
    println!(
        "script: {} ops ({:?}) -> {} ({} bytes RON)",
        script.ops.len(),
        counts,
        ron_path,
        ron_text.len(),
    );

    // The measured render goes through the saved data:
    // load the RON, replay twice, demand byte-identity.
    let loaded = match SessionScript::load(Path::new(ron_path)) {
        Ok(script) => script,
        Err(error) => return fail(format!("reload script: {error}")),
    };
    let first = match loaded.run() {
        Ok(doc) => doc,
        Err(error) => return fail(format!("replay 1: {error}")),
    };
    let second = match loaded.run() {
        Ok(doc) => doc,
        Err(error) => return fail(format!("replay 2: {error}")),
    };
    let hash1 = first.composite_hash();
    let hash2 = second.composite_hash();
    let hex = |bytes: &[u8; 32]| {
        let mut text = String::with_capacity(64);
        for byte in bytes {
            text.push_str(&format!("{byte:02x}"));
        }
        text
    };
    println!("replay 1 composite_hash={}", hex(&hash1));
    println!("replay 2 composite_hash={}", hex(&hash2));
    if hash1 != hash2 {
        return fail("twin replay diverged: the op script is not deterministic".to_string());
    }
    let png = match first.export_png() {
        Ok(png) => png,
        Err(error) => return fail(format!("export png: {error}")),
    };
    if let Err(error) = std::fs::write(png_path, &png) {
        return fail(format!("write {png_path}: {error}"));
    }
    println!(
        "frame -> {png_path} ({} bytes), twin replay byte-identical",
        png.len()
    );
    ExitCode::SUCCESS
}
