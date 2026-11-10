# FEATURE-GAPS.md — Torogao feature-gap register for the rung-2 worktree

**Status:** register, not a brief. Commissioned by the Owner
2026-11-10 (D-1242) as required reading alongside `RUNG2-BRIEF.md`.
The brief is the contract for renderer modules M1–M7; this document
is the map of everything Torogao still lacks or has planned *around*
that contract — the wider programme the rung-2 work feeds into. It
authorizes nothing by itself. A gap becomes work only through a
brief, a module pickup under the brief, or an Owner ruling.

**Scope discipline.** Where the brief already owns a subject (M1–M7,
the fathom commissions F1–F5), this register points at the brief and
does not duplicate it. Where an area belongs to another lane (the
Prima C-lane, the paint phases, the audio spike), the register
records the state, the gap, and the researched best approach, and
names the owning lane.

**Lane.** This document lives in the NTRanch torogao tree only
(BLOCH lane law). It discusses adult-game systems in the same
technical register as the rest of the estate's Torogao documents.

## 0. Citation discipline (mirrors the brief §0)

- Estate facts cite their record: a decision number (D-nnnn), a
  file path in the estate, or a landed commit. Numbers without a
  record behind them are not stated.
- External claims cite a source in §13 by number [E-n]. Only
  sources actually opened during research are cited as evidence.
  Sources that were located but not opened are listed separately
  in §13.3 and carry no claims; anything resting on them is marked
  REVIEW.
- Proposed numeric bounds are stated as *proposals* (the brief's
  Appendix B discipline): they bind when a module's first gate
  ratifies or revises them, not before.
- Anything that could not be verified is marked **REVIEW**. Any
  area whose estate records are thin is marked **THIN** rather
  than padded. The full flag index is §12.
- No personal names (D-489): "the Owner", studio names only.
- This document is prose, not code: it is **not-LSP-validated**
  (named per the theodolite clause; no theodolite route exists
  for Markdown verdicts).

## 1. Animation — beyond the brief's M1–M7

### 1.1 Estate state

- The brief owns the renderer side: M1 cast-wide parity, M2 scene
  system, M3 two-body interaction, M4 secondary motion (soft
  body / cloth / hair, XPBD), M5 fluids and particles, M6 shading
  parity, M7 performance and presentation. Poses arrive evaluated
  from `torogao-motion-clip`; the renderer never evaluates clips
  (worktree CLAUDE.md, hard boundary).
- The cross-medium doctrine sits above the renderer: characters
  come from one record and are emitted as 2D or 3D (D-077 as
  amended 2026-10-27); the 2D emitter route is renders of the
  generated 3D (route A; an independent 2D projection was
  rejected); conversion to Unity / Unreal / Blender / Aseprite
  must be specified and deterministic. Video is reference only,
  never a runtime input.
- The quality bar for the whole cross-medium pipeline is
  Laplace3D-class animation (Owner ruling D-196), measured
  against the estate's Laplace reference material via the
  animation-grammar atlas (fathom commission F3 in the brief).
- 2D puppet evaluation has an evaluator of record: the lumen rig
  stack, INP1-fed (D-869). The vendored inox2d is the documented
  fallback, not the route of record.
- Expression doctrine: an expression is whole-body — posture,
  hands, stance, weight — never face-only (Owner rule,
  2026-10-06). Each character has exactly two canonical models
  (chibi base; one adult early-20s mature); see §10.
- Two-body scenes are the programme's stress test and are brief
  M3's subject; the interaction *content* systems that drive
  them in a Prima-class title are §11.

### 1.2 Gaps

- **G-A1 — Clip selection and blending above frozen clips.** The
  estate evaluates frozen authored clips deterministically. It
  has no shared *selection* layer: which clip plays next, when a
  switch is allowed, how switches blend. Per-title S5 shells
  hand-roll this today.
- **G-A2 — IK solver architecture.** M3 proposes an IK residual
  bound (≤1 mm, brief Appendix B, proposal) but no solver
  architecture is chosen: two-bone analytic, CCD, and FABRIK
  have different determinism and look characteristics.
