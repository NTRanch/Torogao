# Rung 2 / M1 — cast-wide parity: close-out report

**Status at commit time: M1 is NOT closed.** The analysis half and
the Rust step are complete and gated on CPU. Every GPU gate is
**running detached and unread** — none of its results is asserted
here. Theodolite is **REVIEW**. No Owner PASS is declared.

Evidence root: primo
`~/.local/share/pax/torogao-renderer-rung2-20261110/m1/` (below:
`m1/`). This file is Markdown: **not-LSP-validated**.

## 1. Gate table — analysis half (CPU, complete)

Bounds stated before the runs and unchanged: twist ≤ 0.05°; residual
worst bone max ≤ 0.10°; worst per-bone mean ≤ 0.05°; joint placement
(frame 0) ≤ 1e-4 m.

| Girl | Twist ° | Worst max ° | Worst mean ° | Joint m | Provenance | Result |
|---|---|---|---|---|---|---|
| Prima (control) | 0.0228 | 0.0560 | 0.0429 | 2.80e-7 | regression byte-identical | inside |
| Alma | 0.0082 | 0.0540 | 0.0480 | 3.21e-7 | proof passed | inside |
| Lux | 0.0287 | 0.0597 | 0.0443 | 3.33e-7 | proof passed | inside |
| Piper | 0.0285 | 0.0476 | 0.0389 | 1.99e-7 | proof passed | inside |
| Sloane | 0.0136 | 0.0434 | 0.0258 | 2.81e-7 | proof passed | inside |
| Yui | 0.0235 | 0.0592 | 0.0423 | 4.67e-7 | **exception** (§5) | inside |

Tightest margin: Alma's worst per-bone mean, 0.0480° against 0.05°.
Artifacts per girl: `m1/<girl>/residual_table_<girl>.md`,
`twist-<girl>.txt`, `joint-placement-<girl>.txt`; Prima:
`m1/tool-control/`. Per-girl reports:
`docs/render3d/m1/{LUX,PIPER,SLOANE,YUI}-RATIFICATION.md` and
`docs/rung2-m1/ALMA-RATIFICATION.md` (Alma's was committed at that
path first and is not moved).

**Twist — two quantities, kept apart.** The gate quantity above is
the rest-data deviation from Rx(+90°) (Blender `matrix_local` vs GLB
rest, `m1/tool-control/twist_dev.py`). The addendum's 0.0198°
(LeftShoulder) is a different, historical quantity; it was not
re-measured and is not a gate number here.

## 2. Gate table — renderer side, CPU (complete)

Inputs staged at `m1/<girl>/inputs/`: fleet GLB copied unedited,
regenerated clip / def / channels, and `plate-camera.json`
(`36962c49…`, the camera rows of rung-1's `scene-probe.json`,
identical on every probed frame).

| Girl | GLB | Clip | Def | Replay hash | Loader census = fleet GLB | Outfit plan | Bust weight |
|---|---|---|---|---|---|---|---|
| Prima | `822afd99…` | `bb060717…` | `1c6087af…` | `1bd9dd534a0ea37f8e827e5d9e7fcdcac3d8cebfd53401c3cd05b9be46e986eb` | yes (4 prims, 22,340 verts) | `0:1,90:2` | 1 (D-1247) |
| Alma | `4011cbd5…` | `07dd6908…` | `b8038d0c…` | `263d6ced318a6319e80dafb8df4ae809f7a1ae34c39a63609dec2fe1afb54bc7` | yes (4, 25,799) | `0:1` | as saved (0) |
| Lux | `5400cd56…` | `a07c5809…` | `da537c68…` | `ce0d20ed5f66672dd830fa62835de73f9d290022ddaf8a92d51ee0128949d3aa` | yes (5, 21,260) | `0:1,90:2` | as saved (0) |
| Piper | `4c5e1872…` | `8e67c54b…` | `f4bdb4ea…` | `add3a87df5404180fbbc881345947294c2032539a6637180fdea3abdcd154e22` | yes (4, 19,440) | `0:2` | as saved (0) |
| Sloane | `7d5bf2d3…` | `7a34ab01…` | `4b682429…` | `df8cc71c5610fab7bf694693e1ab1caa3b319dc93d429e5123605b82e6e113a1` | yes (4, 22,233) | `0:2` | as saved (0) |
| Yui | `b55fec36…` | `3c47f406…` | `bfd41873…` | `e3ff8bbe9b163a5d22f01a16f4c1ed1977b37af7b831de8a88a9e62b107c9886` | yes (4, 18,604) | `0:2` | as saved (0) |

