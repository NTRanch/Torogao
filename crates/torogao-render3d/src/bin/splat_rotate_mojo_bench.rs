//! Extension-trial harness (2026-11-27): the Mojo splat-rotation
//! kernel (`mojo/splat_rotate.mojo`) against the CPU reference
//! (`splat::rotate_cloud`) on a full-size witness.
//!
//! Profiling basis (adoption lane, full 1,157,141-splat bonsai
//! witness, medians): projection 563.8 ms, layer rotation
//! 401.8 ms standalone (71% of projection's cost), frame-record
//! build 160.1 ms. Rotation is the trial target: the hottest
//! remaining pure-f64 step, with no transcendentals — the same
//! property that made projection bit-exact.
//!
//! The kernel works on the estate's frame records
//! (`splat_gpu::GpuSplatRecord`, 256 bytes): rotation touches
//! exactly the fields the record carries bit-identically from
//! the asset splat (pos, quaternion, SH rest coefficients), and
//! the expected side is `frame_records(rotate_cloud(..))`, so
//! the trial judges the real route's data, not a paraphrase.
//!
//! The witness rotation is 40° about the (1, 2, 3) axis, built
//! exactly as the splat_rotate unit gates build theirs
//! (axis-angle through `Rotation::from_quaternion`), so every
//! matrix entry, quaternion component and SH band is fully
//! exercised.
//!
//! Verdict rule: the kernel's output records must equal the CPU
//! reference's records bit-for-bit (all 64 f32 fields per
//! record), or the trial is recorded as NOT REACHED with the
//! divergence measured (records/fields affected, worst absolute
//! deviation, first divergence named). Exit 0 = bit-exact,
//! 2 = diverged, 1 = harness failure.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use torogao_render3d::splat::{SplatCloud, rotate_cloud};
use torogao_render3d::splat_gpu::frame_records;
use torogao_render3d::splat_rotate::{Rotation, ShRotation};

const HEADER_BYTES: usize = 776;
const RECORD_BYTES: usize = 256;
const FIELDS: usize = 64;

struct Args {
    ply: PathBuf,
    mojo_bin: PathBuf,
    work: PathBuf,
    device_id: u32,
    reps: u32,
    angle_deg: f64,
}

fn parse_args() -> Result<Args, String> {
    let mut ply = None;
    let mut mojo_bin = None;
    let mut work = None;
    let mut device_id = 1u32;
    let mut reps = 20u32;
    let mut angle_deg = 40.0f64;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut take = |slot: &mut Option<PathBuf>| -> Result<(), String> {
            let v = it.next().ok_or_else(|| format!("{a} needs a value"))?;
            *slot = Some(PathBuf::from(v));
            Ok(())
        };
        match a.as_str() {
            "--ply" => take(&mut ply)?,
            "--mojo-bin" => take(&mut mojo_bin)?,
            "--work" => take(&mut work)?,
            "--device-id" => {
                device_id = it
                    .next()
                    .ok_or("--device-id needs a value")?
                    .parse()
                    .map_err(|_| "bad --device-id")?;
            }
            "--reps" => {
                reps = it
                    .next()
                    .ok_or("--reps needs a value")?
                    .parse()
                    .map_err(|_| "bad --reps")?;
            }
            "--angle-deg" => {
                angle_deg = it
                    .next()
                    .ok_or("--angle-deg needs a value")?
                    .parse()
                    .map_err(|_| "bad --angle-deg")?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        ply: ply.ok_or("--ply is required")?,
        mojo_bin: mojo_bin.ok_or("--mojo-bin is required")?,
        work: work.ok_or("--work is required")?,
        device_id,
        reps,
        angle_deg,
    })
}

