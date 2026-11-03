//! The TUI state machine: screens, key handling, and every
//! operation the front-end performs.
//!
//! Semantics live in `torogao_author`, never here. Every verb
//! runs through `torogao_author::run_verb` — the same face-neutral
//! dispatcher the CLI uses — so a refusal on this surface is the
//! CLI's refusal, byte for byte. The only direct core calls are
//! the ones the CLI face also makes: `store::load` /
//! `store::read_log` to hold the project, and the codec/RON
//! parsers as the file editor's save gate (the estate rule that
//! a project's files are edited as text, Addition V — with the
//! replay screen surfacing any divergence a text edit causes).

use crate::companion::CompanionState;
use crate::media::{GraphicsMode, MediaItem, MediaKind, ModelRender, VideoInfo};
use crate::records::{self, Family};
use crate::textbuf::TextBuf;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::RgbaImage;
use serde_json::{Value, json};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Instant;
use torogao_author::model::Project;
use torogao_author::{Refusal, RefusalClass, Status};

/// Projects listed on the home screen, at most.
pub const PROJECTS_LIST_MAX: usize = 256;
/// Log entries the replay screen pages through, at most.
pub const REPLAY_LOG_MAX: usize = 4096;

/// Which screen the body pane shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    /// Project picker (home).
    Projects,
    /// Family/record browser.
    Browse,
    /// One record's detail.
    Detail,
    /// Verb picker + inputs editor + answer pane.
    Console,
    /// `project_validate` findings.
    Validate,
    /// Op-log stepper + replay verdict.
    Replay,
    /// Companion/persona structured readout.
    Preview,
    /// Media preview: artwork, model renders, video frames.
    Media,
    /// The project directory's text files.
    Files,
    /// The text editor (over Files).
    Editor,
    /// Settings.
    Settings,
    /// The keymap.
    Help,
}

/// The Addition U palette registers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Accent {
    /// Hot pink on black (Persona register, Addition U).
    Pink,
    /// Monochrome (Nerd register).
    Mono,
}

/// What a bottom-line prompt will do on Enter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptAction {
    /// Create a project with the prompted name.
    NewProject,
    /// Open the project at the prompted path.
    OpenPath,
    /// Move the projects root to the prompted path.
    ProjectsRoot,
    /// Preview the media file at the prompted path.
    MediaPath,
}

/// The open project and everything the screens hold about it.
pub struct Open {
    /// Project directory.
    pub root: PathBuf,
    /// The parsed project.
    pub project: Project,
    /// The project file's text as loaded.
    pub text: String,
    /// sha256 of the project file's bytes as loaded.
    pub sha256: String,
    /// Record families built from `project`.
    pub families: Vec<Family>,
}

/// One verb row in the console picker.
#[derive(Clone, Debug)]
pub struct VerbRow {
    /// Verb name.
    pub name: String,
    /// Manifest group.
    pub group: String,
    /// Whether the verb mutates the project.
    pub mutating: bool,
    /// The slice label when the verb is not yet implemented.
    pub deferred: Option<String>,
}

/// Console screen state.
#[derive(Default)]
pub struct ConsoleState {
    /// All manifest verbs (implemented first, then deferred).
    pub verbs: Vec<VerbRow>,
    /// Selected verb row.
    pub sel: usize,
    /// Inputs JSON being edited.
    pub inputs: TextBuf,
    /// Whether the inputs editor holds the keyboard.
    pub editing: bool,
    /// The last answer: status + pretty document.
    pub answer: Option<(Status, String)>,
}

/// Validate screen state.
#[derive(Default)]
pub struct ValidateState {
    /// Findings as `(code, message)` rows.
    pub findings: Vec<(String, String)>,
    /// Selected finding.
    pub sel: usize,
    /// Whether validate has run for the current project state.
    pub ran: bool,
    /// The project hash validate ran against.
    pub sha256: String,
}

/// Replay screen state.
#[derive(Default)]
pub struct ReplayState {
    /// Op-log records, in log order.
    pub log: Vec<Value>,
    /// Selected log record.
    pub sel: usize,
    /// The replay report document, when replay succeeded.
    pub report: Option<Value>,
    /// The replay refusal `(code, message)`, when it refused —
    /// `TG-AUTH-REPLAY-DIVERGENCE` is the core's verdict that
    /// the log and the on-disk project disagree.
    pub refusal: Option<(String, String)>,
    /// Whether replay has run.
    pub ran: bool,
}

/// Preview screen state.
#[derive(Default)]
pub struct PreviewState {
    /// Selected expression id.
    pub sel: usize,
    /// The last `expression_test` result lines.
    pub tested: Vec<String>,
}

/// Video transport state, while a video item is loaded.
pub struct VideoState {
    /// The probe result.
    pub info: VideoInfo,
    /// The frame the raster currently shows.
    pub frame: u64,
    /// Playback position in seconds (the transport's clock).
    pub position_s: f64,
    /// Whether the transport is playing.
    pub playing: bool,
    /// When the transport last advanced.
    pub last_tick: Option<Instant>,
}

/// Media screen state.
pub struct MediaState {
    /// Listed media items (project scan or a prompted path).
    pub items: Vec<MediaItem>,
    /// Selected item.
    pub sel: usize,
    /// Detected graphics mode for this terminal.
    pub mode: GraphicsMode,
    /// Why that mode was picked (shown in the pane).
    pub mode_reason: String,
    /// The raster on display: a still, a video frame, or a
    /// model render, fitted by the media layer.
    pub raster: Option<RgbaImage>,
    /// Info lines under the viewport (dims, model metadata,
    /// the transport readout).
    pub info: Vec<String>,
    /// The last named refusal, when the selected item would
    /// not preview.
    pub error: Option<String>,
    /// Video transport, when the item is a video.
    pub video: Option<VideoState>,
    /// The model render report, when the item is a model.
    pub model: Option<ModelRender>,
    /// Display zoom, 0.25..=3.0.
    pub zoom: f32,
    /// The graphics blast cache: (signature, escape text) the
    /// shell writes after the frame draw.
    pub blast: Option<(u64, String)>,
    /// Serial for kitty image ids.
    pub blast_serial: u32,
}

impl Default for MediaState {
    fn default() -> MediaState {
        let (mode, mode_reason) = crate::media::detect_graphics_env();
        MediaState {
            items: Vec::new(),
            sel: 0,
            mode,
            mode_reason,
            raster: None,
            info: Vec::new(),
            error: None,
            video: None,
            model: None,
            zoom: 1.0,
            blast: None,
            blast_serial: 0,
        }
    }
}

/// One graphics blast for the shell: write `text` to the
/// terminal after the draw when `signature` changes.
pub struct GraphicsBlast {
    /// Content signature; the shell skips repeats.
    pub signature: u64,
    /// The escape text to write.
    pub text: String,
}

/// One file row on the Files screen.
#[derive(Clone, Debug)]
pub struct FileRow {
    /// File name within the project directory.
    pub name: String,
    /// Whether the file exists yet.
    pub exists: bool,
    /// Whether the editor may write it (`ops.jsonl` is append-only).
    pub editable: bool,
}

