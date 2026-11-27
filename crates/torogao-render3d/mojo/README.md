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
