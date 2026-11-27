# Torogao in-engine 3D renderer — gap analysis (D-1213)

Commissioned 2026-11-10 (Owner, D-1213), following the
Prima gap analysis
(`~/workspace/nsfw-remakes-20261108/games/personality-excretion/PRIMA-TOROGAO-GAP-ANALYSIS.md`,
its item C5), which measured the blocking fact: **Torogao
has no in-engine 3D renderer.** This analysis prices
closing that gap. It is programme infrastructure under
D-457 (Torogao is the estate's singular build/runtime
engine), not a Prima-only feature: the Lane D programme
(27+ titles, dual-target Linux + Android per D-1094)
inherits whatever is built here.

**Analysis only.** No tree was modified. Primo state was
read live on 2026-11-10 (torogao clone
`~/.GH/NTRanch/torogao` @ main `6ffbffb`, matching the
verified push of record). Every number cites the artifact
it came from (D-713); anything not measured is marked
REVIEW, never asserted.

**One correction to the commissioning brief, up front:**
the brief describes the Torogao cast GLBs as carrying
"painted texture atlases per D-1152". Measured (§1.3):
the cast GLBs carry **no textures at all** — their paint
is vertex colour. The painted-atlas family is the
**Strobe** fleet (the D-1152/D-1164 texture route). The
renderer must serve both material models; the requirement
set in §2 covers both.

---

## 1. Census — what exists, measured

### 1.1 The Torogao tree (primo clone @ `6ffbffb`)

Workspace: 24 members, edition 2024, rust-version 1.85,
workspace version 0.4.0; `torogao-accel` is excluded by
design (own workspace, own gates). `Cargo.lock` resolves
566 packages (counted this run).

Render-adjacent stack, from `crates/torogao-app/Cargo.toml`
and the lock:

| Dependency | Version (lock) | Role today |
|---|---|---|
| `wgpu` | 29.0.4 | the app's 2D puppet renderer |
| `egui` / `egui-wgpu` / `egui-winit` | 0.35.0 | shell UI + wgpu presentation |
| `winit` | 0.30.13 | windowing (desktop + Android route) |
| `bytemuck`, `pollster`, `png` | 1 / 0.4 / 0.18 | buffer casts, adapter future, image decode |

**Absent from the lock** (grep this run, zero hits):
`gltf`, `glam`, `bevy`, `rend3`, and every audio playback
crate (`cpal` / `rodio` / `kira` / `symphonia`). There is
no glTF loader and no 3D math library in the tree.

What the current shell actually draws —
`crates/torogao-app/src/render.rs` (1,733 lines): a real,
in-house wgpu renderer for **evaluated 2D puppet parts**.
Per-part render pipelines (`HashMap<&'static str,
RenderPipeline>`), masked variants, a mask pipeline, a
line pipeline, offscreen `Rgba8UnormSrgb` targets with
`Depth24PlusStencil8`, `draw_part()` over `PartDraw`
records, adapter requested at `PowerPreference::
HighPerformance`. Two shader modules. Beside it:
`paint_window.rs` (a second wgpu surface), `headless.rs`
(155 lines; the `--headless-eval` path), `video_bridge.rs`,
`vtuber.rs`, `sdf_face.rs`. The estate's wgpu competence is
proven and current-version; what it renders is flat,
textured, masked 2D parts — no perspective camera, no
skinning, no morph targets, no 3D lighting model.

The pose side already exists and is deterministic:
`torogao-motion-clip` parses, validates, and replays
`torogao-motion-clip/1` skeletal records (bone TRS +
scalar channels, canonical 60 Hz) with replay hashes —
Prima's arc: 900 frames evaluated, replay hash
`84b8c442…`, `deterministic: true` (Prima RECORD.md).
`torogao-engine` (5,463 src lines per the lumen Stage 0
report) evaluates puppets — parameters, deformers,
fixed-step physics. `torogao-rig` independently evaluates
INP1 puppets with a measurement-grade **CPU** rasterizer
(2D). None of these render a 3D mesh.

GLB *data* competence exists at gate grade:
`torogao-gate` + `torogao-gate-cli` (branch
`pax/gate-crates-stage2` @ `7d4d13f`; PARITY-REPORT-2.md)
decode glTF accessors in Rust — including sparse
accessors — and ship census / geometry / morph-stats /
weights / anchors / channel-depth / blink-geometric
verbs, 54 tests passing, clippy/fmt clean. `torogao-vrm`
manipulates GLB at the JSON level for VRM 1.0 export
(byte-preserved geometry, read-back validation). What is
missing is specifically the **GPU half**: upload,
skinning, morph blending, shading.

