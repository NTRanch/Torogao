# CLAUDE.md — torogao renderer rung 2 worktree

You are working in the **rung-2 worktree** of Torogao's native 3D
renderer. Read this file, then `RUNG2-BRIEF.md` (same directory) —
the brief is the contract; this file is the map. Then read
`FEATURE-GAPS.md` (same directory) — the register of everything
the programme still lacks beyond the brief's M1–M7, with
researched best approaches. Required reading; it authorizes
nothing by itself.

## Mission in one paragraph

Rung 1 proved deterministic, Vulkan-native skinned rendering of one
character (Prima) against her Blender plates. Rung 2 takes
`crates/torogao-render3d` to a scene renderer for the whole estate:
the full six-girl cast at per-bone parity, multi-character two-body
scenes at the Laplace3D quality bar (Owner ruling D-196),
Prima-class scenes in the class of Personality Excretion Simulator
(JinkakuHaisetsu), deterministic soft-body / cloth / hair and
fluid / particle simulation, plate shading parity, and silent
scene-to-video export — every claim gated by measurement, every
frame reproducible from a replay hash.

## Where things live

| Thing | Path |
|---|---|
| This repo checkout (work ONLY here) | primo `~/.GH/NTRanch/torogao` |
| Branch | `pax/renderer-rung2-20261110` (base: rung-1 tip `63896dd`) |
| Renderer crate | `crates/torogao-render3d` |
| Pose evaluation (never reimplement) | `crates/torogao-motion-clip` |
| Rung-1 verdict pack | `docs/rung1-verdict/VERDICT.md` |
| Item-5 addendum (limb parity) | `docs/rung1-verdict/ADDENDUM-ITEM5.md` |
| Gap analysis (requirements R1–R8, gaps G1–G10) | `docs/GAP-ANALYSIS.md` |
| Prima gap analysis (A/B/C registers) | `docs/PRIMA-TOROGAO-GAP-ANALYSIS.md` |
| Bundle reader reports + rung-2 tools (F1) | `docs/bundle-reader/` |
| Cast records (canon, per girl) | `docs/cast-records/` |
| Arc shows (per-girl clips) | `~/.local/share/pax/torogao-cast-reference/arc-shows/<girl>/` — `<girl>.clip.json` + channels + skeleton + anim_gate + replay-proof, all six girls |
| D-1206 verdict pack (Vivienne) | `~/.local/share/pax/torogao-cast-reference/verdict-pack-d1206/` (VERDICT.md + her sheets); the per-girl working evidence is at `~/.local/share/pax/strobe-smoothing-20261110/vivienne/evidence/` |
| Item-5 analysis scripts | `~/.local/share/pax/torogao-item5-fix-20261110/` (analyze_frames.py, test_gen_clip_frames.py, residual_table.md; dump_bone_data.py under probe/) |
| Reference images (sheets, keepers, plates) | `docs/REFERENCE-IMAGES.md` |
| Estate skills cited by the brief | Installed for this session at `~/.claude/skills/` (local-tiger-style-rust, local-bundle-native-reader, local-assetripper-rung, local-fathom-game-extract, local-animation-grammar-atlas, local-hypno-technique, local-kinetic-ui, local-skill-trust) |
| Session tree state | One pre-existing uncommitted file: `crates/torogao-cli/src/registry_data.rs` — a two-line schema regen from the MCP stand-up, NOT session work. Leave it; land it only on the Owner's word. |
| Feature-gap register (D-1242) | `FEATURE-GAPS.md` (this worktree, same directory) |
| Plate spec v2 (D-1224) | `docs/PLATE-SPEC.md` |
| Cast GLBs of record | `~/.local/share/pax/nsfix-stage/fleet/<girl>/<girl>.glb` (smoothed keeper fleet, all six); rung-1's consumed Prima copy: `~/.local/share/pax/torogao-renderer-rung1-20261110/inputs-v2/prima.glb` |
| Prima design template (held title) | primo `~/.local/share/ntranch-game-re/JinkakuHaisetsu/` |

## Hard boundaries

- **Never touch the rung-1 worktree**
  (`~/.local/share/pax/torogao-renderer-rung1-wt`). Another lane is
  active there with uncommitted changes. This tree only.
- **Local commits only. No pushes, ever** — a push is a separate
  Owner authorization this brief does not carry.
- The **backend boundary is law**: everything above
  `src/backend/` is API-agnostic plain-data packets; the only
  `unsafe` in the crate lives in `src/backend/vulkan.rs`, with its
  safety obligations stated in place. New modules are
  `#![forbid(unsafe_code)]`.
- **Determinism is law (D-212)**: no wall-clock input anywhere in
  evaluation, simulation, or render. Identical evaluated state →
  bit-identical frame on the bench machine, witnessed by per-frame
  SHA-256. Fixed steps, fixed iteration counts, fixed order.
- The renderer **never evaluates clips itself** — poses arrive
  evaluated from `torogao-motion-clip`. Simulation state is
  evaluated state: it enters through the same packet discipline.

## Picking up work

1. Read `RUNG2-BRIEF.md` §2 (where rung 1 stands) and §4.
2. Pick **ONE** module (M1–M7). State its acceptance gates back,
   with numbers, before writing code.
3. Work the module; run its gates; prepare the verdict pack if the
   module has a visual gate. **You never declare an Owner PASS** —
   packs are prepared for the Owner's eye.
4. Validate every touched file through theodolite (brief §7 gives
   the full clause — cured protocol, positive control, verified
   zero) before reporting done.
5. Commit locally on this branch. Report with run artifacts cited
   for every number (D-713); anything unverifiable is marked
   REVIEW, never asserted.
6. Record skill outcomes at finish:
   `skill-trust record` (Pax-side: report outcomes in the module report; Pax logs them) for each custom skill
   materially used (brief §7).

## Style and discipline

- Tiger Style Rust — load
  `~/workspace/custom-skills/local-tiger-style-rust/SKILL.md`
  before writing or reviewing Rust.
- Tests split half validation / half adversarial, always.
- Fix failures curatively in code. Never document a failure as a
  known limitation and move on.
- No personal names in any artifact (D-489): "the Owner", studio
  names only.
- No CI. No Docker/Podman. Renders are silent files — nothing
  plays audio on primo.
- Clean up your build artifacts and intermediates at task end;
  keep deliverables, gate evidence, and anything a named follow-on
  consumes.

## If you are stuck

Stop and report the blocker with the evidence, in the shape the
brief's modules use (measured state, what was tried, what the
decision needs). Do not widen scope, do not substitute a different
substrate, and do not guess a constant that a fathom commission
(brief §5) is chartered to measure.
