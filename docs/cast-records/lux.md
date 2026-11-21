# RECORD — Lux (Torogao cast 3D rig, 2026-11-04)
Design source: v1 solo sheet `sheet-lux.png` + panels + canon keeper portrait (cast-solo-sheets-20261104). Adult early-20s model (two-model rule). Built by the generalized D-654 parametric builder (`build/build_character.py`, cast extensions for braid / side ponytail / hair accent / worn accessories).
## Identity

Jaw bob, black with teal underlayer/tips (lowest back-shell row carries the HairAccent teal material); teal-green eyes, round pupils; camera pendant (jewellery set, teal gem); H 165 cm.
## Deliverables
- `lux.glb` / `lux.blend`, `build_record.json`, `expression-map.json`, `param-map.json`
- Body: 3084 verts / 4302 faces; bones: 54; morphs: Basis + 36 Fac_
- Posed strain at build (builder BQ-5 pose): {"hip_L": 0.5508, "neck_weld": 0.4481, "shoulder_L": 0.0, "wrist_weld_L": 0.0833}
- UV protection: {"head_post": "4921824c7f8d257de78febad1af2b6bf5927855b973cc2d787e48b6f3a07cfdd", "head_pre": "4921824c7f8d257de78febad1af2b6bf5927855b973cc2d787e48b6f3a07cfdd", "head_unchanged": true, "torso_post": "2d182d9de972f4a54d148c6a004a400d4c42bc08cbe9bced41bfea9a6a57e298", "torso_pre": "2d182d9de972f4a54d148c6a004a400d4c42bc08cbe9bced41bfea9a6a57e298", "torso_unchanged": true}

