# Roadmap — torogao (unclassified) — v0.1 → v1.0
<!-- bloch:roadmap schema=1 kind=unclassified lane=general baseline=0.1.0 target=1.0.0 -->

How to work this roadmap (any agent): run `bloch task start R-xxx --agent <you>` before working a task, `bloch task done R-xxx` when you believe it is complete — done runs every `verify:` command and flips the task green only if all pass; there is no override. `bloch task fail R-xxx --error "..."` records a failure. Fixes learned the hard way land in `.ai/memory/lessons.md` via the inbox/promote flow and are surfaced in every HANDOFF.

## Phase 1 — Baseline truth
- [ ] R-101 Classify this repo (mcp, game, app, education, or tutorial), then regenerate this roadmap with `bloch init --kind <kind>` on a branch
  - verify: test -f .ai/roadmap.md
  - evidence:
- [ ] R-102 Build is green from a clean checkout
  - verify: cargo build
  - evidence:
- [ ] R-103 Existing tests pass at the baseline
  - verify: cargo test
  - evidence:
- [ ] R-104 Repo identity, baseline version, and this roadmap are recorded
  - verify: test -f .ai/roadmap.md
  - evidence:

## Phase 2 — Core complete
- [ ] R-201 Core functionality is complete against the repo's own spec
  - verify: cargo test
  - evidence:
- [ ] R-202 Primary workflow runs end to end
  - verify: cargo test
  - evidence:

## Phase 3 — Tests and gates
- [ ] R-301 Validation tests cover the happy path end to end (half the test split)
  - verify: cargo test
  - evidence:
- [ ] R-302 Adversarial tests cover malformed, oversized, and hostile inputs (half the test split)
  - verify: cargo test
  - evidence:

## Phase 4 — Hardening
- [ ] R-401 Error paths return typed, bounded failures
  - verify: cargo test
  - evidence:
- [ ] R-402 No unbounded work: inputs, outputs, retries, and time are all bounded
  - verify: cargo clippy -- -D warnings
  - evidence:

## Phase 5 — Docs
- [ ] R-501 README documents install, usage, and verification
  - verify: test -f README.md
  - evidence:
- [ ] R-502 CHANGELOG records every user-facing change
  - verify: test -f CHANGELOG.md
  - evidence:

## Phase 6 — Release v1
- [ ] R-601 Version is bumped to 1.0.0 and the changelog records the release
  - verify: grep -q 'version = "1.0.0"' Cargo.toml
  - evidence:
- [ ] R-602 Clean-environment install and run is verified
  - verify: cargo build
  - evidence:
- [ ] R-603 Release artefacts are built from a clean checkout and checksummed
  - verify: cargo build
  - evidence:


## Phase 7 — Jam port-backs (torogao-jam-laws)
- [x] R-701 Port-back landed: trancefall (W1 face-transition table)
  - verify: cargo test -p torogao-jam-laws face_transition
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:11:52Z
- [x] R-702 Port-back landed: idle_hands (W2 idle performer)
  - verify: cargo test -p torogao-jam-laws blink_slows
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:11:57Z
- [x] R-703 Port-back landed: handfast (W4 coupled-anchor contact)
  - verify: cargo test -p torogao-jam-laws contact_is_anchor
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:11:58Z
- [x] R-704 Port-back landed: redlight (W5 reaction sizing + attention floor)
  - verify: cargo test -p torogao-jam-laws redlight_reaction
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:00Z
- [x] R-705 Port-back landed: crescendo (W6 arousal-engine/1 tier law)
  - verify: cargo test -p torogao-jam-laws crescendo_tempos
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:01Z
- [x] R-706 Port-back landed: by_heart (W7 correction ladder)
  - verify: cargo test -p torogao-jam-laws ladder_scales
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:03Z
- [x] R-707 Port-back landed: creamery (W8 sample-clock sync + chart ledger)
  - verify: cargo test -p torogao-jam-laws sample_clock_never_drifts
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:04Z
- [x] R-708 Port-back landed: milkmaid (W9 fluid timeline + culmination)
  - verify: cargo test -p torogao-jam-laws fluid_timeline
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:05Z
- [x] R-709 Port-back landed: withheld (W11 Governor control law)
  - verify: cargo test -p torogao-jam-laws governor_trips
  - verify: cargo test -p torogao-jam-laws petitions_move
  - evidence: 2 verify commands, all exit 0 at 2026-10-31T12:12:09Z
- [x] R-710 Port-back landed: second_wave (W12 continuation + rig memory)
  - verify: cargo test -p torogao-jam-laws second_wave_continuation
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:10Z
- [x] R-711 Port-back landed: exposure (W13 development tracks + ledger)
  - verify: cargo test -p torogao-jam-laws track_law
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:12Z
- [x] R-712 Port-back landed: standard_issue (W14 book of record + T-gate)
  - verify: cargo test -p torogao-jam-laws t_gate
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:13Z
- [x] R-713 Port-back landed: blushline (W3 face-channel curves + gaze scorer)
  - verify: cargo test -p torogao-jam-laws blush_curve_stack
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:15Z
- [x] R-714 Port-back landed: build_up (fathom RE 8245833 envelope + quartermaster drill gates)
  - verify: cargo test -p torogao-jam-laws build_up
  - evidence: 1 verify commands, all exit 0 at 2026-10-31T12:12:17Z
