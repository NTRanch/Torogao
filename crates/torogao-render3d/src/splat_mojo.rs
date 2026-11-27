//! The Mojo desktop projection route (adopted by
//! Owner ruling, decision walk 2026-11-27): the
//! projection half of the splat pack, computed by the
//! Mojo GPU kernel in `mojo/splat_project.mojo`
//! instead of the CPU loop, on desktop machines where
//! a kernel binary has been built and configured.
//!
//! The contract is the estate's bit-exactness law
//! (ACCELERATION-DESIGN §3): the kernel's output is
//! byte-identical to the CPU reference
//! ([`crate::splat::project_splats`]) on the full
//! 1,157,141-splat witness, proven before adoption
//! (prototype record:
//! `~/workspace/torogao-mojo-ptx-20261127/RECORD.md`)
//! and re-proven against this integrated build by the
//! route gates in `tests/splat_mojo_route.rs`. Because
//! the two paths produce the same packet, the route
//! can fall back to the CPU reference at any failure —
//! missing binary, spawn error, short output — and the
//! result cannot change. The fallback is therefore
//! total and silent by design: configuring the route
//! is an optimisation, never a behaviour change.
//!
//! Eligibility, exactly:
//! - The layer's light must be inactive. Lit layers
//!   (Rung 4 shadow receive / light response) pack on
//!   the CPU; the kernel transcribes the unlit
//!   projection only.
//! - Layer rotation is fine: the pack's own host-side
//!   rotation ([`crate::splat::rotate_cloud`]) runs
//!   first, exactly as the CPU path runs it, and the
//!   kernel projects the rotated cloud. (A rotation
//!   the CPU path would reject — zero or non-finite —
//!   makes this route decline, and the CPU path then
//!   raises its usual panic.)
//! - Desktop only. Mojo 1.2.0's stdlib refuses
//!   Android, so mobile builds never see this module's
//!   route: the Rust CPU path is the only mobile
//!   projection path, and the Vulkan GPU path remains
//!   the runtime path everywhere. This route serves
//!   the compile-time pre-pack (frozen-sequence
//!   packing through `crate::splat_spec`), which is
//!   where the CPU projection cost is paid.
//!
//! Configuration is explicit, through the
//! environment: `TOROGAO_MOJO_PROJECT_BIN` names the
//! per-architecture kernel binary (built by
//! `mojo/build.sh`); without it there is no route.
//! `TOROGAO_MOJO_DEVICE_ID` selects the GPU (default
//! 0; on primo, 0 = RTX 4070 Laptop, 1 = RTX A6000 as
//! the MAX runtime enumerates them).
//! `TOROGAO_MOJO_WORK_DIR` overrides the scratch
//! directory for the kernel's file protocol (default:
//! a `torogao-mojo-route` directory under the system
//! temp dir).
//!
//! Licence: the installed Mojo toolchain declares
//! `LicenseRef-MAX-Platform-Software-License` (not
//! plain Apache). Adoption covers internal/desktop
//! use of the compiler; compiled kernel binaries are
//! local build outputs (gitignored, rebuilt by
//! `mojo/build.sh`) and are not shipped. Redistributing
//! a compiled Mojo binary inside a product needs a
//! fresh check of the MAX Platform terms first — see
//! the dated note in `mojo/README.md`.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::setscene::{ArcScene, projection};
use crate::splat::{
    PackedSplats, SplatCloud, SplatProjection, SplatTransform, pack_splats_lit, rotate_cloud,
};
use crate::splat_gpu::frame_records;
use crate::splat_light::SplatLight;
use crate::splat_rotate::{Rotation, is_identity_quat};

/// The pack's alpha cull (`splat.rs`'s `ALPHA_CULL`,
/// f32): the kernel header carries its bit pattern.
const ALPHA_CULL: f32 = 1.0 / 255.0;
/// Header bytes before the record stream (the kernel
/// source's contract).
const HEADER_BYTES: usize = 368;
/// One resident record's bytes (`GpuSplatRecord`).
const RECORD_BYTES: usize = 256;
/// One projected output record's bytes.
const OUT_BYTES: usize = 96;

