# NTRanch-lane style and effects plan (D-1257, D-1258)

**Status: DRAFT for gate review. Nothing here is built.** Written
2026-11-10; amended 2026-11-11 under D-1295 (planning only). Every gate number is a proposal until ratified at the
module's first gate. A plan may propose gates; it never declares a
keeper, and no Owner PASS is declared here. This file is Markdown:
not-LSP-validated.

**Why this document exists (D-1257).** The fathom and lumen plans
sit in qompassai repositories and stay at the SFW / spec register.
Planning that is specific to adult content lives here, in the
NTRanch lane, and those plans point at it. Nothing in this
document crosses back.

**Law on every line.** Adult characters only, estate-original
characters only. The bar library and the downloaded originals are
analysis material (Tier A, D-555): measurements, laws and specs
cross; assets, characters, likenesses, curves, code, text and
voice performances never do. Sources with no stated licence keep
their D-1144 quarantined track. The D-798 fold, the Crescendo
ladders, the hypnosis honesty register (disclosed technique, never
claimed effect), the seizure-risk holds (OE-1b / D-785) and the
Audio Silence Rule bind by reference. Every record a module emits
carries `origin_lane` + `content_label` + `evidence` from birth;
records born here are NSFW-labelled and never reach a qompassai
surface.

**Sibling documents.** `docs/render3d/m2/M2-GATE-PLAN.md` (this
repo); `RUNG2-BRIEF.md` modules M3–M5 and commissions F1–F3;
`FEATURE-GAPS.md`; fathom `docs/CAPABILITY-PLAN.md` §5–§6 (the
viability record and the motion-study instrument); lumen
`docs/CROSS-MEDIUM-PLAN.md` §2.4–§2.5 (freeze points, ardor).

---

## 1. The bar, as it stands on disk

`~/.local/share/pax/animation-bar-20261110/` — 16 clips in seven
family directories; all 60 fps with audio; `manifest.json` carries
codec, size, duration and SHA-256 per file. The whole library is
the bar for style, aesthetic and effects. `laplace/` is the
flagship two-body standard (D-196) and is what the two-body stress
test is judged against. `unattributed/` (176 files) is quarantine
and is not bar.

### 1.1 What has been measured — interim only

The eight-stage study instrument is designed (fathom plan §6) and
not built. What ran is the D-555 sketch, unchanged, one clip per
family: artifact
`~/.local/share/ntranch/remakes/bar-sketch-interim-20261110/sketch-measurements.json`.

| Family | Clip | Whole-file period s (r) | Camera class | Note |
|---|---|---|---|---|
| `laplace` | 1080p, 75.9 s | 0.25 (0.63) | static | at the sketch's 3-sample floor — not reliable |
| `neneg` | 3058×2160, 100.0 s | 1.417 (0.88) | shake/handheld | |
| `samart` | 778×1080, 235.0 s | 0.417 (0.63) | static | |
| `topu` | 746×1080, 260.3 s | 0.583 (0.66) | shake/handheld | |
| `marciana-unattributed` | 1478×1080, 266.6 s | 0.917 (0.66) | shake/handheld | attribution unconfirmed |
| `vicineko` | 4K, 367.1 s | none (0.24) | static | no period found |
| `anna-anon` | 1080p, 910.0 s | 0.25 (0.50) | static | compilation; at the floor |

Stage labels for these numbers: census PARTIAL, loop period REAL,
pose/flow PROXY; cuts, layers, timing chart, spring fit and hashed
records NOT RUN. These clips are multi-shot edits, so a whole-file
period mixes shots. **No style or effects claim rests on this
table.** Nobody on this lane has characterized what each family
does well; that is module B1's job, on the validated instrument.

### 1.2 Effect classes the bar is measured for

The classes are the renderer brief's own, so every measurement has
a consumer:

| Class | Measured by (instrument stage) | Consumed by |
|---|---|---|
| Body motion grammar — rhythm, easing, holds, weight shift | timing chart (S8), loop period per shot (S5) | action authoring (R1, lumen X4) |
| Two-body contact — persistence, relative sliding, penetration | flow + layers (S6, S7) on the `laplace` set | renderer M3; R2, R3 |
| Secondary motion — soft tissue, hair, cloth follow-through | spring fit (S9) | renderer M4; R5 |
| Fluids and particles — emission timing, lifetime, coverage | layers (S7), timing chart (S8) | renderer M5; R6 |
| Reaction — latency, amplitude levels, whole-body participation, face beats | timing chart (S8) + the face-state read of the grammar atlas | R4 |
| Camera — class, cut rhythm, zoom | cuts (S4), flow (S6) | scene system (M2.2) |
| Sound relation — onsets against motion beats | audio-timing track (S12) | A4, lumen A6 |
| Finish — shading, line, colour, post | **no instrument stage** | renderer plate parity — **unverified, needs a gate shape** |