All six: 54 joints, 59 nodes, 900 clip frames. Prima's replay hash
through the new loader equals the rung-1 hash of record. The five
other hashes are **first recordings**, not comparisons against a
prior record; they are pinned in `tests/proving.rs`.
Artifacts: `m1/<girl>/cast-info.json`, `census-parity.json`
(`m1/census_parity.py` reads the glTF JSON independently).

## 3. Rust step

Commits on `pax/renderer-rung2-20261110`:

- `d3f1d4a` — `src/cast.rs` (cast-generic loader, typed errors),
  `src/bin/cast.rs` (`render3d-cast info | crosscheck | render`),
  `src/setscene.rs` (plate rig takes the camera as data;
  `prima_arc_scene` output unchanged), 11 unit tests + 8 proving
  tests (half validation, half adversarial).
- `b5815db` — **clippy fix, pre-existing debt from rung-1 commit
  `7b9297b`**: `needless_range_loop` in `morph_spot_fixtures`
  (`tests/proving.rs`), index loop → zip, same comparisons and
  tolerance. Ruled curative at gate review.

| Gate | Result |
|---|---|
| `cargo test -p torogao-render3d` (unit) | 26 passed |
| `cargo test -p torogao-motion-clip` | all suites passed (67 / 22 / 19) |
| CPU proving tests (`--ignored --skip gpu_smoke`), after `b5815db` | 15 passed, 2 GPU tests not run |
| `cargo clippy -p torogao-render3d --all-targets -- -D warnings` | passes at `b5815db` (failed at `d3f1d4a` on the one lint above) |
| `cargo fmt --check` | clean |
| Prima regression after the change (fleet GLB → clip, skeleton) | byte-identical (`m1/prima-regression/post-rust/`) |
| Theodolite | **REVIEW** — see below |

**Theodolite = REVIEW.** Planted E0308 on `src/cast.rs` was seen
(2 error diagnostics). After restore, `cast.rs`, `bin/cast.rs`,
`setscene.rs`, `lib.rs` read 0 and `tests/proving.rs` read only the
lint since fixed — but every read carried
`possibly_incomplete: server_not_quiescent`, so these are not
verified zeros. `tests/proving.rs` was not re-read after `b5815db`.
Pax re-runs the cured protocol (producer first, quiescent read) on
the committed tree at the close-out gate.

## 4. GPU gates — running detached, results not yet read

- tmux session on primo: **`pax-m1-gpu-queue`**; script
  `m1/gpu_queue.sh`; log `m1/gpu-queue.log`. Launched 15:57 on
  2026-11-10. It waits for the plates-v2 arc (Blender pid 4017864)
  to exit before any GPU work; at launch that process was alive.
- Step 0: full proving suite, 17 tests (Prima rung-1 path, 9, plus
  the 8 cast tests incl. GPU smoke) → `m1/proving-suite.log`,
  done-file `m1/PROVING-DONE` or `m1/PROVING-FAILED`.
- Then, one girl at a time — Alma, Lux, Piper, Sloane, Yui, Prima
  (Prima with `--bust-weight 1`): crosscheck on frames
  0/90/180/225/270/359 (bound: positions ≤ 1e-4 m, normals ≤ 1e-3),
  twin 360-frame character-only renders at the plate camera and rig,
  then `m1/twin_compare.py` (360/360 RGBA SHA-256 identical).
