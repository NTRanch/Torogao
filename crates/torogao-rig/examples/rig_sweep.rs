//! Parameter sweep proof on the standing estate protocol
//! (RENDER-1 pattern, D-859 gate a): render the rest frame (all
//! defaults), then for every parameter render at min and at max
//! with all other parameters at default (30 frames each so physics
//! settles). A third render at the peak axis point (largest binding
//! deviation from the default point) covers cycle-shaped channels
//! whose extremes both equal the rest pose. A parameter passes iff
//! (1) at least one extreme changes > 0 pixels vs rest
//! (per-channel max diff > 8) and (2) the minimum region fraction
//! over the rendered extremes is >= 0.98, where the region is the
//! single bounding box of the bound nodes' descendant parts'
//! static AABBs (ancestor translations accumulated, mesh verts),
//! dilated by the largest binding offset + 14 model units.
//! Extremes equal to the default are skipped; an empty region
//! excludes the fraction test.
//!
//! Usage: rig_sweep --inx FILE --out FILE.json
//!        [--frames N] [--scale S] [--width W] [--height H]

use std::collections::BTreeSet;
use std::process::ExitCode;

use torogao_rig::math::Vec2;
use torogao_rig::model::{BindingKind, BindingValues, NodeKind, Param, Puppet};
use torogao_rig::{Canvas, Evaluator, load_inp, render_frame};

const DIFF_THRESHOLD: i32 = 8;
const REGION_FRACTION_MIN: f64 = 0.98;
const DILATION_BASE: f32 = 14.0;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut inx: Option<String> = None;
    let mut out: Option<String> = None;
    let mut frames: usize = 30;
    let mut scale: f32 = 1.0;
    let mut width: usize = 600;
    let mut height: usize = 800;
    let mut i = 0;
    while i < args.len() {
        let take = |i: &mut usize| -> Option<String> {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--inx" => inx = take(&mut i),
            "--out" => out = take(&mut i),
            "--frames" => frames = take(&mut i).and_then(|v| v.parse().ok()).unwrap_or(30),
            "--scale" => scale = take(&mut i).and_then(|v| v.parse().ok()).unwrap_or(1.0),
            "--width" => width = take(&mut i).and_then(|v| v.parse().ok()).unwrap_or(600),
            "--height" => height = take(&mut i).and_then(|v| v.parse().ok()).unwrap_or(800),
            other => {
                eprintln!("rig_sweep: unknown argument '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let (Some(inx), Some(out)) = (inx, out) else {
        eprintln!("usage: rig_sweep --inx FILE --out FILE.json");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(&inx) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("rig_sweep: cannot read {inx}: {e}");
            return ExitCode::from(2);
        }
    };
    let model = match load_inp(&bytes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("rig_sweep: import failed: {e}");
            return ExitCode::from(2);
        }
    };
    let statics = StaticGeometry::build(&model);
    let rest = render_candidate(&model, None, frames, width, height, scale);
    let mut param_reports = Vec::new();
    let mut passed = 0usize;
    for param in &model.params {
        let region = statics.region_box(param);
        let dilation = dilation(param);
        let mut tests: Vec<(&str, f32)> = vec![("min", param.min.x), ("max", param.max.x)];
        if let Some(peak) = peak_value(param) {
            if peak != param.min.x && peak != param.max.x && peak != param.defaults.x {
                tests.push(("peak", peak));
            }
        }
        let mut candidate_reports = Vec::new();
        let mut moved = 0usize;
        let mut fractions: Vec<f64> = Vec::new();
        for (label, value) in tests {
            if value == param.defaults.x {
                candidate_reports.push(serde_json::json!({
                    "at": label, "value": value, "changed_px": 0,
                    "note": "extreme==default", "pass": false,
                }));
                continue;
            }
            let canvas =
                render_candidate(&model, Some((param, value)), frames, width, height, scale);
            let changed = changed_pixels(&rest, &canvas);
            moved = moved.max(changed.len());
            // The protocol leaves the fraction unset for a
            // zero-change extreme, excluding it from the minimum.
            let fraction = if changed.is_empty() {
                None
            } else {
                region.map(|r| {
                    let inside = changed
                        .iter()
                        .filter(|&&(px, py)| in_box(r, dilation, px, py, width, height, scale))
                        .count();
                    inside as f64 / changed.len() as f64
                })
            };
            if let Some(f) = fraction {
                fractions.push(f);
            }
            candidate_reports.push(serde_json::json!({
                "at": label, "value": value, "changed_px": changed.len(),
                "region_fraction": fraction, "pass": !changed.is_empty(),
            }));
        }
        let min_fraction = fractions.iter().copied().reduce(f64::min);
        let param_pass = moved > 0
            && match min_fraction {
                None => true,
                Some(f) => f >= REGION_FRACTION_MIN,
            };
        if param_pass {
            passed += 1;
        }
        param_reports.push(serde_json::json!({
            "name": param.name,
            "pass": param_pass,
            "candidates": candidate_reports,
        }));
    }
    let report = serde_json::json!({
        "file": inx,
        "protocol": "standing sweep protocol (RENDER-1 pattern): rest vs min/max/peak, 30 frames, diff>8, min region fraction>=0.98 over the bound-descendant AABB box",
        "passed": passed,
        "total": model.params.len(),
        "params": param_reports,
    });
    if let Err(e) = std::fs::write(
        &out,
        serde_json::to_string_pretty(&report).unwrap_or_default(),
    ) {
        eprintln!("rig_sweep: cannot write {out}: {e}");
        return ExitCode::from(2);
    }
    println!(
        "{{\"passed\": {}, \"total\": {}}}",
        passed,
        model.params.len()
    );
    ExitCode::SUCCESS
}

