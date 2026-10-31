# Changelog

## 1.0.0 — 2026-10-31

- Jam port-backs landed: all 13 game filings plus the fathom RE
  8245833 build-up filing, in `docs/jam-port-backs/`.
- New crate `torogao-jam-laws` added to the workspace: 15 law
  modules (trancefall, idle_hands, by_heart, withheld, exposure,
  standard_issue, build_up, handfast, creamery, blushline,
  milkmaid, crescendo, second_wave, redlight), each a const-table /
  pure-function port of one validated jam law, 16 tests.
- Bloch roadmap Phase 7 (R-701..R-714): 14 of 14 items flipped
  green, each on its own module-test verify.
- Toolchain pin: nightly-2026-10-30 (rustc 1.101.0-nightly
  92b5b8c87 2026-10-29, clippy 0.1.101).
- Gates at the bump: `cargo test --workspace` 101 passed / 0
  failed; `cargo clippy --workspace --all-targets -- -D warnings`
  clean; `cargo fmt --check` clean; no future-incompatibility
  reports.