- **Done-file convention, in `m1/<girl>/`:** `GPU-DONE` = every gate
  of that girl ran and passed; `GPU-FAILED` = a gate failed or
  errored. On the first failure the queue **stops** and writes
  `m1/GPU-QUEUE-STOPPED`; on completion `m1/GPU-QUEUE-DONE`.
  Per-girl outputs: `crosscheck.json`, `twin-a/`, `twin-b/`
  (PNGs + `manifest.json`), `twin-compare.json`, `gpu-run.log`.
- A done-file records the scripts' exit status only. The numbers in
  `crosscheck.json` and `twin-compare.json` still need reading at
  the gate. Whatever the queue leaves ungated is taken over by Pax.
- The cast render path has never run on the GPU before this queue.

## 5. Yui provenance exception (ruled)

The pre-fix exporter on the supplied show does not reproduce the
supplied pre-fix clip (`f872d425…` regenerated vs `c365561e…`
supplied). The difference is confined to `mixamorig_Hips`
translation: 31 of 60 keys, frames 435–885, y by −0.05 / −0.1 /
−0.100044. The show generator re-run is JSON-equal to the supplied
show (not byte-identical — true of all six girls). Disposition as
ruled: supplied clip superseded and kept on disk; the regenerated
clip is the clip of record. **Cause not fully determined.**
(`m1/yui/provenance-diff.txt`, `m1/yui/gen-show-check/`)

## 6. Prima bust — record

My statement in the gate plan, verbatim: "Prima's plates were
rendered from the rung-1 variant".

The imprecision: I asserted this from the saved weight in the
rung-1 GLB copy (`Body_Bust_Size` = 1) without checking what the
rung-1 renderer applies. It overrides the saved value to 0
(`src/arc.rs`, `PLATE_BUST_WEIGHT = 0.0`, asserted by the test
`bust_override_applies`). The imprecision concerns that renderer
override. It is not a statement about the show generator's
mechanism (bust parameter driving the shape key, 1.0 = m 1.50),
which I did not mis-state.

Corrected fact, per gate review's own measurement of the D-1247
chain (GLB mesh weights, both source blends, the morph delta): the
applied render value for Prima is **1**. Measured by gate review,
not by me. Fleet `prima.glb` saves 0, so her cast gates pass
`--bust-weight 1` explicitly; the choice is echoed in every
artifact's identity block.

Left as it stands, named for the gate: the rung-1 proving path
still renders Prima at 0 through `PLATE_BUST_WEIGHT`. Step 0 of the
queue exercises that path unchanged; this report makes no change
to it.

## 7. Kept files, not used

`m1/alma-blocker/probe_rekey.py` and `prima-rekeyed.clip.json` (the
derived re-key route: not authorized, not used). Left in place.
Nothing under `m1/` was deleted.

## 8. What remains unproven

- Every GPU gate in §4, all six girls, until read on disk.
- Theodolite verified zero (§3).
- Family B intake (Vivienne: atlas decode, UV sampling, verdict pack
  against the D-1206 pack): treated as not authorized in this
  session; not started. M1 as the brief defines it is not closed
  without it.
- Visual verdict: no pack was assembled for the Owner's eye; the
  twin renders are determinism evidence, not a plate comparison.
- The cause of the Yui clip difference (§5).

## 9. Skill outcomes (for Pax to log)

- `local-tiger-style-rust`: materially used for `cast.rs`,
  `bin/cast.rs`, and the proving tests. Outcome: compiles on the
  pinned nightly, clippy `-D warnings` and fmt clean, 26 unit and
  15 CPU proving tests pass. Verification strength: compiler +
  tests; GPU behaviour of the new path unverified at commit time.
- No other custom skill was materially used.

## 10. Tree state

Only `crates/torogao-cli/src/registry_data.rs` is uncommitted, as
found. No pushes. Build output under `target/` is left in place:
the detached queue runs the release `render3d-cast` and the
proving-suite test binary from it.