- **G-A3 — Puppet ↔ renderer seam.** Lumen evaluates 2D puppets;
  render3d presents 3D scenes. The handoff — puppet parameter
  packets driving presentation inside a 3D scene (portraits,
  insets, broadcast overlays in §11's C6) — is unspecified.
- **G-A4 — Presentation/staging language.** Camera language,
  staging, and performance direction above M7's frame budgets
  belong to title lanes (§11 C-lane); no shared vocabulary
  exists yet.

### 1.3 Best approach (researched)

**Selection over frozen clips, not a mocap database.** Motion
matching [E-19] is the reference discipline for clip selection:
a brute-force search over the animation database, run every
frame (or every few frames), scoring each candidate by current
pose and velocities plus the predicted future trajectory; if
the winner is not within ~0.2 s of the current playhead, jump
to it with a ~0.25 s blend; several switches per second are
normal. The estate has no mocap corpus and wants none — the
transferable part is the *cost shape and switch discipline*
applied to a small authored clip library: fixed candidate
order, fixed cost weights, fixed search cadence in step counts,
so the selector is exactly as deterministic as the evaluator
under it (D-212). The known cost of the full technique is
database memory (the learned variant, Learned Motion Matching,
exists to compress databases of the >0.5 GB class [E-21,
REVIEW — figures not re-verified this pass]); the estate
avoids that cost by construction.

**IK: two solvers, chosen by chain.** For limbs, closed-form
two-bone analytic IK: fixed cost, exactly deterministic. For
multi-segment chains (spines, reaching arms), FABRIK [E-4]:
joints are treated as points joined by rigid rods of fixed
length; a backward pass anchors the end effector and propagates
to the root, a forward pass re-anchors the root; the symmetric
treatment avoids CCD's end-effector bias, and typically 2–4
full iterations suffice for humanoid chains. A fixed iteration
count (stated per chain at the module's first gate) keeps
FABRIK deterministic; joint limits are enforced by clamping
directions inside the passes. The Khronos tutorial's own
guidance matches this split: CCD is sufficient for short limb
chains and small corrections; FABRIK reads better for full-arm
reaching and is strongly preferred for spines [E-4]. Contact
constraints on bodies ride M4's XPBD pass [E-20] rather than a
third solver.

**Puppet seam: parameter packets.** Inochi2D's model [E-14] —
layered 2D art deformed at runtime by named parameters — is
the 2D analogue of the estate's morph/parameter discipline.
The seam to specify is a plain-data parameter packet per
frame, in the same packet discipline as the renderer's
backend boundary. The fallback's own state confirms the
posture: inox2d describes itself as prototype-state, "not
recommended to use this library in production", with
animations and mesh groups unimplemented [E-14] — a fallback,
never a dependency.

### 1.4 Alternatives and why they lose

- A full motion-matching runtime over a captured database:
  no corpus, wrong memory class, and a search whose cost
  varies with database size; the estate's authored libraries
  are orders of magnitude smaller.
- Neural/learned animation at runtime: violates the
  freeze-to-deterministic-data law unless distilled into
  frozen clips — at which point it is an authoring tool, not
  a runtime system.
- A per-title state-machine graph editor (the Unity/Unreal
  pattern): the estate already pays per-title costs in S5
  shells; the gap is a shared selector, not another editor.

### 1.5 Risks

- Selector cost weights are tuned by eye against the Laplace3D
  bar (D-196), not by a numeric gate alone; expect verdict-pack
  iterations, and remember a session never declares an Owner
  PASS.
- FABRIK iteration counts interact with M3's proposed ≤1 mm
  bound: the bound must be re-stated per chain length at M3's
  first gate (brief Appendix B discipline).

## 2. Sound

### 2.1 Estate state

- `torogao-audio` is landed as the playback substrate (D-1203 /
  D-1221; evidence in `~/workspace/rhythm-sync-spike-20261110/`):
  cpal **0.18.2** pinned with `features = ["pipewire"]`, native
  PipeWire host, frame-count scheduling inside the callback,
  default buffer.
  - Leg 1 (timing): default buffer and Fixed(1024) both scored
    740/740 clicks, 0 missed, drift ≈ 0, at idle and under
    `stress-ng --cpu 10`. Fixed(256) is exact at idle and
    slipped two one-quantum events (256 samples = 5.33 ms)
    under load — inside the strictest D-1203 budget class
    (±16.7 ms).
  - Leg 2 (device loss): absorbed at the session-manager layer
    with ≤2 error callbacks total, against ALSA's 17–35M-call
    POLLERR spin on the same fault.
- **Audio silence rule (standing):** generation, QC, and
  verification are silent; audio exists as files only; primo
  runs only on a null/dummy sink. Nothing in this section
  licenses audible testing.
- The Prima gap analysis's negative census (B12) recorded, at
  its writing: no audio playback dependency anywhere in the
  estate, and no runtime music/SFX synthesis or mixing. The
  playback half is now closed by `torogao-audio`; the rest
  stands.
- Voice is a separate programme: torori owns synthesis.
  Per-character synthetic English voices; a locked reference
  per heroine (Aria discipline — a swap is a re-approval);
  publish-clear engines only: Kokoro and Dia-1.6B, with
  Chatterbox named as the upgrade path. Output is
  English-canonical (D-472). `torogao-voice` is a deterministic
  command *grammar* (an input surface), not synthesis.

### 2.2 Gaps

- **G-S1 — Mixing/graph layer.** Tracks, buses, effects,
  ducking, and parameter automation above raw playback: not
  built. Prima's SessionAudio hook idiom is PARTIAL for exactly
  this reason — the hook exists, nothing sits behind it (§11).
- **G-S2 — Runtime music/SFX synthesis.** The estate
  synthesizes offline (Addition D, §3) for shipped audio
  artefacts. In-game procedural music/SFX generation and
  mixing at runtime does not exist.
- **G-S3 — Rhythm-sync gameplay.** The D-1203 spike priced the
  rhythm genre kit; the kit itself is last in the Phorge kit
  build order and unbuilt.
- **G-S4 — Voice playback integration.** Torori synthesis →
  `torogao-audio` playback inside a title, with per-character
  locked references wired to dialogue (§4, §11): unbuilt.
- **G-S5 — Android audio.** Entirely unmeasured (§9).

### 2.3 Best approach (researched)

**Mixer: evaluate Kira over the landed cpal substrate.**
Kira [E-5] is a backend-agnostic Rust game-audio library
whose default backend is cpal — the same substrate
`torogao-audio` already pins. It provides the missing G-S1
shape directly: tweens for smooth parameter changes, a
flexible mixer with sub-tracks and per-track effects (e.g.
filter), spatial audio, and a clock system for precisely
timing audio events (`ClockSpeed::TicksPerMinute`; sounds
scheduled N ticks ahead). The determinism design rule is the
estate's, and it is not optional: Kira's clock is a musical
wall-clock, while the proven D-1221 pattern derives the
schedule from frame counts inside the callback. The audio
layer must *receive* a frame-counted schedule computed by
the deterministic step; it never owns tempo decisions. Pin
the Kira version at adoption and re-check its API then —
the crate's API has changed materially across major
versions.

**Voice: Kokoro default, Dia for dialogue, Chatterbox as a
frozen upgrade.** Kokoro-82M [E-12]: 82M parameters,
Apache-2.0 weights, v1.0 released 2025-01-27, 54 voices
across 8 languages, 24 kHz output, trained exclusively on
permissive/non-copyrighted data per its model card — the
publish-clear evidence the programme requires. The estate
already locks it: the Surrender Curriculum's voice is
Kokoro `af_heart` (§3). Dia-1.6B [E-10]: generates dialogue
directly from a transcript with `[S1]`/`[S2]` speaker tags
and nonverbal tags (`(laughs)`, `(sighs)`, …), Apache-2.0,
English only, GPU-only at ~4.4 GB VRAM (bf16, RTX 4090,
per its README); voice consistency requires an audio
prompt or a fixed seed — which is the locked-reference
discipline stated in engine terms. Chatterbox [E-11]:
zero-shot voice identity from a short reference (~5–10 s),
Turbo at 350M parameters (English, paralinguistic tags),
Nano at 110M running 3× realtime on 8 CPU cores, and every
output carrying Resemble's PerTh neural watermark.
**Freshness warning:** the Chatterbox README now states the
project is retired research, no longer maintained, code
archived under MIT (as of August 2026) [E-11]. The upgrade
path remains usable only as a vendored, frozen dependency —
never as a living upstream.

**Synthesis stays authoring-time.** The estate's pattern —
synthesize offline to files under provenance law (§3,
Addition D), ship files, play files — keeps the silence
rule testable and the runtime deterministic. Runtime TTS
would break both; it enters only if a title's brief rules
it in explicitly.

### 2.4 Alternatives and why they lose

- FMOD / Wwise: proprietary middleware; wrong shape for a
  Tiger Style crate stack, and their schedulers sit outside
  the estate's determinism boundary.
- rodio alone: playback without the mixer/clock model;
  G-S1 would be rebuilt by hand in a worse shape.
- Cloud TTS at any point: fails publish-clear and the
  offline posture.

### 2.5 Risks

- Real-time thread discipline: no allocation on the audio
  thread; Kira's own docs flag dev-profile optimisation
  needs for its dependency chain [E-5].
- PipeWire behaviour is desktop-Linux evidence only; the
  D-1221 budgets have no Android counterpart (§9, THIN).

## 3. Hypnosis systems

### 3.1 Estate state

- The technique record is the hypno-technique skill
  (`~/workspace/custom-skills/local-hypno-technique/`, landed
  2026-11-02, build verified 2026-11-07), built from a
  1,777-file corpus of Bambi-genre text decomposition (primo
  `~/.local/share/ntranch-game-re/hypno/`). Its confidence
  discipline is the estate's honesty model for this whole
  area: of 726 hidden layers, only 123 are corroborated
  (7 VERIFIED + 44 CONFIRMED + 72 PARTIAL); 603 UNCONFIRMED
  items are never ground truth.
- Techniques with measured parameters: T1 SSB-AM fixed
  (band-limit 300–3,000 Hz; 16,000 Hz carrier; upper sideband
  isolated at carrier+300; gain ×5; limiter ceiling 0.8;
  decode ×4; minimum sample rate 44,100 Hz); T2 adjustable
  carrier (default 17,500 Hz; allowed 14,000–20,000 Hz;
  practical window ~16–17.5 kHz at 44.1/48 kHz); T3 layered
  stems; T4 hidden-layer grammar (small closed affirmation
  set, cue phrase as cycle head, present tense, second-person
  lean); T5 drone loop; T6 session architecture (set-up →
  induction → deepener → suggestion body → trigger
  installation → emergence, plus an "everyday" accumulation
  variant); T7 binaural — the corpus holds **no** parametric
  evidence; the estate anchor is Addition D (carrier `baseHz`
  plus a per-phase `beatHz` pacing curve); T8 layer-subset
  publishing (editions are mix decisions over kept stems);
  T9 visual accompaniment specimen (portrait 720×1280,
  60 fps, exactly 120.000 s loop; **no** flash-text schedules
  or spiral parameters exist in the evidence — the estate's
  visual primitives are designed, not extracted); T11
  null-calibrated detection (a real layer must beat the
  bed-only null ceiling).
- **Addition D is production law:** deterministic seeded
  procedural audio; house sample rate 48 kHz; provenance
  −16 LUFS / −1 dBTP; renders carry recipe hash + seed +
  licence fields; a subliminal is specified by its depth
  below the bed (`levelDb < 0`), never by absolute level;
  refusals `TG-SESSION-SUBLIMINAL-LEVEL` /
  `TG-SESSION-SUBLIMINAL-PEAK` are refused, never clipped
  into compliance.
- **The build is landed** (torogao `e4cb21d1`, branch
  `pax/fathom-trio-20261107`; G-1 glide extension `9d89769`):
  SSB-AM encode + decode-verify in `torogao-cli` (decode is
  a gate instrument, **never** a shipped feature);
  `HypnosisTrackBinding` in `torogao-author`; visual
  induction primitives (phase-synced loop playback,
  frame-accurate flash-text cues by reference, seeded
  spiral/overlay hooks — all on the session's shared sample
  clock); `torogao-session/2` typed blocks (induction →
  deepener → suggestion → anchor → emergence) with
  estate-original, versioned affirmation sets and triggers
  as data. V1 session records keep compiling.
- The Surrender Curriculum is the shipped artefact on this
  law: a 10-lesson ladder (L1 Permission → L10 The Spiral)
  with a three-dial escalation — subliminal depth from
  −30 dB to −18 dB across the ladder, descending/lengthening
  binaural plateaus, thinning narration — series constants
  48 kHz, −16 LUFS / −1 dBTP, 200 Hz anchor ear, SSB-AM
  carrier 17,000 Hz, Kokoro `af_heart` at speed 0.73 with
  seed 20261108, per-lesson amber→violet hue drift. L3–L10
  fold into the remake *Where She Asks to Fall* (title
  ruled); L1/L2 stay live until that game's page ships.
- Progression heritage: Strobe's capability progression is
  hypnosis-count-based (HYPNOSYNC heritage; Strobe
  commissioned 2026-10-19). Persona/register law for
  Prima-class titles is D-1180 / D-1193 / D-1228 (§11).

### 3.2 The hard safety law — OE-1b (D-785)

Dynamic-stage transitions at the original ~0.45 s budget,
with flashing and high contrast at that speed, are a
seizure risk. Persona-batch and dynamic-stage landings are
**held** until: transitions are slowed materially;
large-area luminance flicker/inversion is removed; shard
contrast and edges are softened; and a persisted
Reduced-motion toggle is kept. The mechanism of record is
flash rate × luminance contrast × flashing screen area —
and the gate is revised clips judged by the Owner's eye,
not a duration number alone. **Everything in this section,
and every visual primitive in §4, ships under OE-1b.**

### 3.3 Gaps

- **G-H1 — Title-level session direction.** The landed build
  is authoring + session machinery. No shipped Torogao
  title yet runs a live induction sequence against the
  renderer; a session director is Prima C-lane work (§11).
- **G-H2 — "Eye-tracking beats" are authored, not sensed.**
  Commissions name eye-tracking beats; the estate has no
  eye-tracking hardware or input path. Until hardware
  exists, treat these as gaze-*direction* cues authored
  into clips. **REVIEW** any design that assumes sensed
  gaze.
- **G-H3 — Hypnosis-count progression as a shared system.**
  Strobe's progression is design canon, not an engine
  system other titles can consume.
- **G-H4 — Visual primitive parameters are designed, not
  measured** (T9). Spiral rates, overlay densities, and
  flash-text timing ship only under OE-1b discipline with
  reduced-motion variants.

### 3.4 Best approach (researched)

**Keep the landed architecture; the gap is integration,
not technique.** Subliminals as data (depth below bed),
sessions as typed blocks, decode-verify as a gate-only
instrument, provenance on every render — this is already
the researched shape.

**SSB-AM cross-check.** Standard single-sideband practice
suppresses the carrier and one sideband; the remaining
sideband carries the full modulating signal, the upper
sideband sitting above the carrier [E-16]. That is exactly
T1's shape (upper sideband isolated at carrier+300; the
encoder gate requires ≥99% of encoded band energy within
[carrier+300, carrier+3,000] Hz and none below the
carrier). SSB reception requires carrier reinsertion at
the receiver [E-16] — in estate terms that reinsertion
*is* the decode-verify gate, and it stays out of shipped
builds by design.

**Photosafety: WCAG is the floor, D-785 is the law.** The
WCAG 2.3.1 thresholds [E-1]: content passes if there are
no more than 3 general flashes and no more than 3 red
flashes in any 1 s period, **or** the combined flashing
area is ≤ 0.006 steradians within any 10° visual field
(25% of that field; a 341×256 px rectangle at 1024×768 is
the reference estimate). A general flash is a pair of
opposing relative-luminance changes ≥10% of maximum where
the darker image is below 0.80 relative luminance; a red
flash is an opposing transition involving saturated red.
W3C names automated analysis tooling (Harding FPA, Trace
Center PEAT) suitable for exported-video gates. The
estate's posture is stricter than WCAG on purpose:
flicker-free by construction, reduced-motion toggle,
Owner's eye as the gate.

**Honesty register — efficacy is contested, and the
register says so.** The evidence base for subliminal
influence is weak and conditional: a meta-analysis of
strong studies found effects on actual choice between two
alternatives **not statistically significant**, and the
supported effects operate only in line with pre-existing
intentions and contexts [E-13]. Subliminal layers in this
estate are therefore a *disclosed authoring technique and
a genre convention* — specified, measured, and provenance-
stamped as audio engineering — and no design, marketing,
or in-fiction claim may present them as a proven
behavioural effect.

### 3.5 Alternatives and why they lose

- Runtime-generated (non-frozen) induction media: breaks
  D-212 and the recipe-hash provenance law in one move.
- Third-party hypnosis-audio libraries or corpus text
  shipped as content: the corpus is study-only by law —
  its texts are never reproduced, shipped, or used as
  generation fodder; the estate ships estate-original text
  only, and the validator refuses affirmation sets whose
  provenance names a studied creator.

### 3.6 Risks

- Carrier audibility varies with listener age and hearing;
  the ~16–17.5 kHz practical window at 44.1/48 kHz is a
  population statement, not a per-listener guarantee.
- The OE-1b hold is Owner-eye-gated; no measurement in
  this register clears it.

## 4. Persona 5-style UI/UX

### 4.1 Estate state

- The shell is egui + egui-wgpu (Lumen-as-TUI adoption,
  D-895; Stage 0 complete; Bevy was falsified for the
  shell role). The Vulkan renderer sits behind the backend
  boundary; shell and renderer are separate layers.
- The estate's kinetic-UI technique set is specified as a
  skill (`~/workspace/custom-skills/local-kinetic-ui/`)
  with measured timings:
  1. **Typewriter** — variable rate; commas breathe 150 ms,
     periods land 300 ms, ellipses hang 500 ms+; blinking
     continue-cursor.
  2. **Dialogue panel** — easeOutBack pop-in; portrait +
     name tag; per-line slide.
  3. **Personality progress** — fast start, honest stall at
     90–99% with a self-aware status line; a stall past
     ~2.5 s stops being charming.
  4. **Glitch text** — RGB split + slice offsets +
     character scramble, 2–6 frames.
  5. **Text disintegration** — glyphs to particles, word by
     word, at emotional beats only.
  6. **Count-up** — eased, with digit roll.
  7. **Emphasis pop** — 1.0 → 1.25 → 1.0, easeOutBack.
  Rules: readability is the floor (test at phone scale);
  one voice per line, never stacked effects; stalls must
  resolve.
- Demo doctrine (D-194): a demo is production-esque — real
  UI/UX, smooth animation, sound, features; a slice of the
  finished product, never a systems demo.

### 4.2 Gap

**G-U1 — No P5-class presentation layer exists.** No
shipped Torogao title carries a Persona 5-class menu
system: angular masked compositions, pose-driven menu art,
animated typography as identity. egui is immediate-mode;
its ceiling is styled rectangles. P5-class polish demands
a retained, timeline-driven layer above it — authored
transition timelines as data, custom-drawn vector panels
and masks, per-screen bespoke composition — plus the
kinetic-UI techniques implemented as a shared crate rather
than a skill document. None of that layer is built.

### 4.3 Best approach (researched)

**What Atlus actually did** (CEDEC + KYUSHU 2017 talks,
reported in [E-2]): the UI carries the game's identity
*and* its information. P5 uses **no sub-colors except
HP/MP**, protecting a single identity colour. The player's
gaze is guided by central lines, and angle + contrast
changes carry context; relevant information always sits on
the mainly-white side. Menus were composed layout-first
(in Photoshop); a special pose→2D tool then rendered a
spinning 3D model ending in a pose as the menu art. Menu
and battle UI had *separate* designers, and motion was
checked frame by frame by designer and programmer
together.

**The calibration lesson** (director interview, [E-3]):
the earliest iterations were "aggressively animated" to
the point of illegibility — messages at extreme diagonals
drew an internal "this is way too much" — and were
deliberately toned down: subtler rotations, calmed
animations, until style and user-friendliness balanced.
P5-class is not maximal motion; it is motion budgeted
against legibility.

**The structural reading** ([E-17]): the menus are legible
not despite the chaos but because of it — every menu has a
completely distinct visual identity, so the player always
knows where they are without reading a header, and the
motion signals state transitions clearly enough that an
input never feels unregistered. The costs are named
plainly: enormous production expense, near-zero
reusability, and compositional layouts that do not stretch
for localisation. The estate's mitigations: text is
English-canonical (D-472), so the localisation-stretch
problem is absent by law; and the reusable half — the
timeline/mask/transition engine — is shared even when
each title's compositions are bespoke. A visual census of
menu states for reference lives in the Game UI Database's
Persona 5 coverage [E-22, listed, not opened this pass].

**Recommended shape for Torogao.** A UI-motion layer
(crate name is a proposal, not a ruling): timelines as
frozen data — keyframed transforms, masks, and eases
evaluated by the deterministic step, the same freeze
discipline as motion clips. egui remains the widget/input
substrate and the editor/debug surface. Title menus and
screens render through the Vulkan presentation path, with
egui overlays only where immediate mode is genuinely the
right tool. Dialogue presentation adopts the kinetic-UI
timings above as named constants under the phone-scale
readability gate. Menu art follows the Atlus route: pose
the estate's own 3D characters and render the pose — the
renderer and cast already exist (§10); the pose→menu-art
tool is the new piece, and it is an authoring tool, not a
runtime dependency.

### 4.4 Alternatives and why they lose

- Pure egui styling: cannot reach masked/angled
  composition or pose-art menus; rectangles are the
  ceiling.
- A web/CEF UI layer: heavyweight runtime in the shipped
  path; determinism and offline posture both fail.
- Per-title bespoke UI code with no shared engine: pays
  Atlus's full cost while banking none of the reusable
  half.

### 4.5 Risks

- Bespoke composition is the most expensive UI path there
  is — Atlus's bill is the evidence. Budget per title,
  not per engine.
- The OE-1b Reduced-motion toggle (§3.2) must reach UI
  motion too: every timeline needs a reduced variant,
  which doubles the motion-authoring surface. Design the
  layer so the reduced variant is derived, not authored
  twice, wherever the motion permits.
- Frame-by-frame designer/programmer checking is a
  process cost; tooling does not remove it.

## 5. Rendering leftovers (pointer section)

The brief owns this substance; this section exists so the
register is complete without duplicating M6/M7.

- **Shading parity (rung-1 verdict item 6)** is brief M6.
  Measured state: rung 1 renders brighter than the plates —
  full-frame mean signed Δ ≈ (+42.5, +43.5, +41.7)/255;
  wrap-vs-plain diffuse ≈ 1.37× on the Sun and ≈ 1.96× on
  the Fill; an AgX-fit residual of +9…+12/255 against a
  proposed bound of ≤3/255 (brief Appendix B, proposal).
  No ad-hoc retune: the M6 fit is the work. The fit target
  follows the AgX transform's published source [E-23,
  listed — the brief's own citation, not re-opened this
  pass].
