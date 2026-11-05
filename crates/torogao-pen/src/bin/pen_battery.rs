// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! Gate 4 (SPEC SS8 Phase 1): the spike's S-1 synthetic
//! battery re-run against the PRODUCTION shim — create
//! the uinput pen, find it with the production scan, read
//! it with the production reader, replay its placements
//! through the production document core twice, and write
//! `analysis.json` with the measured numbers.
//!
//! Usage: pen-battery <outdir>; exit 0 PASS, 3 FAIL.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use torogao_paint::stroke::StrokeSample;
use torogao_paint::{Document, Op, StampMode, StrokeParams, build_stamp_run};
use torogao_pen::synthetic::{BATTERY_PEN_NAME, SyntheticPen};
use torogao_pen::{PenReader, PenSample, scan_pen_devices};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: pen-battery <outdir>");
        return ExitCode::from(2);
    }
    match run(Path::new(&args[1])) {
        Ok((report, passed)) => {
            println!("{report}");
            if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(3)
            }
        }
        Err(message) => {
            eprintln!("pen-battery: {message}");
            ExitCode::from(3)
        }
    }
}

/// Create the pen, then poll the production scan until
/// the battery pen appears (the kernel needs a moment).
fn create_and_find() -> Result<(SyntheticPen, PathBuf), String> {
    let pen = SyntheticPen::create_battery_pen().map_err(|err| format!("create pen: {err}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for info in scan_pen_devices() {
            if info.name == BATTERY_PEN_NAME {
                return Ok((pen, info.node));
            }
        }
        if Instant::now() > deadline {
            return Err("battery pen never appeared in the production scan".to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Drain the reader until 750 ms of silence follows the
/// feed, or the overall deadline passes.
fn collect(receiver: &std::sync::mpsc::Receiver<PenSample>) -> Vec<PenSample> {
    let mut samples: Vec<PenSample> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    while let Ok(sample) = receiver.recv_timeout(Duration::from_millis(750)) {
        samples.push(sample);
        if Instant::now() > deadline {
            break;
        }
    }
    samples
}

/// Replay the touch placements through torogao-paint
/// twice in fresh documents; the composite hashes must
/// agree (the spike's replay leg, on production code).
fn replay_hash(samples: &[PenSample]) -> Result<([u8; 32], [u8; 32]), String> {
    let held: Vec<StrokeSample> = samples
        .iter()
        .filter(|sample| sample.touch)
        .map(|sample| StrokeSample {
            x_px: f64::from(sample.x_units),
            y_px: f64::from(sample.y_units),
            pressure: f64::from(sample.pressure) / 65_535.0,
        })
        .collect();
    let params = StrokeParams {
        diameter_px: 64,
        opacity: 255,
        flow: 255,
        spacing_fraction: 0.25,
        stabilizer_alpha: 1.0,
        pressure_map: torogao_paint::stroke::PressureMap {
            size: true,
            alpha: true,
        },
        ..StrokeParams::default()
    };
    let stamps = build_stamp_run(&held, &params).map_err(|err| err.to_string())?;
    let mut hashes = [[0u8; 32]; 2];
    for slot in &mut hashes {
        let mut doc = Document::new_empty(1024, 1024).map_err(|err| err.to_string())?;
        let layer = doc
            .add_layer("pen".to_string())
            .map_err(|err| err.to_string())?;
        doc.apply_op(Op::StampRun {
            layer,
            mode: StampMode::Paint,
            color: [24, 22, 40, 255],
            opacity: 255,
            flow: 255,
            stamps: stamps.clone(),
        })
        .map_err(|err| err.to_string())?;
        *slot = doc.composite_hash();
    }
    Ok((hashes[0], hashes[1]))
}

fn run(outdir: &Path) -> Result<(String, bool), String> {
    let (mut pen, node) = create_and_find()?;
    let (reader, receiver) =
        PenReader::spawn(&node).map_err(|err| format!("spawn reader: {err}"))?;
    pen.feed_script().map_err(|err| format!("feed: {err}"))?;
    let samples = collect(&receiver);
    let dropped = reader.dropped_samples();
    reader.stop();
    drop(pen);

    let touch: Vec<&PenSample> = samples.iter().filter(|s| s.touch).collect();
    let mut segments = 0usize;
    let mut was_touch = false;
    for sample in &samples {
        if sample.touch && !was_touch {
            segments += 1;
        }
        was_touch = sample.touch;
    }
    // The ramp is the first stroke: 300 touch samples.
    let ramp: Vec<&&PenSample> = touch.iter().take(300).collect();
    let mut distinct: Vec<u16> = ramp.iter().map(|s| s.pressure).collect();
    distinct.sort_unstable();
    distinct.dedup();
    let rising_violations = ramp
        .windows(2)
        .filter(|pair| pair[1].pressure <= pair[0].pressure)
        .count();
    let max_dev = ramp
        .iter()
        .enumerate()
        .map(|(k, s)| (s.x_units - (100 + 3 * k as i32)).abs())
        .max()
        .unwrap_or(-1);
    let first_pressure = touch.first().map(|s| s.pressure).unwrap_or(0);
    let (hash_a, hash_b) = replay_hash(&samples)?;
    let replay_identical = hash_a == hash_b;

    let checks = [
        ("touch_samples_is_302", touch.len() == 302),
        ("stroke_segments_is_2", segments == 2),
        ("ramp_distinct_levels_is_300", distinct.len() == 300),
        ("ramp_rising_violations_is_0", rising_violations == 0),
        ("ramp_pos_max_dev_within_2px", (0..=2).contains(&max_dev)),
        ("first_sample_pressure_is_2048", first_pressure == 2048),
        ("dropped_samples_is_0", dropped == 0),
        ("replay_byte_identical", replay_identical),
    ];
    let passed = checks.iter().all(|(_, ok)| *ok);
    let checks_json: Vec<String> = checks
        .iter()
        .map(|(name, ok)| format!("\"{name}\": {}", if *ok { "true" } else { "false" }))
        .collect();
    let report = format!(
        "{{\n  \"battery\": \"torogao-pen S-1 (production shim)\",\n  \"touch_samples\": {},\n  \"stroke_segments\": {},\n  \"ramp_distinct_pressure_levels\": {},\n  \"ramp_rising_violations\": {},\n  \"ramp_pos_max_dev_px\": {},\n  \"first_sample_pressure\": {},\n  \"dropped_samples\": {},\n  \"replay_byte_identical\": {},\n  \"checks\": {{ {} }},\n  \"verdict\": \"{}\"\n}}\n",
        touch.len(),
        segments,
        distinct.len(),
        rising_violations,
        max_dev,
        first_pressure,
        dropped,
        replay_identical,
        checks_json.join(", "),
        if passed { "PASS" } else { "FAIL" },
    );
    std::fs::create_dir_all(outdir).map_err(|err| format!("mkdir outdir: {err}"))?;
    std::fs::write(outdir.join("analysis.json"), &report)
        .map_err(|err| format!("write analysis: {err}"))?;
    Ok((report, passed))
}