Android today: `torogao-coin-android` — winit 0.30 +
`android-activity` 0.6 (native-activity), a `cdylib`
running the coin fixture and filling the window with a
success colour via `softbuffer` (CPU). Its own manifest:
"no rendering beyond a success-colour fill." **There is
no GPU rendering on Android anywhere in the tree.**

Where a renderer plugs in: a new workspace crate beside
`torogao-engine`, consumed by `torogao-app` (windowed +
headless paths both exist) and by game shells on the
fall-shell pattern. Nothing in the shell layer needs to
change shape to receive it.

### 1.2 Lumen's stack — what is and is not there to share

Record: `~/workspace/lumen-engine-20261104/SPEC.md`
(adopted, D-895) and `STAGE0-REPORT.md` in the same tree.

Stage 0's load-bearing finding (§2.2, item 1):
**"(Bevy runtime)" as Torogao's cell is falsified** —
`grep bevy` over Torogao's root manifest and every crate
manifest returns zero hits; Torogao's graphical surface
is egui/wgpu over a custom engine, and "Bevy is the SFW
hosts' runtime (D-861/D-825), never Torogao's." Inside
lumen itself, the entire Bevy surface is `src/bevy.rs`,
141 lines: request types for a JSON-RPC call to a
*running external* Bevy app (Stage 0 report §1). Lumen's
root `Cargo.toml` has **no bevy dependency**. There is no
`lumen-rig-bevy` crate. Conclusion for this analysis:
**there is no Bevy renderer anywhere in the estate to
embed, reuse, or share** — route (b) in §4 starts from
zero in-tree assets, exactly as a greenfield adoption
would, but inside a D-457 singular-engine estate.

What lumen does hold is the **2D puppet evaluator of
record** (D-869), with `torogao-rig` as the independent
Torogao-lane implementation (D-876): they share the INP1
format and the gate/measurement conventions, and no code.
D-895 DP-8 keeps that arrangement untouched — an engine
*consumes* the evaluator of record, never becomes one.
A 3D renderer duplicates none of this: the overlap risk
is only at pose evaluation, and Torogao's motion-clip
crate already owns pose evaluation in-tree. DP-6 also
stands: **D-457 is unamended until lumen Stage 4's
evidence pack** — any route that changes the engine's
shape must be weighed against that deferral (§4b).

### 1.3 The assets the renderer must serve — measured

**Family A — the Torogao cast** (canonical GLBs,
`~/workspace/torogao-models-20261104/<girl>/<girl>.glb`,
censused this run, per-file):

| Girl | Verts (all meshes) | Indices | GLB size |
|---|---|---|---|
| Prima | 22,340 | 26,796 | 3,172,560 B |
| Alma | 25,799 | 31,752 | 3,381,164 B |
| Lux | 21,260 | 25,380 | 3,233,872 B |
| Piper | 19,440 | 23,244 | 2,909,192 B |
| Sloane | 22,233 | 27,684 | 3,284,096 B |
| Yui | 18,604 | 22,212 | 2,763,408 B |

