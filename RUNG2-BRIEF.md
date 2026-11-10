# Torogao renderer — RUNG 2 brief

**Status:** commissioned by the Owner, 2026-11-10. This brief
governs all work on branch `pax/renderer-rung2-20261110` in the
torogao repo checkout (primo
`~/.GH/NTRanch/torogao`, base = rung-1 tip
`63896dd`, off torogao main `6ffbffb`). Local commits only.

**Reading order for a fresh agent:** `CLAUDE.md` (this tree's root)
→ this brief → the rung-1 verdict pack
(`~/workspace/torogao-renderer-rung1-20261110/verdict-pack/VERDICT.md`)
→ the module you are picking up.

---

## 0. Citation discipline (Owner requirement)

Every technical claim in this brief carries a citation, in one of
two forms:

- **Estate claims** cite the estate artifact that measured them
  (file path + the item/section inside it). Estate files are the
  authority for estate facts; no web source outranks a measurement
  taken on primo.
- **External claims** cite a source that was opened during the
  authoring of this brief, as a Markdown link with a one-line
  statement of what the source establishes. Appendix A collects
  them.

A claim that could not be sourced is marked **REVIEW** and listed
in Appendix B. REVIEW means "not yet established" — it is neither
true nor false, and no module may silently build on a REVIEW claim
without either discharging it by measurement or flagging the
dependency in its gate report (estate doctrine D-713:
provenance-first numbers; unverifiable routes to review).

---

## 1. Mission and bars

**Mission.** Take `torogao-render3d` from rung 1 — one character,
deterministic, proven against Blender plates — to the estate's
scene renderer: full scenes, animated in-engine, rendered silently
to files and video, reproducible from replay hashes. The renderer
is programme infrastructure under D-457 (Torogao is the estate's
singular build/runtime engine), not a Prima-only feature (gap
analysis, preamble).

**The three bars, in the order they gate:**

1. **Keeper quality only.** A frame ships when it would survive
   the Owner's eye beside its reference. The verdict-pack doctrine
   (§9) is how visual claims are made: expected-vs-seen written
   first, crops from the FINAL artifact at viewing size, an honest
   FAIL exemplar beside each item where one exists, one-sentence
   stakes per item. The agent never declares an Owner PASS.
2. **The two-body scene is the stress test.** Solo-character
   rendering is rung 1's proven ground. The capability this rung
   exists to prove is two characters in believable contact —
   touching, holding, pressing, carrying weight — with contact
   that reads as contact (no interpenetration, no hovering, no
   sliding through), because every target scene class (Laplace3D
   reference scenes; Prima's scenes) is built from it.
3. **Determinism (D-212).** Same inputs → byte-identical frames
   on the bench machine, replay hashes as gates. Fixed-step
   evaluation and simulation; no wall-clock input anywhere. The
   classic statement of why variable timesteps destroy
   reproducibility, and of the fixed-step accumulator that
   prevents it, is Fiedler's *Fix Your Timestep!* — fully fixed
   delta time is what buys "exact reproducibility from one run to
   the next given the same inputs"
   ([Fiedler, *Fix Your Timestep!*](https://github.com/laurentongaro/odinrag/blob/HEAD/odin-knowledge-base/references/fix_your_timestep.md)
   — establishes the fixed-step/accumulator pattern and its
   reproducibility argument). Cross-machine pixel identity is
   **not** promised: the gap analysis (R6) fixes the contract as
   bit-identity on a fixed machine/adapter plus a numeric
   frame-diff bound across machines.

**Scene classes in scope:**

- **Cast scenes** — any subset of the six Torogao cast girls
  (Prima, Alma, Lux, Piper, Sloane, Yui), posed by motion-clips,
  on sets, under rigs beyond the plate rig.
- **Laplace3D-class scenes** — multi-character scenes at the
  quality bar of the estate's Laplace3D reference material. Owner
  ruling D-196: the cross-medium animation pipeline's quality
  target is "as good as or better than Laplace3D animations,
  measured against criteria from the estate's Laplace reference
  material" (estate memory of record; the measurement criteria
  are a fathom commission, §5 F3 — the internal techniques of the
  reference material are not public and are **REVIEW**, App. B).
- **Prima-class scenes** — scenes in the class of Personality
  Excretion Simulator (JinkakuHaisetsu, held at primo
  `~/.local/share/ntranch-game-re/JinkakuHaisetsu/`), Prima's
  design template (Prima gap analysis,
  `~/workspace/nsfw-remakes-20261108/games/personality-excretion/PRIMA-TOROGAO-GAP-ANALYSIS.md`):
  staged solo and duo scenes with emission/fluid beats, reactive
  staging, and runtime camera selection — the capabilities the
  gap analysis §4(d) priced as unimplementable on the plate route.

**Out of scope for this rung:** the Android GPU surface (gap G7 —
priced as its own later rung, unmeasured in-estate, **REVIEW**);
shipping-density/LOD decisions for the Strobe family (Owner call,
gap analysis open question 2); any push, release, or publication.

---

## 2. Where rung 1 stands (estate state, cited)

### 2.1 What rung 1 built

`crates/torogao-render3d` (5,830 lines at the rung-2 base commit):
a Vulkan-native, headless, skinned-3D renderer via **ash 0.38**
(GLSL 460 + glslc, SPIR-V). Ash is "a very lightweight wrapper
around Vulkan", generated from `vk.xml`, in which "no validation,
everything is unsafe"
([ash 0.38.0+1.3.281, docs.rs](https://docs.rs/crate/ash/0.38.0+1.3.281)
— establishes ash's shape and why it must live behind a boundary).
The crate's layering (its own `lib.rs` doc): everything above
`src/backend/` is API-agnostic — GLB decoding (`accessor.rs`,
`model.rs`), sparse morph packing (`morph.rs`), deterministic
pose evaluation reusing `torogao-motion-clip` (`pose.rs`), a CPU
morph+LBS reference (`reference.rs`), the procedural arc scene
(`setscene.rs`), plate-comparison statistics (`compare.rs`). The
backend boundary (`src/backend/mod.rs`) is a 3-method
`RenderBackend` trait over plain-data packets (`PackedScene`,
`DrawFrame`, `RenderedFrame`); the sole unsafe island is
`src/backend/vulkan.rs` (1,832 lines). Proof binaries:
`render_arc`, `crosscheck`, `compare`; proving suite in
`tests/proving.rs`.

### 2.2 What rung 1 proved (verdict pack, item by item)

From `verdict-pack/VERDICT.md` (Prima's 360-frame arc, rendered
vs the Blender plates of record):

- **Framing, camera, set, schedule — MATCH** (item 1). Set pieces
  appear/vanish on the plate schedule; composition is the plates'.
- **Paint — MATCH** (item 2). The COLOR_1 question is **resolved
  by measurement**: COLOR_1 is the paint; COLOR_0 is constant
  white on the cast GLBs. (This discharges gap analysis R3's
  REVIEW.) Materials' base colours are baked into COLOR_1 and
  must not multiply again — see `PackedMesh::paint_from_color1`.
- **Face and morphs — MATCH** (item 3). The CPU reference
  reproduces Blender's evaluated vertices at witness points to
  2.0e-7 … 5.6e-7 m.
- **Joint placement — MATCH** (item 4): frame-0 joint world
  positions equal Blender's pose-bone positions exactly.
- **Limb articulation — DIVERGED at rung 1, FIXED by D-1223**
  (item 5 + `ADDENDUM-ITEM5.md`). Root cause: clip rotation tracks
  were authored in Blender bone frames while the GLB rig's frames
  differ per bone. The Owner-ruled fix landed in **gen_clip**
  (both live copies on primo), writing each key as
  `normalize(rest ⊗ delta)`; measured per-bone residuals vs
  Blender collapsed from ~180° to **≤ 0.0560° worst bone** (the
  f32 GLB-storage noise floor). New arc replay hash:
  `1bd9dd534a0ea37f8e827e5d9e7fcdcac3d8cebfd53401c3cd05b9be46e986eb`.
  The renderer, clip format, and evaluator were **not** changed.
  The other five girls' clips were **not** regenerated — that is
  module M1.
- **Light level — BRIGHTER than plates, parity not claimed**
  (item 6). Full-frame mean signed Δ ≈ (+42.5, +43.5, +41.7)/255
  over 60 sampled frames; the delta decomposes into wrap-vs-plain
  diffuse (≈1.37× sun term, ≈1.96× fill term at the floor
  normal), EEVEE light-energy semantics, and a +9…+12/255
  AgX-fit residual. No ad-hoc retune was applied. Closing this
  is module M6.
- **Self-shadowing — PARTIAL** (item 7). The plates self-shadow
  from **both** lights (this discharges gap G10). Rung 1
  implements the Sun's shadow only (2048², 3×3 PCF); the Fill
  shadow is a named omission — module M6.
- **Determinism — EXACT** (item 8): twin 360-frame renders on the
  A6000, per-frame RGBA SHA-256 identical 360/360.
- **GPU skinning vs CPU reference — EXACT within bound** (item
  9): bound stated before the run, positions ≤ 1e-4 m, normals
  ≤ 1e-3; measured max Δ 2.68e-7 m / 2.38e-7 over all 13,920 Body
  vertices × 6 frames.

### 2.3 The asset facts every module builds on

From the gap analysis census (§1.3, measured per file):

- **Cast family (Family A):** six GLBs, 18.6k–25.8k verts each,
  4 meshes (Body, Hair, Outfit, OutfitEnd), **1 skin of 54
  joints**, 59 nodes, 0 animations, 0 embedded images. Morph
  targets: Body carries 37 (36 `Fac_*` + `Body_Bust_Size`);
  Outfit/OutfitEnd carry 1 each → **39 weighted target sets per
  girl**, POSITION + NORMAL deltas only, and **sparse** (e.g.
  `Fac_Eye_Close` moves 232 of Prima's 13,920 Body verts). Every
  primitive carries POSITION, NORMAL, TEXCOORD_0, COLOR_0,
  COLOR_1, JOINTS_0, WEIGHTS_0. glTF's own model is the contract
  here: linear blend skinning is "a skinning method that
  computes a per-vertex transformation matrix as a linear
  weighted sum of transformation matrices of the designated
  nodes", a morph target is "an altered state of a mesh
  primitive defined as a set of difference values for its vertex
  attributes", skins are defined by a `joints` array whose order
  must match `inverseBindMatrices`, and target names live in
  `mesh.extras.targetNames` by convention
  ([glTF 2.0 Specification, Khronos](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
  — establishes the skinning/morph/animation data model the
  loader and evaluator implement).
- **Strobe family (Family B):** ten smoothed keeper GLBs,
  113k–169k verts, same 54-joint skeleton shape, 36 Body morph
  targets, real PBR materials: 4 embedded 2048×2048 PNG atlases
  per girl, metallic 0, **roughnessFactor 0.75**, double-sided.
  One girl decoded ≈ 67 MB of atlas + ≈ 28 MB geometry/morph
  payload (gap §1.3, computed from measured figures).
- **Plates of record** are Blender EEVEE renders under a fixed
  rig — NOT Cycles parity (gap §1.4, R5). Plate spec v2
  (D-1224, `PLATE-SPEC.md`) is now normative: **6144×7680**
  (8K-class, 4:5), TAA 256 samples, Sun 3.0 @ (55°, −25°), Fill
  **1.1** @ (70°, 140°), world (0.16, 0.15, 0.19); scene content
  frozen (the `anim.py` code path); plate frame *k* ↔ clip frame
  2.5*k*. The v1 arc plates the rung-1 verdict used were
  512×640 under the older spec — when a gate compares against
  plates, it names **which plate generation** it used.

### 2.4 Standing constraints inherited from rung 1

- Poses arrive evaluated from `torogao-motion-clip` (canonical
  60 Hz, replay hashes); the renderer never evaluates clips.
- The wgpu fallback stays *possible* through the backend
  boundary — it is not built, and no rung-2 module may narrow
  the boundary in a way that forecloses it.
- Vulkan synchronization inside the island follows the
  explicit model: pipeline barriers carry an execution
  dependency (which stages wait) and a memory dependency (which
  accesses become visible), and Synchronization2 keeps the
  stage masks in the barrier structures
  ([Khronos, *Understanding Vulkan Synchronization*](https://www.khronos.org/blog/understanding-vulkan-synchronization)
  — establishes the barrier/event/subpass-dependency model and
  the execution-vs-memory distinction every new pass must
  respect). New render passes (shadow, TAA resolve, particle
  compositing) state their barriers in code comments at the
  dependency they create.

---

## 3. Architecture invariants for rung 2

1. **Packets above, Vulkan below.** Scene, simulation, and
   shading logic produce plain-data packets; only
   `backend/vulkan.rs` touches ash handles. New capabilities
   (scenes, physics, fluids) land as new **estate crates**
   (§8b) that depend on nothing GPU-side.
2. **One evaluation order.** Per fixed step: scene timeline →
   clip poses → IK/constraints → secondary simulation → skinning
   packets → render. The order is part of the determinism
   contract; a module that needs a different order proposes it
   in its gate report, never silently.
3. **Simulation is evaluated state.** Physics/fluid state is a
   pure function of (initial state, step count, inputs). Replay
   = re-evaluation. Any state that cannot be reconstructed from
   the scene description + step index does not exist.
4. **Numbers cite artifacts (D-713).** Every gate number names
   the run artifact it came from. A number with no citable
   artifact does not travel; an unverifiable number is REVIEW.
5. **Bounds before runs.** Every numeric gate states its bound
   *before* the run that tests it (the rung-1 proving idiom:
   bound stated, then measured). Brief-set bounds in §4 are
   proposals the first gate of each module ratifies or revises
   with evidence — they are not promises.

---

## 4. Module work packages

Each module: purpose, current state, what to build, acceptance
gates, dependencies. Work **one** module per pickup (§10).

### M1 — Cast-wide parity

**Purpose.** Prima's limb parity (D-1223) becomes all six girls',
and Family B enters the renderer.

**Current state.** gen_clip is fixed at both live copies on primo
(addendum §"What changed": deployed sha256 `875ab1e5…`, key =
`normalize(rest ⊗ delta)`); the fix is general by construction —
the per-bone twist was measured T_b = Rx(+90°) = G⁻¹ for all 54
bones of Prima's rig, and the frame gate re-derives it per rig.
Only Prima's clip was regenerated (inputs-v2, clip sha256
`bb060717…`). The other five girls still carry pre-fix clips.
Family B (Strobe) has never been loaded by the renderer: its
path needs texture decode/upload and the atlas material model
(gap G1's texture half).

**Build.**
1. For each of Alma, Lux, Piper, Sloane, Yui: run the frame gate
   (`dump_bone_data.py` + `analyze_frames.py`, item-5 staging)
   against her rig to re-derive the twist from *her* rest data;
   regenerate her arc clip through the fixed gen_clip; record
   clip sha256 + skeleton sha256 per girl.
2. Extend the proving suite per girl: per-bone world
   deformation-operator residuals vs Blender over her show key
   frames (the addendum's method), joint placement at frame 0,
   GPU-vs-CPU skinning crosscheck, twin-render determinism.
3. Family B intake: PNG atlas decode + upload (the `png` crate
   is already in the workspace graph — gap §4a), TEXCOORD_0
   sampling in the fragment path, `paint_from_color1` logic
   re-derived for atlas materials (do NOT assume the cast rule
   transfers; measure per family). Vivienne first.

**Acceptance gates.**
- Per girl: worst-bone residual ≤ 0.10°, mean ≤ 0.05° over her
  key frames (brief-set bound; Prima achieved 0.0560° worst —
  ratify or revise with evidence at the first girl's gate).
- Per girl: GPU-vs-CPU max Δ within the rung-1 stated bounds
  (positions ≤ 1e-4 m, normals ≤ 1e-3); twin renders 100%
  frame-hash identical; replay hash recorded per girl.
- Census parity: rendered vert/morph/joint counts equal the
  gate census for every girl loaded (gap rung-2 proof shape).
- Vivienne: verdict pack reproducing her D-1206 pack
  compositions in-engine, prepared for the Owner's eye beside
  the smoothed sheets of record. No Owner PASS is declared by
  the agent.
- Theodolite verified-zero on every touched Rust file (§7).

**Dependencies.** None blocking. The frame gate's Blender
witnesses run under the estate's Blender discipline (official
build, `-Y` always) via the blender connector (§7).

### M2 — Scene system

**Purpose.** From "one character + a hardcoded arc set"
(`setscene.rs`'s procedural `ArcScene`) to data-driven scenes:
multiple characters, sets, cameras, lights, and a timeline an
author can write without touching Rust.

**Current state.** `setscene.rs` builds Prima's couch set
procedurally with a visibility schedule; `backend/mod.rs`
already generalizes set pieces as packets with
`visible_from/visible_until`. Camera = the plate-rig camera
only. Poses come from motion-clip per character. There is no
scene file format, no multi-character composition, no camera
path, no light beyond the plate pair.

**Build.** New estate crate **`torogao-scene`** (§8b):
1. Scene description format (RON — the estate's data-format
   idiom): characters (GLB + clip + placement), set pieces,
   cameras, lights, timeline. Versioned from birth
   (`torogao-scene/1`), fail-closed on unknown fields.
2. Timeline evaluation at the canonical 60 Hz: per-character
   clip tracks with offsets/loops, scalar channels (the
   motion-clip channel vocabulary), camera tracks, light
   tracks, set-piece visibility — all evaluated into the same
   packets rung 1 consumes, extended for N characters.
3. Camera paths: keyframed position/target/fov with the
   clip-format interpolation idioms; a "plate camera" preset
   that reproduces the gate rig exactly (M1/M6 keep using it).
4. Lighting rigs beyond the plate pair: additional directional
   lights, point/spot lights with distance falloff, and an
   ambient term. For environment-style ambient, the reference
   result is that diffuse irradiance from distant illumination
   is captured to ~1% average error by just **9 spherical-
   harmonic coefficients** (bands l ≤ 2), equivalently a
   quadratic polynomial in the surface normal
   ([Ramamoorthi & Hanrahan 2001](http://www0.cs.ucl.ac.uk/research/vr/Projects/VLF/vlfpapers/ibr/Ramamoorthi_R__An_Efficient_Representation_for_Irradiance_Environment_Maps__SIGGRAPH2001.pdf)
   — establishes the 9-coefficient SH irradiance result the
   ambient packet implements). Scene files may carry SH
   ambient; the plate rig's flat world colour remains the
   special case SH reduces to at l = 0.
   As a cross-check of the scene/skin data model against the
   Web ecosystem: three.js's `SkinnedMesh` binds a `Skeleton`
   whose bone matrices feed GPU skinning from `skinIndex` /
   `skinWeight` attributes — the same attribute contract as
   glTF JOINTS_0/WEIGHTS_0 — and its ecosystem also demonstrates
   baking bone matrices into float textures to instance crowds
   ([threeforge design notes](https://github.com/tallslab/threeforge/blob/HEAD/docs/design.md)
   — establishes the WebGL-side skinning contract and the
   bone-texture technique M7 may evaluate; technique reference
   only, no code transfers).

**Acceptance gates.**
- A two-character scene file renders both characters posed by
  their own clips, twin-run hash-identical, with per-character
  replay hashes matching their solo M1 hashes at shared frames
  (composition must not perturb evaluation).
- The Prima arc expressed as a scene file reproduces the
  rung-1 replay hash `1bd9dd53…` exactly (the hardcoded path
  becomes a special case of the data path — this is the
  module's regression gate).
- Camera path evaluation: keyframe poses reproduced to f32
  exactness at keys; interpolated frames deterministic across
  twin runs.
- Scene loader adversarial set: unknown version, unknown
  field, dangling character/clip reference, empty timeline —
  each fails closed with a typed error, no partial scene.
- Format spec written in the crate's `DESIGN.md` before the
  loader lands (format-first, per estate practice).

**Dependencies.** M1 (per-girl clips + hashes). F1/F3 (§5)
feed scene *content*, not the format.

### M3 — Two-body interaction

**Purpose.** The stress test (§1): two characters in believable
contact. Contact, attachment, reach, and weight — as evaluated,
deterministic pose-layer machinery, not per-scene hand-tuning.

**Current state.** Nothing exists: poses are clip-only, bodies
interpenetrate freely, there is no IK. This module builds the
pose-layer solver stack in the new crate **`torogao-softbody`**
(shared with M4, §8b), consumed by the scene evaluator (M2)
between clip pose and skinning (§3, order 2).

**Build.**
1. **Collision proxies.** Per-character capsule/sphere sets
   bound to bones (authored in the scene/rig data, measured
   from the GLB, not guessed). Pairwise distance queries at
   fixed step; penetration depth + contact normal per pair.
2. **Contact constraints.** Positional contact projection in
   the XPBD solver (M4's solver, used here at rigid gain):
   XPBD's contribution is constraint stiffness expressed as
   **compliance** (inverse stiffness) with a total Lagrange
   multiplier, making the effective stiffness independent of
   timestep and iteration count — the property that lets the
   same authored contact feel identical at any fixed step the
   estate chooses
   ([Macklin, Müller & Chentanez 2016, *XPBD*](http://mmacklin.com/xpbd.pdf)
   — establishes the compliance formulation and the
   timestep/iteration independence claim).
3. **Attachment constraints.** Distance/pin constraints
   between named bones across characters (hand-to-wrist,
   hand-to-hip, body-to-set-piece), authored in scene data,
   with compliance per attachment.
4. **IK for reach and placement.** Two-bone analytic IK for
   limbs plus FABRIK for chains (spine, tail-class chains):
   FABRIK solves joint *positions* by forward/backward
   reaching along fixed bone lengths — no rotation matrices,
   no singularity failures, converging in few iterations on
   humanoid chains
   ([Aristidou & Lasenby 2011, *FABRIK*](https://andreasaristidou.com/FABRIK)
   — establishes the algorithm, its position-space form, and
   its convergence behaviour; a worked implementation walk-
   through also lives in the Vulkan documentation project's
   tutorial set,
   [docs.vulkan.org FABRIK tutorial](https://docs.vulkan.org/tutorial/latest/Advanced_glTF/Procedural_Animation_IK/03_fabrik.html)).
   IK runs *inside* the fixed step, fixed iteration counts,
   targets from scene data or attachment constraints.
5. **Stress scene.** One canonical two-body stress scene
   (authored as scene data): sustained contact, a reach, an
   attachment, a weight shift. This scene is the module's
   permanent regression fixture.

**Acceptance gates.**
- Stress scene: max interpenetration over all frames ≤ a
  brief-set bound of 5 mm at proxy level, measured per frame
  from the solver's own contact log (artifact cited); zero
  contact pairs unresolved at step end.
- Determinism: stress scene twin-run — solver state hash per
  step identical; rendered frames hash-identical.
- IK: end-effector positional error ≤ 1 mm at convergence on
  the fixture chains; iteration counts fixed and recorded.
- Verdict pack on the stress scene: expected-vs-seen written
  against the believability criteria (contact reads as
  contact; no hover, no slide-through, no candy-wrapper limb
  twist), crops at decision size, honest FAIL exemplar if any
  frame fails. Owner's eye rules.
- Adversarial: attachment to a missing bone, IK target beyond
  reach, zero-length chain, contact storm (all proxies
  overlapping at step 0) — typed failure or bounded recovery,
  never NaN, never panic.

**Dependencies.** M2 (scene data + evaluation order), M4's
solver core (build the XPBD core first if picking both — the
modules share `torogao-softbody` by design). F3/F4 (§5) supply
measured contact behaviour from reference titles.

### M4 — Secondary motion (soft body, cloth, hair)

**Purpose.** Everything that moves because physics says so:
breast/tissue soft-body, outfit cloth, hair — deterministic,
authored by physical parameters, identical on every replay.

**Current state.** No simulation exists in the renderer. The
estate's pose-side physics precedent is `torogao-engine`'s
fixed-step puppet physics (2D; gap §1.1) — posture to follow,
not code to port. JinkakuHaisetsu's measured soft-body
constants are being extracted by bundle-reader rung 2 **now**
(§5 F1) — they are this module's calibration target, and the
module does not invent its own constants while that extraction
is in flight.

**Build.** In **`torogao-softbody`**:
1. **XPBD core.** Particles + constraints at fixed step with
   substeps; the small-steps result — many small substeps with
   one iteration each outperform few steps with many iterations
   for the same budget — is the scheduling baseline (surveyed
   in [Seabra, Lopes & Pereira, *Survey of Rigid Body
   Simulation with XPBD*](https://iid-intranet.inesc-id.pt/publications/downloadfile?id=f7930362-bbd0-440a-a454-4bcd495b5ed6)
   — establishes the substep-vs-iteration tradeoff and XPBD's
   compliance semantics as the survey of record). Fixed
   substep count, fixed iteration count, fixed constraint
   order — all named constants with units (Tiger Style).
2. **Soft body.** Tetrahedral (or proxy-lattice) volume
   elements with shape-matching / volume-preservation
   constraints; skin coupling: simulated particles drive a
   bone/morph layer (the coupling design is the module's first
   written decision, recorded in the crate `DESIGN.md` before
   code).
3. **Cloth.** Distance + bend constraints on authored grids
   (outfit pieces, hair cards); self-collision off by default
   (named, measured decision if enabled); collision against
   M3 proxies.
4. **Hair.** Strand chains (distance + bend + attachment at
   scalp bones), rendered through the existing Hair mesh path
   or as scene set geometry — the representation choice is a
   written decision with the perf numbers from a spike.
5. **Determinism armour.** No hash-map iteration in the solve
   path (ordered storage only); no transcendental function
   whose result feeds state unless it comes from the crate's
   own audited math (the determinism conditions Rapier's
   documentation enumerates — same timesteps, strict IEEE-754
   behaviour, deterministic math functions — are the checklist,
   [Rapier determinism guide](https://rapier.rs:443/docs/user_guides/templates/determinism/)
   — establishes the conditions under which a Rust physics
   simulation is locally and cross-platform deterministic).

**Acceptance gates.**
- Calibration scene per effect class, twin-run state-hash
  identical per step over ≥ 3,600 steps (60 s at 60 Hz).
- Soft-body volume drift ≤ 1% over the calibration scene
  (brief-set bound); cloth grid rest-shape recovery after
  disturbance; hair chain rest recovery — each measured from
  solver state artifacts, bounds stated before runs.
- F1's extracted JinkakuHaisetsu constants, once landed, run
  through the calibration scene and the deltas vs the title's
  observed behaviour are reported (match or named divergence —
  never silently retuned).
- NaN armour: adversarial states (zero-length constraint,
  coincident particles, inverted tet) fail typed or recover
  bounded; fuzz the solver with seeded random states,
  determinism = same seed → same state hash.
- Perf: solver step cost measured and reported worst-step,
  inside M7's frame budget for the two-body stress scene.

**Dependencies.** M3 (proxies + solver sharing), F1 (measured
constants). M4 may start before F1 lands, on placeholder
constants **labelled as such in every artifact**.

### M5 — Fluids and particles

**Purpose.** Deterministic particle/fluid beats for the two
scene classes that need them: Laplace3D-class liquid scenes and
Prima-class emission scenes — simulated in-engine, rendered
in-engine, replayable exactly.

**Current state.** Nothing exists. There is no particle system,
no fluid solver, no particle rendering path in the crate.

**Build.** New estate crate **`torogao-fluids`** (§8b):
1. **Solver: position-based fluids.** SPH is the substrate —
   Müller, Charypar & Gross brought smoothed particle
   hydrodynamics to interactive graphics with the poly6 /
   spiky / viscosity kernel set and free-surface rendering by
   point splatting and marching cubes
   ([Müller et al. 2003](https://cgl.ethz.ch/Downloads/Publications/Papers/2003/Mue03b/Mue03b.abstract)
   — establishes the SPH formulation and kernels the solver's
   density/viscosity terms follow). The estate solver is
   **PBF**: Macklin & Müller's iterative density solver inside
   the PBD framework, which "allows similar incompressibility
   and convergence to modern SPH solvers, but inherits the
   stability of" position-based dynamics
   ([Macklin & Müller 2013, *Position Based Fluids*](http://gbib.siggraph.org/index.php?detail=Macklin:2013:PBF)
   — establishes the PBF algorithm and its stability claim) —
   it shares M4's constraint machinery and determinism posture
   by construction (the unified-particle view: one particle
   framework for fluids, cloth, and rigid coupling is Macklin
   et al. 2014's *Unified Particle Physics*, listed in the
   Stanford CS348C syllabus,
   [cs348c course page](http://graphics.stanford.edu/courses/cs348c-20-winter/)).
   Where stronger incompressibility is needed, DFSPH is the
   named upgrade path: it adds a divergence-free velocity
   solve beside the density solve (characterized in
   [Diaz et al. 2025, *Implicit Position-Based Fluids*](https://graphics.cs.utah.edu/research/projects/ipbf/ipbf.pdf)
   — establishes the DFSPH two-solver structure and PBF's
   stability framing).
2. **Neighbour search, deterministic.** Spatial hash grid with
   **sorted** cell order — solver results must not depend on
   insertion or hash order (§3.3). Neighbour counts bounded by
   named constants; overflow is a typed error, never a silent
   truncation.
3. **Coupling.** Two-way coupling with M3 proxies and M4 soft
   bodies through the shared particle framework (fluid
   particles push, bodies displace volume).
4. **Rendering.** First path: point-splat rendering into the
   scene (Müller 2003's splatting route), composited by the
   backend as a new packet type. A meshed/screen-space surface
   path is a later decision — its technique choice is
   **REVIEW** (App. B) until a spike measures both against a
   Laplace3D-class reference frame (F3).
5. **Prior-art evaluation (not adoption by default):** the
   Rust ecosystem's SPH entry is Dimforge's **salva3d** —
   DFSPH and IISPH pressure solvers, XSPH/artificial
   viscosity, surface tension, and optional two-way coupling
   with Rapier bodies
   ([salva3d, docs.rs source](https://docs.rs/salva3d/0.9.0/src/salva3d/lib.rs.html)
   — establishes salva's solver inventory; listed at 0.10.0 on
   lib.rs at authoring time). Evaluate it the way the gap
   analysis evaluated rend3: if its determinism contract,
   ordering guarantees, and maintenance state fit, propose
   adoption with evidence; otherwise it is study material —
   techniques transfer, code doesn't (estate Lane D doctrine).

**Acceptance gates.**
- Dam-break / emitter calibration scenes: twin-run state-hash
  identical per step over ≥ 3,600 steps; particle count,
  memory, and worst-step cost reported against named bounds.
- Incompressibility: mean density deviation from rest density
  reported per scene against a bound stated before the run
  (PBF's known softness is measured, not hidden).
- Coupling scene (fluid vs M3 proxy): no particle tunnels
  through the proxy at the fixed step over the scene's length;
  measured, artifact cited.
- Rendered verdict pack for one Laplace3D-class beat and one
  Prima-class beat, prepared for the Owner's eye.
- Adversarial: emitter at boundary, zero particles, particle
  spawned inside a proxy, grid overflow — typed outcomes.

**Dependencies.** M4 (shared solver core), M3 (proxies),
F1/F2 (Prima-class measured behaviour), F3 (reference
criteria). M5 is the last module to start; its design document
lands before its code.

### M6 — Shading parity (the rung-3 items, in scope here)

**Purpose.** Close verdict items 6 and 7: match the plates'
light level and complete self-shadowing, at plate-spec v2.

**Current state.** Rung 1's R5 model (wrapped diffuse + single
GGX lobe + AgX fit) renders measurably brighter than plates:
full-frame mean signed Δ ≈ (+42.5, +43.5, +41.7)/255; the
decomposition is measured (verdict item 6): wrap-vs-plain
diffuse ≈1.37× (sun) / ≈1.96× (fill) at the floor normal,
EEVEE light-energy semantics, AgX-fit residual +9…+12/255.
Sun shadow exists (2048², 3×3 PCF); Fill shadow missing.
Plates are now v2 (6144×7680, TAA 256) — comparisons run
against v2 plates at matched framing, downsampled by a stated
filter, or at v2 native for crop regions (the comparison
method is part of the gate report).

**Build.**
1. **Parameterized wrap diffuse.** Replace the fixed wrap with
   the parameterized family: Valve's Half Lambert —
   "scale the dot product … by ½, add ½ and square the result"
   so the terminator softens without flattening the back side
   ([Mitchell, McTaggart & Green 2006, *Shading in Valve's
   Source Engine*](https://ia903203.us.archive.org/27/items/valve-publications/2006/SIGGRAPH06_Course_ShadingInValvesSourceEngine.pdf)
   — establishes the Half Lambert formulation rung 1's wrap
   descends from) — generalized as f(θ, w) = (cos θ + w)/(1 + w),
   which reduces to plain Lambert at w = 0 and integrates with
   SH lighting ([Sloan et al., *Wrap Shading*](https://cim.mcgill.ca/~derek/files/publication-files/jgt_wrap_final.pdf)
   — establishes the parameterized wrap and its SH form).
   The wrap parameter becomes a **material/scene datum**,
   fitted per light against plates — fitted, recorded, never
   hand-waved.
2. **The GGX lobe, specified.** The single specular lobe
   follows the production PBR reference model: Filament's
   public documentation is the estate's cited reference for a
   GGX-based lit model with physical light units and the
   metallic/roughness parameterization the Strobe materials
   already speak ([Filament, *Physically Based Rendering*](https://google.github.io/filament/main/filament.html)
   and [*Materials*](https://google.github.io/filament/main/materials.html)
   — establish the reference BRDF/light model vocabulary;
   Filament is a study reference, **not** a dependency — gap
   §4 route (a) stands). EEVEE's specular response to a wrapped
   diffuse is not a standard BRDF pairing (gap R5): the lobe
   is tuned against plates, and the tuning constants are data
   with provenance.
3. **AgX fit, tightened.** The output transform approximates
   Blender's AgX (plate spec v2 colour management: AgX, look
   None). AgX originates with Troy Sobotka; Blender's OCIO
   configuration credits the original at
   [github.com/sobotka/AgX](https://github.com/sobotka/AgX)
   (via Blender's own config header, projects.blender.org) —
   the fit target is Blender's shipped transform, measured
   from plate/render pairs, not the upstream repository's
   evolving form. The +9…+12/255 residual is the number to
   beat; the fit method (analytic curve vs 3D LUT) is chosen
   by which one measures smaller on the background + emissive
   screen witnesses the verdict pack names.
4. **Fill shadow.** Second shadow map with the Sun pass's
   discipline; both lights shadow in the plate rig and in
   scene rigs (M2 lights carry a `casts_shadow` datum).
5. **Anti-aliasing decision: MSAA vs TAA, measured.** The
   verdict pack's named edge delta (TAA-vs-no-MSAA) is decided
   here with renders, not taste. TAA's mechanism — distribute
   samples over multiple frames, reproject history, and clip
   history against the current frame's neighbourhood
   ([Karis 2014, *High Quality Temporal Supersampling*](https://archive.org/download/random-papers14/High%20Quality%20Temporal%20Supersampling%20%28SIGGRAPH%202014%29.pdf)
   — establishes the TAA pipeline and neighbourhood clipping)
   — carries known failure modes at disocclusion and under
   fast motion (ghosting vs aliasing trade,
   [Marrs et al. 2018, *Adaptive Temporal Antialiasing*](https://research.nvidia.com/sites/default/files/pubs/2018-08_Adaptive-Temporal-Antialiasing/adaptive-temporal-antialiasing-preprint.pdf)).
   Estate constraint TAA must satisfy or be rejected:
   **history is state** — the jitter sequence is a pure
   function of frame index, history buffers are part of the
   replayed state, and twin runs must still hash-identical.
   If TAA cannot meet that bar in the headless path, MSAA
   (with its Vulkan cost stated) ships and TAA is recorded as
   rejected-with-evidence.

**Acceptance gates.**
- Plate comparison at v2, 60 sampled frames (the rung-1
  sampling idiom): full-frame mean signed Δ and
  character-masked mean |Δ| reported against the rung-1
  numbers — the gate is *movement by measurement* toward
  (0,0,0) / the noise floor, with the residual decomposition
  updated (wrap factor, light-energy factor, AgX residual
  each named). A brief-set target: AgX-fit residual ≤ 3/255
  on the witness regions; ratify or revise at first gate.
- Fill shadow: scene-probe equivalent re-run — both lights
  demonstrably shadow (probe artifact), no peter-panning /
  acne beyond the Sun pass's measured class.
- Determinism unchanged: twin renders hash-identical with
  the full M6 stack on.
- Verdict pack: pair frames + diffs at v2 crops for the
  Owner's eye; the pack states which plate generation and
  which downsample filter every number used.
- No ad-hoc retune: every changed constant lands with the
  plate pair that justifies it, in the commit message or the
  gate report.

**Dependencies.** M1 (v2 plates exist per girl via the D-1224
lane's spec — plates are Blender-side, produced by the plate
pipeline, not by this crate). Independent of M3–M5.

### M7 — Performance and presentation

**Purpose.** Scenes at budget, in the app, out as video.

**Current state.** Rung 1 renders one character + set at
512×640 headless; no frame-time budget has been measured for
multi-character scenes; `torogao-app` does not consume the
renderer yet (gap rung 6); export is PNG frame sequences.

**Build.**
1. **Budgets, measured worst-frame.** The inherited envelope
   (gap R8): Family A scenes at 60 fps (16.6 ms) at
   1024×1280-class internal resolution on primo-class
   hardware; report **worst frame**, never an average that
   hides the spike (gap rung-4 idiom). Rung-2 working
   resolutions add the v2 plate size for offline renders —
   offline has no fps budget, it has a wall-clock-per-frame
   budget stated per scene class. GPU memory: per-scene
   decoded payload reported against the §1.3 computed
   budgets. If hand-rolled allocation in the Vulkan island
   starts costing correctness, the adoption candidate is
   **gpu-allocator** (0.28.0 at authoring): "a fully written
   in Rust memory allocator for Vulkan, DirectX 12 and
   Metal", built on ash
   ([gpu-allocator, docs.rs](https://docs.rs/crate/gpu-allocator/latest)
   — establishes the crate's scope and ash basis; adoption
   is an evidence decision inside the island only).
2. **Instancing and batching.** Set pieces and particles
   instance; per-character draws batch by material. The
   bone-texture crowd technique (M2's three.js citation) is
   evaluated only if a scene class demands crowd counts —
   the cast is six, not six hundred; do not build for crowds
   speculatively.
3. **Presentation.** `torogao-app` stage mode consumes the
   renderer beside the puppet stage (gap rung 6 shape):
   windowed and headless modes produce identical frame
   hashes for the same evaluated state; the egui surface
   (already the app's UI stack, gap §1.1) carries scene
   selection, transport, and gate readouts — presentation
   only, never evaluation.
4. **Silent video export.** Frame sequences (PNG today; EXR
   for HDR intermediates via the pure-Rust **exr** crate —
   1.5.1 at authoring, "read and write OpenEXR files without
   any unsafe code",
   [exr, docs.rs](https://docs.rs/crate/exr/1.5.1/source/examples/7_crop_alpha_any_image.rs)
   — establishes the crate's safety posture and currency)
   encoded to video by the pipeline's encoder step. The
   encoder binding choice (FFmpeg bindings vs external
   encoder process) is **REVIEW** (App. B) — decide at
   pickup with a spike; the frame-sequence contract is fixed
   either way. Audio: none. Renders are silent files; the
   audio-silence rule binds every export (§9).

**Acceptance gates.**
- Two-body stress scene (M3) + full secondary motion (M4) at
  the R8 desktop envelope: worst-frame time reported from a
  logged run, inside 16.6 ms or the overrun is named with
  its cost centre (measured, not guessed).
- App integration: windowed/headless hash parity on the
  Prima fixture (gap rung-6 proof shape).
- Export: a scene exports to a video file whose decoded
  frames hash-match the frame sequence it was encoded from
  (lossy-codec allowance stated numerically before the run).
- Memory: peak GPU allocation for the largest scene class
  reported; no unbounded growth over a 3,600-frame render
  (allocation log artifact).

**Dependencies.** M2 (scenes), M3/M4 (the load), M6
(presentation shows final shading).

---

## 5. Fathom & reverse-engineering commissions

Fathom is the estate's clean-room reverse-engineering MCP.
The law, from the bundle reader's report of record
(`~/workspace/bundle-native-20261110/REPORT.md`): extraction is
built **only** from public format documentation and observed
bytes of held titles; other implementations' outputs may serve
as test *fixtures* (data), never as source. What transfers into
the renderer is **techniques and measured constants** — never
copied assets, curves, or code (estate Lane D doctrine, gap
§4c). Each commission names: target, what to extract, route,
and the module it feeds. Commissions are proposed by the
renderer agent and executed as their own gated lanes; a
renderer module blocked on a commission says so and works on
unblocked scope — it does not invent the constant.

**F1 — JinkakuHaisetsu MonoBehaviour recovery (IN FLIGHT).**
Target: Personality Excretion Simulator (primo
`~/.local/share/ntranch-game-re/JinkakuHaisetsu/`). Route:
bundle-native reader rung 2 — stripped-tree MonoBehaviour
recovery via MonoScript → assembly-metadata join (rung 1
proved the reader: 212/212 TextAssets byte-identical vs the
AssetRipper cross-check; stripped files route MonoBehaviours
to REVIEW by design, which rung 2 exists to close — REPORT.md
+"What parses today"; tooling state in
`~/workspace/bundle-native-20261110/rung2-tools/`). Extract:
soft-body constants, emission/fluid parameters, stage/scene
configuration values. Feeds: **M4** (calibration), **M5**
(Prima-class parameters), **M2** (scene configuration shape).
**Do not duplicate this lane** — consume its outputs.

**F2 — JinkakuHaisetsu code-level constants.**
Target: same title's `GameAssembly.dll` + `global-metadata`.
Route: the native IL2CPP rung (D-1202; method packaged with
the estate's IL2CPP rung work). Extract: animation state
machine structure, camera-selection logic shape, scene/phase
transition constants — as *specifications* (state names,
transition conditions, numeric constants), never decompiled
code pasted into the tree. Feeds: **M2** (timeline/camera
design), Prima's sequencer work downstream.

**F3 — Laplace3D reference measurement.**
Target: the estate's Laplace3D reference material (corpus of
record for D-196). Route: the animation-grammar atlas method
(skill: `~/workspace/custom-skills/local-animation-grammar-atlas/SKILL.md`)
plus verdict-pack-style frame analysis. Extract: the
*measurable criteria* D-196 names — contact quality, secondary
motion amplitude/timing, camera behaviour, liquid behaviour —
as numbers and written criteria the M3/M5 verdict packs can be
judged against. Feeds: **M3, M5, M6** gate criteria. The
reference material's internal production techniques are not
public and stay **REVIEW**; the commission measures the
*output*, which is what the bar is defined on.

**F4 — Held-title scene fixtures via the AssetRipper rung.**
Target: held Unity titles with multi-character scenes.
Route: AssetRipper rung (D-1219; skill
`~/workspace/custom-skills/local-assetripper-rung/SKILL.md`) —
GPL tool, used on RE material only, never linked or shipped.
Extract: exported scene/animator *data* as fixtures for the
bundle reader's cross-checks and as scene-format design
evidence (how a production Unity title structures a staged
scene). Feeds: **M2** (format design), **M3** (contact
fixture shapes). Fixtures are data; Unity's and the titles'
code does not transfer.

**F5 — Fathom game-extract passes on staged-scene titles.**
Target: held titles (Lane D queue) whose staging is
Prima-class. Route: fathom directly, per the fathom extract
skill (`~/workspace/custom-skills/local-fathom-game-extract/SKILL.md`).
Extract: dynamics specifications in the Lane D S2b shape —
how the original animates: state machines, timing constants,
interaction radii, emission schedules — written as specs with
measured values. Feeds: **M2, M3, M5**. Each title's spec is
its own artifact with its own provenance; the renderer cites
the spec, never the binary.

---

## 6. Research routes

When a module needs prior art, search in this order and cite
what you actually opened (§0):

1. **Estate first.** The gap analysis, verdict packs, gate
   programme documents, and the skills tree
   (`~/workspace/custom-skills/`) hold measured answers to
   most "how does the estate do X" questions.
2. **arXiv** via the estate connector
   (`~/workspace/custom-connectors/custom.arxiv/CONNECTOR.md`;
   helper `bin/arxiv_tools.py search|id`). Rules from the
   connector doc: one request per invocation, ~3 s between
   invocations, quote ids exactly, and treat a preprint as an
   unreviewed claim — check for a published version before
   citing it as settled.
3. **Specifications and vendor docs.** Khronos: the
   [glTF 2.0 spec](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
   and KHR extension registry; the
   [Vulkan documentation project](https://docs.vulkan.org)
   (tutorials and the synchronization guides); Vulkan spec
   questions resolve to the spec, not to blog posts.
4. **Rust documentation.** docs.rs for every crate claim
   (version, features, safety posture) — a crate's docs.rs
   page, opened, is the citation; the Rust reference for
   language semantics. The renderer keeps its own `math.rs`
   (rung-1 choice); **glam** is the ecosystem's standard
   SIMD math library
   ([glam, docs.rs](https://docs.rs/crate/glam/0.30.9) —
   establishes its scope: f32/f64 vectors, matrices,
   quaternions, column-major) — evaluate it for the *new*
   physics/scene crates where no estate math exists yet, and
   pin the exact current version from its crate page at
   adoption time (version currency is a pickup task, App. B).
5. **Code hosts for prior art.** GitHub (estate connectors:
   `custom.github-ntranch` for the studio account, `github`
   for search), GitLab, Codeberg — for reference
   implementations to *study* (techniques transfer, code
   doesn't; check licences before even studying, and record
   what you studied in the module's notes).
6. **HuggingFace** via the estate connector
   (`custom.huggingface-ntranch`) — reference assets and
   datasets the estate already holds; the generated-media
   backup rule applies to anything produced from them.
7. **WebGL documentation as technique cross-reference.** The
   WebGL/three.js ecosystem solves the same skinning/morph
   problems under tighter constraints; its published
   techniques (bone textures, texture-baked animation) are
   legitimate design evidence — see the M2/M7 citations.
8. **Filament's public documentation** as the shading
   reference of record (M6) — a documentation source, never
   a dependency.

---

## 7. Tool-calling guide

| Need | Call | Notes |
|---|---|---|
| LSP validation of touched files | **theodolite** (primo `~/.local/bin/theodolite`, bridge `~/.local/share/theodolite/bridge.toml`) | The clause below binds every pickup. |
| Plate / witness renders, rig measurements | **blender connector** (`~/workspace/custom-connectors/custom.blender/CONNECTOR.md`) | Official build for rigging work, `-Y` always, `-y` never; headless, silent. |
| Clean-room RE on held titles | **fathom** + skills `local-fathom-game-extract`, `local-bundle-native-reader`, `local-assetripper-rung` | §5 commissions; clean-room law binds. |
| Bundle/SerializedFile reading | **bundle-native** crate + its skill | Rung-1 report is the capability statement; rung 2 (F1) is in flight — check state before assuming a capability. |
| Prior art / papers | **arXiv connector**, browser search/open | §6 order; cite what you opened. |
| Reference assets | **HuggingFace connector** (`custom.huggingface-ntranch`) | Read its CONNECTOR.md before calling. |
| Repo state (studio account) | **github-ntranch connector** | Read-only for this brief's work; pushes are not authorized (§9). |
| Lessons learned | **bloch** | The clause below binds. |
| Skill outcomes at finish | `~/workspace/bin/skill-trust.py record` | The step below binds. |

**THE THEODOLITE CLAUSE (binds every pickup).** Validate
every touched file through theodolite (primo
`~/.local/bin/theodolite` with
`~/.local/share/theodolite/bridge.toml`) using the language
server for that file's type; zero error diagnostics before
reporting done; the report cites the diagnostics result per
file type. A file type with no theodolite route is named in
the report as **not-LSP-validated** — never silently skipped.
Cured-protocol operating rules (proven on this estate;
skipping them manufactures fake zeros):

- **Producer first.** The `.bacon-locations` file must exist
  and be current **when the session starts** (run the bacon
  producer — `bacon --headless -j bacon-ls` or the editor's
  bacon — before opening the session). Mid-session creation
  is not picked up.
- **Verified zero.** A zero counts as verified only with a
  same-session-shape **positive control**: plant a deliberate
  error of the class you are clearing (the rung-1 lanes used
  a planted E0308), watch the session report it, restore the
  file, watch it return to zero. A zero without a control is
  reported as unverified.
- **Lua route:** a session's first query can stall — open Lua
  sessions with a sacrificial first query on any file before
  reading verdicts.
- `session_status` printing `not_started` is not a failure
  signal.

**THE BLOCH CLAUSE.** Lessons recorded from this work carry
`origin_lane` + `content_label` + `evidence` from birth —
no record is born unlabelled. This tree is the NTRanch lane:
engine-infrastructure lessons label `sfw` only when they are
pure technique with no title content (the gap analysis's own
record is the shape: origin_lane `ntranch/torogao-engine`,
content_label `sfw`, evidence = the artifacts cited);
anything touching title content labels honestly, and
NSFW-pure lessons **never** flow to qompassai repos or
callers. Unclassified defaults to the restrictive label.

**SKILL-TRUST RECORDING.** At finish, record outcomes for
any custom skill materially used:
`~/workspace/bin/skill-trust.py record` (schema and counting
rules in `~/workspace/custom-skills/local-skill-trust/SKILL.md`).

---

## 8. Crates and packages

### 8a. Existing crates — evaluate or adopt

Adoption rule: a crate enters the tree with (i) its docs.rs
page opened and cited, (ii) its current version pinned from
that page at adoption time, (iii) a named module that needs
it, and (iv) the determinism/safety posture checked against
§9. Evaluation outcomes are recorded either way.

| Crate | For | Posture |
|---|---|---|
| **ash** 0.38 (in tree) | Vulkan bindings | Keep. The island exists because ash is all-unsafe by design (§2.1 citation). |
| **wgpu** 29 / **egui** 0.35 (in tree, app side) | Fallback possibility + app presentation | Keep the boundary wgpu-shaped; do not build the fallback. |
| **gpu-allocator** 0.28.0 | Vulkan memory allocation inside the island | Evaluate at M7 (citation in M7 §1). Island-only if adopted. |
| **png** (in graph) | Atlas decode (M1), frame export (M7) | Adopt for render3d use. |
| **exr** 1.5.1 | HDR frame intermediates (M7) | Evaluate at M7 (citation in M7 §4). Pure Rust, no unsafe — fits §9. |
| **glam** (pin at adoption) | Math for the NEW crates (scene/softbody/fluids) | Evaluate per §6.4; render3d keeps its own `math.rs` — do not churn proven code. |
| **Rapier (rapier3d)** | Rigid-body reference / possible prop physics | **Evaluate-only at M3.** Its determinism guide (§M4 citation) is the checklist reference. Adoption requires evidence its solver fits the §3 order and determinism armour; the character solver stack is estate-built regardless. Pin the current version from docs.rs at evaluation time (currency is a pickup task). |
| **salva3d** | SPH prior art / possible fluids base | Evaluate-only at M5 (citation in M5 §5). |
| **serde / ron** (workspace idiom) | Scene format (M2) | Follow the workspace's existing versions; the scene crate's DESIGN.md pins them. |

Not adopted, with reasons of record: **Bevy** (gap §4b —
a second engine inside the singular engine, D-457),
**rend3-class renderers** (gap §4c — archived upstream,
wgpu version skew), **Filament** as a dependency (M6 —
documentation reference only).

### 8b. New estate crates to author

All: Tiger Style Rust, `#![forbid(unsafe_code)]`, edition and
toolchain per the workspace, tests half validation / half
adversarial, `DESIGN.md` before code, local commits only.

1. **`torogao-scene`** (M2). Contract: parse + validate a
   versioned scene description; evaluate it at a fixed step
   into render packets + per-character poses. Owns no GPU
   types, no clock, no file watching. Determinism: evaluation
   is a pure function of (scene, step).
2. **`torogao-softbody`** (M3/M4). Contract: XPBD particle/
   constraint solver — proxies, contact, attachment, cloth,
   soft body, hair — over ordered storage, fixed substeps and
   iterations, state hash per step. Owns no rendering; emits
   pose-layer corrections (bone overrides, morph weights,
   mesh-space offsets) as plain data.
3. **`torogao-fluids`** (M5). Contract: PBF particle solver
   sharing softbody's determinism posture; emits particle
   packets for rendering and coupling forces for softbody.
   Particle counts and neighbour bounds are named constants;
   overflow is typed failure.

Crate boundaries are also determinism boundaries: if state
crosses a crate line, it crosses as data that can be hashed.

---

## 9. Non-negotiables

1. **Tiger Style Rust** — load
   `~/workspace/custom-skills/local-tiger-style-rust/SKILL.md`
   before writing or reviewing Rust; its full guide
   (`references/TIGER_STYLE_RUST.md`) wins on disagreement.
   Safety > performance > developer experience; contracts
   first; bound everything with named constants carrying
   units; `#![forbid(unsafe_code)]` outside the one Vulkan
   island.
2. **Tests half validation / half adversarial** — every
   suite, every module, no exceptions of convenience.
3. **Curative fixes only.** A failure is fixed in code or
   the work stops and reports; nothing is documented as a
   "known limitation" and shipped past.
4. **Numbers cite run artifacts (D-713).** Unverifiable =
   REVIEW. Bounds stated before runs (§3.5).
5. **Visual verdicts by the verdict-pack doctrine**
   (D-221/D-629): written SHOULD-be-seen, FINAL-artifact
   crops with the decision region large, honest FAIL
   exemplar beside each item where one exists (never
   fabricated), one-sentence stakes. The agent **never**
   declares an Owner PASS.
6. **No personal names in any artifact (D-489)** — "the
   Owner", studio names only; the exceptions (main README,
   LICENSE, CITATION.cff) do not apply to anything authored
   under this brief.
7. **No CI. No Docker/Podman.** Validation is local, on
   primo, through the gates named here.
8. **Audio silence.** Renders and exports are silent files.
   Nothing built under this brief plays audio on primo;
   test rigs render to null/dummy sinks only.
9. **Local commits only; no pushes.** Each push is a
   separate, explicit Owner authorization naming its scope.
   No destructive git operation without the git-wip-guard
   protocol (skill `local-git-wip-guard`).
10. **Clean-room law for all RE inputs** (§5): techniques
    and measured constants transfer; assets, curves, and
    code never do. GPL tooling (AssetRipper) is used on RE
    material only, never linked or shipped.
11. **Task-end cleanup.** Build artifacts, intermediates,
    and scratch trees you will not use again are deleted at
    task end; deliverables, gate evidence, and named
    follow-on inputs are kept — and anything kept on a
    judgement call is named in the report.
12. **Heavy-compute courtesy.** One heavy job at a time on
    primo; the A6000 is the bench device (pinned, per plate
    spec v2); the Owner's interactive work always wins —
    bounded, checkpointable stages that yield on request.

---

## 10. Pickup protocol

1. Read `CLAUDE.md`, this brief, and the rung-1 verdict pack
   (+ the item-5 addendum). Skim the gap analysis §2/§3 for
   the requirement your module serves.
2. Verify the tree state yourself: branch
   `pax/renderer-rung2-20261110`, clean status, base
   `63896dd`. (D-567: a handoff — including this brief — is
   not evidence; the disk is.)
3. Pick **one** module. Write down its acceptance gates with
   numbers, and the REVIEW claims it depends on (App. B),
   before writing code. If a gate cannot be stated, the
   module is not ready to build — write its DESIGN.md
   instead and report.
4. Check the fathom commissions (§5) your module consumes:
   landed, in flight, or not started. Work unblocked scope
   first; never invent a pending constant.
5. Build in small, gated increments: tests (half/half),
   clippy `-D warnings`, `cargo fmt --check`, theodolite per
   §7 on every touched file, module gates as you reach them.
6. Prepare verdict packs for visual gates; prepare gate
   reports with every number cited to its run artifact.
7. Commit locally, in scoped commits, with D-489-clean
   messages. Clean up (§9.11). Record skill outcomes (§7).
8. Report: what landed (commits), gate results with
   artifacts, REVIEW items opened or discharged, and what
   the next pickup should take. The report is a summary —
   Pax gates from disk, not from the report.

---

## Appendix A — Sources cited (external)

All opened during authoring, 2026-11-10. One line each: what
the source establishes.

1. [glTF 2.0 Specification (Khronos)](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
   — the LBS/morph/skin data model; `targetNames` convention.
2. [ash 0.38.0+1.3.281 (docs.rs)](https://docs.rs/crate/ash/0.38.0+1.3.281)
   — ash is a generated, all-unsafe Vulkan wrapper.
3. [Khronos, *Understanding Vulkan Synchronization*](https://www.khronos.org/blog/understanding-vulkan-synchronization)
   — barrier model; execution vs memory dependencies; sync2.
4. [Fiedler, *Fix Your Timestep!*](https://github.com/laurentongaro/odinrag/blob/HEAD/odin-knowledge-base/references/fix_your_timestep.md)
   — fixed-step accumulator; reproducibility argument.
5. [Ramamoorthi & Hanrahan 2001](http://www0.cs.ucl.ac.uk/research/vr/Projects/VLF/vlfpapers/ibr/Ramamoorthi_R__An_Efficient_Representation_for_Irradiance_Environment_Maps__SIGGRAPH2001.pdf)
   — 9 SH coefficients ≈ 1% diffuse irradiance error.
6. [threeforge design notes](https://github.com/tallslab/threeforge/blob/HEAD/docs/design.md)
   — WebGL skinning contract; bone-texture crowd technique.
7. [Macklin, Müller & Chentanez 2016, *XPBD*](http://mmacklin.com/xpbd.pdf)
   — compliance formulation; stiffness independent of step/iterations.
8. [Aristidou & Lasenby 2011, *FABRIK*](https://andreasaristidou.com/FABRIK)
   — position-space IK; fast convergence on humanoid chains.
9. [docs.vulkan.org, FABRIK tutorial](https://docs.vulkan.org/tutorial/latest/Advanced_glTF/Procedural_Animation_IK/03_fabrik.html)
   — worked FABRIK implementation in the Vulkan docs project.
10. [Seabra, Lopes & Pereira, *Survey of Rigid Body Simulation with XPBD*](https://iid-intranet.inesc-id.pt/publications/downloadfile?id=f7930362-bbd0-440a-a454-4bcd495b5ed6)
    — XPBD survey; substep-vs-iteration tradeoff.
11. [Rapier determinism guide](https://rapier.rs:443/docs/user_guides/templates/determinism/)
    — determinism conditions for a Rust physics simulation.
12. [Müller, Charypar & Gross 2003](https://cgl.ethz.ch/Downloads/Publications/Papers/2003/Mue03b/Mue03b.abstract)
    — SPH for interactive fluids; kernels; surface rendering.
13. [Macklin & Müller 2013, *Position Based Fluids*](http://gbib.siggraph.org/index.php?detail=Macklin:2013:PBF)
    — PBF: incompressibility with PBD stability.
14. [Stanford CS348C syllabus](http://graphics.stanford.edu/courses/cs348c-20-winter/)
    — *Unified Particle Physics* (Macklin et al. 2014) reference of record.
15. [Diaz et al. 2025, *Implicit Position-Based Fluids*](https://graphics.cs.utah.edu/research/projects/ipbf/ipbf.pdf)
    — DFSPH's two-solver structure; PBF framing.
16. [salva3d (docs.rs source)](https://docs.rs/salva3d/0.9.0/src/salva3d/lib.rs.html)
    — Salva's solver inventory (DFSPH/IISPH, XSPH, coupling).
17. [Mitchell, McTaggart & Green 2006, *Shading in Valve's Source Engine*](https://ia903203.us.archive.org/27/items/valve-publications/2006/SIGGRAPH06_Course_ShadingInValvesSourceEngine.pdf)
    — Half Lambert formulation (§8.4.4 of the course chapter).
18. [Sloan et al., *Wrap Shading*](https://cim.mcgill.ca/~derek/files/publication-files/jgt_wrap_final.pdf)
    — parameterized wrap f(θ, w); SH integration of wrap.
19. [Filament, *Physically Based Rendering*](https://google.github.io/filament/main/filament.html)
    — the reference GGX lit-model exposition.
20. [Filament, *Materials*](https://google.github.io/filament/main/materials.html)
    — material models incl. subsurface and cloth models.
21. [AgX (Sobotka), via Blender's OCIO config credit](https://github.com/sobotka/AgX)
    — AgX's origin; Blender ships a developed form of it.
22. [Karis 2014, *High Quality Temporal Supersampling*](https://archive.org/download/random-papers14/High%20Quality%20Temporal%20Supersampling%20%28SIGGRAPH%202014%29.pdf)
    — TAA pipeline; neighbourhood clipping.
23. [Marrs et al. 2018, *Adaptive Temporal Antialiasing*](https://research.nvidia.com/sites/default/files/pubs/2018-08_Adaptive-Temporal-Antialiasing/adaptive-temporal-antialiasing-preprint.pdf)
    — TAA failure modes: ghosting, disocclusion.
24. [gpu-allocator (docs.rs)](https://docs.rs/crate/gpu-allocator/latest)
    — pure-Rust Vulkan/D3D12/Metal allocator on ash; 0.28.0.
25. [exr (docs.rs)](https://docs.rs/crate/exr/1.5.1/source/examples/7_crop_alpha_any_image.rs)
    — pure-Rust OpenEXR read/write, no unsafe; 1.5.1.
26. [glam (docs.rs)](https://docs.rs/crate/glam/0.30.9)
    — SIMD game math library scope and conventions.

Estate sources are cited inline by path throughout (§2, §5,
§7) and are not repeated here.

## Appendix B — REVIEW register (unsourced / unmeasured)

1. **Laplace3D internal production techniques** — not public;
   F3 measures outputs only. No module may claim technique
   parity with the reference material, only measured-output
   parity against F3's criteria.
2. **JinkakuHaisetsu engine internals** (Unity version
   specifics, solver shapes, shader behaviour) — until F1/F2
   land measured artifacts. The title is the design template;
   its internals are not yet estate knowledge.
3. **Fluid surface rendering technique** (splat vs meshed vs
   screen-space) — M5 decides by spike against a reference
   frame; no default is asserted here.
4. **Video encoder binding** (FFmpeg bindings vs external
   encoder process) — M7 decides by spike; the frame-sequence
   contract is fixed meanwhile.
5. **Crate version currency at adoption** (glam, rapier3d,
   salva3d, serde/ron pins) — versions move; the adopting
   module pins from docs.rs at pickup and cites the page.
   (gpu-allocator 0.28.0 and exr 1.5.1 were verified at this
   brief's authoring and are stated with their pages.)
6. **Android GPU behaviour for this renderer** — unmeasured
   in-estate (gap R7/G7); out of scope here, priced as a
   later rung.
7. **Brief-set numeric bounds** in §4 (per-bone 0.10°,
   penetration 5 mm, volume drift 1%, AgX residual 3/255,
   IK 1 mm) — engineering proposals, each ratified or
   revised with evidence at its module's first gate; they
   bind as *stated-before-the-run* bounds from the moment a
   module's first gate runs under them.