/// Editor screen state.
pub struct EditorState {
    /// The file being edited.
    pub path: PathBuf,
    /// Display name.
    pub name: String,
    /// The text.
    pub buf: TextBuf,
    /// Unsaved changes present.
    pub dirty: bool,
    /// The save gate's last refusal, when it refused.
    pub diagnostic: Option<String>,
    /// Read-only (append-only log, or beyond the editor bounds).
    pub readonly: bool,
    /// An Esc with unsaved changes is awaiting y/n.
    pub confirm_discard: bool,
}

/// The whole application state.
pub struct App {
    /// Active screen.
    pub screen: Screen,
    /// Where Help returns to.
    pub help_return: Screen,
    /// Palette register.
    pub accent: Accent,
    /// Directory holding the listed projects.
    pub projects_root: PathBuf,
    /// Listed project directories.
    pub projects: Vec<PathBuf>,
    /// Selected project row.
    pub project_sel: usize,
    /// The open project, if any.
    pub open: Option<Open>,
    /// Selected family (Browse).
    pub family_sel: usize,
    /// Selected record (Browse).
    pub record_sel: usize,
    /// Whether the record pane holds Browse focus.
    pub browse_records_focus: bool,
    /// Detail scroll offset.
    pub detail_scroll: usize,
    /// Console state.
    pub console: ConsoleState,
    /// Validate state.
    pub validate: ValidateState,
    /// Replay state.
    pub replay: ReplayState,
    /// Preview state.
    pub preview: PreviewState,
    /// Media screen state.
    pub media: MediaState,
    /// The media viewport's screen rect `(x, y, width, height)`
    /// as last drawn; the graphics blast places itself there.
    /// Set by the view through interior mutability (draw takes
    /// `&App`), read by the app when building a blast.
    pub media_viewport: Cell<Option<(u16, u16, u16, u16)>>,
    /// The companion pane's animation state (D-639).
    pub companion: CompanionState,
    /// Whether the companion pane is enabled (Settings,
    /// default on; persisted).
    pub companion_on: bool,
    /// Whether the decorations (DEC-1/2/3) are enabled
    /// (Settings, default on; persisted).
    pub decorations_on: bool,
    /// Event-loop turns since start; the decorations' clock
    /// and the companion's cadence both read it.
    pub tick_count: u64,
    /// The companion pane's image rect `(x, y, width, height)`
    /// as last drawn; the companion blast places itself
    /// there. Set by the view through interior mutability,
    /// like `media_viewport`.
    pub companion_viewport: Cell<Option<(u16, u16, u16, u16)>>,
    /// Blast signatures the composed [`App::graphics_blast`]
    /// last handed the shell, `(media, companion)`; 0 = none.
    /// Lets an unchanged component stay out of the composed
    /// text when the other component changes.
    pub blast_stage: Cell<(u64, u64)>,
    /// Whether the terminal currently holds focus. Focus
    /// loss freezes the companion (advance + blast refresh);
    /// terminals that never report focus never clear this.
    pub focused: bool,
    /// File rows for the open project.
    pub files: Vec<FileRow>,
    /// Selected file row.
    pub file_sel: usize,
    /// Editor state, while the editor is open.
    pub editor: Option<EditorState>,
    /// The active prompt, if any: (action, buffer).
    pub prompt: Option<(PromptAction, TextBuf)>,
    /// The status line: the last action's outcome, in plain text.
    pub status: String,
    /// Set by the quit key; the shell exits on it.
    pub quit: bool,
}

impl App {
    /// A fresh app listing projects under `projects_root`.
    pub fn new(projects_root: PathBuf) -> App {
        let mut app = App {
            screen: Screen::Projects,
            help_return: Screen::Projects,
            accent: Accent::Pink,
            projects_root,
            projects: Vec::new(),
            project_sel: 0,
            open: None,
            family_sel: 0,
            record_sel: 0,
            browse_records_focus: false,
            detail_scroll: 0,
            console: ConsoleState {
                verbs: verb_rows(),
                ..ConsoleState::default()
            },
            validate: ValidateState::default(),
            replay: ReplayState::default(),
            preview: PreviewState::default(),
            media: MediaState::default(),
            media_viewport: Cell::new(None),
            companion: CompanionState::default(),
            companion_on: true,
            decorations_on: true,
            tick_count: 0,
            companion_viewport: Cell::new(None),
            blast_stage: Cell::new((0, 0)),
            focused: true,
            files: Vec::new(),
            file_sel: 0,
            editor: None,
            prompt: None,
            status: String::new(),
            quit: false,
        };
        app.refresh_projects();
        app
    }

    /// The projects root from settings/environment conventions:
    /// `$TOROGAO_PROJECTS`, else `<cwd>/torogao-projects`.
    pub fn default_projects_root() -> PathBuf {
        if let Ok(root) = std::env::var("TOROGAO_PROJECTS") {
            if !root.is_empty() {
                return PathBuf::from(root);
            }
        }
        PathBuf::from("torogao-projects")
    }

