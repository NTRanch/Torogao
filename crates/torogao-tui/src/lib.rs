//! The Torogao terminal front-end (D-487, 2026-11-01).
//!
//! The GUI's workflow, minus the mouse: a complete keyboard-only
//! terminal UI over the same surfaces the CLI drives. Project
//! create/open, record browsing and editing across the format's
//! families, the verb console, `project_validate` findings,
//! deterministic replay inspection, a companion/persona
//! structured readout, a media preview (artwork, model
//! renders and video frames over terminal graphics with a
//! half-block fallback), a companion pane on the front door
//! and the media screen (Prima's baked Daydream loop, with
//! three bounded decorations, D-639), the project's text files (Addition V:
//! files are the project), settings, and this keymap — every
//! action reachable by keyboard, mouse input used nowhere.
//!
//! There is no third semantics here: every operation runs
//! through `torogao_author`'s face-neutral dispatcher
//! (`run_verb`), exactly as the `torogao author` CLI does. The
//! TUI holds presentation state only; mutations are admitted or
//! refused by the core's `decide()` pipeline, and its refusals
//! are displayed verbatim.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod app;
pub mod companion;
pub mod media;
pub mod records;
pub mod textbuf;
pub mod view;

pub use app::{Accent, App, Screen};

/// The complete keymap, as shown on the Help screen. Every
/// action in the crate is reachable through these keys; the
/// mouse is never required.
pub const KEYMAP: &[(&str, &str)] = &[
    (
        "1..8",
        "jump to Browse / Verbs / Validate / Replay / Preview / Files / Settings / Media",
    ),
    ("?", "this keymap, from anywhere"),
    ("q", "quit (Projects screen)"),
    (
        "o (Media)",
        "preview a media file by path (artwork, .inx model, video)",
    ),
    (
        "Space (Media, video)",
        "play / pause — visual only, no audio is ever played",
    ),
    (
        "Left / Right (Media, video)",
        "step one frame back / forward",
    ),
    ("+ / - (Media)", "zoom the preview in / out"),
    ("j k / Up Down", "move selection"),
    ("g / G", "first / last row"),
    ("PgUp / PgDn", "move by ten rows"),
    ("Tab or h l", "switch Browse pane (families / records)"),
    ("Enter", "open / select / edit inputs (context)"),
    (
        "Esc",
        "back one screen; closes the open project from Browse",
    ),
    (
        "n / o (Projects)",
        "new project (prompt) / open project by path",
    ),
    ("r", "refresh list / re-run validate or replay"),
    (
        "e (Browse, Detail)",
        "edit the selected record via its upsert verb",
    ),
    ("x (Verbs)", "run the selected verb with the current inputs"),
    ("F5 or Ctrl+R (editing)", "run the verb being edited"),
    ("Ctrl+U (editing)", "clear the editor / inputs buffer"),
    ("Ctrl+S (file editor)", "save through the parse gate"),
    ("a (Settings)", "toggle the accent register (Pink / Mono)"),
    (
        "c (Settings)",
        "toggle the companion pane (Prima on Projects / Media)",
    ),
    (
        "d (Settings)",
        "toggle decorations (selection breath, title sparkle, status pulse)",
    ),
    ("p (Settings)", "change the projects root (prompt)"),
    ("y / n (discard prompt)", "confirm / keep editing"),
];
