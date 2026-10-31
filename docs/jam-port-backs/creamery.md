# Port-back — Creamery → torogao

What W8 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Validated, ready to port

1. **Sample-clock sync for audio-driven scenes** (SFW technique). The
   animation phase is DERIVED from a stream sample counter (nominal
   48 kHz; one 16 ms step = 768 samples), never from a second timeline.
   Measured tolerance in the shipped core: **zero steps of drift**,
   asserted at every step of a full demo run (`audio clock and cycle
   never drift` in the core suite). The engine research's sokol_audio
   finding — stream position as master clock — ports in translated
   form: in torogao's Bevy runtime, the audio stream position is the
   single source both rig phase and cue scheduling read. The one term
   no design removes is WebAudio/browser output latency; the estate
   criterion (+12 ms ±25 ms) stands as the shell-side calibration
   bound, not a core property.
2. **Chart-entry shape for the book of record** (SFW technique).
   `ChartEntry { cycle, preset, hits, strokes, yield_ml, full }` — an
   append-only, bounded ledger (24 entries; overflow drops are counted,
   never silently overwritten), where the session total is exactly the
   sum of the entries (tested invariant). Ports directly as torogao's
   development/exposure ledger entry: a witnessed event, the tool
   configuration it ran under, what landed, and the measured result.
3. **Transition timing table, second use** (SFW technique). W1's total
   table (900 ms/step escalation, 500 ms/step recovery, via
   intermediates) drove a differently-shaped face set (rest / warm /
   peak / glow) with zero changes — evidence the table is portable
   data for lumen-rig-core, not tuning for one game.
4. **Queued comfort changes** (SFW technique). Preset changes made
   mid-cycle QUEUE and apply at the cycle boundary; a running cycle is
   never re-judged retroactively, and comfort options are never gated
   behind progression. Ports as torogao's tool-mode change contract:
   mode changes land on beat boundaries, stated to the player in the
   HUD (`PRESET X — NEXT Y`).
5. **Hit-stop freezes the clock** (SFW technique). On a full let-down
   the juice is a 96 ms hit-stop + a 240 ms three-band flash — and the
   hit-stop freezes the sample clock with everything else. Freezing
   only the visuals while the stream runs would spend the sync the
   design exists to prove. Reduced-flash mode (border only) is an
   accessibility setting, reachable before any play.

## Language verdict (Zig, second pass)

W1 proved comptime tables for state machines; W8 proves the same
discipline for TIME. Because every cycle duration is an integer number
of steps, the sync invariant is exact integer arithmetic — the gate is
a one-line equality test, not a tolerance hunt. The costs were two
one-line strictness fixes (signed `%`, an unused parameter). Against
W1's verdict: Zig is, if anything, a stronger fit for audio-led cores
than for state machines, because the master-clock design is mostly
arithmetic and Zig makes the arithmetic honest. Zig stays in the
rotation.

## Breakage filed

See `docs/BREAKAGE-LOG.md` (clock-exactness design catch, D-221 frame
findings, seahawk/GJ-07 follow-ups, save scoping ruling).
