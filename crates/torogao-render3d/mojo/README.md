# Mojo kernels for torogao-render3d — the desktop projection route

`splat_project.mojo` is torogao-render3d's splat
projection kernel: the projection half of the CPU
splat pack (`crates/torogao-render3d/src/splat.rs`,
`project_splats`), adopted as the **desktop
projection route** by Owner ruling (decision walk,
2026-11-27) after the Mojo+PTX evaluation lane proved
it bit-exact against the CPU reference on the full
1,157,141-splat bonsai witness (kernel median
4.54 ms on the RTX A6000 / 8.66 ms on the RTX 4070
Laptop vs 594–871 ms for the CPU projection measured
the same nights). The wiring lives in
`src/splat_mojo.rs`; the spec compiler's pre-pack
(`src/splat_spec.rs`) packs through it whenever a
route is configured.

The CPU pack remains in tree as the reference and
the fallback — and the only mobile path: Mojo
1.2.0's stdlib refuses Android, so this route is
desktop-only by construction. The Vulkan GPU path in
`splat_gpu.rs` remains the runtime path; this kernel
serves the compile-time pre-pack, where every frame
of a frozen sequence is projected at compile time.

## The persistent worker (invocation-overhead cure, 2026-11-27)

The one-shot programs pay the MAX runtime start +
device-context creation on EVERY invocation —
measured at ~2.1–3.2 s of process wall against
5–180 ms of transfers + kernel on the full witness,
essentially independent of input size (a
1,000-splat invocation walls the same ~3 s), and
the rotated route paid it twice (two programs).
`splat_worker.mojo` cures the shape, not a kernel:
it creates one device context and serves pack
requests over a length-prefixed pipe protocol on
its stdin/stdout until EOF. It imports
`project_kernel` and `rotate_kernel` from the
one-shot sources — the same functions, not
transcriptions — and a rotated pack is one fused
request: rotation and projection run back to back
with the records staying in a device buffer
between them, so the 296 MB intermediate never
round-trips the host. The Rust route
(`src/splat_mojo.rs`) offers each pack to the
worker first when `TOROGAO_MOJO_WORKER_BIN` is
configured; any failure — spawn, death, rejection,
a 300 s hang timeout — falls back to the
per-invocation path, and from there to the CPU
reference, packet-identical throughout. The worker
exits on stdin EOF, so it dies with the host
process; three consecutive failures disable it for
the process lifetime. With no worker configured,
the route behaves exactly as before it existed.

## Contract

