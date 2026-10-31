# Port-back — Withheld → torogao

What W11 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Validated, ready to port

1. **The Governor control law as data** (SFW technique). A session is
   a `Level` row — wall, band width, rise, decay, latch — over one
   law: rise under stimulus, decay without, trip-and-latch at a FIXED
   threshold. Ports into lumen-rig-core as a reusable threshold-latch
   primitive for any ride-the-line interaction; because torogao is
   Rust, it ports without translation (the jam's stated purpose).
2. **Threshold feel numbers** (SFW technique). Release margin: letting
   go 25 milli under the wall never trips, even when a disturbance
   doubles the rise to 14/step (max overshoot 14 < margin 25).
   Re-engage hysteresis at band_lo − 30 gives a ride duty of ~55–65%
   and a calm, readable oscillation instead of chattering. Latch bleed
   25/step empties the drive in ~2 s, so a cut reads as a cut.
3. **The petition pattern** (SFW technique). An ask judged on FORM —
   state position, latch state, cooldown, cap — whose effect is
   deferred to the NEXT session's schedule (time + band), with the
   wall itself immutable. Ports as torogao's ask/petition interaction
   shape wherever a character's terms are negotiated between scenes.
4. **Counted Down presentation** (SFW technique). The term as huge
   central numerals (seconds, ceiling-rounded, pink under 15 s) plus
   a tenths ride-progress line. Denial reads off the clock, not off
   prose. Ports as a UI pattern for any timed contract.
5. **Denial bookkeeping shape** (SFW technique). Per-session stats
   (trips, latch ms added) exported at close; the shell accumulates
   the book across sessions. Ports as torogao's session-record seam:
   the sim reports, the shell remembers.

## Verdict against Yui's ladder record

Withheld carries Yui's edging/denial and orgasm-control sets. The game
proves the denial is in the CONTROL, not in description: players feel
the wall because the law is fixed, legible (gold line, band, clock),
and unforgiving in exactly one way. The begging register works as
mechanics — a mistimed ask is genuinely worthless, a well-formed one
genuinely changes the next term — which is the ladder's begging step
expressed as a system.

## Language verdict (Rust, second pass)

Zero-dependency cdylib: unsafe confined to the FFI statics, the core
proper is safe Rust and natively testable (15 tests run on the host).
Clippy `-D warnings` clean and rustfmt clean under the estate pin
(nightly-2026-09-25, resolved clippy 0.1.100). Wasm 47,992 bytes,
0.024 ms/step — the draw-list seam costs no more in Rust than in Zig.
Cost of record: the sandbox's sccache/Redis wrapper was down and had
to be overridden (breakage B-1); nothing in the language fought back.

## Breakage filed

See `docs/BREAKAGE-LOG.md` (sccache outage, HUD collision, test-vs-
latch bleed, macroquad preflight decision, seahawk/GJ-07 held).
