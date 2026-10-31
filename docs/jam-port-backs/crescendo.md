# Port-back — Crescendo → torogao

What W6 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Tier-feel findings (for the engine's tuning constants)

1. **The canon tempos play.** At 16 ms fixed steps the four tiers
   resolve to rise+hold steps of 250/250, 438/375, 688/563, 1000/750.
   The two non-integer tiers (T2 rise 7000 ms, T3 rise 11000 ms,
   T3 hold 9000 ms) quantise upward by <1 step (<16 ms, <0.25%) —
   inside any plausible feel tolerance, but the engine's tuning
   table should state durations in ms and accept the quantisation,
   not pretend frame-exactness it does not have.
2. **Edge play is the game.** Rise = hold, edge = release is a
   complete, readable verb pair: the F-03 face + gold progress bar
   + "RELEASE" prompt made the edge legible with zero tutorialising
   beyond one how-to line. Recommendation: the engine spec should
   name an edge interaction (see divergence D-2), because "hold
   duration" alone does not say what the player *does* at the edge.
3. **Session clocks sized at ~2× the perfect run** (30 s perfect →
   90 s; 50 s → 120 s; 78 s → 150 s) leave room for exactly the
   failure texture the design wants (two slips recoverable, three
   fatal) without clock pressure dominating. Keep the ratio, not
   the absolute numbers, as the tuning constant.

## Per-girl face-coupling deltas

Girls differ ONLY in draw parameters (accent palette index, blush
radius 26/20/32, gaze dy 0/+4/−3, hair palette). Findings:

- Blush radius is the strongest per-girl signal at delivery size:
  Alma's 32 reads as a different performance; Yui's 20 is subtle
  to the edge of invisible at 480 px. The rig's per-girl coupling
  table should treat blush *gain* (not base radius) as the primary
  delta, scaled by tier as here (×(0.5 + 0.18·tier)).
- Gaze dy deltas of ±4 px at full-face scale are below readability;
  if per-girl gaze deltas matter to torogao, they need to be
  ≥8 px at 960-logical scale or carried by another channel.
- Accent-index swaps (pupil colour) read instantly and cost
  nothing. Keep pupils in the girl's accent, always.

## SPEC-DIVERGENCE list (spec findings, filed per the W6 charter)

The re-implementation was from the mechanism list + canon numbers;
the Rust reference code was NOT consulted. Where the spec as
written could not be implemented as written, the resolution taken
is recorded — each is a proposed spec amendment:

- **D-1 Peak/daze durations unstated.** The spec names F-05 at
  peak and F-10 "in the daze after" but gives neither duration.
  Resolution: PEAK 3000 ms, DAZE 6000 ms (daze = 2× peak; the
  afterglow is the longer beat). Amend the spec with numbers.
- **D-2 Edge has no verb.** "F-03 at edge" + a hold duration does
  not define player behaviour or a failure mode at the edge.
  Resolution: edge accrues only while released; holding accrues
  strain, 1200 ms = slip to half rise; 3 slips fail the session.
  Amend the spec to name the edge interaction and its failure.
- **D-3 Rise decay unstated.** Whether rise progress decays when
  the stimulus is released is unspecified. Resolution: decay at
  dt/4 (slow bleed, never punitive). State it in the spec.
- **D-4 Culmination trigger point unstated.** "Exactly-once" is
  specified; *when* is not. Resolution: culmination fires on
  completion of the session's TARGET tier edge (levels are target-
  tier sessions), latched so no path re-fires it. The spec should
  name the trigger and the latch explicitly.
- **D-5 Weather mapping unstated beyond "shift at boundaries".**
  Resolution: weather index = tier−1 during rise/edge, a distinct
  peak state (4) and daze state (5). Cheap, total, testable.
- **D-6 Session failure unstated.** No clock or fail state exists
  in the spec. Resolution: per-level session clocks at ~2× the
  perfect run + the slip limit (D-2). A spec for a *game* needs
  its lose condition written down.

None of these blocked the build; all six are places a third
implementer would diverge silently. That is the week's evidence
that the spec is ~80% complete: timings and the exactly-once law
are fully specified; the interaction layer is not.

## Language verdict (GDScript / Godot)

GDScript carried the pure-core discipline without a fight: a
RefCounted class with no scene-tree access ran headless under
`godot --headless --script` on the first attempt, printed its own
28-check suite, and dumped JSON conformance vectors — the cheapest
CI citizen in the program, as the research chapter predicted.
Integer-only arithmetic made the JS port exact (242/242 samples,
zero drift). Two honest costs: (1) Godot's `JSON.stringify` sorts
dictionary keys — vector files are not byte-stable across engines
unless consumers canonicalise (the conformance harness does);
(2) GDScript has no bare blocks and no expression-level asserts
that survive export, so the suite is hand-rolled functions with
explicit quit codes — fine, but it is scaffolding every GDScript
week will rewrite. **Verdict: GDScript stays in the rotation** —
for spec-honesty weeks it is arguably the best seat in the house,
because the headless loop is minutes, not hours.

## §9 transfer filing

- Tier quantisation note, edge-verb pattern, clock-ratio rule,
  blush-gain coupling, conformance-harness shape (vectors dumped
  from the native core, canonical-key comparison): **SFW
  technique**, filed for transfer.
- Session scripts, face-code staging as shipped in this game,
  girl parameter sets: **NSFW-pure**, stay in this lane.