Uniform structure (all six, verified per file): 4 meshes
(`<Girl>_Body`, `_Hair`, `_Outfit`, `_OutfitEnd`), 1 skin
of **54 joints**, 59 nodes, 0 animations, **0 embedded
images**. Morph targets: Body carries **37** (36 named
`Fac_*` — the exact list is in the Prima gap analysis's
sources and in each GLB's `mesh.extras.targetNames` —
plus `Body_Bust_Size`); Outfit and OutfitEnd carry 1 each
(`Body_Bust_Size`) → **39 weighted target sets per girl**.
Morph deltas carry POSITION + NORMAL only. Every
primitive carries POSITION, NORMAL, TEXCOORD_0,
**COLOR_0 and COLOR_1** (two vertex-colour sets),
JOINTS_0, WEIGHTS_0. Materials: 2–3, flat, no
baseColorTexture — `ExportVC` (body/outfits) and `Hair`
(+ `HairAccent` on Lux). **The cast's paint is vertex
colour.** (Prima detail, from the attribute census: Body
13,920 verts; the gate stack's driven-target evidence —
e.g. `Fac_Eye_Close` nonzero-delta verts 232 of 13,920,
PARITY-REPORT-2.md — shows the deltas are **sparse**,
which §4a's design exploits.)

**Family B — the Strobe fleet, smoothed keepers**
(primo `~/.local/share/pax/strobe-smoothing-20261110/<girl>/<girl>.glb`,
all ten censused this run):

| Girl | Verts | Indices | GLB size |
|---|---|---|---|
| Vivienne | 169,481 | 741,024 | 30,201,216 B |
| Kaisa | 165,021 | 719,328 | 31,075,612 B |
| Cordelia | 159,599 | 698,976 | 30,696,788 B |
| Yuna | 147,477 | 633,216 | 28,978,600 B |
| Valentina | 145,760 | 630,912 | 28,767,784 B |
| Zuzana | 130,714 | 568,512 | 27,608,932 B |
| Maren | 130,604 | 565,632 | 27,362,328 B |
| Katya | 128,183 | 548,928 | 26,838,120 B |
| Sadie | 125,507 | 540,576 | 28,143,616 B |
| Manon | 113,045 | 490,176 | 25,318,572 B |

Same skeleton shape (4 meshes, 54 joints, 59 nodes).
Morphs: Body carries **36** targets (35 `Fac_*` +
`Fac_Pupil_Heart`; no `Body_Bust_Size` in this family);
the other meshes carry none. Materials are real PBR:
`Atlas_<Girl>_<Part>` with a baseColorTexture per part —
**4 embedded PNG atlases per girl, each 2048×2048**
(dimensions verified on Vivienne and Yuna; byte sizes
fleet-wide 24 KB–1.66 MB per image) — metallicFactor 0,
**roughnessFactor 0.75**, doubleSided true. COLOR_0 /
COLOR_1 / TEXCOORD_0 are also present. Fleet context
(SMOOTHING-REPORT.md, D-1188): subdivision took the
fleet 56,172 → 1,040,976 verts; these limit-surface GLBs
are the keeper artifacts of record (all ten Owner-ruled
keepers, D-1206/D-1207).

Computed from the measured figures (arithmetic, labelled
as such): one Strobe girl decoded for the GPU is
≈ 67 MB of atlas (4 × 2048 × 2048 × 4 B RGBA8) plus
≈ 28 MB of geometry + morph payload (Vivienne: 30,201,216 B
file − 2,223,194 B of PNG = 27,978,022 B binary chunk).
One cast girl is ≈ 3 MB all-in. These two families differ
by an order of magnitude in every budget that matters.

### 1.4 The plates of record — what "matching" means

The renders a 3D presentation is compared against are
**Blender EEVEE** renders under a fixed rig
(`gate_fleet.py` in each cast girl's directory; rig
constants quoted in the gate-crates CAPABILITY-MAP.md
§1): EEVEE at **1024×1280**, world colour
(0.55, 0.55, 0.6), **Sun 3.0 at (55°, −25°)**, **Fill 1.2
at (70°, 140°)**, camera placed at spec-scaled targets,
morphs driven by name (`Fac_Eye_Close`, `Fac_Mth_Aa`,
gaze via eye bones at 0.30 rad, Head at 0.35 rad).
Prima's arc of record: 360 frames at 24 fps rendered
from the blend (`anim-arc/frames-canon-m100/`, mp4
beside it), and the D-1 canon stills' **decoded-pixel
hashes reproduce identically across runs** (Prima
RECORD.md) — pixel-hash determinism on a fixed machine
is already the estate's working standard for plates.

So the comparison target is not Cycles and not
"photoreal": it is a **fixed two-directional-light EEVEE
rig over vertex-colour / atlas albedo**. That is what
makes in-engine parity a tractable engineering problem
instead of an open-ended one (§2, R5).

### 1.5 The gate programme's render probe — today's state

`torogao-gate` (Stage 2 complete, §1.1) owns every
*geometry* quantity. Every *pixel* quantity in the gate
family — channel-depth iris/white counts, blink ratios,
lash travel, gaze shift (CAPABILITY-MAP.md classes (b)) —
is today served by the Blender EEVEE renders of §1.4, or
by a **geometric substitute** (blink-geometric) that is
explicitly a successor metric, "not the same number"
(CAPABILITY-MAP §2). The gate DESIGN.md (Phase 3) defines
`render-gate` / `render-blink` / `render-aa` verbs whose
contract reads: *"renders come from Torogao's renderer
or the bridge's Blender batch; this verb owns only the
pixel statistics."* The gate programme has already named
an in-engine renderer as one of its two planned render
sources; Phase 3 cannot complete its render half without
either it or a permanent Blender-batch dependency. The
CPU pose evaluator those verbs also need (`stress-pose`,
LBS skinning over glTF data) is likewise Phase 3 and
unbuilt — it is the same mathematics as the renderer's
skinning, on the other side of the GPU boundary (§4a
exploits this as a cross-check).

---

## 2. Requirement set — derived from the assets + estate law

Each requirement names its source. "Of record" = the
census figures of §1.3 and the rig of §1.4.

- **R1 — Skinned-mesh rendering.** Linear-blend skinning,
  54 joints, JOINTS_0/WEIGHTS_0 (4 influences), one skin
  shared by 4 meshes, from the rigs of record. (Source:
  §1.3 census.) Poses arrive as evaluated bone TRS from
  torogao-motion-clip — the renderer never evaluates
  clips itself. (Source: §1.1, D-869/DP-8 posture.)
- **R2 — Morph-target blending at the counts of record.**
  Up to **39 simultaneous weighted targets** (cast) /
  36 (Strobe), POSITION + NORMAL deltas, applied before
  skinning (glTF order), weights driven by name from the
  same `Fac_*` vocabulary the gates drive. Deltas are
  sparse; the representation must not pay dense cost for
  sparse data. (Source: §1.3; gate driven-set evidence.)
- **R3 — Albedo: vertex colour × optional atlas.**
  COLOR_0 multiplies the base colour; Strobe parts
  sample their 2048×2048 atlas via TEXCOORD_0; cast
  parts render from vertex colour alone. **COLOR_1's
  semantic role is REVIEW** — it is present on every
  primitive of both families and its meaning lives in
  the paint pipeline, not in any artifact read here; the
  renderer must carry it and must not guess its role.
- **R4 — Camera/framing parity with the plates.**
  Reproduce the gate rig's compositions: 1024×1280
  portrait framing, camera at the spec-scaled targets
  the gate scripts compute (their constants are in
  `gate_fleet.py` / CAPABILITY-MAP §1). Without this,
  no frame comparison against a plate is meaningful.
- **R5 — Lighting/shading: EEVEE-rig equivalence, not
  Cycles parity.** Target model, named: **albedo
  (R3) under the fixed two-light rig — Sun 3.0 @
  (55°, −25°), Fill 1.2 @ (70°, 140°), world
  (0.55, 0.55, 0.6) as ambient — with a half-Lambert
  wrapped diffuse (the anime register the plates read
  in) and a single GGX specular lobe at the materials'
  measured roughness (Strobe 0.75; cast flat), metallic
  0, output through an AgX-approximating tonemap.**
  Why this model: the plates are EEVEE renders of
  unlit-by-design painted surfaces — the paint carries
  the detail, the lights carry the modelling. Full
  Cycles parity is explicitly not the bar (different
  integrator, path-traced GI; chasing it costs a
  programme and buys nothing the eye gate can see at
  these material settings). The honest residual costs,
  named: (i) Blender's AgX view transform is a fitted
  filmic curve — an approximation will differ in
  highlight roll-off, measurably, at the frame-diff
  level; (ii) EEVEE's specular response to a wrapped
  diffuse is not a standard BRDF pairing, so the lobe
  is tuned against plates, not derived; (iii) whether
  the plate rig casts **shadows** is REVIEW — the gate
  scripts as read set lights and world but the shadow
  configuration was not verified, and self-shadowing is
  the largest single shading cost if it is in the
  plates. Rung 1's comparison (§5) is the arbiter of
  "close enough"; the Owner's eye is its gate (D-221).
- **R6 — Determinism (D-212).** Pose and morph weights
  are evaluated from frozen rig/clip data at fixed steps
  (motion-clip's canonical 60 Hz; the arc plates are
  24 fps presentations of the same evaluation). The
  render path contains no wall-clock input — no
  time-based noise, no frame-rate-dependent integration.
  Identical evaluated state → identical frame, checked
  by **frame hash on a fixed machine/adapter** (the
  standard the D-1 stills already meet under Blender).
  Stated precisely: wgpu does not guarantee bit-identical
  pixels across different GPUs/vendors, and this
  analysis does not promise it; cross-machine agreement
  is a **numeric frame-diff bound** (the arc pipeline's
  own idiom — "changed fraction ≤ 0.0006 at sampled
  frames", Prima RECORD.md — is the precedent shape),
  while D-212's cross-platform *trajectory* hashes live
  in the evaluation layer above the renderer and are
  unaffected by it.
- **R7 — Dual target (D-1094).** One crate renders on
  Linux desktop and on Android through the winit
  `android-activity` route the estate has already proven
  for windowing. Renderer-choice implications, from the
  stack's evidence: wgpu 29's Android backends are
  Vulkan (primary) and GLES; a morph/skinning design
  built on **vertex-stage storage buffers requires
  Vulkan or GLES 3.1+** — a GLES 3.0 floor would force a
  second, texture-based morph path (two parity surfaces).
  The in-tree Android evidence today is a CPU-filled
  window (§1.1); no estate code has yet created a GPU
  surface on Android, so Android GPU behaviour is
  **unmeasured in-estate** and is priced as a rung, not
  assumed (§5, Rung 5).
- **R8 — Performance envelope.** Frame budget for the
  cast scenes of record: **60 fps (16.6 ms) at
  1024×1280-class internal resolution on primo-class
  hardware** — at ≤ 26k verts / ≤ 39 morphs (Family A)
  this is not a stretch target; the binding budget is
  Family B on Android: **one smoothed character
  (≤ 741k indices, 36 morphs, ≈ 95 MB decoded payload)
  at ≥ 30 fps on a mid-tier Android device**, morph +
  skin vertex cost dominant. The Android half of this
  envelope is a **target, REVIEW until Rung 5 measures
  it** — no Android GPU measurement exists in the estate
  to cite.

---

## 3. The gap list

Severity: **HIGH** = blocks any in-engine 3D frame;
**MED** = blocks a named capability or gate; **LOW** =
blocks production polish, not proof.

| # | Missing capability | Severity | Blocks |
|---|---|---|---|
| G1 | GLB → GPU loader: accessor decode exists at gate grade (`torogao-gate`); interleaving, GPU upload, texture decode/upload do not | HIGH | everything — no mesh reaches the GPU today |
| G2 | GPU linear-blend skinning (54 joints) | HIGH | any posed render; the CPU twin (gate `stress-pose`) is also unbuilt |
| G3 | GPU morph blending at 36–39 targets over sparse deltas | HIGH | all facial performance — the entire `Fac_*` system exists only as gate data today |
| G4 | In-engine camera reproducing the plate rig's framing math (R4) | HIGH | rung-1 comparison itself; broadcast camera selection (Prima A9) permanently |
| G5 | The R5 shading model: two-light rig, wrapped diffuse, GGX lobe, AgX-approximating tonemap; COLOR_1 semantics (REVIEW) | MED | eye-parity with plates; not first pixels |
| G6 | Deterministic offscreen 3D render + readback + frame-hash discipline (the app's offscreen path is 2D-only) | MED | the D-212 render gate; gate Phase 3 render source |
| G7 | Android GPU surface (wgpu inside the android-activity shell) | MED | D-1094's Android half for any 3D-presented title |
| G8 | Pose plumbing: motion-clip evaluated pose + named morph weights → renderer per fixed step | MED | any animated frame; the replay-hash chain currently ends at numbers, not pixels |
| G9 | Stage/backdrop hook (background plates, letterbox) for production scenes | LOW | shipped scenes; rung comparisons are character-against-world-colour by design of the plate rig |
| G10 | Shadow configuration of the plates of record | REVIEW | potentially G5's scope — unverified in the gate scripts as read; if the plates self-shadow, shading parity costs a shadow pass |

What is **not** a gap: pose evaluation (motion-clip),
GLB data reading (gate), 2D presentation (engine/app),
windowing on both targets (winit routes proven),
determinism discipline (state/trajectory hashes are
estate-standard). The gap is exactly one layer thick —
**evaluated pose in, pixels out** — and it is entirely
unbuilt.

---

## 4. Routes — priced

Size classes follow the Prima analysis: **rung** = one
S4-style increment; **stage** = several rungs under one
gate; **programme** = cross-cutting, more than one title
consumes it.

### (a) A wgpu-native renderer crate inside Torogao — RECOMMENDED

Working label `torogao-render3d`. Built on what the
census confirms is already in the tree: wgpu 29.0.4 +
egui-wgpu 0.35 + winit 0.30 (the app's own stack), a
1,733-line in-house wgpu renderer proving device /
surface / offscreen competence, `torogao-gate`'s
accessor decoding to reuse for the loader, and
motion-clip for pose.

**Honest scope of the new work.** The loader is the
smallest piece (gate-grade decode exists; the new code
is buffer layout + upload + PNG decode via the `png`
crate already in the app's graph, or the `gltf` crate —
mature 1.x line — if its importer proves cleaner than
extending the gate decoder; that choice is a rung-1
implementation detail, not a design fork). The real
work is three shaders' worth of vertex-stage machinery:
(1) **morph accumulation** — weighted POSITION + NORMAL
deltas from a packed sparse-delta buffer (gate evidence:
driven targets touch hundreds of verts, not thousands —
Prima's Close moves 232 of 13,920 — so sparse lists,
not dense target textures, are the right layout, and
the gate crate already decodes the sparse form);
(2) **LBS skinning** — 54 joint matrices, 4 influences,
textbook math whose CPU twin the gate programme already
plans (`stress-pose`, Phase 3): the two implementations
cross-check each other vert-for-vert, so the estate's
most error-prone new math arrives with its own
independent witness; (3) the **R5 fragment model** —
deliberately small (two directional lights, wrapped
diffuse, one GGX lobe, tonemap), tuned against plates.
Plus the unglamorous halves: a deterministic offscreen
target + readback + hash (G6), the plate-framing camera
(G4), and the Android surface (G7).

**Price: programme — 6 rungs** (§5). Risks, named:
vertex-stage skinning/morph WGSL is genuinely new code
for the estate (nothing in the tree does it); wgpu
major-version churn is annual and the crate rides it
(the app already does — same exposure, not new); AgX
parity is approximate by construction (R5); Android
storage-buffer floors (R7) are unmeasured in-estate.
Determinism fit: **best of the four routes** — the
estate owns pipeline state, draw order, and every float
in the path. Android fit: wgpu is the only GPU path the
estate's Android route (winit/android-activity) already
speaks. Maintenance posture: estate-owned, Tiger Style,
`#![forbid(unsafe_code)]`-compatible (wgpu's safe API
suffices throughout), no new upstream to track beyond
wgpu/egui, which are already tracked.

### (b) Embed an existing engine renderer (Bevy render) — REJECTED

Rejected on estate evidence and estate law, not on
Bevy's quality:

- **There is nothing to embed from.** §1.2: the estate's
  entire Bevy surface is a 141-line RPC client to an
  external Bevy process; no Bevy renderer, scene, or
  asset pipeline exists in any tree. This route is a
  greenfield adoption wearing a reuse costume.
- **D-457.** Torogao is the singular build/runtime
  engine; adopted capabilities land in Torogao layers.
  Bevy's renderer is not a library-shaped part — it
  arrives with its ECS, asset server, scene format, and
  scheduler. Embedding it makes the estate's "singular
  engine" a host shell around a second engine, i.e.
  precisely the engine-hopping D-457 ended, with the
  seam inside the binary.
- **D-895 DP-6.** The D-457 amendment choice is deferred
  to lumen Stage 4's evidence pack. Route (b) pre-empts
  that ruling by fait accompli.
- **Determinism.** Bevy renders inside its app
  scheduler; fixed-step evaluation feeding render is
  achievable but fights the framework's grain — the
  estate's D-212 contract (frozen data, fixed steps, no
  wall-clock) is native to route (a) and adversarial to
  route (b).
- **Churn.** Bevy's release cadence and breaking-change
  rate exceed wgpu's; the tree's lock (566 packages,
  zero bevy) absorbs a second full dependency universe
  for one capability.

### (c) A third-party renderer crate (rend3 class) — REJECTED

The named exemplar disqualifies itself on measured
facts (GitHub, read this run): **rend3's repository
was archived by its owner on 2025-06-07, in "maintenance
mode" since May 2024, and its last wgpu sync targets
wgpu 0.20** — nine major versions behind the tree's
29.0.4. Adopting it means either a second, stale wgpu
in the lock (two GPU stacks, doubled surface area) or a
fork-and-upgrade project — the visible fate of its
remaining users is self-maintained forks chasing wgpu
versions. Licence is not the killer (rend3 is
MIT/Apache-2.0); **abandonment + version skew is.**

The wider class does not rescue the route: the
surviving wgpu renderers are single-maintainer,
WebGPU-profile-first designs whose storage-buffer
assumptions are exactly wrong for the Android GLES tail
(R7), none of them speaks the estate's determinism
contract or its glTF morph+sparse-delta shape out of
the box, and each would still need the loader, camera,
and shading-parity work of route (a) around it. What
the class is good for is what Lane D doctrine already
says: **techniques transfer, code doesn't** — rend3's
culling and material-manager designs are study material
for route (a)'s author, cited, not linked.

### (d) The null route — Blender offline plates forever (contrast)

What it costs nothing: zero build. EEVEE parity is
perfect by definition (the plates *are* the ground
truth), and the arc pipeline's determinism is proven.
For stills, arcs, and verdict sheets it remains the
right tool permanently — route (a) does not retire it.

What the programme loses, priced concretely:

- **Prima's camera selector (A9) is unimplementable.**
  The broadcast fiction's camera is a *runtime* choice
  among framings of a live scene. Plates can pre-render
  a finite framing set, but every runtime combination —
  camera × stage (11) × phase (5) × performance set —
  multiplies the render farm's output; the design's
  reactive staging (A8 cold opens, marked scenes,
  rewatch restaging, A10) becomes a combinatorial
  pre-render obligation instead of a sequencer playing
  evaluated state.
- **Content volume scales multiplicatively.** Under
  plates, every pose/expression/timing change is a
  Blender farm job; under a renderer, it is a parameter
  change. The Prima C10 content programme (11 stages ×
  5 phases of performance sets) is priced in the Prima
  analysis as a programme on the hybrid route; on the
  pure-plate route every one of those sets must exist
  as rendered frames for every camera the fiction
  offers.
- **The gate programme keeps a permanent dependency.**
  Phase 3's render verbs (gate DESIGN.md) name
  "Torogao's renderer or the bridge's Blender batch" —
  null route makes it Blender batch forever: every
  render-gate run pays scene construction in a live
  `.blend`, and the render-dependent gate quantities
  stay split between EEVEE numbers and geometric
  substitutes that are "not the same number."
- **D-457 keeps a named hole.** "Present a governed 3D
  character interactively" stays a permanent
  TOROGAO-MODEL-GAP in the singular engine — the exact
  class of gap D-457 exists to close — and every future
  3D-presented title in a 27+-title queue re-pays the
  plate route's multiplicative cost.

The null route is the correct price of the gap itself:
it is what *not* building costs, per title, forever.

### Comparison

| | (a) wgpu-native | (b) Bevy embed | (c) rend3 class | (d) null |
|---|---|---|---|---|
| In-tree assets reused | wgpu 29 stack, gate decode, motion-clip, app shell | none (§1.2) | none | Blender pipeline |
| New work | loader + 3 shader systems + camera + Android surface | a second engine, integrated | fork/upgrade + all of (a)'s parity work anyway | none |
| Determinism (D-212) | native | adversarial | uncontracted | proven (plates only) |
| Android (D-1094) | wgpu Vulkan/GLES, one crate | unproven in-estate | WebGPU-profile assumptions | n/a (no runtime 3D) |
| D-457 posture | capability lands in Torogao | violates as built | dependency, not a layer | permanent model gap |
| Maintenance | estate-owned | upstream engine cadence | archived / single-maintainer | Blender version drift |
| Price | programme, 6 rungs | programme+, plus a D-457 ruling it cannot supply | stage to fork, programme to trust | 0 now, multiplicative per title forever |

**Recommendation: route (a).** It is the only route that
(a) starts from measured in-tree assets, (b) satisfies
D-212 and D-1094 from the same crate, (c) lands the
capability inside the singular engine as D-457
requires, and (d) has a priced, staged plan whose first
rung is decided by the Owner's eye against the plates
of record — the estate's standing way of knowing.

---

## 5. Rung plan — route (a), each rung with its acceptance proof

Rungs are S4-class increments: local commits on a gated
branch, cargo test half validation / half adversarial,
clippy `-D warnings`, fmt, theodolite on touched files,
no pushes without their own authorization. The renderer
crate lands in the torogao workspace (working label
`torogao-render3d`).

**Rung 1 — the vertical slice (proof fixed by the
commission).** Loader (G1) + morphs (G3) + skinning (G2)
+ plate camera (G4) + a first R5 shading pass, for
**one cast girl: Prima** — her arc, her plates, her
RECORD are the most complete in the fleet.
*Acceptance proof (fixed):* one already-gated arc —
Prima's 360-frame solo arc, driven by its motion-clip
of record (replay hash `84b8c442…`) — rendered in-engine
at the plate rig's framing, sampled frames compared
against the **Blender plates of record**
(`anim-arc/frames-canon-m100/`): **Owner eye verdict**
under the D-221 doctrine (expected-vs-seen, crops at
viewing size) **plus a numeric frame comparison**
(changed-fraction and per-channel deltas at sampled
frames; the bound is set from the measurement, reported
with the frames, and ruled at the gate — not invented
here). Supporting gates: twin-run frame hashes identical
on the dev machine (G6 discipline starts here); the
CPU-LBS cross-check — renderer-skinned verts vs the
gate crate's morph/skin arithmetic on CPU for the driven
targets — agrees within a stated tolerance, or the rung
reports the divergence instead of shipping it.

**Rung 2 — determinism + the whole cast.** All six cast
GLBs load and render through the same path; the
headless render binary (the `--headless-eval` posture)
emits frame hashes for a fixed pose/morph battery.
*Proof:* twin-run identical hashes across the battery
on the dev machine; cross-machine numeric diff within
the Rung-1-ruled bound; cast-wide census parity —
rendered vert/morph counts equal the gate census for
every girl (the renderer reads what the gate reads).

**Rung 3 — shading parity.** The full R5 model:
two-light rig as uniforms, wrapped diffuse, GGX lobe at
measured roughness, AgX-approximating tonemap, COLOR_1
resolved from the paint pipeline (its semantics
established from source, not guessed) and shaded
correctly; G10 answered (shadow configuration of the
plates read from the gate scripts' full text; if the
plates self-shadow, the shadow pass lands here).
*Proof:* the gate family's own driven set — neutral /
`Fac_Eye_Close` / gaze / `Fac_Mth_Aa` at plate framing
for two girls (one cast, Vivienne for the atlas family
at clay-for-rung purposes or smoothed — see Rung 4) —
passes the **render-gate pixel statistics** (the
channel-depth masks and ratios of CAPABILITY-MAP §1)
computed over in-engine renders, agreeing with the
EEVEE-computed values within the calibration bound the
gate programme sets; Owner eye on the verdict pack.

**Rung 4 — the Strobe family at keeper density.**
Atlas path (2048² × 4 per girl), Family B morph set,
the smoothed GLBs as-is.
*Proof:* all ten keepers render in-engine; Vivienne's
verdict-pack views (the D-1206 pack compositions)
reproduced in-engine and eye-compared by the Owner
against the smoothed sheets of record; perf gate: the
R8 desktop envelope measured (frame times logged over
the arc, worst frame reported, not averaged away);
memory: decoded payload per girl within the §1.3
computed budget.

**Rung 5 — Android.** wgpu surface inside an
android-activity shell on the coin-android precedent;
backend selection measured (Vulkan first; the GLES
floor question of R7 answered with device evidence);
Family A scene + one Family B character.
*Proof (D-1094/D-212):* APK builds on primo; the
trajectory/state hashes of the shared fixture equal the
desktop run (that equality lives in the evaluation
layer — the renderer must not disturb it); on-device or
emulator smoke with frame times for both families
against the R8 Android envelope; backend + device
named in the report — no "Android works" without the
hardware it was measured on.

**Rung 6 — integration.** `torogao-app` stage mode
consumes the renderer beside the puppet stage; game
shells can bind it on the fall-shell frame contract;
the gate programme's Phase 3 render verbs gain their
second render source.
*Proof:* shell/core hash parity on the Prima fixture
(the Fall shell proof shape); `render-blink` run
against an in-engine isolation render reports its
statistics beside the Blender-sourced numbers for one
girl, calibration deltas named; the app's windowed and
headless modes produce identical frame hashes for the
same evaluated state.

After Rung 6 the Prima C5 fork is closable on evidence:
route (ii) exists, priced and gated, and Prima's C4
sequencer stages against a live renderer instead of a
plate inventory.

---

## 6. Open questions — genuinely the Owner's

1. **The C5 ruling itself.** This analysis prices route
   (ii); ruling Prima (and the programme) onto in-engine
   3D — vs the hybrid (iii) for Prima v1 with the
   renderer landing behind it — is the decision the
   analysis feeds. Sequencing is part of the same call:
   the renderer programme (6 rungs) ahead of Prima's
   critical path delays Prima; in parallel, Prima ships
   hybrid and migrates. (§4d prices both postures;
   neither is assumable by the build.)
2. **Shipping density for Family B.** The keeper GLBs
   are limit-surface (113k–169k verts, 25–31 MB each).
   Gates must serve them as-is. Whether *shipped games*
   present at that density, or a game-density export
   (clay / LOD) is commissioned as content, is an Owner
   call with direct Android-budget consequences (R8) —
   the renderer is built to the keepers either way, but
   Rung 5's envelope verdict depends on the answer.
3. **The Android floor.** Which Android GPU generation
   is the programme's floor decides Vulkan-only vs a
   GLES fallback path (R7) — a second morph path is a
   real, priceable scope increase, and only the Owner
   can set the floor it buys.
4. **Shadows, if G10 resolves to "the plates
   self-shadow."** Matching self-shadowing is the
   largest single shading cost in R5's scope. Whether
   plate-parity at that depth is worth a rung is an
   eye-and-value call the Owner makes at Rung 3's gate,
   with both renders in front of him — it is listed
   here so the possible scope is visible now, not
   discovered then.
5. **Gate Phase 3's render source.** When the renderer
   lands, does it become the render-gate's source of
   record (Blender batch demoted to calibration
   reference), or do both sources stay live
   indefinitely? The gate DESIGN.md leaves the choice
   open by construction; it is a doctrine call about
   which pixels the estate trusts, and it is the
   Owner's.

---

Validation note: this analysis is prose over the cited
artifacts — **not-LSP-validated**. Census commands were
run live on 2026-11-10 against primo main `6ffbffb`,
the gate-crates stage-2 worktree `7d4d13f`, and the
named local trees; tree claims should be re-verified
against HEAD before any build rung prices them.
Bloch record for this rung: origin_lane =
`ntranch/torogao-engine`, content_label = `sfw`
(engine measurement/infrastructure; no title content),
evidence = this file plus the census artifacts it cites.
