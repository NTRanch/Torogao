//! Snapshot-diff fixtures: identical frames pass, planted
//! differences produce exactly `QC_SNAPSHOT_DIFF`, and the
//! threshold is honoured in both directions.

use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use torogao_qc::diff::diff_dirs;
use torogao_qc::report::Report;

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("torogao-qc-diff-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

fn solid(path: &Path, w: u32, h: u32, px: [u8; 4]) {
    let img = RgbaImage::from_pixel(w, h, Rgba(px));
    img.save(path).expect("save png");
}

fn codes(report: &Report) -> Vec<&str> {
    report.findings().iter().map(|f| f.code).collect()
}

#[test]
fn identical_frames_pass() {
    let dir = tempdir("identical");
    let (reference, candidate) = (dir.join("ref"), dir.join("cand"));
    std::fs::create_dir_all(&reference).expect("ref dir");
    std::fs::create_dir_all(&candidate).expect("cand dir");
    solid(&reference.join("f000.png"), 64, 64, [10, 20, 30, 255]);
    solid(&candidate.join("f000.png"), 64, 64, [10, 20, 30, 255]);
    let report = diff_dirs(&reference, &candidate, 8).expect("diff");
    assert_eq!(codes(&report), Vec::<&str>::new());
    let json = report.to_json();
    assert_eq!(json["frames_compared"], serde_json::json!(1));
    assert_eq!(json["frames_over"], serde_json::json!(0));
}

#[test]
fn changed_block_is_over_with_snapshot_diff() {
    let dir = tempdir("block");
    let (reference, candidate) = (dir.join("ref"), dir.join("cand"));
    std::fs::create_dir_all(&reference).expect("ref dir");
    std::fs::create_dir_all(&candidate).expect("cand dir");
    solid(&reference.join("f000.png"), 64, 64, [0, 0, 0, 255]);
    let mut img = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 255]));
    for y in 0..32 {
        for x in 0..32 {
            img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    img.save(candidate.join("f000.png")).expect("save png");
    let report = diff_dirs(&reference, &candidate, 8).expect("diff");
    assert_eq!(codes(&report), ["QC_SNAPSHOT_DIFF"]);
    let json = report.to_json();
    assert_eq!(json["frames_over"], serde_json::json!(1));
    assert_eq!(
        json["worst_frames"][0]["frame"],
        serde_json::json!("f000.png")
    );
    assert_eq!(
        json["worst_frames"][0]["over_fraction"],
        serde_json::json!(0.25)
    );
}

#[test]
fn threshold_is_honoured_in_both_directions() {
    let dir = tempdir("threshold");
    let (reference, candidate) = (dir.join("ref"), dir.join("cand"));
    std::fs::create_dir_all(&reference).expect("ref dir");
    std::fs::create_dir_all(&candidate).expect("cand dir");
    solid(&reference.join("f000.png"), 16, 16, [100, 100, 100, 255]);
    solid(&candidate.join("f000.png"), 16, 16, [104, 100, 100, 255]);
    let report = diff_dirs(&reference, &candidate, 8).expect("diff");
    assert_eq!(codes(&report), Vec::<&str>::new());
    let report = diff_dirs(&reference, &candidate, 2).expect("diff");
    assert_eq!(codes(&report), ["QC_SNAPSHOT_DIFF"]);
}

#[test]
fn missing_counterpart_is_over() {
    let dir = tempdir("missing");
    let (reference, candidate) = (dir.join("ref"), dir.join("cand"));
    std::fs::create_dir_all(&reference).expect("ref dir");
    std::fs::create_dir_all(&candidate).expect("cand dir");
    solid(&reference.join("f000.png"), 16, 16, [0, 0, 0, 255]);
    let report = diff_dirs(&reference, &candidate, 8).expect("diff");
    assert_eq!(codes(&report), ["QC_SNAPSHOT_DIFF"]);
    assert!(
        report.findings()[0].detail.contains("missing"),
        "detail should name the absence: {}",
        report.findings()[0].detail
    );
}

#[test]
fn dimension_mismatch_is_over() {
    let dir = tempdir("dims");
    let (reference, candidate) = (dir.join("ref"), dir.join("cand"));
    std::fs::create_dir_all(&reference).expect("ref dir");
    std::fs::create_dir_all(&candidate).expect("cand dir");
    solid(&reference.join("f000.png"), 64, 64, [0, 0, 0, 255]);
    solid(&candidate.join("f000.png"), 32, 32, [0, 0, 0, 255]);
    let report = diff_dirs(&reference, &candidate, 8).expect("diff");
    assert_eq!(codes(&report), ["QC_SNAPSHOT_DIFF"]);
    assert!(report.findings()[0].detail.contains("dimensions differ"));
}

#[test]
fn nested_frames_pair_by_relative_path() {
    let dir = tempdir("nested");
    let (reference, candidate) = (dir.join("ref"), dir.join("cand"));
    std::fs::create_dir_all(reference.join("seq")).expect("ref seq");
    std::fs::create_dir_all(candidate.join("seq")).expect("cand seq");
    solid(&reference.join("seq/f000.png"), 16, 16, [1, 2, 3, 255]);
    solid(&candidate.join("seq/f000.png"), 16, 16, [1, 2, 3, 255]);
    let report = diff_dirs(&reference, &candidate, 8).expect("diff");
    assert_eq!(codes(&report), Vec::<&str>::new());
    assert_eq!(report.to_json()["frames_compared"], serde_json::json!(1));
}
