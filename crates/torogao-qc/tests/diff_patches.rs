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
        exclude_aa: false,
        artifacts: None,
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
        exclude_aa: false,
        artifacts: None,
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
        exclude_aa: false,
        artifacts: None,
    };
    let err = diff_dirs_with_options(&reference, &candidate, &opts).expect_err("must refuse");
    assert_eq!(err.code(), "QC-E-REGION-GAP");
}

// ---- P2 validation -------------------------------------------

/// 64x64 frame split black|white at column 32; the candidate
/// inserts a one-column grey blend at the boundary -- the
/// classic anti-aliased edge. The blend column's pixels are
/// over threshold (delta 127) but AA-class.
fn aa_pair(dir: &Path) -> (PathBuf, PathBuf) {
    let (reference, candidate) = pair_dirs(dir);
    let mut ref_img = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 255]));
    for y in 0..64 {
        for x in 32..64 {
            ref_img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    ref_img.save(reference.join("f000.png")).expect("save png");
    let mut cand_img = ref_img.clone();
    for y in 0..64 {
        cand_img.put_pixel(32, y, Rgba([128, 128, 128, 255]));
    }
    cand_img.save(candidate.join("f000.png")).expect("save png");
    (reference, candidate)
}

#[test]
fn aa_edge_pixels_are_counted_and_still_over_by_default() {
    let dir = tempdir("p2-count");
    let (reference, candidate) = aa_pair(&dir);
    let report = diff_dirs_with_options(&reference, &candidate, &options()).expect("diff");
    let json = report.to_json();
    // The full blend column is over threshold; its interior
    // rows are AA-class (the two frame-edge rows lack the
    // three-sibling neighbourhood and are not).
    assert_eq!(json["worst_frames"][0]["aa_pixels"], serde_json::json!(62));
    assert_eq!(json["frames_over"], serde_json::json!(1));
    assert_eq!(json["findings"].as_array().map(|a| a.len()), Some(1));
    // The exclusion is recorded only when used.
    assert!(json.get("exclude_aa").is_none());
}

#[test]
fn default_verdict_math_matches_the_pre_patch_verb() {
    let dir = tempdir("p2-regression");
    let (reference, candidate) = two_block_pair(&dir);
    let plain = torogao_qc::diff::diff_dirs(&reference, &candidate, 8).expect("plain diff");
    let with_opts = diff_dirs_with_options(&reference, &candidate, &options()).expect("opts diff");
    let (a, b) = (plain.to_json(), with_opts.to_json());
    assert_eq!(a["summary"], b["summary"]);
    assert_eq!(a["frames_over"], b["frames_over"]);
    assert_eq!(
        a["worst_frames"][0]["over_fraction"],
        b["worst_frames"][0]["over_fraction"]
    );
    // A solid block edge is real change, not AA shimmer.
    assert_eq!(b["worst_frames"][0]["aa_pixels"], serde_json::json!(0));
}

// ---- P2 adversarial ------------------------------------------

#[test]
fn exclude_aa_changes_the_verdict_only_via_the_flag() {
    let dir = tempdir("p2-flag");
    let (reference, candidate) = aa_pair(&dir);
    let opts = DiffOptions {
        threshold: 8,
        region_gap: 0,
        exclude_aa: true,
        artifacts: None,
    };
    let report = diff_dirs_with_options(&reference, &candidate, &opts).expect("diff");
    let json = report.to_json();
    assert_eq!(json["exclude_aa"], serde_json::json!(true));
    // Only the two frame-edge blend pixels remain over:
    // 2/4096 = 0.000488, under the 0.1% frame fraction.
    assert_eq!(
        json["worst_frames"][0]["over_fraction"],
        serde_json::json!(0.000488)
    );
    assert_eq!(json["worst_frames"][0]["aa_pixels"], serde_json::json!(62));
    assert_eq!(json["frames_over"], serde_json::json!(0));
    assert_eq!(json["findings"].as_array().map(|a| a.len()), Some(0));
}

// ---- P3 adversarial: determinism across runs -----------------

#[test]
fn artifacts_have_deterministic_names_and_the_report_is_byte_identical_across_runs() {
    let dir = tempdir("p3-artifacts");
    let (reference, candidate) = two_block_pair(&dir);
    let run = |artifacts: &Path| {
        let opts = DiffOptions {
            threshold: 8,
            region_gap: 0,
            exclude_aa: false,
            artifacts: Some(artifacts.to_path_buf()),
        };
        diff_dirs_with_options(&reference, &candidate, &opts).expect("diff")
    };
    let (art1, art2) = (dir.join("art1"), dir.join("art2"));
    let report1 = run(&art1);
    let report2 = run(&art2);
    // The JSON report is byte-identical across runs: artefact
    // references are bare deterministic filenames, never
    // paths, timestamps, or wall-clock values.
    assert_eq!(report1.to_json_string(), report2.to_json_string());
    let json = report1.to_json();
    assert_eq!(
        json["worst_frames"][0]["heatmap"],
        serde_json::json!("heatmap-f000.png")
    );
    assert_eq!(
        json["worst_frames"][0]["composite"],
        serde_json::json!("composite-f000.png")
    );
    for art in [&art1, &art2] {
        assert!(art.join("heatmap-f000.png").is_file());
        assert!(art.join("composite-f000.png").is_file());
    }
    // Heatmap evidence: the changed block is opaque, an
    // unchanged corner is transparent.
    let heat = image::open(art1.join("heatmap-f000.png"))
        .expect("open heatmap")
        .into_rgba8();
    assert_eq!(heat.get_pixel(8, 8).0[3], 255);
    assert_eq!(heat.get_pixel(60, 60).0[3], 0);
}
