//! The snapshot diff (contract section 3): two directories
//! of PNG frames, paired by relative path, compared per
//! frame. A frame is *over* when the fraction of pixels whose
//! largest channel delta exceeds `threshold` is greater than
//! [`FRAME_OVER_FRACTION`], when its dimensions differ from
//! the reference's, or when it exists on only one side.
//!
//! Patch P1 (C2 study, 2026-11-26) extends the verb without
//! moving any existing verdict semantics: each measured
//! frame's over-threshold mask is labelled into connected
//! regions (8-connectivity), and each region's bbox, pixel
//! count, and mean/max delta are recorded on the frame's
//! `worst_frames` entry and on its `QC_SNAPSHOT_DIFF`
//! finding. Region merging takes an explicit, recorded
//! `region_gap` parameter — never an auto-derived one.
//! Regions past [`REGIONS_PER_FRAME_MAX`] are counted into
//! `regions_dropped`, never silently dropped.
//!
//! Decoding uses the `image` crate's PNG support — the same
//! dependency torogao-gen and torogao-rig already carry — so
//! no new dependency enters the workspace for this verb.

use crate::error::QcError;
use crate::limits::*;
use crate::report::{Finding, Report, Severity, round6};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Caller-declared parameters for the diff verb. Every field
/// is an input, recorded in the report — nothing is derived
/// from the images being compared.
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Per-pixel channel delta threshold (0–255).
    pub threshold: u8,
    /// Region merging gap, in pixels: components whose
    /// bounding boxes, expanded by this many pixels, overlap
    /// are merged into one region. `0` means pure
    /// 8-connectivity. Bounded by [`REGION_GAP_MAX`].
    pub region_gap: u32,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions {
            threshold: PIXEL_THRESHOLD_DEFAULT,
            region_gap: 0,
        }
    }
}

/// One connected region of over-threshold change in a frame
/// (P1). Deltas are in the verb's own max-channel units.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// Bounding box left edge, in pixels.
    pub x: u32,
    /// Bounding box top edge, in pixels.
    pub y: u32,
    /// Bounding box width, in pixels.
    pub w: u32,
    /// Bounding box height, in pixels.
    pub h: u32,
    /// Number of over-threshold pixels in the region.
    pub pixel_count: u64,
    /// Mean max-channel delta over the region's pixels.
    pub mean_delta: f64,
    /// Largest max-channel delta in the region.
    pub max_delta: u32,
}

impl Region {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "x": self.x,
            "y": self.y,
            "w": self.w,
            "h": self.h,
            "pixel_count": self.pixel_count,
            "mean_delta": round6(self.mean_delta),
            "max_delta": self.max_delta,
        })
    }
}

/// One compared frame's measurements.
struct FrameDiff {
    rel: String,
    over: bool,
    over_fraction: f64,
    mean_abs_delta: Option<f64>,
    detail: Option<String>,
    regions: Vec<Region>,
    regions_dropped: u64,
}

impl FrameDiff {
    /// A frame whose outcome is absence, decode failure, or
    /// a dimension mismatch: over by definition, unmeasured.
    fn unmeasured(rel: &str, detail: String) -> FrameDiff {
        FrameDiff {
            rel: rel.to_string(),
            over: true,
            over_fraction: 1.0,
            mean_abs_delta: None,
            detail: Some(detail),
            regions: Vec::new(),
            regions_dropped: 0,
        }
    }

    fn regions_json(&self) -> serde_json::Value {
        Value::Array(self.regions.iter().map(Region::to_json).collect())
    }
}

use serde_json::Value;

/// Compares `reference` against `candidate` at `threshold`
/// with default parameters (no region merging).
///
/// # Errors
///
/// Typed `QC-E-*` when either directory cannot be walked or
/// the frame bound is exceeded.
pub fn diff_dirs(reference: &Path, candidate: &Path, threshold: u8) -> Result<Report, QcError> {
    diff_dirs_with_options(
        reference,
        candidate,
        &DiffOptions {
            threshold,
            ..DiffOptions::default()
        },
    )
}

