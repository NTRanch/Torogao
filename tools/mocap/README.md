# tools/mocap — estate motion-capture pipeline

Markerless mocap from a video file: pose estimation → joint
trajectories → deterministic cleanup → retarget onto the canonical
162-joint rig → draft `torogao-motion-clip/2`, validated (and, once
the bench permits, frozen) through the H0/H1 motion-clip bench.

Per D-1397 and the 2026-11-27 hybrid ruling, capture is a
**proposal source**: the output is a DRAFT clip plus a loss report.
Raw capture never ships; drafts are hand/code-finished, then
frozen. Live camera capture is out of scope (named follow-on).

## Run

```
python -m mocap.cli run    --video V --name N --out DIR [--config C]
python -m mocap.cli freeze --clip N.clip.json --check-bin BIN
```

`run` writes `N.clip.json` (draft), `N.loss.json` (cleanup counts,
held gaps, named losses) and `N.stage.npz` (+ `.meta.json`, the
raw estimator stage for re-emission without re-estimation).
`freeze` validates the draft with `motion-clip-check`, freezes it,
and revalidates the frozen document. Exit codes: 0 = frozen and
revalidated; **3 = draft valid, freeze blocked by the
torogao-format canonical-writer finding** (see
`FINDING-torogao-format-f64.md` — currently the state for every
draft carrying real-valued curves); anything else = invalid.

Models are not committed. `fetch_models.sh` downloads them;
`config/default.json` pins each by SHA-256 and the estimate stage
refuses a mismatched file.

## Design

- **Estimator**: MediaPipe PoseLandmarker Heavy (BlazePose GHUM),
  Tasks API, IMAGE mode — stateless per frame, so the estimator is
  a pure function of (frame, pinned model); temporal behaviour
  lives in the cleanup stage, where it is named and inspectable.
  FaceLandmarker blendshapes and HandLandmarker world landmarks
  ride the same stack as auxiliary channels. Census and licence
  verification: see the lane record
  (~/workspace/mocap-pipeline-20261127/RECORD.md). The RTMPose
  route (rtmlib) was the runner-up: 2D keypoints only, no direct
  depth.
- **World-frame conversion**: MediaPipe world landmarks are
  y-down; they are negated in y at ingest into the right-handed
  y-up frame the solver assumes. (Found by the ground-truth loop:
  without it, end-to-end joint error was 68 cm; with it, 22 cm —
  the estimator's own noise floor.)
- **Cleanup** (deterministic, named, in order): confidence gate
  (visibility τ = 0.5) → spike reject (3.0 m/s cap) → gap fill
  (linear ≤ 15 frames; longer gaps held and reported) →
  Savitzky–Golay (window 5, poly 2) → foot pin (contact = ankle
  within 0.04 m of clip ground; x/z pinned to the run median,
  min run 4 frames). No learned components anywhere in cleanup.
- **Retarget**: analytic construction frames from landmark
  geometry; per-joint world delta against the same construction
  applied to landmarks synthesised from the rig's own rest pose,
  so the solver is exact by construction on true landmarks
  (oracle floor: 1.17 cm mean joint error, 0.00° flexion error —
  see validate/). The torso is one observed segment distributed
  over Spine/Spine1/Spine2 at fixed slerp stops; Neck takes a
  fixed 0.45 share of the chest→head delta. Driven: 20 joints.
  Not driven (named loss): ToeBase, eyes, fingers from body
  capture, hair, cloth, twist joints. Hips translation z is held
  at the rest plane (weak-perspective unprojection; named loss).
- **Rig**: `retarget/canonical162.json`, hierarchy SHA-256
  `a570e608…` (the clip `rig` value). Rest pose is derived from
  the GLB's inverse bind matrices — regenerate with
  `retarget/build_canonical162.py`. The GLB's node transforms
  disagree with the bind pose by up to ~1.9 cm on arm/foot joints;
  do not rebuild the rest pose from node transforms.

## Determinism

Same video + same config → byte-identical draft, every run
(verified: repeated full runs, identical SHA-256). The estimator
runs CPU-only in IMAGE mode; two fresh landmarker instances
produce byte-identical landmarks. No GPU is used, so there is no
GPU nondeterminism to bound. Model files are SHA-256-pinned.

## Validation (validate/)

- `gt_scene.py` + `gt_score.py`: ground-truth loop. An authored
  90-frame action on Prima's canonical rig (known curves),
  rendered by Blender, run back through the pipeline, scored
  three ways (estimator / oracle solver / end-to-end). The GT
  render uses a mannequin material pass: the estate model's
  untextured export is invisible to the pose detector
  (0/30 detections unmodified, 30/30 recoloured).
- `laplace_check.py`: reference check against the Laplace hybrid
  study's measurements (cycle period, lower-band lag) on the
  corpus clips.

Headline numbers (2026-11-27): solver floor 1.17 cm; end-to-end
22.3 cm mean joint error on the body-visible GT variant (knees
12–14° flexion RMSE; elbows 33–37°, equal to the estimator's own
angle error — the solver transmits angles losslessly); Ayaka
cycle recovered 0.600 s vs the study's 0.567 s (+5.8%); Serval
fails at detection (~25% of frames). Full account in the lane
record.
