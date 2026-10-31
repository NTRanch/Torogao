# Port-back — Trancefall → torogao

What W1 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Validated, ready to port

1. **Face transition timing table** (SFW technique). A total table over
   ordered face-state pairs: descent 900 ms per state step, recovery
   500 ms per step, same-state 0; paths that skip a state route through
   the intermediate keyframes with per-segment smoothstep easing.
   Ports directly into lumen-rig-core as the face-transition data table
   (the rig runtime's `face` module): keyframes become rig parameter
   sets, the table becomes its transition contract. Gate evidence: core
   tests 1–2 + demo playthrough visiting edge and peak.
2. **Posture+expression keyframe shape** (SFW technique). One parameter
   set carries expression (lids, gaze disc centre, mouth, blush, brows)
   AND posture (head tilt/sink, shoulder offset); interpolating the set
   as a unit is what makes the performance read as one body. Ports as
   the rig's keyframe struct shape.
3. **Weather coupling function** (SFW technique). Ambience as a pure
   function of (depth, face state): rain/star channels monotone up,
   ambient-life channels monotone down. Ports to torogao's scene
   ambience driver: environment answers the performance state, never
   an independent timeline.
4. **Counted-exhale beat grammar** (SFW technique). Breath cycle
   (inhale 2.4 s, hold 1.1 s, counted exhale of N beats, settle 1.6 s)
   with ±280 ms judge windows and culmination evaluated at cycle end.
   Ports as the torogao scene-beat timing template for induction-shaped
   scenes.
5. **Revelation ramp** (SFW technique). completeness = clamp((depth −
   0.30)/0.55); a two-label reveal (Hinted < 0.75 ≤ Glimpsed). Ports as
   a generic reveal-progress driver.

## Language verdict (Zig)

Comptime tables beat hand-written state machines for this shape: the
transition table is total by construction and testable as data; the
draw-list emitter with a bounded writer made the wasm↔JS boundary
boring, which is the point. Zig cost: two keyword collisions (`resume`)
and strict integer-division rules — both one-line fixes. Verdict: Zig
stays in the rotation for performance-shaped cores.

## Breakage filed

See `docs/BREAKAGE-LOG.md` (L3 target-vs-drift fix, rasteriser font-row
fix, seahawk/GJ-07 follow-ups).
