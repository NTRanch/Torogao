# The cross-architecture twin gate (runtime plan Rung R2)

`twin-gate.sh` is the scripted local gate for plan SS2.4/SS4.1.
It runs both R1 witness corpora — in their committed file form
(`torogao-runtime-corpus/1`, see `../corpus/`) — on three
executed targets and judges the plan's two tiers separately:

| Target | Build | Execution |
|---|---|---|
| `native` | x86_64-unknown-linux-gnu | native (the reference run) |
| `gnu` | aarch64-unknown-linux-gnu | qemu-aarch64 user-mode, `-L /usr/aarch64-linux-gnu` |
| `android` | aarch64-linux-android (cargo-ndk, NDK 30) | qemu-aarch64 user-mode (static harness) |

Per witness (interpreter + Loop Service):

1. **Within-target twin runs byte-identical** — Tier 1 and
   Tier 2 alike; one architecture must reproduce itself
   exactly (the D-1397 twin criterion).
2. **Tier 1 byte-identical across all three targets** — the
   genesis, per-tick hash chain, checkpoint save hash, and
   replay verdict admit no tolerance.
3. **Tier 2 inside the recorded band across targets** —
   `compare_twin.py` measures every gauge's numeric distance
   and prints it; the digest mismatches are reported as
   information, never as the verdict.
4. **Cross-target save gate** — every target's checkpoint
   save envelope is byte-identical, and all 9
   producer/consumer target pairs continue it to the
   identical chain.
5. **estate-rng/1 golden vectors** — pass natively and on
   the aarch64 build under qemu.

`negative-control.sh` proves the tiers are really separate:
it applies `negative-control.patch` (a deliberate +1.25e-3
perturbation of a Tier 2 gauge in the interpreter's
`present()`), and requires Tier 1 to PASS against the
perturbed build while the band comparison FAILS at exactly
the planted distance. It reverts the patch and rebuilds
afterwards.

## The band of record

Recorded 2026-11-26 (Rung R2, first measurement):

- **Band: max_abs 1e-6, max_rel 1e-6** (combined tolerance —
  a gauge passes on either leg), also named in code as
  `torogao_runtime::RECORDED_BAND`.
- **Measured: max_abs 0.0, max_rel 0.0, digest mismatches 0**
  across the three executed targets, both witnesses
  (56 interpreter gauge values + 30 Loop gauge values
  compared). The witnesses' gauges are integer ratios
  evaluated in f64 — IEEE division is exact across these
  targets — so a zero measurement is the honest expectation
  today, and the band is set at its floor for the day
  engine/rig transcendentals join Tier 2, not loosened to
  fit a measurement. It changes only by evidence, recorded
  here and in the rung record.

## Provisioning baked into the scripts

- The aarch64-linux-gnu **linker pin** is structural: the
  workspace `.cargo/config.toml` (R0). The Loop tree carries
  its own mirror of the same pin (R1).
- qemu-user needs **`-L /usr/aarch64-linux-gnu`** for the
  dynamically linked `gnu` binaries.
- The **Android leg** runs with `TMPDIR` pointed at the gate
  scratch dir (Rust's Android `temp_dir` defaults to
  `/data/local/tmp`, which a qemu-user process on a Linux
  host does not have; on a device the app sandbox provides
  it) and is linked **fully static** (`crt-static` +
  `--allow-multiple-definition` for the NDK/std
  `rust_eh_personality` duplicate + the NDK clang builtins
  archive for the outline-atomics helpers) so qemu-user can
  run it without a bionic loader. These flags scope to this
  gate's harness builds only; the shipped Android route
  (dynamic .so inside an APK) does not use them.

## Open column

**Physical ARM device: OPEN.** Decision 5 as amended
requires a physical-device gate column beside qemu. No
physical aarch64 device was available at R2; the column is
carried, not waived. When a device joins the estate, this
same harness runs on it unchanged and its chain joins the
comparison as a fourth target.
