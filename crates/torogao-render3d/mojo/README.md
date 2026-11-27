# Mojo/PTX prototype: splat projection kernel

`splat_project.mojo` is a measured prototype of the projection half of
the CPU splat pack (`crates/torogao-render3d/src/splat.rs`,
`project_splats_sorted`), written for the Mojo+PTX evaluation lane of
2026-11-27. It is NOT wired into the renderer; the Vulkan compute path
in `splat_gpu.rs` remains the production GPU path. The prototype
exists to answer one question with numbers: can a Mojo kernel replace
the CPU projection step (the per-frame cost paid when sequence frames
are pre-packed at compile time) without breaking the estate's
bit-exactness law (ACCELERATION-DESIGN §3)?

Answer (full record: `~/workspace/torogao-mojo-ptx-20261127/RECORD.md`):
yes — bit-exact against the CPU reference on the full 1,157,141-splat
bonsai witness, kernel median 4.54 ms on the RTX A6000 vs 594–871 ms
for the CPU projection measured the same nights.

## Contract

The Rust harness `splat_mojo_bench` (same crate, `src/bin/`) writes
the input file, runs this binary, and compares the output
record-for-record, bitwise. File formats are specified in the header
comment of `splat_project.mojo`. Scope: plate configuration —
identity layer rotation, unlit. Opacity and scales are pre-activated
on the host by the pack's own functions, so the kernel needs no
transcendentals; every operation is IEEE `+ - * / sqrt` on f64.

## Build (pinned toolchain)

Mojo 1.2.0 (2016d0cd), installed on primo at
`~/.local/share/pax/mojo-ptx-20261127/venv` (uv pip route; see the
lane record for the licence note). One binary per GPU architecture —
the compiler embeds SASS for a single `--target-accelerator`:

    MOJO=~/.local/share/pax/mojo-ptx-20261127/venv/bin/mojo
    $MOJO build --fp-mode contract=off --target-accelerator sm_86 \
        splat_project.mojo -o splat_project_sm86   # RTX A6000
    $MOJO build --fp-mode contract=off --target-accelerator sm_89 \
        splat_project.mojo -o splat_project_sm89   # RTX 4070 Laptop

`--fp-mode contract=off` is LOAD-BEARING: the default
(`contract=fast`) fuses multiply-adds into FMAs and the output differs
from the CPU reference in the last bit (worst relative depth deviation
2.2e-16 measured; 13,204 of 20,000 debug records differed, axis fields
worst). With contraction off the gate is bit-exact. Do not remove the
flag.

## Run

    cargo run --release -p torogao-render3d --bin splat_mojo_bench -- \
        --ply <bonsai point_cloud.ply> --mojo-bin <splat_project_smXX> \
        --work <dir> --device-id <0=4070 Laptop, 1=A6000 on primo>

The harness prints `GATE: BIT-EXACT` and exits 0 only when every
surviving record is bitwise identical and the cull state matches.

## CPU kernel + the mobile question (2026-11-27, render3d-mobile-compute lane)

`splat_project_cpu.mojo` is the CPU transcription of the GPU kernel:
same input/output file contract, same CLI (`IN OUT REPS DEVICE_ID`,
the device id is ignored), same math in the same order, single
thread by design. It exists for the phone question — what of the
Mojo win can a Galaxy Flip 7-class device use? Measured verdicts
(full record: `~/workspace/render3d-mobile-compute-20261127/RECORD.md`):

- x86-64 CPU: builds and gates BIT-EXACT against the Rust CPU
  reference through the same `splat_mojo_bench` harness, on the
  debug cut (20,000 splats) and the full witness (1,157,141):
  kernel median 350.8 ms vs ~400 ms for the Rust pack's projection
  share the same night. The FP invariant is the same one as the
  GPU build, and it is load-bearing on CPU too: build with
  `--fp-mode contract=off`. The default (contract=fast) build is
  the same recorded REJECT — worst relative depth deviation
  2.217e-16, the identical 1-ulp signature as the GPU reject.
- aarch64 Linux: the compiler accepts the target and code
  generation completes, but the link cannot complete from the
  pip install: Mojo drives the host `/usr/bin/cc` with an aarch64
  archive (rejected by the host linker), and the venv ships
  x86-64-only runtime libraries. Blocked on an aarch64 Mojo
  distribution, not on the source.
- Android (`aarch64-linux-android`): refused by Mojo 1.2.0's own
  standard library — `Current compilation target does not support
  operation: get_errno`. Mojo cannot target Android in this
  version.
- Phone GPUs: no Mojo path exists (PTX is NVIDIA-only). The
  phone's acceleration route is the Vulkan f32 projection in
  `shaders/splat_project_f32.comp` + `backend::mobile_project`,
  whose `precise` qualifiers are this flag's lesson translated
  to GLSL. Mojo's role in the phone story is the reference
  implementation the other ports are gated against, plus a
  possible future ARM CPU path if a distribution appears.

Build (CPU, from this directory):

    $MOJO build --fp-mode contract=off splat_project_cpu.mojo \
        -o splat_project_cpu

Gate it exactly like the GPU binary: `splat_mojo_bench --mojo-bin
./splat_project_cpu` prints `GATE: BIT-EXACT`.
