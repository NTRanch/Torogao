# Port-back — Idle Hands → torogao

What W2 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Validated, ready to port

1. **Idle-cycle parameter set** (SFW technique). The companion idle as
   data: breath loop 3.8 s (smoothstep in/out with a hold), blink
   interval 2.4–5.2 s modulated by a state scalar (loneliness slows
   the blink rate ×(1 + loneliness)), blink close 130 ms, gaze springs
   (exponential chase, stiffness ~6/s) toward look targets with
   full-disc-centre offsets. Ports into lumen-rig-bevy as the idle
   performer's parameter block.
2. **Fidget vocabulary structure** (SFW technique). Fidgets as a table
   of {name, kind, duration} scheduled from room quiet (>6 s without
   input), with per-kind param deltas (hands orbit for coil, sleeve
   tug offset, hum notes). The six-entry vocabulary (coil rope, tug
   sleeve, glance away, hum, shift seat, touch hair) is the seed list
   for torogao companions; the "Descent trace as behaviour" pattern —
   a character's significant sequence replayed as ambient fidgets —
   ports as a design rule.
3. **Secondary-motion lag springs** (SFW technique). One exponential
   chase per channel (hair sway, sleeve L/R, rope sway) with per-
   channel stiffness (2.6–3.2/s), driven by breath + gaze. Cheap,
   stable, and it reads as life. Ports to the rig's secondary-motion
   layer verbatim in shape.
4. **Look-settle rule for interaction** (SFW technique). A look must
   be held ≥900 ms before it can be "met", and meeting it consumes
   it. Prevents target-farming in any attention-shaped interaction;
   ports to torogao's companion interaction layer.
5. **Cross-session mood bridge** (SFW technique). A two-value bridge
   (visit count, carried-away state scalar) restored at boot shifts
   the actor's starting posture/meters. Ports as torogao's companion
   memory seam: storage stays outside the sim; the sim exposes
   apply/stats exports.

## Language verdict (Odin)

Data-oriented layout suits actor sims well: one Game struct, plain
procedures, fixed tables — the whole core is auditable in one read,
and freestanding wasm32 output is clean (no imports) once a wasm-ld
is on PATH. Costs: no nested procedures, exhaustive-switch discipline
(bites early, then helps), and the test-file trap (see breakage B-2).
Verdict: Odin stays in the rotation for sim-shaped cores.

## Breakage filed

See `docs/BREAKAGE-LOG.md` (connection farming ×2, test-file collision,
glow-ring face erase, HUD/window collisions, seahawk/GJ-07 held).
