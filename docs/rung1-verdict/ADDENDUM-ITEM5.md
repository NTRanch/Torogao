# Item 5 addendum — limb articulation (D-1223 fix in gen_clip)

**Stakes:** rung 1's item 5 was the one structural divergence in the
renderer verdict: the clip's rotation data was written in the wrong
frame, so every limb bone posed wrong while the pipeline around it
was exact. This addendum re-proves item 5 after the Owner-ruled fix
(D-1223: fix in gen_clip; no renderer change, no def v2, no per-bone
correction table). The Owner rules item 5 closed or not, by eye, on
the pair frames in `item5/pack/`.

## What changed

- **gen_clip only.** Both live copies —
  `~/.local/share/pax/nsd1/fleet/gen_clip.py` and
  `~/.local/share/pax/night-school/plan-20/fleet/gen_clip.py` on
  primo — now write each body track's rotation key as the bone's
  **posed local rotation in the glTF node frame**:
  `key = normalize(rest ⊗ delta)`, where `rest` is the GLB skin
  node's rest rotation and `delta` is the show euler converted
  exactly as before. Deployed sha256
  `875ab1e570f61a417b804306ac51ac9b5187e8fbac9e2d4b5f100e91c02aa8b8`
  (both copies identical; pre-fix copies kept as
  `gen_clip.py.bak-20261110-d1223` beside each).
- **Why `rest ⊗ delta` is the general change of basis, not a fit:**
  the full form is `q* = r · T · (G Δ G⁻¹) · T⁻¹` with
  `T_b = W_b⁻¹ · (G A_b G⁻¹)` the per-bone twist between the GLB
  joint frame (W from node worlds / inverse bind matrices) and the
  converted Blender bone frame (A = `bone.matrix_local`;
  G = the (x, z, −y) witness conversion). Measured on Prima's rig,
  T_b = Rx(+90°) = G⁻¹ for **all 54 bones** (max deviation 0.0198°,
  worst LeftShoulder), under which the general form collapses
  exactly to `rest ⊗ delta` — no per-bone constants, no bone-name
  special cases. The measurement is re-derived per rig by the frame
  gate (`dump_bone_data.py` + `analyze_frames.py`, item-5 staging);
  the exporter documents this in its header.
- **Semantics it satisfies:** the motion-clip evaluator seats a bone
  at its def rest and then *replaces* the rotation with the track
  value, so the key must carry the full posed local rotation —
  exactly what Blender composes as rest local × matrix_basis.
  Writing the raw delta was the defect: bones with identity rest
  (the axial chain) were unaffected; limb bones were wrong by their
  rest rotation.
- **Unchanged:** clip format, evaluator, renderer, the def
  (skeleton sha256 stays
  `1c6087af79f67c63d719b24ae4625260f75372dafcdba9d5e8d4d833abee7638`),
  face tracks, channels, and the Hips track (identity rest — its
  keys, translation passthrough included, are byte-identical to the
  legacy export; proven in the gate tests, not assumed). The hips
  translation binding rule in pose.rs was not touched and does not
  interact: the change is rotation-only.
- **New clip:** primo staging
  `~/.local/share/pax/torogao-renderer-rung1-20261110/inputs-v2/`
  (clip sha256
  `bb0607176c22c56c2c4852889eab1d54651aa75945a0afb9cbb655acadbab2a3`;
  rung-1 inputs with the old clip, sha256 `47936b39…`, kept intact).
- **New arc replay hash:
  `1bd9dd534a0ea37f8e827e5d9e7fcdcac3d8cebfd53401c3cd05b9be46e986eb`**
  (was `84b8c442a16d66f17a4faf2b8460727be6649411dc3836771c67ad2df66ca8a4`).
  The proving suite's expected value was updated to the new hash —
  that one assertion only, every other assertion unchanged —
  committed locally on branch `pax/renderer-rung1-20261110` as
  `63896dd`. No pushes.

## Residuals — item 5's measurement, before → after

World deformation-operator residuals vs Blender over all 60 show
key frames (renderer joint-matrix replica vs Blender posed
`bone.matrix` deformation, converted by G). Full per-bone table
below; headline chains:

- Leg chain: UpLeg 179.9864° → 0.0475°, Leg 179.5092° → 0.0260°,
  Foot 134.5578° → 0.0273°, ToeBase 100.7048° → 0.0384°.
- Arm chain: Arm 135.5692° → 0.0411°, ForeArm 124.6667° → 0.0352°,
  fingers 128–147° → ≤ 0.0560°.
- Eyes: 180.0000° → 0.0312° (untracked-in-show bones were also
  carried wrong by their rest; now exact by construction).
- Axial chain unchanged, as required: Hips 0.0123°, Spine 0.0000°,
  Spine1 0.0109°, Neck 0.0221°, Head 0.0264° — identical before and
  after.
