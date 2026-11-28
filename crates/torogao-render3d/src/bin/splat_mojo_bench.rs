//! Mojo/PTX prototype harness (Mojo+PTX evaluation lane,
//! 2026-11-27): drives the crate's CPU splat projection
//! (`splat::project_splats`, the projection half of the pack) and
//! the Mojo GPU kernel in `mojo/splat_project.mojo` over the same
//! input — the bonsai production fixture under the bench's plate
//! config A (512x640, identity layer transform) — and gates
//! the kernel's output against the CPU reference record-for-record,
//! bitwise, per asset index. Default is unlit (config A); `--lit`
//! names the kernel-candidate-#1 light response (a non-trivial
//! grade + interior world mix against the scene's own world
//! colour) on both sides, so the same gate judges the response
//! transcription at full scale. `--shadow` names the
//! kernel-candidate-#2 shadow: the same response plus a shadow
//! map rasterised over the witness from overhead caster quads,
//! carried to the kernel as a device-input trailer; the bench
//! prints a visibility census of the CPU map so the caster
//! rig's coverage is on the record, not assumed.
//!
//! The kernel's input contract is the crate's own resident record
//! (`splat_gpu::frame_records`): opacity and scales pre-activated
//! on the host by the pack's own functions, so the device side
//! needs no transcendentals and every operation is IEEE-exact
//! f64 +, -, *, /, sqrt — the same property the Vulkan projection
//! (DESIGN 6.1) relies on for its bit-exact gate.
//!
//! Usage:
//!   splat-mojo-bench --ply PATH --mojo-bin PATH --work DIR
//!                    [--reps N] [--kernel-reps N] [--device-id N]
//!                    [--limit N] [--lit] [--shadow]
//!
//! Writes `inputs.bin` / `expected.bin` / `kernel-out.bin` into
//! the work dir and prints one fact per line (CPU median, kernel
//! timings as reported by the Mojo program, mismatch census).

use std::io::Write;
use std::process::{Command, ExitCode};
use std::time::Instant;

use torogao_render3d::math::Mat4;
use torogao_render3d::setscene::{ArcScene, plate_rig_scene, projection};
use torogao_render3d::splat::{SplatCloud, SplatProjection, SplatTransform, project_splats};
use torogao_render3d::splat_gpu::frame_records;
use torogao_render3d::splat_light::{LightResponse, SplatLight, SplatShadowMap};

const WIDTH: u32 = 512;
const HEIGHT: u32 = 640;
/// The pack's alpha cull (`splat.rs` ALPHA_CULL, f32).
const ALPHA_CULL: f32 = 1.0 / 255.0;
/// Header bytes before the record stream (see the Mojo source).
const HEADER_BYTES: usize = 640;
/// The `--shadow` gate's map edge: the production map size
/// (`splat_light::SHADOW_MAP_SIZE`), so the witness exercises
/// the route at the size production packs carry.
const SHADOW_SIZE: u32 = 2048;
/// The `--lit` gate's response: cross-channel terms and
/// negative entries (the clamp path) with an interior
/// world mix, so every term of `LightResponse::apply`
/// is exercised at full scale. The world colour is the
/// bench scene's own, read from the scene at run time.
const LIT_GRADE: [[f64; 3]; 3] = [[1.08, 0.02, -0.03], [0.01, 0.94, 0.04], [-0.02, 0.05, 1.12]];
const LIT_WORLD_MIX: f64 = 0.35;
/// Output record stride (see the Mojo source).
const OUT_BYTES: usize = 96;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

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

/// Camera world matrix from eye/target/up-hint — copied verbatim
/// from `splat_bench.rs` so this harness builds the identical
/// config-A scene the standing bench measures.
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

