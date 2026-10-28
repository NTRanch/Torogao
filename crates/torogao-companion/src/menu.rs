//! The right-click menu: her name and a portrait swatch,
//! Play submenu entries (Greeting, Daydream), Open
//! Torogao, the click-through lock toggle, and Quit —
//! Persona-register styling (PROGRAM.md §3.3 host rules:
//! Persona red on near-black, serif).
//!
//! egui is driven manually (no eframe / egui-winit): each
//! presentation translates its own pointer events into
//! `egui::Event`s, and the tessellated paint jobs are drawn
//! into the same surface after the character pass with
//! `LoadOp::Load`, so menu and character share one
//! presented frame. Actions leave as values; the
//! presentation applies them against the companion core —
//! the menu holds no session state of its own (T4 §3.3:
//! the companion is a presentation, never a second store).

use std::time::Instant;

use torogao_app::persona::{FRAME_BG, HOUSE_ACCENT, TEXT_HEADING, TEXT_RESTING};

/// What the user picked. The presentation applies these;
/// the menu itself touches nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Play Greeting (once).
    PlayGreeting,
    /// Play Daydream (looping ambient).
    PlayDaydream,
    /// Launch the panel app on the same repertoire.
    OpenTorogao,
    /// Flip the click-through lock.
    ToggleClickThrough,
    /// Quit the companion.
    Quit,
    /// Dismiss the menu.
    Close,
}

fn persona_color(color: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(color[0], color[1], color[2])
}

/// Menu window size, logical pixels.
pub const MENU_SIZE: (f32, f32) = (232.0, 292.0);

/// The menu's egui context + GPU renderer + input state.
pub struct MenuUi {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    open_pos: (f64, f64),
    pointer_pos: Option<egui::Pos2>,
    pending_events: Vec<egui::Event>,
    /// Mirrored from placement so the toggle row can show
    /// its state; the presentation writes it at open.
    pub click_through_locked: bool,
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
            click_through_locked: false,
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
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        surface_size: (u32, u32),
        character_name: &str,
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
        let anchor = egui::pos2(self.open_pos.0 as f32, self.open_pos.1 as f32);
        let full = self.ctx.run_ui(raw, |root_ui| {
            let ctx = root_ui.ctx();
            egui::Window::new("companion-menu")
                .title_bar(false)
                .resizable(false)
                .movable(false)
                .fixed_size(egui::vec2(MENU_SIZE.0, MENU_SIZE.1))
                .fixed_pos(anchor)
                .frame(
                    egui::Frame::default()
                        .fill(persona_color(FRAME_BG))
                        .stroke(egui::Stroke::new(1.0, persona_color(HOUSE_ACCENT)))
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 2.0, persona_color(HOUSE_ACCENT));
                        ui.label(
                            egui::RichText::new(character_name)
                                .color(persona_color(TEXT_HEADING))
                                .size(17.0),
                        );
                    });
                    ui.separator();
                    ui.label(
                        egui::RichText::new("Play")
                            .color(persona_color(TEXT_HEADING))
                            .size(13.0),
                    );
                    if menu_button(ui, "Greeting") {
                        actions.push(MenuAction::PlayGreeting);
                    }
                    if menu_button(ui, "Daydream") {
                        actions.push(MenuAction::PlayDaydream);
                    }
                    ui.separator();
                    if menu_button(ui, "Open Torogao") {
                        actions.push(MenuAction::OpenTorogao);
                    }
                    let lock_label = if self.click_through_locked {
                        "Click-through lock: ON"
                    } else {
                        "Click-through lock: off"
                    };
                    if menu_button(ui, lock_label) {
                        actions.push(MenuAction::ToggleClickThrough);
                    }
                    ui.separator();
                    if menu_button(ui, "Quit") {
                        actions.push(MenuAction::Quit);
                    }
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

fn menu_button(ui: &mut egui::Ui, label: &str) -> bool {
    let button = egui::Button::new(egui::RichText::new(label).size(14.0))
        .min_size(egui::vec2(MENU_SIZE.0 - 24.0, 26.0));
    ui.add(button).clicked()
}