- **Worst bone after the fix: 0.0560°** (Left/RightHandPinky2) —
  the f32 GLB-storage noise floor, the same class the axial chain
  sat at before. Evidence: `analysis_out.txt` (before) and
  `analysis_after.txt` (after, run against the regenerated clip
  file itself) in `~/workspace/torogao-item5-fix-20261110/`.

| Bone | Before max° | After max° | After mean° |
|---|---:|---:|---:|
| mixamorig:Hips | 0.0123 | 0.0123 | 0.0053 |
| mixamorig:LeftUpLeg | 179.9864 | 0.0475 | 0.0429 |
| mixamorig:LeftLeg | 179.5092 | 0.0260 | 0.0148 |
| mixamorig:LeftFoot | 134.5578 | 0.0273 | 0.0160 |
| mixamorig:LeftToeBase | 100.7048 | 0.0384 | 0.0324 |
| mixamorig:RightUpLeg | 179.9864 | 0.0475 | 0.0429 |
| mixamorig:RightLeg | 179.5092 | 0.0260 | 0.0148 |
| mixamorig:RightFoot | 134.5578 | 0.0273 | 0.0160 |
| mixamorig:RightToeBase | 100.7048 | 0.0384 | 0.0324 |
| mixamorig:Spine | 0.0000 | 0.0000 | 0.0000 |
| mixamorig:Spine1 | 0.0109 | 0.0109 | 0.0006 |
| mixamorig:Spine2 | 0.0109 | 0.0109 | 0.0006 |
| mixamorig:LeftShoulder | 0.0000 | 0.0000 | 0.0000 |
| mixamorig:LeftArm | 135.5692 | 0.0411 | 0.0304 |
| mixamorig:LeftForeArm | 124.6667 | 0.0352 | 0.0196 |
| mixamorig:LeftHand | 124.6667 | 0.0402 | 0.0255 |
| mixamorig:LeftHandIndex1 | 131.3257 | 0.0462 | 0.0327 |
| mixamorig:LeftHandIndex2 | 134.6027 | 0.0444 | 0.0240 |
| mixamorig:LeftHandIndex3 | 138.8317 | 0.0531 | 0.0347 |
| mixamorig:LeftHandMiddle1 | 133.1342 | 0.0539 | 0.0360 |
| mixamorig:LeftHandMiddle2 | 136.4499 | 0.0438 | 0.0095 |
| mixamorig:LeftHandMiddle3 | 140.6435 | 0.0485 | 0.0128 |
| mixamorig:LeftHandPinky1 | 138.3822 | 0.0462 | 0.0294 |
| mixamorig:LeftHandPinky2 | 141.8855 | 0.0560 | 0.0424 |
| mixamorig:LeftHandPinky3 | 146.1505 | 0.0538 | 0.0361 |
| mixamorig:LeftHandRing1 | 135.4369 | 0.0342 | 0.0118 |
| mixamorig:LeftHandRing2 | 138.8658 | 0.0382 | 0.0145 |
| mixamorig:LeftHandRing3 | 143.1318 | 0.0481 | 0.0260 |
| mixamorig:LeftHandThumb1 | 128.2118 | 0.0430 | 0.0213 |
| mixamorig:LeftHandThumb2 | 129.2999 | 0.0427 | 0.0153 |
| mixamorig:LeftHandThumb3 | 130.1988 | 0.0477 | 0.0201 |
| mixamorig:Neck | 0.0221 | 0.0221 | 0.0033 |
| mixamorig:Head | 0.0264 | 0.0264 | 0.0054 |
| mixamorig:LeftEye | 180.0000 | 0.0312 | 0.0056 |
| mixamorig:RightEye | 180.0000 | 0.0312 | 0.0056 |
| mixamorig:RightShoulder | 0.0000 | 0.0000 | 0.0000 |
| mixamorig:RightArm | 135.5692 | 0.0411 | 0.0299 |
| mixamorig:RightForeArm | 124.6667 | 0.0352 | 0.0189 |
| mixamorig:RightHand | 124.6667 | 0.0402 | 0.0257 |
| mixamorig:RightHandIndex1 | 131.5211 | 0.0462 | 0.0317 |
| mixamorig:RightHandIndex2 | 134.7791 | 0.0444 | 0.0216 |
| mixamorig:RightHandIndex3 | 138.9732 | 0.0531 | 0.0334 |
| mixamorig:RightHandMiddle1 | 133.6033 | 0.0539 | 0.0373 |
| mixamorig:RightHandMiddle2 | 136.9154 | 0.0438 | 0.0126 |
| mixamorig:RightHandMiddle3 | 141.1020 | 0.0485 | 0.0176 |
| mixamorig:RightHandPinky1 | 138.9257 | 0.0462 | 0.0301 |
| mixamorig:RightHandPinky2 | 142.4554 | 0.0560 | 0.0422 |
| mixamorig:RightHandPinky3 | 146.7731 | 0.0538 | 0.0353 |
| mixamorig:RightHandRing1 | 135.9811 | 0.0342 | 0.0128 |
| mixamorig:RightHandRing2 | 139.3379 | 0.0382 | 0.0148 |
| mixamorig:RightHandRing3 | 143.6145 | 0.0481 | 0.0264 |
| mixamorig:RightHandThumb1 | 128.6620 | 0.0430 | 0.0214 |
| mixamorig:RightHandThumb2 | 129.6455 | 0.0427 | 0.0149 |
| mixamorig:RightHandThumb3 | 130.2470 | 0.0477 | 0.0209 |