The Rust side writes the input file, runs the kernel
binary, and reads the output records (formats are
specified in the header comment of
`splat_project.mojo`). Opacity and scales are
pre-activated on the host by the pack's own
functions, so the kernel needs no transcendentals;
every operation is IEEE `+ - * / sqrt` on f64.
Eligibility: every layer routes except one whose
light carries a shadow map with an edge over 4096
texels. The Rung 4 light response (candidate #1)
and the shadow centre test (candidate #2) are both
transcribed by the projection kernel: a
shadow-carrying light's rasterised map travels as
a device input (a trailer after the records in the
input format the kernel source specifies) and the
kernel applies the map's 3x3 PCF visibility at each
splat centre. The triangle rasteriser itself stays
host-side — it runs once per scene at spec-compile
time, caller-owned, not per layer. An over-cap map
declines the route by name and the layer packs on
the CPU. Layer rotation runs
on the GPU rotation kernel (below) when one is
configured, and otherwise host-side by the pack's
own `rotate_cloud`, exactly as the CPU path applies
it. A configured route that fails for any
reason falls back to the CPU reference — the packet
is identical either way; that is what the
bit-exactness gate buys.

## Build (pinned toolchain)

Mojo 1.2.0 (2016d0cd), installed on primo at
`~/.local/share/pax/mojo-ptx-20261127/venv` (uv pip
route). Build both architecture binaries with the
pinned script:

    mojo/build.sh        # -> mojo/build/splat_project_sm86 (RTX A6000)
                         #    mojo/build/splat_project_sm89 (RTX 4070 Laptop)
                         #    mojo/build/splat_rotate_sm86 / _sm89 likewise
                         #    mojo/build/splat_worker_sm86 / _sm89 likewise

`--fp-mode contract=off` is LOAD-BEARING and pinned
in `build.sh`, with a unit test standing over it
(`splat_mojo::tests::build_script_pins_contract_off`).
The default (`contract=fast`) fuses multiply-adds
into FMAs and the output differs from the CPU
reference in the last bit (worst relative depth
deviation 2.2e-16 measured; 13,204 of 20,000 debug
records differed) — a recorded REJECT. With
contraction off the gate is bit-exact. Do not remove
the flag.

## Configure

The route is opt-in by environment (no route is the
default, and an unconfigured pack is the CPU pack
exactly):

    TOROGAO_MOJO_PROJECT_BIN=.../mojo/build/splat_project_sm86
    TOROGAO_MOJO_ROTATE_BIN=.../mojo/build/splat_rotate_sm86
                                    # optional; without it, layer
                                    # rotations run on the host
    TOROGAO_MOJO_WORKER_BIN=.../mojo/build/splat_worker_sm86
                                    # optional; the persistent
                                    # worker (above) — when set,
                                    # packs are offered to it
                                    # before the one-shot programs
    TOROGAO_MOJO_DEVICE_ID=1        # optional; MAX-runtime enumeration
                                    # (primo: 0 = 4070 Laptop, 1 = A6000)
    TOROGAO_MOJO_WORK_DIR=...       # optional scratch root

## Gates

- `splat_mojo_bench` (same crate, `src/bin/`): the
  kernel against the CPU reference on the full
  witness, record-for-record, bitwise. It prints
  `GATE: BIT-EXACT` and exits 0 only when every
  surviving record is bitwise identical and the cull
  state matches. Modes: default unlit, `--lit` the
  response-carrying pack, `--shadow` the
  shadow-carrying pack (response plus a 2048-edge
  map rasterised over the witness; the bench prints
  the map's visibility census).

      cargo run --release -p torogao-render3d --bin splat_mojo_bench -- \
          --ply <bonsai point_cloud.ply> --mojo-bin <splat_project_smXX> \
          --work <dir> --device-id <0=4070 Laptop, 1=A6000 on primo>

- `tests/splat_mojo_route.rs`: the integrated route
  against the CPU pack — small-cloud equality, the
  full witness, and a twin-render of a Mojo-packed
  scene (ignored tests; they need the kernel binary
  and, for the render, a Vulkan device + the toon
  fixture). The worker gates in the same file put
  the full witness through the persistent worker
  (identity twice — one spawn across both packs,
  the one-shot binary marker-proven never to run —
  and rotated through the fused request), plus a
  dead-worker fallback gate; the worker pool's
  spawn/reuse/rejection/disable/timeout behaviour
  is covered by unit tests in `src/splat_mojo.rs`
  against scripted fake workers (no GPU needed).
  The lit gates (candidate #1) and shadow gates
  (candidate #2) run the same shapes with
  response-only and shadow-carrying lights; each
  shadow gate first asserts its caster rig actually
  darkens a record, so the gate cannot pass on an
  all-lit map.

Any Mojo upgrade re-runs both gates before its
binaries are used; the flag's existence is the first
thing to re-verify.

## Extension trial (2026-11-27): layer rotation — ADOPTED

`splat_rotate.mojo` trials a second kernel: the
layer-rotation step (`splat::rotate_cloud`), chosen
on profiling evidence from the adoption lane (full
witness, medians: projection 563.8 ms, rotation
401.8 ms standalone — 71% of projection's cost —
frame-record build 160.1 ms; rotation is pure f64
`+ - * / sqrt`, no transcendentals, the property
that made projection bit-exact). The trial harness
is `src/bin/splat_rotate_mojo_bench.rs`: the kernel
over the estate's frame records against
`frame_records(rotate_cloud(..))` on the full
1,157,141-splat witness under a 40° rotation about
(1, 2, 3).

Verdict (2026-11-27): **BIT-EXACT on both GPUs**
(0 field mismatches in 1,157,141 records; whole-file
output SHA-256 identical to the CPU reference) —
the extension is adopted under the same
`--fp-mode contract=off` pin (its build lines are in
`build.sh`). Kernel medians measured the same night:
12.2 ms on the RTX 4070 Laptop; the A6000 run was
contention-inflated (79.3 ms; its host-to-device
copy alone took 135.8 ms against the 4070's
23.8 ms) and is reported as such, not averaged away.
Wired into the pack route by the follow-on lane
(2026-11-27): `src/splat_mojo.rs` runs the rotation
kernel over the cloud's frame records ahead of the
projection kernel whenever `TOROGAO_MOJO_ROTATE_BIN`
names a binary, with the host rotation as the
packet-identical fallback. Its route gate is
`route_matches_cpu_on_full_witness_rotated` in
`tests/splat_mojo_route.rs` — full-witness packet
equality with the rotation proven live by a marker
wrapper. Full trial record:
`~/workspace/render3d-mojo-adopt-20261127/RECORD.md`.

## Licence note (2026-11-27)

The installed toolchain's PyPI artefact declares
`License: LicenseRef-MAX-Platform-Software-License`:
the MAX Platform licence, not plain Apache (the Mojo
compiler/stdlib source was separately reported
open-sourced under Apache 2.0 with LLVM Exceptions
on 2026-08-18; the governing fact for what is
installed is the artefact's own declaration).
Adoption covers internal/desktop use — compiler only,
self-hosted, nothing redistributed. A compiled Mojo
binary embeds MAX host-runtime components: **before
any product ships such a binary, re-check the MAX
Platform terms for redistribution.** Accordingly this
repo ships `.mojo` source only; compiled binaries are
gitignored local build outputs of `build.sh`, never
committed. Full licence record:
`~/workspace/torogao-mojo-ptx-20261127/RECORD.md` §2.