    /// Re-scans the projects root: subdirectories holding a
    /// `project.ron`, sorted by path, capped at `PROJECTS_LIST_MAX`.
    pub fn refresh_projects(&mut self) {
        self.projects.clear();
        if let Ok(entries) = std::fs::read_dir(&self.projects_root) {
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_dir() && path.join(torogao_author::store::PROJECT_FILE).is_file()
                })
                .collect();
            found.sort();
            found.truncate(PROJECTS_LIST_MAX);
            self.projects = found;
        }
        if self.project_sel >= self.projects.len() {
            self.project_sel = self.projects.len().saturating_sub(1);
        }
    }

    /// Opens the project at `path` and lands on the browser.
    pub fn open_project(&mut self, path: &Path) {
        match torogao_author::store::load(path) {
            Ok(Some(loaded)) => {
                self.families_into(loaded, path.to_path_buf());
                self.console.answer = None;
                self.screen = Screen::Browse;
                self.status = format!("opened {}", path.display());
            }
            Ok(None) => {
                self.status = format!("no project.ron at {}", path.display());
            }
            Err(refusal) => {
                self.status = format!("{}: {}", refusal.code(), refusal.message);
            }
        }
    }

    fn families_into(&mut self, loaded: torogao_author::store::Loaded, root: PathBuf) {
        let families = records::families(&loaded.project);
        self.open = Some(Open {
            root,
            project: loaded.project,
            text: loaded.text,
            sha256: loaded.sha256,
            families,
        });
        self.family_sel = 0;
        self.record_sel = 0;
        self.browse_records_focus = false;
        self.validate = ValidateState::default();
        self.replay = ReplayState::default();
        self.preview = PreviewState::default();
        self.refresh_files();
    }

    /// Reloads the open project from disk (after a mutation or a
    /// text save), keeping selections where they still exist.
    pub fn reload_project(&mut self) {
        let Some(root) = self.open.as_ref().map(|open| open.root.clone()) else {
            return;
        };
        match torogao_author::store::load(&root) {
            Ok(Some(loaded)) => {
                let family_sel = self.family_sel;
                let record_sel = self.record_sel;
                self.families_into(loaded, root);
                self.family_sel = family_sel.min(
                    self.open
                        .as_ref()
                        .map_or(0, |o| o.families.len().saturating_sub(1)),
                );
                if let Some(open) = &self.open {
                    let count = open
                        .families
                        .get(self.family_sel)
                        .map_or(0, |family| family.records.len());
                    self.record_sel = record_sel.min(count.saturating_sub(1));
                }
            }
            Ok(None) => {
                self.status = format!("project vanished at {}", root.display());
            }
            Err(refusal) => {
                self.status = format!("{}: {}", refusal.code(), refusal.message);
            }
        }
    }

    fn refresh_files(&mut self) {
        let Some(open) = &self.open else {
            self.files.clear();
            return;
        };
        let content = open.root.join("content.ron");
        self.files = vec![
            FileRow {
                name: torogao_author::store::PROJECT_FILE.to_string(),
                exists: true,
                editable: true,
            },
            FileRow {
                name: "content.ron".to_string(),
                exists: content.is_file(),
                editable: true,
            },
            FileRow {
                name: torogao_author::store::OPS_FILE.to_string(),
                exists: open.root.join(torogao_author::store::OPS_FILE).is_file(),
                editable: false,
            },
        ];
        if self.file_sel >= self.files.len() {
            self.file_sel = 0;
        }
    }

    /// Runs one verb against the open project through the shared
    /// dispatcher. No-op (with a status) when no project is open.
    pub fn run_open_verb(&mut self, verb: &str, inputs: &Value) -> Option<(Status, Value)> {
        let root = self.open.as_ref()?.root.clone();
        let projects_root = self.projects_root.clone();
        let resolve = |name: &str| -> Result<PathBuf, Refusal> {
            let candidate = projects_root.join(name);
            if candidate
                .join(torogao_author::store::PROJECT_FILE)
                .is_file()
            {
                Ok(candidate)
            } else {
                Err(Refusal::new(
                    RefusalClass::UnknownTarget,
                    format!("no project named {name}"),
                ))
            }
        };
        let answer = torogao_author::run_verb(&root, &root, verb, inputs, &resolve);
        Some((answer.status, answer.doc))
    }

    /// Routes one key event. Mouse events never reach this
    /// function — the shell does not enable mouse capture, and
    /// the app ignores them defensively if a backend sends any.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.prompt.is_some() {
            self.prompt_key(key);
            return;
        }
        if self.screen == Screen::Editor {
            self.editor_key(key);
            return;
        }
        if self.console.editing && self.screen == Screen::Console {
            self.console_edit_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('?') => {
                self.help_return = self.screen;
                self.screen = Screen::Help;
                return;
            }
            KeyCode::Char('1') if self.open.is_some() => self.screen = Screen::Browse,
            KeyCode::Char('2') if self.open.is_some() => self.screen = Screen::Console,
            KeyCode::Char('3') if self.open.is_some() => {
                self.screen = Screen::Validate;
                self.run_validate();
            }
            KeyCode::Char('4') if self.open.is_some() => {
                self.screen = Screen::Replay;
                self.run_replay();
            }
            KeyCode::Char('5') if self.open.is_some() => self.screen = Screen::Preview,
            KeyCode::Char('8') => self.goto_media(),
            KeyCode::Char('6') if self.open.is_some() => {
                self.refresh_files();
                self.screen = Screen::Files;
            }
            KeyCode::Char('7') => self.screen = Screen::Settings,
            _ => {}
        }
        match self.screen {
            Screen::Projects => self.projects_key(key),
            Screen::Browse => self.browse_key(key),
            Screen::Detail => self.detail_key(key),
            Screen::Console => self.console_key(key),
            Screen::Validate => self.validate_key(key),
            Screen::Replay => self.replay_key(key),
            Screen::Preview => self.preview_key(key),
            Screen::Media => self.media_key(key),
            Screen::Files => self.files_key(key),
            Screen::Settings => self.settings_key(key),
            Screen::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                    self.screen = self.help_return;
                }
            }
            Screen::Editor => unreachable!("editor handled above"),
        }
    }

    // -- Projects -------------------------------------------------

    fn projects_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('r') => self.refresh_projects(),
            KeyCode::Char('n') => self.prompt = Some((PromptAction::NewProject, TextBuf::new())),
            KeyCode::Char('o') => self.prompt = Some((PromptAction::OpenPath, TextBuf::new())),
            KeyCode::Enter => {
                if let Some(path) = self.projects.get(self.project_sel).cloned() {
                    self.open_project(&path);
                }
            }
            _ => {
                nav(&mut self.project_sel, self.projects.len(), key);
            }
        }
    }

    fn create_project(&mut self, name: &str) {
        let root = self.projects_root.join(name);
        if let Err(err) = std::fs::create_dir_all(&root) {
            self.status = format!("cannot create {}: {err}", root.display());
            return;
        }
        let inputs = json!({
            "consent": {"adult": true, "schema": "torogao-consent/1", "wants_selected_scenes": true},
            "name": name,
            "stamp": {"basis": "tui", "lane": "all-ages", "pack": "all-ages", "rating": "all-ages"},
        });
        let resolve = |_: &str| -> Result<PathBuf, Refusal> {
            Err(Refusal::new(
                RefusalClass::UnknownTarget,
                "no projects resolvable during init".to_string(),
            ))
        };
        let answer = torogao_author::run_verb(&root, &root, "project_init", &inputs, &resolve);
        match answer.status {
            Status::Ok => {
                self.refresh_projects();
                self.open_project(&root);
            }
            _ => {
                self.status = format!(
                    "project_init refused: {}",
                    answer.doc["message"].as_str().unwrap_or("see console")
                );
            }
        }
    }

    // -- Browse / Detail ------------------------------------------

    fn browse_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.open = None;
                self.screen = Screen::Projects;
                self.refresh_projects();
            }
            KeyCode::Tab
            | KeyCode::Char('h')
            | KeyCode::Char('l')
            | KeyCode::Left
            | KeyCode::Right => {
                self.browse_records_focus = !self.browse_records_focus;
            }
            KeyCode::Enter if self.browse_records_focus => {
                if self.current_record().is_some() {
                    self.detail_scroll = 0;
                    self.screen = Screen::Detail;
                }
            }
            KeyCode::Char('e') => self.edit_selected_record(),
            _ => {
                if self.browse_records_focus {
                    let count = self
                        .open
                        .as_ref()
                        .and_then(|open| open.families.get(self.family_sel))
                        .map_or(0, |family| family.records.len());
                    nav(&mut self.record_sel, count, key);
                } else {
                    let count = self.open.as_ref().map_or(0, |open| open.families.len());
                    if nav(&mut self.family_sel, count, key) {
                        self.record_sel = 0;
                    }
                }
            }
        }
    }

    fn detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Browse,
            KeyCode::Char('e') => self.edit_selected_record(),
            KeyCode::Char('j') | KeyCode::Down => self.detail_scroll += 1,
            KeyCode::Char('k') | KeyCode::Up => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            _ => {}
        }
    }

    /// The selected family + record, when both exist.
    pub fn current_family(&self) -> Option<&Family> {
        self.open.as_ref()?.families.get(self.family_sel)
    }

    /// The selected record, when it exists.
    pub fn current_record(&self) -> Option<&records::Record> {
        self.current_family()?.records.get(self.record_sel)
    }

    /// Opens the console on the verb that edits the selected
    /// record, inputs prefilled with the record's current shape.
    fn edit_selected_record(&mut self) {
        let Some(open) = &self.open else { return };
        let Some(family) = open.families.get(self.family_sel) else {
            return;
        };
        let Some(record) = family.records.get(self.record_sel) else {
            return;
        };
        match records::prefill(family.name, &record.id, &open.project) {
            Some((verb, inputs)) => {
                if let Some(index) = self.console.verbs.iter().position(|row| row.name == verb) {
                    self.console.sel = index;
                }
                self.console.inputs = TextBuf::from_text(&pretty(&inputs));
                self.console.answer = None;
                self.console.editing = true;
                self.screen = Screen::Console;
                self.status = format!("editing {} via {verb} — F5 runs it", record.id);
            }
            None => {
                self.screen = Screen::Console;
                self.status = format!(
                    "no in-place edit verb for '{}' records — definitions are create-once \
                     in the core (changes refuse TG-AUTH-CONFLICT); remove/define here",
                    family.name
                );
            }
        }
    }

    // -- Console ----------------------------------------------------

    fn console_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Browse,
            KeyCode::Enter => {
                self.console.editing = true;
                if self.console.inputs.text().trim().is_empty() {
                    self.console.inputs = TextBuf::from_text("{}");
                }
            }
            KeyCode::Char('x') => self.run_console_verb(),
            _ => {
                let count = self.console.verbs.len();
                nav(&mut self.console.sel, count, key);
            }
        }
    }

    fn console_edit_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('r') => {
                    self.run_console_verb();
                }
                KeyCode::Char('u') => self.console.inputs.clear(),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.console.editing = false,
            KeyCode::F(5) => self.run_console_verb(),
            _ => text_key(&mut self.console.inputs, key),
        }
    }

    /// Runs the selected verb with the edited inputs. Bad JSON is
    /// a diagnostic in the answer pane; the project is untouched
    /// because nothing ran.
    pub fn run_console_verb(&mut self) {
        let Some(row) = self.console.verbs.get(self.console.sel) else {
            return;
        };
        let verb = row.name.clone();
        let mutating = row.mutating;
        let text = self.console.inputs.text();
        let inputs: Value = match serde_json::from_str(if text.trim().is_empty() {
            "{}"
        } else {
            text.trim()
        }) {
            Ok(value) => value,
            Err(err) => {
                self.console.answer = Some((
                    Status::Refused,
                    format!(
                        "inputs are not valid JSON: {err}\n(nothing ran; the project is untouched)"
                    ),
                ));
                return;
            }
        };
        match self.run_open_verb(&verb, &inputs) {
            Some((status, doc)) => {
                self.console.answer = Some((status, pretty(&doc)));
                self.status = format!("{verb}: {}", status_label(status));
                if mutating && status == Status::Ok {
                    self.reload_project();
                }
            }
            None => {
                self.status = "no project open".to_string();
            }
        }
    }

    // -- Validate ---------------------------------------------------

    /// Runs `project_validate` and stores the findings.
    pub fn run_validate(&mut self) {
        if self.open.is_none() {
            return;
        }
        if let Some((status, doc)) = self.run_open_verb("project_validate", &json!({})) {
            self.validate.findings = doc["findings"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .map(|row| {
                            (
                                row["code"].as_str().unwrap_or("?").to_string(),
                                row["message"].as_str().unwrap_or("").to_string(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.validate.sel = 0;
            self.validate.ran = true;
            self.validate.sha256 = doc["sha256"].as_str().unwrap_or("").to_string();
            self.status = if self.validate.findings.is_empty() {
                "project_validate: clean".to_string()
            } else {
                format!(
                    "project_validate: {} finding(s)",
                    self.validate.findings.len()
                )
            };
            let _ = status;
        }
    }

    fn validate_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Browse,
            KeyCode::Char('r') => self.run_validate(),
            _ => {
                let count = self.validate.findings.len();
                nav(&mut self.validate.sel, count, key);
            }
        }
    }

    // -- Replay -----------------------------------------------------

    /// Runs `project_replay` and reloads the op log for stepping.
    /// A refusal is stored, not hidden: divergence between the
    /// log and `project.ron` is exactly what replay exists to
    /// catch, and the core refuses rather than reporting past it.
    pub fn run_replay(&mut self) {
        let Some(open) = &self.open else { return };
        let root = open.root.clone();
        if let Some((status, doc)) = self.run_open_verb("project_replay", &json!({})) {
            if status == Status::Refused {
                self.replay.report = None;
                self.replay.refusal = Some((
                    doc["code"].as_str().unwrap_or("?").to_string(),
                    doc["message"].as_str().unwrap_or("").to_string(),
                ));
            } else {
                self.replay.report = Some(doc);
                self.replay.refusal = None;
            }
        }
        match torogao_author::store::read_log(&root) {
            Ok(mut log) => {
                log.truncate(REPLAY_LOG_MAX);
                self.replay.log = log;
            }
            Err(refusal) => {
                self.status = format!("{}: {}", refusal.code(), refusal.message);
            }
        }
        self.replay.sel = 0;
        self.replay.ran = true;
    }

    /// Whether replay refused with `TG-AUTH-REPLAY-DIVERGENCE`:
    /// the hash chain rebuilt from the op log does not reproduce
    /// the on-disk `project.ron` — the divergence a text edit (or
    /// a tampered log) produces, surfaced rather than hidden.
    pub fn replay_diverged(&self) -> bool {
        self.replay
            .refusal
            .as_ref()
            .is_some_and(|(code, _)| code == "TG-AUTH-REPLAY-DIVERGENCE")
    }

    fn replay_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Browse,
            KeyCode::Char('r') => self.run_replay(),
            _ => {
                let count = self.replay.log.len();
                nav(&mut self.replay.sel, count, key);
            }
        }
    }

    // -- Preview ------------------------------------------------------

    fn preview_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Browse,
            KeyCode::Enter => self.test_selected_expression(),
            _ => {
                let count = self
                    .open
                    .as_ref()
                    .map_or(0, |open| open.project.expressions.len());
                nav(&mut self.preview.sel, count, key);
            }
        }
    }

    fn test_selected_expression(&mut self) {
        let Some(open) = &self.open else { return };
        let Some(expression) = open.project.expressions.get(self.preview.sel) else {
            return;
        };
        let id = expression.id.clone();
        if let Some((status, doc)) = self.run_open_verb("expression_test", &json!({"id": id})) {
            self.preview.tested = match status {
                Status::Ok => pretty(&doc["state"]).lines().map(str::to_string).collect(),
                _ => vec![format!(
                    "refused: {}",
                    doc["message"].as_str().unwrap_or("unknown")
                )],
            };
        }
    }

    // -- Media ------------------------------------------------------

    /// Enters the Media screen: rescans the open project's
    /// media (assets are ordinary files under the project
    /// directory) and loads the selected item. With no project
    /// open the list keeps whatever a path prompt loaded.
    pub fn goto_media(&mut self) {
        if let Some(open) = &self.open {
            let root = open.root.clone();
            self.media.items = crate::media::collect_media(&root);
            if self.media.sel >= self.media.items.len() {
                self.media.sel = self.media.items.len().saturating_sub(1);
            }
            self.media_load();
        } else if self.media.items.is_empty() {
            self.media.info = vec!["no project open — o opens a media file by path".to_string()];
        }
        self.screen = Screen::Media;
    }

    /// Loads the selected media item into the viewport: a
    /// still decode, a model render through `torogao-app`, or
    /// a video probe + first frame. Every failure lands in
    /// `media.error` as its named refusal; the pane is never
    /// silently blank.
    pub fn media_load(&mut self) {
        self.media.raster = None;
        self.media.error = None;
        self.media.info = Vec::new();
        self.media.video = None;
        self.media.model = None;
        self.media.blast = None;
        let Some(item) = self.media.items.get(self.media.sel).cloned() else {
            return;
        };
        match item.kind {
            MediaKind::Image => match crate::media::decode_still(&item.path) {
                Ok(raster) => {
                    self.media.info = vec![
                        format!("{} × {} px", raster.width(), raster.height()),
                        item.path.display().to_string(),
                    ];
                    self.media.raster = Some(raster);
                    self.status = format!("previewing {}", item.label);
                }
                Err(err) => self.media_refuse(&item, &err.line()),
            },
            MediaKind::Model => match crate::media::render_model(&item.path) {
                Ok(render) => {
                    let mut info = Vec::new();
                    if !render.summary.is_empty() {
                        info.push(render.summary.clone());
                    }
                    if !render.state_hash.is_empty() {
                        info.push(format!("state-hash {}", render.state_hash));
                    }
                    match (&render.png, &render.unavailable) {
                        (Some(png), _) => match crate::media::decode_still(png) {
                            Ok(raster) => {
                                info.push(render.render_line.clone());
                                self.media.raster = Some(raster);
                                self.status = format!("rendered {}", item.label);
                            }
                            Err(err) => self.media.error = Some(err.line()),
                        },
                        (None, Some(why)) => {
                            self.media.error =
                                Some(format!("TG-TUI-MEDIA-RENDER-UNAVAILABLE: {why}"));
                            self.status =
                                format!("{}: render unavailable on this machine", item.label);
                        }
                        (None, None) => {
                            self.media.error = Some(
                                "TG-TUI-MEDIA-RENDER-UNAVAILABLE: the renderer wrote no PNG and gave no reason"
                                    .to_string(),
                            );
                        }
                    }
                    self.media.info = info;
                    self.media.model = Some(render);
                }
                Err(err) => self.media_refuse(&item, &err.line()),
            },
            MediaKind::Video => match crate::media::probe_video(&item.path) {
                Ok(info) => {
                    let frame =
                        crate::media::extract_frame(&item.path, 0.0, crate::media::FRAME_EDGE_MAX);
                    match frame {
                        Ok(raster) => {
                            self.media.raster = Some(raster);
                            self.media.video = Some(VideoState {
                                info,
                                frame: 0,
                                position_s: 0.0,
                                playing: false,
                                last_tick: None,
                            });
                            self.media_refresh_info();
                            self.status = format!("previewing {}", item.label);
                        }
                        Err(err) => self.media_refuse(&item, &err.line()),
                    }
                }
                Err(err) => self.media_refuse(&item, &err.line()),
            },
        }
        self.refresh_blast();
    }

    fn media_refuse(&mut self, item: &MediaItem, line: &str) {
        self.media.error = Some(line.to_string());
        self.status = format!("{}: preview refused", item.label);
    }

    /// Rebuilds the transport/info line for the loaded video.
    fn media_refresh_info(&mut self) {
        let Some(video) = &self.media.video else {
            return;
        };
        let info = &video.info;
        let position = format_clock(video.position_s);
        let duration = format_clock(info.duration_s);
        self.media.info =
            vec![
            format!(
                "{} frame {}/{} · {position}/{duration} · {:.2} fps · {}×{}",
                if video.playing { "▶ playing" } else { "❚❚ paused" },
                video.frame + 1,
                info.frames.max(1),
                info.fps,
                info.width,
                info.height,
            ),
            "space play/pause · ←/→ step one frame · visual only — no audio is decoded or played"
                .to_string(),
        ];
    }

    /// Steps the loaded video by `delta` frames (clamped),
    /// extracting the landing frame. A failed extraction is a
    /// named refusal in the pane.
    pub fn media_step(&mut self, delta: i64) {
        let Some(item) = self.media.items.get(self.media.sel).cloned() else {
            return;
        };
        let Some(video) = &mut self.media.video else {
            return;
        };
        video.playing = false;
        video.last_tick = None;
        let last = video.info.frames.saturating_sub(1);
        let frame = (video.frame as i64 + delta).clamp(0, last as i64) as u64;
        video.frame = frame;
        video.position_s = video.info.frame_time(frame);
        let at = video.position_s;
        match crate::media::extract_frame(&item.path, at, crate::media::FRAME_EDGE_MAX) {
            Ok(raster) => {
                self.media.raster = Some(raster);
                self.media.error = None;
            }
            Err(err) => self.media.error = Some(err.line()),
        }
        self.media_refresh_info();
        self.refresh_blast();
    }

    /// Advances the transport: called once per event-loop
    /// turn by the shell. While a video plays, position moves
    /// by real elapsed time (capped per turn) and the raster
    /// is re-extracted when the displayed frame changes —
    /// playback stays time-true, skipping frames the
    /// extractor cannot keep up with rather than slowing.
    ///
    /// The same turn advances the companion (D-639): one
    /// frame per turn — the shell's 100 ms poll is the 10 fps
    /// cadence — while the front door or the Media screen is
    /// showing, the toggle is on, and the terminal holds
    /// focus. Any other screen stops the advance; the blast
    /// is withdrawn by [`App::refresh_companion_blast`].
    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.companion_on && matches!(self.screen, Screen::Projects | Screen::Media) {
            if self.focused {
                self.companion.advance();
            }
            self.companion.ensure_raster();
        }
        if self.screen != Screen::Media {
            return;
        }
        let now = Instant::now();
        let Some(video) = &mut self.media.video else {
            return;
        };
        if !video.playing {
            video.last_tick = None;
            return;
        }
        let elapsed = video
            .last_tick
            .map(|then| now.duration_since(then).as_secs_f64().min(0.25))
            .unwrap_or(0.0);
        video.last_tick = Some(now);
        video.position_s += elapsed;
        if video.info.duration_s > 0.0 && video.position_s >= video.info.duration_s {
            video.position_s = video.info.duration_s;
            video.playing = false;
            video.frame = video.info.frames.saturating_sub(1);
            self.media_refresh_info();
            return;
        }
        let frame = video.info.frame_at(video.position_s);
        if frame != video.frame {
            video.frame = frame;
            let Some(item) = self.media.items.get(self.media.sel).cloned() else {
                return;
            };
            match crate::media::extract_frame(
                &item.path,
                video.info.frame_time(frame),
                crate::media::FRAME_EDGE_MAX,
            ) {
                Ok(raster) => {
                    self.media.raster = Some(raster);
                    self.media.error = None;
                }
                Err(err) => {
                    self.media.error = Some(err.line());
                    if let Some(video) = &mut self.media.video {
                        video.playing = false;
                    }
                }
            }
            self.media_refresh_info();
            self.refresh_blast();
        }
    }

    /// (Re)builds the graphics blast for the current raster +
    /// viewport when the mode is a direct-graphics protocol.
    /// Half-block renders inside the buffer and never blasts.
    pub fn refresh_blast(&mut self) {
        if self.screen != Screen::Media || self.media.mode == GraphicsMode::HalfBlock {
            self.media.blast = None;
            return;
        }
        let (Some(raster), Some((x, y, w, h))) =
            (self.media.raster.as_ref(), self.media_viewport.get())
        else {
            return;
        };
        if w == 0 || h == 0 {
            return;
        }
        let cols = ((f32::from(w) * self.media.zoom) as u16).clamp(1, w);
        let rows = ((f32::from(h) * self.media.zoom) as u16).clamp(1, h);
        let col = x + (w - cols) / 2 + 1;
        let row = y + (h - rows) / 2 + 1;
        let item_key = self
            .media
            .items
            .get(self.media.sel)
            .map_or(0, |item| hash_of(&item.path.to_string_lossy()));
        let frame_key = match &self.media.video {
            Some(video) => video.frame,
            None => 0,
        };
        let signature = item_key
            ^ (frame_key << 8)
            ^ u64::from(self.media.zoom.to_bits())
            ^ (u64::from(col) << 40)
            ^ (u64::from(row) << 48)
            ^ (u64::from(cols) << 52)
            ^ (u64::from(rows) << 58);
        if self.media.blast.as_ref().map(|(sig, _)| *sig) == Some(signature) {
            return;
        }
        let text = match self.media.mode {
            GraphicsMode::Kitty => {
                let fitted = crate::media::fit_edge(raster, crate::media::KITTY_EDGE_MAX);
                match crate::media::encode_png(&fitted) {
                    Ok(png) => {
                        self.media.blast_serial += 1;
                        format!(
                            "{}{}",
                            crate::media::kitty_delete(),
                            crate::media::kitty_blast(
                                &png,
                                self.media.blast_serial,
                                col,
                                row,
                                cols,
                                rows,
                            )
                        )
                    }
                    Err(_) => return,
                }
            }
            GraphicsMode::Sixel => {
                let fitted = crate::media::fit_edge(raster, crate::media::SIXEL_EDGE_MAX);
                format!(
                    "\x1b[{};{}H{}",
                    row,
                    col,
                    crate::media::sixel_blast(&fitted)
                )
            }
            GraphicsMode::HalfBlock => unreachable!("half-block returned above"),
        };
        self.media.blast = Some((signature, text));
    }

    /// Records the terminal's focus state. Focus loss
    /// freezes the companion: no advance (in [`App::tick`])
    /// and no blast refresh (in
    /// [`App::refresh_companion_blast`]); the last frame
    /// stands. Focus gain resumes both.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// (Re)builds the companion's graphics blast for the
    /// current frame + pane rect when the mode is a
    /// direct-graphics protocol. The companion lives only on
    /// Projects and Media; anywhere else, or with the toggle
    /// off, the blast is withdrawn so no image lingers over
    /// another screen. Focus loss freezes the blast as it
    /// stands rather than withdrawing it.
    pub fn refresh_companion_blast(&mut self) {
        if !self.companion_on
            || !matches!(self.screen, Screen::Projects | Screen::Media)
            || self.media.mode == GraphicsMode::HalfBlock
        {
            self.companion.blast = None;
            return;
        }
        if !self.focused {
            return;
        }
        let Some(rect) = self.companion_viewport.get() else {
            self.companion.blast = None;
            return;
        };
        let mode = self.media.mode;
        self.companion.build_blast(mode, rect);
    }

    /// The blast the shell should write after this frame's
    /// draw, when its signature is new. Composes the Media
    /// blast and the companion blast (D-639): the text
    /// carries only the components whose signatures changed
    /// since the last composed blast, except that a changed
    /// Media blast always re-places the companion after
    /// itself — the Media kitty blast opens by deleting
    /// every placed image, the companion's included. The
    /// composed signature covers both components, so the
    /// shell's change check stays exact.
    pub fn graphics_blast(&self) -> Option<GraphicsBlast> {
        let media_sig = self.media.blast.as_ref().map_or(0, |(sig, _)| *sig);
        let comp_sig = self.companion.blast.as_ref().map_or(0, |(sig, _)| *sig);
        if media_sig == 0 && comp_sig == 0 {
            self.blast_stage.set((0, 0));
            return None;
        }
        let (staged_media, staged_comp) = self.blast_stage.get();
        let media_text = self.media.blast.as_ref().map(|(_, text)| text.as_str());
        let comp_text = self.companion.blast.as_ref().map(|(_, text)| text.as_str());
        let dropped = (staged_media != 0 && media_sig == 0) || (staged_comp != 0 && comp_sig == 0);
        let mut text = String::new();
        if dropped && self.media.mode == GraphicsMode::Kitty {
            // A component vanished while the other survives:
            // delete every placed image, then re-place the
            // survivor — nothing lingers, nothing is lost.
            text.push_str(crate::media::kitty_delete());
            if let Some(media_text) = media_text {
                // The Media text opens with its own delete;
                // a second delete is a no-op, and the full
                // text re-places the image exactly.
                text.push_str(media_text);
            }
            if let Some(comp_text) = comp_text {
                text.push_str(comp_text);
            }
        } else if media_sig != staged_media {
            if let Some(media_text) = media_text {
                text.push_str(media_text);
            }
            if let Some(comp_text) = comp_text {
                text.push_str(comp_text);
            }
        } else if comp_sig != staged_comp {
            if let Some(comp_text) = comp_text {
                text.push_str(comp_text);
            }
        }
        self.blast_stage.set((media_sig, comp_sig));
        let mut signature: u64 = 0xcbf2_9ce4_8422_2325;
        for value in [media_sig, comp_sig] {
            signature ^= value;
            signature = signature.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Some(GraphicsBlast { signature, text })
    }

    fn media_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.media.blast = None;
                if let Some(video) = &mut self.media.video {
                    video.playing = false;
                }
                self.screen = if self.open.is_some() {
                    Screen::Browse
                } else {
                    Screen::Projects
                };
            }
            KeyCode::Char('o') => {
                self.prompt = Some((PromptAction::MediaPath, TextBuf::new()));
            }
            KeyCode::Enter => self.media_load(),
            KeyCode::Char(' ') => {
                if let Some(video) = &mut self.media.video {
                    video.playing = !video.playing;
                    video.last_tick = None;
                    self.media_refresh_info();
                }
            }
            KeyCode::Left => self.media_step(-1),
            KeyCode::Right => self.media_step(1),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.media.zoom = (self.media.zoom + 0.25).min(3.0);
                self.refresh_blast();
            }
            KeyCode::Char('-') => {
                self.media.zoom = (self.media.zoom - 0.25).max(0.25);
                self.refresh_blast();
            }
            _ => {
                let count = self.media.items.len();
                if nav(&mut self.media.sel, count, key) {
                    self.media_load();
                }
            }
        }
    }

    /// Loads one prompted media path as the media list's sole
    /// item (any previewable file, project or not).
    pub fn media_open_path(&mut self, path: &Path) {
        match crate::media::classify(path) {
            Some(kind) if path.is_file() => {
                self.media.items = vec![MediaItem {
                    label: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    kind,
                    path: path.to_path_buf(),
                }];
                self.media.sel = 0;
                self.media_load();
            }
            Some(_) => {
                self.status = format!("no file at {}", path.display());
            }
            None => {
                self.status = format!(
                    "{} is not previewable media (image, .inx/.inp model, video)",
                    path.display()
                );
            }
        }
    }

    // -- Files / Editor ----------------------------------------------

    fn files_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Browse,
            KeyCode::Char('r') => self.refresh_files(),
            KeyCode::Enter => self.open_editor(),
            _ => {
                let count = self.files.len();
                nav(&mut self.file_sel, count, key);
            }
        }
    }

    fn open_editor(&mut self) {
        let Some(open) = &self.open else { return };
        let Some(row) = self.files.get(self.file_sel) else {
            return;
        };
        let path = open.root.join(&row.name);
        let (text, exists) = match std::fs::read_to_string(&path) {
            Ok(text) => (text, true),
            Err(_) => (String::new(), false),
        };
        let buf = TextBuf::from_text(&text);
        let beyond_bounds = exists && buf.text() != text;
        self.editor = Some(EditorState {
            name: row.name.clone(),
            path,
            dirty: false,
            diagnostic: if beyond_bounds {
                Some("file exceeds the editor's bounds; opened read-only".to_string())
            } else {
                None
            },
            readonly: !row.editable || beyond_bounds,
            buf,
            confirm_discard: false,
        });
        self.screen = Screen::Editor;
    }

    fn editor_key(&mut self, key: KeyEvent) {
        let Some((confirm_discard, dirty)) = self
            .editor
            .as_ref()
            .map(|editor| (editor.confirm_discard, editor.dirty))
        else {
            self.screen = Screen::Files;
            return;
        };
        if confirm_discard {
            match key.code {
                KeyCode::Char('y') => {
                    self.editor = None;
                    self.screen = Screen::Files;
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    if let Some(editor) = &mut self.editor {
                        editor.confirm_discard = false;
                    }
                }
                _ => {}
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('s') => self.save_editor(),
                KeyCode::Char('u') => {
                    if let Some(editor) = &mut self.editor {
                        if !editor.readonly {
                            editor.buf.clear();
                            editor.dirty = true;
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => {
                if dirty {
                    if let Some(editor) = &mut self.editor {
                        editor.confirm_discard = true;
                    }
                } else {
                    self.editor = None;
                    self.screen = Screen::Files;
                }
            }
            _ => {
                if let Some(editor) = &mut self.editor {
                    if !editor.readonly {
                        let before = editor.buf.text();
                        text_key(&mut editor.buf, key);
                        if editor.buf.text() != before {
                            editor.dirty = true;
                        }
                    } else {
                        nav_keys_only(&mut editor.buf, key);
                    }
                }
            }
        }
    }

    /// Saves the editor's file through its parse gate. A refused
    /// gate leaves the file byte-identical and the buffer dirty,
    /// with the refusal as the editor's diagnostic.
    pub fn save_editor(&mut self) {
        let Some(editor) = &self.editor else { return };
        let name = editor.name.clone();
        let path = editor.path.clone();
        let text = editor.buf.text();
        if editor.readonly {
            self.status = format!("{name} is read-only here");
            return;
        }
        let gate = if name == torogao_author::store::PROJECT_FILE {
            torogao_author::codec::parse_project(&text).map(|_| ())
        } else if name == "content.ron" {
            torogao_author::ron::parse_document(&text).map(|_| ())
        } else {
            Ok(())
        };
        if let Err(refusal) = gate {
            if let Some(editor) = &mut self.editor {
                editor.diagnostic = Some(format!("{}: {}", refusal.code(), refusal.message));
            }
            self.status = format!("save refused for {name} — file unchanged");
            return;
        }
        match write_atomic(&path, &text) {
            Ok(()) => {
                if let Some(editor) = &mut self.editor {
                    editor.dirty = false;
                    editor.diagnostic = None;
                }
                self.status = format!("saved {name}");
                if name == torogao_author::store::PROJECT_FILE {
                    self.reload_project();
                }
                self.refresh_files();
            }
            Err(err) => {
                self.status = format!("cannot write {}: {err}", path.display());
            }
        }
    }

    // -- Settings / prompt -------------------------------------------

    fn settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.screen = if self.open.is_some() {
                    Screen::Browse
                } else {
                    Screen::Projects
                };
            }
            KeyCode::Char('a') => {
                self.accent = match self.accent {
                    Accent::Pink => Accent::Mono,
                    Accent::Mono => Accent::Pink,
                };
                self.save_settings();
                self.status = "accent register toggled (saved)".to_string();
            }
            KeyCode::Char('c') => {
                self.companion_on = !self.companion_on;
                if !self.companion_on {
                    self.companion.blast = None;
                }
                self.save_settings();
                self.status = format!(
                    "companion pane {} (saved)",
                    if self.companion_on { "on" } else { "off" }
                );
            }
            KeyCode::Char('d') => {
                self.decorations_on = !self.decorations_on;
                self.save_settings();
                self.status = format!(
                    "decorations {} (saved)",
                    if self.decorations_on { "on" } else { "off" }
                );
            }
            KeyCode::Char('p') => {
                let mut buf = TextBuf::new();
                for ch in self.projects_root.display().to_string().chars() {
                    buf.insert_char(ch);
                }
                buf.move_top();
                self.prompt = Some((PromptAction::ProjectsRoot, buf));
            }
            _ => {}
        }
    }

    /// Persists accent + projects root + the companion and
    /// decorations toggles to the settings file
    /// (`$XDG_CONFIG_HOME/torogao/tui-settings.txt`, else
    /// `~/.config/torogao/tui-settings.txt`). Best-effort: a
    /// failed write is reported on the status line, never hidden.
    pub fn save_settings(&self) {
        let Some(path) = settings_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&path, self.settings_text()).is_err() {
            // The caller surfaces state; the shell reports at exit.
        }
    }

    /// The settings file's full text for the current state:
    /// the pure half of the round-trip the settings tests
    /// exercise.
    fn settings_text(&self) -> String {
        format!(
            "accent = {}\nprojects_root = {}\ncompanion = {}\ndecorations = {}\n",
            match self.accent {
                Accent::Pink => "pink",
                Accent::Mono => "mono",
            },
            self.projects_root.display(),
            if self.companion_on { "on" } else { "off" },
            if self.decorations_on { "on" } else { "off" },
        )
    }

    /// Loads the persisted settings, when a settings file
    /// exists. Absent or unknown keys keep their defaults —
    /// the toggles default on, matching the file's
    /// absence-tolerant loading.
    pub fn load_settings(&mut self) {
        let Some(path) = settings_path() else { return };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        self.apply_settings_text(&text);
    }

    /// Applies parsed settings text: the pure half of the
    /// load path.
    fn apply_settings_text(&mut self, text: &str) {
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("accent = ") {
                self.accent = match value.trim() {
                    "mono" => Accent::Mono,
                    _ => Accent::Pink,
                };
            } else if let Some(value) = line.strip_prefix("companion = ") {
                self.companion_on = value.trim() != "off";
            } else if let Some(value) = line.strip_prefix("decorations = ") {
                self.decorations_on = value.trim() != "off";
            }
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Enter => {
                let Some((action, buf)) = self.prompt.take() else {
                    return;
                };
                let value = buf.text().trim().to_string();
                match action {
                    PromptAction::NewProject => {
                        if value.is_empty() {
                            self.status = "project name is empty".to_string();
                        } else {
                            self.create_project(&value);
                        }
                    }
                    PromptAction::OpenPath => {
                        if !value.is_empty() {
                            self.open_project(Path::new(&value));
                        }
                    }
                    PromptAction::MediaPath => {
                        if !value.is_empty() {
                            self.media_open_path(Path::new(&value));
                        }
                    }
                    PromptAction::ProjectsRoot => {
                        if !value.is_empty() {
                            self.projects_root = PathBuf::from(value);
                            self.open = None;
                            self.screen = Screen::Projects;
                            self.refresh_projects();
                            self.save_settings();
                        }
                    }
                }
            }
            _ => {
                if let Some((_, buf)) = &mut self.prompt {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('u')
                    {
                        buf.clear();
                    } else {
                        text_key(buf, key);
                    }
                }
            }
        }
    }
}

