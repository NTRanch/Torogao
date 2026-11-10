# Rung 2 / M1 — Yui ratification report (analysis half)

**Stakes:** this report decides whether Yui's regenerated arc clip
is fit to be her clip of record: D-1223 limb parity proven on her
own rig against her own Blender witness, at the ratified bounds.

**Status:** analysis half complete, all gates inside their bounds.
Renderer-side gates (replay hash, loader census, GPU-vs-CPU
crosscheck, twin renders) are **not run yet** — appended below when
the GPU queue opens. No Owner PASS is declared here.

Artifacts: primo
`~/.local/share/pax/torogao-renderer-rung2-20261110/m1/` (below:
`m1/`). Markdown: **not-LSP-validated** (no theodolite route).

## Inputs (sha256, `m1/yui/input-hashes.txt`)

| Input | sha256 |
|---|---|
| `nsfix-stage/fleet/yui/yui.glb` (unedited) | `b55fec36…` |
| `nsfix-stage/fleet/yui/yui.blend` (same hash after the dump) | `d766d925…` |
| `arc-shows/yui/yui-arc.json` | `c5b07fb8…` |
| `gen_clip.py` (D-1223) | `875ab1e5…` |
| `analyze_frames.py` | `839d52b1…` |
| `dump_bone_data.py` | `0414a237…` |

## Controls

1. Tool control and Prima regression: as recorded in
   `docs/rung2-m1/ALMA-RATIFICATION.md` (54/54 rows; clip
   `bb060717…`, skeleton `1c6087af…` byte-identical).
2. **Skeleton identity.** The def the fixed exporter derives from
   her fleet GLB is byte-identical to her supplied def.
3. **Show provenance — EXCEPTION (ruled at gate review).** See the section below.

## Outputs (`m1/yui/output-hashes.txt`)

| Output | sha256 |
|---|---|
| `yui.clip.json` (D-1223 keys, 900 frames) | `3c47f4067b65cb8aeb1385145e21740379a35d196384f1b5ff0f9c4f7748e981` |
| `yui-skeleton.json` | `bfd41873c5593867ab8c7370b51113b345ec906859a3402d372306d018159796` |
| `yui-channels.json` | `8122a116369206d7967d341a38fd9ff4e624a173bdcc527473a9c9b05686ed5c` |

## Gates (bounds ratified at the first-girl gate)

| Gate | Bound | Measured | Artifact |
|---|---|---|---|
| Twist: rest-data deviation from Rx(+90°) (Blender `matrix_local` vs GLB rest), worst of 54 | ≤ 0.05° | **0.0235°** (RightHandThumb2) | `m1/yui/twist-yui.txt` |
| Residual vs Blender, 60 show keys: worst bone max | ≤ 0.10° | **0.0592°** (LeftHandPinky3) | `m1/yui/residual_table_yui.md` |
| Residual: worst per-bone mean | ≤ 0.05° | **0.0423°** (RightHand) — 15% headroom | same |
| Joint placement, frame 0, 54 joints | ≤ 1e-4 m | **4.67e-07 m** (RightHandRing2) | `m1/yui/joint-placement-yui.txt` |

All four inside. Before → after, left chain (full 54-row table in
the artifact): UpLeg 179.9811° → 0.0357°; Leg 179.5513° → 0.0370°; Foot 134.9108° → 0.0450°;
Arm 136.0421° → 0.0494°; ForeArm 124.7445° → 0.0329°; Eye 180.0000° → 0.0278°.
Hips 0.0123° before and 0.0123° after.

## Provenance exception — full record

- **Measured mismatch** (`m1/yui/provenance-diff.txt`): the pre-fix
  exporter on the supplied show yields `f872d425…`; the supplied
  pre-fix clip is `c365561e…`. One track differs:
  `mixamorig_Hips` translation, **31 of 60 keys** (clip frames
  435–885), y component only, regenerated minus supplied = −0.05,
  −0.1 or −0.100044; **max 0.100044**. Every rotation key, the
  face, camera and hands groups, and the skeleton are identical.
- **Generator corroboration** (`m1/yui/gen-show-check/`,
  `generator-check.txt`): `gen_show.py` (`c5fe33cd…`) regenerates
  her arc show with **equal parsed content** to the supplied
  `yui-arc.json`. The files are **not byte-identical** (regenerated
  `5ef355fb…` vs supplied `c5b07fb8…`); the same holds for all six
  girls, so the byte difference is not Yui-specific.
- **Disposition (gate review):** the supplied historical clip
  `c365561e…` is retired as her provenance anchor — **superseded,
  file kept in place, not deleted**. Her clip of record is the
  fixed exporter's output from the supplied show, as for every
  other girl.
- **Cause of the stale Hips track: not fully determined.** Nothing
  here establishes when or why the supplied clip's hips keys
  diverged from the show.
- Consequence to carry: her supplied `replay-proof.json`
  (`1932a66d…`) and `anim_gate.json` describe the superseded clip.

## Not run yet (GPU queue)

Replay hash, loader census parity, GPU-vs-CPU crosscheck (positions
≤ 1e-4 m, normals ≤ 1e-3), twin 360-frame character-only renders at
the plate camera and rig, saved morph weights.
