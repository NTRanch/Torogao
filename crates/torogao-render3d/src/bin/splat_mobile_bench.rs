//! Mobile splat-projection benchmark and gate
//! (render3d-mobile-compute lane, 2026-11-27).
//!
//! One scene — the estate's bonsai witness under the
//! bench plate config A the desktop GPU benches use
//! (512x640, identity layer transform, unlit) — one
//! CPU reference (`splat::project_splats`, f64), and
//! every Vulkan device on the machine measured
//! through `backend::mobile_project`:
//!
//! - the fp64 leg (devices with shaderFloat64) is
//!   gated BITWISE against the CPU reference —
//!   records, cull state, and the radix depth keys;
//! - the f32 leg (the phone path) is gated against
//!   the estate's Tier 2 band — per-field max abs /
//!   rel error, band failures under the combined
//!   criterion |err| <= 1e-6 + 1e-6*|ref|, cull-set
//!   agreement, and the depth-order damage (position
//!   mismatches + inversions vs the CPU order);
//! - timings separate upload / dispatch (median over
//!   `--dispatch-reps`, records resident) / readback —
//!   the same honesty split as the Mojo lane's record.
//!
//! Modes: `--cpu-only` (the phone-CPU baseline shape,
//! under qemu or an on-device shell), `--device 0,2`,
//! `--limit N`, `--out FILE` (the phone harness: the
//! same lines written to a file the Owner can share
//! back from the physical device).
//!
//! The plate-scene helpers are copied verbatim from
//! splat_mojo_bench.rs (themselves from
//! splat_bench.rs) so every bench in the crate frames
//! the witness identically.

use std::time::Instant;

use torogao_render3d::backend::mobile_project::MobileProjector;
use torogao_render3d::math::Mat4;
use torogao_render3d::setscene::{ArcScene, plate_rig_scene};
use torogao_render3d::splat::{SplatCloud, SplatTransform, project_splats};
use torogao_render3d::splat_gpu::{SplatCamera, SplatGpuParams, depth_key_words, frame_records};

const WIDTH: u32 = 512;
const HEIGHT: u32 = 640;

struct Log {
    lines: Vec<String>,
}

