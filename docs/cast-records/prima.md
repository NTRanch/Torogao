# RECORD — Prima (Torogao cast 3D rig, 2026-11-04)
Design source: v1 solo sheet `sheet-prima.png` + panels + canon keeper portrait (cast-solo-sheets-20261104). Adult early-20s model (two-model rule). Built by the generalized D-654 parametric builder (`build/build_character.py`, cast extensions for braid / side ponytail / hair accent / worn accessories).
## Identity

Silver long loose waves; red iris; solid heart pupils (Fac_Pupil_Heart state, never baked); soft oval face; normal warm complexion (revised 2026-11-09, see Canon revisions); large bust (revised 2026-11-09, see Canon revisions); H 165 cm.

## Canon revisions (Owner rulings, 2026-11-09)
- **Complexion:** normal warm complexion — supersedes the pilot spec's "porcelain skin" (spec palette Skin .953/.851/.776 as generated read too pale on the Owner's eye). Ruled on the dream-style pilot stills (PILOT.md, round 2).
- **Bust:** bustier — large bust supersedes the "medium bust" line above and the pilot's modest tagging. The 3D rig as built still carries the medium vector (bust_param amp_canon 0.030, §4); the rig follows this canon only on a separate rig ruling — this revision governs the 2D/stills canon now. **Rig ruling landed (D-1152, 2026-11-10): the 3D rig's bust canon is now m* = 1.50** — see "Rig follow-up (D-1152, 2026-11-10)" at the end of this record.
- Round-2 stills under the revised canon: ntranch-b25246875716 (bust 1), ntranch-4a26ef9a4d38 (bust 2), ntranch-e99e6461fcbf (full 1), ntranch-202e0fc7f959 (full 2), same seeds 276001–276004 as the pilot.
## Deliverables
- `prima.glb` / `prima.blend`, `build_record.json`, `expression-map.json`, `param-map.json`
- Body: 3084 verts / 4302 faces; bones: 54; morphs: Basis + 36 Fac_
- Posed strain at build (builder BQ-5 pose): {"hip_L": 0.5603, "neck_weld": 0.4495, "shoulder_L": 0.0, "wrist_weld_L": 0.0835}
- UV protection: {"head_post": "4921824c7f8d257de78febad1af2b6bf5927855b973cc2d787e48b6f3a07cfdd", "head_pre": "4921824c7f8d257de78febad1af2b6bf5927855b973cc2d787e48b6f3a07cfdd", "head_unchanged": true, "torso_post": "2d182d9de972f4a54d148c6a004a400d4c42bc08cbe9bced41bfea9a6a57e298", "torso_pre": "2d182d9de972f4a54d148c6a004a400d4c42bc08cbe9bced41bfea9a6a57e298", "torso_unchanged": true}

