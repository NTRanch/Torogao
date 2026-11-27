# Torogao — evaluation and plan under the studio template (D-1261)

**Status: pointer document, DRAFT.** Written 2026-11-10. Torogao
already carries its plans; this file says where they are and how
the studio template (`phorge` repo, `docs/STUDIO-TEMPLATE.md`)
sits on them. Nothing here is built and no PASS is declared.

| Question | Where it is answered |
|---|---|
| What the renderer programme is and its modules M1–M7 | `RUNG2-BRIEF.md` |
| What the programme still lacks | `FEATURE-GAPS.md` |
| Where M1 stands; M2's proposed gates | `docs/render3d/m1/M1-REPORT.md` (stale on the GPU gates — see the M2 plan's table), `docs/render3d/m2/M2-GATE-PLAN.md` |
| Adult-lane style, effects and extraction planning | `docs/render3d/NTRANCH-STYLE-EFFECTS-PLAN.md` |

**Template seat.** Torogao owns stage T6 (assemble) and, with
fathom, T7 (evaluate): it is the build engine of record that every
Phorge path must name a layer of, or record a gap against.

**What the template asks of this repository that no plan above
already holds:**

- **G-1 — Path → template binding.** A `phorge-path` names the
  template `instantiate-template` produces. Needs phorge P1.
- **G-2 — TOROGAO-MODEL-GAP hook.** A combination with no layer is
  written to Phorge's gap register, not left as prose.
- **G-3 — Motion-record reader** (depends on the `motion-record`
  schema crate only — never the analysis crate, never a child
  process: the determinism law holds).
- **G-4 — Verdict-pack assembler** as a shared tool; M1 and M2
  each need one.

**Pick-up order.** None of these precedes the brief's own order
(M2 next). G-4 falls out of M2; G-1 and G-2 wait on phorge; G-3
waits on the fathom instrument's S1.
