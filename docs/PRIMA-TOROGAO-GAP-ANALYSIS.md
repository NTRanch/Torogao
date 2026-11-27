# Prima remake — Torogao gap analysis (QUEUE #28)

Research rung, 2026-11-10. Question: what must be **added**
to Torogao to build the Prima remake of Personality
Excretion Simulator (Burarakku, v1.0) as a clean build —
brainwashing/corruption under the Owner's desire
inversion. No code was written; this is the census of
needs against the engine as it stands.

**Sources (every claim traces to one of these):**

- Intake census: `games/personality-excretion-intake.md`
  (S0/S1 — Unity 2022.3.22f1 IL2CPP, metadata v29; bundle
  census 12,315 objects: AudioClip 403, AnimationClip 84,
  Mesh 109, VideoClip **0**).
- Mechanics/animation study: `games/personality-excretion-study.md`
  (S2b — vocabulary and thresholds MEASURED from metadata;
  all formula-level dynamics UNKNOWN, native code).
- Design sketch: `games/prima-excretion-design.md`
  (DESIGN PROPOSAL under D-1180; crate label `torogao-prima`).
- Prima canon: `~/workspace/torogao-models-20261104/prima/RECORD.md`
  (54 bones; Basis + 36 Fac_ morphs; seven states resolve —
  neutral / desire / longing / arousal / trance / bliss /
  overwhelmed; bust canon m* = 1.50; D-1 jiggle cages +
  seven fluid anchors landed).
- Owner rulings: DECISIONS.md **D-1180** (desire
  inversion; no Forced axis; desire is the progression
  engine) and **D-1193** (Prima's recording register =
  self-rewatch brainwashing loop, distinct from Lux's
  audience-display RECORDED register).
- Torogao census: primo clone `~/.GH/NTRanch/torogao` @
  main `6ffbffb` (workspace Cargo.toml, 24 members) plus
  the gate-crates stage-2 worktree @ `7d4d13f`
  (`torogao-gate` + `torogao-gate-cli`), and the S4
  worktrees for `torogao-fall`
  (`~/.local/share/pax/fall-s4-20261109/wt`) and
  `torogao-hypnocard`
  (`~/.local/share/pax/hypnocard-s4-20261109/wt`), which
  are **not** members of the clone's workspace.

**S2c discipline:** this title is S2c EXCLUDED,
fail-closed (QUEUE.md row 28; intake §S2c). Everything
below is structure, dynamics, and vocabulary. The remake
takes no roster, no text, no pose parameters, no curves,
and not the original's character. All shipped content is
estate-original English (D-472).

**Grades:** HAVE = a landed crate/module covers the need
today, cited. PARTIAL = a pattern or adjacent system
exists but the need itself is unbuilt. MISSING = nothing
in the tree does this. REVIEW = could not be verified
from the artifacts read.

---

## (a) NEEDS — what the remake requires, system by system

Each need is traced to the study's measured grammar
(study §) or the design sketch (design §).

### A1. Stimulation simulation (the core verb)
Three lanes {Anal, External, Vaginal} running concurrently,
each with intensity {Weak, Normal, Strong} and a target
zone {Clitoris, Nipple, Vagina, Anal} refined to
inside/outside variants; lane status machine {Unselected,
Prepared, Playing, Paused}; selection changes are typed
events {NewSelection, SwitchWithinLane, IntensityChange}
(study §2, MEASURED enums). Devices attach to lanes as
data, not classes (study §2: a few mechanism-family
controllers beside generic rule/swap machinery). Stimulus
changes must emit **reflex events** {FirstAttack,
IntensityUp, IntensityDown, CommandChange} that the body
answers (study §2, MEASURED).

### A2. Tempo engine with audience coupling
A gauge that integrates lane inputs at reduced weight for
additional lanes (template constant shape: additional-lane
weight 0.6), chained peaks inside a short gap window,
tease phases with real budgets (8 s / 14 s class beats;
teasing that only starts deep into the climax count), a
short-peak regime from a mid-campaign stage, a deep-mode
second wave, and an afterglow gauge with a cap (study §2,
constants MEASURED; the formula itself UNKNOWN and — by
design §8 — **not inherited**: the remake tunes its own).
Critically, **viewer attention is a tempo input**, not
decoration (study §6: viewer-boost constants inside the
tempo set, MEASURED coupling).

### A3. The inverted monitor ladder
The original renders its boundary as a five-step ladder
{Composure, Wavering, Limit, Breakdown, Whiteout} shared
by three vocabularies — monitor mode, inner-thought
phase, dialogue phase mask (study §3, MEASURED). Under
the inversion (D-1180; design §3) the remake needs the
same five-step *readability* with the sign flipped:
**Composed → Stirring → Yearning → Overwhelmed →
Whiteout**, derived from craving + willingness bands.
What "crumbles" is her **reserve** — the performed
persona — not her ego. Whiteout keeps the name and
changes meaning: bliss-overload, not breakdown. The
side states change too: Escape is deleted outright;
Faint is replaced by **Swoon** (overstimulation overflow
→ aftercare staging → session may resume; a pacing
event, never a punishment). The ladder must drive three
consumers, as in the original: the HUD/monitor read, her
inner-thought register, and the phase mask gating her
lines and the audience's comments.

