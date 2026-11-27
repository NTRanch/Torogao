//! Torogao Mobile desktop shell (GPUI, Linux first).
//!
//! The desktop companion to the Slint phone shell: the same screens
//! over the same shell-agnostic core (`torogao-mobile-core`) — cast
//! picker (Prima / Lana / Sirena), scene prompt, ComfyUI seat URL,
//! Generate, status line, newest-still preview, and the local
//! gallery list. GPUI's genuine home is the desktop; the phone
//! deliverable is the Slint shell (see FEASIBILITY.md).
//!
//! Seat default on desktop is loopback: this shell normally runs on
//! primo itself, where the ComfyUI seat binds 127.0.0.1:8188 when
//! it is up. The seat is not a standing service (idle-reaped), so
//! "seat unreachable" is a normal status, surfaced by the core as a
//! typed error and shown verbatim.
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

use gpui::{
    App, Application, Bounds, Context, FocusHandle, InteractiveElement, KeyDownEvent,
    MouseButton, ParentElement, Render, Styled, Window, WindowBounds, WindowOptions, div, img,
    prelude::*, px, rgb, size,
};
use torogao_mobile_core::cast::{CAST, Character};
use torogao_mobile_core::client::{GenError, SeatClient};
use torogao_mobile_core::gallery::{Gallery, GalleryEntry};

/// Which text field the keyboard edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Scene,
    Seat,
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
    selected: usize,
    active: Field,
    scene: TextBuf,
    seat: TextBuf,
    busy: bool,
    status: String,
    gallery: Vec<GalleryEntry>,
    gallery_dir: PathBuf,
    preview: Option<PathBuf>,
}

impl Shell {
    fn new(cx: &mut Context<Self>) -> Self {
        let data_dir = data_dir();
        let gallery_dir = data_dir.join("gallery");
        let (gallery, status) = match Gallery::open(&gallery_dir) {
            Ok(g) => match g.list() {
                Ok(entries) => {
                    let status = if entries.is_empty() {
                        "Pick a cast member, write a scene, generate.".to_string()
                    } else {
                        format!("{} still(s) in the gallery.", entries.len())
                    };
                    (entries, status)
                }
                Err(e) => (Vec::new(), format!("Gallery error: {e}")),
            },
            Err(e) => (Vec::new(), format!("Gallery error: {e}")),
        };
        let preview = gallery
            .last()
            .map(|e| gallery_dir.join(&e.file));
        Self {
            focus: cx.focus_handle(),
            selected: 0,
            active: Field::Scene,
            scene: TextBuf::new(""),
            seat: TextBuf::new("http://127.0.0.1:8188"),
            busy: false,
            status,
            gallery,
            gallery_dir,
            preview,
        }
    }

    fn buf_mut(&mut self) -> &mut TextBuf {
        match self.active {
            Field::Scene => &mut self.scene,
            Field::Seat => &mut self.seat,
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
                self.active = match self.active {
                    Field::Scene => Field::Seat,
                    Field::Seat => Field::Scene,
                };
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
        if self.busy {
            return;
        }
        let character: Character = CAST[self.selected];
        let scene = self.scene.text.trim().to_string();
        if scene.is_empty() {
            self.status = "Write a scene first.".to_string();
            cx.notify();
            return;
        }
        let seat_url = self.seat.text.trim().to_string();
        let gallery_dir = self.gallery_dir.clone();
        self.busy = true;
        self.status = format!("Generating {} at {seat_url}…", character.name);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let gallery = Gallery::open(&gallery_dir)?;
                    SeatClient::new(&seat_url).generate(&character, &scene, None, &gallery)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(entry) => {
                        this.status = format!(
                            "Filed {} (seed {}).",
                            entry.file, entry.seed
                        );
                        this.preview = Some(this.gallery_dir.join(&entry.file));
                        if let Ok(g) = Gallery::open(&this.gallery_dir) {
                            if let Ok(entries) = g.list() {
                                this.gallery = entries;
                            }
                        }
                    }
                    Err(e) => {
                        this.status = describe_error(&e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Status-line wording per core error, mirroring the Slint shell.
fn describe_error(e: &GenError) -> String {
    match e {
        GenError::SeatUnreachable(d) => {
            format!("Seat unreachable — is ComfyUI up on primo? ({d})")
        }
        other => format!("Generation failed: {other}"),
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

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected;
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
            .child(div().text_2xl().child("Torogao Mobile"))
            .child(div().text_sm().text_color(rgb(0x9a8fb5)).child("Cast"));

        // Cast chips.
        let mut chips = div().flex().flex_row().gap(px(8.));
        for (i, member) in CAST.iter().enumerate() {
            let is_sel = i == selected;
            chips = chips.child(
                div()
                    .px(px(14.))
                    .py(px(7.))
                    .rounded(px(16.))
                    .bg(if is_sel { rgb(0x6d5bd0) } else { rgb(0x2c2440) })
                    .text_color(if is_sel { rgb(0xffffff) } else { rgb(0xcfc6e8) })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event, _window, cx| {
                            this.selected = i;
                            cx.notify();
                        }),
                    )
                    .child(member.name),
            );
        }
        root = root.child(chips);

        // Scene + seat fields.
        root = root.child(self.field(Field::Scene, "Scene", cx));
        root = root.child(self.field(Field::Seat, "Seat", cx));

        // Generate button + status.
        let busy = self.busy;
        root = root
            .child(
                div()
                    .px(px(18.))
                    .py(px(9.))
                    .rounded(px(18.))
                    .bg(if busy { rgb(0x453a6e) } else { rgb(0x6d5bd0) })
                    .text_color(rgb(0xffffff))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, _window, cx| {
                            this.start_generate(cx);
                        }),
                    )
                    .child(if busy { "Generating…" } else { "Generate" }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xbdb2d9))
                    .child(self.status.clone()),
            );

        // Preview of the newest still.
        if let Some(path) = &self.preview {
            root = root.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_sm().text_color(rgb(0x9a8fb5)).child("Latest"))
                    .child(img(path.clone()).max_h(px(320.)).rounded(px(8.))),
            );
        }

        // Gallery list (newest last in the index; show newest first).
        let mut list = div().flex().flex_col().gap(px(2.));
        for entry in self.gallery.iter().rev().take(12) {
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
        root
    }
}

impl Shell {
    /// One labelled text field with a block cursor when active.
    fn field(&self, which: Field, label: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let buf = match which {
            Field::Scene => &self.scene,
            Field::Seat => &self.seat,
        };
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
                // keep the cursor visible ahead of the hint
                text_row = div().flex().flex_row().child(
                    div().bg(rgb(0x6d5bd0)).child(" "),
                );
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(div().text_sm().text_color(rgb(0x9a8fb5)).child(label.to_string()))
            .child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(rgb(0x1e1830))
                    .border_1()
                    .border_color(if is_active { rgb(0x6d5bd0) } else { rgb(0x3a3157) })
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
        let bounds = Bounds::centered(None, size(px(460.), px(800.)), cx);
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
