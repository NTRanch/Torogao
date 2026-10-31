# Port-back — By Heart → torogao

What W7 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Validated, ready to port

1. **The culmination package inside a command/response loop** (SFW
   technique). The package (torogao-culmination/1 shape) integrates
   as a closing chain of orders at 3/5 window inside an ordinary
   recall loop: the exactly-once completion is a construction
   property (a flag set on the final chain order's correct answer,
   tested under adversarial replays), not a scripted cutscene.
   Integration note for the main build: culmination belongs at the
   END of the order sequence and inherits the loop's correction
   ladder — a failed chain order retries like any other, and the
   package still completes exactly once when it completes.
2. **The Permission Plug as a state gate** (SFW technique). Plug
   orders refuse ALL answers — including the correct text — until
   the trigger phrase arms them, per order. Arming is an input with
   its own event and caption, not a menu toggle. Phrase-set finding:
   one phrase ("PLEASE. MAY I?") taught as its own card carries the
   whole mechanic; a larger phrase set would dilute, not deepen.
3. **The correction ladder** (SFW technique). Train → correct →
   retry, window tightening ×3/4 then ×1/2, three rungs fail the
   ORDER (recorded) and never the sitting. Players experience
   correction as care, not punishment — the ladder is why. Ports as
   torogao's failure-handling shape for protocol play.
4. **On-the-beat timing** (SFW technique). An answer inside the
   first half of the window is a distinct, better event (its own
   sound, its own stat). Command/response audio timing ports as:
   order-issued tick, correct tick, beat chime, correction buzz —
   four cues carry the whole loop's rhythm.
5. **Protocol recall as pure data** (SFW technique). The law is a
   table of {command, response, near-miss decoy}; the decoy (one word
   wrong) is the difficulty dial at the top level. Ports directly:
   torogao's protocol content can ship as data packs against this
   shape.

## Verdict against Piper's ladder record

By Heart carries Piper's protocol, punishment, orgasm-control, and
commands sets. The jam proves the stack composes: plug, recall,
correction, and culmination run in one loop without any piece
becoming a cutscene, and "by heart" as the flawless grade gives the
ladder record a skill expression it can point at.

## Language verdict (C++ / Unreal seam)

Freestanding C++20 is a clean core language for this seam: the whole
game is one translation unit, 380 native checks run on the host, and
the wasm is 16,216 bytes at 0.011 ms/step. Costs: the -fno-builtin
flag is load-bearing (B-1), the FFI skin needs its #if guard (B-2),
and there is no STL — by design. The UE 5.8 pairing and the seahawk
session are primo follow-ups (B-5); nothing about them is claimed
here.

## Breakage filed

See `docs/BREAKAGE-LOG.md` (strlen idiom, FFI guard, raster audit,
phantom-read note, UE/seahawk held).