---

## 2. The originals — what the estate can study

Census: `~/.local/share/ntranch/remakes/originals-census-20261110/`
(`originals-census.tsv`, `family-counts.txt`); listings only,
nothing unpacked, folder unchanged (773,125,450,642 bytes, 487
entries before and after). `~/.local/share/ntranch-game-re` is a
symlink to the same folder. Family by file-name fingerprint, first
match wins — a heuristic.

| Family | Entries |
|---|---|
| Unity Mono | 128 |
| RPG Maker MV / MZ | 71 |
| Unity IL2CPP | 60 |
| Asset packs (not games) | 58 |
| Ren'Py | 45 |
| Documents / media (not games) | 40 |
| Unknown | 35 |
| Unreal | 17 |
| GameMaker | 7 |
| Godot | 5 |
| HTML / web | 5 |
| Unreadable listing | 4 |
| Tyrano | 3 |
| Cubism model only | 3 |
| Wolf RPG | 2 |
| Flash, RGSS, Unity (backend undetermined), nested archive | 1 each |

Which rung reads which family, what is missing and what a new
rung costs: fathom plan §5.2. Not repeated here.

**The worked precedent.** Personality Excretion (design template
at `~/.local/share/ntranch-game-re/JinkakuHaisetsu/`, a held
title): its personality system was extracted clean-room through
the native IL2CPP rung (metadata v29) with stripped-tree value
recovery, and that *system specification* — states, transitions,
tuning values as measurements — drives the Prima remake design.
No asset, text or code of the title is in the estate's work.
(As stated in the commission, D-1258; not re-verified in this
session — REVIEW.) It
is the pattern every module in §3 family E follows: a system
becomes a spec with provenance; the spec is what crosses.

**What this lane wants from the originals, by kind:**

