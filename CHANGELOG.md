# Changelog

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
