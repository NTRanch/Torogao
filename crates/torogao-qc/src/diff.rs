//! The snapshot diff (contract section 3): two directories
//! of PNG frames, paired by relative path, compared per
//! frame. A frame is *over* when the fraction of pixels whose
//! largest channel delta exceeds `threshold` is greater than
//! [`FRAME_OVER_FRACTION`], when its dimensions differ from
//! the reference's, or when it exists on only one side.
//!
//! Decoding uses the `image` crate's PNG support — the same
//! dependency torogao-gen and torogao-rig already carry — so
//! no new dependency enters the workspace for this verb.

use crate::error::QcError;
use crate::limits::*;
use crate::report::{Finding, Report, Severity, round6};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One compared frame's measurements.
struct FrameDiff {
    rel: String,
    over: bool,
    over_fraction: f64,
    mean_abs_delta: Option<f64>,
    detail: Option<String>,
}

/// Compares `reference` against `candidate`, frame by frame.
///
/// # Errors
///
/// Typed `QC-E-*` when either directory cannot be walked or
/// the frame bound is exceeded. An undecodable frame is a
/// finding on that frame, not an error — the rest of the
/// comparison still reports.
pub fn diff_dirs(reference: &Path, candidate: &Path, threshold: u8) -> Result<Report, QcError> {
    let ref_frames = collect_frames(reference)?;
    let cand_frames = collect_frames(candidate)?;
    let all: BTreeSet<&String> = ref_frames.iter().chain(cand_frames.iter()).collect();
    if all.len() > FRAMES_MAX {
        return Err(QcError::new(
            "QC-E-TOO-MANY-FRAMES",
            format!(
                "{} frames across both dirs, over the {FRAMES_MAX} bound",
                all.len()
            ),
        ));
    }
    let scene = reference
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut report = Report::new("diff", scene);
    let mut frames: Vec<FrameDiff> = Vec::new();
    for rel in all {
        let frame = compare_one(reference, candidate, rel, threshold);
        if frame.over {
            report.push(Finding {
                code: "QC_SNAPSHOT_DIFF",
                severity: Severity::Fail,
                element: None,
                frame: None,
                detail: frame.detail.clone().unwrap_or_else(|| {
                    format!(
                        "frame {rel}: {} of pixels over threshold {threshold}",
                        format_pct(frame.over_fraction)
                    )
                }),
                spec_ref: rel.clone(),
            });
        }
        frames.push(frame);
    }
    let frames_over = frames.iter().filter(|f| f.over).count();
    let mut worst: Vec<&FrameDiff> = frames.iter().collect();
    worst.sort_by(|a, b| {
        b.over_fraction
            .partial_cmp(&a.over_fraction)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rel.cmp(&b.rel))
    });
    let worst_json: Vec<serde_json::Value> = worst
        .iter()
        .take(WORST_FRAMES_MAX)
        .map(|f| {
            json!({
                "frame": f.rel,
                "over": f.over,
                "over_fraction": round6(f.over_fraction),
                "mean_abs_delta": f.mean_abs_delta.map(round6),
            })
        })
        .collect();
    report.extra.push(("threshold", json!(threshold)));
    report.extra.push(("frames_compared", json!(frames.len())));
    report.extra.push(("frames_over", json!(frames_over)));
    report.extra.push(("worst_frames", json!(worst_json)));
    report.sort_findings();
    Ok(report)
}

fn format_pct(fraction: f64) -> String {
    format!("{:.2}%", fraction * 100.0)
}