### A4. Ego-boundary counters, re-derived
Original spine, MEASURED: climax count drives a boundary
that is fully exhausted at **50** counted climaxes
(`EgoBoundaryFullOrgasmCount = 50`) presented as a
**10-block** bar (`EgoBoundaryBlockCount = 10`); whether
depletion is linear is UNKNOWN (a per-stage erosion
table exists; its values are init data, study §3/§9).
The remake re-derives this as (design §3/§5):
`surrender` 0–1000 filling by counted climaxes across the
campaign, rendered as **ten veils she lowers herself**;
per-veil climax costs are the remake's own RON tuning.
Stage progression (11 stages, the studied shape) is
gated by surrender thresholds — she advances when she
has given enough of herself away, never by "winning"
against her.

### A5. Climax beats, taxonomy, recovery/afterglow
Climax is a full-screen authored event, and afterglow is
*simulated*, not a fade (study §7: dedicated screen-FX
system + stage table; body-afterglow with a slow curl-in
and a long heat decay — constants MEASURED as template
shape). The remake needs: a climax taxonomy of its own
(the original's kind set {Normal, Intense, Faint, Oho}
is template shape only — Faint has no place under the
inversion; design §7 grades by phase), voice phases
{Normal, Climax, Afterglow, Recover} as a state machine,
and a post-climax recovery sequence (the original's
recovery phase machine, study §3) re-derived as
afterglow → sign-off staging.