/// Compares `reference` against `candidate`, frame by frame,
/// under the declared `options`.
///
/// # Errors
///
/// Typed `QC-E-*` when either directory cannot be walked,
/// the frame bound is exceeded, `region_gap` is past
/// [`REGION_GAP_MAX`], or an artefact cannot be written. An
/// undecodable frame is a finding on that frame, not an
/// error — the rest of the comparison still reports.
pub fn diff_dirs_with_options(
    reference: &Path,
    candidate: &Path,
    options: &DiffOptions,
) -> Result<Report, QcError> {
    if options.region_gap > REGION_GAP_MAX {
        return Err(QcError::new(
            "QC-E-REGION-GAP",
            format!(
                "region_gap {} is past the {REGION_GAP_MAX} bound",
                options.region_gap
            ),
        ));
    }
    let threshold = options.threshold;
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
        let frame = compare_one(reference, candidate, rel, options);
        if frame.over {
            let data = if frame.detail.is_none() {
                Some(json!({
                    "regions": frame.regions_json(),
                    "regions_dropped": frame.regions_dropped,
                }))
            } else {
                None
            };
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
                data,
            });
        }
        frames.push(frame);
    }
    let frames_over = frames.iter().filter(|f| f.over).count();
    // The worst-frames order: over fraction descending, then
    // relative path — a pure function of the measurements.
    let mut order: Vec<usize> = (0..frames.len()).collect();
    order.sort_by(|&a, &b| {
        frames[b]
            .over_fraction
            .partial_cmp(&frames[a].over_fraction)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| frames[a].rel.cmp(&frames[b].rel))
    });
    let worst_idx: Vec<usize> = order.into_iter().take(WORST_FRAMES_MAX).collect();
    let worst_json: Vec<serde_json::Value> = worst_idx
        .iter()
        .map(|&idx| {
            let f = &frames[idx];
            json!({
                "frame": f.rel,
                "over": f.over,
                "over_fraction": round6(f.over_fraction),
                "mean_abs_delta": f.mean_abs_delta.map(round6),
                "regions": f.regions_json(),
                "regions_dropped": f.regions_dropped,
            })
        })
        .collect();
    report.extra.push(("threshold", json!(threshold)));
    report.extra.push(("region_gap", json!(options.region_gap)));
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
fn compare_one(
    reference: &Path,
    candidate: &Path,
    rel: &str,
    options: &DiffOptions,
) -> FrameDiff {
    let threshold = options.threshold;
    let ref_path = reference.join(rel);
    let cand_path = candidate.join(rel);
    if !ref_path.is_file() {
        return FrameDiff::unmeasured(
            rel,
            format!("frame {rel} exists only in the candidate"),
        );
    }
    if !cand_path.is_file() {
        return FrameDiff::unmeasured(
            rel,
            format!("frame {rel} is missing from the candidate"),
        );
    }
    let (Ok(a), Ok(b)) = (decode(&ref_path), decode(&cand_path)) else {
        return FrameDiff::unmeasured(
            rel,
            format!("frame {rel} could not be decoded on one side"),
        );
    };
    if a.dimensions() != b.dimensions() {
        return FrameDiff::unmeasured(
            rel,
            format!(
                "frame {rel} dimensions differ: reference {:?}, candidate {:?}",
                a.dimensions(),
                b.dimensions()
            ),
        );
    }
    let (width, height) = a.dimensions();
    let pixels = (width as usize) * (height as usize);
    let mut deltas: Vec<u32> = Vec::with_capacity(pixels);
    let mut sum_abs: u64 = 0;
    for (pa, pb) in a.as_raw().chunks_exact(4).zip(b.as_raw().chunks_exact(4)) {
        let mut max_delta = 0u32;
        for channel in 0..4 {
            let delta = (i32::from(pa[channel]) - i32::from(pb[channel])).unsigned_abs();
            sum_abs += u64::from(delta);
            max_delta = max_delta.max(delta);
        }
        deltas.push(max_delta);
    }
    let mut mask: Vec<bool> = Vec::with_capacity(pixels);
    let mut over_pixels: u64 = 0;
    for &delta in &deltas {
        let over = delta > u32::from(threshold);
        mask.push(over);
        if over {
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
    let (regions, regions_dropped) =
        compute_regions(&mask, &deltas, width, height, options.region_gap);
    FrameDiff {
        rel: rel.to_string(),
        over: over_fraction > FRAME_OVER_FRACTION,
        over_fraction,
        mean_abs_delta: Some(mean_abs_delta),
        detail: None,
        regions,
        regions_dropped,
    }
}

/// Per-component aggregates before gap merging and capping.
struct Component {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
    pixel_count: u64,
    delta_sum: u64,
    max_delta: u32,
}

/// Labels the over-threshold mask into 8-connected
/// components, merges components whose bounding boxes
/// overlap once expanded by `gap` pixels, and returns the
/// regions in their published order (pixel count
/// descending, then y, then x) plus the count of regions
/// dropped by the [`REGIONS_PER_FRAME_MAX`] cap.
fn compute_regions(
    mask: &[bool],
    deltas: &[u32],
    width: u32,
    height: u32,
    gap: u32,
) -> (Vec<Region>, u64) {
    let w = width as usize;
    let mut labels: Vec<i32> = vec![-1; mask.len()];
    let mut components: Vec<Component> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || labels[start] >= 0 {
            continue;
        }
        let id = components.len() as i32;
        let mut comp = Component {
            min_x: u32::MAX,
            min_y: u32::MAX,
            max_x: 0,
            max_y: 0,
            pixel_count: 0,
            delta_sum: 0,
            max_delta: 0,
        };
        labels[start] = id;
        stack.push(start);
        while let Some(idx) = stack.pop() {
            let x = (idx % w) as u32;
            let y = (idx / w) as u32;
            comp.min_x = comp.min_x.min(x);
            comp.min_y = comp.min_y.min(y);
            comp.max_x = comp.max_x.max(x);
            comp.max_y = comp.max_y.max(y);
            comp.pixel_count += 1;
            comp.delta_sum += u64::from(deltas[idx]);
            comp.max_delta = comp.max_delta.max(deltas[idx]);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = i64::from(x) + i64::from(dx);
                    let ny = i64::from(y) + i64::from(dy);
                    if nx < 0 || ny < 0 || nx >= i64::from(width) || ny >= i64::from(height) {
                        continue;
                    }
                    let nidx = (ny as usize) * w + (nx as usize);
                    if mask[nidx] && labels[nidx] < 0 {
                        labels[nidx] = id;
                        stack.push(nidx);
                    }
                }
            }
        }
        components.push(comp);
    }
    let groups = merge_groups(&components, gap);
    let mut regions: Vec<Region> = groups
        .into_iter()
        .map(|comp| Region {
            x: comp.min_x,
            y: comp.min_y,
            w: comp.max_x - comp.min_x + 1,
            h: comp.max_y - comp.min_y + 1,
            pixel_count: comp.pixel_count,
            mean_delta: comp.delta_sum as f64 / comp.pixel_count as f64,
            max_delta: comp.max_delta,
        })
        .collect();
    regions.sort_by(|a, b| {
        b.pixel_count
            .cmp(&a.pixel_count)
            .then_with(|| a.y.cmp(&b.y))
            .then_with(|| a.x.cmp(&b.x))
    });
    let dropped = (regions.len() - regions.len().min(REGIONS_PER_FRAME_MAX)) as u64;
    regions.truncate(REGIONS_PER_FRAME_MAX);
    (regions, dropped)
}

