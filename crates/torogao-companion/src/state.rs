//! The companion's persisted placement state (Addition S):
//! `~/.local/state/torogao/companion.json` on Linux
//! (XDG state), `%LOCALAPPDATA%\Torogao\companion.json` on
//! Windows. Only placement-class facts live here — anchor,
//! margins, output choice, display height, and the
//! click-through lock toggle. No session state: the
//! character, her area, and her current performance belong
//! to the session host (PROGRAM.md §3.4).
//!
//! Writes follow the shell's proven pattern (Phase F write
//! class #5): serialize, write a sibling temp file, rename
//! over the target — and the caller writes only on change.
//! Reads are strict: unknown fields are tolerated (a newer
//! build may add placement facts), but wrong types, an
//! unsupported version, an unknown anchor, or an
//! out-of-range display height refuse typed
//! (`TG-COMP-STATE-CORRUPT`); the binary then starts at the
//! default placement and says so (Gate G7). A missing file
//! is not corruption — it is a first run.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CompanionError, CompanionResult};

/// The state file's format version.
pub const STATE_VERSION: u32 = 1;
/// Default display height, pixels (PROGRAM.md §3.2: inside
/// the portrait-scale readability class).
pub const DISPLAY_HEIGHT_DEFAULT: u32 = 480;
/// Minimum display height, pixels (the readability floor,
/// PROGRAM.md §3.2: the sprite-scale LOST class is out of
/// bounds for the companion).
pub const DISPLAY_HEIGHT_MIN: u32 = 360;
/// Maximum display height, pixels (a sane bound, not a
/// compositor limit).
pub const DISPLAY_HEIGHT_MAX: u32 = 4096;
/// Default margin from the anchored right edge, pixels.
pub const MARGIN_DEFAULT: i32 = 24;
/// Margins the drag may reach, either direction: enough to
/// park her almost off-screen, never so far that no part of
/// her can be grabbed again on a 4K output.
pub const MARGIN_BOUND: i32 = 8192;

/// Which screen corner the companion anchors to. On
/// Wayland this selects the layer-shell anchor edges and
/// the two margins the drag updates; on winit platforms it
/// selects the corner the persisted margins measure from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    /// Bottom-left corner.
    BottomLeft,
    /// Bottom-right corner (the default placement).
    BottomRight,
    /// Top-left corner.
    TopLeft,
    /// Top-right corner.
    TopRight,
}

impl Anchor {
    /// The anchor's stable file name.
    pub fn name(self) -> &'static str {
        match self {
            Anchor::BottomLeft => "bottom-left",
            Anchor::BottomRight => "bottom-right",
            Anchor::TopLeft => "top-left",
            Anchor::TopRight => "top-right",
        }
    }

    /// Whether the anchor sits on the right edge.
    pub fn is_right(self) -> bool {
        matches!(self, Anchor::BottomRight | Anchor::TopRight)
    }

    /// Whether the anchor sits on the bottom edge.
    pub fn is_bottom(self) -> bool {
        matches!(self, Anchor::BottomLeft | Anchor::BottomRight)
    }
}

/// The persisted placement state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanionState {
    /// Format version (must be `STATE_VERSION`).
    pub version: u32,
    /// The anchored corner.
    pub anchor: Anchor,
    /// Margin from the anchor's vertical edge, pixels.
    pub margin_side: i32,
    /// Margin from the anchor's horizontal edge, pixels.
    pub margin_edge: i32,
    /// Display height, pixels.
    pub display_height: u32,
    /// The chosen output's connector name (Wayland), when
    /// one was picked. `None` = the compositor's choice.
    #[serde(default)]
    pub output_name: Option<String>,
    /// The click-through lock: when set, her whole surface
    /// passes pointer events through (she is present but
    /// untouchable until the lock is lifted from a surface
    /// that still receives events — the persisted toggle is
    /// also a command-line/menu state, see the Wayland
    /// module's note).
    #[serde(default)]
    pub click_through_locked: bool,
}

impl Default for CompanionState {
    fn default() -> CompanionState {
        CompanionState {
            version: STATE_VERSION,
            anchor: Anchor::BottomRight,
            margin_side: MARGIN_DEFAULT,
            margin_edge: 0,
            display_height: DISPLAY_HEIGHT_DEFAULT,
            output_name: None,
            click_through_locked: false,
        }
    }
}

