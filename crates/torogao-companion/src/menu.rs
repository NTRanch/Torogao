//! The right-click menu: her name and a portrait swatch,
//! the Play entries (Greeting, Daydream), the cast
//! roster (Stage 1.2: all three rigs selectable), the
//! display list when more than one output exists, the
//! behaviour toggles (quiet hours, click-through lock),
//! Open Torogao, and Quit — Persona-register styling
//! (PROGRAM.md §3.3 host rules: Persona red on
//! near-black, serif).
//!
//! egui is driven manually (no eframe / egui-winit): each
//! presentation translates its own pointer events into
//! `egui::Event`s, and the tessellated paint jobs are drawn
//! into the same surface after the character pass with
//! `LoadOp::Load`, so menu and character share one
//! presented frame. The menu is rendered from a
//! `MenuModel` the presentation fills at open time;
//! actions leave as values; the presentation applies them
//! against the companion core — the menu holds no session
//! state of its own (T4 §3.3: the companion is a
//! presentation, never a second store). Every action in
//! this enum is a row of the Persona audit's companion
//! capability table; the parity test fails if the two
//! drift apart.

use std::time::Instant;

use torogao_app::persona::{FRAME_BG, HOUSE_ACCENT, TEXT_HEADING, TEXT_RESTING};

/// What the user picked. The presentation applies these;
/// the menu itself touches nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// Play Greeting (once).
    PlayGreeting,
    /// Play Daydream (looping ambient).
    PlayDaydream,
    /// Put the cast member at this roster index on stage.
    SwitchCharacter(usize),
    /// Move her surface to the named output.
    SendToOutput(String),
    /// Flip quiet hours / focus mode.
    ToggleQuiet,
    /// Flip the click-through lock.
    ToggleClickThrough,
    /// Launch the panel app on the same repertoire.
    OpenTorogao,
    /// Quit the companion.
    Quit,
    /// Dismiss the menu.
    Close,
}

/// Everything the menu renders, supplied by the
/// presentation at open time (the menu never reads the
/// core or the state file itself).
#[derive(Debug, Clone, Default)]
pub struct MenuModel {
    /// The character on stage, for the header.
    pub character_name: String,
    /// The cast roster: (display name, on stage now).
    pub cast: Vec<(String, bool)>,
    /// The click-through lock's standing state.
    pub click_through_locked: bool,
    /// The outputs she could be sent to: (connector
    /// name, she is there now). The Display section only
    /// renders when more than one output exists.
    pub outputs: Vec<(String, bool)>,
    /// Quiet hours / focus mode's standing state.
    pub quiet: bool,
}

fn persona_color(color: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(color[0], color[1], color[2])
}

/// Menu window width, logical pixels.
pub const MENU_WIDTH: f32 = 232.0;
/// One action row's pitch (button + spacing).
const ROW_PITCH: f32 = 30.0;
/// One section label's pitch.
const LABEL_PITCH: f32 = 18.0;
/// One separator's pitch.
const SEPARATOR_PITCH: f32 = 12.0;
/// The header row's pitch.
const HEADER_PITCH: f32 = 24.0;

/// The menu window size for a model: fixed width, and a
/// height that fits exactly the sections the model
/// carries (so the single-output fallback never shows a
/// Display section's empty space).
pub fn menu_size(model: &MenuModel) -> (f32, f32) {
    let mut height = 20.0 + HEADER_PITCH + SEPARATOR_PITCH;
    height += LABEL_PITCH + 2.0 * ROW_PITCH; // Play: Greeting, Daydream
    height += SEPARATOR_PITCH + LABEL_PITCH + model.cast.len() as f32 * ROW_PITCH;
    if model.outputs.len() > 1 {
        height += SEPARATOR_PITCH + LABEL_PITCH + model.outputs.len() as f32 * ROW_PITCH;
    }
    height += SEPARATOR_PITCH + 2.0 * ROW_PITCH; // quiet, click-through
    height += SEPARATOR_PITCH + 2.0 * ROW_PITCH; // Open Torogao, Quit
    (MENU_WIDTH, height)
}

