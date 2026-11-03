# torogao-viseme

Lip sync for Torogao, baked at authoring time: Rhubarb
Lip Sync analyses an existing recording and emits timed
mouth-shape cues; this crate converts a cue file into a
60 Hz morph-weight track over the governed Fac_ mouth
channels. The oracle runs at authoring time only — what
ships is the baked track, and the runtime never depends
on Rhubarb. (Survey row #16; adopted D-746.)

## The oracle of record

- **Tool:** Rhubarb Lip Sync, by Daniel Wolf —
  <https://github.com/DanielSWolf/rhubarb-lip-sync>
- **Version of record:** 1.14.0 (release of 2025-04-03).
  Linux release archive SHA-256:
  `a9a9074862cff47b2d59b8bf399a678a3b0b74f9452ad6ad94cb292913dd8667`.
- **Licence:** MIT. Verified from the licence file
  inside the release archive itself (`LICENSE.md`:
  "Rhubarb Lip Sync is released under the MIT License",
  copyright Daniel Wolf; the same file confirms the
  mouth-shape images are MIT — the author's Dec 2024
  clarification the survey cited). Rhubarb's own summary
  adds that lip-sync data produced by running it belongs
  to the user alone.
- **Invocation of record:**
  `rhubarb -f json --extendedShapes GHX -o cues.json <recording.wav>`
  (JSON cues with explicit end times; the full A–H + X
  shape set).

Voice generation is tabled (D-481): nothing here
synthesises speech. The oracle consumes recordings that
already exist; this crate consumes the oracle's cue
files. Audio is handled as files, never played.

## What the crate does

`viseme-bake <cue-file> <track-file> [--duration <seconds>]`

1. Parses a Rhubarb cue file (JSON, or TSV by the same
   laws — see `src/cue.rs`). Validation is total: sorted,
   non-overlapping, finite cues or a typed `TV-` failure.
   An unknown shape token is a loud error
   (`TV-CUE-UNMAPPED-SHAPE`), never a substitution.
2. Resolves each shape through the mapping table
   (below) onto Fac_ mouth channel weights.
3. Bakes `torogao-viseme-track/1`: a face-group track
   document in the motion-clip spec's shape (§4.3 scalar
   tracks; §6 provenance naming converter
   `torogao-viseme/1`, source class `converted`, and the
   cue file's SHA-256). The bake law — 60 Hz sampling,
   `round(t × 60)`, stepped keys, rest gaps, counted
   sub-frame drops — is stated normatively in
   `src/bake.rs`. Same cue file in, byte-identical track
   out.

No frame map is recorded: a cue sheet is continuous-time
data, not a frame sequence, so no frame retime happened
(motion-clip spec §6.2); the sampling law above is this
converter's named retime.

## The mapping table (`torogao-viseme-map/1`)

`data/viseme-map-1.json` — data, not code. Every Rhubarb
shape (rows) carries an explicit weight for every
governed Fac_ mouth channel (columns, the `mmd-profile/1`
vocabulary): Aa, Ee, Ii, L_Down, L_In, L_Out, L_Up, Oo,
R_Down, R_In, R_Out, R_Up, Uu. A missing row or cell is
a load-time failure, never a silent zero; the zeros in
the table are authored zeros. Retuning a weight amends
the data and bumps the map id.

Weights are authored against Rhubarb's own shape
definitions (its README's mouth-shape table):

- **X** (idle, relaxed closed) and **A** (closed with lip
  pressure, P/B/M): all zeros. The Fac_ vocabulary has no
  lip-press morph, so closure is the zero state; A and X
  remain distinct cues in the sheet even where their
  weights coincide.
- **B** (slightly open, clenched teeth; EE): Aa 0.20,
  Ee 0.30, Ii 0.10 — a narrow opening with teeth spread.
- **C** (open; EH/AE): Ee 0.50, Aa 0.40.
- **D** (wide open; AA): Aa 1.00, with L/R_Down 0.10 for
  the jaw-drop drag on the lip corners.
- **E** (slightly rounded; AO/ER — never wider than C):
  Oo 0.80, Aa 0.25, Uu 0.20.
- **F** (puckered; UW/OW/W): Uu 1.00, L/R_In 0.30
  (corners drawn in), Oo 0.15.
- **G** (upper teeth on lower lip; F/V): L/R_Down 0.45,
  L/R_In 0.25 (lower lip drawn in under the teeth),
  Aa 0.15.
- **H** (tongue raised; L — open between C and D):
  Aa 0.55, Ee 0.20. The Fac_ vocabulary has no tongue
  morph; the shape reads through its openness.

## Bounds and errors

Input bounds are named constants (`INPUT_BYTES_MAX`,
`CUES_MAX`, `TIMELINE_SECONDS_MAX`, `FRAMES_MAX`).
Failures are typed `TV-` errors (`src/error.rs`): cue
malformation, overlap, unmapped shape, invalid map,
invalid duration, I/O. Nothing panics on untrusted
input, and nothing degrades silently.
