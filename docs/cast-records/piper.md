# RECORD — Piper (Torogao cast 3D rig, 2026-11-04)
Design source: v1 solo sheet `sheet-piper.png` + panels + canon keeper portrait (cast-solo-sheets-20261104). Adult early-20s model (two-model rule). Built by the generalized D-654 parametric builder (`build/build_character.py`, cast extensions for braid / side ponytail / hair accent / worn accessories).
## Identity

Long ash-brown hair in ONE braid over her left shoulder (segmented plait tube, alternating radius); grey-violet eyes, round pupils; light freckles (geometry, 14 spots, the D-654 Sadie precedent); H 170 cm.
## Deliverables
- `piper.glb` / `piper.blend`, `build_record.json`, `expression-map.json`, `param-map.json`
- Body: 3182 verts / 4386 faces; bones: 54; morphs: Basis + 36 Fac_
- Posed strain at build (builder BQ-5 pose): {"hip_L": 0.6229, "neck_weld": 0.0844, "shoulder_L": 0.1125, "wrist_weld_L": 0.0832}
- UV protection: {"head_post": "450e5e40ec056becc177ce7359907c04c44e3f62694cf68d5535f5406ba8616a", "head_pre": "450e5e40ec056becc177ce7359907c04c44e3f62694cf68d5535f5406ba8616a", "head_unchanged": true, "torso_post": "2d182d9de972f4a54d148c6a004a400d4c42bc08cbe9bced41bfea9a6a57e298", "torso_pre": "2d182d9de972f4a54d148c6a004a400d4c42bc08cbe9bced41bfea9a6a57e298", "torso_unchanged": true}

