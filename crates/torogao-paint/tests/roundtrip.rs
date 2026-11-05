// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! Gate 1 (SPEC SS8 Phase 1): the scripted session below
//! exercises brush, eraser, layer add/reorder/opacity and
//! lock-alpha; it is run by the harness in two fresh
//! processes and the printed hashes must agree exactly.

use std::process::Command;

use torogao_paint::layer::LayerId;
use torogao_paint::ops::{LayerPropsChange, Op, Rect};
use torogao_paint::script::SessionScript;
use torogao_paint::stroke::StampMode;
use torogao_paint::{StrokeParams, StrokeSample, build_stamp_run};

fn stroke(samples: &[(f64, f64, f64)], diameter: u32) -> Vec<torogao_paint::Stamp> {
    let held: Vec<StrokeSample> = samples
        .iter()
        .map(|(x, y, p)| StrokeSample {
            x_px: *x,
            y_px: *y,
            pressure: *p,
        })
        .collect();
    let params = StrokeParams {
        diameter_px: diameter,
        ..StrokeParams::default()
    };
    build_stamp_run(&held, &params).expect("stamp run")
}

fn gate_script() -> SessionScript {
    let paper = LayerId(1);
    let ink = LayerId(2);
    let wash = LayerId(3);
    let ops = vec![
        Op::LayerAdd {
            id: paper,
            name: "paper".to_string(),
            after: None,
        },
        Op::FillRect {
            layer: paper,
            rect: Rect {
                x: 0,
                y: 0,
                width: 300,
                height: 200,
            },
            color: [238, 234, 224, 255],
        },
        Op::LayerAdd {
            id: ink,
            name: "ink".to_string(),
            after: Some(paper),
        },
        Op::StampRun {
            layer: ink,
            mode: StampMode::Paint,
            color: [30, 28, 60, 255],
            opacity: 255,
            flow: 200,
            stamps: stroke(
                &[(20.0, 100.0, 0.4), (150.0, 60.0, 0.9), (280.0, 120.0, 0.5)],
                36,
            ),
        },
        Op::LayerProps {
            id: ink,
            change: LayerPropsChange {
                opacity: Some(180),
                ..LayerPropsChange::default()
            },
        },
        Op::LayerAdd {
            id: wash,
            name: "wash".to_string(),
            after: Some(ink),
        },
        Op::StampRun {
            layer: wash,
            mode: StampMode::Paint,
            color: [200, 120, 60, 255],
            opacity: 160,
            flow: 255,
            stamps: stroke(&[(60.0, 40.0, 1.0), (240.0, 160.0, 1.0)], 90),
        },
        Op::LayerReorder {
            id: wash,
            to_index: 1,
        },
        Op::LayerProps {
            id: paper,
            change: LayerPropsChange {
                lock_alpha: Some(true),
                ..LayerPropsChange::default()
            },
        },
        Op::StampRun {
            layer: paper,
            mode: StampMode::Paint,
            color: [90, 140, 200, 255],
            opacity: 255,
            flow: 255,
            stamps: stroke(&[(0.0, 0.0, 1.0), (299.0, 199.0, 1.0)], 48),
        },
        Op::StampRun {
            layer: ink,
            mode: StampMode::Erase,
            color: [0, 0, 0, 0],
            opacity: 255,
            flow: 255,
            stamps: stroke(&[(150.0, 60.0, 1.0)], 30),
        },
    ];
    SessionScript {
        width: 300,
        height: 200,
        base_png: None,
        ops,
    }
}

fn run_harness(root: &std::path::Path, tag: &str) -> (String, Vec<u8>) {
    let dir = root.join(tag);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let script_path = dir.join("script.ron");
    let text = ron::ser::to_string_pretty(&gate_script(), ron::ser::PrettyConfig::new())
        .expect("script ron");
    std::fs::write(&script_path, text).expect("write script");
    let out_png = dir.join("out.png");
    let output = Command::new(env!("CARGO_BIN_EXE_paint-roundtrip"))
        .arg(&script_path)
        .arg(dir.join("doc.tpaint"))
        .arg(&out_png)
        .output()
        .expect("run harness");
    assert!(
        output.status.success(),
        "harness failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let png = std::fs::read(&out_png).expect("read png");
    (String::from_utf8(output.stdout).expect("stdout text"), png)
}

#[test]
fn round_trip_is_identical_across_two_fresh_processes() {
    let root = std::env::temp_dir().join(format!("torogao-paint-roundtrip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (report_a, png_a) = run_harness(&root, "run-a");
    let (report_b, png_b) = run_harness(&root, "run-b");
    assert_eq!(report_a, report_b, "hash reports diverged across processes");
    assert_eq!(png_a, png_b, "exported PNGs diverged across processes");
    assert!(report_a.contains("ops_applied=11"), "report: {report_a}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn in_process_replay_matches_too() {
    let doc = gate_script().run().expect("script");
    let again = gate_script().run().expect("script again");
    assert_eq!(doc.composite_hash(), again.composite_hash());
    assert_eq!(doc.ops().len(), 11);
}

/// Serialize the gate script to `$PAINT_GATE_SCRIPT_OUT`
/// (no-op when unset) so the standalone two-process
/// harness runs in the gate script use byte-identical
/// input to this integration gate.
#[test]
fn write_gate_script_ron() {
    if let Ok(path) = std::env::var("PAINT_GATE_SCRIPT_OUT") {
        let text = ron::ser::to_string_pretty(&gate_script(), ron::ser::PrettyConfig::new())
            .expect("script ron");
        std::fs::write(path, text).expect("write gate script");
    }
}
