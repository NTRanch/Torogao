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
//! - The layer's light may carry a shadow map whose
//!   edge is at most [`SHADOW_MAX_EDGE`] texels: the
//!   rasterised map travels as a device input (a
//!   trailer after the records) and the projection
//!   kernel applies the centre visibility test, as it
//!   already applies the Rung 4 light RESPONSE
//!   (grade + world tint). A map with a larger edge
//!   declines the route by name — the trailer at the
//!   cap is already 64 MiB — and the layer packs on
//!   the CPU exactly as before the shadow was
//!   transcribed. The triangle rasteriser itself
//!   stays host-side: it runs once per scene at
//!   spec-compile time, caller-owned, not per layer.
//! - Layer rotation is fine, and has its own kernel:
//!   when a rotation binary is configured (see
//!   below), the Mojo rotation kernel
//!   (`mojo/splat_rotate.mojo`, adopted by the
//!   extension trial — bit-exact against the host
//!   step on the full witness) rotates the cloud's
//!   frame records on the GPU ahead of projection.
//!   With no rotation binary configured, or on any
//!   rotation-kernel failure, the pack's own host-side
//!   rotation ([`crate::splat::rotate_cloud`]) runs
//!   instead, exactly as the CPU path runs it — the
//!   fallback cannot change the packet, which is what
//!   the trial's bit-exactness gate buys. (A rotation
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
//! per-architecture projection kernel binary (built
//! by `mojo/build.sh`); without it there is no route.
//! `TOROGAO_MOJO_ROTATE_BIN` optionally names the
//! rotation kernel binary; without it, layer
//! rotations run on the host exactly as before the
//! rotation kernel was wired in.
//! `TOROGAO_MOJO_DEVICE_ID` selects the GPU (default
//! 0; on primo, 0 = RTX 4070 Laptop, 1 = RTX A6000 as
//! the MAX runtime enumerates them).
//! `TOROGAO_MOJO_WORK_DIR` overrides the scratch
//! directory for the kernel's file protocol (default:
//! a `torogao-mojo-route` directory under the system
//! temp dir).
//!
//! The persistent worker (`TOROGAO_MOJO_WORKER_BIN`,
//! `mojo/splat_worker.mojo`): the one-shot programs
//! pay the MAX runtime start + device-context
//! creation on every invocation — measured at
//! ~2.1–3.2 s of process wall against 5–180 ms of
//! transfers + kernel on the full witness, and the
//! rotated route paid it twice. When a worker binary
//! is configured, the route first asks a long-lived
//! worker process (spawned lazily, one per host
//! process) to serve the pack over a pipe protocol:
//! the worker creates its device context once and
//! runs the same kernel functions the one-shot
//! programs run (imported, not transcribed), and for
//! a rotated layer it runs rotation and projection
//! back to back with the records staying in a device
//! buffer between them. The worker is an optimisation
//! in exactly the route's existing sense: any failure
//! — spawn, death mid-request, a rejected request, a
//! hang past the request timeout — falls back to the
//! per-invocation path below, and from there to the
//! CPU reference, packet-identical throughout. With
//! no worker configured, behaviour is exactly the
//! pre-worker route. The worker exits when its stdin
//! closes, so it dies with the host process; a worker
//! that dies is dropped and (on a later request)
//! respawned, and after three consecutive failures
//! the worker is disabled for the process lifetime.
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

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::setscene::{ArcScene, projection};
use crate::splat::{
    PackedSplats, SplatCloud, SplatProjection, SplatTransform, pack_splats_lit, rotate_cloud,
};
use crate::splat_gpu::frame_records;
use crate::splat_light::SplatLight;
use crate::splat_rotate::{Rotation, ShRotation, is_identity_quat};

/// The pack's alpha cull (`splat.rs`'s `ALPHA_CULL`,
/// f32): the kernel header carries its bit pattern.
const ALPHA_CULL: f32 = 1.0 / 255.0;
/// Header bytes before the record stream (the kernel
/// source's contract).
const HEADER_BYTES: usize = 640;
/// The largest shadow map edge (texels) the route
/// carries as a device input: the production map is
/// 2048 (`crate::splat_light::SHADOW_MAP_SIZE`), so
/// the cap leaves 2x headroom and the trailer at the
/// cap is 64 MiB of f32 depths. A larger map declines
/// the route by name (see the module docs) and the
/// kernel rejects it too — the two checks must agree.
pub const SHADOW_MAX_EDGE: u32 = 4096;
/// One resident record's bytes (`GpuSplatRecord`).
const RECORD_BYTES: usize = 256;
/// One projected output record's bytes.
const OUT_BYTES: usize = 96;
/// Header bytes before the record stream in the
/// rotation kernel's file protocol (count, matrix,
/// quaternion, SH band block — the kernel source's
/// contract, written identically by the trial bench).
const ROTATE_HEADER_BYTES: usize = 776;

