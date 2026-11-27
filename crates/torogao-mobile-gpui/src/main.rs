//! Torogao Mobile desktop shell (GPUI, Linux first).
//!
//! The desktop companion to the Slint phone shell: the same
//! feature set over the same shell-agnostic core — a [`Session`]
//! owns the attestation, seats, recipe, queue, and gallery, and
//! this shell renders them: 18+ gate, cast picker, scene prompt,
//! generation queue with cancel, gallery with delete/export,
//! seat save/test, bounded recipe settings, and the helper
//! panel (Prima: seat setup, Lana: generation, Sirena: gallery)
//! rendering the core helper state machine with admitted stills.
//! The phone deliverable is the Slint shell (see FEASIBILITY.md);
//! this shell's helpers use stills where the phone uses the
//! code-stepped bob — the states, lines, and proposals are the
//! same core machine.
//!
//! Text input is the minimal honest version of the GPUI examples'
//! pattern: one focus handle on the root, an active-field marker,
//! keystroke handling for printable characters (via `key_char`)
//! plus backspace / delete / arrows / home / end, and a block
//! cursor rendered in the field. No selection, no IME composition
//! — named as a gap, not hidden.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    App, Application, Bounds, Context, FocusHandle, InteractiveElement, KeyDownEvent, MouseButton,
    ParentElement, Render, Styled, Window, WindowBounds, WindowOptions, div, img, prelude::*, px,
    rgb, size,
};
use torogao_mobile_core::cast::CAST;
use torogao_mobile_core::helper::{
    FlowId, GateContext, Helper, HelperArea, HelperEvent, ProposedAction,
};
use torogao_mobile_core::queue::JobStatus;
use torogao_mobile_core::{RecipeSettings, SeatConfig, Session};

/// Helper stills: admitted cast material, embedded and written
/// to the data dir on first run so GPUI can load them by path.
/// Provenance is recorded in the release RECORD (Step 2B).
const PRIMA_PNG: &[u8] = include_bytes!("assets/prima.png");
const LANA_PNG: &[u8] = include_bytes!("assets/lana.png");
const SIRENA_PNG: &[u8] = include_bytes!("assets/sirena.png");

/// Which text field the keyboard edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Scene,
    SeatName,
    SeatUrl,
    SeatToken,
    Steps,
    Cfg,
    Width,
    Height,
}

const FIELD_ORDER: [Field; 8] = [
    Field::Scene,
    Field::SeatName,
    Field::SeatUrl,
    Field::SeatToken,
    Field::Steps,
    Field::Cfg,
    Field::Width,
    Field::Height,
];

/// A seat form's token text as a settings value: blank means
/// "no token", never an empty-string token.
fn nonempty_token(text: &str) -> Option<String> {
    let t = text.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

/// One editable text buffer with a byte-index cursor kept on a
/// char boundary at all times.
#[derive(Debug, Clone)]
struct TextBuf {
    text: String,
    /// Byte index into `text`, always a char boundary.
    cursor: usize,
}

impl TextBuf {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cursor: text.len(),
        }
    }

    fn insert(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.text[..self.cursor]
            .char_indices()
            .last()
            .map_or(0, |(i, _)| i);
        self.text.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let next = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map_or(self.text.len(), |(i, _)| self.cursor + i);
        self.text.replace_range(self.cursor..next, "");
    }

    fn left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .last()
                .map_or(0, |(i, _)| i);
        }
    }

    fn right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.text[self.cursor..]
                .char_indices()
                .nth(1)
                .map_or(self.text.len(), |(i, _)| self.cursor + i);
        }
    }

    /// (before-cursor, cursor-char-or-space, after) for rendering.
    fn split_at_cursor(&self) -> (String, String, String) {
        let before = self.text[..self.cursor].to_string();
        let mut chars = self.text[self.cursor..].chars();
        match chars.next() {
            Some(c) => (before, c.to_string(), chars.as_str().to_string()),
            None => (before, " ".to_string(), String::new()),
        }
    }
}