- **Fill shadow (item 7)** is brief M7/presentation scope:
  rung 1 shadows from the Sun only (2048², 3×3 PCF); the
  plates shadow from both lights. Plate spec v2 (D-1224,
  6144×7680, TAA 256 samples) is the target of record
  (`docs/PLATE-SPEC.md`).
- **TAA vs MSAA** for the Vulkan path: no estate
  measurement yet chooses between them against the plates.
  The plates' own reference is Blender TAA at 256 samples
  (plate spec v2). **REVIEW** until M7 measures both.

## 6. Paint / editor charter — torogao-paint

### 6.1 Estate state

- The charter is accepted (D-964): minimum Krita-class
  drawing tooling. The D-965 expansion moved the
  not-minimum list **in scope** as late phases: vector
  layers, text tooling, animation timeline, brush-preset
  breadth, filter gallery, perspective assistants, and
  CMYK with honest cost accounting. PSD stays OUT;
  OpenRaster is the Krita round-trip format.
- **Phase 1 is landed** (D-991; branch
  `pax/paint-phase1-20261104`, tip `8944b138`, marker
  `refs/landing/paint-phase1-d991`, no push):
  `crates/torogao-paint` document core — sparse 256 px
  tiles (`TILE_BYTES` 262,144), integer-exact pixel math
  (`mul255` proven over the full 256×256 domain), blend
  modes normal/multiply/overlay, eraser as
  destination-out, Krita-style two-stage stroke semantics
  (stamps at flow, deposit at opacity), a 256-entry
  falloff LUT, fixed-point positions at 1/64 px, and the
  op log as source of truth.