/// A light view-projection fitted to the witness
/// cloud, following `backend::light_view_proj`'s
/// construction line for line (same look-at, same
/// ortho shape) with the fit constants replaced by
/// the cloud's own centre and radius: the production
/// frustum is fitted to the arc set (half-extent
/// 2.6 m around (0, 0.85, 0)) and the bonsai witness
/// lives in raw PLY units spanning tens of units, so
/// under the production fit every centre falls
/// outside the map and the shadow gate would judge an
/// all-lit map. The route and the kernel consume
/// whatever view-projection the map carries, exactly
/// as production consumes the backend's — the gate's
/// subject is the transcription, not the fit.
fn witness_light_view_proj(sun_to_light: [f64; 3], centre: [f64; 3], radius: f64) -> Mat4 {
    let eye = [
        centre[0] + sun_to_light[0] * (2.0 * radius + 6.0),
        centre[1] + sun_to_light[1] * (2.0 * radius + 6.0),
        centre[2] + sun_to_light[2] * (2.0 * radius + 6.0),
    ];
    let up = if sun_to_light[1].abs() > 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let forward = norm(sub(centre, eye));
    let right = norm(cross(forward, up));
    let true_up = cross(right, forward);
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let view = Mat4::from_rows([
        [right[0], right[1], right[2], -dot(right, eye)],
        [true_up[0], true_up[1], true_up[2], -dot(true_up, eye)],
        [-forward[0], -forward[1], -forward[2], dot(forward, eye)],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let (extent, near, far) = (radius * 1.05, 1.0, 4.0 * radius + 13.0);
    let mut ortho = [0.0_f64; 16];
    ortho[0] = 1.0 / extent;
    ortho[5] = 1.0 / extent;
    ortho[10] = 1.0 / (near - far);
    ortho[14] = near / (near - far);
    ortho[15] = 1.0;
    Mat4 { m: ortho }.multiply(view)
}

/// Bounds of a cloud: centre (bounds midpoint) and the
/// bounding-sphere radius around it. Copied from `splat_bench.rs`.
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

/// The bench's plate scene for config A, built exactly as
/// `splat_bench.rs` builds it. Returns the scene and the camera
/// position (the eye the rig was placed at).
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

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.total_cmp(b));
    xs[xs.len() / 2]
}

/// One expected/kernel output record in the harness layout.
#[derive(Clone, Copy, Default)]
struct OutRecord {
    centre_px: [f64; 2],
    radii: [f64; 2],
    axis1: [f64; 2],
    depth: f64,
    conic: [f32; 3],
    opacity: f32,
    colour: [f32; 3],
    z_ndc: f32,
    culled: bool,
}

impl OutRecord {
    fn from_projection(p: &SplatProjection) -> Self {
        OutRecord {
            centre_px: p.centre_px,
            radii: p.radii,
            axis1: p.axes[0],
            depth: p.depth,
            conic: p.conic,
            opacity: p.opacity,
            colour: p.colour,
            z_ndc: p.z_ndc,
            culled: false,
        }
    }

    fn to_bytes(self) -> [u8; OUT_BYTES] {
        let mut b = [0u8; OUT_BYTES];
        b[0..8].copy_from_slice(&self.centre_px[0].to_le_bytes());
        b[8..16].copy_from_slice(&self.centre_px[1].to_le_bytes());
        b[16..24].copy_from_slice(&self.radii[0].to_le_bytes());
        b[24..32].copy_from_slice(&self.radii[1].to_le_bytes());
        b[32..40].copy_from_slice(&self.axis1[0].to_le_bytes());
        b[40..48].copy_from_slice(&self.axis1[1].to_le_bytes());
        b[48..56].copy_from_slice(&self.depth.to_le_bytes());
        b[56..60].copy_from_slice(&self.conic[0].to_le_bytes());
        b[60..64].copy_from_slice(&self.conic[1].to_le_bytes());
        b[64..68].copy_from_slice(&self.conic[2].to_le_bytes());
        b[68..72].copy_from_slice(&self.opacity.to_le_bytes());
        b[72..76].copy_from_slice(&self.colour[0].to_le_bytes());
        b[76..80].copy_from_slice(&self.colour[1].to_le_bytes());
        b[80..84].copy_from_slice(&self.colour[2].to_le_bytes());
        b[84..88].copy_from_slice(&self.z_ndc.to_le_bytes());
        b[88..92].copy_from_slice(&u32::from(self.culled).to_le_bytes());
        b
    }