/// The shell's single view: the whole screen stack in one entity,
/// like the Slint shell's single AppWindow.
struct Shell {
    focus: FocusHandle,
    session: Option<Arc<Session>>,
    helper: Mutex<Helper>,
    selected: usize,
    active: Field,
    scene: TextBuf,
    seat_name: TextBuf,
    seat_url: TextBuf,
    seat_token: TextBuf,
    steps: TextBuf,
    cfg: TextBuf,
    width: TextBuf,
    height: TextBuf,
    status: String,
    seat_status: String,
    preview: Option<PathBuf>,
    assets_dir: PathBuf,
}

impl Shell {
    fn new(cx: &mut Context<Self>) -> Self {
        let data_dir = data_dir();
        let assets_dir = data_dir.join("helper-assets");
        write_assets(&assets_dir);
        let (session, status) =
            match Session::open(&data_dir, "http://127.0.0.1:8190", "Primo (this device)") {
                Ok(s) => {
                    let n = s.gallery().list().map(|v| v.len()).unwrap_or(0);
                    let status = if s.is_attested() {
                        format!("{n} still(s) in the gallery.")
                    } else {
                        "Torogao Mobile is adults-only: confirm the 18+ gate to begin.".to_string()
                    };
                    (Some(Arc::new(s)), status)
                }
                Err(e) => (None, format!("Settings could not be loaded: {e}")),
            };
        let (seat_name, seat_url, seat_token, steps, cfg, width, height, preview) = match &session {
            Some(s) => {
                let settings = s.settings();
                let (name, url, token) = settings
                    .active_seat()
                    .map(|seat| {
                        (
                            seat.name.clone(),
                            seat.base_url.clone(),
                            seat.token.clone().unwrap_or_default(),
                        )
                    })
                    .unwrap_or_default();
                let preview = s
                    .gallery()
                    .list()
                    .ok()
                    .and_then(|entries| entries.last().map(|e| s.gallery().path_of(e)));
                (
                    name,
                    url,
                    token,
                    settings.recipe.steps.to_string(),
                    settings.recipe.cfg.to_string(),
                    settings.recipe.width.to_string(),
                    settings.recipe.height.to_string(),
                    preview,
                )
            }
            None => (
                String::new(),
                String::new(),
                String::new(),
                "28".to_string(),
                "6.0".to_string(),
                "832".to_string(),
                "1344".to_string(),
                None,
            ),
        };
        Self {
            focus: cx.focus_handle(),
            session,
            helper: Mutex::new(Helper::new()),
            selected: 0,
            active: Field::Scene,
            scene: TextBuf::new(""),
            seat_name: TextBuf::new(&seat_name),
            seat_url: TextBuf::new(&seat_url),
            seat_token: TextBuf::new(&seat_token),
            steps: TextBuf::new(&steps),
            cfg: TextBuf::new(&cfg),
            width: TextBuf::new(&width),
            height: TextBuf::new(&height),
            status,
            seat_status: String::new(),
            preview,
            assets_dir,
        }
    }

    fn gate(&self) -> GateContext {
        GateContext {
            attested: self.session.as_ref().is_some_and(|s| s.is_attested()),
        }
    }

    fn feed(&self, event: HelperEvent) {
        let gate = self.gate();
        if let Ok(mut helper) = self.helper.lock() {
            helper.on_event(event, &gate);
        }
    }

    fn buf_mut(&mut self) -> &mut TextBuf {
        match self.active {
            Field::Scene => &mut self.scene,
            Field::SeatName => &mut self.seat_name,
            Field::SeatUrl => &mut self.seat_url,
            Field::SeatToken => &mut self.seat_token,
            Field::Steps => &mut self.steps,
            Field::Cfg => &mut self.cfg,
            Field::Width => &mut self.width,
            Field::Height => &mut self.height,
        }
    }