## Gates
- Blender census agreement: {"blend_bones": 54, "glb_body_match": false, "glb_bones": 54, "record_body_faces": 4386, "record_body_verts": 3182, "record_bones": 54}; channel depth: {"blink_crop_diff_frac": 0.3877, "eye_crops": [[588, 684, 984, 1080], [339, 435, 984, 1080]], "gaze_centroid_shift_px": 42.58, "headturn_changed_px_frac": 0.1998, "iris_occlusion_ratio": 0.2873, "iris_px_blink": 784, "iris_px_neutral": 1100, "lash_row_blink": 1032.7076793284473, "lash_row_neutral": 1031.4745584135208, "lash_travel_px": 1.23, "white_occlusion_ratio": 1.0, "white_px_blink": 0, "white_px_neutral": 25}
- Torogao: {"info": {"meshes": 4, "morphTargetCount": 36, "animations": 0}, "skins": 1, "nodes": 59, "check": {"governed": 10, "problems": [], "result": "complete"}, "states_resolved": {"arousal": true, "bliss": true, "desire": true, "longing": true, "neutral": true, "overwhelmed": true, "trance": true}}
- Neutral animation proof: frames 96, strain {"0": {"all": 0.1113, "displaced_verts_gt_3m": 0, "hip_L": 0.0, "shoulder_L": 0.0586, "vert11": [-0.049, 0.012, 1.469], "vert35": [-0.064, 0.014, 1.457], "worst_edge": {"rest_a": [0.116, 0.0, 1.401], "rest_b": [0.144, 0.002, 1.311], "rest_len": 0.09426, "strain": 0.111, "verts": [1926, 1938]}, "wrist_weld_L": 0.0}, "24": {"all": 1.1669, "displaced_verts_gt_3m": 0, "hip_L": 0.9472, "neck_weld": 0.0728, "shoulder_L": 0.0866, "vert11": [-0.049, -0.032, 1.269], "vert35": [-0.064, -0.029, 1.257], "worst_edge": {"rest_a": [0.0, 0.085, 0.835], "rest_b": [0.0, 0.078, 0.81], "rest_len": 0.02646, "strain": 1.167, "verts": [1435, 1459]}, "wrist_weld_L": 0.0}, "45": {"all": 2.0825, "displaced_verts_gt_3m": 0, "hip_L": 1.7538, "neck_weld": 0.1292, "shoulder_L": 0.1249, "vert11": [-0.049, -0.063, 1.073], "vert35": [-0.064, -0.06, 1.062], "worst_edge": {"rest_a": [0.0, 0.085, 0.835], "rest_b": [0.0, 0.078, 0.81], "rest_len": 0.02646, "strain": 2.083, "verts": [1435, 1459]}, "wrist_weld_L": 0.0}, "60": {"all": 1.206, "displaced_verts_gt_3m": 0, "hip_L": 0.9238, "neck_weld": 0.1982, "shoulder_L": 0.1324, "vert11": [-0.049, 0.109, 1.089], "vert35": [-0.064, 0.102, 1.078], "worst_edge": {"rest_a": [0.0, 0.085, 0.835], "rest_b": [0.0, 0.078, 0.81], "rest_len": 0.02646, "strain": 1.206, "verts": [1435, 1459]}, "wrist_weld_L": 0.0}, "84": {"all": 2.7957, "displaced_verts_gt_3m": 0, "hip_L": 0.4342, "neck_weld": 0.149, "shoulder_L": 0.6021, "vert11": [-0.049, 0.425, 0.318], "vert35": [-0.064, 0.413, 0.316], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 2.796, "verts": [1483, 1484]}, "wrist_weld_L": 0.0}, "95": {"all": 2.7957, "displaced_verts_gt_3m": 0, "hip_L": 0.4342, "neck_weld": 0.1473, "shoulder_L": 0.6021, "vert11": [-0.049, 0.425, 0.322], "vert35": [-0.064, 0.413, 0.321], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 2.796, "verts": [1483, 1484]}, "wrist_weld_L": 0.0}}
- Solo arc animation: frames 360, strain {"0": {"all": 0.7981, "displaced_verts_gt_3m": 0, "hip_L": 0.2291, "neck_weld": 0.2349, "shoulder_L": 0.0911, "vert11": [-0.049, -0.041, 0.97], "vert35": [-0.064, -0.038, 0.958], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 0.798, "verts": [1483, 1484]}, "wrist_weld_L": 0.0013}, "135": {"all": 1.206, "displaced_verts_gt_3m": 0, "hip_L": 0.9238, "neck_weld": 0.1981, "shoulder_L": 0.1324, "vert11": [-0.049, 0.123, 1.078], "vert35": [-0.064, 0.117, 1.068], "worst_edge": {"rest_a": [0.0, 0.085, 0.835], "rest_b": [0.0, 0.078, 0.81], "rest_len": 0.02646, "strain": 1.206, "verts": [1435, 1459]}, "wrist_weld_L": 0.0068}, "225": {"all": 3.8848, "displaced_verts_gt_3m": 0, "hip_L": 0.4898, "neck_weld": 0.4215, "shoulder_L": 0.5251, "vert11": [-0.049, 0.417, 0.28], "vert35": [-0.064, 0.405, 0.281], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 3.885, "verts": [1483, 1484]}, "wrist_weld_L": 0.027}, "24": {"all": 0.7981, "displaced_verts_gt_3m": 0, "hip_L": 0.2291, "neck_weld": 0.2344, "shoulder_L": 0.0911, "vert11": [-0.049, -0.049, 0.97], "vert35": [-0.064, -0.045, 0.958], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 0.798, "verts": [1483, 1484]}, "wrist_weld_L": 0.0012}, "315": {"all": 2.7957, "displaced_verts_gt_3m": 0, "hip_L": 0.4342, "neck_weld": 0.1483, "shoulder_L": 0.6021, "vert11": [-0.049, 0.425, 0.319], "vert35": [-0.064, 0.413, 0.318], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 2.796, "verts": [1483, 1484]}, "wrist_weld_L": 0.0022}, "45": {"all": 0.7981, "displaced_verts_gt_3m": 0, "hip_L": 0.2291, "neck_weld": 0.2347, "shoulder_L": 0.0911, "vert11": [-0.049, -0.043, 0.97], "vert35": [-0.064, -0.039, 0.958], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 0.798, "verts": [1483, 1484]}, "wrist_weld_L": 0.0012}, "60": {"all": 0.7981, "displaced_verts_gt_3m": 0, "hip_L": 0.2291, "neck_weld": 0.2346, "shoulder_L": 0.0911, "vert11": [-0.049, -0.036, 0.97], "vert35": [-0.064, -0.033, 0.958], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 0.798, "verts": [1483, 1484]}, "wrist_weld_L": 0.0013}, "84": {"all": 0.7981, "displaced_verts_gt_3m": 0, "hip_L": 0.2291, "neck_weld": 0.2346, "shoulder_L": 0.0911, "vert11": [-0.049, -0.036, 0.97], "vert35": [-0.064, -0.033, 0.958], "worst_edge": {"rest_a": [0.0, 0.068, 0.791], "rest_b": [-0.025, 0.066, 0.791], "rest_len": 0.02521, "strain": 0.798, "verts": [1483, 1484]}, "wrist_weld_L": 0.0013}}
- Motion-clip replay (arc): {"result": "ok", "frames_evaluated": 900, "replay_hash": "b98d3b823612bc71ccf6700676b71b7657d8622e7ee035fb5ac65247a940ad88", "deterministic": true}
- Motion-clip replay (neutral): {"result": "ok", "frames_evaluated": 240, "replay_hash": "59a9e44bf923ee689a5c5d1f1eb5d157f24fd79a4776be6578b1947caa1da45b", "deterministic": true}

