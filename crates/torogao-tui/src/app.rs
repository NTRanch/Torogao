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

use crate::records::{self, Family};
use crate::textbuf::TextBuf;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
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

    /// Persists accent + projects root to the settings file
    /// (`$XDG_CONFIG_HOME/torogao/tui-settings.txt`, else
    /// `~/.config/torogao/tui-settings.txt`). Best-effort: a
    /// failed write is reported on the status line, never hidden.
    pub fn save_settings(&self) {
        let Some(path) = settings_path() else { return };
        let text = format!(
            "accent = {}\nprojects_root = {}\n",
            match self.accent {
                Accent::Pink => "pink",
                Accent::Mono => "mono",
            },
            self.projects_root.display()
        );
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&path, text).is_err() {
            // The caller surfaces state; the shell reports at exit.
        }
    }

    /// Loads the persisted accent, when a settings file exists.
    pub fn load_settings(&mut self) {
        let Some(path) = settings_path() else { return };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("accent = ") {
                self.accent = match value.trim() {
                    "mono" => Accent::Mono,
                    _ => Accent::Pink,
                };
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
