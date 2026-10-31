# Port-Back — Milkmaid (v1.0.0)

Filed at the week's close, for the Torogao consolidation.

- **Verdict:** PLAYS. Against Alma's ladder record, the entry carries
  the rungs it promised: service as the scene itself, the four-method
  circuit (pinch, clamps, pump suction, cups) as switchable mechanics
  on one body, and the ladder's signature move — culmination lands
  in-body, in whichever method is active, and the record names it.
  All three rounds complete under an ordinary demo policy
  (3,942 / 6,979 / 12,382 steps against budgets of 9,375 / 11,250 /
  13,125), with the circuit requirement binding on rounds 2–3 exactly
  as the ladder's escalation asks.
- **Numbers:** 14 conformance scenarios / 30 checkpoints exact; 14
  unit tests (6 validation, 8 adversarial); 0.010 ms/step against a
  0.2 ms budget; `web/index.html` 39,470 B; previews ~92 KB per round.
- **What ports back to Torogao:**
  1. **The fluid timeline model.** Yield is a cumulative integer
     ledger sampled into a ring (every 30 steps, 56 samples) and drawn
     as the round's own curve, with growth booked as capacity, not
     yield. Torogao's presentation layer takes the model as-is: any
     accumulating quantity gets a ledger, a sampler, and a curve —
     never a bare counter.
  2. **Let-down attribution.** A culmination event carries the active
     method's id, per-method counts, and a per-method yield split;
     the chart screen is just a rendering of that record. This is the
     event shape Torogao's arousal-engine log wants: exactly-once,
     attributed, and impossible to rewrite after the verdict (the
     state freezes; the boundary semantics — a same-step switch
     captures the culmination, a late one does not — are unit-tested).
  3. **Method response tables as data.** Four methods × pressure,
     trickle, let-down share, heat, threshold, addressing — one table,
     integer milli-units, shared verbatim by the reference kernel, the
     shipped core, and the conformance vectors. Torogao tuning tables
     should ship in this shape: data first, code reads it.
  4. **The persistence split.** Settings + lifetime aggregates persist
     in the shell; a round is one sitting, held in-core (B-7). A clean
     save-model line for Torogao: durable preferences and lifetime
     records at the shell, session state in the core.
- **Language verdict (Mojo, second pass):** as a kernel-of-record
  author, Mojo carried the design well — integer arithmetic, value
  semantics, and no hidden allocation are exactly a deterministic
  kernel's wants, and the fixed-point discipline (microlitres,
  milli-units) made the JS transliteration mechanical. The cost is
  the seam itself: no wasm target and no toolchain in the estate, so
  the kernel is gated by conformance vectors from an independent
  second transcription rather than executed (B-1, stated plainly).
  The harness is the reusable part: any future Mojo week inherits
  `tools/gen-vectors.mjs` + `tools/conformance.mjs` unchanged, and the
  day a toolchain lands, the same vectors judge the real kernel.
  Recommendation: keep Mojo for weeks where the math is the game.
- **Breakage:** `docs/BREAKAGE-LOG.md` — B-1 (Mojo unexecuted), B-2
  (seahawk pass primo-bound), B-3 (GJ-07 open), B-4 (round-3 retune),
  B-5 (label overlap, fixed), B-6 (raster glyph, filed), B-7
  (persistence ruling).
- **§9 transfer filing:** the deterministic fixed-step core pattern,
  the conformance-vector seam for cross-language kernels, the
  draw-list shell architecture, and the ledger/sampler/curve timeline
  model are technique with an SFW content label and may transfer to
  the SFW lane. The game's content, method names in context, and
  Alma's ladder material are NSFW-pure and stay in this lane.