## Gates
- Blender census agreement: {"blend_bones": 54, "glb_body_match": false, "glb_bones": 54, "record_body_faces": 4302, "record_body_verts": 3084, "record_bones": 54}; channel depth: {"blink_crop_diff_frac": 0.642, "eye_crops": [[592, 688, 971, 1067], [335, 431, 971, 1067]], "headturn_changed_px_frac": 0.2253, "iris_px_blink": 0, "iris_px_neutral": 0, "lash_row_blink": 1000.0833333333333, "lash_row_neutral": 1019.4119851518944, "lash_travel_px": 19.33, "white_occlusion_ratio": 1.0, "white_px_blink": 0, "white_px_neutral": 3437}
- Torogao: {"info": {"meshes": 4, "morphTargetCount": 36, "animations": 0}, "skins": 1, "nodes": 59, "check": {"governed": 10, "problems": [], "result": "complete"}, "states_resolved": {"arousal": true, "bliss": true, "desire": true, "longing": true, "neutral": true, "overwhelmed": true, "trance": true}}
- Neutral animation proof: frames 96, strain {"0": {"all": 0.1167, "displaced_verts_gt_3m": 0, "hip_L": 0.0, "vert11": [-0.05, 0.012, 1.426], "vert35": [-0.066, 0.014, 1.414], "worst_edge": {"rest_a": [-0.12, 0.0, 1.359], "rest_b": [-0.149, 0.002, 1.271], "rest_len": 0.09187, "strain": 0.117, "verts": [2453, 2465]}, "wrist_weld_L": 0.0}, "24": {"all": 1.2067, "displaced_verts_gt_3m": 0, "hip_L": 0.9679, "neck_weld": 0.0169, "shoulder_L": 0.0, "vert11": [-0.05, -0.031, 1.226], "vert35": [-0.066, -0.028, 1.215], "worst_edge": {"rest_a": [0.0, 0.088, 0.81], "rest_b": [0.0, 0.08, 0.786], "rest_len": 0.02582, "strain": 1.207, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "45": {"all": 2.1454, "displaced_verts_gt_3m": 0, "hip_L": 1.785, "neck_weld": 0.0307, "shoulder_L": 0.0, "vert11": [-0.05, -0.062, 1.03], "vert35": [-0.066, -0.058, 1.019], "worst_edge": {"rest_a": [0.0, 0.088, 0.81], "rest_b": [0.0, 0.08, 0.786], "rest_len": 0.02582, "strain": 2.145, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "60": {"all": 1.2493, "displaced_verts_gt_3m": 0, "hip_L": 0.9443, "neck_weld": 0.0465, "shoulder_L": 0.0, "vert11": [-0.05, 0.101, 1.048], "vert35": [-0.066, 0.095, 1.037], "worst_edge": {"rest_a": [0.0, 0.088, 0.81], "rest_b": [0.0, 0.08, 0.786], "rest_len": 0.02582, "strain": 1.249, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "84": {"all": 2.6691, "displaced_verts_gt_3m": 0, "hip_L": 0.432, "neck_weld": 0.0347, "shoulder_L": 0.0, "vert11": [-0.05, 0.41, 0.289], "vert35": [-0.066, 0.398, 0.287], "worst_edge": {"rest_a": [0.0, 0.07, 0.768], "rest_b": [-0.026, 0.068, 0.768], "rest_len": 0.02599, "strain": 2.669, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}, "95": {"all": 2.6691, "displaced_verts_gt_3m": 0, "hip_L": 0.432, "neck_weld": 0.0346, "shoulder_L": 0.0, "vert11": [-0.05, 0.41, 0.293], "vert35": [-0.066, 0.398, 0.292], "worst_edge": {"rest_a": [0.0, 0.07, 0.768], "rest_b": [-0.026, 0.068, 0.768], "rest_len": 0.02599, "strain": 2.669, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}}
- Solo arc animation: frames 360, strain {"0": {"all": 2.1813, "displaced_verts_gt_3m": 0, "hip_L": 1.8186, "neck_weld": 0.0344, "shoulder_L": 0.0, "vert11": [-0.05, -0.075, 1.024], "vert35": [-0.066, -0.07, 1.013], "worst_edge": {"rest_a": [0.0, 0.088, 0.81], "rest_b": [0.0, 0.08, 0.786], "rest_len": 0.02582, "strain": 2.181, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "135": {"all": 2.7285, "displaced_verts_gt_3m": 0, "hip_L": 0.9688, "neck_weld": 0.0465, "shoulder_L": 0.0, "vert11": [-0.05, 0.116, 1.037], "vert35": [-0.066, 0.109, 1.027], "worst_edge": {"rest_a": [0.0, 0.07, 0.768], "rest_b": [-0.026, 0.068, 0.768], "rest_len": 0.02599, "strain": 2.729, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}, "225": {"all": 2.6443, "displaced_verts_gt_3m": 0, "hip_L": 0.4097, "neck_weld": 0.1028, "shoulder_L": 0.0, "vert11": [-0.05, 0.402, 0.252], "vert35": [-0.066, 0.39, 0.253], "worst_edge": {"rest_a": [0.0, 0.07, 0.768], "rest_b": [-0.026, 0.068, 0.768], "rest_len": 0.02599, "strain": 2.644, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}, "24": {"all": 2.1813, "displaced_verts_gt_3m": 0, "hip_L": 1.8186, "neck_weld": 0.0344, "shoulder_L": 0.0, "vert11": [-0.05, -0.082, 1.024], "vert35": [-0.066, -0.077, 1.012], "worst_edge": {"rest_a": [0.0, 0.088, 0.81], "rest_b": [0.0, 0.08, 0.786], "rest_len": 0.02582, "strain": 2.181, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "315": {"all": 2.6691, "displaced_verts_gt_3m": 0, "hip_L": 0.432, "neck_weld": 0.0345, "shoulder_L": 0.0, "vert11": [-0.05, 0.41, 0.29], "vert35": [-0.066, 0.398, 0.289], "worst_edge": {"rest_a": [0.0, 0.07, 0.768], "rest_b": [-0.026, 0.068, 0.768], "rest_len": 0.02599, "strain": 2.669, "verts": [1385, 1386]}, "wrist_weld_L": 0.0}, "45": {"all": 2.1813, "displaced_verts_gt_3m": 0, "hip_L": 1.8186, "neck_weld": 0.0344, "shoulder_L": 0.0, "vert11": [-0.05, -0.076, 1.024], "vert35": [-0.066, -0.072, 1.013], "worst_edge": {"rest_a": [0.0, 0.088, 0.81], "rest_b": [0.0, 0.08, 0.786], "rest_len": 0.02582, "strain": 2.181, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "60": {"all": 2.1813, "displaced_verts_gt_3m": 0, "hip_L": 1.8186, "neck_weld": 0.0344, "shoulder_L": 0.0, "vert11": [-0.05, -0.07, 1.025], "vert35": [-0.066, -0.066, 1.014], "worst_edge": {"rest_a": [0.0, 0.088, 0.81], "rest_b": [0.0, 0.08, 0.786], "rest_len": 0.02582, "strain": 2.181, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}, "84": {"all": 2.1813, "displaced_verts_gt_3m": 0, "hip_L": 1.8186, "neck_weld": 0.0344, "shoulder_L": 0.0, "vert11": [-0.05, -0.07, 1.025], "vert35": [-0.066, -0.066, 1.014], "worst_edge": {"rest_a": [0.0, 0.088, 0.81], "rest_b": [0.0, 0.08, 0.786], "rest_len": 0.02582, "strain": 2.181, "verts": [1337, 1361]}, "wrist_weld_L": 0.0}}
- Motion-clip replay (arc): {"result": "ok", "frames_evaluated": 900, "replay_hash": "84b8c442a16d66f17a4faf2b8460727be6649411dc3836771c67ad2df66ca8a4", "deterministic": true}
- Motion-clip replay (neutral): {"result": "ok", "frames_evaluated": 240, "replay_hash": "a3770d899030940de7bbd06a2daa83a6d8dc441f7b90fc03b54907d25e18986c", "deterministic": true}

## Animation beat map (arc, 24 fps, 360 frames)

| stage | frames | staging | face |
|---|---|---|---|
| BUILD | 0–89 | her build pose (see show) | composed, teasing; first half-close + blinks |
| BUILD II | 90–179 | reclined; signature kink in active use; self-touch rhythm on the right forearm |
| PEAK | 180–269 | sprawled + arch overlay, tremor; rolled eyes (eye-bone pitch), Aa/Shout, heart pupils at 1.0 for the cast trio |
| AFTERGLOW | 270–359 | sprawled limp, slow deep breathing | crossed eyes, Open+Triangle (tongue register), torogao savoring |

## Face honesty

Template sculpt with Prima palette/hair: heart pupils READ in the threshold render (pink heart inside the red iris ring at half-close). Coarse first-pass face, per D-654 honesty — not final art.
## Props

Rig: none (spreader bar is an act prop, skipped by brief). Arc scene: playroom couch, hypno screen (BUILD only), spreader bar (BUILD II–PEAK), hand toy prop.
## Deviations / named gaps

Sheet's bust trends larger in the 2D panels; the model follows the canon medium vector.

## Motion-clip format drops (named)

Carried: all keyed bone rotations (49 body tracks incl. finger curl), hips translation, all Fac_ morph channels keyed in the show (60 Hz grid). Dropped: prop animation (machine thrust, toy placement), outfit visibility swaps, set/camera, Blender bezier easing (clip is linear), and the eye-bone gaze keys are carried as bone tracks like any other bone. Bone names are renamed mixamorig:X -> mixamorig_X in the clip + def (the format's id charset bars ':'); the def document records the mapping.

## Rebuild 3 (D-1001, 2026-11-05) — hairline seam cure

Third build of this rig, fleet-wide under D-1001. One builder change, applied at the cause: the scalp-cap face-cut constant in `build_character.py` went 1.632 → 1.60, dropping the cap rim from polar ring 5 (canonical z 1.646, ~8 mm above the fringe top at 1.640 — the visible skin band) to ring 7 (z 1.6024), behind the fringe curtain. Hair part delta vs build 2: +10 verts / +12 faces (the two extra cap rings); every other part census identical. Build 2 is preserved in full under `v2/` in this directory.

- Census: Body 3084 verts / 4302 faces; 54 bones; Basis + 36 Fac_ morphs — exact vs build record.
- Torogao ingest: 4 meshes, 36 morph targets, expression check complete, governed 10, problems [], 7 threshold states resolve.
- BQ-5 posed strain (builder): {'hip_L': 0.5603, 'neck_weld': 0.4495, 'shoulder_L': 0.0, 'wrist_weld_L': 0.0835} — identical to build 2 (body untouched), all zones ≤ 0.64.
- Blink: white occlusion 1.0.
- **Hairline seam: GONE by measurement.** Ray-cast seam metric (`build/hairline_gate.py`; front rays over canonical z 1.590–1.675; seam band z 1.636–1.652): band skin-first fraction build 2 = 0.2537 → build 3 = 0.0000. Remaining skin-first rays sit at canonical z ≤ 1.602 — the open forehead below the fringe's lower edge, i.e. the face opening working as designed, not a seam remnant. Close-up sheet: `hairline-sheet.png` (crown + both temples, build 2 row over build 3 row); crops in `evidence-v2ref/` and `evidence-v3/`.
- Motion: neutral + arc re-rendered from the rebuild-3 geometry. Frame diffs vs build 2 are confined to narrow hairline bands (changed fraction ≤ 0.0006 at sampled frames). Clips re-exported and replay-deterministic through torogao-motion-clip/1, hashes identical to build 2 — neutral `a3770d899030940de7bbd06a2daa83a6d8dc441f7b90fc03b54907d25e18986c`, arc `84b8c442a16d66f17a4faf2b8460727be6649411dc3836771c67ad2df66ca8a4`.
- Maps: build 2's governed `expression-map.json` / `param-map.json` carried forward byte-identical (the rebuild changes geometry only). The R3 build initially copied the raw D-654 template maps (stale "Vivienne" stamp; heart pupils unrestored for the troupe); Gate C was re-run against the carried-forward maps and passed (evidence refreshed in `evidence/gate_torogao.json`).

## Conforming fix — D-1 six-girl standard (2026-11-06)

Additive layers landed on the build-3 rig per SIX-GIRL-STANDARD §4/§5/§6 (evidence: `night-school-20261105/runs/d1-conforming-fixes/`; build-3 originals preserved in `build3-original/`).

- **§4 bust parameter:** `Body_Bust_Size` shape key on Body + Outfit + OutfitEnd, emitted from the builder's own gaussian delta (value 1.0 = m 1.50, −1.0 = m 0.50, 0 = canon; body max delta 15.0 mm). `bust_param {amp_canon 0.030, 0.50, 1.50}` recorded in spec.
- **§5 jiggle cages:** `PrimaCage.L/R` (56 verts each), `Cage.L/R` mask groups + bound `CageDeform.L/R` Mesh Deform on Body, Outfit and OutfitEnd; `Bounce` keys at rest (unkeyed). Band 1.103–1.396 m, 236 verts.
- **§6 fluid anchors:** 7 `Anchor_Fluid_*` empties placed by the §6.2 rules at canon (max err vs §6.4 landmarks 0.041 mm; nipple anchors track m by ±12.68 mm, re-derivation tool `build/place_anchors.py`).
- **Identity gate:** decoded-pixel renders at canon identical to build-3 on full / bust-front / bust-side / face views (blink view identical too).
- **§4.5 gate:** cage re-derivation + jiggle determinism at m ∈ {0.50, 1.00, 1.50}: two independent runs pixel-identical at every setting; beat displacement max 14.9 / 15.2 / 16.4 mm; outfit apex margin positive at every setting (min over settings/sides 14.26 mm); hairline seam band at m-max: 0.0000.

## Pilot keeper set (Owner verdict, 2026-11-09)
Round-2 dream-style pilot set ruled ALL KEEPER by the Owner
("that's way better those are all great", 2026-11-09 20:50
PST): bust 1 ntranch-b25246875716, bust 2 ntranch-4a26ef9a4d38,
full 1 ntranch-e99e6461fcbf, full 2 ntranch-202e0fc7f959 —
generated under the 2026-11-09 canon revisions (normal warm
complexion, large bust). This is Prima's current reference
set for the dream-style finish. The 3D rig's bust vector
awaited its own ruling (see Canon revisions) — **ruled
m* = 1.50 and landed 2026-11-10 (D-1152)**, section below.

## Rig follow-up (D-1152, 2026-11-10) — bust canon m* = 1.50 landed

Owner ruled the §2 number from the rig-completion report:
**m* = 1.50**. Applied mechanically, no geometry rebuilt —
Basis untouched throughout (proof below).

- **Canon value:** `Body_Bust_Size` = 1.0 (= m 1.50) saved in
  `prima.blend` on Prima_Body, Prima_Outfit, Prima_OutfitEnd
  (verified by re-open: all three read 1.0). `prima.glb`
  re-exported with the fleet export route: Body_Bust_Size
  default morph weight 0 → 1 on all three meshes; Torogao
  ingest gate re-run on the new GLB: governed 10, problems [],
  result "complete". `bust_param.m_canon = 1.5` recorded in
  `prima_spec.json`.
- **§6 fluid anchors re-derived at m = 1.50** with
  `build/place_anchors.py` (saved): placement errors ≤ 0.004 mm;
  nipple anchors at y −0.13878 vs canon-m100 −0.12610 — the
  recorded ±12.68 mm track, exact.
- **§4.5 gates re-run at the new canon:** outfit apex margin
  (fleet `outfit_apex.py`, cage deform disabled) L 17.67 /
  R 17.67 / C 18.40 mm — identical to the D-1 m1.5 record, all
  positive. Jiggle beat at m = 1.50 run twice: decoded-pixel
  hashes and measures identical across runs AND identical to
  the 2026-11-06 m1.5 beat of record (beat displacement max
  16.4 mm, band 236 verts); outfit clearance min 6.95 mm.
- **Identity:** the updated blend rendered at bust value 0
  reproduces the D-1 canon stills' decoded-pixel hashes
  exactly on all five views (full / bust-front / bust-side /
  face / face-blink) — only the canon slider position and the
  anchors moved. Gate A re-run at the new canon: census
  agreement unchanged (54 bones, 3084 verts / 4302 faces),
  blink white occlusion 1.0, lash travel 19.33 px.
- **Arc re-rendered** from the new-canon blend (360 frames,
  24 fps, 15.0 s, h264, no audio): `anim-arc/prima-arc.mp4`
  replaced; the m = 1.00-canon render is preserved at
  `anim-arc/prima-arc-canon-m100.mp4` and its frames under
  `anim-arc/frames-canon-m100/`. Clip / skeleton / channels
  JSONs regenerated byte-identical to the landed set (the show
  does not key the bust channel), so the arc replay hash
  `84b8c442a1…` stands unchanged.
- Pre-change blend + GLB preserved under `pre-m150-20261110/`.
  Evidence bundle: `evidence-followup-20261110/` in this
  directory; lane report:
  `~/workspace/rig-completion-20261109/RIG-FOLLOWUP-20261110.md`.
