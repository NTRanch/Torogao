# Torogao — the Rust port plan

Torogao is a character-animation editor: 2D puppet rigs, governed
expressions, scenes, and a playroom, built as a downstream fork of
the Inochi Creator / inochi2d codebase (BSD-2-Clause; upstream
attribution in `NOTICE` when source lands). It is currently
implemented in D. This plan ports it to **100% Rust**, and the port
runs **before** the next feature phase (P6) — new features are
written once, in Rust.

Decision of record: 2026-10-26. The D build is feature-complete
through phase P5b and passes its full gate suite; it freezes as
the reference oracle for the port.

## 1. What exists (measured on the D tree, 2026-10-26)

62,993 lines of D in 226 files, in four buckets with very
different port costs:

| Bucket | Size | Nature |
|---|---|---|
| Vendored engine (inochi2d) | 39 files, 12,016 LOC | INP1 parsing, parameter/deformer evaluation, physics, OpenGL rendering. The deep work: correctness is visual. |
| Vendored editor (upstream Creator app) | ~30,000 of creator/'s 34,312 LOC | The traditional panel editor. Does **not** need a faithful port — see §3. |
| Studio code (tools + Creator additions) | ~17–20,000 LOC | The CLI, MCP + A2A servers, the 57-tool registry, scenes, the writer, tutorials, the Playroom staging, the Character Creator panel, the theme and font work. Fully specified by contracts and gates. |
| Dependencies | SDL2, OpenGL, cimgui, image loading, math, face tracking | Every one has a maintained Rust equivalent; no exotic blockers. |

## 2. What carries over for free

- **All data.** The INP1/JSON formats, scenes, packs, fixtures,
  tutorial data, and manifests are language-neutral and do not
  change. INP1 stays the format.
- **The acceptance gates.** The phase harnesses (P3: 90 checks,
  P4: 150, P5: 141, plus MCP and A2A protocol harnesses) drive
  the CLI over its wire and file interfaces and take the binary
  path as an argument. They run **unchanged** against the Rust
  binaries. The port is done when the same gates pass.
- **The specifications.** Additions A–Y are design documents, not
  D code; they govern the Rust build exactly as written.
- **The D build itself**, frozen at the P5b tip, as a differential
  oracle: same inputs to both binaries, outputs compared.

## 3. The ruling that makes the port tractable

The traditional editor UI is no longer the product's face. The
whole UI is the Persona-style stage (Addition Y); the traditional
toolbar/panel UI survives as **Nerd mode**, a toggle, default
off. So the port never transliterates ~30k lines of upstream
editor: it builds the Persona presentation (new work in any
language) plus a lean Rust panel set for Nerd mode, held to a
parity gate — every capability in the tool inventory reachable
from both presentations.

## 4. Target architecture

One cargo workspace, latest Rust (pinned in
`rust-toolchain.toml`):

- `torogao-format` — INP1 and the vendor-extension records,
  parsing and byte-stable serialization.
- `torogao-engine` — puppet model, parameter/deformer
  evaluation, physics, rendering. Grows from the estate's
  existing Rust rig-core lineage (model + validation already
  landed and gated there) rather than starting cold.
- `torogao-cli` — the tools: registry, MCP server, A2A server,
  scenes, writer, tutorials, toolbox, conversion. The harness
  gates pin its behavior.
- `torogao-app` — the GUI. Pure-Rust stack: `winit` (windows),
  `wgpu` (rendering), `egui` (immediate-mode UI, customized for
  the Persona presentation layer), `glam` (math), `image` /
  `serde` throughout. No SDL, no cimgui: "100% Rust" means the
  linked stack is Rust crates over system graphics drivers.

## 5. Port phases and gates

- **RP0 — Workspace + gate wiring.** Workspace skeleton, license
  and attribution files, the harness suite parameterized on the
  Rust binary paths. Gate: harnesses run (and fail honestly)
  against the skeleton.
- **RP1 — CLI port.** Gate: P3 90/90, P4 150/150, P5 141/141,
  MCP PASS, A2A PASS against the Rust CLI; the registry's 57
  tools enumerate identically (tools/list diff empty);
  differential run vs. the D oracle on the fixture corpus,
  outputs byte-identical where the format promises byte
  stability.
- **RP2 — Engine: model + evaluation.** Gate: fixture rigs
  evaluate to the D engine's parameter outputs within the
  recorded tolerances; deterministic replay byte-stable ×3.
- **RP3 — Engine: physics + render.** Gate: golden-image
  fixtures match the D renders within tolerance and pass the
  human eye gate; the 8K texture target holds.
- **RP4 — App: Nerd mode.** Lean panel set over the ported
  machinery, theme applied. Gate: parity inventory — every
  registry capability reachable; write-location audit clean.
- **RP5 — App: the Persona presentation.** Addition Y in full:
  per-area stages and hosts, signature accents, the readability
  rules as gate criteria. Gate: per-area eye verdicts + the
  parity gate re-run.

After RP5, the P6 feature items proceed in Rust under their own
briefs (toolbox completion, conversion capstone, tracking
interop, teaching completion, the Neovim front-end, the
write-location audit, and the completion sequence: documentation
with previews, this repo as the release home).

## 6. Risks and open points

- **Fonts.** The D build's font work (scaled UI text, Nerd Font
  merging) has one recorded lesson — in a merged font stack, the
  first-added glyph wins a shared codepoint. The Rust font stack
  must carry that as a spec test, not rediscover it.
- **Face tracking.** The D side binds a tracking library; the
  Rust side takes tracking in over VMC/OSC and ARKit first
  (Addition G), and camera tracking is scoped separately.
- **Internationalization.** The upstream editor ships
  translations; scope for Nerd mode is decided at RP4, not
  assumed.
- **Text shaping** for the display typography (per-letter
  treatments are procedural and in-house; body text shaping must
  be verified against the fixture corpus at RP4).

## 7. Effort shape

The CLI port is the most predictable chunk — behavior is pinned
by pre-written gates. The engine is the largest and least
compressible: evaluation and rendering to a golden-image
standard. The GUI is mostly *not* port cost, since the Persona
presentation is new-build under any language. Total: a program
on the order of the original D build-out (phases P0–P5b), plus
engine depth — undertaken because the alternative (building the
next feature phase in D, then porting that too) is strictly more
work, and builds the whole UI twice.