impl CompanionState {
    /// Validates the state's own invariants (after a parse,
    /// or after a drag produced new margins).
    pub fn validate(&self) -> CompanionResult<()> {
        if self.version != STATE_VERSION {
            return Err(corrupt(format!(
                "state version {} is not supported (this build writes {STATE_VERSION})",
                self.version
            )));
        }
        if !(DISPLAY_HEIGHT_MIN..=DISPLAY_HEIGHT_MAX).contains(&self.display_height) {
            return Err(corrupt(format!(
                "display_height {} is outside {DISPLAY_HEIGHT_MIN}..={DISPLAY_HEIGHT_MAX}",
                self.display_height
            )));
        }
        for (field, value) in [
            ("margin_side", self.margin_side),
            ("margin_edge", self.margin_edge),
        ] {
            if !(-MARGIN_BOUND..=MARGIN_BOUND).contains(&value) {
                return Err(corrupt(format!(
                    "{field} {value} is outside the margin bound"
                )));
            }
        }
        Ok(())
    }

    /// The canonical serialization (pretty JSON, keys in
    /// field order — serde's struct order).
    pub fn to_json(&self) -> CompanionResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|err| corrupt(format!("state does not serialize: {err}")))
    }

    /// Parses and validates one state document.
    pub fn from_json(text: &str) -> CompanionResult<CompanionState> {
        let state: CompanionState = serde_json::from_str(text)
            .map_err(|err| corrupt(format!("state file is not valid companion state: {err}")))?;
        state.validate()?;
        Ok(state)
    }
}

fn corrupt(message: String) -> CompanionError {
    CompanionError::new("TG-COMP-STATE-CORRUPT", message)
}

/// The Addition S state directory:
/// `$XDG_STATE_HOME/torogao` or `~/.local/state/torogao`
/// (Windows: `%LOCALAPPDATA%\Torogao`).
pub fn state_dir() -> CompanionResult<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir).join("torogao"));
        }
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir).join("Torogao"));
        }
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        CompanionError::new(
            "TG-COMP-STATE-IO",
            "no XDG_STATE_HOME, LOCALAPPDATA, or HOME is set; the state path cannot be resolved",
        )
    })?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("torogao"))
}

/// The default state file path (`state_dir()/companion.json`).
pub fn default_state_path() -> CompanionResult<PathBuf> {
    Ok(state_dir()?.join("companion.json"))
}

/// How a state load turned out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// The file existed and parsed; this is its content.
    Loaded(CompanionState),
    /// No file: a first run, defaults apply.
    Missing(CompanionState),
    /// The file exists but refuses typed; defaults apply
    /// and the refusal is carried for the caller to report.
    Corrupt(CompanionState, CompanionError),
}

/// Loads the state file under the G7 semantics: missing is
/// a first run, corrupt is a typed refusal plus defaults —
/// neither is a crash.
pub fn load_state(path: &Path) -> LoadOutcome {
    match std::fs::read_to_string(path) {
        Ok(text) => match CompanionState::from_json(&text) {
            Ok(state) => LoadOutcome::Loaded(state),
            Err(err) => LoadOutcome::Corrupt(CompanionState::default(), err),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            LoadOutcome::Missing(CompanionState::default())
        }
        Err(err) => LoadOutcome::Corrupt(
            CompanionState::default(),
            CompanionError::new(
                "TG-COMP-STATE-IO",
                format!("state file {} cannot be read: {err}", path.display()),
            ),
        ),
    }
}

/// Writes the state: sibling temp file + rename, creating
/// the parent directory when needed.
pub fn save_state(path: &Path, state: &CompanionState) -> CompanionResult<()> {
    state.validate()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            CompanionError::new(
                "TG-COMP-STATE-IO",
                format!("cannot create state directory {}: {err}", parent.display()),
            )
        })?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, state.to_json()?).map_err(|err| {
        CompanionError::new(
            "TG-COMP-STATE-IO",
            format!("cannot write {}: {err}", temp.display()),
        )
    })?;
    std::fs::rename(&temp, path).map_err(|err| {
        CompanionError::new(
            "TG-COMP-STATE-IO",
            format!(
                "cannot rename {} over {}: {err}",
                temp.display(),
                path.display()
            ),
        )
    })?;
    Ok(())
}