/// Static payload geometry: per-part model-space AABBs from the
/// raw record (ancestor translations accumulated; no rotation or
/// scale, matching the protocol script), and descendant-part sets.
struct StaticGeometry {
    /// Part node index -> static AABB [x0, y0, x1, y1].
    part_aabbs: Vec<Option<[f32; 4]>>,
    /// Node index -> set of descendant part indices (incl. self).
    descendants: Vec<BTreeSet<usize>>,
}

impl StaticGeometry {
    fn build(model: &Puppet) -> StaticGeometry {
        let mut part_aabbs: Vec<Option<[f32; 4]>> = vec![None; model.nodes.len()];
        fn accumulate(
            model: &Puppet,
            index: usize,
            ox: f32,
            oy: f32,
            part_aabbs: &mut [Option<[f32; 4]>],
        ) {
            let node = &model.nodes[index];
            let (ax, ay) = (
                ox + node.offset.translation.x,
                oy + node.offset.translation.y,
            );
            if let NodeKind::Part(part) = &node.kind {
                if !part.mesh.verts.is_empty() {
                    let mut aabb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                    for v in &part.mesh.verts {
                        aabb[0] = aabb[0].min(ax + v.x);
                        aabb[1] = aabb[1].min(ay + v.y);
                        aabb[2] = aabb[2].max(ax + v.x);
                        aabb[3] = aabb[3].max(ay + v.y);
                    }
                    part_aabbs[index] = Some(aabb);
                }
            }
            for &child in &node.children {
                accumulate(model, child, ax, ay, part_aabbs);
            }
        }
        if !model.nodes.is_empty() {
            accumulate(model, 0, 0.0, 0.0, &mut part_aabbs);
        }
        let mut descendants: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); model.nodes.len()];
        fn collect(
            model: &Puppet,
            index: usize,
            part_aabbs: &[Option<[f32; 4]>],
            descendants: &mut [BTreeSet<usize>],
        ) -> BTreeSet<usize> {
            let mut set = BTreeSet::new();
            if part_aabbs[index].is_some() {
                set.insert(index);
            }
            for &child in &model.nodes[index].children {
                let sub = collect(model, child, part_aabbs, descendants);
                set.extend(sub);
            }
            descendants[index] = set.clone();
            set
        }
        if !model.nodes.is_empty() {
            collect(model, 0, &part_aabbs, &mut descendants);
        }
        StaticGeometry {
            part_aabbs,
            descendants,
        }
    }

    /// The single bounding box over the AABBs of every part
    /// descended from a node this parameter binds to, or None
    /// when the parameter binds no meshed part.
    fn region_box(&self, param: &Param) -> Option<[f32; 4]> {
        let mut region: Option<[f32; 4]> = None;
        for binding in &param.bindings {
            for &part in &self.descendants[binding.node] {
                if let Some(aabb) = self.part_aabbs[part] {
                    region = Some(match region {
                        None => aabb,
                        Some(r) => [
                            r[0].min(aabb[0]),
                            r[1].min(aabb[1]),
                            r[2].max(aabb[2]),
                            r[3].max(aabb[3]),
                        ],
                    });
                }
            }
        }
        region
    }
}

/// Render one candidate: a fresh evaluator stepped `frames` times
/// with an optional single parameter set on the x component.
fn render_candidate(
    model: &Puppet,
    set: Option<(&Param, f32)>,
    frames: usize,
    width: usize,
    height: usize,
    scale: f32,
) -> Canvas {
    let mut evaluator = Evaluator::new(model.clone());
    for _ in 0..frames {
        evaluator.begin_frame();
        if let Some((param, value)) = set {
            evaluator.set_param_index(
                model.param_index(&param.name).unwrap_or(usize::MAX),
                Vec2::new(value, 0.0),
            );
        }
        evaluator.end_frame(1.0 / 60.0);
    }
    render_frame(&evaluator, width, height, scale)
}