| Kind | Where it is readable today | State |
|---|---|---|
| State and reaction systems (arousal, mood, corruption, trust meters; thresholds; decay) | Unity IL2CPP (precedent); RPG Maker MV / MZ and web (source level) | proven on one title; 60 + 71 + 5 entries in reach |
| The same, on Unity Mono | needs the IL reader (fathom A2 ruling, then 2–4 sessions) | **open — 128 entries, the largest family** |
| Scene sequencing and branching structure | Ren'Py archives (structure in compiled scripts — rung missing); RPG Maker events | partly open |
| Animation as authored curves and animator graphs | Unity serialized data (both backends) | reachable; no title measured for it yet |
| Effects as authored parameters (particle systems, timelines) | Unity serialized data | reachable; compiled shaders are not — a wall to name, not to route around |
| Device-timing scripts (11 found in the Owner's download folder) | plain text | timing measurements only; rhythm sync is lumen A6 / torori |
| Audio structure and cue timing | audio census tools on fathom `main` (absent from the deployed server) | blocked on fathom A3 |

---

## 3. Modules

One agent session each. Fields as in the sibling plans.

### Family B — the bar, measured (blocked on the instrument)

**B1 — Per-family characterization.** One motion record set per
bar family (fathom S14 run under this lane's labels). Out: per
family, the measured ranges for each class in §1.2 and what could
not be measured. Gates: every number traces to a record hash; a
class with no measurement is listed, not filled in.

**B2 — Two-body reference ranges.** From the `laplace` set: the
measured ranges the stress test is judged against (contact
persistence, relative sliding, reaction latency, secondary-motion
frequency and damping). Supplies the numbers the M2 plan and
lumen's bar family leave empty by design. Gates: ranges with
sample counts; none stated without its shot list.

**B3 — Finish gate shape.** A proposal for how shading, line and
post are compared at all (the one class with no instrument).
Ends in a design note for the gate; builds nothing.

### Family E — extraction to specs (the precedent, repeated)

**E1 — Second-pass census.** Resolve the 35 unknown and 4
unreadable entries; map entries to titles so the count can be
compared with the 303-title figure. Gates: every entry has a
family or a stated reason.

**E2 — State-system specs, IL2CPP.** One title per session, by
the precedent's method. Out: a system spec (states, transitions,
constants as measurements) + lesson record. Gates: every constant
a ledger row in state `cross-checked` (fathom C2) or marked
`measured-once`; zero copied text or code in the spec — checked
by a scan against the source strings.

**E3 — State-system specs, source-level runtimes** (RPG Maker
MV / MZ, web). Same gates as E2.

**E4 — Authored animation and effects parameters, Unity.** Clip
curves, animator graphs and particle-system values as
measurements. Gates: values round-trip against a second reader
(the AssetRipper rung as oracle, beside, never linked).

**E5 — Lesson records.** The shape every extraction ends in:
technique or spec, source provenance, licence state, the three
labels, evidence hashes. Gates: a record with a missing label is
refused; a record whose evidence hash does not resolve is
refused. NSFW-labelled lessons stay on this lane.

### Family R — actions, reactions and effects (moved here from the lumen plan, same ids)

All modules here are **ntranch** lane, adult characters only, and
specify *data and gates*, not content. Content specs follow the
estate canon in their own trees.

**The seam, stated once.** Lumen supplies **authored, frozen
data**: characters (X2), actions (X4), reaction tables, proxy
shapes and material parameters. The renderer **evaluates**:
contact and IK (its M3), soft body, cloth and hair (M4), fluids
and particles (M5). Lumen never bakes simulation — simulation
state is evaluated state and enters through the renderer's
packets. If a module here needs a solver, it is on the wrong side
of the seam.

**R1 — Action library schema.** Entries: id, participants, clip
references, contact annotations, intensity level on the Crescendo
ladder, prerequisites. Gates: schema fail-closed; every entry's
clips replay to recorded hashes.

**R2 — Two-body contact annotations.** For an action, which
proxy pairs are expected in contact, when, and with what
tolerance. Out: annotation tracks. Gates: annotations agree with
the renderer's measured contact intervals once its M3 exists;
until then: internal consistency only, said plainly. The existing
`actlab-probe` contact-pair probe on lumen's local `origin/main` ref is the starting
instrument. Needs: X4.

**R3 — Collision proxy authoring.** Per-character capsule and
sphere sets **measured from the GLB**, not guessed (the brief's
own wording). Gates: proxy surface within a stated distance of
the skinned mesh at rest (proposal: ≤ 5 mm, the brief's
penetration class). Needs: X2.

**R4 — Reaction system data.** A table from stimulus (contact
pair, intensity, duration) to whole-body response channels (F6),
face targets (F2) and vocalization level (A4). Gates: every
response channel exists in the contract; deterministic lookup;
no reaction is face-only.

**R5 — Secondary-motion parameters.** Per-character soft-body,
cloth and hair parameter sets as data for the renderer's M4.
Gates: parameters inside the renderer's accepted ranges; no
constant that a fathom commission is chartered to measure is
guessed — such fields stay empty and named.

**R6 — Fluid and particle effect descriptors.** Emitter
placement, timing and material descriptors for the renderer's
M5. Same gate shape as R5.

**R7 — One end-to-end beat.** One action, two characters, one
reaction, through X4 → renderer, prepared as a verdict pack.
Needs: R1–R4, renderer M2–M3.

### Family A — audio specific to this lane (same ids as the lumen plan)

**A4 — Reaction vocalizations.** A graded, non-lexical set per
character tied to reaction levels (R-family). Gates: each file
labelled with level and duration; levels separable by a measured
feature. Lane: ntranch.

**A8 — Hypnosis-audio honesty gate.** Any session audio carries a
disclosure record of the techniques used and makes no effect
claim; seizure-risk holds apply. Gates: a session without a
disclosure record is refused. Lane: ntranch.


**Module count: 17 named ids** (B 3 · E 5 · R 7 · A 2).

---

### Families added under D-1295 (D-1283 to D-1288, D-1276) — the adult-specific half

*In one line:* the lumen plan defines the machinery — seven
channels frozen once and drawn by every medium; this section
holds what is specific to this lane: what the channels carry
here, each woman's reaction signature, the doujin devices, and
the three videos.

Mechanism, ids and gates are in lumen `docs/CROSS-MEDIUM-PLAN.md`
§2.7 and families U, C, V and module M8; they are not repeated.
Everyone named below is an adult character of the estate's own
casts (six Torogao, ten Strobe). **Canon sourcing:** every kink
attribution in this section is taken from the Owner's addendum
D-1287, which cites D-273 (Torogao sets) and the D-798 fold
(Strobe). This lane did **not** read the D-273 or D-798 records
themselves, so each row is REVIEW against those records until
someone checks it; no kink is invented here. Planning only.

**HV1 — Which channels separate bar from sub-bar (the run behind
lumen U4).** The Owner's hypothesis: the flagship and `samart`
precision comes from the eyes. Run: all seven channels' curves
across the bar library (17 clips per the inquiry: flagship
family 7, `samart` 2, and the rest; the library manifest on disk
is the count of record), against a ratified sub-bar set. Report
which channels separate, with clip counts, and say plainly if
the eyes do not. Also answers, as a by-product, whether the
library contains any multi-peak scene (needed by PG1). Blocked
on the instrument's runner and a region-channel stage. Tier A
law: measurement and grammar only.

**CH1 — Channel 4 here: nipple state, breast secondary motion,
and lactation as an event channel (D-1285).** Three parts.
(a) Nipple state — a scalar track; control: a morph pair, absent
today (build item, with lumen C8). (b) Breast secondary motion —
amplitude and phase lag against body motion; parameters are R5's;
the bar's bound is lag ≤ 17 ms at 60 fps with no fitted spring
constants, so amplitude is the measurable part. (c) Lactation —
**events**, not a curve: each event has a time, a side, a
duration and an intensity, timed to stimulation cycles. The
cycle model is drawn from the estate's own records, not
invented: the Strobe-lane lactation canon (D-798 fold), the
Torogao lactation demonstrator led by Alma (D-1055), and the
Milk Factory study's automatic cycle and harvest verb. **None of
those three records was read for this amendment** — the model is
a staged prerequisite: read them, write the cycle as a table,
then author. Render side: events drive R6 fluid descriptors (3D)
and a drawn device (DJ1). Gates: event-to-stimulation-peak
offset within tolerance, measured on frames; an event with no
stimulation cycle in the record is refused; fluid volume per
event deterministic across twin runs.

**CH2 — The threshold ladder here (D-1285 channel 6).** Named
thresholds for this lane's scenes, as beats with channel
bundles, starting from the solo-sheet vocabulary in the cast
records (BUILD frames 0–89, BUILD II 90–179, then PEAK and
AFTERGLOW). Per threshold: which channels move, order, rate.
The orgasm beat: all seven channels reach peak targets inside
one window, plus a contraction pattern on channel 5 (a short
train at a stated rate — rate **estate-authored** until HV1
measures one), then per-channel decay to afterglow values. The
flagship's measured shape is the reference for one build:
anticipation ≈ 3.5 s, rise 1.9 s, hold 6.4 s, return 2.0 s, body
cycle 2.31 Hz with humanized spacing. Gates: lumen C3's.

**PG1 — Progressive mode, Prima as first consumer (D-1286).**
Lumen C4's parameters, instantiated. Her three-stage fall mapped
onto the ladder:

| Stage | Cycles | Composure channels (gaze focus, mouth closure, voice coherence) | Arousal channels |
|---|---|---|---|
| composed | first build | held near baseline; decay starts only at the first peak | single-build shape |
| cracking | middle cycles | each afterglow returns less far (recovery fraction < 1, falling) | thresholds arrive sooner (drift), peaks grow, blush and tremor floors ratchet |
| gone | terminal regime | absent — held at zero, not spiking | held peak-band values; quiver is the resting state; eye roll held with partial return only |

Her record already carries the state names this needs
(`arousal`, `bliss`, `desire` resolve on her rig) and solid heart
pupils as a state, never baked. Her hypnosis pairing adds two
things to her signature (K1): a trance-eye state and
suggestion-triggered **step** responses. Her self-rewatch loop
(D-1193) is the same ratchet at story scale: each rewatch is one
more cycle that does not reset. Parameter sourcing: every one of
the five parameters is **estate-authored until HV1 finds a
multi-peak bar scene**; the record says which. Safety, by
reference: flashing or strobing in any trance-eye device stays
under the seizure-risk holds. Gates: lumen C4's monotonicity
gates on her rendered scene; composure channels non-increasing
across cycles; the terminal regime held for its whole interval.

**K1 — Signature tables, sixteen women (D-1287).** One parameter
set each, in lumen C5's modifier vocabulary (gain, onset shift,
envelope shape, gate, step, suppress-then-release). Summary — the
data files are the deliverable, this table is their index:

| Woman (cast) | Signature kink, per D-1287 | Eyes | Mouth / voice (ch. 2, 7) | Body (ch. 3–5) | Staging (ch. 6) |
|---|---|---|---|---|---|
| Prima (T) | overstimulation; hypnosis | trance-eye state; roll holds longer each cycle | coherence decays per cycle | tremor floor ratchets | **progressive** (PG1); steps on suggestion cues |
| Alma (T) | lactation lead; service, breeding, medical play as stimulus classes | soft, lowered | breath-led | **channel 4 drives the curve** (CH1 events lead) | builds follow milk cycles |
| Lux (T) | exhibitionism / recorded | seeks the observer | praise produces a spike of its own shape | all gains scaled by an **observed** variable | amplified when watched or recorded |
| Sloane (T) | tantric | flutter and soft focus, not a roll | breath and mantra fragments | slow full-body waves; tactile-object class has its own skin response | plateau, then successive **waves**, no spike |
| Piper (T) | protocol, punishment, orgasm control | attentive, waiting | formal address that degrades at thresholds | flinch / brace mixed in on punishment events | **gated**: channels rise only on permission events; held plateaus |
| Yui (T) | edging / denial, begging; shibari, suspension | pleading | the begging escalation is the voice curve | tremor suppressed then leaking; rope-strain and suspended-body channels | long suppressed build; disproportionate release |
| Zuzana (S) | chastity / denial | pleading | suppressed, then breaking | blush high and sustained; tremor suppressed then leaking | as Yui's staging; denial cycles feed the ratchet |
| Kaisa (S) | enslavement | gaze-down patterns | compliance speech | spikes on commands and on displayed compliance | display is itself a stimulus; baseline moves along her route |
| Vivienne (S) | pet | — | nonverbal-heavy | animalistic posture and nuzzle channels | — |
| Valentina (S) | discipline | composure held | held against protocol; cracking measured against adherence | infraction → punishment events | composure vs adherence |
| Maren (S) | lactation lead | as Alma, Strobe register | as Alma | channel 4 leads | milk cycles |
| Katya (S) | free use | relaxed | unforced | low-resistance body; **no tension build** | arousal without a tension ramp |
| Cordelia (S) | corruption | — | — | her tattoo (D-799) is a **visible channel state** | progressive mode native |
| Sadie (S) | behaviour modification | — | cue-bound | **step functions** on named trained cues | immediate, not gradual |
| Yuna (S) | shibari + sensory deprivation | covered / returning | quiet, then heightened | stillness under restraint | heightened response when senses return |
| Manon (S) | bimboification | vacant, pleasure-forward | simple and eager | inhibition channels absent | early saturation |