- Input evidence: spike S-1 (synthetic) PASS — 300
  distinct pressure levels, 0 monotonicity violations,
  0.000 px position deviation, pen-down first sample
  preserved, independent evdev witness agreement
  1,279/1,279 SYN frames. Spike S-2 (real hardware) is
  PENDING-HARDWARE indefinitely: the tablet question is
  closed by Owner ruling and paint proceeds mouse-first.
  Do not resurface it.

### 6.2 Gaps

- **G-P1 — Phases 2+:** brush-engine breadth beyond the
  Phase-1 stamp model; the D-965 late phases (vector,
  text, timeline, filters, perspective, CMYK); OpenRaster
  round-trip implementation.
- **G-P2 — Editor surface.** Phase 1 is a document core,
  not an application: no canvas UI, no tool palette, no
  preset system exists yet.

### 6.3 Best approach (researched)

Krita's architecture [E-9] validates the Phase-1 choices
and names the next problems. Krita stores paint layers as
fixed-size tiles under a tiled data manager (the survey
reports 64×64 data tiles, with a separate 256-granularity
GPU texture pool for display — tile size is a subsystem
choice, not a law; the estate's sparse 256 px tiles follow
the same bounded-memory logic). Krita keeps its brush
engines and compositing CPU-bound and uses the GPU canvas
for display plus **Instant Preview**: live stroke feedback
rendered against a lower-resolution proxy while the
full-resolution stroke computes, swapped in when ready.
Two transferable items for Phase 2+:

1. **Proxy preview for expensive brushes.** The editor
   surface will run over the estate's Vulkan renderer,
   where a reduced-resolution stroke proxy is cheap —
   adopt the Instant Preview shape rather than blocking
   input on full-res strokes.
2. **Brush behaviour as composable option/sensor data.**
   Krita models a brush as options, each a curve mapping
   an input sensor (pressure, tilt, speed) to a
   parameter. That is the right shape for torogao-paint
   presets: deterministic, serializable, op-log friendly
   data — never code per preset.

### 6.4 Alternatives and why they lose

- A GEGL-style lazy node graph (GIMP's core [E-9]): built
  for filter pipelines and out-of-core processing, not
  stroke latency. Krita's tile + CPU-engine split is the
  closer prior art for a painting tool.
- GPU-compute brushes first: Krita deliberately keeps
  engines CPU-bound; the estate's op-log determinism is
  likewise easier to keep exact on the CPU path.

### 6.5 Risks

- CMYK's "honest cost accounting" is a charter obligation
  (D-965), not a feature flag — the phase that scopes it
  must price it, not wave it through.
- Editor-surface scope creep is the historical failure
  mode of this charter; phases stay gated one at a time,
  on the D-991 pattern.

## 7. Device layer

### 7.1 Estate state

- The device library is **adopted** (D-676): the Buttplug
  Rust crate, v12, BSD-3-Clause, as Torogao's device layer
  **behind** the Lovense-MCP safety gate — master off by
  default, ceiling clamped, synchronous stop, no resume on
  reconnect, 5 s heartbeat watchdog, pure `decide()` gate,
  8 tools. It is never in the ai.qompass registry, never
  on qompass.ai, never routed to Azoth, and **never
  branded 'Buttplug'** in any shipped surface.
- Placement is ruled: **no new crates.** Funscript is a
  module in `torogao-format` (serde model + conversion
  to/from Torogao take/timeline types). TCode is transport
  inside a new `device` module in `torogao-engine`,
  alongside the D-676 adapter seam — one module owns
  everything that can reach hardware, with the gate in
  front of all of it. TCode builds to the verified 0.3
  spec; 0.4 is a watch item behind conformance tests.
- State honesty: the adoption is **not yet in tree** — no
  device module and no library dependency exist on
  origin/main (2026-11-05 correction). Prima C9 restates:
  zero landed code. The in-tree seam precedent is Fall's
  pairing-tick idiom: schedule data in the shell, off by
  default, never required.

### 7.2 Gap

**G-D1 — The whole layer is unbuilt:** adapter
implementation, Funscript conversion, TCode transport,
conformance tests, and per-title integration (Prima C9
class) all remain. Android Bluetooth behaviour for this
layer is unmeasured (§9).

### 7.3 Best approach (researched)

Build to the library's own architecture [E-7]. Its
DeviceConfigurationManager matches discovered hardware —
Bluetooth LE, USB, serial, and network specifiers — to
protocol configurations covering hundreds of devices, and
its user-configuration layer supports allow/deny lists
and **per-device message-attribute limits** (for example,
capping one device's vibration ceiling in configuration
rather than in call sites). That mechanism maps directly
onto the estate's ceiling-clamp law: enforce the clamp in
*both* the MCP `decide()` gate *and* the device
configuration, so no single layer's defect can exceed the
ceiling. Above the `device` module, everything speaks
take/timeline data (Funscript-converted), so titles are
fully testable with zero hardware present — the Fall
pairing-tick precedent, generalized.

### 7.4 Alternatives and why they lose

- A separate Intiface-class service process in the
  shipped path: external runtime dependency; packaging
  and posture fail. The crate embeds instead.
- Hand-rolled BLE per device family: protocol breadth is
  precisely what the library exists to absorb.
- A single vendor's proprietary API as the only path:
  the estate's gate is vendor-neutral by design.

### 7.5 Risks

- Upstream cadence vs the estate's pinned-version
  discipline (D-646): pin, and gate upgrades like any
  fleet dependency.
- Device identifiers are **not portable across
  platforms** (the library's own documentation [E-7]):
  save data and pairing records must key devices by
  estate-side identity, never by bus address.

## 8. Scene / save / determinism infrastructure

### 8.1 Estate state

- **Determinism is law (D-212):** no wall-clock input
  anywhere in evaluation, simulation, or render; identical
  evaluated state → bit-identical frame on the bench
  machine, witnessed by per-frame SHA-256 (rung 1:
  360/360 exact). Fixed steps, fixed iteration counts,
  fixed order.
- The scene system is brief M2; `torogao-scene` is a new
  estate crate under the brief.
- **There is no shared game-save crate, deliberately.**
  `torogao-session` holds canonical RON records (a replay
  must reproduce the live state hash exactly).
  `torogao-app/src/save.rs` carries session evidence plus
  transport play state, with both hashes proven equal at
  restore — a mismatch is a typed refusal. Per-title S4
  crates own their save vocabulary in-crate (Fall's
  `state.rs` is the worked example). Refusing-loader
  discipline everywhere.

### 8.2 Gaps

- **G-I1 — Renderer simulation-state persistence.**
  Replay-from-hash covers evaluation, not arbitrary
  mid-scene simulation state (M4 soft-body / M5 fluid
  state). No capture/restore story exists yet.
- **G-I2 — Cross-title replay tooling.** Verdict packs
  and replay checks are per-lane crafts today; no shared
  harness consumes a replay hash across titles.
- The absence of a universal save crate is **not** a gap:
  vocabularies are per-title by evidence. The shared part
  is the hash + refusal discipline, and it already lives
  in the `torogao-session` / `torogao-app` patterns.

### 8.3 Best approach (researched)

The fixed-step canon is Fiedler's accumulator [E-6]: the
renderer *produces* time and the simulation *consumes* it
in fixed `dt` chunks; leftover time becomes the
interpolation alpha between the previous and current
state; frame time is clamped (0.25 s in the canonical
example) so a hitch cannot trigger the spiral of death.
Exact reproducibility requires the fixed step itself —
with variable `dt`, floating-point non-associativity
((v·dt) + (v·dt) ≠ v·(2dt)) makes runs differ in the low
bits. D-212 is this pattern with the interpolation half
deliberately amputated for gate work: gates replay at
exact steps, and any presentation-layer interpolation
must be proven never to feed back into evaluated state.

For G-I1, extend the landed save discipline rather than
replacing it: snapshot solver state **as data at a step
boundary** (never mid-iteration), hash it, version it
the way `torogao-session/2` versioned the session format,
and refuse on mismatch.

### 8.4 Alternatives and why they lose

- Variable-dt with "close enough" determinism: fails the
  bit-identical witness that every gate in the programme
  rests on.
- A universal save crate imposed now: premature —
  vocabularies differ per title on the evidence of four
  landed S4 crates.

### 8.5 Risks

- GPU-side simulation (brief M5) threatens bit-determinism
  across drivers. The control is the brief's packet
  discipline: simulation state enters presentation as
  evaluated state. Android GPUs widen the variance (§9).

## 9. Android

**THIN — the thinnest section in this register.** The
estate's Android evidence is one spike phase; everything
quantitative here is a target or an open question, and is
marked as such.

### 9.1 Estate state

- Dual-target law (D-1094): titles run on native Linux
  **and** Android, under the S6 + S6b gates, with the
  D-212 determinism gate applying on both. Route 1 is
  winit + android-activity.
- The Android spike's Phase 1 landed (under takeover,
  2026-11-09); Phase 2 waits for the primo heavy-compute
  slot.
- The D-1213 envelope (R8) states Family B as a *target*:
  one smoothed character at ≥30 fps on mid-tier Android.
  **REVIEW until measured.**
- Open Owner calls carried from the gap analysis (D-1213
  §6): Family B shipping density; the Android GPU floor —
  Vulkan-only vs a GLES fallback, where a fallback means
  a second morph path whose cost must be priced.
- Unmeasured on Android, all of it: renderer performance,
  audio (the D-1221 budgets are desktop PipeWire evidence;
  Android audio runs through cpal's Android host), device
  layer Bluetooth (§7), GPU determinism variance (§8).

### 9.2 Best approach (researched)

Host exactly as Route 1 rules: winit + android-activity.
The glue crate loads the app's `cdylib` from the
Activity's `onCreate`, runs `android_main` on a native
thread, and marshals lifecycle and input events between
the Java and native threads [E-15]. Consume
android-activity **through winit's re-export**
(`winit::platform::android::activity::*`) so the glue
version can never split from winit's — the docs warn the
versions must match because the glue owns the entry
point. `NativeActivity` is the convenient start (no
Java/Kotlin build step); `GameActivity` is the
richer-input path (AppCompat base, onscreen-keyboard and
gamepad growth) — choose per title at its S6 gate, not
globally [E-15].

The GPU-floor question is a **measurement brief on real
mid-tier hardware**, not a debate. The renderer's backend
boundary exists precisely so this can be decided with
numbers: a GLES fallback would be a second backend behind
the same boundary, and pricing its morph path is the
work the gap analysis already names.

### 9.3 Alternatives and why they lose

- GameActivity-first for everything: Maven/Java build
  weight for titles that may never need its input
  surface.
- A separate Android fork of the renderer: the backend
  boundary law exists to prevent exactly this.

## 10. Character factory

### 10.1 Estate state

- The code-first aim (D-077, as amended 2026-10-27): code
  is truth for logic/runtime; editor state is an authoring
  source under capture discipline; the end state is
  **characters from TEXT descriptions, emitted as 2D or 3D
  from one record**; conversion to Unity / Unreal /
  Blender / Aseprite must be specified and deterministic.
- Landed pieces:
  - The lumen sprite-to-3D factory: ingest a 2D sprite →
    lock identity → project onto the canonical rigged base
    (Mixamo-compatible skeleton, morph-target expression
    set) → parametric body/appearance edits → glTF/GLB-first
    export to Blender, Unity, Unreal, Godot.
  - `torogao-vrm`: GLB JSON-level manipulation, VRM 1.0
    export, byte-preserved geometry.
  - `torogao-gencond` (D-822): RON conditioning record +
    compiler into torogao-gen-graph/1, structural-off
    semantics, typed uniform-map refusal.
  - Cast records of record:
    `docs/cast-records/<girl>.md`.
    The six-girl rig programme is complete: 54 joints,
    39 morph target sets per girl (36 `Fac_*` + bust sets;
    POSITION+NORMAL deltas, sparse — `Fac_Eye_Close` moves
    232 of Prima's 13,920 Body verts), per brief §2.
  - The two-model rule: exactly two canonical models per
    character (chibi base + one adult early-20s mature);
    extra designs are non-canonical and purged/quarantined;
    classification is by looking, never by filename.

### 10.2 Gaps

- **G-C1 — The text→record compiler is the unbuilt
  centre.** Per-girl records exist as documents. A single
  deterministic pipeline — text description → character
  record → both emitters (3D GLB and route-A 2D renders)
  — is D-077's end state and is not implemented as one
  pipeline.
- **G-C2 — Emitter coverage is partial.** The Blender
  path is proven via the plate pipeline; Unity / Unreal /
  Aseprite emitters are specified, not built. **THIN.**

### 10.3 Best approach (researched)

**Emit VRM 1.0 as the interchange superset where humanoid
interoperability matters.** VRM 1.0 is glTF 2.0 plus
modular `VRMC_*` extensions [E-8]: `VRMC_vrm` (meta,
humanoid, expressions, lookAt), `VRMC_springBone`
(per-joint spring chains — stiffness, gravityPower,
gravityDir, dragForce, hitRadius — with sphere/capsule
colliders), `VRMC_node_constraint`, and
`VRMC_materials_mtoon` layered over plain PBR (plain
`pbrMetallicRoughness` is legal VRM 1.0). Three facts do
the work for this estate:

1. The spec mandates a runtime evaluation order —
   LookAt → Expressions → Constraints → SpringBone —
   which slots directly into fixed-order determinism
   (D-212).
2. The humanoid requirement is 15 bones; the estate's
   54-joint cast rig is a superset.
3. SpringBone gives M4's authored secondary motion an
   industry-standard *data format* (frozen chains and
   colliders), while XPBD [E-20] remains the solver for
   simulated bodies.

For Rust emission, pixiv's `vrm-spec` crate provides
serde structs for the VRMC 1.0 extensions generated from
the official JSON schemas (Apache-2.0) [E-8];
`torogao-vrm` currently manipulates GLB JSON directly,
so schema-generated structs are the lower-risk path for
new emitters.

### 10.4 Alternatives and why they lose

- Plain glTF only, no VRM layer: loses the standardized
  expression/spring semantics that consumers already
  implement.
- VRM 0.x: frozen ecosystem, group-based spring physics,
  0–100 expression weights — superseded.
- Hand-rolled per-engine emitters first: D-077 demands
  specified, deterministic conversion from one record —
  emitters in a stated order, GLB/VRM first as landed.

### 10.5 Risks

- Morph-target sparsity support is uneven across
  importers [E-8]: emit dense first, sparse only after
  per-target validation. The estate's face morphs are
  sparse by nature (see §10.1), so this validation is a
  gate, not a hope.
- VRM meta requirements (e.g. `licenseUrl`, authors)
  interact with D-489: authors fields carry studio names
  only.

## 11. Prima-class game systems (the C-lane)

Requirements of record:
`docs/PRIMA-TOROGAO-GAP-ANALYSIS.md`.
This register records the lane; its design decisions belong
to the C2 spec, not here.

### 11.1 Estate state

- **Needs (A1–A16):** a stimulation simulation (3 lanes ×
  3 intensities); a tempo engine with audience coupling;
  the inverted ladder (D-1180: Composed → Stirring →
  Yearning → Overwhelmed → Whiteout — she wants it, and
  loves what is happening to her; there is no Forced
  axis); surrender 0–1000 across ten veils / 11 stages;
  events exactly {Ask, Bargain, Swoon}; the session shape
  (cold open → tease → show → marked scenes → sign-off /
  afterglow); Free Play + Recall modes; cinematic staging
  under OE-1b discipline (§3.2); a broadcast layer with
  reactive comments (10 trigger classes, gated by phase
  mask); her-lines in 3 kinds {Spoken, InnerThought,
  AsWritten}, InnerThought being the confession register;
  the self-rewatch conditioning loop (A10 / D-1193 —
  rewatch is a progression verb); a **recorded-only**
  audience register (D-1228 — no live-audience element);
  per-stage performance sets (11 stages × 5 phases;
  talk-mouth phoneme classes); ~403-clip-class audio scale
  (A13); a save vocabulary (A14); endings (A15) that are
  **proposals, unruled** — {Unveiled, Hers Openly, Still
  Wanting} plus stop-outcomes. Do not treat A15 as canon.
- **Haves (B1–B12):** S4 pure-crate pattern FULL across
  four titles; S5 shell pattern FULL; SessionAudio hook
  idiom PARTIAL (the blocker is §2's G-S1); puppet
  evaluation FULL for 2D; motion-clip FULL evaluation /
  PARTIAL presentation; gate stack FULL; AUTH-G PARTIAL;
  offline audio synthesis FULL for authoring, NOT
  playback; and the B12 negative census — no
  comment/audience system, no gallery/rewatch machinery,
  no device code (§7).
- **Build order (C1–C11):** C2 spec → C5 ruling → C1
  `torogao-prima` crate → C7 comments → C3
  `torogao-audio` (parallel; blocks at C6) → C4 sequencer
  → C8 rewatch → C6 broadcast shell → C10 content → C11
  Android. C9 (real-device seam) only if ruled. Critical
  path: C2 → C5 → C1 → C4 → C6 → C11/S7.

### 11.2 Gap

**G-PR1 — Essentially the whole C-lane is unbuilt.** The
C2 spec is the first artefact. Renderer rung 2 supplies
this lane's presentation substrate (M2 scenes, M3
two-body, M7 presentation); it does not substitute for
any C item.

### 11.3 Best approach (researched)

**Tempo: the Director pattern, re-signalled.** The
canonical pacing engine is Left 4 Dead's Director [E-18]:
a per-participant intensity estimate that rises under
pressure and decays when safe; the group maximum drives
a four-phase loop — Build Up → Sustain Peak → Peak Fade
→ Relax — adjusting pacing and frequency rather than
difficulty amplitude; boss-class beats are dealt from a
shuffled deck that never repeats consecutively. Prima's
tempo engine is the same shape with different signals:
craving/willingness plus audience heat in place of
survivor stress. The estate adaptation has one hard
modification: every shuffle is seeded-deterministic —
deck order derives from the session seed and is recorded
in the save/replay record (D-212). **Comments are the
response-rule half of the same tradition:** trigger
classes + phase masks, already the A-shape — implement
them as data tables evaluated by the deterministic step,
never as wall-clock reactions.

### 11.4 Alternatives and why they lose

- Hand-scripted session timelines only: the A2
  requirement is explicitly a coupled engine, not a
  playlist.
- Runtime LLM-driven comments or lines: non-deterministic
  and against the offline posture. Generated text belongs
  at authoring time, frozen into tables — that is the C7
  shape.

### 11.5 Risks

- A15 endings are unruled proposals; nothing downstream
  may presume them.
- The audience is recorded-only (D-1228). Any design
  drifting toward live-audience simulation contradicts a
  ruling, not a preference.

## 12. REVIEW / THIN index

| Flag | Where | What is unverified or thin |
|---|---|---|
| REVIEW | §1.3 | Learned Motion Matching compression figures [E-21] — reported, not re-verified this pass |
| REVIEW | §3.3 G-H2 | Any design assuming sensed eye-tracking; no hardware or input path exists |
| REVIEW | §5 | TAA vs MSAA for the Vulkan path — unmeasured until brief M7 |
| REVIEW | §9.1 | Family B ≥30 fps mid-tier Android — a target, not a measurement |
| THIN | §9 | Android overall: one spike phase of estate evidence |
| THIN | §10.2 G-C2 | Unity / Unreal / Aseprite emitters: specified, not built, little estate record |
| Held | §3.2 | OE-1b (D-785): persona-batch + dynamic-stage landings held for the Owner's eye |
| Unruled | §11.1 | Prima A15 endings are proposals, not canon |

## 13. Sources

### 13.1 Estate records

- `RUNG2-BRIEF.md` (this worktree) — M1–M7, §2 rung-1
  state, §5 fathom commissions, §6 research routes,
  Appendix B proposed bounds.
- `docs/GAP-ANALYSIS.md`
  (D-1213) — requirements R1–R8, gaps G1–G10, §6 Owner
  calls.
- `docs/PRIMA-TOROGAO-GAP-ANALYSIS.md`
  — needs A1–A16, haves B1–B12, build order C1–C11.
- `docs/rung1-verdict/VERDICT.md`
  and `ADDENDUM-ITEM5.md`; `PLATE-SPEC.md` (D-1224).
- `~/workspace/rhythm-sync-spike-20261110/` — `REPORT.md`,
  `PIPEWIRE-ADDENDUM.md`, `REQUIREMENT.md` (D-1203 /
  D-1221).
- `~/workspace/custom-skills/local-hypno-technique/SKILL.md`
  + Addition D; `~/workspace/custom-skills/local-kinetic-ui/`.
- `~/workspace/night-20261026/DECISIONS.md` — the decision
  register (D-numbers cited inline throughout).
- Landed commits cited inline: torogao `e4cb21d1`,
  `9d89769` (hypnosis build); paint Phase 1 tip `8944b138`
  (D-991).

### 13.2 External sources — opened during research

- [E-1] W3C, *Understanding Success Criterion 2.3.1:
  Three Flashes or Below Threshold* —
  https://w3c.github.io/wcag21/understanding/three-flashes-or-below-threshold.html
- [E-2] Siliconera, *Atlus Reveals The Design Secrets
  Behind Persona 5's Distinctive UI* (CEDEC + KYUSHU
  2017) — https://www.siliconera.com/atlus-reveals-design-secrets-behind-persona-5s-distinctive-ui/?ref=exresearch.co
- [E-3] Eurogamer, *Let's talk about Persona 5's menus*
  (Hashino interview) —
  https://www.eurogamer.net/lets-talk-about-persona-5s-menus
- [E-4] Khronos Vulkan documentation, *FABRIK: Forward
  And Backward Reaching IK* —
  https://docs.vulkan.org/tutorial/latest/Advanced_glTF/Procedural_Animation_IK/03_fabrik.html
  (algorithm: Aristidou & Lasenby, *Graphical Models* 73
  (2011), DOI 10.1016/j.gmod.2011.05.003)
- [E-5] Kira crate documentation —
  https://docs.rs/kira/latest/kira/
- [E-6] Glenn Fiedler, *Fix Your Timestep!*, Gaffer On
  Games (mirror used; the original is at
  gafferongames.com) —
  https://github-wiki-see.page/m/hwangroman/articles/wiki/Fix-Your-Timestep!
- [E-7] Buttplug server device-configuration
  documentation —
  https://docs.rs/buttplug_server_device_config/latest/buttplug_server_device_config/
- [E-8] *Dossier: Exporting procedurally-generated rigged
  characters from Rust to VRM / glTF* (VRM 1.0 spec
  summary; spec of record: the VRM Consortium's
  `vrm-c/vrm-specification`) —
  https://github.com/thejanusstream/symbios-avatar/blob/HEAD/docs/research/research-05-vrm-export.md
- [E-9] *Linux Graphics Stack Book*, ch. 241 (GIMP /
  Krita / darktable architectures; Krita primary material
  at docs.krita.org) —
  https://github.com/turbofish-pk/linux-graphics-stack-book/blob/HEAD/chapters/part-11-engine-creative-tools/ch241-gimp-krita-darktable-photo-editing.md
- [E-10] Nari Labs, *Dia* README (Dia-1.6B) —
  https://github.com/nari-labs/dia/blob/HEAD/README.md
- [E-11] Resemble AI, *Chatterbox* README —
  https://github.com/resemble-ai/chatterbox/blob/HEAD/README.md
- [E-12] hexgrad, *Kokoro-82M* model card —
  https://huggingface.co/hexgrad/Kokoro-82M
- [E-13] Wikipedia, *Subliminal stimuli* (Effectiveness
  section and its meta-analysis citations) —
  https://en.wikipedia.org/wiki/Subliminal_stimuli
- [E-14] Inox2D README (Inochi2D standard: Inochi2D
  project) —
  https://raw.githubusercontent.com/Inochi2D/inox2d/main/README.md
- [E-15] android-activity crate documentation (0.6.0) —
  https://docs.rs/crate/android-activity/0.6.0
- [E-16] Wikipedia, *Sideband* (SSB carrier suppression;
  receiver carrier reinsertion) —
  https://en.wikipedia.org/wiki/sidebands
- [E-17] Wandr, *Best Game UI Examples: What They Do
  Right* (Persona 5 section) —
  https://www.wandr.studio/blog/best-game-ui-examples
- [E-18] Michael Booth (Valve), *The AI Systems of Left
  4 Dead*, AIIDE 2009 —
  https://steamcdn-a.akamaihd.net/apps/valve/2009/ai_systems_of_l4d_mike_booth.pdf
- [E-19] Simon Clavet (Ubisoft Montréal), *Motion
  Matching and The Road to Next-Gen Animation*, GDC 2016
  (full transcript) —
  https://archive.org/stream/GDC2016Clavet/GDC2016-Clavet_djvu.txt
- [E-20] Macklin, Müller, Chentanez, *XPBD:
  Position-Based Simulation of Compliant Constrained
  Dynamics*, MiG 2016 (the brief M4 anchor) —
  https://mmacklin.com/xpbd.pdf

### 13.3 External sources — located, not opened (no claims rest on these)

- [E-21] Holden et al., *Learned Motion Matching*,
  SIGGRAPH 2020 — database-compression figures cited in
  §1.3 as REVIEW.
- [E-22] Game UI Database, Persona 5 coverage —
  www.gameuidatabase.com (named as a resource by [E-17]).
- [E-23] AgX tonemapping, sobotka/AgX —
  github.com/sobotka/AgX (the brief M6 fit target's
  provenance).

---

*End of register. Amend by commit on this branch, with the
same citation discipline; do not edit the brief's numbers
from here — where this register and the brief disagree,
the brief governs and the disagreement is a defect to
report.*