/// The peak-deviation axis value (protocol form): the axis point
/// whose bindings deviate most from the default point's values,
/// deviation taken as the maximum absolute element difference
/// over all bindings. None for axes of two or fewer points.
fn peak_value(param: &Param) -> Option<f32> {
    let axis = &param.axis_points[0];
    if axis.len() <= 2 {
        return None;
    }
    let span = param.max.x - param.min.x;
    let default_t = if span == 0.0 {
        0.0
    } else {
        (param.defaults.x - param.min.x) / span
    };
    let default_index = axis
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (**a - default_t).abs().total_cmp(&(**b - default_t).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let mut best = (default_index, 0.0f32);
    for (i, _) in axis.iter().enumerate() {
        let mut deviation = 0.0f32;
        for binding in &param.bindings {
            deviation = deviation.max(binding_deviation(&binding.values, i, default_index));
        }
        if deviation > best.1 {
            best = (i, deviation);
        }
    }
    Some(param.min.x + axis[best.0] * span)
}

/// Maximum absolute element difference between two grid columns
/// of a binding (flattened, per the protocol script).
fn binding_deviation(values: &BindingValues, x: usize, default_x: usize) -> f32 {
    fn max_dev<T: Copy>(
        grid: &[Vec<T>],
        x: usize,
        default_x: usize,
        dev: impl Fn(T, T) -> f32,
    ) -> f32 {
        let (Some(col), Some(base)) = (grid.get(x), grid.get(default_x)) else {
            return 0.0;
        };
        col.iter()
            .zip(base.iter())
            .fold(0.0f32, |acc, (&a, &b)| acc.max(dev(a, b)))
    }
    match values {
        BindingValues::Scalars(grid) => max_dev(grid, x, default_x, |a, b| (a - b).abs()),
        BindingValues::Vectors(grid) => max_dev(grid, x, default_x, |a, b| {
            (a.x - b.x).abs().max((a.y - b.y).abs())
        }),
        BindingValues::Deforms(grid) => {
            let (Some(col), Some(base)) = (grid.get(x), grid.get(default_x)) else {
                return 0.0;
            };
            col.iter().zip(base.iter()).fold(0.0f32, |acc, (a, b)| {
                a.iter().zip(b.iter()).fold(acc, |acc2, (&u, &v)| {
                    acc2.max((u.x - v.x).abs()).max((u.y - v.y).abs())
                })
            })
        }
    }
}

/// The dilation for a parameter's region (protocol form): the
/// largest binding offset + 14. Deform bindings contribute every
/// cell's component magnitudes; translate bindings contribute
/// the first element of each axis row.
fn dilation(param: &Param) -> f32 {
    let mut dil = DILATION_BASE;
    for binding in &param.bindings {
        match (&binding.values, binding.kind) {
            (BindingValues::Deforms(grid), _) => {
                for cell in grid.iter().flatten().flatten() {
                    dil = dil
                        .max(cell.x.abs() + DILATION_BASE)
                        .max(cell.y.abs() + DILATION_BASE);
                }
            }
            (BindingValues::Scalars(grid), BindingKind::TranslateX | BindingKind::TranslateY) => {
                for row in grid {
                    if let Some(first) = row.first() {
                        dil = dil.max(first.abs() + DILATION_BASE);
                    }
                }
            }
            (BindingValues::Vectors(grid), BindingKind::Translate) => {
                for row in grid {
                    if let Some(first) = row.first() {
                        dil = dil.max(first.x.abs() + DILATION_BASE);
                    }
                }
            }
            _ => {}
        }
    }
    dil
}

/// Pixels whose per-channel max difference exceeds the threshold.
fn changed_pixels(a: &Canvas, b: &Canvas) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for y in 0..a.height {
        for x in 0..a.width {
            let index = (y * a.width + x) * 4;
            let mut delta = 0i32;
            for c in 0..4 {
                delta = delta.max((a.pixels[index + c] as i32 - b.pixels[index + c] as i32).abs());
            }
            if delta > DIFF_THRESHOLD {
                out.push((x, y));
            }
        }
    }
    out
}

/// Whether a changed pixel falls inside the protocol region:
/// the dilated model-space box projected to canvas pixels with
/// the script's integer bounds.
fn in_box(
    region: [f32; 4],
    dilation: f32,
    px: usize,
    py: usize,
    width: usize,
    height: usize,
    scale: f32,
) -> bool {
    let (cx, cy) = (width as f32 * 0.5, height as f32 * 0.5);
    let px0 = ((cx + (region[0] - dilation) * scale) as i64).max(0);
    let py0 = ((cy + (region[1] - dilation) * scale) as i64).max(0);
    let px1 = ((cx + (region[2] + dilation) * scale) as i64 + 1).min(width as i64);
    let py1 = ((cy + (region[3] + dilation) * scale) as i64 + 1).min(height as i64);
    let (x, y) = (px as i64, py as i64);
    x >= px0 && x < px1 && y >= py0 && y < py1
}