/// A configured Mojo projection route: where the
/// kernel binary lives, which GPU it runs on, and
/// where its file protocol is staged.
#[derive(Debug, Clone, PartialEq)]
pub struct MojoRoute {
    /// The compiled `splat_project` kernel binary for
    /// this machine's GPU architecture (see
    /// `mojo/build.sh`).
    pub project_bin: PathBuf,
    /// GPU index as the MAX runtime enumerates
    /// devices.
    pub device_id: usize,
    /// Scratch root: each call stages its files in a
    /// unique subdirectory and removes it afterwards.
    pub work_dir: PathBuf,
}

impl MojoRoute {
    /// The route the environment configures, if any.
    /// No `TOROGAO_MOJO_PROJECT_BIN` — or a device id
    /// that does not parse — means no route, and every
    /// pack takes the CPU reference path exactly as
    /// before this module existed.
    pub fn from_env() -> Option<MojoRoute> {
        let project_bin = std::env::var_os("TOROGAO_MOJO_PROJECT_BIN")?;
        let device_id = match std::env::var("TOROGAO_MOJO_DEVICE_ID") {
            Ok(v) => v.parse::<usize>().ok()?,
            Err(_) => 0,
        };
        let work_dir = std::env::var_os("TOROGAO_MOJO_WORK_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("torogao-mojo-route"));
        Some(MojoRoute {
            project_bin: PathBuf::from(project_bin),
            device_id,
            work_dir,
        })
    }
}

/// Project a cloud through the Mojo kernel: the
/// kernel's records converted back to
/// [`SplatProjection`]s and sorted into the pack's
/// total order. `None` whenever the kernel did not
/// produce a complete output (spawn failure, non-zero
/// exit, wrong output size) — the caller then uses
/// the CPU reference, whose result is identical by
/// the bit-exactness gate.
///
/// The layer must be unlit; this function has no
/// light parameter because the kernel transcribes
/// the unlit projection only (see the module docs).
pub fn project_splats_mojo(
    cloud: &SplatCloud,
    transform: &SplatTransform,
    scene: &ArcScene,
    width: u32,
    height: u32,
    route: &MojoRoute,
) -> Option<Vec<SplatProjection>> {
    // Rotation first, on the host, by the pack's own
    // function — the same rotated cloud the CPU path
    // projects. The identity skips it untouched.
    let rotated;
    let cloud = if is_identity_quat(transform.rotation) {
        cloud
    } else {
        let rotation = Rotation::from_quaternion(transform.rotation)?;
        rotated = rotate_cloud(cloud, &rotation);
        &rotated
    };
    let count = cloud.splats.len();

    // Uniforms, in the kernel contract's order (the
    // bench harness writes the identical block).
    let view_proj = projection(scene).multiply(scene.view);
    let camera_world = scene.view.inverse();
    let cam = camera_world.translation();
    let mut uniforms = [0.0_f64; 44];
    uniforms[0..16].copy_from_slice(&scene.view.m);
    uniforms[16..32].copy_from_slice(&view_proj.m);
    uniforms[32..35].copy_from_slice(&[cam.x, cam.y, cam.z]);
    uniforms[35..38].copy_from_slice(&transform.translation);
    uniforms[38] = transform.scale;
    uniforms[39] = scene.tan_half_x;
    uniforms[40] = scene.tan_half_y;
    uniforms[41] = scene.near;
    uniforms[42] = f64::from(width);
    uniforms[43] = f64::from(height);

    let records = frame_records(cloud);
    let record_bytes: &[u8] = bytemuck::cast_slice(&records);
    debug_assert_eq!(record_bytes.len(), count * RECORD_BYTES);

    // Stage the file protocol in a per-call directory.
    static CALL_COUNTER: AtomicU64 = AtomicU64::new(0);
    let call_dir = route.work_dir.join(format!(
        "call-{}-{}",
        std::process::id(),
        CALL_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let staged = stage_and_run(route, &call_dir, count, &uniforms, record_bytes);
    let _ = std::fs::remove_dir_all(&call_dir);
    let kernel_bytes = staged?;

    if kernel_bytes.len() != count * OUT_BYTES {
        return None;
    }
    let mut projected: Vec<SplatProjection> = Vec::new();
    for (index, rec) in kernel_bytes.chunks_exact(OUT_BYTES).enumerate() {
        let g64 = |off: usize| f64::from_le_bytes(rec[off..off + 8].try_into().unwrap());
        let g32 = |off: usize| f32::from_le_bytes(rec[off..off + 4].try_into().unwrap());
        if u32::from_le_bytes(rec[88..92].try_into().unwrap()) != 0 {
            continue; // culled: the remaining fields carry no meaning
        }
        let axis1 = [g64(32), g64(40)];
        projected.push(SplatProjection {
            depth: g64(48),
            index,
            centre_px: [g64(0), g64(8)],
            z_ndc: g32(84),
            conic: [g32(56), g32(60), g32(64)],
            radii: [g64(16), g64(24)],
            axes: [axis1, [-axis1[1], axis1[0]]],
            colour: [g32(72), g32(76), g32(80)],
            opacity: g32(68),
        });
    }
    // The pack's total order: depth descending, asset
    // index ascending — the same comparator the CPU
    // path sorts by.
    projected.sort_by(|a, b| {
        b.depth
            .total_cmp(&a.depth)
            .then_with(|| a.index.cmp(&b.index))
    });
    Some(projected)
}

/// Write the kernel's input file under `call_dir`,
/// run the kernel program once, and return its output
/// file's bytes. Any failure is `None` (the caller
/// falls back to the CPU reference).
fn stage_and_run(
    route: &MojoRoute,
    call_dir: &std::path::Path,
    count: usize,
    uniforms: &[f64; 44],
    record_bytes: &[u8],
) -> Option<Vec<u8>> {
    std::fs::create_dir_all(call_dir).ok()?;
    let inputs_path = call_dir.join("inputs.bin");
    {
        let mut f = std::fs::File::create(&inputs_path).ok()?;
        let mut header = vec![0u8; HEADER_BYTES];
        header[0..8].copy_from_slice(&(count as u64).to_le_bytes());
        for (k, v) in uniforms.iter().enumerate() {
            header[8 + k * 8..16 + k * 8].copy_from_slice(&v.to_le_bytes());
        }
        header[360..364].copy_from_slice(&ALPHA_CULL.to_bits().to_le_bytes());
        f.write_all(&header).ok()?;
        f.write_all(record_bytes).ok()?;
    }
    let out_path = call_dir.join("kernel-out.bin");
    let output = Command::new(&route.project_bin)
        .arg(&inputs_path)
        .arg(&out_path)
        .arg("1")
        .arg(route.device_id.to_string())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    std::fs::read(&out_path).ok()
}

/// Pack a splat layer, taking the Mojo desktop route
/// when one is configured and the layer is eligible
/// (unlit), and the CPU reference otherwise — the
/// packet is identical either way, by the route's
/// bit-exactness gate. This is the pack entry the
/// spec compiler (`crate::splat_spec`) uses, so a
/// configured route serves every desktop pre-pack.
pub fn pack_splats_mojo(
    cloud: &SplatCloud,
    transform: &SplatTransform,
    scene: &ArcScene,
    width: u32,
    height: u32,
    light: &SplatLight<'_>,
    route: Option<&MojoRoute>,
) -> PackedSplats {
    if let Some(route) = route {
        if !light.is_active() {
            if let Some(projected) =
                project_splats_mojo(cloud, transform, scene, width, height, route)
            {
                return crate::splat::emit_quads(&projected, width, height);
            }
        }
    }
    pack_splats_lit(cloud, transform, scene, width, height, light)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setscene::plate_rig_scene;
    use crate::splat::{pack_splats, room_corner_cloud};
    use crate::splat_light::LightResponse;

    /// The splat proving tests' framing (eye
    /// (2.6, 1.9, -3.4) toward (0, 0.9, 0)), which
    /// frames the room-corner cloud.
    fn test_scene() -> ArcScene {
        let eye = [2.6, 1.9, -3.4];
        let target = [0.0, 0.9, 0.0];
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let norm = |a: [f64; 3]| {
            let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            [a[0] / l, a[1] / l, a[2] / l]
        };
        let forward = norm(sub(target, eye));
        let right = norm(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        let back = [-forward[0], -forward[1], -forward[2]];
        plate_rig_scene(crate::math::Mat4::from_rows([
            [right[0], up[0], back[0], eye[0]],
            [right[1], up[1], back[1], eye[1]],
            [right[2], up[2], back[2], eye[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]))
    }

    /// The gated invariant: `--fp-mode contract=off`
    /// is pinned on every kernel build line in
    /// `mojo/build.sh`, and no build line names any
    /// other fp-mode. The default `contract=fast` is a
    /// measured REJECT (it fuses multiply-adds into
    /// FMAs and the projection differs from the CPU
    /// reference in the last bit — 13,204 of 20,000
    /// debug records, worst relative depth deviation
    /// 2.217e-16; prototype record §4). If this test
    /// fails, the flag moved: put it back, do not
    /// adjust the test.
    #[test]
    fn build_script_pins_contract_off() {
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("mojo")
                .join("build.sh"),
        )
        .expect("mojo/build.sh exists");
        let mut pinned = 0usize;
        for line in script.lines() {
            let code = line.split('#').next().unwrap_or("");
            if code.contains("--fp-mode") {
                assert!(
                    code.contains("--fp-mode contract=off"),
                    "build line names an fp-mode other than contract=off: {line}"
                );
                pinned += 1;
            }
        }
        assert!(
            pinned >= 2,
            "expected the flag pinned on both architecture build lines, found {pinned}"
        );
    }

    /// A configured route whose binary does not exist
    /// falls back to the CPU reference, packet-equal.
    #[test]
    fn missing_binary_falls_back_to_cpu() {
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let route = MojoRoute {
            project_bin: PathBuf::from("/nonexistent/splat_project"),
            device_id: 0,
            work_dir: std::env::temp_dir().join("torogao-mojo-route-test"),
        };
        let light = SplatLight::default();
        let via_route = pack_splats_mojo(
            &cloud,
            &SplatTransform::default(),
            &scene,
            512,
            640,
            &light,
            Some(&route),
        );
        let cpu = pack_splats(&cloud, &SplatTransform::default(), &scene, 512, 640);
        assert_eq!(via_route, cpu);
    }

    /// A lit layer never invokes the kernel binary:
    /// proven with a stand-in "binary" that records
    /// its invocation in a marker file. The unlit
    /// control case does invoke it (and then falls
    /// back, because the stand-in writes no output).
    #[cfg(unix)]
    #[test]
    fn lit_layer_never_invokes_the_kernel() {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("torogao-mojo-route-lit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let marker = dir.join("invoked");
        let fake_bin = dir.join("fake-kernel.sh");
        std::fs::write(
            &fake_bin,
            format!("#!/bin/sh\ntouch \"{}\"\nexit 0\n", marker.display()),
        )
        .expect("write fake kernel");
        std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake kernel");
        let route = MojoRoute {
            project_bin: fake_bin,
            device_id: 0,
            work_dir: dir.join("work"),
        };
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let transform = SplatTransform::default();

        // Unlit control: the route tries the kernel.
        let unlit = SplatLight::default();
        let _ = pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &unlit, Some(&route));
        assert!(marker.exists(), "unlit pack must invoke the kernel binary");
        std::fs::remove_file(&marker).expect("clear marker");

        // Lit: the route must not touch the binary.
        let lit = SplatLight {
            shadow: None,
            response: Some(LightResponse {
                grade: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                world_mix: 0.25,
            }),
        };
        let via_route = pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &lit, Some(&route));
        assert!(
            !marker.exists(),
            "lit pack must not invoke the kernel binary"
        );
        let cpu = pack_splats_lit(&cloud, &transform, &scene, 512, 640, &lit);
        assert_eq!(via_route, cpu);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