/// Lists the `.png` files under a directory as `/`-joined
/// relative paths, bounded in count and depth.
fn collect_frames(dir: &Path) -> Result<BTreeSet<String>, QcError> {
    if !dir.is_dir() {
        return Err(QcError::new(
            "QC-E-DIFF-DIR",
            format!("{} is not a directory", dir.display()),
        ));
    }
    let mut out = BTreeSet::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(dir.to_path_buf(), 0)];
    while let Some((current, depth)) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|err| {
            QcError::new(
                "QC-E-DIFF-DIR",
                format!("cannot list {}: {err}", current.display()),
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|err| {
                QcError::new("QC-E-DIFF-DIR", format!("cannot list entry: {err}"))
            })?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|err| {
                QcError::new("QC-E-DIFF-DIR", format!("cannot stat entry: {err}"))
            })?;
            if kind.is_dir() {
                if depth < DIFF_DEPTH_MAX {
                    stack.push((path, depth + 1));
                }
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            {
                let rel = path
                    .strip_prefix(dir)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                out.insert(rel);
                if out.len() > FRAMES_MAX {
                    return Err(QcError::new(
                        "QC-E-TOO-MANY-FRAMES",
                        format!("over {FRAMES_MAX} frames under {}", dir.display()),
                    ));
                }
            }
        }
    }
    Ok(out)
}

/// Compares one paired frame; absence and decode failure are
/// measured outcomes (over, with a detail), never errors.
fn compare_one(reference: &Path, candidate: &Path, rel: &str, threshold: u8) -> FrameDiff {
    let ref_path = reference.join(rel);
    let cand_path = candidate.join(rel);
    if !ref_path.is_file() {
        return FrameDiff {
            rel: rel.to_string(),
            over: true,
            over_fraction: 1.0,
            mean_abs_delta: None,
            detail: Some(format!("frame {rel} exists only in the candidate")),
        };
    }
    if !cand_path.is_file() {
        return FrameDiff {
            rel: rel.to_string(),
            over: true,
            over_fraction: 1.0,
            mean_abs_delta: None,
            detail: Some(format!("frame {rel} is missing from the candidate")),
        };
    }
    let (Ok(a), Ok(b)) = (decode(&ref_path), decode(&cand_path)) else {
        return FrameDiff {
            rel: rel.to_string(),
            over: true,
            over_fraction: 1.0,
            mean_abs_delta: None,
            detail: Some(format!("frame {rel} could not be decoded on one side")),
        };
    };
    if a.dimensions() != b.dimensions() {
        return FrameDiff {
            rel: rel.to_string(),
            over: true,
            over_fraction: 1.0,
            mean_abs_delta: None,
            detail: Some(format!(
                "frame {rel} dimensions differ: reference {:?}, candidate {:?}",
                a.dimensions(),
                b.dimensions()
            )),
        };
    }
    let pixels = a.as_raw().len() / 4;
    let mut over_pixels: u64 = 0;
    let mut sum_abs: u64 = 0;
    for (pa, pb) in a.as_raw().chunks_exact(4).zip(b.as_raw().chunks_exact(4)) {
        let mut max_delta = 0u32;
        for channel in 0..4 {
            let delta = (i32::from(pa[channel]) - i32::from(pb[channel])).unsigned_abs();
            sum_abs += u64::from(delta);
            max_delta = max_delta.max(delta);
        }
        if max_delta > u32::from(threshold) {
            over_pixels += 1;
        }
    }
    let over_fraction = if pixels == 0 {
        0.0
    } else {
        over_pixels as f64 / pixels as f64
    };
    let mean_abs_delta = if pixels == 0 {
        0.0
    } else {
        sum_abs as f64 / (pixels * 4) as f64
    };
    FrameDiff {
        rel: rel.to_string(),
        over: over_fraction > FRAME_OVER_FRACTION,
        over_fraction,
        mean_abs_delta: Some(mean_abs_delta),
        detail: None,
    }
}

/// Decodes one PNG to RGBA8 under the frame bounds.
fn decode(path: &Path) -> Result<image::RgbaImage, image::ImageError> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(FRAME_DIM_MAX);
    limits.max_image_height = Some(FRAME_DIM_MAX);
    limits.max_alloc = Some(FRAME_ALLOC_MAX);
    let mut reader = image::ImageReader::open(path)?;
    reader.limits(limits);
    reader.set_format(image::ImageFormat::Png);
    Ok(reader.decode()?.into_rgba8())
}