## Re-proof with the unchanged renderer + evaluator

- **Proving suite** (`TOROGAO_RENDER3D_TEST_INPUTS=inputs-v2`,
  `--ignored`): **9/9 pass**, including `replay_hash_matches_record`
  at the new hash and `gpu_smoke_determinism_and_skin`.
- **Blender strain witnesses** (proving test, `--nocapture`):
  deltas **2.00e-7 … 5.62e-7 m** over frames 0/24/60/84 × verts
  72/229 — the same sub-micron class as rung 1.
- **Determinism:** twin full 360-frame renders (A6000) — per-frame
  RGBA SHA-256 **identical 360/360** (evidence-item5/run-a,
  run-b manifests on primo).
- **GPU-vs-CPU crosscheck:** max position delta **2.384e-7 m**,
  max normal delta 1.192e-7 over frames 0/90/180/225/270/359,
  against the bound stated before the run (positions ≤ 1e-4 m,
  normals ≤ 1e-3). (`item5/crosscheck.json`)
- **Plate comparison, 60 sampled frames** (`item5/stats.json`):
  character-masked mean |Δ| (RGB) **(56.0, 64.3, 57.9) →
  (43.4, 46.3, 43.3)**; the character region's mean *signed* Δ is
  now (40.3, 43.3, 40.9) against the set region's (42.1, 42.4,
  40.2) — i.e. what remains on the character is the same uniform
  light-level offset the set shows, which is item 6, not pose.
  Full-frame mean signed Δ barely moved, (42.5, 43.5, 41.7) →
  (41.9, 42.6, 40.4): item 6 untouched, as required. (The
  character-mask pixel count differs between runs because the mask
  is the char-only silhouette, whose pose changed.)
- **Pair frames** (`item5/pack/`, same 2× format as rung 1):
  f0000 arms and f0180 limbs now land on the plate pose — at f0180
  the body lies extended along the couch with arms raised beside
  the head, where rung 1 folded the limbs differently; at f0000
  the seated arms/hands match. The remaining visible difference is
  brightness (item 6).

## Gates

- **gen_clip gate tests** (`test_gen_clip_frames.py`, 18 checks,
  half validation / half adversarial): **0 failures**. Validation:
  rotated-rest composition (Rx180 ⊗ Rx−78 = Rx102), identity-rest
  byte-identical to legacy, identity-delta carries rest, Prima
  regression (def bytes + sha identical, channels/face/hips
  identical, UpLeg changed). Adversarial: composition order is
  load-bearing (rest⊗delta ≠ delta⊗rest on a 120°@(1,1,1) rest),
  unknown show bone fails loud (KeyError — a key without a rest
  frame is never silently written), wrap past 180° stays the unit
  product, missing-key bone carries its rest.
- **Rust:** `cargo test -p torogao-render3d` 15/15 unit +
  proving 9/9 (above); `cargo test -p torogao-motion-clip` 108/108;
  clippy `-D warnings` clean (0 warnings); `cargo fmt --check`
  clean.
- **Theodolite (cured protocol)** on the one touched Rust file,
  `tests/proving.rs`: crate-scoped bacon producer first
  (`.bacon-locations` current at session start), planted E0308
  control returned **errors=2** in the same session shape, restored
  file returned **errors=0, warnings=0** — a verified zero.
- **Python discipline:** gen_clip is stdlib-only plain Python
  (not Blender-side — it reads the show JSON + GLB only); the fix
  keeps that shape and `py_compile` passes on primo. Its execution
  gate is the regeneration itself + the residual analysis against
  Blender's own posed matrices.

## Scope notes

- Items **6 (light level) and 7 (Fill shadow) are untouched** —
  rung-3 scope, exactly as the rung-1 pack left them.
- The other five girls' clips were **not** regenerated; the fix is
  general by construction (derived from each rig's own rest data),
  and cast-wide regeneration is a later rung's work.
- Concurrent-lane note: during this run the D-1224 plates-v2 lane
  edited `src/bin/render_arc.rs` and `src/compare.rs` in the same
  worktree (additive --width/--height defaults = the rung-1 plate
  constants). Those edits are that lane's, were left untouched,
  and are not in this lane's commit; the twin renders above were
  produced by the pre-edit binary.