## Animation beat map (arc, 24 fps, 360 frames)

| stage | frames | staging | face |
|---|---|---|---|
| BUILD | 0–89 | her build pose (see show) | composed, teasing; first half-close + blinks |
| BUILD II | 90–179 | reclined; signature kink in active use; self-touch rhythm on the right forearm |
| PEAK | 180–269 | sprawled + arch overlay, tremor; rolled eyes (eye-bone pitch), Aa/Shout, heart pupils at 1.0 for the cast trio |
| AFTERGLOW | 270–359 | sprawled limp, slow deep breathing | crossed eyes, Open+Triangle (tongue register), torogao savoring |

## Face honesty

Template sculpt + freckle geometry. Coarse first-pass face. Anchor (piper-R-01) Owner-pending, disclosed.
## Props

Rig: blindfold band modelled on outfit 2 (worn BUILD through AFTERGLOW per her sheet). Arc scene: bed, kneeling cushion, the open ledger (BUILD/BUILD II).
## Deviations / named gaps

Blindfold is fixed in place (the sheet's AFTERGLOW 'askew' state would need a second blindfold pose — recorded, not modelled). Braid is a plait-read tube, not strand geometry.

## Motion-clip format drops (named)

Carried: all keyed bone rotations (49 body tracks incl. finger curl), hips translation, all Fac_ morph channels keyed in the show (60 Hz grid). Dropped: prop animation (machine thrust, toy placement), outfit visibility swaps, set/camera, Blender bezier easing (clip is linear), and the eye-bone gaze keys are carried as bone tracks like any other bone. Bone names are renamed mixamorig:X -> mixamorig_X in the clip + def (the format's id charset bars ':'); the def document records the mapping.

## Addendum — blindfold-off channel evidence (2026-11-05)

The standard gate renders Piper's face through her worn blindfold
(outfit 2), so the standard white-pixel reading (25 px) is an
occlusion artifact, not a blink measurement. Supplementary evidence
rendered from an evidence-only copy of piper.blend with the 32
Blindfold-material faces deleted (shipped files untouched):
`evidence/face_rest_off.png`, `evidence/face_blink_off.png`.
Measured with the gate's own white definition (lum > 0.74,
blue >= red - 0.02) over eye crops: white rest 944 px -> blink 0 px,
**occlusion ratio 1.0**, blink crop-diff 0.1804. Blink PASS.
Also visible in `evidence/face_threshold.png`: the gate's threshold
probe applies Fac_Pupil_Heart = 1.0 unconditionally (fleet-uniform
channel probe); Piper's governed expression map carries no heart
entries (troupe round pupils) and her Torogao-resolved states grow
no hearts — the hearts in that one render are the probe, seen
through the blindfold band.

## Rebuild 3 (D-1001, 2026-11-05) — hairline seam cure

Third build of this rig, fleet-wide under D-1001. One builder change, applied at the cause: the scalp-cap face-cut constant in `build_character.py` went 1.632 → 1.60, dropping the cap rim from polar ring 5 (canonical z 1.646, ~8 mm above the fringe top at 1.640 — the visible skin band) to ring 7 (z 1.6024), behind the fringe curtain. Hair part delta vs build 2: +10 verts / +12 faces (the two extra cap rings); every other part census identical. Build 2 is preserved in full under `v2/` in this directory.

- Census: Body 3182 verts / 4386 faces; 54 bones; Basis + 36 Fac_ morphs — exact vs build record.
- Torogao ingest: 4 meshes, 36 morph targets, expression check complete, governed 10, problems [], 7 threshold states resolve.
- BQ-5 posed strain (builder): {'hip_L': 0.6229, 'neck_weld': 0.0844, 'shoulder_L': 0.1125, 'wrist_weld_L': 0.0832} — identical to build 2 (body untouched), all zones ≤ 0.64.
- Blink: white occlusion 1.0.
- **Hairline seam: GONE by measurement.** Ray-cast seam metric (`build/hairline_gate.py`; front rays over canonical z 1.590–1.675; seam band z 1.636–1.652): band skin-first fraction build 2 = 0.3044 → build 3 = 0.0000. Remaining skin-first rays sit at canonical z ≤ 1.602 — the open forehead below the fringe's lower edge, i.e. the face opening working as designed, not a seam remnant. Close-up sheet: `hairline-sheet.png` (crown + both temples, build 2 row over build 3 row); crops in `evidence-v2ref/` and `evidence-v3/`.
- Motion: neutral + arc re-rendered from the rebuild-3 geometry. Frame diffs vs build 2 are confined to narrow hairline bands (changed fraction ≤ 0.0006 at sampled frames). Clips re-exported and replay-deterministic through torogao-motion-clip/1, hashes identical to build 2 — neutral `59a9e44bf923ee689a5c5d1f1eb5d157f24fd79a4776be6578b1947caa1da45b`, arc `b98d3b823612bc71ccf6700676b71b7657d8622e7ee035fb5ac65247a940ad88`.
- Maps: build 2's governed `expression-map.json` / `param-map.json` carried forward byte-identical (the rebuild changes geometry only). The R3 build initially copied the raw D-654 template maps (stale "Vivienne" stamp; heart pupils unrestored for the troupe); Gate C was re-run against the carried-forward maps and passed (evidence refreshed in `evidence/gate_torogao.json`).

## Conforming fix — D-1 six-girl standard (2026-11-06)

Additive layers landed on the build-3 rig per SIX-GIRL-STANDARD §4/§5/§6 (evidence: `night-school-20261105/runs/d1-conforming-fixes/`; build-3 originals preserved in `build3-original/`).

- **§4 bust parameter:** `Body_Bust_Size` shape key on Body + Outfit + OutfitEnd, emitted from the builder's own gaussian delta (value 1.0 = m 1.50, −1.0 = m 0.50, 0 = canon; body max delta 13.0 mm). `bust_param {amp_canon 0.026, 0.50, 1.50}` recorded in spec.
- **§5 jiggle cages:** `PiperCage.L/R` (56 verts each), `Cage.L/R` mask groups + bound `CageDeform.L/R` Mesh Deform on Body, Outfit and OutfitEnd; `Bounce` keys at rest (unkeyed). Band 1.136–1.438 m, 236 verts.
- **§6 fluid anchors:** 7 `Anchor_Fluid_*` empties placed by the §6.2 rules at canon (max err vs §6.4 landmarks 0.054 mm; nipple anchors track m by ±11.00 mm, re-derivation tool `build/place_anchors.py`).
- **Identity gate:** decoded-pixel renders at canon identical to build-3 on full / bust-front / bust-side / face views (blink view identical too).
- **§4.5 gate:** cage re-derivation + jiggle determinism at m ∈ {0.50, 1.00, 1.50}: two independent runs pixel-identical at every setting; beat displacement max 15.5 / 15.8 / 16.5 mm; outfit apex margin positive at every setting (min over settings/sides 13.28 mm); hairline seam band at m-max: 0.0000.
