# Roadmap — Torogao (app) — v0.1 → v1.0
<!-- bloch:roadmap schema=1 kind=app lane=general baseline=0.1.0 target=1.0.0 -->

How to work this roadmap (any agent): run `bloch task start R-xxx --agent <you>` before working a task, `bloch task done R-xxx` when you believe it is complete — done runs every `verify:` command and flips the task green only if all pass; there is no override. `bloch task fail R-xxx --error "..."` records a failure. Fixes learned the hard way land in `.ai/memory/lessons.md` via the inbox/promote flow and are surfaced in every HANDOFF.

## What 1.0 means here (curated 2026-10-27 — bloch fleet rollout)

1.0 = the Rust port complete: phases RP0–RP5 of PORT-PLAN.md
each passed at their gates — workspace and gate wiring, CLI,
engine (model + evaluation; physics + render), the Nerd-mode
app, and the Persona presentation — so Torogao is 100% Rust and
the frozen D tree stands as reference only. P6 feature items
proceed under their own gates after 1.0.

## Phase 1 — Baseline truth
- [ ] R-101 Build is green from a clean checkout
  - verify: cargo build
  - evidence:
- [ ] R-102 Existing tests pass at the baseline
  - verify: cargo test
  - evidence:
- [ ] R-103 Repo identity, baseline version, and this roadmap are recorded
  - verify: test -f .ai/roadmap.md
  - evidence:

## Phase 2 — Core complete
- [ ] R-201 Port phases RP0–RP5 are complete, each at its PORT-PLAN gate (CLI, engine, Nerd mode, Persona presentation)
  - verify: cargo test
  - evidence:
- [ ] R-202 Persistence and data migration round-trip without loss
  - verify: cargo test
  - evidence:
- [ ] R-203 Error and offline states are handled explicitly
  - verify: cargo test
  - evidence:

## Phase 3 — Tests and gates
- [ ] R-301 Validation tests cover the happy path end to end (half the test split)
  - verify: cargo test
  - evidence:
- [ ] R-302 Adversarial tests cover malformed, oversized, and hostile inputs (half the test split)
  - verify: cargo test
  - evidence:
- [ ] R-303 Port parity is proven against the frozen D reference implementation (fixture corpus, golden images)
  - verify: cargo test
  - evidence:

## Phase 4 — Hardening
- [ ] R-401 Install and update path is verified from a clean environment
  - verify: cargo build
  - evidence:
- [ ] R-402 Data-safety review is completed and recorded
  - verify: cargo test
  - evidence:
- [ ] R-403 Platform builds produce installable artefacts
  - verify: cargo build
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


## Phase 7 — Rig export bridge (BIND-1)
- [x] R-604 BIND-1 bridge: torogao rigexport emits INP node translation bindings as lumen part translation_keyforms (build_part_translation via the export walk delta_to_world); Aka drop report 193 -> 98, 57/100 parts carry lattices; brow 3-axis bindings remain dropped under the 2-axis lattice law
  - verify: cargo test --workspace
  - evidence: 1 verify commands, all exit 0 at 2026-10-30T08:22:06Z