### A6. Character events — the inverted set
The original interrupts the loop with Deal / Escape /
Faint / Resistance, and progression can be *regressed* by
the character's own event choices (study §3, MEASURED
vocabulary). The remake's event scheduler needs exactly
three (design §3): **Ask** (she requests a device, lane,
intensity, or a pause to be looked at — answering an Ask
is the highest-value play), **Bargain** (she offers a
trade — a veil, a marked scene, a new device — against a
condition: heat target, sustained tempo, stage
objective; the original's Deal with polarity reversed),
**Swoon** (A3). No fourth event; there is nothing to
escape from. There is no regression-by-resistance:
progression regresses only by the broadcast ending
before she got what she came for (design §2).

### A7. Session/stage loop and modes
One broadcast = one session with five beats (design §4):
cold open (real-time cinematic; her session Ask stated
as the objective) → tease phase (willingness climbs,
audience gathers) → the show (free stimulation play,
events interrupt, climaxes bank surrender) → marked
scenes at stage thresholds (real-time cinematic) →
sign-off + afterglow (session tally, unlocks granted;
surrender persists across broadcasts, willingness resets
per session). Modes beside the campaign: **Free Play**
(sandbox over unlocked content) and **Recall** (gallery
replay of marked scenes — real-time restaging, since no
video exists; study §8). Achievements/unlock tracking
per device, ending, event, and scene (study §8
vocabulary shape; unlock *rules* are the remake's own).

### A8. Cinematic staging (SLATE-equivalent)
The original's marked scenes are real-time sequences on
a cinematic sequencer (SLATE, 286 types in its image;
VideoClip = 0, intake asset census) plus a hand-rolled
title cinema. The remake needs a deterministic
equivalent: cold opens, marked scenes, veil-lowering
scenes, the finale staging (the original frames its
finale as a letterboxed set-piece — `ExcretionLetterboxHeight`
class constants, study §5), and sign-off/aftercare —
all performed live by the engine, replay-identical, with
**OE-1b discipline**: slow, flicker-free transitions
(design §7; the studied build's hard cuts do not carry
over). The D-1193 rewatch loop (A10) replays through
this same staging machinery.

### A9. Broadcast layer + reactive comments
The fiction frame is a broadcast: camera selector,
screen colour modes, sponsor-jingle classes, a broadcast
runtime state set (study §6, MEASURED vocabulary). The
remake needs (design §6): a stream overlay (viewer
count, heat, comment rail, camera selector); an
**audience model** (count + heat) feeding tempo (A2);
and a **reactive comment engine** keyed to play events —
triggers {AttackStart, AttackSwitch, IntensityUp,
IntensityDown, MultiAttackStart, Peak, PostPeak,
StageTransition, BodyContext, Ambient} gated by the
phase mask (A3) — with estate-original lines and an
authored cast of regulars whose register is adoration,
not jeering (the original's hostile-audience register
does not carry over). Beside it, **her own line engine**
with the studied kind split {Spoken, InnerThought,
AsWritten} on the same trigger/mask grammar — under the
inversion, InnerThought is a confession register: the
unguarded self surfacing (design §2).

### A10. The self-rewatch conditioning loop (D-1193)
Ruled, not proposed: Prima is recorded as a form of
brainwashing — **she rewatches herself, and the
rewatching continues the conditioning that corrupts
her, as she wants.** Mechanically the remake needs:
(a) session recording — a marked session's full event
stream captured so it can be restaged identically;
(b) a rewatch mode (the Recall machinery of A7 aimed at
her own broadcasts, framed diegetically as her
recordings); (c) a **conditioning effect on rewatch** —
watching herself lowers the next veil's cost / primes
craving / advances surrender, tuned in RON — i.e.
rewatch is a progression verb, not a passive gallery.
This loop is what distinguishes her register from Lux's:
Lux's RECORDED is display to an audience; Prima's is a
mirror she corrupts herself with.

### A11. Device roster system
Data-driven roster (study §2): rows in one RON table,
each row = lane, zone, intensity-curve family, reflex
profile, swap/clearance class (the original runs a
device–body clearance system; the remake needs at
minimum a fit/compatibility rule per row). The remake's
roster is **estate-original design** in function classes
(design §8: surface vibrators, piston/insertable class,
suction class, attachment class — clip/petal/pump —
wearable class). Size/content is S5 design work (design
§9 gate 4). **The original's 24-device roster is
UNKNOWN** (bundle MonoBehaviour/ScriptableObject
payloads; fathom gap G5, study §9.1) — and is *not* a
build input: the remake neither waits for it nor may
use it (S2c). See (e).

### A12. Presentation of Prima herself
Per-stage **performance sets**: the original's face is a
per-stage procedural pose system (per-stage pose
families, channel splits {eye, aperture, mouth, tongue,
structural}, a 6-step blush channel — architecture
MEASURED, pose parameters UNKNOWN and inadmissible,
study §7). The remake authors its own: for each of 11
stages × 5 phases, morph/pose choreography over Prima's
rig states (her record resolves exactly the seven states
the ladder needs, RECORD.md gates), a graded blush
channel, heart pupils engaging at peak phases (her
canon `Fac_Pupil_Heart` state, never baked), a
talk-mouth layer (phoneme classes {Vowel, Nasal, Stop,
Prolong, Pause, Latin} is the template shape), stimulus
reflexes as procedural secondary motion over the rig
(her D-1 jiggle cages and fluid anchors already landed,
RECORD.md D-1 section), and wet/fluid presentation
families with matched sound families (study §7 shapes).
Her existing solo-arc beat map (BUILD / BUILD II / PEAK
/ AFTERGLOW, 360 frames) is the staging template for a
session's shape (design §7). **Not a need:** the
original's diegetic mosaic companions — that is the
censored build's compliance layer (study §7); the
estate build carries no censorship system.

### A13. Audio at ~403-clip class scale
The original ships 403 AudioClip objects with payload
streamed from a 77 MB stream store (intake census).
The remake needs runtime audio of the same *class*:
graded voice utterances per phase/climax kind plus
one-shot reflexes (the original's voice-grade set is
14-member template shape, study §7), wet/device sound
families keyed to device rows (A11), jingles (A9),
session BGM — loaded, mixed, and interruptible at
runtime, on Linux **and** Android. Voice itself is
estate synthetic (Prima's locked reference, publish-
clear engines; design §7) — production is a content
programme; *playback* is an engine need. Audio silence
in the lab and the Owner's ear as ship gate stand
(design §7).

### A14. Save / progression shape
Campaign state (design §3 is the whole save-relevant
vocabulary): three meters 0–1000, climax counts
(session + campaign), veils 0–10, stage 1–11, audience
is session-scoped, unlock/seen bitsets (devices,
stages, gallery scenes, endings), settings. The
original persists broadcast progress across its
title/broadcast boundary via a progress store with a
pending-return ending kind (study §8, MEASURED shape);
its full save schema is UNKNOWN and not needed — the
remake's schema is its own, versioned, with the estate's
refusing-loader discipline.

### A15. Endings evaluation
One axis keyed to how far the counted arc ran, with
stop-outcomes beside it (study §5 shape: the original's
enum {Failed, Forced, Accepted, Climax} over a wider
outcome tree with stop members {FaintStop,
EscapeFailedStop, DealBetrayed}). The remake's evaluator
needs the same *shape* over the inverted content
(design §5, all PROPOSALS, unruled): **Unveiled** (full
arc), **Hers, Openly** (arc complete by an alternate
path), **Still Wanting** (campaign ended early — the
soft fail; replaces Failed/Forced, which do not exist),
plus session-level stop-outcomes **Swoon sign-off** and
**Her call** (she ends a session early when its Ask is
answered — a success state the original has no slot
for).

### A16. Dual target + determinism (standing law)
Linux + Android from the one crate + one RON (D-1094);
determinism per D-212: twin-run identical, cross-
platform trajectory hashes equal on shared fixtures;
tests half validation / half adversarial (PIPELINE.md
S4/S6/S6b/S7 gates). Onboarding beats (the original
teaches its systems by name through a staged tutorial
vocabulary, study §4) are a small in-shell need.

---

## (b) HAVE — what Torogao already covers

### B1. The S4 pure-crate pattern — FULL
The remake's systems shape (design §8) is the estate's
most-repeated pattern, proven four times: a pure systems
crate + one schema-versioned RON dynamics file with a
refusing loader + a deterministic smoke binary +
a trajectory/state-hash module.

- `torogao-coin` (clone `crates/torogao-coin`):
  `dynamics.rs` + `data/coin-dynamics.ron` + `session.rs`
  + `trajectory.rs` + smoke `main.rs`.
- `torogao-fall` (fall-s4 worktree, not in the clone
  workspace): `state.rs` + `protocol.rs` +
  `data/protocol.ron` + `trajectory.rs`; its `Meters`
  struct with `get(MeterKind) -> u32` is the direct
  ancestor of the design's 0–1000 integer meters.
- `torogao-hypnocard` (hypnocard worktree): `dynamics.rs`
  + `data/hypnocard-dynamics.ron` + `trajectory.rs` +
  `vocab.rs` — and, relevant to A1, a recorded-intent
  `target_part` seam in `battle.rs` (the part is recorded
  intent for a desire layer; the S4a rung left it
  discarded, which is exactly the seam the rebuild
  queue's contract 001 completes).
- `torogao-modapp` (clone `crates/torogao-modapp`): the
  same data + systems shape for a device/app fiction.

Covers the *form* of A1, A2, A4, A6, A14, A15 — the
prima crate itself is new (see C1), but nothing about
its skeleton needs inventing.

### B2. The S5 shell pattern — FULL (as pattern)
`torogao-fall/src/shell.rs` + `shell_main.rs`: an
in-crate shell module binding the S4 core to a frame
contract — 18+ gate before any protocol call, session
presentation hooks, a SURFACE control that interrupts
to an aftercare screen leaving the save byte-identical,
and a smoke binary proving the shell walks the same
states as the core (prefix hash equals the S4 fixture).
The separate `-android` crate half is proven by
`torogao-coin-android` (clone): Route 1 winit
`android.app.NativeActivity` host that runs the
canonical fixture once and reports the hash line to
logcat — spike-grade rendering, real route evidence.

### B3. SessionAudio hook idiom — PARTIAL (the hook, not the audio)
Fall's shell wires an audio hook (`SilentAudio` in the
smoke; the Surrender v2 masters slot in behind it) and
a pairing-schedule tick stream for a sensation lane
that is off by default and never required. That is the
estate's established shape for A13's *wiring* and for
any real-device seam (C9): data and hooks in the shell,
the actual sound/device layer outside the systems
crate. What sits behind the hook today is nothing —
see C3.

### B4. Puppet evaluation + graphical shell — FULL for the 2D route
`torogao-engine` (clone): the evaluation engine —
payload loading, parameter/binding evaluation,
automations, fixed-step physics, f32 arithmetic with
evaluation order preserved. `torogao-app`: the live
shell over it — winit + egui panels + a wgpu renderer
owned by the crate, two entry modes sharing every code
path except the window (windowed, and `--headless-eval`
which loads, evaluates, hashes). `torogao-rig`
independently evaluates INP1 puppets at measurement
grade. If Prima is presented as a 2D puppet, the
runtime exists end to end. (Which route the remake
takes is C5's fork.)

### B5. Motion-clip deterministic replay — FULL (evaluation), PARTIAL (presentation)
`torogao-motion-clip`: parser, validator, and layer
replay evaluator for `torogao-motion-clip/1` — skeletal
master records (bone TRS + scalar channels at 60 Hz)
with additive layers. Prima's own arc clip replays
deterministically through it: 900 frames evaluated,
replay hash `84b8c442…`, `deterministic: true`
(RECORD.md gates). This evaluates *poses*; it does not
render them — see C5.

### B6. Rig governance gates — FULL
`torogao-gate` + `torogao-gate-cli` (stage-2 branch
`7d4d13f`): GLB census, mesh selection, blink-geometric,
channel-depth, and anchors verbs over the fleet's GLBs,
with parity against the Python gates of record
(channel-depth agrees to the record's own rounding;
Prima blink-geometric occlusion 1.0 on its own scale;
anchors reproduce the Python `place_anchors` values).
Prima's GLB already reads clean through this stack
(RECORD.md: governed 10, problems []). Covers the
*measurement* side of A12's authoring gates.

### B7. Authored game model (AUTH-G) — PARTIAL
`torogao-author/src/game/`: a game-authoring layer with
a `Game` record, stat definitions (scope/kind/overflow),
flags, conditions and an expression language, effect
sets, character definitions, scene definitions, and a
message-thread family with interlude semantics (instant
batch delivery at scene entry, authored reply delay on
the step clock, thread choices through the choice
machinery), plus `save.rs`, `simulate.rs` (a read-only
interpreter), and a `content.ron` sidecar — all mutating
verbs running load → `decide()` → apply → canonical
write with an op log. This is real coverage for the
*authoring* shape of A7/A9/A14 — but it is an
editor-side, gate-mediated pipeline, and none of the
four landed game crates build on it; they are
standalone pure crates. Whether Prima authors through
AUTH-G or follows the S4 pattern is a live fork (see
(e)); the design sketch already elects the S4 pattern
(design §8).

### B8. Offline audio synthesis — FULL (authoring), and it is not playback
`torogao-cli/src/audioproc.rs`: recipe-driven synthesis
to files — frequency-glide stems with phase-continuous
glide endpoints, subliminal parameter derivation, LUFS
and true-peak measurement; `sfx/` at the clone root
carries cue/room-bed recipes. This is how estate audio
*content* is made (the Surrender masters route). It
renders nothing at runtime. Naming it here so the gap
in C3 is not mistaken for an audio absence in the
estate — the absence is specifically *runtime playback*.

### B9. Voice — the synthesis is torori's; torogao-voice is input
`torogao-voice` is a deterministic command *grammar*
(voice as an input surface over the tool registry),
not speech synthesis. Prima's voice content stands on
the estate voice programme (locked per-character
reference, publish-clear engines — design §7), which
lives outside this workspace. No addition is needed
*inside* Torogao for synthesis; playback is C3.

### B10. Save — PARTIAL (two halves, no shared game-save crate)
Editor side: `torogao-session` (canonical RON session
records; replay must reproduce the live state hash
exactly) and `torogao-app/src/save.rs` (a save document
= session evidence + transport play state, both hashes
proven equal at restore, mismatch is a typed refusal).
Game side: each S4 crate owns its save vocabulary
in-crate (Fall's `state.rs` is the worked example).
There is no shared campaign-save crate; the prima crate
follows the in-crate pattern (counted under C1, not a
separate addition).

### B11. Scene/staging precedents — PARTIAL
`torogao-cli/src/playroom.rs` + clone-root `scenes/*.json`:
scene documents (ambient beams/flicker/motes, background
plates with parallax, characters with consent flags)
hosted over engine puppets, with a fluids simulation
(`simulate_fluid`, hit areas) on the puppet route.
AUTH-G's SceneDef/ThreadDef (B7) is the second
precedent. Neither is a deterministic cinematic
sequencer for a shipped game shell — see C4.

### B12. What is *not* in the tree (negative census)
Verified by grep over the clone's crates: **no audio
playback dependency anywhere** (no rodio / kira / cpal /
symphonia in any Cargo.toml); **no gltf loader and no
3D renderer** (the app's wgpu renderer draws evaluated
2D puppet parts); **no Lovense/Buttplug reference** in
source or manifests (the D-676 device layer is ruled
doctrine with zero landed code — see C9); no comment/
audience system; no gallery/rewatch machinery beyond
the trajectory-replay pattern itself.

---

## (c) ADD — every missing piece, named

Size classes: **rung** = one S4-style increment;
**stage** = several rungs under one gate; **programme**
= cross-cutting, more than one title consumes it.

### C1. `torogao-prima` pure systems crate + `torogao-prima-dynamics/1` RON
The core. Session stepper (lanes × intensities × zones
→ craving/willingness deltas, A1), tempo engine with
audience coupling (A2), phase derivation (A3), climax
resolution + taxonomy + recovery machine (A5), event
scheduler {Ask, Bargain, Swoon} (A6), surrender/veil
accounting (A4), ending evaluation (A15), unlock/seen
bitsets and gallery registry (A7), in-crate campaign
save (A14). RON holds the device roster rows, the
11-row stage table, tempo constants, phase bands, veil
costs, comment-trigger table, rewatch tuning — loader
refuses absent values. Deterministic smoke binary +
trajectory hash module on the coin/fall pattern.
**Belongs in:** new crate in the torogao workspace (the
fall/hypnocard worktree pattern, landing into the clone
workspace at its S4 gate). **Size: stage** (the largest
single-crate S4 yet — fall + hypnocard class combined).
**Dependencies:** none in-tree beyond the pattern; gated
on C2. **Gates:** cargo test half/half, clippy/fmt,
theodolite on touched files, twin-run determinism,
fixture state-hash pinned.

### C2. S3 dynamics spec for #28 (COIN-DYNAMIC class)
The follow-on rung the study itself names (study §0):
the loop as behavior + constants at the highest grade
reachable — except the constants are the **remake's
own tuning**, derived from the design sketch over the
study's measured *shapes* (the original's formula is
UNKNOWN and is not on this path; design §8 D-1097
obligations: each relied-on measured claim becomes a
ledger obligation discharged by a fixture at S4/S7).
**Belongs in:** `games/` beside the study (programme
paper, not code). **Size: rung.** **Dependencies:**
none. **Gates:** every number cites its artifact or is
marked as remake tuning (D-713 discipline).

### C3. Runtime audio playback + mixing for Torogao shells
The estate's one true engine-level hole (B12): no crate
can play a sound. Prima needs streamed, mixed,
interruptible playback at ~403-clip class scale (A13):
voice grades, device/wet families, jingles, BGM, with
the Fall hook idiom (B3) as its control surface and
SURFACE-style instant stop. **Belongs in:** a new
workspace crate (working label `torogao-audio`) consumed
by game shells, so Fall and future titles inherit it —
not buried in the prima shell. **Size: programme**
(one crate, but it sets the audio contract for every
shell after it; Android audio output is the risk
surface). **Dependencies:** C6 (the prima shell is its
first real consumer); Fall's hook shape as the interface
precedent. **Gates:** headless render/mix determinism
where the estate can measure it (levels, LUFS via the
audioproc measurement code), Android smoke, audio-as-
files discipline in the lab (nothing plays aloud),
Owner's ear at S7.

### C4. Deterministic cinematic sequencer (SLATE-equivalent)
Beat/scene sequencer for cold opens, marked scenes,
veil-lowering scenes, finale, sign-off/aftercare (A8):
RON scene tables (beats, camera/framing classes,
letterbox, cue points for audio and performance sets),
slow flicker-free transitions by construction (OE-1b),
replay-identical from the session's recorded event
stream. **Belongs in:** the prima shell first (C6),
shaped for extraction into a shared shell-layer module
if a second title needs it. **Size: stage.**
**Dependencies:** C1 (scene triggers are state events),
C5 (it stages whatever presents Prima), C3 (cue
playback). **Gates:** replay-hash equality across runs
and platforms; transition-timing floor enforced as a
typed refusal, not a convention.

### C5. Prima's presentation route — the fork (decision + build)
Torogao today presents 2D puppets in-engine (B4) and
evaluates 3D motion clips without rendering them (B5).
Prima exists as a governed 3D rig whose states resolve
in the gate stack (B6) and as stills; the design sketch
§7 speaks of her rig states as if in-engine. Three
honest routes:
- **(i) 2D puppet Prima** — INP1 puppet of Prima through
  torogao-engine/app. Runtime exists today. Cost: a
  puppet build + per-stage performance authoring in INP
  parameter space. *REVIEW: a Prima INP1 puppet's
  existence was not verified in this census — the lumen
  programme covers the cast, but no Prima .inx artifact
  was located in the trees read.*
- **(ii) In-engine 3D** — a GLB/skinning renderer inside
  Torogao. Nothing of it exists (B12). **Size:
  programme**, and it re-opens the D-457 engine-shape
  question; not assumable inside this remake's budget.
- **iii) Motion-clip + rendered-plate hybrid** — the
  show staged from deterministic motion-clip evaluation
  driving offline-rendered plates/stills (her arc
  pipeline already renders this way: 360-frame arc
  rendered from the blend, RECORD.md). Runtime =
  plate/clip sequencer (a C4 consumer). Cost: content
  volume — every stage × phase combination that must
  move needs a rendered clip.
**Belongs in:** Owner/design decision first (see (e));
build follows the ruling. **Size: rung (i) / stage
(iii) / programme (ii).** **Dependencies:** gates C4,
C6, C7 — nothing downstream of presentation can be
gated until this is ruled. **This is the critical
path's head.**

### C6. `torogao-prima` broadcast shell (S5 shell binding)
The windowed shell over C1 + C4 + C3: stimulation UI
(three lane rails × intensity, device rail, camera
selector), stream overlay (viewer count, heat, comment
rail), veil-bar HUD, phase readout, 18+ gate, SURFACE
interrupt to aftercare, Free Play and Recall entry
points, onboarding beats. egui/winit on the
`torogao-app` precedent; the frame contract proven by a
smoke binary whose prefix hash equals the C1 fixture
(the Fall shell proof, B2). **Belongs in:** in-crate
`shell` module + `shell_main` in `torogao-prima` (the
landed S5 pattern). **Size: stage.** **Dependencies:**
C1, C3, C4, C5, C7 (comments render here), C8
(rewatch is a shell mode). **Gates:** shell/core hash
parity, headless smoke, D-221 visual doctrine at S7
(Owner's eye).

### C7. Reactive comment + her-lines engine
Seeded-deterministic line selection over the
trigger × phase-mask grammar (A9): comment table and
her-lines table in the prima RON (tables in the data
file; *content* is S5 writing, C10), commenter-persona
registry (her regulars), heat feedback from comment
events into the audience model (A2's coupling closes
here). **Belongs in:** systems in `torogao-prima`
(selection is a pure function — it belongs with the
stepper, not the shell); rendering in C6. **Size:
rung** (system) — the grammar is fully specified by the
study; the volume is content, not code.
**Dependencies:** C1 (phase + triggers are crate
events). **Gates:** fixed-seed replay hash; mask
coverage tests (every trigger × phase cell either
yields a line or a typed empty, never a panic or a
fallthrough line from the wrong phase).

### C8. Session recording + rewatch conditioning (D-1193)
Trajectory capture for marked sessions (the
trajectory.rs pattern already records input streams
against state hashes — B1), a rewatch mode that
restages a recorded broadcast through C4, and the
conditioning rule: rewatching applies surrender/craving
effects per RON tuning (A10) — the brainwashing verb.
Includes the diegetic framing surface (her recordings,
kept by veil/stage) in the shell's Recall area.
**Belongs in:** capture + effects in `torogao-prima`;
restaging in C6/C4. **Size: rung-to-stage** (the
machinery is small; the tuning and framing are design
work). **Dependencies:** C1, C4; meaningless before
both. **Gates:** recorded session replays to the
identical state hash; rewatch effects apply exactly
once per rewatch (idempotence adversarial tests —
reloading a save must not re-bank conditioning).

### C9. Real-device seam — named, NOT assumed
The original's devices are fictional in-game items
(A11). The estate's real-device layer (Lovense MCP
behind its `decide()` gate; Buttplug as the device
layer per D-676) has **zero landed code in Torogao**
(B12). Fall's pairing-tick idiom (B3) is the in-tree
precedent for how such a seam is shaped: schedule data
in the shell, off by default, never required. If — and
only if — the Owner wants any fictional device class
mapped to the real layer, it is a separate gated
addition on that idiom, with the MCP's standing safety
contract (master off by default, synchronous stop, no
resume on reconnect) inherited whole. **Size: rung**
(data seam) if commissioned. **Not on the critical
path; excluded from the build until ruled** (see (e)).

### C10. S5 content programme (the volume behind the systems)
Not code, but the build does not exist without it, and
the gap analysis that omits it lies by omission:
per-stage performance sets (11 stages × 5 phases, A12)
authored and gated through B6/B4; Prima voice set
(graded utterances per phase/climax kind, estate
synthesis, locked reference); comment + her-lines
writing (hand-written, Owner-approved per project
rules); estate-original device roster content (A11);
SFX families from the cleared library + audioproc
recipes (B8). **Size: programme** (it is the S5 stage
of PIPELINE.md, Owner-gated throughout). **Gates:**
D-221 visual doctrine, Owner's ear, per-item verdicts.

### C11. `torogao-prima-android` shell crate (S6b)
Per-title Android host on the coin-android precedent
(B2): winit NativeActivity, the full shell (not the
spike's fixture-and-fill), same crate + same RON.
**Size: rung-to-stage** (rung if C3's Android audio and
C6's shell port stay clean; stage if they don't).
**Dependencies:** C6, C3. **Gates (D-1094/D-212):**
APK builds on primo, cross-platform trajectory hashes
equal on shared fixtures, emulator/device smoke. S6 +
S6b are one deliverable — the title is not done at
Linux alone.

---

## (d) BUILD ORDER

Critical path marked ◆.

1. **C2 — S3 dynamics spec** (rung, paper). Fixes the
   remake's own numbers and the D-1097 ledger. Nothing
   else can be gated without it.
2. ◆ **C5 — presentation route ruling** (Owner/design).
   The fork gates every presentation artefact; rule it
   before C4/C6/C10 spend anything.
3. ◆ **C1 — `torogao-prima` systems crate + RON** (S4
 gate).
   The spine: stepper, tempo, phases, climax, events,
   surrender/veils, endings, save — proven by smoke +
   trajectory hash before any shell exists.
4. **C7 — comment + her-lines engine** (rung). Lands
   inside C1's crate while its event vocabulary is
   fresh; content tables stubbed with typed empties.
5. ◆ **C3 — runtime audio** (programme crate). Starts
   in parallel with C1 (it has no prima dependency —
   Fall is its interface precedent), but the prima
   shell cannot gate without it.
6. ◆ **C4 — cinematic sequencer** (stage). On C1's
   event stream, staging C5's chosen route.
7. **C8 — recording + rewatch conditioning** (rung-to-
   stage). On C1 + C4; the D-1193 register lands as a
   mechanic here, not as a skin.
8. ◆ **C6 — broadcast shell** (S5 binding). C1 + C3 +
   C4 + C7 + C8 behind the frame contract; shell/core
   hash parity is its gate.
9. **C10 — S5 content programme** (runs beside 4–8,
   Owner-gated item by item): performance sets, voice
   set, comment/line writing, roster content, SFX.
   The shell gates on stub content; the *title* gates
   on this.
10. **C11 — Android shell** (S6b). Last build step,
    first-class deliverable: S6 + S6b together, then
    S7 gates (determinism twin-run + cross-platform
    hashes, Owner eye/ear on the production-esque
    slice) and S8 release prep under lane law.
11. **C9 — real-device seam**: only if ruled, only
    after C6, never required for the title to ship.

**Critical path:** C2 → C5 ruling → C1 → C4 → C6 →
C11/S7, with C3 running parallel-but-blocking at C6.
The two places this path can stall are both decisions,
not code: the C5 route ruling and the (e) items below.

---

## (e) OPEN — Owner calls and unknowns

1. **Presentation route (C5).** The single largest
   call in the build: 2D puppet (runtime exists;
   Prima puppet existence REVIEW — unverified in this
   census), motion-clip/rendered-plate hybrid (her
   proven arc pipeline, content-volume cost), or
   in-engine 3D (programme-class, re-opens the engine
   question). The design sketch §7 is written as if
   the rig presents in-engine; the tree does not
   support that reading today.
2. **Endings — PROPOSALS, unruled.** Unveiled / Hers,
   Openly / Still Wanting, plus the Swoon sign-off and
   Her call stop-outcomes (design §5). D-1193 restates
   their status: design proposal, not ruled. The C1
   evaluator is built to the *shape* (one arc-keyed
   axis + stop-outcomes) so a ruling changes data,
   not structure — but the ruling is still owed before
   S5 content writes toward an ending.
3. **Title.** "Prima: Broadcast of Surrender" is the
   sketch's working label, not a ruled title
   (design §9 gate 3).
4. **The Lux adjacency — narrowed by D-1193, one
   residue.** D-1193 settles the recording layer:
   Prima's recordings are a self-rewatch brainwashing
   loop, distinct from Lux's audience-display register.
   The design sketch's §6 flag was written more
   broadly — it also covers the *live broadcast's
   audience* (being watched, in real time, by
   strangers) as exhibition-adjacent. The sketch's
   position (the camera as the witness her corruption
   requires under TPE, not the object of the kink)
   stands as the design's answer; whether D-1193's
   ruling closes the live-audience half of the flag
   or only the recording half is an Owner call. The
   design doc still carries the flag until he rules
   it closed.
5. **Real-device mapping (C9).** Whether any fictional
   device class maps to the estate's real-device layer
   is a design question, not an assumption. Default
   posture if unruled: no mapping; the game ships
   complete without it (Fall's pairing idiom — off by
   default, never required — is the ceiling of what
   exists today).
6. **Device roster content + veil-cost tuning.**
   Roster size/content within the design §8 function
   classes, and the per-veil climax costs, are S4/S5
   design work the sketch deliberately leaves open
   (design §9 gate 4). C2 proposes; S5 rules.
7. **AUTH-G vs S4 pattern (B7).** The design elects a
   standalone pure crate. `torogao-author`'s game layer
   exists and covers authoring shapes the S4 crates
   re-implement per title. Not a blocker — but if the
   Owner wants Prima authored through AUTH-G (scenes,
   threads, stats in the gated op pipeline), that is a
   direction call to make at C2, not after C1.
8. **UNKNOWN — the original's 24-device roster**
   (study §9.1; fathom G5: roster rows live in bundle
   MonoBehaviour/ScriptableObject payloads). A fathom
   bundle rung could extract the *structure* of those
   rows. Recorded here because the commission named
   it — with the discipline stated plainly: the remake
   does not need it, may not use its content, and does
   not wait for it. It is a study-completeness item
   only, pursued if the Owner wants the study closed
   for its own sake.
9. **UNKNOWN — formula-level dynamics** (study §9.2–
   9.4, 9.8: tempo formula, erosion table values,
   ending selection conditions, linear-vs-not block
   depletion; the Ghidra-class rung's brief, shared
   with QUEUE #10/#13/#22/#26). Same posture as item
   8: the remake tunes its own numbers under the
   inversion (design §8), so none of this is on the
   build path. It stays UNKNOWN, named, not filled.
10. **REVIEW — Prima INP1 puppet.** Route C5(i) assumes
    the lumen programme's cast coverage includes a
    Prima puppet; no `.inx` artifact for Prima was
    located in the trees this census read. Verify
    before the C5 ruling is priced.
11. **REVIEW — Free Play scope.** The original's Free
    Play unlock scope is UNKNOWN (study §8). The
    design assumes all-unlocked-content sandbox; that
    is the remake's own choice and needs no evidence —
    flagged only so the assumption is visible.

---

Validation note: this analysis is prose over the cited
artifacts — **not-LSP-validated**. Primo state was read
live on 2026-11-10 (clone HEAD `6ffbffb`, stage-2
worktree `7d4d13f`, fall worktree @ `3be1a32`, hypnocard
worktree @ `b0d1d28`); tree claims should be re-verified
against HEAD before any build rung prices them.
Bloch record for this rung: origin_lane =
`ntranch/lane-d-queue-28`, content_label = `nsfw`,
evidence = this file plus the intake, study, and design
documents it cites.
