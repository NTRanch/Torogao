# Port-back — Blushline → torogao

What W3 validated, in torogao terms. (Content labels per §9: the
technique filings below are SFW-labelled; the game's NSFW-pure design
content stays in this lane.)

## Validated, ready to port

1. **Channel response curves for the face coupling** (SFW technique).
   The blush channel as a pure function stack: a saturating cubic
   response curve `1 - (1 - x)^3` (monotone, exact endpoints, no
   transcendentals — bit-reproducible in any language), a session
   accumulator (gain 0.16 per unit stimulus, clamped), and a step
   integrator whose rise rate is 4x its fall rate (1/32 vs 1/128 per
   16 ms step) — colour arrives fast, leaves slowly. Tier coupling for
   the arousal engine's low tiers: T1 at blush >= 0.25, T2 at >= 0.55,
   thresholds inclusive at the bottom. Ports into lumen-rig-core as the
   face-channel curve set the arousal engine drives: the engine owns
   the stimulus, these curves own the face's answer. Gate evidence:
   100 conformance checks + curve monotonicity/convergence tests +
   demo playthroughs reaching T2 in all three levels.
2. **Gaze scoring under §1.7, as a reusable scorer** (SFW technique).
   Full iris-disc centre displacement normalised by disc radius;
   rule order is contractual — downcast first on normalised downward
   offset (>= 0.45), then toward at magnitude <= 0.30, else away;
   unreadable geometry returns an INVALID sentinel instead of a false
   verdict. The companion stimulus derivation (integer-only, from one
   RNG value, channel-correct by construction over 500 sampled stream
   values x 3 channels in test) ports as torogao's way to *script* a
   gaze and know its scored channel without measuring a render.
3. **Measurement-readout UI** (SFW technique). Readings reported as
   scene text in a fixed three-line report — the reading itself, the
   gaze line ("HER GAZE: DOWNCAST - SEEN, KINDLY"), the steady line —
   reads as dialogue, not HUD: in the D-221 frame inspection the report
   phase is the most characterful screen in the game. Ports as the
   Fit-Check readout pattern for any measurement-framed scene in
   torogao (appraisal, examination, calibration beats): the number is
   the line, the channel verdict is the subtext, and the report phase
   gets its own beat (~2.2 s) instead of flashing past.

## Language verdict (Mojo)

The kernel was the cheapest part of the week: one small file of pure
functions with explicit contracts, written once, read many times. The
real cost of a Mojo week is the **conformance discipline**, and it is
front-loaded: with no Mojo toolchain in the build sandbox, the golden
vectors had to be derived from the kernel's formulas by an independent
exact-arithmetic implementation (Python integers + rationals) — about
an hour of care, and the reason the harness's 100 checks mean anything.
The transliteration itself was mechanical (same operation order, same
constants, no improvements) and the harness caught nothing because the
vectors were derived honestly. Verdict: Mojo stays in the rotation for
measurement-and-model weeks. The kernel+vectors pattern costs less
than one wasm boundary, and the vector file — not the source — is the
artifact that survives the language's stdlib churn. When a toolchain
is available, the only added step is regenerating the vectors by
executing the kernel; the harness shape does not change.

## Breakage filed

See `docs/BREAKAGE-LOG.md` (B-1 Mojo unexecuted honesty record, B-2
preview text overflow found by D-221 and fixed, B-4/B-5 held legs:
seahawk pass, GJ-07).
