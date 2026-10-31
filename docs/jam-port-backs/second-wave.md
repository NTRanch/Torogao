# Port-back — Second Wave → torogao

What W12 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Verdict against Prima's ladder record

**Plays.** The continuation floor works as a loop, not a cutscene:
stopping at the peak is a real, priced choice (150 points, recorded,
and the Rig remembers less), and continuing is a distinct skill —
riding a target band on a reduced curve while a sensitivity meter
climbs toward a soft cap. The second scene is where the design proves
itself: residue plus a raised floor after a continuation makes the
Rig's memory *visible in the first seconds* of scene 2 (channels open
at 260–420 milli instead of 0), which is exactly the "persistence a
player can notice" the week was asked to validate.

## Continuation-curve findings (numbers)

- Reduced curve of record: rise ×0.55 (floored), decay ×0.60 (floored)
  at 16 ms steps. An earlier ×1.40 decay draft made the continuation
  mathematically unsustainable under any hold/release duty cycle —
  depth bled out and every scene faded; the shipped curve sustains a
  ~50% duty cycle at the sensitivity ceiling indefinitely.
- Sensitivity: seeded at 300 milli on entry (the second wave starts
  already tender), +5/step held, −3/step released, cap 1000 (1250
  with the assist setting). Time-in-band to close: 4/5/6 s cumulative
  at targets 650/720/780 across the three levels.
- Memory arithmetic: scene-2 start = end-value × 40% + 90 floor
  (continuation success), × 25% + 40 (settled), × 15% + 0 (stopped),
  × 10% (faded). The stopped path's missing floor is the STOP cost
  made mechanical, not just numeric.
- Phase grammar check against the 5447477 recipe: build → peak in the
  72–86% band of a scene's depth budget, then a held, face-led
  continuation — the solo form of the recipe's after-phase.

## Validated, ready to port

1. **Three-phase continuation state machine** (SFW technique). Build /
   Peak (frozen values, timed decision window) / Continuation phases
   with exactly-one-continuation enforced structurally. Ports into
   lumen-rig-core as the arousal engine's post-peak state.
2. **Reduced-curve continuation arithmetic** (SFW technique). The
   ×0.55 rise / ×0.60 decay pair plus the sensitivity meter (+5/−3,
   seeded 300, soft cap with a settle outcome instead of a fail).
3. **Rig memory as residue + floor** (SFW technique). Cross-scene
   persistence expressed as per-channel residue percentages with a
   floor that depends on HOW the previous scene ended — outcome-typed
   memory, portable to any multi-scene rig.
4. **STOP as a priced choice** (SFW technique). Ending early is always
   available, always recorded, and its cost is partly mechanical
   (weaker memory) rather than only a score deduction.
5. **Trance-Deep weather coupling** (SFW technique). Ambience as a
   pure function of (depth, phase) — rain density rises with depth,
   mist bands arrive with continuation. Same shape as W1's coupling,
   independently re-derived; the two implementations agree.

## Language verdict (C# / Unity, second pass)

C# carried the semantics cleanly: integer milli arithmetic made the
C#↔JS conformance story exact (903 trace points, zero drift), and the
pure-core/console-runner split kept the whole gate runnable without
Unity. What it fought: nothing in the language — the week's friction
was the delivery seam, not C#. Unity itself was never reached tonight
(primo tunnel down; B-1): the WebGL packaging leg, and with it the
URP/Cubism findings from the research chapter, remain unverified
against a real build. Verdict: C# stays in the rotation for
systems-shaped cores; the Unity leg is a primo follow-up, not a
sandbox claim.

## Breakage filed

See `docs/BREAKAGE-LOG.md` (Unity-on-primo deferral, peak-collapse
design fix, reduced-decay rebalance, raster cursor glyph).