A dash means the addendum gives no modifier for that channel;
the cell is **to be derived from canon and marked derived**, not
left generic — a signature with an empty channel is the likeliest
way to fail the separation gate. Gates: every modifier row cites
its canon decision; lumen C6 over all pairs sharing a stimulus
class. Known near-pairs to check first, because the text gives
them almost the same words: **Alma / Maren** and **Yui / Zuzana**.
If measurement cannot separate them, the fix is in the data
(register, voice, staging differences drawn from each woman's own
canon), exactly as the addendum rules.

**K2 — Woman × stimulus matrix.** The Owner's five worked classes
against the women they apply to; other cells by the derivation
rule (character modifiers first, then the class's; marked
*derived*).

| Stimulus class | Worked for | What the class itself contributes |
|---|---|---|
| tantric / tactile object | Sloane | slow onset, wave envelope, a skin-channel response to drag |
| permission / plugs | Piper | a gate: no rise without a granted event; hold while waiting |
| chastity / denial | Zuzana, Yui | suppression with stored release; feeds PG1's ratchet |
| punishment | Piper, Valentina | a flinch / brace transient braided into the arousal channels |
| obedience / display | Kaisa (and Lux through the observed variable) | spikes on command and on being seen complying |

**K3 — Composition with the progressive mode.** Rule, stated
once: a signature may set the progressive parameters and nothing
else of that mode. Denial and overstimulation signatures
**ratchet** (positive drift, rising floors); the tantric
signature **rides waves** (no floor ratchet, growing wave
amplitude instead); corruption runs the mode at route scale
(baseline moves between scenes, not only within one). Gate: for
each composed pair, the mode's monotonicity gates hold or the
signature states why they do not apply.