/// The manifest's verb rows: implemented first, then deferred —
/// each in the order `verbs_doc` reports.
fn verb_rows() -> Vec<VerbRow> {
    let doc = torogao_author::verbs_doc();
    let mut rows = Vec::new();
    if let Some(list) = doc["implemented"].as_array() {
        for row in list {
            rows.push(VerbRow {
                name: row["verb"].as_str().unwrap_or("").to_string(),
                group: row["group"].as_str().unwrap_or("").to_string(),
                mutating: row["mutating"].as_bool().unwrap_or(false),
                deferred: None,
            });
        }
    }
    if let Some(list) = doc["deferred"].as_array() {
        for row in list {
            rows.push(VerbRow {
                name: row["verb"].as_str().unwrap_or("").to_string(),
                group: row["group"].as_str().unwrap_or("").to_string(),
                mutating: false,
                deferred: Some(row["slice"].as_str().unwrap_or("later").to_string()),
            });
        }
    }
    rows
}

/// Applies j/k/arrow/g/G navigation to `sel` over `len` rows.
/// Returns true when the selection moved.
fn nav(sel: &mut usize, len: usize, key: KeyEvent) -> bool {
    if len == 0 {
        return false;
    }
    let before = *sel;
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => *sel = (*sel + 1).min(len - 1),
        KeyCode::Char('k') | KeyCode::Up => *sel = sel.saturating_sub(1),
        KeyCode::Char('g') => *sel = 0,
        KeyCode::Char('G') => *sel = len - 1,
        KeyCode::PageDown => *sel = (*sel + 10).min(len - 1),
        KeyCode::PageUp => *sel = sel.saturating_sub(10),
        _ => {}
    }
    *sel != before
}