/// The menu's egui context + GPU renderer + input state.
pub struct MenuUi {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    open_pos: (f64, f64),
    pointer_pos: Option<egui::Pos2>,
    pending_events: Vec<egui::Event>,
    /// What the menu renders; the presentation writes it
    /// at open.
    pub model: MenuModel,
    started: Instant,
}

impl MenuUi {
    /// Builds the menu renderer on the presentation's
    /// device. Fonts: egui's bundled defaults (the panel
    /// embeds Noto Serif JP for its own canvas text; the
    /// companion menu's serif register is approximated by
    /// egui's proportional family at this slice — the
    /// typeface is a presentation detail the Owner's eye
    /// gate rules on, and the deviation is named in the
    /// gate report).
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, open_pos: (f64, f64)) -> MenuUi {
        let ctx = egui::Context::default();
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = persona_color(FRAME_BG);
        visuals.window_fill = persona_color(FRAME_BG);
        visuals.override_text_color = Some(persona_color(TEXT_RESTING));
        visuals.selection.bg_fill = persona_color(HOUSE_ACCENT);
        ctx.set_visuals(visuals);
        MenuUi {
            renderer: egui_wgpu::Renderer::new(
                device,
                format,
                egui_wgpu::RendererOptions::default(),
            ),
            ctx,
            open_pos,
            pointer_pos: None,
            pending_events: Vec::new(),
            model: MenuModel::default(),
            started: Instant::now(),
        }
    }

    /// Feeds a pointer move (surface coordinates).
    pub fn push_pointer_move(&mut self, pos: (f64, f64)) {
        let pos2 = egui::pos2(pos.0 as f32, pos.1 as f32);
        self.pointer_pos = Some(pos2);
        self.pending_events.push(egui::Event::PointerMoved(pos2));
    }

    /// Feeds a pointer button transition. Only the left
    /// button actuates egui widgets; a right-button press
    /// while the menu is open dismisses it (the
    /// presentation checks `wants_close`).
    pub fn push_pointer_button(&mut self, left: bool, _right: bool, pressed: bool) {
        if let Some(pos) = self.pointer_pos {
            if left {
                self.pending_events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                });
            }
        }
    }

    /// Runs one menu frame over the just-presented
    /// character frame and returns any picked actions.
    /// `view` is the surface texture view the character
    /// pass drew into; the menu pass loads it.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        surface_size: (u32, u32),
    ) -> Vec<MenuAction> {
        let mut actions: Vec<MenuAction> = Vec::new();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(surface_size.0 as f32, surface_size.1 as f32),
            )),
            time: Some(self.started.elapsed().as_secs_f64()),
            events: std::mem::take(&mut self.pending_events),
            focused: self.pointer_pos.is_some(),
            ..Default::default()
        };
        // The anchor is the pointer position in surface
        // coordinates — which a compositor may report in a
        // shifted frame (primo's Hyprland offsets x by the
        // output's layout position). Clamp the menu into
        // the surface so it always opens on her, next to
        // the grab point as far as the frame allows.
        let size = menu_size(&self.model);
        let max_x = (surface_size.0 as f64 - f64::from(size.0)).max(0.0);
        let max_y = (surface_size.1 as f64 - f64::from(size.1)).max(0.0);
        let anchor = egui::pos2(
            self.open_pos.0.clamp(0.0, max_x) as f32,
            self.open_pos.1.clamp(0.0, max_y) as f32,
        );
        let model = self.model.clone();
        let full = self.ctx.run_ui(raw, |root_ui| {
            let ctx = root_ui.ctx();
            egui::Window::new("companion-menu")
                .title_bar(false)
                .resizable(false)
                .movable(false)
                .fixed_size(egui::vec2(size.0, size.1))
                .fixed_pos(anchor)
                .frame(
                    egui::Frame::default()
                        .fill(persona_color(FRAME_BG))
                        .stroke(egui::Stroke::new(1.0, persona_color(HOUSE_ACCENT)))
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ctx, |ui| {
                    draw_menu_body(ui, &model, &mut actions);
                });
        });
        let jobs = self.ctx.tessellate(full.shapes, full.pixels_per_point);
        for (id, delta) in &full.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }
        if jobs.is_empty() {
            return actions;
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [surface_size.0, surface_size.1],
            pixels_per_point: full.pixels_per_point,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("torogao companion menu encoder"),
        });
        self.renderer
            .update_buffers(device, queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("torogao companion menu pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        queue.submit([encoder.finish()]);
        for id in &full.textures_delta.free {
            self.renderer.free_texture(id);
        }
        actions
    }
}