    fn from_bytes(b: &[u8]) -> Self {
        let g64 = |off: usize| f64::from_le_bytes(b[off..off + 8].try_into().unwrap());
        let g32 = |off: usize| f32::from_le_bytes(b[off..off + 4].try_into().unwrap());
        OutRecord {
            centre_px: [g64(0), g64(8)],
            radii: [g64(16), g64(24)],
            axis1: [g64(32), g64(40)],
            depth: g64(48),
            conic: [g32(56), g32(60), g32(64)],
            opacity: g32(68),
            colour: [g32(72), g32(76), g32(80)],
            z_ndc: g32(84),
            culled: u32::from_le_bytes(b[88..92].try_into().unwrap()) != 0,
        }
    }
}

/// Names of the fields that differ between two surviving records,
/// compared bitwise (f64/f32 bit patterns, not values).
fn differing_fields(a: &OutRecord, b: &OutRecord) -> Vec<&'static str> {
    let mut out = Vec::new();
    let f64s = |name: &'static str, x: f64, y: f64, out: &mut Vec<&'static str>| {
        if x.to_bits() != y.to_bits() {
            out.push(name);
        }
    };
    let f32s = |name: &'static str, x: f32, y: f32, out: &mut Vec<&'static str>| {
        if x.to_bits() != y.to_bits() {
            out.push(name);
        }
    };
    f64s("centre_px[0]", a.centre_px[0], b.centre_px[0], &mut out);
    f64s("centre_px[1]", a.centre_px[1], b.centre_px[1], &mut out);
    f64s("radii[0]", a.radii[0], b.radii[0], &mut out);
    f64s("radii[1]", a.radii[1], b.radii[1], &mut out);
    f64s("axis1[0]", a.axis1[0], b.axis1[0], &mut out);
    f64s("axis1[1]", a.axis1[1], b.axis1[1], &mut out);
    f64s("depth", a.depth, b.depth, &mut out);
    f32s("conic[0]", a.conic[0], b.conic[0], &mut out);
    f32s("conic[1]", a.conic[1], b.conic[1], &mut out);
    f32s("conic[2]", a.conic[2], b.conic[2], &mut out);
    f32s("opacity", a.opacity, b.opacity, &mut out);
    f32s("colour[0]", a.colour[0], b.colour[0], &mut out);
    f32s("colour[1]", a.colour[1], b.colour[1], &mut out);
    f32s("colour[2]", a.colour[2], b.colour[2], &mut out);
    f32s("z_ndc", a.z_ndc, b.z_ndc, &mut out);
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(ply), Some(mojo_bin), Some(work)) = (
        arg_value(&args, "--ply"),
        arg_value(&args, "--mojo-bin"),
        arg_value(&args, "--work"),
    ) else {
        eprintln!(
            "usage: splat-mojo-bench --ply PATH --mojo-bin PATH --work DIR \
             [--reps N] [--kernel-reps N] [--device-id N] [--limit N] [--lit] [--shadow]"
        );
        return ExitCode::FAILURE;
    };
    let lit = args.iter().any(|a| a == "--lit");
    let shadow = args.iter().any(|a| a == "--shadow");
    let reps: usize = arg_value(&args, "--reps")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let kernel_reps: usize = arg_value(&args, "--kernel-reps")
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let device_id: usize = arg_value(&args, "--device-id")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let limit: Option<usize> = arg_value(&args, "--limit").and_then(|v| v.parse().ok());

    let bytes = std::fs::read(&ply).expect("read ply");
    println!("asset: {ply} ({} bytes)", bytes.len());
    let t_load = Instant::now();
    let mut cloud = SplatCloud::load_ply(&bytes).expect("load ply");
    println!(
        "strict load: {:.1} ms, {} splats",
        t_load.elapsed().as_secs_f64() * 1e3,
        cloud.splats.len()
    );
    if let Some(n) = limit {
        cloud.splats.truncate(n);
        println!("limited to first {} splats (--limit)", cloud.splats.len());
    }
    let count = cloud.splats.len();

    let (scene, eye) = plate_scene(&cloud);
    let transform = SplatTransform::default();
    assert!(
        torogao_render3d::splat_rotate::is_identity_quat(transform.rotation),
        "the Mojo prototype covers identity layer rotation only"
    );

    // The `--shadow` gate's map: two overhead caster
    // quads in the XZ plane above the cloud — a broad
    // plate covering the cloud's XZ bounds plus margin,
    // and a smaller plate offset to one side so PCF edge
    // texels land inside the cloud too — rasterised by
    // the crate's own CPU rasteriser under a light
    // view-projection fitted to the witness cloud
    // (`witness_light_view_proj`). The census below
    // reports what the rig actually covers.
    let shadow_map = if shadow {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for s in &cloud.splats {
            for i in 0..3 {
                lo[i] = lo[i].min(f64::from(s.pos[i]));
                hi[i] = hi[i].max(f64::from(s.pos[i]));
            }
        }
        let y = hi[1] + 0.5;
        let quad = |cx: f64, cz: f64, h: f64| -> [[f64; 3]; 6] {
            let a = [cx - h, y, cz - h];
            let b = [cx + h, y, cz - h];
            let c = [cx + h, y, cz + h];
            let d = [cx - h, y, cz + h];
            [a, b, c, a, c, d]
        };
        let cx = (lo[0] + hi[0]) / 2.0;
        let cz = (lo[2] + hi[2]) / 2.0;
        let half = (hi[0] - lo[0]).max(hi[2] - lo[2]) / 2.0 + 1.0;
        let mut casters: Vec<[f64; 3]> = Vec::new();
        casters.extend_from_slice(&quad(cx, cz, half));
        casters.extend_from_slice(&quad(cx + 0.4, cz - 0.3, half * 0.45));
        let tris: Vec<[[f64; 3]; 3]> = casters
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        let (centre, radius) = cloud_bounds(&cloud);
        let sun = scene.sun.to_light;
        let map = SplatShadowMap::rasterise(
            &tris,
            witness_light_view_proj([sun.x, sun.y, sun.z], centre, radius),
            SHADOW_SIZE,
        );
        // Visibility census over every splat centre
        // (identity layer transform: centre = pos).
        let (mut full, mut dark, mut partial) = (0usize, 0usize, 0usize);
        for s in &cloud.splats {
            let v = map.visibility([
                f64::from(s.pos[0]),
                f64::from(s.pos[1]),
                f64::from(s.pos[2]),
            ]);
            if v >= 1.0 {
                full += 1;
            } else if v <= 0.0 {
                dark += 1;
            } else {
                partial += 1;
            }
        }
        println!(
            "shadow mode: map {SHADOW_SIZE}x{SHADOW_SIZE} from {} caster triangles; \
             visibility census over {} centres: lit {full}, dark {dark}, partial {partial}",
            tris.len(),
            cloud.splats.len()
        );
        Some(map)
    } else {
        None
    };

    // The light both sides apply in `--lit` / `--shadow`
    // mode: the response against the scene's own world
    // colour, plus the map in shadow mode.
    let light = if lit || shadow {
        Some(SplatLight {
            shadow: shadow_map.as_ref(),
            response: Some(LightResponse {
                grade: LIT_GRADE,
                world_mix: LIT_WORLD_MIX,
            }),
        })
    } else {
        None
    };
    if lit || shadow {
        println!(
            "lit mode: grade {LIT_GRADE:?}, world_mix {LIT_WORLD_MIX}, world {:?}",
            scene.world
        );
    }

    // CPU reference: the projection half of the pack, timed.
    let mut cpu_times = Vec::new();
    let mut projected = Vec::new();
    for _ in 0..reps {
        let t = Instant::now();
        projected = project_splats(&cloud, &transform, &scene, WIDTH, HEIGHT, light);
        cpu_times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let cpu_median = median(cpu_times.clone());
    println!(
        "cpu project_splats{}: median {:.1} ms over {} reps ({} survivors)",
        if shadow {
            " (shadow)"
        } else if lit {
            " (lit)"
        } else {
            ""
        },
        cpu_median,
        reps,
        projected.len()
    );
    // Sort share of the CPU number, measured on the same output:
    // re-sort a reversed copy with the pack's comparator.
    let mut resort = projected.clone();
    resort.reverse();
    let t = Instant::now();
    resort.sort_by(|a, b| {
        b.depth
            .total_cmp(&a.depth)
            .then_with(|| a.index.cmp(&b.index))
    });
    let resort_ms = t.elapsed().as_secs_f64() * 1e3;
    println!("cpu re-sort of the same output: {resort_ms:.1} ms (sort share bound)");

    // Expected per-index records.
    let mut expected: Vec<OutRecord> = (0..count)
        .map(|_| OutRecord {
            culled: true,
            ..OutRecord::default()
        })
        .collect();
    for p in &projected {
        expected[p.index] = OutRecord::from_projection(p);
    }

    // Kernel inputs: header + the crate's resident records.
    let view_proj = projection(&scene).multiply(scene.view);
    let mut uniforms = [0.0_f64; 78];
    uniforms[0..16].copy_from_slice(&scene.view.m);
    uniforms[16..32].copy_from_slice(&view_proj.m);
    uniforms[32..35].copy_from_slice(&eye);
    uniforms[35..38].copy_from_slice(&transform.translation);
    uniforms[38] = transform.scale;
    uniforms[39] = scene.tan_half_x;
    uniforms[40] = scene.tan_half_y;
    uniforms[41] = scene.near;
    uniforms[42] = f64::from(WIDTH);
    uniforms[43] = f64::from(HEIGHT);
    // The response block, staged exactly as the route
    // stages it (splat_mojo.rs): flag, grade, mix, world.
    if let Some(light) = &light {
        let response = light.response.expect("lit mode names a response");
        uniforms[44] = 1.0;
        for i in 0..3 {
            for j in 0..3 {
                uniforms[45 + i * 3 + j] = response.grade[i][j];
            }
        }
        uniforms[54] = response.world_mix;
        for i in 0..3 {
            uniforms[55 + i] = f64::from(scene.world[i]);
        }
    }
    // The shadow block, staged exactly as the route
    // stages it: flag, map edge, the map's own light
    // view-projection; the depths ride as a trailer
    // after the records (below).
    let shadow_trailer: &[u8] = if let Some(map) = &shadow_map {
        uniforms[58] = 1.0;
        uniforms[59] = f64::from(map.size());
        uniforms[60..76].copy_from_slice(&map.light_view_proj().m);
        bytemuck::cast_slice(map.depths())
    } else {
        &[]
    };

    let records = frame_records(&cloud);
    let record_bytes: &[u8] = bytemuck::cast_slice(&records);
    assert_eq!(record_bytes.len(), count * 256);

    let workdir = std::path::Path::new(&work);
    std::fs::create_dir_all(workdir).expect("create work dir");
    let inputs_path = workdir.join("inputs.bin");
    {
        let mut f = std::fs::File::create(&inputs_path).expect("create inputs.bin");
        let mut header = vec![0u8; HEADER_BYTES];
        header[0..8].copy_from_slice(&(count as u64).to_le_bytes());
        for (k, v) in uniforms.iter().enumerate() {
            header[8 + k * 8..16 + k * 8].copy_from_slice(&v.to_le_bytes());
        }
        header[632..636].copy_from_slice(&ALPHA_CULL.to_bits().to_le_bytes());
        f.write_all(&header).expect("write header");
        f.write_all(record_bytes).expect("write records");
        f.write_all(shadow_trailer).expect("write shadow trailer");
    }
    let expected_path = workdir.join("expected.bin");
    {
        let mut f = std::fs::File::create(&expected_path).expect("create expected.bin");
        for r in &expected {
            f.write_all(&r.to_bytes()).expect("write expected");
        }
    }
    println!(
        "inputs: {} bytes ({} records, {} shadow trailer bytes), expected written",
        HEADER_BYTES + record_bytes.len() + shadow_trailer.len(),
        count,
        shadow_trailer.len()
    );

    // Run the Mojo kernel program.
    let kernel_out_path = workdir.join("kernel-out.bin");
    let t_wall = Instant::now();
    let output = Command::new(&mojo_bin)
        .arg(&inputs_path)
        .arg(&kernel_out_path)
        .arg(kernel_reps.to_string())
        .arg(device_id.to_string())
        .output()
        .expect("run mojo kernel program");
    let wall_ms = t_wall.elapsed().as_secs_f64() * 1e3;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        eprintln!("mojo kernel program failed: {}", output.status);
        return ExitCode::FAILURE;
    }
    println!("mojo process wall (incl. runtime start): {wall_ms:.1} ms");

    // The gate: record-for-record, bitwise, per asset index.
    let kernel_bytes = std::fs::read(&kernel_out_path).expect("read kernel output");
    assert_eq!(kernel_bytes.len(), count * OUT_BYTES);
    let mut cull_mismatch = 0usize;
    let mut field_mismatch_records = 0usize;
    let mut field_counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    let mut first: Option<(usize, Vec<&'static str>)> = None;
    let mut worst_f64_rel = 0.0_f64;
    for i in 0..count {
        let got = OutRecord::from_bytes(&kernel_bytes[i * OUT_BYTES..(i + 1) * OUT_BYTES]);
        let want = &expected[i];
        if got.culled != want.culled {
            cull_mismatch += 1;
            if first.is_none() {
                first = Some((i, vec!["<cull-state>"]));
            }
            continue;
        }
        if want.culled {
            continue;
        }
        let diffs = differing_fields(want, &got);
        if !diffs.is_empty() {
            field_mismatch_records += 1;
            for d in &diffs {
                *field_counts.entry(d).or_insert(0) += 1;
            }
            if first.is_none() {
                first = Some((i, diffs));
            }
            // Magnitude context for the record (not the verdict):
            // worst relative f64 deviation among the depth field.
            if want.depth != 0.0 {
                let rel = ((got.depth - want.depth) / want.depth).abs();
                worst_f64_rel = worst_f64_rel.max(rel);
            }
        }
    }
    println!(
        "gate: {} splats, {} survivors; cull mismatches {}, records with field mismatches {}",
        count,
        projected.len(),
        cull_mismatch,
        field_mismatch_records
    );
    for (field, n) in &field_counts {
        println!("gate: field {field}: {n} records differ");
    }
    if let Some((i, diffs)) = &first {
        println!("gate: first mismatch at index {i}: {diffs:?}");
    }
    if worst_f64_rel > 0.0 {
        println!("gate: worst relative depth deviation {worst_f64_rel:.3e}");
    }
    if cull_mismatch == 0 && field_mismatch_records == 0 {
        println!("GATE: BIT-EXACT — all surviving records identical, cull state identical");
        ExitCode::SUCCESS
    } else {
        println!("GATE: NOT BIT-EXACT");
        ExitCode::FAILURE
    }
}
