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

## Contract

The Rust side writes the input file, runs the kernel
binary, and reads the output records (formats are
specified in the header comment of
`splat_project.mojo`). Opacity and scales are
pre-activated on the host by the pack's own
functions, so the kernel needs no transcendentals;
every operation is IEEE `+ - * / sqrt` on f64.
Eligibility: unlit layers only (the Rung 4 light
interaction packs on the CPU); layer rotation is
applied host-side by the pack's own `rotate_cloud`
before the kernel runs, exactly as the CPU path
applies it. A configured route that fails for any
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
    TOROGAO_MOJO_DEVICE_ID=1        # optional; MAX-runtime enumeration
                                    # (primo: 0 = 4070 Laptop, 1 = A6000)
    TOROGAO_MOJO_WORK_DIR=...       # optional scratch root

## Gates

- `splat_mojo_bench` (same crate, `src/bin/`): the
  kernel against the CPU reference on the full
  witness, record-for-record, bitwise. It prints
  `GATE: BIT-EXACT` and exits 0 only when every
  surviving record is bitwise identical and the cull
  state matches.

      cargo run --release -p torogao-render3d --bin splat_mojo_bench -- \
          --ply <bonsai point_cloud.ply> --mojo-bin <splat_project_smXX> \
          --work <dir> --device-id <0=4070 Laptop, 1=A6000 on primo>

- `tests/splat_mojo_route.rs`: the integrated route
  against the CPU pack — small-cloud equality, the
  full witness, and a twin-render of a Mojo-packed
  scene (ignored tests; they need the kernel binary
  and, for the render, a Vulkan device + the toon
  fixture).

Any Mojo upgrade re-runs both gates before its
binaries are used; the flag's existence is the first
thing to re-verify.

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