/// The menu body: header, then the sections in their
/// standing order (Play, Character, Display, Behaviour,
/// Open, Quit).
fn draw_menu_body(ui: &mut egui::Ui, model: &MenuModel, actions: &mut Vec<MenuAction>) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 2.0, persona_color(HOUSE_ACCENT));
        ui.label(
            egui::RichText::new(&model.character_name)
                .color(persona_color(TEXT_HEADING))
                .size(17.0),
        );
    });
    ui.separator();
    section_label(ui, "Play");
    if menu_button(ui, "Greeting") {
        actions.push(MenuAction::PlayGreeting);
    }
    if menu_button(ui, "Daydream") {
        actions.push(MenuAction::PlayDaydream);
    }
    ui.separator();
    section_label(ui, "Character");
    for (index, (name, on_stage)) in model.cast.iter().enumerate() {
        let label = if *on_stage {
            format!("\u{25cf} {name}")
        } else {
            format!("\u{25cb} {name}")
        };
        if menu_button(ui, &label) {
            actions.push(MenuAction::SwitchCharacter(index));
        }
    }
    if model.outputs.len() > 1 {
        ui.separator();
        section_label(ui, "Display");
        for (name, current) in &model.outputs {
            let label = if *current {
                format!("\u{25cf} {name}")
            } else {
                format!("Send to {name}")
            };
            if menu_button(ui, &label) {
                actions.push(MenuAction::SendToOutput(name.clone()));
            }
        }
    }
    ui.separator();
    let quiet_label = if model.quiet {
        "Quiet hours: ON"
    } else {
        "Quiet hours: off"
    };
    if menu_button(ui, quiet_label) {
        actions.push(MenuAction::ToggleQuiet);
    }
    let lock_label = if model.click_through_locked {
        "Click-through lock: ON"
    } else {
        "Click-through lock: off"
    };
    if menu_button(ui, lock_label) {
        actions.push(MenuAction::ToggleClickThrough);
    }
    ui.separator();
    if menu_button(ui, "Open Torogao") {
        actions.push(MenuAction::OpenTorogao);
    }
    if menu_button(ui, "Quit") {
        actions.push(MenuAction::Quit);
    }
}

fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .color(persona_color(TEXT_HEADING))
            .size(13.0),
    );
}

fn menu_button(ui: &mut egui::Ui, label: &str) -> bool {
    let button = egui::Button::new(egui::RichText::new(label).size(14.0))
        .min_size(egui::vec2(MENU_WIDTH - 24.0, 26.0));
    ui.add(button).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_model() -> MenuModel {
        MenuModel {
            character_name: "Prima".to_string(),
            cast: vec![
                ("Prima".to_string(), true),
                ("Yui".to_string(), false),
                ("Alma".to_string(), false),
            ],
            click_through_locked: false,
            outputs: vec![("HDMI-A-1".to_string(), true), ("DP-1".to_string(), false)],
            quiet: false,
        }
    }

    #[test]
    fn the_menu_grows_with_its_sections() {
        let single_output = MenuModel {
            outputs: vec![("HDMI-A-1".to_string(), true)],
            ..full_model()
        };
        let (width, height) = menu_size(&full_model());
        assert_eq!(width, MENU_WIDTH);
        assert!(
            height > menu_size(&single_output).1,
            "a Display section must add height"
        );
        assert!(height < 600.0, "the menu stays a menu: {height}");
    }
}