## Gates
- Blender census agreement: {"blend_bones": 54, "glb_body_match": false, "glb_bones": 54, "record_body_faces": 4302, "record_body_verts": 3084, "record_bones": 54}; channel depth: {"blink_crop_diff_frac": 0.6701, "eye_crops": [[589, 685, 971, 1067], [338, 434, 971, 1067]], "gaze_centroid_shift_px": 110.47, "headturn_changed_px_frac": 0.1893, "iris_px_blink": 0, "iris_px_neutral": 13, "lash_row_blink": 1021.3416666666667, "lash_row_neutral": 1019.3397260273973, "lash_travel_px": 2.0, "white_occlusion_ratio": 1.0, "white_px_blink": 0, "white_px_neutral": 2631}
- Torogao: {"info": {"meshes": 4, "morphTargetCount": 36, "animations": 0}, "skins": 1, "nodes": 59, "check": {"governed": 10, "problems": [], "result": "complete"}, "states_resolved": {"arousal": true, "bliss": true, "desire": true, "longing": true, "neutral": true, "overwhelmed": true, "trance": true}}
- Neutral animation proof: frames 96, strain {"0": {"all": 0.1149, "displaced_verts_gt_3m": 0, "hip_L": 0.0, "vert11": [-0.049, 0.012, 1.426], "vert35": [-0.064, 0.014, 1.414], "worst_edge": {"rest_a": [-0.118, 0.0, 1.359], "rest_b": [-0.145, 0.002, 1.272], "rest_len": 0.09174, "strain": 0.115, "verts": [2453, 2465]}, "wrist_weld_L": 0.0}, "24": {"all": 1.1932, "displaced_verts_gt_3m": 0, "hip_L": 0.961, "neck_weld": 0.0172, "shoulder_L": 0.0, "vert11": [-0.049, -0.031, 1.226], "vert35": [-0.064, -0.028, 1.215], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 1.193, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "45": {"all": 2.124, "displaced_verts_gt_3m": 0, "hip_L": 1.7745, "neck_weld": 0.0313, "shoulder_L": 0.0, "vert11": [-0.049, -0.062, 1.03], "vert35": [-0.064, -0.058, 1.019], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 2.124, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "60": {"all": 1.2347, "displaced_verts_gt_3m": 0, "hip_L": 0.9375, "neck_weld": 0.0474, "shoulder_L": 0.0, "vert11": [-0.049, 0.101, 1.048], "vert35": [-0.064, 0.095, 1.038], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 1.235, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "84": {"all": 2.7107, "displaced_verts_gt_3m": 0, "hip_L": 0.4329, "neck_weld": 0.0352, "shoulder_L": 0.0, "vert11": [-0.049, 0.41, 0.289], "vert35": [-0.064, 0.398, 0.288], "worst_edge": {"rest_a": [0.0, 0.069, 0.768], "rest_b": [-0.025, 0.066, 0.768], "rest_len": 0.02547, "strain": 2.711, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}, "95": {"all": 2.7107, "displaced_verts_gt_3m": 0, "hip_L": 0.4329, "neck_weld": 0.0353, "shoulder_L": 0.0, "vert11": [-0.049, 0.41, 0.293], "vert35": [-0.064, 0.398, 0.292], "worst_edge": {"rest_a": [0.0, 0.069, 0.768], "rest_b": [-0.025, 0.066, 0.768], "rest_len": 0.02547, "strain": 2.711, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}}
- Solo arc animation: frames 360, strain {"0": {"all": 2.1597, "displaced_verts_gt_3m": 0, "hip_L": 1.808, "neck_weld": 0.0351, "shoulder_L": 0.0, "vert11": [-0.049, -0.075, 1.024], "vert35": [-0.064, -0.071, 1.013], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 2.16, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "135": {"all": 1.2347, "displaced_verts_gt_3m": 0, "hip_L": 0.9375, "neck_weld": 0.0474, "shoulder_L": 0.0, "vert11": [-0.049, 0.115, 1.037], "vert35": [-0.064, 0.109, 1.027], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 1.235, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "225": {"all": 3.768, "displaced_verts_gt_3m": 0, "hip_L": 0.4999, "neck_weld": 0.1047, "shoulder_L": 0.0, "vert11": [-0.049, 0.402, 0.252], "vert35": [-0.064, 0.39, 0.253], "worst_edge": {"rest_a": [0.0, 0.069, 0.768], "rest_b": [-0.025, 0.066, 0.768], "rest_len": 0.02547, "strain": 3.768, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}, "24": {"all": 2.1597, "displaced_verts_gt_3m": 0, "hip_L": 1.808, "neck_weld": 0.0351, "shoulder_L": 0.0, "vert11": [-0.049, -0.082, 1.024], "vert35": [-0.064, -0.078, 1.012], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 2.16, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "315": {"all": 2.7107, "displaced_verts_gt_3m": 0, "hip_L": 0.4329, "neck_weld": 0.0353, "shoulder_L": 0.0, "vert11": [-0.049, 0.41, 0.29], "vert35": [-0.064, 0.398, 0.289], "worst_edge": {"rest_a": [0.0, 0.069, 0.768], "rest_b": [-0.025, 0.066, 0.768], "rest_len": 0.02547, "strain": 2.711, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}, "45": {"all": 2.1597, "displaced_verts_gt_3m": 0, "hip_L": 1.808, "neck_weld": 0.0351, "shoulder_L": 0.0, "vert11": [-0.049, -0.076, 1.024], "vert35": [-0.064, -0.072, 1.013], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 2.16, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "60": {"all": 2.1597, "displaced_verts_gt_3m": 0, "hip_L": 1.808, "neck_weld": 0.0351, "shoulder_L": 0.0, "vert11": [-0.049, -0.07, 1.025], "vert35": [-0.064, -0.066, 1.014], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 2.16, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "84": {"all": 2.1597, "displaced_verts_gt_3m": 0, "hip_L": 1.808, "neck_weld": 0.0351, "shoulder_L": 0.0, "vert11": [-0.049, -0.07, 1.025], "vert35": [-0.064, -0.066, 1.014], "worst_edge": {"rest_a": [0.0, 0.086, 0.81], "rest_b": [0.0, 0.078, 0.786], "rest_len": 0.02577, "strain": 2.16, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}}
- Motion-clip replay (arc): {"result": "ok", "frames_evaluated": 900, "replay_hash": "89d833fa6cd5b2a1f0824cbfff1513fc12d7cd7d68b67f7edfd86b091ba90d6b", "deterministic": true}
- Motion-clip replay (neutral): {"result": "ok", "frames_evaluated": 240, "replay_hash": "84de2fd1b28a830a0cf3f11e2971c978bf10bf83f9c5d77f595c9cc5d7d9b87c", "deterministic": true}

## Animation beat map (arc, 24 fps, 360 frames)

| stage | frames | staging | face |
|---|---|---|---|
| BUILD | 0–89 | her build pose (see show) | composed, teasing; first half-close + blinks |
| BUILD II | 90–179 | reclined; signature kink in active use; self-touch rhythm on the right forearm |
| PEAK | 180–269 | sprawled + arch overlay, tremor; rolled eyes (eye-bone pitch), Aa/Shout, heart pupils at 1.0 for the cast trio |
| AFTERGLOW | 270–359 | sprawled limp, slow deep breathing | crossed eyes, Open+Triangle (tongue register), torogao savoring |

## Face honesty

Template sculpt, troupe register. Coarse first-pass face. Anchor (lux-R restyle) Owner-pending, disclosed.
## Props

Rig: pendant necklace + earrings (Gem = teal). Arc scene: bed, tripod camera with red record light and ring light (she plays to the lens), hand toy prop.
## Deviations / named gaps

The pendant is the builder's gem pendant, not a modelled miniature camera. Body writing from her sheet is not modelled (paint-over item in 2D; no texture channel in this build).

## Motion-clip format drops (named)

Carried: all keyed bone rotations (49 body tracks incl. finger curl), hips translation, all Fac_ morph channels keyed in the show (60 Hz grid). Dropped: prop animation (machine thrust, toy placement), outfit visibility swaps, set/camera, Blender bezier easing (clip is linear), and the eye-bone gaze keys are carried as bone tracks like any other bone. Bone names are renamed mixamorig:X -> mixamorig_X in the clip + def (the format's id charset bars ':'); the def document records the mapping.

## Rebuild 3 (D-1001, 2026-11-05) — hairline seam cure

Third build of this rig, fleet-wide under D-1001. One builder change, applied at the cause: the scalp-cap face-cut constant in `build_character.py` went 1.632 → 1.60, dropping the cap rim from polar ring 5 (canonical z 1.646, ~8 mm above the fringe top at 1.640 — the visible skin band) to ring 7 (z 1.6024), behind the fringe curtain. Hair part delta vs build 2: +10 verts / +12 faces (the two extra cap rings); every other part census identical. Build 2 is preserved in full under `v2/` in this directory.

- Census: Body 3084 verts / 4302 faces; 54 bones; Basis + 36 Fac_ morphs — exact vs build record.
- Torogao ingest: 4 meshes, 36 morph targets, expression check complete, governed 10, problems [], 7 threshold states resolve.
- BQ-5 posed strain (builder): {'hip_L': 0.5508, 'neck_weld': 0.4481, 'shoulder_L': 0.0, 'wrist_weld_L': 0.0833} — identical to build 2 (body untouched), all zones ≤ 0.64.
- Blink: white occlusion 1.0.
- **Hairline seam: GONE by measurement.** Ray-cast seam metric (`build/hairline_gate.py`; front rays over canonical z 1.590–1.675; seam band z 1.636–1.652): band skin-first fraction build 2 = 0.2537 → build 3 = 0.0000. Remaining skin-first rays sit at canonical z ≤ 1.602 — the open forehead below the fringe's lower edge, i.e. the face opening working as designed, not a seam remnant. Close-up sheet: `hairline-sheet.png` (crown + both temples, build 2 row over build 3 row); crops in `evidence-v2ref/` and `evidence-v3/`.
- Motion: neutral + arc re-rendered from the rebuild-3 geometry. Frame diffs vs build 2 are confined to narrow hairline bands (changed fraction ≤ 0.0006 at sampled frames). Clips re-exported and replay-deterministic through torogao-motion-clip/1, hashes identical to build 2 — neutral `84de2fd1b28a830a0cf3f11e2971c978bf10bf83f9c5d77f595c9cc5d7d9b87c`, arc `89d833fa6cd5b2a1f0824cbfff1513fc12d7cd7d68b67f7edfd86b091ba90d6b`.
- Maps: build 2's governed `expression-map.json` / `param-map.json` carried forward byte-identical (the rebuild changes geometry only). The R3 build initially copied the raw D-654 template maps (stale "Vivienne" stamp; heart pupils unrestored for the troupe); Gate C was re-run against the carried-forward maps and passed (evidence refreshed in `evidence/gate_torogao.json`).

## Conforming fix — D-1 six-girl standard (2026-11-06)

Additive layers landed on the build-3 rig per SIX-GIRL-STANDARD §4/§5/§6 (evidence: `night-school-20261105/runs/d1-conforming-fixes/`; build-3 originals preserved in `build3-original/`).

- **§4 bust parameter:** `Body_Bust_Size` shape key on Body + Outfit + OutfitEnd, emitted from the builder's own gaussian delta (value 1.0 = m 1.50, −1.0 = m 0.50, 0 = canon; body max delta 15.0 mm). `bust_param {amp_canon 0.030, 0.50, 1.50}` recorded in spec.
- **§5 jiggle cages:** `LuxCage.L/R` (56 verts each), `Cage.L/R` mask groups + bound `CageDeform.L/R` Mesh Deform on Body, Outfit and OutfitEnd; `Bounce` keys at rest (unkeyed). Band 1.103–1.396 m, 236 verts.
- **§6 fluid anchors:** 7 `Anchor_Fluid_*` empties placed by the §6.2 rules at canon (max err vs §6.4 landmarks 0.060 mm; nipple anchors track m by ±12.69 mm, re-derivation tool `build/place_anchors.py`).
- **Identity gate:** decoded-pixel renders at canon identical to build-3 on full / bust-front / bust-side / face views (blink view identical too).
- **§4.5 gate:** cage re-derivation + jiggle determinism at m ∈ {0.50, 1.00, 1.50}: two independent runs pixel-identical at every setting; beat displacement max 15.3 / 15.6 / 16.7 mm; outfit apex margin positive at every setting (min over settings/sides 13.72 mm); hairline seam band at m-max: 0.0000.
