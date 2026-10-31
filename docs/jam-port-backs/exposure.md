# Port-back — Exposure → torogao

What W13 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Validated, ready to port

1. **The development-system at one-sitting scale** (SFW technique).
   The 24-track system's jam slice — four tracks, no decay, drift cap
   140, thresholds Stirring 40 / Forming 100 — is FELT in one sitting:
   players diversify contexts because the ledger pays for it, and
   they plan around appraisals because nothing moves without one.
   The full system can keep these ratios; the cap doing quiet work
   (overflow visibly "drifts off" at appraisal) is the piece to keep.
2. **Pending-until-witnessed as a UI law** (SFW technique). Gains are
   shown as gold "+N PENDING — UNWITNESSED" from the moment they are
   earned and only join the bar at appraisal. Track-visibility finding
   for the main game: pending must be a first-class visual state, not
   a tooltip — it is what makes the appraisal feel like an event and
   the tipping (Stirring → Forming) land as a witnessed change.
3. **The exposure ledger rule** (SFW technique). Distinct contexts in
   full, repetitions at a quarter, counted per day. It produces
   variety-seeking play with two numbers and no quest text. Ports as
   the ledger shape for any context-based progression in torogao.
4. **Attention economy shape** (SFW technique). A single meter,
   multiplicative decay between beats (×0.88/slot), and a praise
   weighting (×3/2) that applies only while a recorded context is
   warm (≤2 slots) — synergy the player can schedule around. Ports as
   the Lens Toy programme model.
5. **The written record as the close** (SFW technique). The day ends
   in a document, not a score screen: tracks, stages, goals named
   MET or SHORT. Ports as torogao's day/session closing shape.

## Verdict against Lux's ladder record

Exposure carries Lux's exhibitionism and recorded/voyeurism sets. The
double meaning holds up as design: exposure the mechanic (the ledger
of contexts) IS exposure the fantasy (being seen, in more kinds of
places, witnessed). The appraisal stub proves the main game's core
claim cheaply — development that only moves when witnessed reads as
relationship, not as a progress bar.

## Language verdict (Lua, second pass)

Lua is the right tool for ledger/track scripting: the whole system is
tables, and the core reads like the design document. The wasmoon seam
(real Lua 5.4 in wasm, source shipped readable) cost 0.238 ms per
step+draw — 67× inside the frame budget — and the identical file runs
the busted specs under PUC Lua, so the shipped artefact and the tested
artefact are the same bytes. Cost of record: the page grows to
~557 KB (VM + glue inlined), the price of a readable core.

## Breakage filed

See `docs/BREAKAGE-LOG.md` (impossible goal, demo calibration ×2,
D-221 collisions, the raster-advance audit, seahawk/GJ-07 held).


## Adaptation filed 2026-10-31 — post 11924184 (content_label: nsfw)

Style B (Live Stage / Draw-List) adaptation of the fathom RE recipe
for post 11924184 (deep-throating performance; recipe at
`~/workspace/fathom-re-11924184-20261031/RECIPE.md`): the act as a
schedulable live context in the day — cycle/depth-curve as seeded
data, the phase-locked face drawn as an eye/cheek op cluster, gains
in 6-cycle PENDING blocks paid only at appraisal, praise ×3/2 on
gaze beats while a recorded context is warm (≤2 slots). Full spec:
`docs/ADAPT-11924184.md` in this repo. NSFW — stays in this lane.