impl Log {
    fn say(&mut self, line: String) {
        println!("{line}");
        self.lines.push(line);
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// Per-field error gauge against the Tier 2 band.
struct FieldStat {
    name: &'static str,
    max_abs: f64,
    max_rel: f64,
    band_fail: usize,
    first_fail: Option<usize>,
}

impl FieldStat {
    fn new(name: &'static str) -> FieldStat {
        FieldStat {
            name,
            max_abs: 0.0,
            max_rel: 0.0,
            band_fail: 0,
            first_fail: None,
        }
    }

    fn observe(&mut self, index: usize, got: f64, want: f64) {
        let abs = (got - want).abs();
        let rel = if want != 0.0 { abs / want.abs() } else { abs };
        if abs > self.max_abs {
            self.max_abs = abs;
        }
        if rel > self.max_rel {
            self.max_rel = rel;
        }
        if abs > 1e-6 + 1e-6 * want.abs() {
            self.band_fail += 1;
            if self.first_fail.is_none() {
                self.first_fail = Some(index);
            }
        }
    }

    /// The eigen-axis error itself: axes are
    /// sign-ambiguous, so the error is the better of
    /// the two signs (max component).
    fn axis_err(got: [f32; 2], want: [f64; 2]) -> f64 {
        let plus = (f64::from(got[0]) - want[0])
            .abs()
            .max((f64::from(got[1]) - want[1]).abs());
        let minus = (f64::from(got[0]) + want[0])
            .abs()
            .max((f64::from(got[1]) + want[1]).abs());
        plus.min(minus)
    }

    /// The eigen-axis gauge against a unit reference —
    /// the band criterion is |err| <= 2e-6.
    fn observe_axis(&mut self, index: usize, got: [f32; 2], want: [f64; 2]) {
        let abs = Self::axis_err(got, want);
        if abs > self.max_abs {
            self.max_abs = abs;
        }
        if abs > self.max_rel {
            self.max_rel = abs;
        }
        if abs > 2e-6 {
            self.band_fail += 1;
            if self.first_fail.is_none() {
                self.first_fail = Some(index);
            }
        }
    }

    fn line(&self) -> String {
        format!(
            "  {:<12} max_abs {:.3e}  max_rel {:.3e}  band_fail {}{}",
            self.name,
            self.max_abs,
            self.max_rel,
            self.band_fail,
            self.first_fail
                .map(|i| format!(" (first at {i})"))
                .unwrap_or_default()
        )
    }
}

/// Inversion count of a permutation (merge count).
fn inversions(perm: &[u32]) -> u64 {
    fn merge_count(a: &mut [u32], tmp: &mut [u32]) -> u64 {
        if a.len() <= 1 {
            return 0;
        }
        let mid = a.len() / 2;
        let (lo, hi) = a.split_at_mut(mid);
        let (tlo, thi) = tmp.split_at_mut(mid);
        let mut count = merge_count(lo, tlo) + merge_count(hi, thi);
        let (mut i, mut j, mut k) = (0, 0, 0);
        while i < lo.len() && j < hi.len() {
            if lo[i] <= hi[j] {
                tmp[k] = lo[i];
                i += 1;
            } else {
                tmp[k] = hi[j];
                count += (lo.len() - i) as u64;
                j += 1;
            }
            k += 1;
        }
        while i < lo.len() {
            tmp[k] = lo[i];
            i += 1;
            k += 1;
        }
        while j < hi.len() {
            tmp[k] = hi[j];
            j += 1;
            k += 1;
        }
        a.copy_from_slice(tmp);
        count
    }
    let mut a = perm.to_vec();
    let mut tmp = vec![0u32; a.len()];
    merge_count(&mut a, &mut tmp)
}

// --- Plate scene, copied verbatim from
// splat_mojo_bench.rs (itself from splat_bench.rs).

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

fn camera_world(eye: [f64; 3], target: [f64; 3], up_hint: [f64; 3]) -> Mat4 {
    let forward = norm(sub(target, eye));
    let right = norm(cross(forward, up_hint));
    let up = cross(right, forward);
    let back = [-forward[0], -forward[1], -forward[2]];
    Mat4::from_rows([
        [right[0], up[0], back[0], eye[0]],
        [right[1], up[1], back[1], eye[1]],
        [right[2], up[2], back[2], eye[2]],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

fn cloud_bounds(cloud: &SplatCloud) -> ([f64; 3], f64) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for s in &cloud.splats {
        for i in 0..3 {
            lo[i] = lo[i].min(f64::from(s.pos[i]));
            hi[i] = hi[i].max(f64::from(s.pos[i]));
        }
    }
    let centre = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let mut radius = 0.0_f64;
    for s in &cloud.splats {
        let d = sub(
            [
                f64::from(s.pos[0]),
                f64::from(s.pos[1]),
                f64::from(s.pos[2]),
            ],
            centre,
        );
        radius = radius.max((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
    }
    (centre, radius)
}

fn plate_scene(cloud: &SplatCloud) -> (ArcScene, [f64; 3]) {
    let (c, r) = cloud_bounds(cloud);
    let dir = norm([0.55, -0.45, -0.85]);
    let eye0 = [c[0] + dir[0], c[1] + dir[1], c[2] + dir[2]];
    let lens = plate_rig_scene(camera_world(eye0, c, [0.0, -1.0, 0.0]));
    let tan_min = lens.tan_half_x.min(lens.tan_half_y);
    let dist = r / tan_min * 1.02;
    let eye = [
        c[0] + dir[0] * dist,
        c[1] + dir[1] * dist,
        c[2] + dir[2] * dist,
    ];
    (plate_rig_scene(camera_world(eye, c, [0.0, -1.0, 0.0])), eye)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut ply: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut reps = 3usize;
    let mut dispatch_reps = 5usize;
    let mut limit: Option<usize> = None;
    let mut devices: Option<Vec<usize>> = None;
    let mut cpu_only = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ply" => {
                ply = Some(args[i + 1].clone());
                i += 2;
            }
            "--out" => {
                out_path = Some(args[i + 1].clone());
                i += 2;
            }
            "--reps" => {
                reps = args[i + 1].parse().expect("--reps");
                i += 2;
            }
            "--dispatch-reps" => {
                dispatch_reps = args[i + 1].parse().expect("--dispatch-reps");
                i += 2;
            }
            "--limit" => {
                limit = Some(args[i + 1].parse().expect("--limit"));
                i += 2;
            }
            "--device" => {
                devices = Some(
                    args[i + 1]
                        .split(',')
                        .map(|s| s.parse().expect("--device"))
                        .collect(),
                );
                i += 2;
            }
            "--cpu-only" => {
                cpu_only = true;
                i += 1;
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(ply) = ply else {
        eprintln!(
            "usage: splat_mobile_bench --ply point_cloud.ply [--out FILE] [--limit N] \
             [--device 0,2] [--cpu-only] [--reps N] [--dispatch-reps N]"
        );
        std::process::exit(2);
    };

    let mut log = Log { lines: Vec::new() };
    let bytes = std::fs::read(&ply).expect("read ply");
    log.say(format!("asset: {ply} ({} bytes)", bytes.len()));
    let t0 = Instant::now();
    let mut cloud = SplatCloud::load_ply(&bytes).expect("strict load");
    log.say(format!(
        "strict load: {:.1} ms, {} splats",
        t0.elapsed().as_secs_f64() * 1e3,
        cloud.splats.len()
    ));
    if let Some(n) = limit {
        cloud.splats.truncate(n);
        log.say(format!("limited to first {n} splats (--limit)"));
    }
    let n = cloud.splats.len();
    let (scene, _eye) = plate_scene(&cloud);
    let transform = SplatTransform::default();
    let camera = SplatCamera::from_scene(&scene, WIDTH, HEIGHT);

    // -- CPU reference (f64), timed.
    let mut cpu_walls = Vec::new();
    let mut cpu_out = Vec::new();
    for _ in 0..reps.max(1) {
        let t = Instant::now();
        cpu_out = project_splats(&cloud, &transform, &scene, WIDTH, HEIGHT, None);
        cpu_walls.push(t.elapsed().as_secs_f64() * 1e3);
    }
    log.say(format!(
        "cpu project_splats: median {:.1} ms over {} reps ({} splats, {} survivors)",
        median(cpu_walls),
        reps.max(1),
        n,
        cpu_out.len()
    ));
    // Asset index -> position in the CPU order (-1 =
    // culled by the reference).
    let mut cpu_pos = vec![-1i64; n];
    for (pos, p) in cpu_out.iter().enumerate() {
        cpu_pos[p.index] = pos as i64;
    }

    if !cpu_only {
        let infos = MobileProjector::list_devices().unwrap_or_else(|e| panic!("census: {e}"));
        log.say(format!("vulkan devices: {}", infos.len()));
        for (idx, info) in infos.iter().enumerate() {
            log.say(format!(
                "  [{idx}] {} ({}, Vulkan {}.{}.{}) shaderFloat64={} shaderInt64={}",
                info.name,
                info.kind,
                info.api.0,
                info.api.1,
                info.api.2,
                info.shader_float64,
                info.shader_int64
            ));
        }
        let selected = devices.unwrap_or_else(|| (0..infos.len()).collect());
        let records = frame_records(&cloud);
        let params64 = SplatGpuParams::new(&camera, &transform, [0.0; 3], None, None);

        for idx in selected {
            log.say(format!("== device {idx}: {} ==", infos[idx].name));
            let mut proj = match MobileProjector::new(idx) {
                Ok(p) => p,
                Err(e) => {
                    log.say(format!("  open failed: {e}"));
                    continue;
                }
            };

            // -- fp64 leg: bitwise gate.
            if infos[idx].shader_float64 {
                match proj.project_f64(&records, &params64, dispatch_reps) {
                    Ok(fp) => {
                        let (got, keys, t) = (&fp.records, &fp.keys, fp.timings);
                        let mut cull_mm = 0usize;
                        let mut field_mm = 0usize;
                        let mut recs_mm = 0usize;
                        let mut key_mm = 0usize;
                        for (ai, g) in got.iter().enumerate() {
                            let pos = cpu_pos[ai];
                            if (g.culled == 1) != (pos < 0) {
                                cull_mm += 1;
                                continue;
                            }
                            if pos < 0 {
                                continue;
                            }
                            let want = &cpu_out[pos as usize];
                            let mut bad = 0usize;
                            for c in 0..2 {
                                bad += (g.centre_px[c].to_bits() != want.centre_px[c].to_bits())
                                    as usize;
                                bad += (g.radii[c].to_bits() != want.radii[c].to_bits()) as usize;
                                bad += (g.axis1[c].to_bits() != want.axes[0][c].to_bits()) as usize;
                            }
                            for c in 0..3 {
                                bad += (g.conic[c].to_bits() != want.conic[c].to_bits()) as usize;
                                bad += (g.colour[c].to_bits() != want.colour[c].to_bits()) as usize;
                            }
                            bad += (g.opacity.to_bits() != want.opacity.to_bits()) as usize;
                            bad += (g.z_ndc.to_bits() != want.z_ndc.to_bits()) as usize;
                            field_mm += bad;
                            recs_mm += (bad > 0) as usize;
                            let kw = depth_key_words(want.depth, ai as u32);
                            key_mm += (keys[ai] != [kw[0], kw[1], kw[2], 0]) as usize;
                        }
                        log.say(format!(
                            "  fp64: upload {:.1} ms, dispatch median {:.3} ms, readback {:.1} ms",
                            t.upload_ms, t.dispatch_ms, t.readback_ms
                        ));
                        log.say(format!(
                            "  fp64 gate: cull mismatches {cull_mm}, records with field \
                             mismatches {recs_mm} ({field_mm} fields), key mismatches {key_mm} — {}",
                            if cull_mm == 0 && recs_mm == 0 && key_mm == 0 {
                                "BIT-EXACT"
                            } else {
                                "NOT BIT-EXACT"
                            }
                        ));
                    }
                    Err(e) => log.say(format!("  fp64 leg failed: {e}")),
                }
            } else {
                log.say(
                    "  fp64: NOT AVAILABLE (no shaderFloat64) — the phone-class verdict"
                        .to_string(),
                );
            }

            // -- f32 leg: Tier 2 band gate.
            match proj.project_f32(&records, &camera, &transform, dispatch_reps) {
                Ok((got, t)) => {
                    let mut stats: Vec<FieldStat> = [
                        "centre_px[0]",
                        "centre_px[1]",
                        "radii[0]",
                        "radii[1]",
                        "axis1",
                        "depth",
                        "conic[0]",
                        "conic[1]",
                        "conic[2]",
                        "opacity",
                        "colour[0]",
                        "colour[1]",
                        "colour[2]",
                        "z_ndc",
                    ]
                    .iter()
                    .map(|name| FieldStat::new(name))
                    .collect();
                    let mut cull_mm = 0usize;
                    let mut band_records = 0usize;
                    // The axis gauge, conditioned on
                    // anisotropy: for a near-isotropic
                    // splat the eigen-axis is
                    // ill-conditioned (the quad is a
                    // near-circle; any axis describes
                    // it), so the component-wise band
                    // is only well-posed where the
                    // eigenvalue gap is real. Both
                    // numbers are reported — the raw
                    // one above, this one here.
                    let mut axis_cond = FieldStat::new("axis1(aniso>1%)");
                    let mut axis_max_low_aniso = 0.0f64;
                    let mut axis_low_aniso = 0usize;
                    // Error-vs-anisotropy buckets:
                    // (threshold, count, max err, band
                    // fails) — the curve that separates
                    // a conditioning tail (collapses as
                    // the gap grows) from a bug
                    // population (flat).
                    let mut axis_buckets: Vec<(f64, usize, f64, usize)> = [
                        (0.001, 0, 0.0, 0),
                        (0.01, 0, 0.0, 0),
                        (0.05, 0, 0.0, 0),
                        (0.1, 0, 0.0, 0),
                        (0.25, 0, 0.0, 0),
                        (0.5, 0, 0.0, 0),
                        (1.01, 0, 0.0, 0),
                    ]
                    .into_iter()
                    .collect();
                    for (ai, g) in got.iter().enumerate() {
                        let pos = cpu_pos[ai];
                        if (g.culled == 1) != (pos < 0) {
                            cull_mm += 1;
                            continue;
                        }
                        if pos < 0 {
                            continue;
                        }
                        let want = &cpu_out[pos as usize];
                        let fails_before: usize = stats.iter().map(|s| s.band_fail).sum();
                        stats[0].observe(ai, f64::from(g.centre_px[0]), want.centre_px[0]);
                        stats[1].observe(ai, f64::from(g.centre_px[1]), want.centre_px[1]);
                        stats[2].observe(ai, f64::from(g.radii[0]), want.radii[0]);
                        stats[3].observe(ai, f64::from(g.radii[1]), want.radii[1]);
                        stats[4].observe_axis(ai, g.axis1, want.axes[0]);
                        {
                            let (r0, r1) = (want.radii[0], want.radii[1]);
                            let aniso = (r0 * r0 - r1 * r1) / (r0 * r0 + r1 * r1);
                            let e = FieldStat::axis_err(g.axis1, want.axes[0]);
                            if aniso > 0.01 {
                                axis_cond.observe_axis(ai, g.axis1, want.axes[0]);
                            } else {
                                axis_low_aniso += 1;
                                if e > axis_max_low_aniso {
                                    axis_max_low_aniso = e;
                                }
                            }
                            for b in axis_buckets.iter_mut() {
                                if aniso <= b.0 {
                                    b.1 += 1;
                                    if e > b.2 {
                                        b.2 = e;
                                    }
                                    b.3 += (e > 2e-6) as usize;
                                    break;
                                }
                            }
                        }
                        stats[5].observe(ai, f64::from(g.depth), want.depth);
                        for c in 0..3 {
                            stats[6 + c].observe(
                                ai,
                                f64::from(g.conic[c]),
                                f64::from(want.conic[c]),
                            );
                            stats[10 + c].observe(
                                ai,
                                f64::from(g.colour[c]),
                                f64::from(want.colour[c]),
                            );
                        }
                        stats[9].observe(ai, f64::from(g.opacity), f64::from(want.opacity));
                        stats[13].observe(ai, f64::from(g.z_ndc), f64::from(want.z_ndc));
                        let fails_after: usize = stats.iter().map(|s| s.band_fail).sum();
                        band_records += (fails_after > fails_before) as usize;
                    }
                    // Depth-order damage: GPU order =
                    // f32 depth desc, index asc.
                    let mut gpu_order: Vec<u32> = got
                        .iter()
                        .filter(|g| g.culled == 0)
                        .map(|g| g.index)
                        .collect();
                    gpu_order.sort_by(|&a, &b| {
                        let da = f64::from(got[a as usize].depth);
                        let db = f64::from(got[b as usize].depth);
                        db.total_cmp(&da).then(a.cmp(&b))
                    });
                    let mut gpu_pos = vec![-1i64; n];
                    for (pos, &ai) in gpu_order.iter().enumerate() {
                        gpu_pos[ai as usize] = pos as i64;
                    }
                    let mut pos_mm = 0usize;
                    let mut perm: Vec<u32> = Vec::with_capacity(cpu_out.len());
                    for p in &cpu_out {
                        let gp = gpu_pos[p.index];
                        pos_mm += (gp != cpu_pos[p.index]) as usize;
                        if gp >= 0 {
                            perm.push(gp as u32);
                        }
                    }
                    let inv = inversions(&perm);
                    log.say(format!(
                        "  f32: upload {:.1} ms, dispatch median {:.3} ms, readback {:.1} ms",
                        t.upload_ms, t.dispatch_ms, t.readback_ms
                    ));
                    log.say(format!(
                        "  f32 gate (Tier 2 band 1e-6/1e-6): cull mismatches {cull_mm}, \
                         records with any band failure {band_records} / {}",
                        cpu_out.len()
                    ));
                    for s in &stats {
                        log.say(s.line());
                    }
                    log.say(axis_cond.line());
                    log.say(format!(
                        "  axis1 low-aniso records {axis_low_aniso}, their max axis err \
                         {axis_max_low_aniso:.3e} (ill-conditioned by construction)"
                    ));
                    let mut prev = 0.0;
                    for (thr, count, max_err, fails) in &axis_buckets {
                        log.say(format!(
                            "  axis1 aniso ({prev:.3},{thr:.3}]: {count} records, max err \
                             {max_err:.3e}, band fails {fails}"
                        ));
                        prev = *thr;
                    }
                    log.say(format!(
                        "  f32 order: position mismatches {pos_mm} / {}, inversions {inv}",
                        cpu_out.len()
                    ));
                }
                Err(e) => log.say(format!("  f32 leg failed: {e}")),
            }
        }
    }

    if let Some(path) = out_path {
        std::fs::write(&path, log.lines.join("\n") + "\n").expect("write --out");
        println!("wrote {path}");
    }
}