fn field_name(f: usize) -> &'static str {
    match f {
        0..=2 => "position",
        3 => "opacity (copy-through)",
        4..=6 => "dc (copy-through)",
        7 => "pad",
        8..=10 => "scale (copy-through)",
        11 => "pad",
        12..=15 => "quaternion",
        16..=60 => "sh-rest",
        _ => "tail pad",
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("splat_rotate_mojo_bench: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("splat_rotate_mojo_bench: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let bytes = std::fs::read(&args.ply).map_err(|e| format!("read ply: {e}"))?;
    println!("asset: {} ({} bytes)", args.ply.display(), bytes.len());
    let t = Instant::now();
    let cloud: SplatCloud = SplatCloud::load_ply(&bytes).map_err(|e| format!("load ply: {e}"))?;
    let count = cloud.splats.len();
    println!(
        "load: {:.1} ms, {} splats",
        t.elapsed().as_secs_f64() * 1e3,
        count
    );

    // The witness rotation: 40 degrees about (1, 2, 3), built
    // the splat_rotate test helper's way.
    let angle = args.angle_deg.to_radians();
    let axis = [1.0_f64, 2.0, 3.0];
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let s = (angle / 2.0).sin() / n;
    let rotation =
        Rotation::from_quaternion([(angle / 2.0).cos(), axis[0] * s, axis[1] * s, axis[2] * s])
            .ok_or("witness rotation refused")?;
    println!(
        "rotation: {:.1} deg about (1, 2, 3); quat (w, x, y, z) = {:?}",
        args.angle_deg, rotation.quat
    );

    // CPU reference + timing.
    let mut times = Vec::new();
    let mut expected: Option<SplatCloud> = None;
    for _ in 0..5 {
        let t = Instant::now();
        let c = rotate_cloud(&cloud, &rotation);
        times.push(t.elapsed().as_secs_f64() * 1e3);
        expected = Some(c);
    }
    let expected = expected.unwrap();
    println!(
        "cpu rotate_cloud: median {:.1} ms over 5 reps",
        median(times)
    );

    // Frame records on both sides.
    let in_records = frame_records(&cloud);
    let in_bytes: &[u8] = bytemuck::cast_slice(&in_records);
    debug_assert_eq!(in_bytes.len(), count * RECORD_BYTES);
    let exp_records = frame_records(&expected);
    let exp_bytes: &[u8] = bytemuck::cast_slice(&exp_records);
    debug_assert_eq!(exp_bytes.len(), count * RECORD_BYTES);

    // Kernel input: header (count, matrix, quat, flat band
    // block) + input records.
    std::fs::create_dir_all(&args.work).map_err(|e| format!("mkdir work: {e}"))?;
    let mut input = Vec::with_capacity(HEADER_BYTES + in_bytes.len());
    input.extend_from_slice(&(count as u64).to_le_bytes());
    for row in &rotation.matrix {
        for v in row {
            input.extend_from_slice(&v.to_le_bytes());
        }
    }
    for v in &rotation.quat {
        input.extend_from_slice(&v.to_le_bytes());
    }
    let flat = ShRotation::new(&rotation.matrix).matrices_flat();
    for v in &flat {
        input.extend_from_slice(&v.to_le_bytes());
    }
    debug_assert_eq!(input.len(), HEADER_BYTES);
    input.extend_from_slice(in_bytes);
    let in_path = args.work.join("rotate-in.bin");
    std::fs::write(&in_path, &input).map_err(|e| format!("write input: {e}"))?;
    let exp_path = args.work.join("rotate-expected.bin");
    std::fs::write(&exp_path, exp_bytes).map_err(|e| format!("write expected: {e}"))?;

    let out_path = args.work.join("rotate-kernel-out.bin");
    let t = Instant::now();
    let output = Command::new(&args.mojo_bin)
        .arg(&in_path)
        .arg(&out_path)
        .arg(args.reps.to_string())
        .arg(args.device_id.to_string())
        .output()
        .map_err(|e| format!("spawn mojo kernel: {e}"))?;
    println!(
        "mojo process wall (incl. runtime start): {:.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        return Err(format!(
            "mojo kernel exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Bitwise comparison, field by field.
    let got = std::fs::read(&out_path).map_err(|e| format!("read kernel output: {e}"))?;
    if got.len() != exp_bytes.len() {
        return Err(format!(
            "output length {} != expected {}",
            got.len(),
            exp_bytes.len()
        ));
    }
    let mut bad_records = 0usize;
    let mut bad_fields = 0usize;
    let mut worst = 0.0f64;
    let mut worst_field = 0usize;
    let mut first: Option<(usize, usize, u32, u32)> = None;
    for i in 0..count {
        let base = i * RECORD_BYTES;
        let mut rec_bad = false;
        for f in 0..FIELDS {
            let off = base + f * 4;
            let eb = u32::from_le_bytes(exp_bytes[off..off + 4].try_into().unwrap());
            let gb = u32::from_le_bytes(got[off..off + 4].try_into().unwrap());
            if eb != gb {
                rec_bad = true;
                bad_fields += 1;
                let d = (f32::from_bits(eb) as f64 - f32::from_bits(gb) as f64).abs();
                if d > worst {
                    worst = d;
                    worst_field = f;
                }
                if first.is_none() {
                    first = Some((i, f, eb, gb));
                }
            }
        }
        if rec_bad {
            bad_records += 1;
        }
    }
    println!(
        "gate: {} splats; records with field mismatches {}, fields mismatched {}",
        count, bad_records, bad_fields
    );
    if bad_records == 0 {
        println!("GATE: BIT-EXACT — all rotated records identical");
        Ok(())
    } else {
        let (i, f, eb, gb) = first.unwrap();
        println!(
            "first divergence: record {i} field {f} ({}): expected {} (bits {eb:#010x}), got {} (bits {gb:#010x})",
            field_name(f),
            f32::from_bits(eb),
            f32::from_bits(gb)
        );
        println!(
            "worst absolute deviation: {worst:.6e} (field class {})",
            field_name(worst_field)
        );
        println!("GATE: DIVERGED — trial not reached (see lane record)");
        std::process::exit(2);
    }
}
