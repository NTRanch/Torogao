# Rung 2 / M1 — Sloane ratification report (analysis half)

**Stakes:** this report decides whether Sloane's regenerated arc clip
is fit to be her clip of record: D-1223 limb parity proven on her
own rig against her own Blender witness, at the ratified bounds.

**Status:** analysis half complete, all gates inside their bounds.
Renderer-side gates (replay hash, loader census, GPU-vs-CPU
crosscheck, twin renders) are **not run yet** — appended below when
the GPU queue opens. No Owner PASS is declared here.

Artifacts: primo
`~/.local/share/pax/torogao-renderer-rung2-20261110/m1/` (below:
`m1/`). Markdown: **not-LSP-validated** (no theodolite route).

## Inputs (sha256, `m1/sloane/input-hashes.txt`)

| Input | sha256 |
|---|---|
| `nsfix-stage/fleet/sloane/sloane.glb` (unedited) | `7d5bf2d3…` |
| `nsfix-stage/fleet/sloane/sloane.blend` (same hash after the dump) | `b86e5e8f…` |
| `arc-shows/sloane/sloane-arc.json` | `f40401be…` |
| `gen_clip.py` (D-1223) | `875ab1e5…` |
| `analyze_frames.py` | `839d52b1…` |
| `dump_bone_data.py` | `0414a237…` |

## Controls

1. Tool control and Prima regression: as recorded in
   `docs/rung2-m1/ALMA-RATIFICATION.md` (54/54 rows; clip
   `bb060717…`, skeleton `1c6087af…` byte-identical).
2. **Skeleton identity.** The def the fixed exporter derives from
   her fleet GLB is byte-identical to her supplied def.
3. **Show provenance.** The pre-fix exporter (`gen_clip.py.bak-20261110-d1223`) on the supplied show + fleet GLB reproduces the supplied pre-fix clip `817194f4…` byte-for-byte. (`m1/sloane/legacy/`)

## Outputs (`m1/sloane/output-hashes.txt`)

| Output | sha256 |
|---|---|
| `sloane.clip.json` (D-1223 keys, 900 frames) | `7a34ab01a1f24e6849e5324a07c3e5e3f120dae52420fb87a7c2c860fcf18f51` |
| `sloane-skeleton.json` | `4b682429d43e9e2dacec97b305edfbf26f56188ed11bfd6f6042cd771b0c5255` |
| `sloane-channels.json` | `3214d41fa67ef6cbbe33ebaf2bbcb1b9f447837fb921d053f5e65cf48317124d` |

## Gates (bounds ratified at the first-girl gate)

| Gate | Bound | Measured | Artifact |
|---|---|---|---|
| Twist: rest-data deviation from Rx(+90°) (Blender `matrix_local` vs GLB rest), worst of 54 | ≤ 0.05° | **0.0136°** (RightArm) | `m1/sloane/twist-sloane.txt` |
| Residual vs Blender, 60 show keys: worst bone max | ≤ 0.10° | **0.0434°** (LeftHandPinky1) | `m1/sloane/residual_table_sloane.md` |
| Residual: worst per-bone mean | ≤ 0.05° | **0.0258°** (LeftHandPinky1) — 48% headroom | same |
| Joint placement, frame 0, 54 joints | ≤ 1e-4 m | **2.81e-07 m** (RightHandPinky3) | `m1/sloane/joint-placement-sloane.txt` |

All four inside. Before → after, left chain (full 54-row table in
the artifact): UpLeg 180.0000° → 0.0021°; Leg 179.5758° → 0.0254°; Foot 146.7345° → 0.0226°;
Arm 135.3191° → 0.0000°; ForeArm 124.6232° → 0.0000°; Eye 180.0000° → 0.0342°.
Hips 0.0123° before and 0.0123° after.

## Not run yet (GPU queue)

Replay hash, loader census parity, GPU-vs-CPU crosscheck (positions
≤ 1e-4 m, normals ≤ 1e-3), twin 360-frame character-only renders at
the plate camera and rig, saved morph weights.