**DJ1 — Doujin device catalogue, calibrated from the bar
(D-1288).** Catalogue every device in the bar library — `samart`
first, as the register's exemplar — with occurrence correlated
to measured channel values, so calibrations come from the bar,
not from taste. Lane-specific device classes beyond lumen M8's
list: heart, star and spiral pupils and highlight states (heart
pupils at mid / low gaze are already face-state canon); heart
marks and sparkle bursts at the peak band; drawn tears and drool
with their own quantity curves (tears as pleasure-overflow is
canon); milk as a drawn device driven by CH1's events; effect
glyphs. Out: a device table — class, driving channel, threshold,
parameter curve, bar evidence by motion-record hash. Gates: each
device row has a driving channel and a threshold; a device seen
in the bar with no measurable channel correlate is listed as
*uncalibrated*, not given a made-up curve. Blocked on the
instrument (layer decomposition is its stage for effects).

**DJ2 — Per-woman device priorities.** The projection of K1 into
the register: her signature selects which devices dominate.
From the addendum: denial — sweat, pleading-eye devices,
suppressed tremor lines; tantric — steam, soft wave lines, few
heart marks until late peaks; pet — posture marks standing in
for ear and tail language; Cordelia — her tattoo as a drawn
device whose state is her corruption channel; Manon —
heart-saturated early, inhibition devices absent. The other
eleven are derived from K1 and marked so. The estate's Strobe
kink sheet is the per-woman expression reference of record for
this register — **not located on this machine by this lane**
(staged prerequisite: its path). Gate: the priority table is
generated from the signature data, never hand-kept beside it.

**DJ3 — Internal (x-ray) inset.** A doujin-native device class
the bar uses heavily (`samart`, and the x-ray clips in the
`marciana` set). Planned as an **optional** inset driven by the
contact channel data of R2, with its own legibility rules: inset
size against frame, what is drawn at what contact depth, and
that it never contradicts the outer view's timing. Gates: inset
events coincide with contact events within one frame; with
contact data absent the inset cannot fire. Needs: R2; renderer
M3.

