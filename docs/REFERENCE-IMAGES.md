# Reference images — Torogao cast (Prima, Alma, Lux, Piper, Sloane, Yui)

Where the cast's images live, for sessions working in this repo. All
paths are on primo (the machine this repo is checked out on) unless a
Hugging Face dataset is named. **Images are referenced by path and
never committed to this repository.**

## 1. Design authority — solo sheets + keeper chains

`~/.local/share/pax/torogao-cast-reference/solo-sheets/`

- `sheets/` — the v1 solo sheet per girl (`sheet-<girl>.png`), plus
  v2 sheets for Alma, Sloane, and Yui (`sheet-<girl>-v2.png`), which
  supersede v1 for those three per the v2 keeper chains.
- `keeper-chain.json`, `keeper-chain-v2.json`,
  `keeper-chain-v2-sloane.json`, `keeper-chain-v2-yui.json` — the
  recorded keeper chains (path + snapshot id + SHA-256 per keeper).
- `REPORT.md`, `REPORT-V2-ALMA.md`, `REPORT-V2-SLOANE.md`,
  `REPORT-V2-YUI.md`, `SHOULD-BE-SEEN.md` — the programme reports and
  the visual benchmark statements.
- `refs/` — the style-authority references the sheets were directed
  against.

Origin: the cast-solo-sheets programme (2026-11-04) in Pax's
workspace; this is the keeper/design subset. The full programme tree
(panels, candidates, poses, workflows — ~187 MB) remains in Pax's
workspace; any part of it moves here on request. Each girl's
`docs/cast-records/<girl>.md` names her canon keeper portrait from
this set.

## 2. Generated pool per girl (HF-staged)

`~/.local/share/pax/hf-stage-torogao-filter/torogao/<girl>/`

The asset factory's generated plates and portraits per girl,
snapshot-id filenames, with `metadata.jsonl` beside the girl folders
and the by-girl manifest at
`~/.local/share/pax/hf-stage-torogao-filter.manifest.json`.
Counts at indexing time (2026-11-10): Prima 10, Sloane 12, Lux 7,
Yui 3, Alma 2, Piper 2.

Mirrored to the Hugging Face dataset
`NiceThoughtsRanch/ntranch-game-assets` under the generated-media
backup rule (uploaded labelled *candidate* before any verdict; a
file's keeper status comes from the keeper chains in §1 and the
girls' RECORD.md files, never from its presence here).

## 3. Face anchors (TG / TG2 / TG3 re-anchor sets)

`~/.local/share/pax/keeper-art-reanchor-tg/anchors/`,
`~/.local/share/pax/keeper-art-reanchor-tg2/anchors/`,
`~/.local/share/pax/keeper-art-reanchor-tg3/anchors/`
— `PADDED-FACE-<girl>.png` per set, the anchor faces the
Torogao-gen generations were re-anchored against.

## 4. Render plates (this programme's own output)

- Rung-1 plates + evidence (~16 GB):
  `~/.local/share/pax/torogao-renderer-rung1-20261110/`
  (the verdict pack the brief cites lives under its `verdict-pack/`;
  the texts are vendored at `docs/rung1-verdict/`).
- Plates v2 (D-1224; spec vendored at `docs/PLATE-SPEC.md`):
  `~/.local/share/pax/torogao-plate-spec-20261110/`
  (per-girl scenes under `girls/`, renders and audits in-tree).

## Rules

- Cite images by their primo path (or HF dataset path) — do not copy
  image binaries into this repo.
- Keeper status is a verdict, recorded in the keeper chains and
  RECORD.md files; presence in a staging pool is not a verdict.
- MoonDeep and Strobe are different casts; their art lives in their
  own trees and is deliberately not indexed here.
