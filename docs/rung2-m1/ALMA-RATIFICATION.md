# Rung 2 / M1 — Alma ratification report (first-girl gate)

**Stakes:** this report decides whether the M1 bounds (brief
Appendix B item 7, proposals until now) are ratified for the
remaining four girls, and whether Prima's D-1223 limb fix holds on
a second rig without any per-rig constant.

**Status:** analysis half complete, all gates inside their bounds.
GPU half and Rust-side gates are **not run** (held: A6000 busy with
the plates-v2 arc; Rust generalization held until this report is
accepted). No Owner PASS is declared here.

All artifacts: primo
`~/.local/share/pax/torogao-renderer-rung2-20261110/m1/`
(below: `m1/`). This file is Markdown: **not-LSP-validated** (no
theodolite route). No Rust file was touched.

## Inputs (sha256, `m1/alma/input-hashes.txt`)

| Input | sha256 |
|---|---|
| `nsfix-stage/fleet/alma/alma.glb` (vintage of record, unedited) | `4011cbd5…` |
| `nsfix-stage/fleet/alma/alma.blend` (identical before and after the dump) | `3e2cd41d…` |
| `arc-shows/alma/alma-arc.json` | `2d22b861…` |
| `gen_clip.py` (D-1223, both live copies) | `875ab1e5…` |
| `analyze_frames.py` | `839d52b1…` |
| `dump_bone_data.py` | `0414a237…` |

## Controls run before Alma

1. **Tool control (Prima).** `analyze_frames.py` on the primo dump
   reproduces `residual_table.md` in **54 of 54 rows** at four
   decimals (before max, after max, after mean); worst max 0.0560°,
   worst per-bone mean 0.0429°. Fleet GLB and rung-1 GLB give
   byte-identical analysis output.
   (`m1/tool-control/compare-vs-residual-table.txt`)
2. **Prima regression.** Fixed gen_clip on the fleet GLB +
   `prima-arc.json` gives clip `bb060717…`, skeleton `1c6087af…`,
   channels — each **byte-identical** to rung-1 inputs-v2.
   (`m1/prima-regression/`)
3. **Show provenance (Alma).** The pre-fix exporter
   (`gen_clip.py.bak-20261110-d1223`) on the supplied show + fleet
   GLB reproduces the supplied pre-fix clip `8bf6ff2e…`
   byte-for-byte: the show is the one her clip of record came from.
   (`m1/alma/legacy/`)

## Alma — outputs

| Output | sha256 |
|---|---|
| `alma.clip.json` (D-1223 keys, 900 frames, 49 body / 14 face tracks) | `07dd690834ac3bd4ac715dec20d5bf9ce7fe6fee00b0159f51eaef8d6af8b3a3` |
| `alma-skeleton.json` (identical to the supplied def) | `b8038d0c58073383786935ecb5364d38ec23cb165335ea091044255c8283faaf` |
| `alma-channels.json` (identical to the supplied file) | `8122a116369206d7967d341a38fd9ff4e624a173bdcc527473a9c9b05686ed5c` |

## Alma — gates (bounds stated before the run)

| Gate | Bound | Measured | Result | Artifact |
|---|---|---|---|---|
| Frame gate: twist deviation from Rx(+90°), worst of 54 bones | ≤ 0.05° | **0.0082°** (Left/RightArm) | inside | `m1/alma/twist-alma.txt` |
| Residual vs Blender, 60 show keys: worst bone max | ≤ 0.10° | **0.0540°** (Left/RightToeBase) | inside | `m1/alma/residual_table_alma.md` |
| Residual: worst per-bone mean | ≤ 0.05° | **0.0480°** (Left/RightToeBase) | inside, **4% headroom** | same |
| Joint placement, frame 0, 54 joints | ≤ 1e-4 m | **3.21e-7 m** (Left/RightHandPinky3) | inside | `m1/alma/joint-placement-alma.txt` |
| Blender keyframe cross-check (dump tool's own) | 0 | 0.0 | — | `m1/alma/dump.log` |

Before → after, headline chains (full 54-row table in the
artifact): UpLeg 179.9649° → 0.0460°, Leg 179.5824° → 0.0426°,
Foot 136.8476° → 0.0459°, ToeBase 103.8148° → 0.0540°, Arm
135.0213° → 0.0000°, ForeArm 124.5693° → 0.0000°, Eyes 180.0000° →
0.0342°. Axial chain unchanged by the fix, as required: Hips
0.0123°, Spine 0.0171°, Head 0.0331° before and after.

Joint-placement control: the same script on Prima gives 2.80e-7 m
worst (rung 1 reported 0.0000 m at four decimals).
(`m1/tool-control/joint-placement-prima.txt`)

## Bound ratification — recommendation

- **Worst max ≤ 0.10°:** ratify. Two rigs, 0.0560° and 0.0540°.
- **Worst per-bone mean ≤ 0.05°:** it did not bind, but it is the
  tight one — Prima 0.0429° (UpLeg), Alma 0.0480° (ToeBase). Both
  sit at the f32 GLB-storage noise class, not at a pose error. With
  four rigs still to run, a noise-floor residual could cross 0.05°
  without any defect. Not relaxed here; flagged for a ruling: keep
  0.05° and treat a crossing as stop-and-report, or revise now with
  this evidence.
- **Twist ≤ 0.05°, joint placement ≤ 1e-4 m:** ratify.

## Twist quantities — two names, kept apart

- **Frame-basis conversion deviation (addendum, item-5 analysis):**
  0.0198° worst, LeftShoulder, Prima. Not re-measured here; the
  code that produced it is not among the supplied scripts.
- **Per-bone twist deviation (`m1/tool-control/twist_dev.py`), the
  quantity this gate binds:** angle of `T_b · Rx(+90°)⁻¹` with
  `T_b = W_b⁻¹ · (G A_b G⁻¹)`. Prima 0.0228° (Thumb2), Alma
  0.0082° (Arm). Note for the record: the script computes this
  from **rest** data (Blender `matrix_local` and GLB rest worlds);
  it does not read posed matrices.

## Not run (owed after acceptance)

- Alma replay hash: the only replay path in the tree
  (`load_arc_inputs`) is Prima-named and needs the held Rust
  generalization.
- Census parity through the renderer loader, GPU-vs-CPU crosscheck,
  twin 360-frame renders (plate camera and rig, no set, D-1247 bust
  rule applying to Prima only).
- Prima proving suite 9/9 and replay hash `1bd9dd53…` on inputs-v2
  (waits for the A6000).

## Helper scripts written for this gate

`m1/tool-control/`: `cmp_table.py` (row compare vs
`residual_table.md`), `twist_dev.py`, `joint_placement.py`;
`m1/alma/summarize.py`. The latter two import
`analyze_frames.py`'s own loaders and math. Stdlib + numpy; not
Blender-side.

## Skill outcomes

No custom skill was materially used in this gate (no Rust written,
so Tiger Style was not exercised).

## Kept on a judgement call

`m1/alma-blocker/` (the legacy-clip re-key probe from the show-JSON
blocker: derived route proven exact on Prima, **not authorized, not
used**). Delete on the Owner's word.
