# Changelog

## 0.5.0 — 2026-11-15

The first release cut from `main` since the 2026-11-01 version
correction. It publishes the programme work that has landed since
0.4.0 and re-measures the tree against the VERSION-ASSESSMENT work
order (G-01…G-12) that earns 1.0.0:

- **AUTH-G game layer (G-01, G-02):** the `game` module in
  torogao-author — all 27 manifest verbs implemented (the
  Amendment's 22 plus the Phase 2 additions), through the same
  decide → apply → op-log pipeline as the rig verbs, with the
  expression evaluator, the interpreter, and game save schemas.
- **Phase 2 foundation (G-04):** slices P2-1…P2-6 landed in the
  game module — content.ron sidecar, state lifetime, deferral
  semantics, expression table weights, ledger day-stamps,
  visual-state delta table — plus P2-7 message threads.
- **Author completion (G-10):** the rig manifest stands at 49/49
  accounted — 41 verbs implemented, 8 descoped by recorded ruling
  with each capability's home named.
- **New published crates:** torogao-vmc (VMC/OSC tracking interop
  with an ARKit blendshape channel) joins torogao-osc and
  torogao-vrm in the published set from `main`; torogao-gencond
  (the generation-conditioning seam), torogao-paint
  (deterministic raster core) and torogao-pen (pen input shim)
  are published for the first time as dependency closure for
  torogao-author and torogao-app.
- **TUI (G-09):** torogao-tui, the keyboard-only terminal
  front-end over the same decide()-gated surfaces, is in the
  tree (not a published crate).
- **Lane hygiene (D-123):** the paint programme's files carried
  a Qompass AI / Apache-2.0 OR MIT header from their source
  tree; corrected to this repo's BSD-2-Clause / Nice Thoughts
  Ranch attribution. A full-tree sweep for foreign-lane markers
  now reads zero.
- **What 1.0.0 still waits on (G-12 gate, unchanged):** the
  G-05 studied-engine importers, the G-06 remainder
  (P2-8…P2-14), the G-07 T2 interaction stages, the G-08
  recording remainders, and the Owner eye/ear verdicts the
  programme requires. 0.5.0 is the honest figure for the tree
  as it stands; the bump to 1.0.0 remains the work order's
  final item, gated on closure evidence.

## Version correction — 2026-11-01

The 2026-10-31 release below was numbered 1.0.0. An independent
version assessment (the NTRanch game-RE programme's
VERSION-ASSESSMENT.md, 2026-11-01) audited this tree against what
1.0 has to mean for Torogao — a complete game engine — and found a
solid, well-tested rig/editor core with the game layer, the
importers, and most of the 1.0 programme's stages not yet built.
The tree's own roadmap agrees: its Phases 1–6, including
"Release v1", were never checked off.

The number is therefore corrected to 0.4.0, everywhere it is
claimed: the workspace version, torogao-accel, and both content
packs. The entry below is unchanged except for two figures the
audit found wrong — the test count and the jam-laws module
count — corrected in place and marked.

1.0.0 is not cancelled; it is unearned yet. It comes back when the
assessment's work order (G-01…G-11) closes on gate evidence.

## Unreleased

- New crate `torogao-osc` added to the workspace (D-752): the
  VRChat-dialect OSC parameter surface — a bounded OSC 1.0 codec,
  a declared typed surface over /avatar/parameters/*,
  /avatar/change, /input/* and /chatbox/*, and a localhost-first
  UDP transport whose rejections are counted, never panicked on.

## 0.4.0 — 2026-10-31

- Jam port-backs landed: all 13 game filings plus the fathom RE
  8245833 build-up filing, in `docs/jam-port-backs/`.
- New crate `torogao-jam-laws` added to the workspace: 14 law
  modules (trancefall, idle_hands, by_heart, withheld, exposure,
  standard_issue, build_up, handfast, creamery, blushline,
  milkmaid, crescendo, second_wave, redlight), each a const-table /
  pure-function port of one validated jam law, 16 tests.
- Bloch roadmap Phase 7 (R-701..R-714): 14 of 14 items flipped
  green, each on its own module-test verify.
- Toolchain pin: nightly-2026-10-30 (rustc 1.101.0-nightly
  92b5b8c87 2026-10-29, clippy 0.1.101).
- Gates at the bump: `cargo test --workspace` 233 passed / 0
  failed / 1 ignored (audited figure on bcc063c, corrected
  2026-11-01; the 176+ originally recorded here predated later
  landings); torogao-accel standalone `cargo test` 45 passed / 0
  failed / 0 ignored; `cargo clippy --workspace --all-targets --
  -D warnings` clean; `cargo fmt --check` clean; no
  future-incompatibility reports.