/// The drag's margin arithmetic for the anchored corner:
/// the pointer moved by `(dx, dy)` in surface coordinates
/// (y down); the margins adjacent to the anchor move the
/// opposite way on the anchored axes, clamped so at least
/// `keep_visible` pixels of her stay on the output side of
/// each anchored edge (she can be parked, not lost). Pure
/// function — the movement law both presentations share,
/// unit-gated.
pub fn drag_margins(
    state: &CompanionState,
    drag_start: (i32, i32),
    delta: (f64, f64),
    surface_size: (u32, u32),
    keep_visible: i32,
) -> (i32, i32) {
    let (start_side, start_edge) = drag_start;
    let dx = delta.0.round() as i32;
    let dy = delta.1.round() as i32;
    // Side margin measures from the anchored vertical edge:
    // dragging toward that edge (right for a right anchor,
    // +dx) shrinks it; dragging away grows it.
    let side = if state.anchor.is_right() {
        start_side - dx
    } else {
        start_side + dx
    };
    // Edge margin measures from the anchored horizontal
    // edge: dragging down (+dy) shrinks a bottom margin and
    // grows a top margin.
    let edge = if state.anchor.is_bottom() {
        start_edge - dy
    } else {
        start_edge + dy
    };
    let side_min = keep_visible - surface_size.0 as i32;
    let edge_min = keep_visible - surface_size.1 as i32;
    (
        side.clamp(side_min, MARGIN_BOUND),
        edge.clamp(edge_min, MARGIN_BOUND),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_valid_and_round_trips() {
        let state = CompanionState::default();
        state.validate().expect("default is valid");
        let json = state.to_json().expect("serializes");
        let back = CompanionState::from_json(&json).expect("parses back");
        assert_eq!(state, back);
    }

    #[test]
    fn corrupt_documents_refuse_typed() {
        for text in [
            "not json at all",
            "{\"version\": 99, \"anchor\": \"bottom-right\", \"margin_side\": 0, \"margin_edge\": 0, \"display_height\": 480}",
            "{\"version\": 1, \"anchor\": \"middle\", \"margin_side\": 0, \"margin_edge\": 0, \"display_height\": 480}",
            "{\"version\": 1, \"anchor\": \"bottom-right\", \"margin_side\": 0, \"margin_edge\": 0, \"display_height\": 12}",
            "{\"version\": 1, \"anchor\": \"bottom-right\", \"margin_side\": \"wide\", \"margin_edge\": 0, \"display_height\": 480}",
        ] {
            let err = CompanionState::from_json(text).expect_err("each document refuses");
            assert_eq!(err.code, "TG-COMP-STATE-CORRUPT", "for input {text}");
        }
    }

    #[test]
    fn missing_file_is_a_first_run_not_corruption() {
        let path = std::env::temp_dir().join("torogao-companion-no-such-file-1-1.json");
        match load_state(&path) {
            LoadOutcome::Missing(state) => assert_eq!(state, CompanionState::default()),
            other => panic!("expected Missing, got {other:?}"),
        }
    }

    #[test]
    fn save_then_load_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!(
            "torogao-companion-state-test-{}",
            std::process::id()
        ));
        let path = dir.join("companion.json");
        let state = CompanionState {
            margin_side: 137,
            margin_edge: 42,
            display_height: 512,
            click_through_locked: true,
            ..CompanionState::default()
        };
        save_state(&path, &state).expect("saves");
        match load_state(&path) {
            LoadOutcome::Loaded(back) => assert_eq!(back, state),
            other => panic!("expected Loaded, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drag_moves_the_anchored_margins_and_clamps_at_the_park_bound() {
        let state = CompanionState::default(); // bottom-right
        // Drag right+down by (30, 10): both margins shrink.
        let (side, edge) = drag_margins(&state, (24, 0), (30.0, 10.0), (360, 480), 48);
        assert_eq!((side, edge), (-6, -10));
        // Drag far left: the side margin grows without bound
        // trouble; drag far right: clamped so 48 px remain.
        let (side, _) = drag_margins(&state, (24, 0), (10_000.0, 0.0), (360, 480), 48);
        assert_eq!(side, 48 - 360);
        // A top-left anchor reads the same pointer oppositely.
        let tl = CompanionState {
            anchor: Anchor::TopLeft,
            ..CompanionState::default()
        };
        let (side, edge) = drag_margins(&tl, (24, 8), (30.0, 10.0), (360, 480), 48);
        assert_eq!((side, edge), (54, 18));
    }
}