**VS1 — Strobe video (first, D-1276).**
(1) *Art sources of record:* the ten keepers' builds under primo
`~/.local/share/pax/strobe-smoothing-20261110/<name>/` (all ten
directories present: cured and textured `.blend`, `.glb`, twin
GLB, measurement JSON); the D-1206 verdict pack for Vivienne;
the repository's identity anchors (`CHARACTERS.md`) and two
portraits per woman (begin / end).
(2) *Proposed proof scene:* one woman, one continuous take,
about 25 s at 60 fps, one single build through the ladder
(CH2) to a peak with the eye roll on the bar envelope, ending
in afterglow, in her own signature (K1). Vivienne is the natural
subject only because the renderer's M2 takes her through intake
first; the choice is the Owner's. **Solo, stated plainly:** the
bar's universal stress test is two-body, and a solo scene does
not discharge it — the two-body proof is a second scene, after
the renderer's contact module (M3).
(3) *Missing stages, as build items:* cast intake of a Strobe
keeper (M2 G-gates); textured face with joint-driven eyes on her
rig (U2 — her rig's eye geometry is unexamined here); the
channel record and ladder (C1, C3, CH2); her signature data
(K1); 60 fps export gate; region-channel measurement (U3);
verdict-pack assembler.
(4) *Gate:* verdict pack against `PARITY.md` — expected-vs-seen,
a FAIL exemplar beside each item, the channel table, replay
hash. Contact rows are reported *not applicable — solo*, never
passed.

**VS2 — MoonDeep video (second).**
(1) *Art sources:* the 79-plate programme under primo
`~/.local/share/pax/moondeep-art-20261109/` — counted on disk:
pilot 10, roster 18, corruption 12, features 19, endings 11,
environments 9 = 79 PNG files; registered as candidates.
Maelis is a design proposal with no appearance canon and is not
used; the K11 fork-blocked cells stay blocked.
(2) *Proposed proof scene:* one roster heroine with settled
appearance canon, one take of about 20 s over one environment
plate: a hypnosis-induction beat (trance-eye steps under the
honesty register — disclosed technique, never claimed effect)
into a single build. Which heroine is the Owner's choice.
(3) *Staged prerequisite, plainly:* **2D plates cannot feed the
3D route.** Either the plates become a layered 2D rig (cheaper;
not the route the bar was measured on) or they serve as identity
reference for a factory character on the union skeleton
(lumen V2's recommendation), which needs an identity gate before
anything is animated. Then everything in VS1 (3).
(4) *Gate:* as VS1, plus an identity sheet — factory character
beside her plates — judged by the Owner before the scene is
authored.

**VS3 — Hands-On video (third).**
(1) *Art sources:* the title's identity is a terminal interface
and an egui front-end; its cast of record (`CAST.md`, seventeen
level records) says no art was generated and every verdict reads
*pending — Owner play*. **This lane found no finished character
art of record for the title.**
(2) *Proposed proof scene:* none yet. A scene needs a subject
with appearance canon. If the Owner wants the title's own look
— the terminal itself — as the subject, that is a different
video (interface animation, kinetic-UI lineage) and a different
bar than `PARITY.md`'s character bar; say which.
(3) *Staged prerequisite:* an art source of record, or a ruling
that the cast records' hosts borrow nothing from other titles
and are to be designed first.
(4) *Gate:* as VS1 once a subject exists.

**VS4 — Prima: the Personality Extractor scenes (Owner order of
2026-11-10; feeds Lane D queue #28, D-1180).** Scenes at the
flagship two-body set's quality bar, modelled in *structure* on
the held title Personality Excretion Simulator, performed by
Prima. Plan level only.

*Law, first.* The held title (primo
`~/.local/share/ntranch-game-re/JinkakuHaisetsu/Game`) is Tier A
study material: its scene structure, staging and progression
beats are measured and referenced; no asset, character,
likeness, curve, pose parameter, text or audio of it is a
training input or ships. Prima is the estate's own adult
character and the scenes are the estate's own. This entry was
written from the estate's existing study records only — the
gap analysis `docs/PRIMA-TOROGAO-GAP-ANALYSIS.md` (queue #28)
and what it cites; the game was not opened or run for it.

(1) *Reference structure, as the study records hold it.*
Measured (vocabulary and thresholds, from metadata): a
five-step boundary ladder shared by the monitor, the
inner-thought register and the dialogue phase mask; a counter
exhausted at 50 counted climaxes, shown as a 10-block bar;
11 stages; a climax presented as a full-screen authored event;
afterglow *simulated* (slow curl-in, long heat decay), not a
fade; voice phases Normal / Climax / Afterglow / Recover; a
face built as per-stage pose families over split channels (eye,
aperture, mouth, tongue, structural) with a 6-step blush
channel; marked scenes staged in real time on a cinematic
sequencer (no video clips in the build: VideoClip 0,
AnimationClip 84, AudioClip 403), with a letterboxed finale.
**Unknown, and stated as such by the study:** every
formula-level dynamic (native code), whether depletion is
linear, and all pose parameters — which are inadmissible in any
case.

(2) *The estate's scene design, under the desire inversion
(D-1180).* The ladder keeps its five-step readability with the
sign flipped — **Composed → Stirring → Yearning → Overwhelmed →
Whiteout** — and what gives way is her reserve, the performed
persona; Whiteout is bliss-overload. No Forced axis; desire is
the progression engine; Swoon replaces fainting as a pacing
event followed by aftercare staging, and the session may resume.
Stop and aftercare surfaces are never progression-gated. Mapping
onto this plan's machinery:

| Scene design element | Carried by |
|---|---|
| five-step ladder | CH2 threshold ladder, as beats with channel bundles |
| repeated climaxes banking toward ten veils she lowers herself | PG1 progressive mode — this is its native case |
| composed / cracking / gone | PG1's three-stage table; composure channels decay across cycles |
| per-stage performance sets (11 stages × 5 phases) | channel records (lumen C1) authored over her seven resolving rig states; heart pupils as a state at peak phases, never baked |
| simulated afterglow | per-channel decay curves to afterglow values; afterglow is the next build's floor |
| self-rewatch loop (D-1193) | a recorded session's event stream restaged identically; a rewatch is one more cycle that does not reset (K3) |
| marked scenes, veil scenes, finale | the deterministic sequencer of the gap analysis (C4), slow flicker-free transitions under the seizure-risk holds |

(3) *Proposed proof scenes — two, proof length.* (a) **One
marked scene, solo:** a single continuous take, about 40 s at
60 fps, crossing two ladder steps (Stirring → Yearning →
Overwhelmed) with two peaks in progressive mode, so the ratchet
is visible: second threshold sooner, second peak larger, second
afterglow higher than the first. (b) **A veil-lowering beat:**
about 15 s, the letterboxed set-piece form, one peak and a held
afterglow. The bar's two-body stress test is not discharged by
either; whether these scenes are solo-with-device or two-body
is the Owner's call, and the two-body form waits on the contact
module.

(4) *Missing stages, as build items.* Everything in VS1 (3),
plus: PG1's parameter sheet (all five parameters
estate-authored until HV1 finds a multi-peak bar scene); the
graded blush channel reaching the renderer (lumen C8); the
sequencer (gap analysis C4); **the presentation-route ruling
(gap analysis C5)** — the remake's own open fork between a 2D
puppet, a rendered-plate hybrid and in-engine 3D. For *video*
at this bar the route is the native renderer (this programme);
that does not settle the in-game route, and this entry does not
pretend it does. Staged prerequisite for a deeper structural
study (scene counts and beat timings per stage, which the
records do not hold): one fathom extraction session on the held
title's sequencer data — measurement only, cross-checked before
any number is used.

(5) *Gate.* As VS1: verdict pack against `PARITY.md`,
expected-versus-seen with a FAIL exemplar beside each item, the
channel table, replay hash — plus PG1's monotonicity gates
measured on the rendered scene, and a **clean-room line** in the
pack: every structural number cites the study record it came
from, and a reviewer confirms no frame, pose or sound of the
held title was an input. Diffusion image-to-video stays
reference-class only.

## 4. Pick-up order

**Unblocked today:** E1, E3, E5, R1 (schema), R3 (proxies measured
from the six cast GLBs), R4 (table schema).

**Waiting on fathom:** B1, B2 (instrument, S14); E2 beyond the
metadata versions already met; E4's cross-check (C2); the Unity
Mono half of the state-system work (A2 ruling).

**Waiting on this renderer:** R2, R5, R6 measured halves (M3–M5);
R7 (M2–M3).

**Waiting on a ruling:** A4 (voice engine route — lumen plan §5);
B3 ends in one.

**Added by D-1295 (planning only):** *text work, unblocked:* K1's
data shape and the derivation of the dashed cells from canon
(needs the D-273 and D-798 records in hand), K2, K3, CH2's
ladder, PG1's parameter sheet. *Waiting on the instrument:* HV1,
DJ1, every bar-measured parameter. *Waiting on this renderer:*
VS1 (M2 intake), DJ3 and the two-body follow-on (M3). *Staged
prerequisites named above:* the lactation cycle records (CH1),
the Strobe kink sheet (DJ2), MoonDeep's route to a rig (VS2), an
art source for Hands-On (VS3).

**First three, and why:**

1. **E5 — lesson records.** Every other module here ends in one;
   without the shape, tonight's and tomorrow's extraction has
   nowhere lawful to land.
2. **E1 — second-pass census.** Cheap, read-only, and it turns
   "487 entries" into a title list the estate can plan against.
3. **R3 — collision proxies from the cast GLBs.** Needs nothing
   new, is pure measurement, and both the M3 contact work and the
   two-body bar depend on it.

## 5. Questions for the gate

- **Canon records.** D-273, the D-798 fold, D-1055, D-799 and
  the Strobe kink sheet were not read for the D-1295 amendment;
  where are they on this machine?
- **Solo or two-body for the first video?** VS1 proposes solo
  because contact is not built; the bar's stress test is
  two-body.
- **VS4's subject form:** solo with device, or two-body?
- **Hands-On's subject** (VS3): character art to be designed, or
  the interface itself as the subject?
- **Sub-bar set for HV1**: which material?

- **Finish has no instrument** (§1.2, B3). Is expected-vs-seen by
  eye the gate for it, or is a measured comparison wanted?
- **303 titles vs 487 entries.** E1 will reconcile; is the
  303-title list on disk somewhere it should be read from?
- **Unity Mono is the largest family (128) and the one without a
  logic rung.** That raises the weight of the fathom A2 ruling.