/// Applies one key to a text buffer: printable chars, Enter,
/// Backspace/Delete, and cursor motion. Ctrl combos are the
/// caller's business and never reach here.
fn text_key(buf: &mut TextBuf, key: KeyEvent) {
    match key.code {
        KeyCode::Char(ch) => {
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                buf.insert_char(ch);
            }
        }
        KeyCode::Enter => {
            buf.insert_newline();
        }
        KeyCode::Backspace => buf.backspace(),
        KeyCode::Delete => buf.delete(),
        KeyCode::Left => buf.move_left(),
        KeyCode::Right => buf.move_right(),
        KeyCode::Up => buf.move_up(),
        KeyCode::Down => buf.move_down(),
        KeyCode::Home => buf.move_home(),
        KeyCode::End => buf.move_end(),
        _ => {}
    }
}

/// Cursor motion only, for read-only buffers.
fn nav_keys_only(buf: &mut TextBuf, key: KeyEvent) {
    match key.code {
        KeyCode::Left => buf.move_left(),
        KeyCode::Right => buf.move_right(),
        KeyCode::Up => buf.move_up(),
        KeyCode::Down => buf.move_down(),
        KeyCode::Home => buf.move_home(),
        KeyCode::End => buf.move_end(),
        _ => {}
    }
}

