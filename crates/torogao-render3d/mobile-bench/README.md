# Phone benchmark harness — splat projection (render3d mobile compute)

The physical-device column of the mobile projection record is the
estate's named open gate: emulator numbers are indicative only, and
the Galaxy Flip 7 measurement happens when the Owner runs this on
the phone. Everything needed is here.

## What it measures

`splat_mobile_bench` (crate bin, `src/bin/splat_mobile_bench.rs`)
runs the estate's bonsai witness (1,157,141 splats, plate config A)
through:

- the CPU reference projection (f64) — the phone-CPU baseline;
- every Vulkan device the phone exposes, via
  `backend::mobile_project`: the fp64 leg where `shaderFloat64`
  exists (expected: NOT on a phone — the census line prints it),
  and the f32 mobile projection (`shaders/splat_project_f32.comp`)
  gated against the Tier 2 band with the per-field error table.

Output is one fact per line, written to the `--out` file as well as
stdout, so the file can be shared back as-is.

## Build (on primo)

    ./build-phone-bench.sh

Produces `splat_mobile_bench` (aarch64-linux-android, API 24+)
next to this README. Toolchain: the estate's rustup 1.100 +
NDK 30 clang — the same route the mobile apps build with.

## Run (phone connected by adb, USB debugging on)

    ./run-on-phone.sh [path-to-point_cloud.ply] [limit]

Defaults: the estate's bonsai witness on primo; no limit (the full
1,157,141 splats). The script pushes the binary + witness to
`/data/local/tmp/`, runs the bench there (a shell binary — no APK,
no install), and pulls `bench-phone.txt` back next to this README.
Share that file back and the physical-device column of the record
fills in.

A quick smoke first: `./run-on-phone.sh "" 20000` runs the 20k
debug cut (~seconds) before committing to the full witness.

## Reading the result

- `shaderFloat64=false` on the phone's GPU is the expected,
  record-confirming line — it is WHY the f32 path exists.
- The f32 dispatch median is the phone's projection cost with
  records resident; upload/readback are one-time staging costs.
- Compare against the record's primo table
  (`~/workspace/render3d-mobile-compute-20261127/RECORD.md` §3):
  same witness, same gate, one more column.
