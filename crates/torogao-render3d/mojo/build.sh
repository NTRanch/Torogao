#!/usr/bin/env bash
# Build the Mojo kernels torogao-render3d's desktop
# route runs (crates/torogao-render3d/src/splat_mojo.rs).
# Pinned toolchain: Mojo 1.2.0 (2016d0cd), the venv at
# ~/.local/share/pax/mojo-ptx-20261127/venv on primo
# (override with $MOJO). One binary per GPU
# architecture — the compiler embeds SASS for a single
# --target-accelerator: sm_86 = RTX A6000, sm_89 = RTX
# 4070 Laptop.
#
# --fp-mode contract=off is LOAD-BEARING, pinned here
# on every build line, and a unit test in splat_mojo.rs
# fails if it moves. The compiler's default mode
# (contract=fast) fuses multiply-adds into FMAs and is
# a measured REJECT under the estate's bit-exactness
# law: on the 20,000-splat debug witness it differed
# from the CPU reference in 13,204 records (worst
# relative depth deviation 2.217e-16), and the full
# 1,157,141-splat gate reads NOT BIT-EXACT. With
# contraction off the same source is bit-exact on both
# estate GPUs. Full evidence:
# ~/workspace/torogao-mojo-ptx-20261127/RECORD.md §4-5.
# Any Mojo upgrade re-runs the gate harness before its
# binaries are used (README.md, "Gates").
#
# The compiled binaries are local build outputs:
# gitignored, never shipped (licence note in
# README.md).
set -euo pipefail
cd "$(dirname "$0")"
MOJO="${MOJO:-$HOME/.local/share/pax/mojo-ptx-20261127/venv/bin/mojo}"
mkdir -p build
"$MOJO" build --fp-mode contract=off --target-accelerator sm_86 splat_project.mojo -o build/splat_project_sm86
"$MOJO" build --fp-mode contract=off --target-accelerator sm_89 splat_project.mojo -o build/splat_project_sm89
# Extension trial (2026-11-27): the layer-rotation kernel
# (splat_rotate.mojo), judged by the same witness rule and
# pinned under the same fp-mode invariant. Trial artefact,
# not wired into the pack route.
"$MOJO" build --fp-mode contract=off --target-accelerator sm_86 splat_rotate.mojo -o build/splat_rotate_sm86
"$MOJO" build --fp-mode contract=off --target-accelerator sm_89 splat_rotate.mojo -o build/splat_rotate_sm89
echo "built: build/splat_project_sm86 build/splat_project_sm89 build/splat_rotate_sm86 build/splat_rotate_sm89"