/// Pretty-prints a JSON document for the answer/detail panes.
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Formats a seconds count as `m:ss.s` for the transport.
fn format_clock(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total / 60.0).floor() as u64;
    let rest = total - minutes as f64 * 60.0;
    format!("{minutes}:{rest:04.1}")
}

/// A small stable hash (FNV-1a) for blast signatures.
fn hash_of(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The status line's label for a verb outcome.
pub fn status_label(status: Status) -> &'static str {
    match status {
        Status::Ok => "ok",
        Status::Findings => "findings",
        Status::Refused => "refused",
        Status::Unloadable => "unloadable",
    }
}

/// Writes `text` to `path` atomically: temp file in the same
/// directory, then rename over the target.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp-tui");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The settings file path.
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("torogao").join("tui-settings.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app over an empty temp projects root.
    fn temp_app() -> App {
        let root =
            std::env::temp_dir().join(format!("torogao-tui-app-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        App::new(root)
    }

    #[test]
    fn settings_round_trip_both_toggles() {
        let mut app = temp_app();
        app.accent = Accent::Mono;
        app.companion_on = false;
        app.decorations_on = false;
        let text = app.settings_text();
        let mut back = temp_app();
        assert!(back.companion_on && back.decorations_on);
        back.apply_settings_text(&text);
        assert_eq!(back.accent, Accent::Mono);
        assert!(!back.companion_on);
        assert!(!back.decorations_on);
        // And back on again.
        app.companion_on = true;
        app.decorations_on = true;
        back.apply_settings_text(&app.settings_text());
        assert!(back.companion_on && back.decorations_on);
    }

    #[test]
    fn settings_absent_toggle_keys_default_on() {
        // A fresh app defaults both toggles on; a pre-slice
        // settings file carrying neither key leaves them on.
        let mut app = temp_app();
        assert!(app.companion_on && app.decorations_on);
        app.apply_settings_text("accent = pink\nprojects_root = /tmp/x\n");
        assert!(app.companion_on);
        assert!(app.decorations_on);
    }

    #[test]
    fn tick_advances_companion_only_where_it_lives() {
        let mut app = temp_app();
        app.screen = Screen::Projects;
        let start = app.companion.frame;
        app.tick();
        assert_eq!(
            app.companion.frame,
            (start + 1) % crate::companion::FRAME_COUNT
        );
        // A work screen: no advance.
        app.screen = Screen::Console;
        let still = app.companion.frame;
        app.tick();
        assert_eq!(app.companion.frame, still);
        // Focus loss freezes the advance on a companion screen.
        app.screen = Screen::Media;
        app.set_focused(false);
        app.tick();
        assert_eq!(app.companion.frame, still);
        // The toggle stops it too.
        app.set_focused(true);
        app.companion_on = false;
        app.tick();
        assert_eq!(app.companion.frame, still);
        // And it resumes when everything is back.
        app.companion_on = true;
        app.tick();
        assert_eq!(
            app.companion.frame,
            (still + 1) % crate::companion::FRAME_COUNT
        );
    }
}
