# torogao-tui media preview: pty evidence must be emulator-replayed, not raw-searched (pax, 2026-11-03)

- origin_lane: ntranch
- content_label: sfw
- Task: media preview for torogao-tui (stills / .inx models / video) on branch `pax/tui-workflow`; report at `~/workspace/torogao-tui-20261101/PREVIEW-REPORT.md`.

## Failures → fixes

1. **Raw pty byte-stream assertions under-report ratatui apps.** ratatui's CrosstermBackend diff-renders: unchanged cells are never rewritten and changed phrases are split by cursor-move escapes (`p` + CUP + `used frame 1/240`). A first pty evidence driver asserting substrings on the raw stream scored 7/12 with false failures on screens that were actually correct. Fix: replay the captured stream through a terminal emulator (pyte) and assert on the reconstructed screen; keep raw-stream assertions only for protocol payloads (kitty `ESC _G`). Re-run: 17/17 checks pass. Any estate TUI evidence harness should do the same.
2. **Fixed-sleep pty drivers race synchronous loads.** Key handlers that block on subprocess work (model render, ffmpeg frame extraction) make fixed-wait drivers flaky in both directions. Fix: poll-until-screen-contains with a timeout per step.
3. **Sandbox GPU absence is a named refusal, not a failure.** wgpu finds no adapter in the sandbox, so `torogao-app --headless-eval --render` evaluates (state-hash proves it) but declines to render. The TUI surfaces the child's reason verbatim under code `TG-TUI-MEDIA-RENDER-UNAVAILABLE`; the same model rendered for real on primo's prebuilt binary (3063×5606 PNG, state-hash identical). Gate design lesson: separate "evaluates" evidence (hash, portable) from "renders" evidence (GPU machine) instead of failing the whole gate on sandbox hardware.

## Gate verdict

- `cargo test -p torogao-tui --offline`: lib 11 passed / 0 failed; smoke 8 passed / 0 failed. Strict clippy `--all-targets -- -D warnings` clean; `cargo fmt --check` clean. (Sandbox cargo needs `RUSTC_WRAPPER=` emptied — the configured sccache wrapper fails there — and `--offline`.)
- PTY evidence: `~/workspace/torogao-tui-20261101/evidence/` (`drive_pty.py` + per-run `.log` raw streams + `-screen.txt` reconstructions), 17/17 checks: half-block still, kitty payload + delete, video frame-step/play/pause at frame accuracy, model named refusal + metadata.
- Affected paths: `crates/torogao-tui/{Cargo.toml,src/media.rs,src/app.rs,src/view.rs,src/lib.rs,src/main.rs,tests/smoke.rs}`. torogao-author untouched — no new verb; preview is view-layer, fed by the existing `torogao-app --headless-eval --render` path.
