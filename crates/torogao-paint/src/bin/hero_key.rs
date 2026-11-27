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
use torogao_paint::ops::{Op, Rect};
use torogao_paint::script::SessionScript;
use torogao_paint::stroke::{
    PressureMap, Stamp, StampMode, StrokeParams, StrokeSample, build_stamp_run,
};

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
        samples: Vec<[f64; 2]>,
    },
    Dabs {
        layer: String,
        color: Rgba,
        opacity: u8,
        flow: u8,
        /// `[x, y, radius_px, alpha]` per stamp.
        stamps: Vec<[f64; 4]>,
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
            } => {
                let id = match layer_of(layer) {
                    Ok(id) => id,
                    Err(message) => return fail(message),
                };
                let samples: Vec<StrokeSample> = samples
                    .iter()
                    .map(|[x, y]| StrokeSample {
                        x_px: *x,
                        y_px: *y,
                        pressure: 1.0,
                    })
                    .collect();
                let params = StrokeParams {
                    color: *color,
                    diameter_px: *diameter,
                    opacity: *opacity,
                    flow: *flow,
                    spacing_fraction: 0.3,
                    stabilizer_alpha: 1.0,
                    pressure_map: PressureMap {
                        size: false,
                        alpha: false,
                    },
                };
                let stamps = match build_stamp_run(&samples, &params) {
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
