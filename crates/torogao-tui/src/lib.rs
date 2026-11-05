//! The Torogao terminal front-end (D-487, 2026-11-01).
//!
//! The GUI's workflow, minus the mouse: a complete keyboard-only
//! terminal UI over the same surfaces the CLI drives. Project
//! create/open, record browsing and editing across the format's
//! families, the verb console, `project_validate` findings,
//! deterministic replay inspection, a companion/persona
//! structured readout, the project's text files (Addition V:
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
pub mod bakeoff;
pub mod records;
pub mod termcap;
pub mod textbuf;
pub mod view;

pub use app::{Accent, App, Screen};

/// The complete keymap, as shown on the Help screen. Every
/// action in the crate is reachable through these keys; the
/// mouse is never required.
pub const KEYMAP: &[(&str, &str)] = &[
    (
        "1..7",
        "jump to Browse / Verbs / Validate / Replay / Preview / Files / Settings",
    ),
    ("?", "this keymap, from anywhere"),
    ("q", "quit (Projects screen)"),
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
    ("p (Settings)", "change the projects root (prompt)"),
    ("y / n (discard prompt)", "confirm / keep editing"),
];
