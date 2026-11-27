# Rung 2 / M1 — Piper ratification report (analysis half)

**Stakes:** this report decides whether Piper's regenerated arc clip
is fit to be her clip of record: D-1223 limb parity proven on her
own rig against her own Blender witness, at the ratified bounds.

**Status:** analysis half complete, all gates inside their bounds.
Renderer-side gates (replay hash, loader census, GPU-vs-CPU
crosscheck, twin renders) are **not run yet** — appended below when
the GPU queue opens. No Owner PASS is declared here.

Artifacts: primo
`~/.local/share/pax/torogao-renderer-rung2-20261110/m1/` (below:
`m1/`). Markdown: **not-LSP-validated** (no theodolite route).

## Inputs (sha256, `m1/piper/input-hashes.txt`)

| Input | sha256 |
|---|---|
| `nsfix-stage/fleet/piper/piper.glb` (unedited) | `4c5e1872…` |
| `nsfix-stage/fleet/piper/piper.blend` (same hash after the dump) | `07873e14…` |
| `arc-shows/piper/piper-arc.json` | `1f9aa504…` |
| `gen_clip.py` (D-1223) | `875ab1e5…` |
| `analyze_frames.py` | `839d52b1…` |
| `dump_bone_data.py` | `0414a237…` |

## Controls

1. Tool control and Prima regression: as recorded in
   `docs/rung2-m1/ALMA-RATIFICATION.md` (54/54 rows; clip
   `bb060717…`, skeleton `1c6087af…` byte-identical).
2. **Skeleton identity.** The def the fixed exporter derives from
   her fleet GLB is byte-identical to her supplied def.
3. **Show provenance.** The pre-fix exporter (`gen_clip.py.bak-20261110-d1223`) on the supplied show + fleet GLB reproduces the supplied pre-fix clip `5eacdf4a…` byte-for-byte. (`m1/piper/legacy/`)

## Outputs (`m1/piper/output-hashes.txt`)

| Output | sha256 |
|---|---|
| `piper.clip.json` (D-1223 keys, 900 frames) | `8e67c54b578b02ed4c05cefec644d8443fd7b3aedcd059fbb7ce09cff0975c50` |
| `piper-skeleton.json` | `f4bdb4eaa72c84baa52f1a89b21777e346c36ad1d45e6aff7f9533132b27cf8f` |
| `piper-channels.json` | `3214d41fa67ef6cbbe33ebaf2bbcb1b9f447837fb921d053f5e65cf48317124d` |

## Gates (bounds ratified at the first-girl gate)

| Gate | Bound | Measured | Artifact |
|---|---|---|---|
| Twist: rest-data deviation from Rx(+90°) (Blender `matrix_local` vs GLB rest), worst of 54 | ≤ 0.05° | **0.0285°** (RightHandRing3) | `m1/piper/twist-piper.txt` |
| Residual vs Blender, 60 show keys: worst bone max | ≤ 0.10° | **0.0476°** (LeftFoot) | `m1/piper/residual_table_piper.md` |
| Residual: worst per-bone mean | ≤ 0.05° | **0.0389°** (LeftFoot) — 22% headroom | same |
| Joint placement, frame 0, 54 joints | ≤ 1e-4 m | **1.99e-07 m** (RightEye) | `m1/piper/joint-placement-piper.txt` |

All four inside. Before → after, left chain (full 54-row table in
the artifact): UpLeg 179.9885° → 0.0252°; Leg 179.5915° → 0.0385°; Foot 137.3921° → 0.0476°;
Arm 134.7451° → 0.0025°; ForeArm 124.5174° → 0.0000°; Eye 180.0000° → 0.0278°.
Hips 0.0123° before and 0.0123° after.

## Not run yet (GPU queue)

Replay hash, loader census parity, GPU-vs-CPU crosscheck (positions
≤ 1e-4 m, normals ≤ 1e-3), twin 360-frame character-only renders at
the plate camera and rig, saved morph weights.