    fn buf(&self, which: Field) -> &TextBuf {
        match which {
            Field::Scene => &self.scene,
            Field::SeatName => &self.seat_name,
            Field::SeatUrl => &self.seat_url,
            Field::SeatToken => &self.seat_token,
            Field::Steps => &self.steps,
            Field::Cfg => &self.cfg,
            Field::Width => &self.width,
            Field::Height => &self.height,
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        if ks.modifiers.control || ks.modifiers.alt || ks.modifiers.platform {
            return;
        }
        match ks.key.as_str() {
            "backspace" => self.buf_mut().backspace(),
            "delete" => self.buf_mut().delete(),
            "left" => self.buf_mut().left(),
            "right" => self.buf_mut().right(),
            "home" => self.buf_mut().cursor = 0,
            "end" => {
                let b = self.buf_mut();
                b.cursor = b.text.len();
            }
            "tab" => {
                let pos = FIELD_ORDER
                    .iter()
                    .position(|f| *f == self.active)
                    .unwrap_or(0);
                self.active = FIELD_ORDER[(pos + 1) % FIELD_ORDER.len()];
            }
            "enter" | "return" => {
                if self.active == Field::Scene {
                    self.start_generate(cx);
                }
            }
            _ => {
                if let Some(ch) = ks.key_char.as_deref() {
                    for c in ch.chars() {
                        if !c.is_control() {
                            self.buf_mut().insert(c);
                        }
                    }
                }
            }
        }
        cx.notify();
    }

    fn start_generate(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let character = CAST[self.selected];
        let scene = self.scene.text.trim().to_string();
        match session.enqueue(character.id, &scene, None) {
            Ok(id) => {
                self.status = format!("Queued job #{id} on the seat.");
                self.watch_job(cx, id);
            }
            Err(e) => {
                let is_gate = matches!(e, torogao_mobile_core::GenError::AgeGate);
                self.status = format!("{e}");
                if is_gate {
                    self.feed(HelperEvent::AttestationMissing);
                }
            }
        }
        cx.notify();
    }

    /// Poll one job to its terminal state, refreshing the
    /// preview and feeding the helper machine on the way.
    fn watch_job(&self, cx: &mut Context<Self>, job_id: u64) {
        let Some(session) = self.session.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            loop {
                let job = cx
                    .background_spawn({
                        let session = Arc::clone(&session);
                        async move { session.jobs().into_iter().find(|j| j.id == job_id) }
                    })
                    .await;
                let Some(job) = job else { break };
                let terminal = matches!(
                    job.status,
                    JobStatus::Done | JobStatus::Failed(_) | JobStatus::Cancelled
                );
                let done_entry = if job.status == JobStatus::Done {
                    job.entry.clone()
                } else {
                    None
                };
                let failed = matches!(job.status, JobStatus::Failed(_));
                let status_line = match &job.status {
                    JobStatus::Queued => format!("Job #{job_id}: queued."),
                    JobStatus::Running => format!("Job #{job_id}: generating at the seat…"),
                    JobStatus::Done => format!(
                        "Job #{job_id}: filed {} (seed {}).",
                        job.entry.as_ref().map_or("?", |e| e.file.as_str()),
                        job.entry.as_ref().map_or(0, |e| e.seed)
                    ),
                    JobStatus::Failed(m) => format!("Job #{job_id}: {m}"),
                    JobStatus::Cancelled => format!("Job #{job_id}: cancelled."),
                };
                let session2 = Arc::clone(&session);
                let _ = this.update(cx, |this, cx| {
                    this.status = status_line;
                    if let Some(entry) = &done_entry {
                        this.preview = Some(session2.gallery().path_of(entry));
                    }
                    if terminal {
                        if done_entry.is_some() {
                            this.feed(HelperEvent::JobSucceeded);
                        } else if failed {
                            this.feed(HelperEvent::JobFailed);
                        }
                    }
                    cx.notify();
                });
                if terminal {
                    break;
                }
                cx.background_spawn(async {
                    std::thread::sleep(Duration::from_secs(2));
                })
                .await;
            }
        })
        .detach();
    }

    fn helper_confirm(&mut self, cx: &mut Context<Self>) {
        let action = self.helper.lock().ok().and_then(|h| h.proposed_action());
        let Some(session) = self.session.clone() else {
            return;
        };
        match action {
            Some(ProposedAction::UseDefaultSeat) => {
                self.seat_name = TextBuf::new("Primo (this device)");
                self.seat_url = TextBuf::new("http://127.0.0.1:8190");
                self.feed(HelperEvent::ConfirmStep);
            }
            Some(ProposedAction::TestSeatConnection) => {
                self.feed(HelperEvent::ConfirmStep);
                let session = Arc::clone(&session);
                cx.spawn(async move |this, cx| {
                    let _ = cx
                        .background_spawn({
                            let s = Arc::clone(&session);
                            async move { s.wake_seat() }
                        })
                        .await;
                    let mut answered = false;
                    for _ in 0..10 {
                        let status = cx
                            .background_spawn({
                                let s = Arc::clone(&session);
                                async move { s.seat_status() }
                            })
                            .await;
                        if !matches!(status, torogao_mobile_core::SeatStatus::Unreachable(_)) {
                            answered = true;
                            break;
                        }
                        cx.background_spawn(async {
                            std::thread::sleep(Duration::from_secs(3));
                        })
                        .await;
                    }
                    let _ = this.update(cx, |this, cx| {
                        if answered {
                            this.feed(HelperEvent::SeatTestPassed);
                            this.seat_status = "The seat answered.".to_string();
                        } else {
                            this.feed(HelperEvent::SeatLost);
                            this.seat_status = "No answer from the seat.".to_string();
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            Some(ProposedAction::SaveSeatAndFinish) => {
                let seat = SeatConfig {
                    name: self.seat_name.text.trim().to_string(),
                    base_url: self.seat_url.text.trim().to_string(),
                    token: nonempty_token(&self.seat_token.text),
                };
                match session.upsert_seat(seat) {
                    Ok(i) => {
                        let _ = session.set_active_seat(i);
                        self.status = "Seat saved and active.".to_string();
                        self.feed(HelperEvent::ConfirmStep);
                    }
                    Err(e) => self.status = format!("{e}"),
                }
            }
            Some(ProposedAction::RunGeneration) => {
                self.start_generate(cx);
                // start_generate enqueued (or refused); move the
                // flow to waiting only when a job actually went in.
                if self.status.starts_with("Queued job") {
                    self.feed(HelperEvent::ConfirmStep);
                }
            }
            Some(ProposedAction::OpenGallery) => {
                self.feed(HelperEvent::Dismiss);
                self.feed(HelperEvent::EnterArea(HelperArea::Gallery));
            }
            None => {}
        }
        cx.notify();
    }
}

/// Write the embedded helper stills into `dir` (first run).
fn write_assets(dir: &PathBuf) {
    let _ = std::fs::create_dir_all(dir);
    for (name, bytes) in [
        ("prima.png", PRIMA_PNG),
        ("lana.png", LANA_PNG),
        ("sirena.png", SIRENA_PNG),
    ] {
        let path = dir.join(name);
        if !path.exists() {
            let _ = std::fs::write(&path, bytes);
        }
    }
}

/// Data root: `$TOROGAO_MOBILE_DATA`, else
/// `~/.local/share/torogao-mobile` (the same root the Slint host
/// binary uses, so both shells share one gallery on a desktop).
fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("TOROGAO_MOBILE_DATA") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".local/share/torogao-mobile")
}

fn button(label: &str, primary: bool) -> gpui::Div {
    div()
        .px(px(14.))
        .py(px(7.))
        .rounded(px(16.))
        .bg(if primary {
            rgb(0x6d5bd0)
        } else {
            rgb(0x2c2440)
        })
        .text_color(if primary {
            rgb(0xffffff)
        } else {
            rgb(0xcfc6e8)
        })
        .child(label.to_string())
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let attested = self.session.as_ref().is_some_and(|s| s.is_attested());
        let (helper_name, helper_line, helper_action, helper_area) = match self.helper.lock() {
            Ok(h) => (
                h.character().name.to_string(),
                h.line().to_string(),
                h.proposed_action(),
                h.area(),
            ),
            Err(_) => ("".to_string(), "".to_string(), None, HelperArea::SeatSetup),
        };
        let helper_img = self.assets_dir.join(match helper_area {
            HelperArea::SeatSetup => "prima.png",
            HelperArea::Generation => "lana.png",
            HelperArea::Gallery => "sirena.png",
        });

        let mut root = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event, _window, cx| {
                this.on_key_down(event, cx);
            }))
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(16.))
            .size_full()
            .bg(rgb(0x14101c))
            .text_color(rgb(0xe8e2f2))
            .child(div().text_2xl().child("Torogao Mobile"));

        // Helper card.
        let mut card = div()
            .flex()
            .flex_row()
            .gap(px(10.))
            .p(px(10.))
            .rounded(px(10.))
            .bg(rgb(0x1e1830))
            .child(img(helper_img).h(px(120.)).rounded(px(6.)));
        let mut card_text = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(div().text_color(rgb(0xcfc6e8)).child(helper_name))
            .child(div().text_sm().max_w(px(300.)).child(helper_line));
        if let Some(action) = helper_action {
            let label = match action {
                ProposedAction::UseDefaultSeat => "Use the usual seat",
                ProposedAction::TestSeatConnection => "Test the connection",
                ProposedAction::SaveSeatAndFinish => "Save this seat",
                ProposedAction::RunGeneration => "Cue it on the seat",
                ProposedAction::OpenGallery => "Open the gallery",
            };
            card_text = card_text.child(button(label, true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e, _w, cx| this.helper_confirm(cx)),
            ));
        }
        card = card.child(card_text);
        root = root.child(card);

        if !attested {
            // The 18+ gate.
            return root
                .child(div().text_lg().child("Adults only"))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xbdb2d9))
                        .max_w(px(400.))
                        .child(
                            "Torogao Mobile generates adult artwork with the cast of Torogao. \
                         You must be 18 or older to use it. Your confirmation is saved on \
                         this device and gates all generation.",
                        ),
                )
                .child(button("I am 18 or older", true).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _e, _w, cx| {
                        if let Some(session) = &this.session {
                            match session.attest() {
                                Ok(()) => {
                                    this.status = "Attestation saved. Welcome in.".to_string()
                                }
                                Err(e) => this.status = format!("Could not save: {e}"),
                            }
                        }
                        cx.notify();
                    }),
                ))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xbdb2d9))
                        .child(self.status.clone()),
                );
        }

        // Guided-flow starters.
        root = root.child(
            div()
                .flex()
                .flex_row()
                .gap(px(8.))
                .child(button("Guided seat setup", false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _e, _w, cx| {
                        this.feed(HelperEvent::BeginFlow(FlowId::SeatSetup));
                        cx.notify();
                    }),
                ))
                .child(button("Guided first still", false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _e, _w, cx| {
                        this.feed(HelperEvent::BeginFlow(FlowId::FirstGeneration));
                        cx.notify();
                    }),
                )),
        );

        // Cast chips.
        let selected = self.selected;
        let mut chips = div().flex().flex_row().gap(px(8.));
        for (i, member) in CAST.iter().enumerate() {
            chips = chips.child(button(member.name, i == selected).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _e, _w, cx| {
                    this.selected = i;
                    this.feed(HelperEvent::EnterArea(HelperArea::Generation));
                    cx.notify();
                }),
            ));
        }
        root = root
            .child(div().text_sm().text_color(rgb(0x9a8fb5)).child("Cast"))
            .child(chips);

        // Fields.
        root = root.child(self.field(Field::Scene, "Scene", cx));
        root = root.child(self.field(Field::SeatName, "Seat name", cx));
        root = root.child(self.field(Field::SeatUrl, "Seat address (the facade)", cx));
        root = root.child(self.field(Field::SeatToken, "Seat token (facade gate)", cx));

        // Seat actions.
        root = root.child(
            div()
                .flex()
                .flex_row()
                .gap(px(8.))
                .child(button("Save seat", false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _e, _w, cx| {
                        if let Some(session) = &this.session {
                            let seat = SeatConfig {
                                name: this.seat_name.text.trim().to_string(),
                                base_url: this.seat_url.text.trim().to_string(),
                                token: nonempty_token(&this.seat_token.text),
                            };
                            match session.upsert_seat(seat) {
                                Ok(i) => {
                                    let _ = session.set_active_seat(i);
                                    this.status = "Seat saved and active.".to_string();
                                }
                                Err(e) => this.status = format!("{e}"),
                            }
                        }
                        cx.notify();
                    }),
                ))
                .child(button("Test seat", false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _e, _w, cx| {
                        let Some(session) = &this.session else { return };
                        let session = Arc::clone(session);
                        this.seat_status = "Testing the seat…".to_string();
                        cx.spawn(async move |this, cx| {
                            let status = cx
                                .background_spawn(async move { session.seat_status() })
                                .await;
                            let _ = this.update(cx, |this, cx| {
                                this.seat_status = status.describe();
                                cx.notify();
                            });
                        })
                        .detach();
                        cx.notify();
                    }),
                )),
        );
        if !self.seat_status.is_empty() {
            root = root.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xbdb2d9))
                    .child(self.seat_status.clone()),
            );
        }

        // Recipe fields + apply.
        root = root.child(self.field(Field::Steps, "Steps (10-60)", cx));
        root = root.child(self.field(Field::Cfg, "CFG (1.0-12.0)", cx));
        root = root.child(self.field(Field::Width, "Width (512-1024)", cx));
        root = root.child(self.field(Field::Height, "Height (512-1536)", cx));
        root = root.child(button("Apply recipe", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _e, _w, cx| {
                if let Some(session) = &this.session {
                    let parsed = (
                        this.steps.text.trim().parse::<u32>(),
                        this.cfg.text.trim().parse::<f32>(),
                        this.width.text.trim().parse::<u32>(),
                        this.height.text.trim().parse::<u32>(),
                    );
                    match parsed {
                        (Ok(steps), Ok(cfg), Ok(width), Ok(height)) => {
                            match session.set_recipe(RecipeSettings {
                                steps,
                                cfg,
                                width,
                                height,
                            }) {
                                Ok(()) => this.status = "Recipe saved.".to_string(),
                                Err(e) => this.status = format!("{e}"),
                            }
                        }
                        _ => this.status = "Recipe fields must all be numbers.".to_string(),
                    }
                }
                cx.notify();
            }),
        ));

        // Generate + status.
        root = root
            .child(button("Generate", true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e, _w, cx| this.start_generate(cx)),
            ))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xbdb2d9))
                    .child(self.status.clone()),
            );

        // Jobs (newest first, live ones cancellable).
        if let Some(session) = &self.session {
            let jobs = session.jobs();
            if !jobs.is_empty() {
                let mut list = div().flex().flex_col().gap(px(2.));
                for job in jobs.iter().rev().take(6) {
                    let state = match &job.status {
                        JobStatus::Queued => "queued".to_string(),
                        JobStatus::Running => "generating…".to_string(),
                        JobStatus::Done => "done".to_string(),
                        JobStatus::Failed(m) => format!("failed: {m}"),
                        JobStatus::Cancelled => "cancelled".to_string(),
                    };
                    let mut row = div().flex().flex_row().gap(px(8.)).child(
                        div().text_sm().text_color(rgb(0xcfc6e8)).child(format!(
                            "#{} · {} · {}",
                            job.id, job.request.character_id, state
                        )),
                    );
                    if matches!(job.status, JobStatus::Queued | JobStatus::Running) {
                        let id = job.id;
                        row = row.child(button("Cancel", false).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _e, _w, cx| {
                                if let Some(session) = &this.session {
                                    session.cancel(id);
                                }
                                cx.notify();
                            }),
                        ));
                    }
                    list = list.child(row);
                }
                root = root
                    .child(div().text_sm().text_color(rgb(0x9a8fb5)).child("Jobs"))
                    .child(list);
            }
        }

        // Preview + gallery actions.
        if let Some(path) = &self.preview {
            root = root.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_sm().text_color(rgb(0x9a8fb5)).child("Latest"))
                    .child(img(path.clone()).max_h(px(300.)).rounded(px(8.))),
            );
            root = root.child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(button("Delete latest", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _e, _w, cx| {
                            if let Some(session) = &this.session {
                                let gallery = session.gallery();
                                if let Ok(entries) = gallery.list() {
                                    if let Some(entry) = entries.last().cloned() {
                                        match gallery.delete(&entry) {
                                            Ok(()) => {
                                                this.status = format!("Deleted {}", entry.file);
                                                this.preview = gallery.list().ok().and_then(|v| {
                                                    v.last().map(|e| gallery.path_of(e))
                                                });
                                            }
                                            Err(e) => this.status = format!("{e}"),
                                        }
                                    }
                                }
                            }
                            cx.notify();
                        }),
                    ))
                    .child(button("Export latest", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _e, _w, cx| {
                            if let Some(session) = &this.session {
                                let gallery = session.gallery();
                                if let Ok(entries) = gallery.list() {
                                    if let Some(entry) = entries.last() {
                                        let dest = data_dir().join("exports");
                                        match gallery.export(entry, &dest) {
                                            Ok(p) => {
                                                this.status =
                                                    format!("Exported to {}", p.to_string_lossy())
                                            }
                                            Err(e) => this.status = format!("{e}"),
                                        }
                                    }
                                }
                            }
                            cx.notify();
                        }),
                    )),
            );
        }

        // Gallery list.
        if let Some(session) = &self.session {
            if let Ok(entries) = session.gallery().list() {
                let mut list = div().flex().flex_col().gap(px(2.));
                for entry in entries.iter().rev().take(12) {
                    let who = CAST
                        .iter()
                        .find(|c| c.id == entry.character_id)
                        .map_or(entry.character_id.as_str(), |c| c.name);
                    list = list.child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xcfc6e8))
                            .child(format!("{who} — {}", truncate(&entry.prompt, 48))),
                    );
                }
                root = root
                    .child(div().text_sm().text_color(rgb(0x9a8fb5)).child("Gallery"))
                    .child(list);
            }
        }
        root
    }
}

