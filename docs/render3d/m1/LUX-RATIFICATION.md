# Rung 2 / M1 — Lux ratification report (analysis half)

**Stakes:** this report decides whether Lux's regenerated arc clip
is fit to be her clip of record: D-1223 limb parity proven on her
own rig against her own Blender witness, at the ratified bounds.

**Status:** analysis half complete, all gates inside their bounds.
Renderer-side gates (replay hash, loader census, GPU-vs-CPU
crosscheck, twin renders) are **not run yet** — appended below when
the GPU queue opens. No Owner PASS is declared here.

Artifacts: primo
`~/.local/share/pax/torogao-renderer-rung2-20261110/m1/` (below:
`m1/`). Markdown: **not-LSP-validated** (no theodolite route).

## Inputs (sha256, `m1/lux/input-hashes.txt`)

| Input | sha256 |
|---|---|
| `nsfix-stage/fleet/lux/lux.glb` (unedited) | `5400cd56…` |
| `nsfix-stage/fleet/lux/lux.blend` (same hash after the dump) | `d7ce10e5…` |
| `arc-shows/lux/lux-arc.json` | `632e1f2d…` |
| `gen_clip.py` (D-1223) | `875ab1e5…` |
| `analyze_frames.py` | `839d52b1…` |
| `dump_bone_data.py` | `0414a237…` |

## Controls

1. Tool control and Prima regression: as recorded in
   `docs/rung2-m1/ALMA-RATIFICATION.md` (54/54 rows; clip
   `bb060717…`, skeleton `1c6087af…` byte-identical).
2. **Skeleton identity.** The def the fixed exporter derives from
   her fleet GLB is byte-identical to her supplied def.
3. **Show provenance.** The pre-fix exporter (`gen_clip.py.bak-20261110-d1223`) on the supplied show + fleet GLB reproduces the supplied pre-fix clip `39ee6eab…` byte-for-byte. (`m1/lux/legacy/`)

## Outputs (`m1/lux/output-hashes.txt`)

| Output | sha256 |
|---|---|
| `lux.clip.json` (D-1223 keys, 900 frames) | `a07c580980d948297715628708c3bf4f2cde7086a82ccaf806b5ed1ebc870098` |
| `lux-skeleton.json` | `da537c688353f447590700cb9d73764b7c8fcad561722d3553966fae04d5d22d` |
| `lux-channels.json` | `3214d41fa67ef6cbbe33ebaf2bbcb1b9f447837fb921d053f5e65cf48317124d` |

## Gates (bounds ratified at the first-girl gate)

| Gate | Bound | Measured | Artifact |
|---|---|---|---|
| Twist: rest-data deviation from Rx(+90°) (Blender `matrix_local` vs GLB rest), worst of 54 | ≤ 0.05° | **0.0287°** (RightFoot) | `m1/lux/twist-lux.txt` |
| Residual vs Blender, 60 show keys: worst bone max | ≤ 0.10° | **0.0597°** (LeftHandThumb1) | `m1/lux/residual_table_lux.md` |
| Residual: worst per-bone mean | ≤ 0.05° | **0.0443°** (RightHandThumb1) — 11% headroom | same |
| Joint placement, frame 0, 54 joints | ≤ 1e-4 m | **3.33e-07 m** (RightHandPinky3) | `m1/lux/joint-placement-lux.txt` |

All four inside. Before → after, left chain (full 54-row table in
the artifact): UpLeg 180.0000° → 0.0338°; Leg 179.5752° → 0.0357°; Foot 136.3283° → 0.0318°;
Arm 135.2890° → 0.0397°; ForeArm 124.6178° → 0.0387°; Eye 180.0000° → 0.0312°.
Hips 0.0123° before and 0.0123° after.

## Not run yet (GPU queue)

Replay hash, loader census parity, GPU-vs-CPU crosscheck (positions
≤ 1e-4 m, normals ≤ 1e-3), twin 360-frame character-only renders at
the plate camera and rig, saved morph weights.