/// Merges components whose bounding boxes, expanded by
/// `gap` pixels on every side, overlap. With `gap == 0`
/// nothing merges (boxes of distinct 8-connected components
/// may still touch diagonally; the published semantic is
/// bbox-overlap after expansion, and at gap 0 the expansion
/// is empty, so only genuinely overlapping boxes — which
/// distinct components cannot have — would merge).
fn merge_groups(components: &[Component], gap: u32) -> Vec<Component> {
    let n = components.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    if gap > 0 && n > 1 {
        // Bucket component indices by the grid cells their
        // expanded boxes cover, so candidate pairs are few.
        const CELL: i64 = 128;
        let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        let expanded = |c: &Component| {
            (
                i64::from(c.min_x) - i64::from(gap),
                i64::from(c.min_y) - i64::from(gap),
                i64::from(c.max_x) + i64::from(gap),
                i64::from(c.max_y) + i64::from(gap),
            )
        };
        for (idx, comp) in components.iter().enumerate() {
            let (x0, y0, x1, y1) = expanded(comp);
            for cx in (x0.div_euclid(CELL))..=(x1.div_euclid(CELL)) {
                for cy in (y0.div_euclid(CELL))..=(y1.div_euclid(CELL)) {
                    buckets.entry((cx, cy)).or_default().push(idx);
                }
            }
        }
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for cell in buckets.values() {
            for i in 0..cell.len() {
                for j in (i + 1)..cell.len() {
                    pairs.push((cell[i], cell[j]));
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        for (a, b) in pairs {
            let (ax0, ay0, ax1, ay1) = expanded(&components[a]);
            let (bx0, by0, bx1, by1) = expanded(&components[b]);
            if ax0 <= bx1 && bx0 <= ax1 && ay0 <= by1 && by0 <= ay1 {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[rb] = ra;
                }
            }
        }
    }
    let mut by_root: HashMap<usize, Component> = HashMap::new();
    let mut order: Vec<usize> = Vec::new();
    for (idx, comp) in components.iter().enumerate() {
        let root = find(&mut parent, idx);
        match by_root.get_mut(&root) {
            Some(acc) => {
                acc.min_x = acc.min_x.min(comp.min_x);
                acc.min_y = acc.min_y.min(comp.min_y);
                acc.max_x = acc.max_x.max(comp.max_x);
                acc.max_y = acc.max_y.max(comp.max_y);
                acc.pixel_count += comp.pixel_count;
                acc.delta_sum += comp.delta_sum;
                acc.max_delta = acc.max_delta.max(comp.max_delta);
            }
            None => {
                order.push(root);
                by_root.insert(
                    root,
                    Component {
                        min_x: comp.min_x,
                        min_y: comp.min_y,
                        max_x: comp.max_x,
                        max_y: comp.max_y,
                        pixel_count: comp.pixel_count,
                        delta_sum: comp.delta_sum,
                        max_delta: comp.max_delta,
                    },
                );
            }
        }
    }
    order
        .into_iter()
        .filter_map(|root| by_root.remove(&root))
        .collect()
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
