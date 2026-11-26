//! Patch-set fixtures for the snapshot diff (C2 study
//! P1–P3): region records, the AA pixel class, and the
//! heatmap/composite artefacts. Half validation (known
//! inputs, exact records), half adversarial (cap pressure,
//! flag-dependent verdicts, bound violations, determinism).

use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use torogao_qc::diff::{DiffOptions, diff_dirs_with_options};

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "torogao-qc-patches-{}-{}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

fn solid(path: &Path, w: u32, h: u32, px: [u8; 4]) {
    let img = RgbaImage::from_pixel(w, h, Rgba(px));
    img.save(path).expect("save png");
}

fn pair_dirs(dir: &Path) -> (PathBuf, PathBuf) {
    let (reference, candidate) = (dir.join("ref"), dir.join("cand"));
    std::fs::create_dir_all(&reference).expect("ref dir");
    std::fs::create_dir_all(&candidate).expect("cand dir");
    (reference, candidate)
}

/// 64×64 black frame whose candidate carries two separated
/// blocks: 8×8 of delta 255 at (4,4) and 4×4 of delta 100
/// at (40,40).
fn two_block_pair(dir: &Path) -> (PathBuf, PathBuf) {
    let (reference, candidate) = pair_dirs(dir);
    solid(&reference.join("f000.png"), 64, 64, [0, 0, 0, 255]);
    let mut img = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 255]));
    for y in 4..12 {
        for x in 4..12 {
            img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    for y in 40..44 {
        for x in 40..44 {
            img.put_pixel(x, y, Rgba([100, 0, 0, 255]));
        }
    }
    img.save(candidate.join("f000.png")).expect("save png");
    (reference, candidate)
}

fn options() -> DiffOptions {
    DiffOptions {
        threshold: 8,
        region_gap: 0,
    }
}

// ---- P1 validation -------------------------------------------

#[test]
fn two_blocks_produce_exact_region_records() {
    let dir = tempdir("p1-blocks");
    let (reference, candidate) = two_block_pair(&dir);
    let report = diff_dirs_with_options(&reference, &candidate, &options()).expect("diff");
    let json = report.to_json();
    let expected = serde_json::json!([
        {"x": 4, "y": 4, "w": 8, "h": 8, "pixel_count": 64, "mean_delta": 255.0, "max_delta": 255},
        {"x": 40, "y": 40, "w": 4, "h": 4, "pixel_count": 16, "mean_delta": 100.0, "max_delta": 100},
    ]);
    assert_eq!(json["worst_frames"][0]["regions"], expected);
    assert_eq!(json["worst_frames"][0]["regions_dropped"], serde_json::json!(0));
    assert_eq!(json["region_gap"], serde_json::json!(0));
    // The finding carries the same regions as structured data.
    assert_eq!(json["findings"][0]["data"]["regions"], expected);
    assert_eq!(
        json["findings"][0]["data"]["regions_dropped"],
        serde_json::json!(0)
    );
}

#[test]
fn region_gap_merges_and_is_recorded() {
    let dir = tempdir("p1-gap");
    let (reference, candidate) = two_block_pair(&dir);
    let opts = DiffOptions {
        threshold: 8,
        region_gap: 16,
    };
    let report = diff_dirs_with_options(&reference, &candidate, &opts).expect("diff");
    let json = report.to_json();
    assert_eq!(json["region_gap"], serde_json::json!(16));
    let regions = &json["worst_frames"][0]["regions"];
    assert_eq!(regions.as_array().map(|a| a.len()), Some(1));
    assert_eq!(
        regions[0],
        serde_json::json!({"x": 4, "y": 4, "w": 40, "h": 40, "pixel_count": 80, "mean_delta": 224.0, "max_delta": 255})
    );
}

// ---- P1 adversarial ------------------------------------------

#[test]
fn regions_past_cap_are_counted_not_dropped_silently() {
    let dir = tempdir("p1-cap");
    let (reference, candidate) = pair_dirs(&dir);
    solid(&reference.join("f000.png"), 64, 64, [0, 0, 0, 255]);
    let mut img = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 255]));
    // 100 single pixels on a 6-pixel lattice: 100 separate
    // 8-connected components, well past the 64-region cap.
    for i in 0..100u32 {
        let x = 2 + 6 * (i % 10);
        let y = 2 + 6 * (i / 10);
        img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
    }
    img.save(candidate.join("f000.png")).expect("save png");
    let report = diff_dirs_with_options(&reference, &candidate, &options()).expect("diff");
    let json = report.to_json();
    let regions = &json["worst_frames"][0]["regions"];
    assert_eq!(regions.as_array().map(|a| a.len()), Some(64));
    assert_eq!(
        json["worst_frames"][0]["regions_dropped"],
        serde_json::json!(36)
    );
    assert_eq!(
        json["findings"][0]["data"]["regions_dropped"],
        serde_json::json!(36)
    );
}

#[test]
fn region_gap_past_bound_is_a_typed_error() {
    let dir = tempdir("p1-bound");
    let (reference, candidate) = two_block_pair(&dir);
    let opts = DiffOptions {
        threshold: 8,
        region_gap: 17,
    };
    let err = diff_dirs_with_options(&reference, &candidate, &opts).expect_err("must refuse");
    assert_eq!(err.code(), "QC-E-REGION-GAP");
}