/// A configured Mojo projection route: where the
/// kernel binary lives, which GPU it runs on, and
/// where its file protocol is staged.
#[derive(Debug, Clone, PartialEq)]
pub struct MojoRoute {
    /// The compiled `splat_project` kernel binary for
    /// this machine's GPU architecture (see
    /// `mojo/build.sh`).
    pub project_bin: PathBuf,
    /// The compiled `splat_rotate` kernel binary, when
    /// one is configured: layer rotations then run on
    /// the GPU ahead of projection. `None` (or any
    /// failure of the binary) leaves rotation on the
    /// host, exactly as the CPU path performs it.
    pub rotate_bin: Option<PathBuf>,
    /// The compiled `splat_worker` persistent-worker
    /// binary, when one is configured: packs are then
    /// first offered to a long-lived worker process
    /// that keeps its device context across requests
    /// (see the module docs). `None` (or any worker
    /// failure) leaves the per-invocation path in
    /// charge, exactly as before the worker existed.
    pub worker_bin: Option<PathBuf>,
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
            rotate_bin: std::env::var_os("TOROGAO_MOJO_ROTATE_BIN").map(PathBuf::from),
            worker_bin: std::env::var_os("TOROGAO_MOJO_WORKER_BIN").map(PathBuf::from),
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
/// The layer's light may carry a shadow map within
/// the route's edge cap: the kernel transcribes the
/// projection plus both light interactions (see the
/// module docs), and the caller declines only
/// over-cap maps before reaching here. A `light`
/// with no response and no shadow stages both flags
/// clear and the kernel skips the hooks whole,
/// exactly as the CPU pack does.
pub fn project_splats_mojo(
    cloud: &SplatCloud,
    transform: &SplatTransform,
    scene: &ArcScene,
    width: u32,
    height: u32,
    light: &SplatLight<'_>,
    route: &MojoRoute,
) -> Option<Vec<SplatProjection>> {
    let count = cloud.splats.len();

    // Uniforms, in the kernel contract's order (the
    // bench harness writes the identical block).
    let view_proj = projection(scene).multiply(scene.view);
    let camera_world = scene.view.inverse();
    let cam = camera_world.translation();
    let mut uniforms = [0.0_f64; 78];
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
    // The Rung 4 light response block (uniforms
    // 44..58): flag, grade rows, world mix, and the
    // scene's world colour widened from f32 — the
    // exact values `LightResponse::apply` consumes
    // on the CPU path. No response: the flag stays
    // 0.0 and the rest of the block stays zeroed.
    if let Some(response) = &light.response {
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
    // The Rung 4 shadow block (uniforms 58..76): flag,
    // map edge, and the map's own light view-projection
    // — the exact values `SplatShadowMap::visibility`
    // consumes on the CPU path. The rasterised depths
    // themselves travel as a trailer after the records
    // (the kernel source's contract). No shadow: the
    // flag stays 0.0 and the trailer is empty.
    let shadow_depths: &[f32] = match light.shadow {
        Some(map) => {
            uniforms[58] = 1.0;
            uniforms[59] = f64::from(map.size());
            uniforms[60..76].copy_from_slice(&map.light_view_proj().m);
            map.depths()
        }
        None => &[],
    };
    let shadow_bytes: &[u8] = bytemuck::cast_slice(shadow_depths);

    // The persistent worker, when one is configured,
    // is offered the pack first (see the module docs):
    // it runs the same kernels without the per-call
    // process cost. Any failure falls through to the
    // per-invocation path below, unchanged.
    if route.worker_bin.is_some() {
        if let Some(projected) =
            project_via_worker(cloud, transform, &uniforms, shadow_bytes, route)
        {
            return Some(projected);
        }
    }

    // Rotation first. With a rotation binary
    // configured, the adopted Mojo rotation kernel
    // rotates the cloud's frame records on the GPU
    // and its output feeds the projection kernel
    // directly. Any failure — or no rotation binary —
    // falls back to the pack's own host rotation,
    // exactly the CPU path's step, so the fallback
    // cannot change the packet (the rotation kernel
    // is bit-exact against `frame_records` of the
    // host-rotated cloud by its trial gate). The
    // identity skips rotation untouched.
    let host_records;
    let kernel_records;
    let record_bytes: &[u8] = if is_identity_quat(transform.rotation) {
        host_records = frame_records(cloud);
        bytemuck::cast_slice(&host_records)
    } else {
        let rotation = Rotation::from_quaternion(transform.rotation)?;
        match route
            .rotate_bin
            .as_deref()
            .and_then(|bin| rotate_records_mojo(route, bin, cloud, &rotation))
        {
            Some(bytes) => {
                kernel_records = bytes;
                &kernel_records
            }
            None => {
                let rotated = rotate_cloud(cloud, &rotation);
                host_records = frame_records(&rotated);
                bytemuck::cast_slice(&host_records)
            }
        }
    };

    debug_assert_eq!(record_bytes.len(), count * RECORD_BYTES);

    // Stage the file protocol in a per-call directory.
    static CALL_COUNTER: AtomicU64 = AtomicU64::new(0);
    let call_dir = route.work_dir.join(format!(
        "call-{}-{}",
        std::process::id(),
        CALL_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let staged = stage_and_run(
        route,
        &call_dir,
        count,
        &uniforms,
        record_bytes,
        shadow_bytes,
    );
    let _ = std::fs::remove_dir_all(&call_dir);
    let kernel_bytes = staged?;

    parse_projected(&kernel_bytes, count)
}

/// The projection kernel's output bytes converted
/// back to [`SplatProjection`]s and sorted into the
/// pack's total order. `None` on a wrong output size.
/// Shared by the per-invocation path and the worker
/// path: the bytes are the same vessel either way.
fn parse_projected(kernel_bytes: &[u8], count: usize) -> Option<Vec<SplatProjection>> {
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

/// The projection kernel's input header (the kernel
/// source's contract): the splat count, the 78
/// uniforms, and the alpha-cull bit pattern, in a
/// zeroed 640-byte block. Written identically by the
/// per-invocation file protocol and the worker
/// protocol.
fn project_input_header(count: usize, uniforms: &[f64; 78]) -> [u8; HEADER_BYTES] {
    let mut header = [0u8; HEADER_BYTES];
    header[0..8].copy_from_slice(&(count as u64).to_le_bytes());
    for (k, v) in uniforms.iter().enumerate() {
        header[8 + k * 8..16 + k * 8].copy_from_slice(&v.to_le_bytes());
    }
    header[632..636].copy_from_slice(&ALPHA_CULL.to_bits().to_le_bytes());
    header
}

/// The rotation kernel's input header (the kernel
/// source's contract): the splat count, the rotation
/// matrix, the quaternion, and the SH band-rotation
/// block, 776 bytes. Written identically by the
/// per-invocation file protocol and the worker
/// protocol.
fn rotate_input_header(count: usize, rotation: &Rotation) -> Vec<u8> {
    let mut header = Vec::with_capacity(ROTATE_HEADER_BYTES);
    header.extend_from_slice(&(count as u64).to_le_bytes());
    for row in &rotation.matrix {
        for v in row {
            header.extend_from_slice(&v.to_le_bytes());
        }
    }
    for v in &rotation.quat {
        header.extend_from_slice(&v.to_le_bytes());
    }
    let flat = ShRotation::new(&rotation.matrix).matrices_flat();
    for v in &flat {
        header.extend_from_slice(&v.to_le_bytes());
    }
    debug_assert_eq!(header.len(), ROTATE_HEADER_BYTES);
    header
}

/// Offer one pack to the persistent worker: the
/// identity layer as a projection request, a rotated
/// layer as a fused rotate+project request (the
/// worker rotates device-side and projects from the
/// same buffer). `None` on any failure — the caller
/// proceeds down the per-invocation path, whose
/// result is identical by the bit-exactness gates. A
/// rotation the CPU path would reject (zero or
/// non-finite) declines here exactly as there: the
/// per-invocation path then raises its usual panic.
fn project_via_worker(
    cloud: &SplatCloud,
    transform: &SplatTransform,
    uniforms: &[f64; 78],
    shadow_bytes: &[u8],
    route: &MojoRoute,
) -> Option<Vec<SplatProjection>> {
    let count = cloud.splats.len();
    let records = frame_records(cloud);
    let record_bytes: &[u8] = bytemuck::cast_slice(&records);
    debug_assert_eq!(record_bytes.len(), count * RECORD_BYTES);
    let header = project_input_header(count, uniforms);
    let (kind, payload) = if is_identity_quat(transform.rotation) {
        let mut payload =
            Vec::with_capacity(HEADER_BYTES + record_bytes.len() + shadow_bytes.len());
        payload.extend_from_slice(&header);
        payload.extend_from_slice(record_bytes);
        payload.extend_from_slice(shadow_bytes);
        (WORKER_KIND_PROJECT, payload)
    } else {
        let rotation = Rotation::from_quaternion(transform.rotation)?;
        let mut payload = Vec::with_capacity(
            ROTATE_HEADER_BYTES + HEADER_BYTES + record_bytes.len() + shadow_bytes.len(),
        );
        payload.extend_from_slice(&rotate_input_header(count, &rotation));
        payload.extend_from_slice(&header);
        payload.extend_from_slice(record_bytes);
        payload.extend_from_slice(shadow_bytes);
        (WORKER_KIND_ROTATE_PROJECT, payload)
    };
    let bytes = worker_request(route, kind, payload, count * OUT_BYTES)?;
    parse_projected(&bytes, count)
}

/// Rotate a cloud's frame records through the Mojo
/// rotation kernel: returns the rotated records'
/// bytes (`count * RECORD_BYTES`), or `None` on any
/// failure (spawn failure, non-zero exit, wrong
/// output size) — the caller then rotates on the
/// host, exactly the CPU path's own step. The kernel
/// is bit-exact against `frame_records` of the
/// host-rotated cloud (extension-trial gate), so
/// taking this path cannot change the packet.
fn rotate_records_mojo(
    route: &MojoRoute,
    rotate_bin: &std::path::Path,
    cloud: &SplatCloud,
    rotation: &Rotation,
) -> Option<Vec<u8>> {
    let count = cloud.splats.len();
    let in_records = frame_records(cloud);
    let in_bytes: &[u8] = bytemuck::cast_slice(&in_records);
    debug_assert_eq!(in_bytes.len(), count * RECORD_BYTES);

    // Header, in the rotation kernel's contract
    // order (the trial bench writes the identical
    // block), then the records.
    let mut input = Vec::with_capacity(ROTATE_HEADER_BYTES + in_bytes.len());
    input.extend_from_slice(&rotate_input_header(count, rotation));
    input.extend_from_slice(in_bytes);

    // Stage the file protocol in a per-call
    // directory, as the projection route does.
    static ROTATE_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);
    let call_dir = route.work_dir.join(format!(
        "rotate-call-{}-{}",
        std::process::id(),
        ROTATE_CALL_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let staged = (|| -> Option<Vec<u8>> {
        std::fs::create_dir_all(&call_dir).ok()?;
        let in_path = call_dir.join("rotate-in.bin");
        std::fs::write(&in_path, &input).ok()?;
        let out_path = call_dir.join("rotate-out.bin");
        let output = Command::new(rotate_bin)
            .arg(&in_path)
            .arg(&out_path)
            .arg("1")
            .arg(route.device_id.to_string())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        std::fs::read(&out_path).ok()
    })();
    let _ = std::fs::remove_dir_all(&call_dir);
    let bytes = staged?;
    if bytes.len() != count * RECORD_BYTES {
        return None;
    }
    Some(bytes)
}

/// Write the kernel's input file under `call_dir`
/// (header + records + the shadow-depth trailer,
/// empty when the layer's light carries no map), run
/// the kernel program once, and return its output
/// file's bytes. Any failure is `None` (the caller
/// falls back to the CPU reference).
fn stage_and_run(
    route: &MojoRoute,
    call_dir: &std::path::Path,
    count: usize,
    uniforms: &[f64; 78],
    record_bytes: &[u8],
    shadow_bytes: &[u8],
) -> Option<Vec<u8>> {
    std::fs::create_dir_all(call_dir).ok()?;
    let inputs_path = call_dir.join("inputs.bin");
    {
        let mut f = std::fs::File::create(&inputs_path).ok()?;
        f.write_all(&project_input_header(count, uniforms)).ok()?;
        f.write_all(record_bytes).ok()?;
        f.write_all(shadow_bytes).ok()?;
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

/// Request kinds on the worker protocol
/// (`mojo/splat_worker.mojo`): a projection of the
/// payload's records, or a fused rotate+project.
const WORKER_KIND_PROJECT: u8 = 1;
const WORKER_KIND_ROTATE_PROJECT: u8 = 2;
/// One worker request's hang detector. A full-witness
/// request is seconds of work even under heavy
/// contention; five minutes is never a performance
/// knob, only the line past which the worker is
/// declared wedged, killed, and fallen back from.
const WORKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// Consecutive worker failures (spawn errors, deaths,
/// timeouts) after which the worker is disabled for
/// the rest of the process lifetime and every pack
/// takes the per-invocation path directly — a broken
/// worker binary must not tax every pack with a
/// doomed spawn first.
const WORKER_MAX_FAILURES: u32 = 3;

/// A live worker process and its protocol pipes. The
/// pipes are `Option`s so a request can move them
/// into the exchange thread and back (see
/// [`WorkerPool::request`]).
struct WorkerHandle {
    /// The binary this worker was spawned from and
    /// the device it serves: a request naming a
    /// different pair replaces the worker.
    bin: PathBuf,
    device_id: usize,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
}

impl WorkerHandle {
    /// Spawn a worker for `route`'s configuration.
    /// Its stderr goes to a log file under the work
    /// directory (the worker's only diagnostics
    /// channel; stdout is the protocol), or to null
    /// when the log cannot be opened.
    fn spawn(route: &MojoRoute, bin: &Path) -> Option<WorkerHandle> {
        static SPAWN_COUNTER: AtomicU64 = AtomicU64::new(0);
        let _ = std::fs::create_dir_all(&route.work_dir);
        let log_path = route.work_dir.join(format!(
            "worker-{}-{}.log",
            std::process::id(),
            SPAWN_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let stderr = std::fs::File::create(&log_path)
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());
        let mut child = Command::new(bin)
            .arg(route.device_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .ok()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            // Unreachable with piped stdio; never
            // leave a spawned worker behind.
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        Some(WorkerHandle {
            bin: bin.to_path_buf(),
            device_id: route.device_id,
            child,
            stdin: Some(stdin),
            stdout: Some(stdout),
        })
    }

    /// Kill the worker and reap it. The handle is
    /// dropped by the caller right after.
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How one exchange with the worker ended.
enum Exchange {
    /// The worker answered with a complete payload.
    Served(Vec<u8>),
    /// The worker answered but rejected the request
    /// (status 1): it is alive and in sync, and the
    /// caller falls back for this request only.
    Rejected,
    /// The exchange failed (I/O error, EOF, a
    /// malformed frame, a wrong payload length): the
    /// worker is dead or desynchronised and must be
    /// dropped.
    Dead,
}

/// The framing exchange, run on a thread so the
/// caller can bound it with a timeout: write the
/// request frame, read the response frame. The
/// response payload must be exactly `expected_len`
/// bytes on a status-0 answer — anything else is a
/// protocol failure, never a result.
fn worker_exchange(
    stdin: &mut ChildStdin,
    stdout: &mut ChildStdout,
    kind: u8,
    payload: &[u8],
    expected_len: usize,
) -> Exchange {
    let mut frame = Vec::with_capacity(9 + payload.len());
    frame.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    frame.push(kind);
    frame.extend_from_slice(payload);
    if stdin.write_all(&frame).is_err() || stdin.flush().is_err() {
        return Exchange::Dead;
    }
    let mut head = [0u8; 9];
    if stdout.read_exact(&mut head).is_err() {
        return Exchange::Dead;
    }
    let len = u64::from_le_bytes(head[0..8].try_into().unwrap()) as usize;
    let status = head[8];
    if status != 0 {
        // A rejection carries no payload; drain the
        // declared bytes (zero by protocol) so the
        // stream stays framed, then report it.
        let mut sink = vec![0u8; len.min(1 << 20)];
        if len > sink.len() || stdout.read_exact(&mut sink).is_err() {
            return Exchange::Dead;
        }
        return Exchange::Rejected;
    }
    if len != expected_len {
        return Exchange::Dead;
    }
    let mut body = vec![0u8; len];
    if stdout.read_exact(&mut body).is_err() {
        return Exchange::Dead;
    }
    Exchange::Served(body)
}

/// The process-wide worker pool: at most one worker,
/// spawned lazily, shared by every pack in the
/// process. Requests serialise on the pool lock —
/// the per-invocation route they replace spawned a
/// process per pack, so a queue for a warm worker is
/// still the faster shape, and the spec compiler's
/// pre-pack is sequential in any case.
struct WorkerPool {
    handle: Option<WorkerHandle>,
    /// Consecutive failures since the last served
    /// request; reaching [`WORKER_MAX_FAILURES`]
    /// disables the worker for the process lifetime.
    failures: u32,
    disabled: bool,
    /// Requests served (observability for the route
    /// gates: persistence is provable from it).
    served: u64,
}

impl WorkerPool {
    const fn new() -> WorkerPool {
        WorkerPool {
            handle: None,
            failures: 0,
            disabled: false,
            served: 0,
        }
    }

    /// Offer one request to the worker named by
    /// `route`. Every failure is `None` (the caller
    /// falls back); the pool's bookkeeping decides
    /// whether the worker survives the failure.
    fn request(
        &mut self,
        route: &MojoRoute,
        kind: u8,
        payload: Vec<u8>,
        expected_len: usize,
        timeout: Duration,
    ) -> Option<Vec<u8>> {
        let bin = route.worker_bin.as_deref()?;
        if self.disabled {
            return None;
        }
        // A worker serving a different binary or
        // device is replaced, not reused.
        let stale = match &self.handle {
            Some(handle) => handle.bin.as_path() != bin || handle.device_id != route.device_id,
            None => false,
        };
        if stale {
            if let Some(mut handle) = self.handle.take() {
                handle.kill();
            }
        }
        if self.handle.is_none() {
            match WorkerHandle::spawn(route, bin) {
                Some(handle) => self.handle = Some(handle),
                None => {
                    self.note_failure();
                    return None;
                }
            }
        }
        // The exchange runs on a thread; the caller
        // waits with a timeout. On timeout the worker
        // is killed — which closes the pipes and
        // unblocks the thread — and the request falls
        // back like any other failure. The handle
        // borrow is scoped to this block so the
        // outcome match below owns the pool again.
        let outcome = {
            let handle = self.handle.as_mut()?;
            let mut stdin = handle.stdin.take()?;
            let mut stdout = handle.stdout.take()?;
            let (tx, rx) = std::sync::mpsc::channel();
            let exchange = std::thread::spawn(move || {
                let out = worker_exchange(&mut stdin, &mut stdout, kind, &payload, expected_len);
                let _ = tx.send(matches!(out, Exchange::Served(_)));
                (stdin, stdout, out)
            });
            match rx.recv_timeout(timeout) {
                Ok(_) => match exchange.join() {
                    Ok((stdin, stdout, out)) => {
                        handle.stdin = Some(stdin);
                        handle.stdout = Some(stdout);
                        out
                    }
                    Err(_) => Exchange::Dead,
                },
                Err(_) => {
                    handle.kill();
                    let _ = exchange.join();
                    Exchange::Dead
                }
            }
        };
        match outcome {
            Exchange::Served(bytes) => {
                self.failures = 0;
                self.served += 1;
                Some(bytes)
            }
            Exchange::Rejected => None,
            Exchange::Dead => {
                if let Some(mut handle) = self.handle.take() {
                    handle.kill();
                }
                self.note_failure();
                None
            }
        }
    }

    fn note_failure(&mut self) {
        self.failures += 1;
        if self.failures >= WORKER_MAX_FAILURES {
            self.disabled = true;
        }
    }
}

/// The one pool for this process.
static WORKER_POOL: Mutex<WorkerPool> = Mutex::new(WorkerPool::new());

/// Offer one request to the process-wide worker (see
/// [`WorkerPool::request`]). A poisoned pool lock is
/// recovered rather than propagated: the pool's state
/// stays consistent because every mutation completes
/// before the lock is released.
fn worker_request(
    route: &MojoRoute,
    kind: u8,
    payload: Vec<u8>,
    expected_len: usize,
) -> Option<Vec<u8>> {
    let mut pool = WORKER_POOL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    pool.request(route, kind, payload, expected_len, WORKER_REQUEST_TIMEOUT)
}

/// Pack a splat layer, taking the Mojo desktop route
/// when one is configured and the layer is eligible
/// (its light carries no shadow map — a light
/// response is fine, the kernel applies it), and the
/// CPU reference otherwise — the packet is identical
/// either way, by the route's bit-exactness gate.
/// This is the pack entry the spec compiler
/// (`crate::splat_spec`) uses, so a configured route
/// serves every desktop pre-pack.
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
        // Eligibility, exactly: every light routes
        // except one whose shadow map edge exceeds the
        // cap (see the module docs) — the only input
        // this route declines by name.
        let shadow_within_cap = match light.shadow {
            Some(map) => map.size() >= 1 && map.size() <= SHADOW_MAX_EDGE,
            None => true,
        };
        if shadow_within_cap {
            if let Some(projected) =
                project_splats_mojo(cloud, transform, scene, width, height, light, route)
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
            rotate_bin: None,
            worker_bin: None,
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

    /// A 40-degree rotation about a tilted axis, as a
    /// storage-order quaternion (w, x, y, z) — the
    /// route gates' rotation.
    fn tilted_rotation() -> [f64; 4] {
        let axis = [0.3_f64, 1.0, 0.2];
        let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        let half = (40.0_f64).to_radians() / 2.0;
        let s = half.sin() / l;
        [half.cos(), axis[0] * s, axis[1] * s, axis[2] * s]
    }

    fn rotated_transform() -> SplatTransform {
        SplatTransform {
            translation: [0.25, -0.1, 0.4],
            rotation: tilted_rotation(),
            scale: 1.35,
        }
    }

    /// A configured rotation binary that does not
    /// exist leaves rotation on the host: the pack is
    /// the CPU reference's, packet-equal.
    #[test]
    fn missing_rotate_binary_falls_back_to_cpu() {
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let route = MojoRoute {
            project_bin: PathBuf::from("/nonexistent/splat_project"),
            rotate_bin: Some(PathBuf::from("/nonexistent/splat_rotate")),
            worker_bin: None,
            device_id: 0,
            work_dir: std::env::temp_dir().join("torogao-mojo-route-test"),
        };
        let light = SplatLight::default();
        let transform = rotated_transform();
        let via_route =
            pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &light, Some(&route));
        let cpu = pack_splats(&cloud, &transform, &scene, 512, 640);
        assert_eq!(via_route, cpu);
    }

    /// A rotation kernel that runs but produces no
    /// output is attempted, then abandoned for the
    /// host rotation: proven with a stand-in "binary"
    /// that records its invocation in a marker file
    /// and writes nothing. The projection binary does
    /// not exist here, so the pack falls back whole —
    /// the marker is what proves the rotation attempt
    /// happened on the way.
    #[cfg(unix)]
    #[test]
    fn failed_rotate_kernel_falls_back_to_cpu() {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("torogao-mojo-route-rotate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let marker = dir.join("rotate-invoked");
        let fake_rotate = dir.join("fake-rotate.sh");
        std::fs::write(
            &fake_rotate,
            format!("#!/bin/sh\ntouch \"{}\"\nexit 0\n", marker.display()),
        )
        .expect("write fake rotate kernel");
        std::fs::set_permissions(&fake_rotate, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake rotate kernel");
        let route = MojoRoute {
            project_bin: PathBuf::from("/nonexistent/splat_project"),
            rotate_bin: Some(fake_rotate),
            worker_bin: None,
            device_id: 0,
            work_dir: dir.join("work"),
        };
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let light = SplatLight::default();
        let transform = rotated_transform();
        let via_route =
            pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &light, Some(&route));
        assert!(
            marker.exists(),
            "rotated pack must attempt the rotation kernel"
        );
        let cpu = pack_splats(&cloud, &transform, &scene, 512, 640);
        assert_eq!(via_route, cpu);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The marker-binary route the light tests share:
    /// a stand-in "kernel" that records its invocation
    /// in a marker file and writes no output, so any
    /// pack that reaches it falls back to the CPU
    /// reference — the marker is what proves whether
    /// the route attempted the kernel at all.
    #[cfg(unix)]
    fn marker_route(dir: &std::path::Path) -> (MojoRoute, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).expect("temp dir");
        let marker = dir.join("invoked");
        let fake_bin = dir.join("fake-kernel.sh");
        std::fs::write(
            &fake_bin,
            format!("#!/bin/sh\ntouch \"{}\"\nexit 0\n", marker.display()),
        )
        .expect("write fake kernel");
        std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake kernel");
        (
            MojoRoute {
                project_bin: fake_bin,
                rotate_bin: None,
                worker_bin: None,
                device_id: 0,
                work_dir: dir.join("work"),
            },
            marker,
        )
    }

    /// The response the light tests grade with: an
    /// identity grade at a quarter world mix — the
    /// shape the pre-response test used, kept so the
    /// eligibility change is what the tests measure.
    fn test_response() -> LightResponse {
        LightResponse {
            grade: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            world_mix: 0.25,
        }
    }

    /// The shadow light the route tests share: an
    /// empty 64-edge map (every centre lit — the
    /// eligibility plumbing is what these tests
    /// measure; the visibility maths itself is gated
    /// by the witness benches and the route tests)
    /// plus the shared response.
    fn shadow_light(map: &crate::splat_light::SplatShadowMap) -> SplatLight<'_> {
        SplatLight {
            shadow: Some(map),
            response: Some(test_response()),
        }
    }

    /// A shadow-carrying layer DOES take the route
    /// since the shadow transcription (kernel
    /// candidate #2): with a stand-in binary that
    /// writes no output, the pack attempts the kernel
    /// (marker present) and falls back to the CPU
    /// reference, packet-equal to the direct lit pack.
    /// (This replaces candidate #1's
    /// `shadow_layer_never_invokes_the_kernel`, whose
    /// premise — the shadow factor stays host-side —
    /// is exactly what this candidate moved on-device;
    /// the never-invokes invariant now belongs to the
    /// over-cap map below.)
    #[cfg(unix)]
    #[test]
    fn shadow_layer_invokes_and_falls_back() {
        use crate::math::Mat4;
        use crate::splat_light::SplatShadowMap;
        let dir =
            std::env::temp_dir().join(format!("torogao-mojo-route-lit-{}", std::process::id()));
        let (route, marker) = marker_route(&dir);
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let transform = SplatTransform::default();

        // Control: the unlit pack tries the kernel.
        let unlit = SplatLight::default();
        let _ = pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &unlit, Some(&route));
        assert!(marker.exists(), "unlit pack must invoke the kernel binary");
        std::fs::remove_file(&marker).expect("clear marker");

        // Shadow-carrying (with a response named too):
        // the route attempts the kernel and falls back.
        let map = SplatShadowMap::rasterise(&[], Mat4::IDENTITY, 64);
        let lit = shadow_light(&map);
        let via_route = pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &lit, Some(&route));
        assert!(
            marker.exists(),
            "shadow-carrying pack must attempt the kernel binary"
        );
        let cpu = pack_splats_lit(&cloud, &transform, &scene, 512, 640, &lit);
        assert_eq!(via_route, cpu);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A shadow map whose edge exceeds the route's cap
    /// never invokes the kernel binary: the trailer at
    /// the cap is already 64 MiB, so the over-cap map
    /// declines the route by name and the layer packs
    /// on the CPU, packet-equal to the direct lit pack.
    #[cfg(unix)]
    #[test]
    fn oversize_shadow_map_never_invokes_the_kernel() {
        use crate::math::Mat4;
        use crate::splat_light::SplatShadowMap;
        let dir = std::env::temp_dir().join(format!(
            "torogao-mojo-route-oversize-{}",
            std::process::id()
        ));
        let (route, marker) = marker_route(&dir);
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let transform = SplatTransform::default();
        let map = SplatShadowMap::rasterise(&[], Mat4::IDENTITY, SHADOW_MAX_EDGE + 1);
        let lit = shadow_light(&map);
        let via_route = pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &lit, Some(&route));
        assert!(
            !marker.exists(),
            "over-cap shadow pack must not invoke the kernel binary"
        );
        let cpu = pack_splats_lit(&cloud, &transform, &scene, 512, 640, &lit);
        assert_eq!(via_route, cpu);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A configured route whose binary does not exist
    /// falls back to the CPU reference for a
    /// shadow-carrying lit layer, packet-equal to the
    /// direct lit pack.
    #[test]
    fn missing_binary_with_shadow_falls_back_to_cpu() {
        use crate::math::Mat4;
        use crate::splat_light::SplatShadowMap;
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let transform = SplatTransform::default();
        let map = SplatShadowMap::rasterise(&[], Mat4::IDENTITY, 64);
        let lit = shadow_light(&map);
        let dir = std::env::temp_dir().join(format!(
            "torogao-mojo-route-missing-shadow-{}",
            std::process::id()
        ));
        let route = MojoRoute {
            project_bin: dir.join("no-such-kernel"),
            rotate_bin: None,
            worker_bin: None,
            device_id: 0,
            work_dir: dir.join("work"),
        };
        let via_route = pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &lit, Some(&route));
        let cpu = pack_splats_lit(&cloud, &transform, &scene, 512, 640, &lit);
        assert_eq!(via_route, cpu);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A response-only lit layer DOES take the route
    /// since the response transcription (kernel
    /// candidate #1): with a stand-in binary that
    /// writes no output, the pack attempts the kernel
    /// (marker present) and falls back to the CPU
    /// reference, packet-equal to the direct lit pack.
    #[cfg(unix)]
    #[test]
    fn response_only_layer_invokes_and_falls_back() {
        let dir = std::env::temp_dir().join(format!(
            "torogao-mojo-route-response-{}",
            std::process::id()
        ));
        let (route, marker) = marker_route(&dir);
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let transform = SplatTransform::default();
        let lit = SplatLight {
            shadow: None,
            response: Some(test_response()),
        };
        let via_route = pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &lit, Some(&route));
        assert!(
            marker.exists(),
            "response-only pack must attempt the kernel binary"
        );
        let cpu = pack_splats_lit(&cloud, &transform, &scene, 512, 640, &lit);
        assert_eq!(via_route, cpu);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A configured route whose binary does not exist
    /// falls back to the CPU reference for a
    /// response-only lit layer, packet-equal to the
    /// direct lit pack.
    #[test]
    fn missing_binary_with_response_falls_back_to_cpu() {
        let cloud = room_corner_cloud();
        let scene = test_scene();
        let route = MojoRoute {
            project_bin: PathBuf::from("/nonexistent/splat_project"),
            rotate_bin: None,
            worker_bin: None,
            device_id: 0,
            work_dir: std::env::temp_dir().join("torogao-mojo-route-test"),
        };
        let lit = SplatLight {
            shadow: None,
            response: Some(test_response()),
        };
        let via_route = pack_splats_mojo(
            &cloud,
            &SplatTransform::default(),
            &scene,
            512,
            640,
            &lit,
            Some(&route),
        );
        let cpu = pack_splats_lit(&cloud, &SplatTransform::default(), &scene, 512, 640, &lit);
        assert_eq!(via_route, cpu);
    }

    /// Write a fake worker (a Python script speaking
    /// the worker framing) into `dir` and return its
    /// path. The script records each spawn's pid in
    /// the `spawns` file beside it, so tests can tell
    /// a respawn from a reuse.
    #[cfg(unix)]
    fn fake_worker(dir: &std::path::Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let spawns = dir.join("spawns");
        let script = format!(
            "#!/usr/bin/env python3\nimport os, struct, sys\n\
             with open({spawns:?}, 'a') as f: f.write(str(os.getpid()) + '\\n')\n\
             def readn(n):\n\
             \x20   b = b''\n\
             \x20   while len(b) < n:\n\
             \x20       c = sys.stdin.buffer.read(n - len(b))\n\
             \x20       if not c: sys.exit(3)\n\
             \x20       b += c\n\
             \x20   return b\n\
             def frame():\n\
             \x20   (n,) = struct.unpack('<Q', readn(8))\n\
             \x20   kind = readn(1)[0]\n\
             \x20   return kind, readn(n)\n\
             def respond(status, payload):\n\
             \x20   sys.stdout.buffer.write(struct.pack('<Q', len(payload)) + bytes([status]) + payload)\n\
             \x20   sys.stdout.buffer.flush()\n{body}",
            spawns = spawns.display().to_string(),
            body = body,
        );
        let path = dir.join("fake-worker.py");
        std::fs::write(&path, script).expect("write fake worker");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake worker");
        path
    }

    #[cfg(unix)]
    fn pool_route(worker_bin: PathBuf, work_dir: PathBuf) -> MojoRoute {
        MojoRoute {
            project_bin: PathBuf::from("/nonexistent/splat_project"),
            rotate_bin: None,
            worker_bin: Some(worker_bin),
            device_id: 0,
            work_dir,
        }
    }

    /// A worker that answers requests serves them
    /// repeatedly from one spawn: the pool's second
    /// request reuses the process the first spawned.
    #[cfg(unix)]
    #[test]
    fn worker_pool_serves_repeatedly_from_one_spawn() {
        let dir =
            std::env::temp_dir().join(format!("torogao-mojo-worker-pool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        // Echo worker: the response payload is the
        // request payload, so expected_len matches.
        let bin = fake_worker(
            &dir,
            "while True:\n    kind, payload = frame()\n    respond(0, payload)\n",
        );
        let route = pool_route(bin, dir.join("work"));
        let mut pool = WorkerPool::new();
        let payload = vec![7u8; 64];
        for _ in 0..2 {
            let got = pool.request(
                &route,
                WORKER_KIND_PROJECT,
                payload.clone(),
                payload.len(),
                Duration::from_secs(30),
            );
            assert_eq!(got.as_deref(), Some(payload.as_slice()));
        }
        assert_eq!(pool.served, 2);
        let spawns = std::fs::read_to_string(dir.join("spawns")).expect("spawns file");
        assert_eq!(
            spawns.lines().count(),
            1,
            "two requests, one worker spawn: {spawns}"
        );
        if let Some(mut handle) = pool.handle.take() {
            handle.kill();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A worker that rejects a request stays alive
    /// and serves the next one: rejection is not
    /// death, and the pool must not respawn.
    #[cfg(unix)]
    #[test]
    fn worker_pool_rejection_keeps_the_worker() {
        let dir =
            std::env::temp_dir().join(format!("torogao-mojo-worker-reject-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        // Reject the first request, echo the rest.
        let bin = fake_worker(
            &dir,
            "kind, payload = frame()\nrespond(1, b'')\nwhile True:\n    kind, payload = frame()\n    respond(0, payload)\n",
        );
        let route = pool_route(bin, dir.join("work"));
        let mut pool = WorkerPool::new();
        let payload = vec![3u8; 32];
        let rejected = pool.request(
            &route,
            WORKER_KIND_PROJECT,
            payload.clone(),
            payload.len(),
            Duration::from_secs(30),
        );
        assert_eq!(rejected, None, "a rejected request falls back");
        let got = pool.request(
            &route,
            WORKER_KIND_PROJECT,
            payload.clone(),
            payload.len(),
            Duration::from_secs(30),
        );
        assert_eq!(got.as_deref(), Some(payload.as_slice()));
        let spawns = std::fs::read_to_string(dir.join("spawns")).expect("spawns file");
        assert_eq!(
            spawns.lines().count(),
            1,
            "rejection must not cost a respawn: {spawns}"
        );
        if let Some(mut handle) = pool.handle.take() {
            handle.kill();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A worker that dies at spawn is fallen back
    /// from on every request, and after
    /// WORKER_MAX_FAILURES consecutive failures the
    /// pool stops spawning it at all.
    #[cfg(unix)]
    #[test]
    fn worker_pool_dead_worker_falls_back_and_disables() {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("torogao-mojo-worker-dead-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let bin = dir.join("dead-worker.sh");
        std::fs::write(
            &bin,
            "#!/bin/sh\necho $$ >> \"$(dirname \"$0\")/spawns\"\nexit 1\n",
        )
        .expect("write dead worker");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .expect("chmod dead worker");
        let route = pool_route(bin, dir.join("work"));
        let mut pool = WorkerPool::new();
        for _ in 0..WORKER_MAX_FAILURES {
            let got = pool.request(
                &route,
                WORKER_KIND_PROJECT,
                vec![1u8; 8],
                8,
                Duration::from_secs(30),
            );
            assert_eq!(got, None, "a dead worker falls back");
        }
        assert!(pool.disabled, "pool disables a repeatedly dead worker");
        let got = pool.request(
            &route,
            WORKER_KIND_PROJECT,
            vec![1u8; 8],
            8,
            Duration::from_secs(30),
        );
        assert_eq!(got, None);
        let spawns = std::fs::read_to_string(dir.join("spawns")).expect("spawns file");
        assert_eq!(
            spawns.lines().count(),
            WORKER_MAX_FAILURES as usize,
            "no spawn attempts past the disable: {spawns}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A worker that reads a request and then wedges
    /// is killed at the request timeout and fallen
    /// back from — the pool must not hang on it, and
    /// the wedged process must not survive.
    #[cfg(unix)]
    #[test]
    fn worker_pool_timeout_kills_a_wedged_worker() {
        let dir =
            std::env::temp_dir().join(format!("torogao-mojo-worker-wedge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let bin = fake_worker(
            &dir,
            "import time\nkind, payload = frame()\ntime.sleep(60)\n",
        );
        let route = pool_route(bin, dir.join("work"));
        let mut pool = WorkerPool::new();
        let started = std::time::Instant::now();
        let got = pool.request(
            &route,
            WORKER_KIND_PROJECT,
            vec![9u8; 16],
            16,
            Duration::from_secs(2),
        );
        assert_eq!(got, None, "a wedged worker falls back");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the timeout, not the wedge, bounds the request"
        );
        let spawns = std::fs::read_to_string(dir.join("spawns")).expect("spawns file");
        let pid: i32 = spawns.trim().parse().expect("worker pid");
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "the wedged worker (pid {pid}) was killed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