impl Shell {
    /// One labelled text field with a block cursor when active.
    fn field(&self, which: Field, label: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let buf = self.buf(which);
        let is_active = self.active == which;
        let (before, cursor_ch, after) = buf.split_at_cursor();
        let mut text_row = div().flex().flex_row().child(before);
        if is_active {
            text_row = text_row.child(
                div()
                    .bg(rgb(0x6d5bd0))
                    .text_color(rgb(0xffffff))
                    .child(cursor_ch),
            );
        }
        text_row = text_row.child(after);
        if buf.text.is_empty() && which == Field::Scene {
            text_row = div()
                .flex()
                .flex_row()
                .text_color(rgb(0x7a7099))
                .child("e.g. city night, rain, neon signs");
            if is_active {
                text_row = div()
                    .flex()
                    .flex_row()
                    .child(div().bg(rgb(0x6d5bd0)).child(" "));
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x9a8fb5))
                    .child(label.to_string()),
            )
            .child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(rgb(0x1e1830))
                    .border_1()
                    .border_color(if is_active {
                        rgb(0x6d5bd0)
                    } else {
                        rgb(0x3a3157)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event, window, cx| {
                            this.active = which;
                            this.focus.focus(window);
                            cx.notify();
                        }),
                    )
                    .child(text_row),
            )
    }
}

/// Truncate on a char boundary with an ellipsis.
fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let cut: String = s.chars().take(max_chars).collect();
    format!("{cut}…")
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(460.), px(980.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(Shell::new),
        )
        .expect("torogao-mobile-gpui: window opens");
    });
}
