// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! The paint editor surface (SPEC Phase 1, D-991): an
//! egui front over `torogao-paint`'s deterministic
//! document. The editor holds no pixel truth of its own —
//! every edit is an op applied to the document's log, and
//! undo/redo walk that log (SS3.3). The canvas uploads
//! only tiles whose composited hash changed.
//!
//! Input: the mouse is the primary path (DP-2's tablet
//! is forgotten, D-992). Pen pressure joins through the
//! `torogao-pen` evdev side channel when a device exists
//! and degrades silently when none does; pen position
//! comes from the shim stream itself (the D-970 binding
//! design note), never from correlating with the mouse.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use torogao_paint::hash::sha256_bytes;
use torogao_paint::layer::BlendMode;
use torogao_paint::ops::{LayerPropsChange, Op, Rect as PaintRect};
use torogao_paint::stroke::{StampMode, StrokeParams, StrokeSample};
use torogao_paint::{Document, LayerId, Palette, SourceRef, TILE_PX, load_document, save_document};
use torogao_pen::{PenDeviceInfo, PenReader, PenSample, scan_pen_devices};

/// Pen samples drained per frame at most (bounded work).
const PEN_DRAIN_MAX: usize = 8192;

/// Which tool the pointer is currently wielding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Brush,
    Eraser,
    Eyedropper,
}

/// One composited tile's uploaded texture.
struct TileImage {
    hash: [u8; 32],
    texture: egui::TextureHandle,
}

/// The §4.7 reference image: view state only, plus the
/// hash a save may offer into the document's sources.
struct ReferenceImage {
    path: String,
    sha256: [u8; 32],
    width: u32,
    height: u32,
    texture: egui::TextureHandle,
}

/// The editor's whole session state.
pub struct PaintSession {
    doc: Document,
    doc_dir: Option<PathBuf>,
    palette: Palette,
    palette_source: Option<SourceRef>,
    tool: Tool,
    color: [u8; 3],
    params: StrokeParams,
    active_layer: Option<LayerId>,
    mouse_stroke: Vec<StrokeSample>,
    mouse_down: bool,
    pen: Option<PenReader>,
    pen_rx: Option<Receiver<PenSample>>,
    pen_info: Option<PenDeviceInfo>,
    pen_stroke: Vec<StrokeSample>,
    pen_touch: bool,
    pan: egui::Vec2,
    zoom: f32,
    tiles: HashMap<(i32, i32), TileImage>,
    reference: Option<ReferenceImage>,
    reference_opacity: f32,
    split: Option<f32>,
    record_reference_on_save: bool,
    eyedropper_radius: u32,
    new_width: u32,
    new_height: u32,
    open_path: String,
    save_path: String,
    export_path: String,
    import_path: String,
    reference_path: String,
    palette_path: String,
    rename_bufs: HashMap<u32, String>,
    opacity_edits: HashMap<u32, u8>,
    status: String,
}

impl PaintSession {
    /// A fresh session: a 1024x768 document with one
    /// layer, the working palette, and a pen scan (whose
    /// emptiness is a normal state, not an error).
    pub fn new(open_path: Option<&std::path::Path>) -> PaintSession {
        let mut session = PaintSession {
            doc: Document::new_empty(1024, 768).expect("fixed canvas is in bounds"),
            doc_dir: None,
            palette: Palette::working_defaults(),
            palette_source: None,
            tool: Tool::Brush,
            color: [30, 28, 60],
            params: StrokeParams::default(),
            active_layer: None,
            mouse_stroke: Vec::new(),
            mouse_down: false,
            pen: None,
            pen_rx: None,
            pen_info: None,
            pen_stroke: Vec::new(),
            pen_touch: false,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            tiles: HashMap::new(),
            reference: None,
            reference_opacity: 0.5,
            split: None,
            record_reference_on_save: true,
            eyedropper_radius: 3,
            new_width: 1024,
            new_height: 768,
            open_path: String::new(),
            save_path: String::new(),
            export_path: String::new(),
            import_path: String::new(),
            reference_path: String::new(),
            palette_path: String::new(),
            rename_bufs: HashMap::new(),
            opacity_edits: HashMap::new(),
            status: "new document".to_string(),
        };
        if let Ok(layer) = session.doc.add_layer("layer 1".to_string()) {
            session.active_layer = Some(layer);
        }
        session.attach_pen();
        if let Some(path) = open_path {
            session.open_document(path.to_path_buf());
        }
        session
    }

    /// Find a pen and start its reader, if one exists.
    fn attach_pen(&mut self) {
        if let Some(info) = scan_pen_devices().into_iter().next() {
            match PenReader::spawn(&info.node) {
                Ok((reader, rx)) => {
                    self.status = format!("pen attached: {}", info.name);
                    self.pen = Some(reader);
                    self.pen_rx = Some(rx);
                    self.pen_info = Some(info);
                }
                Err(err) => {
                    self.status = format!("pen {} unreadable ({err}); mouse only", info.name);
                }
            }
        }
    }

    /// Apply one op, surfacing a refusal in the status
    /// line instead of failing the frame.
    fn apply(&mut self, op: Op) {
        if let Err(err) = self.doc.apply_op(op) {
            self.status = format!("refused: {err}");
        }
    }

    /// Open a `.tpaint` container, replacing the document.
    fn open_document(&mut self, path: PathBuf) {
        match load_document(&path) {
            Ok(doc) => {
                self.active_layer = doc.layers().last().map(|layer| layer.id);
                self.doc = doc;
                self.doc_dir = Some(path.clone());
                self.tiles.clear();
                self.status = format!("opened {}", path.display());
            }
            Err(err) => {
                self.status = format!("open failed: {err}");
            }
        }
    }

    /// Save through the core, offering the reference
    /// image into the document's sources when asked (§4.7:
    /// sources are provenance, the overlay stays view
    /// state).
    fn save(&mut self, path: PathBuf) {
        if self.record_reference_on_save {
            if let Some(reference) = &self.reference {
                let known = self
                    .doc
                    .sources()
                    .iter()
                    .any(|held| held.sha256 == reference.sha256);
                if !known {
                    self.doc.add_source(SourceRef {
                        path: reference.path.clone(),
                        sha256: reference.sha256,
                    });
                }
            }
        }
        if let Some(source) = self.palette_source.clone() {
            self.doc.set_palette_source(Some(source));
        }
        match save_document(&mut self.doc, &path) {
            Ok(()) => {
                self.doc_dir = Some(path.clone());
                self.status = format!("saved {}", path.display());
            }
            Err(err) => {
                self.status = format!("save failed: {err}");
            }
        }
    }

    /// The frame's UI: file bar, tool/layer side panel,
    /// canvas, status line.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.drain_pen();
        self.file_bar(ui);
        self.side_panel(ui);
        egui::Panel::bottom("paint-status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.separator();
                ui.label(format!(
                    "ops {}/{} · resident {:.1} / 512 MiB · {}",
                    self.doc.cursor(),
                    self.doc.ops().len(),
                    self.doc.resident_bytes() as f64 / 1_048_576.0,
                    self.pen_label(),
                ));
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            self.canvas(ui);
        });
    }

    fn pen_label(&self) -> String {
        match (&self.pen_info, &self.pen) {
            (Some(info), Some(reader)) => {
                format!("pen: {} (dropped {})", info.name, reader.dropped_samples())
            }
            _ => "pen: none — mouse".to_string(),
        }
    }

    fn file_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-file").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("open");
                ui.text_edit_singleline(&mut self.open_path);
                if ui.button("Open").clicked() {
                    let path = PathBuf::from(self.open_path.trim());
                    self.open_document(path);
                }
                ui.label("save to");
                ui.text_edit_singleline(&mut self.save_path);
                if ui.button("Save").clicked() {
                    let path = if self.save_path.trim().is_empty() {
                        self.doc_dir.clone()
                    } else {
                        Some(PathBuf::from(self.save_path.trim()))
                    };
                    match path {
                        Some(path) => self.save(path),
                        None => {
                            self.status = "save needs a container path".to_string();
                        }
                    }
                }
                if ui.button("⟲ undo").clicked() {
                    self.doc.undo();
                }
                if ui.button("⟳ redo").clicked() {
                    self.doc.redo();
                }
                ui.label(format!(
                    "canvas {}×{} · base {}",
                    self.doc.width(),
                    self.doc.height(),
                    match self.doc.base() {
                        torogao_paint::BaseRaster::Empty => "empty",
                        torogao_paint::BaseRaster::Raster { .. } => "png",
                    },
                ));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("new");
                ui.add(egui::DragValue::new(&mut self.new_width).range(1..=8192));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut self.new_height).range(1..=8192));
                if ui.button("New document").clicked() {
                    match Document::new_empty(self.new_width, self.new_height) {
                        Ok(mut doc) => {
                            self.active_layer = doc.add_layer("layer 1".to_string()).ok();
                            self.doc = doc;
                            self.doc_dir = None;
                            self.tiles.clear();
                            self.status = "new document".to_string();
                        }
                        Err(err) => {
                            self.status = format!("new refused: {err}");
                        }
                    }
                }
                ui.label("import PNG");
                ui.text_edit_singleline(&mut self.import_path);
                if ui.button("Import").clicked() {
                    let path = PathBuf::from(self.import_path.trim());
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            match Document::from_png_bytes(&bytes, &path.display().to_string()) {
                                Ok(mut doc) => {
                                    self.active_layer = doc.add_layer("paint".to_string()).ok();
                                    self.doc = doc;
                                    self.doc_dir = None;
                                    self.tiles.clear();
                                    self.status = format!("imported {}", path.display());
                                }
                                Err(err) => {
                                    self.status = format!("import failed: {err}");
                                }
                            }
                        }
                        Err(err) => {
                            self.status = format!("import read failed: {err}");
                        }
                    }
                }
                ui.label("export PNG");
                ui.text_edit_singleline(&mut self.export_path);
                if ui.button("Export").clicked() {
                    match self.doc.export_png() {
                        Ok(png) => {
                            let path = PathBuf::from(self.export_path.trim());
                            match std::fs::write(&path, &png) {
                                Ok(()) => {
                                    self.status = format!("exported {}", path.display());
                                }
                                Err(err) => {
                                    self.status = format!("export failed: {err}");
                                }
                            }
                        }
                        Err(err) => {
                            self.status = format!("export failed: {err}");
                        }
                    }
                }
            });
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("paint-tools")
            .resizable(true)
            .default_size(260.0)
            .show(ui, |ui| {
                ui.heading("Tool");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tool, Tool::Brush, "brush");
                    ui.selectable_value(&mut self.tool, Tool::Eraser, "eraser");
                    ui.selectable_value(&mut self.tool, Tool::Eyedropper, "eyedropper");
                });
                let mut rgb = [
                    f32::from(self.color[0]) / 255.0,
                    f32::from(self.color[1]) / 255.0,
                    f32::from(self.color[2]) / 255.0,
                ];
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    self.color = [
                        (rgb[0] * 255.0).round() as u8,
                        (rgb[1] * 255.0).round() as u8,
                        (rgb[2] * 255.0).round() as u8,
                    ];
                }
                ui.add(
                    egui::Slider::new(&mut self.params.diameter_px, 1..=1024).text("diameter px"),
                );
                ui.add(egui::Slider::new(&mut self.params.opacity, 0..=255).text("opacity"));
                ui.add(egui::Slider::new(&mut self.params.flow, 0..=255).text("flow"));
                ui.add(
                    egui::Slider::new(&mut self.params.spacing_fraction, 0.05..=1.0)
                        .text("spacing"),
                );
                ui.add(
                    egui::Slider::new(&mut self.params.stabilizer_alpha, 0.05..=1.0)
                        .text("stabilizer"),
                );
                ui.checkbox(&mut self.params.pressure_map.size, "pressure → size");
                ui.checkbox(&mut self.params.pressure_map.alpha, "pressure → alpha");
                ui.horizontal(|ui| {
                    ui.label("eyedropper");
                    ui.selectable_value(&mut self.eyedropper_radius, 1, "1×1");
                    ui.selectable_value(&mut self.eyedropper_radius, 3, "3×3");
                    ui.selectable_value(&mut self.eyedropper_radius, 5, "5×5");
                });
                ui.separator();
                self.palette_panel(ui);
                ui.separator();
                self.reference_panel(ui);
                ui.separator();
                self.layers_panel(ui);
            });
    }

    fn palette_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("Palette — {}", self.palette.name));
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.palette_path);
            if ui.button("Load").clicked() {
                let path = PathBuf::from(self.palette_path.trim());
                match Palette::load(&path) {
                    Ok((palette, sha)) => {
                        self.palette_source = Some(SourceRef {
                            path: path.display().to_string(),
                            sha256: sha,
                        });
                        self.palette = palette;
                        self.status = "palette loaded".to_string();
                    }
                    Err(err) => {
                        self.status = format!("palette load failed: {err}");
                    }
                }
            }
        });
        let swatches = self.palette.swatches.clone();
        ui.horizontal_wrapped(|ui| {
            for swatch in &swatches {
                let fill = egui::Color32::from_rgb(swatch.rgb[0], swatch.rgb[1], swatch.rgb[2]);
                let button = egui::Button::new("  ").fill(fill);
                if ui.add(button).on_hover_text(&swatch.name).clicked() {
                    self.color = swatch.rgb;
                }
            }
        });
    }

    fn reference_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Reference (§4.7)");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.reference_path);
            if ui.button("Load").clicked() {
                self.load_reference(ui.ctx());
            }
        });
        if self.reference.is_some() {
            ui.add(
                egui::Slider::new(&mut self.reference_opacity, 0.0..=1.0).text("overlay opacity"),
            );
            let mut split_on = self.split.is_some();
            if ui.checkbox(&mut split_on, "split view").changed() {
                self.split = if split_on { Some(0.5) } else { None };
            }
            if let Some(split) = &mut self.split {
                ui.add(egui::Slider::new(split, 0.0..=1.0).text("split position"));
            }
            ui.checkbox(
                &mut self.record_reference_on_save,
                "record in sources on save",
            );
            if ui.button("Clear reference").clicked() {
                self.reference = None;
            }
        }
    }

    fn load_reference(&mut self, ctx: &egui::Context) {
        let path = PathBuf::from(self.reference_path.trim());
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.status = format!("reference read failed: {err}");
                return;
            }
        };
        match torogao_paint::png_io::decode_png_rgba(&bytes) {
            Ok((width, height, pixels)) => {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    &pixels,
                );
                let texture =
                    ctx.load_texture("paint-reference", image, egui::TextureOptions::LINEAR);
                self.reference = Some(ReferenceImage {
                    path: path.display().to_string(),
                    sha256: sha256_bytes(&pixels),
                    width,
                    height,
                    texture,
                });
                self.status = format!("reference loaded {}", path.display());
            }
            Err(err) => {
                self.status = format!("reference failed: {err}");
            }
        }
    }

    fn layers_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Layers");
        if ui.button("+ add layer").clicked() {
            let name = format!("layer {}", self.doc.layers().len() + 1);
            match self.doc.add_layer(name) {
                Ok(id) => {
                    self.active_layer = Some(id);
                }
                Err(err) => {
                    self.status = format!("add layer refused: {err}");
                }
            }
        }
        // Snapshot the descriptors; ops are committed
        // after the loop so the borrow stays simple.
        let layers: Vec<(LayerId, String, u8, BlendMode, bool, bool)> = self
            .doc
            .layers()
            .iter()
            .map(|layer| {
                (
                    layer.id,
                    layer.name.clone(),
                    layer.opacity,
                    layer.blend,
                    layer.visible,
                    layer.lock_alpha,
                )
            })
            .collect();
        let count = layers.len();
        let mut pending: Option<Op> = None;
        for (index, (id, name, opacity, blend, visible, lock_alpha)) in
            layers.iter().enumerate().rev()
        {
            let id = *id;
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    let selected = self.active_layer == Some(id);
                    if ui.selectable_label(selected, "●").clicked() {
                        self.active_layer = Some(id);
                    }
                    let buffer = self.rename_bufs.entry(id.0).or_insert_with(|| name.clone());
                    let edit = ui.text_edit_singleline(buffer);
                    if edit.lost_focus() && buffer.as_str() != name.as_str() {
                        pending = Some(Op::LayerProps {
                            id,
                            change: LayerPropsChange {
                                name: Some(buffer.clone()),
                                ..LayerPropsChange::default()
                            },
                        });
                    }
                });
                ui.horizontal(|ui| {
                    let mut shown = *visible;
                    if ui.checkbox(&mut shown, "show").changed() {
                        pending = Some(Op::LayerProps {
                            id,
                            change: LayerPropsChange {
                                visible: Some(shown),
                                ..LayerPropsChange::default()
                            },
                        });
                    }
                    let mut locked = *lock_alpha;
                    if ui.checkbox(&mut locked, "lock α").changed() {
                        pending = Some(Op::LayerProps {
                            id,
                            change: LayerPropsChange {
                                lock_alpha: Some(locked),
                                ..LayerPropsChange::default()
                            },
                        });
                    }
                    if ui.button("↑").clicked() && index + 1 < count {
                        pending = Some(Op::LayerReorder {
                            id,
                            to_index: index as u32 + 1,
                        });
                    }
                    if ui.button("↓").clicked() && index > 0 {
                        pending = Some(Op::LayerReorder {
                            id,
                            to_index: index as u32 - 1,
                        });
                    }
                    if ui.button("✕").clicked() {
                        pending = Some(Op::LayerRemove { id });
                    }
                });
                ui.horizontal(|ui| {
                    let mut value = self.opacity_edits.get(&id.0).copied().unwrap_or(*opacity);
                    let slider = ui.add(egui::Slider::new(&mut value, 0..=255).text("opacity"));
                    if slider.dragged() {
                        self.opacity_edits.insert(id.0, value);
                    }
                    if slider.drag_stopped() {
                        self.opacity_edits.remove(&id.0);
                        pending = Some(Op::LayerProps {
                            id,
                            change: LayerPropsChange {
                                opacity: Some(value),
                                ..LayerPropsChange::default()
                            },
                        });
                    }
                    egui::ComboBox::from_id_salt(("blend", id.0))
                        .selected_text(blend_name(*blend))
                        .show_ui(ui, |ui| {
                            for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Overlay]
                            {
                                if ui
                                    .selectable_label(*blend == mode, blend_name(mode))
                                    .clicked()
                                {
                                    pending = Some(Op::LayerProps {
                                        id,
                                        change: LayerPropsChange {
                                            blend: Some(mode),
                                            ..LayerPropsChange::default()
                                        },
                                    });
                                }
                            }
                        });
                });
            });
        }
        if let Some(op) = pending {
            self.apply(op);
        }
    }

    /// The canvas: view transform, tile upload, pointer
    /// and pen strokes, reference overlay/split.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (response, _) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let view = response.rect;
        let ctx = ui.ctx().clone();
        // Keyboard undo/redo, when no text field holds the
        // keyboard (mirrors the stage Shell's guard).
        let text_focused = ctx.memory(|memory| memory.focused().is_some());
        if !text_focused {
            let (undo, redo) = ctx.input(|input| {
                (
                    input.modifiers.ctrl
                        && input.key_pressed(egui::Key::Z)
                        && !input.modifiers.shift,
                    input.modifiers.ctrl
                        && (input.key_pressed(egui::Key::Y)
                            || (input.key_pressed(egui::Key::Z) && input.modifiers.shift)),
                )
            });
            if undo {
                self.doc.undo();
            }
            if redo {
                self.doc.redo();
            }
        }
        // Zoom around the pointer; pan on middle-drag.
        if response.hovered() {
            let scroll = ctx.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(pointer) = response.hover_pos() {
                    let before = self.screen_to_canvas(pointer, view);
                    self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.05, 32.0);
                    let after =
                        self.canvas_to_screen(egui::pos2(before.0 as f32, before.1 as f32), view);
                    self.pan += pointer - after;
                }
            }
            let (middle_down, pointer_delta) = ctx.input(|input| {
                (
                    input.pointer.button_down(egui::PointerButton::Middle),
                    input.pointer.delta(),
                )
            });
            if middle_down {
                self.pan += pointer_delta;
            }
        }
        let doc_clip = match self.split {
            Some(frac) => {
                let mut rect = view;
                rect.max.x = view.min.x + view.width() * frac;
                rect
            }
            None => view,
        };
        self.paint_document(ui, view, doc_clip);
        match (self.reference.as_ref(), self.split) {
            (Some(reference), Some(frac)) => {
                let mut clip = view;
                clip.min.x = view.min.x + view.width() * frac;
                let painter = ui.painter_at(clip);
                self.paint_reference(&painter, reference, view, 1.0);
                let divider = egui::Rect::from_min_max(
                    egui::pos2(view.min.x + view.width() * frac - 1.0, view.min.y),
                    egui::pos2(view.min.x + view.width() * frac + 1.0, view.max.y),
                );
                painter.rect_filled(divider, 0.0, egui::Color32::from_rgb(200, 180, 90));
            }
            (Some(reference), None) => {
                let painter = ui.painter_at(view);
                self.paint_reference(&painter, reference, view, self.reference_opacity);
            }
            (None, _) => {}
        }
        self.mouse_input(&ctx, &response, view);
        // In-progress stroke preview: the stamp discs,
        // drawn as circles at their recorded placements.
        if !self.mouse_stroke.is_empty() || !self.pen_stroke.is_empty() {
            let painter = ui.painter_at(view);
            for stroke in [&self.mouse_stroke, &self.pen_stroke] {
                for sample in stroke.iter() {
                    let pos = self
                        .canvas_to_screen(egui::pos2(sample.x_px as f32, sample.y_px as f32), view);
                    painter.circle_stroke(
                        pos,
                        self.params.diameter_px as f32 * 0.5 * self.zoom * sample.pressure as f32,
                        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(90)),
                    );
                }
            }
        }
    }

    /// Composite and upload the visible tiles (dirty-tile
    /// upload: a tile re-uploads only when its composited
    /// hash changed).
    fn paint_document(&mut self, ui: &mut egui::Ui, view: egui::Rect, clip: egui::Rect) {
        let painter = ui.painter_at(clip);
        painter.rect_filled(view, 0.0, egui::Color32::from_rgb(26, 26, 30));
        let tile = TILE_PX as i32;
        let (min_tx, min_ty) = {
            let origin = self.screen_to_canvas(clip.min, view);
            (
                (origin.0.floor() as i32).div_euclid(tile),
                (origin.1.floor() as i32).div_euclid(tile),
            )
        };
        let (max_tx, max_ty) = {
            let corner = self.screen_to_canvas(clip.max, view);
            (
                (corner.0.ceil() as i32).div_euclid(tile),
                (corner.1.ceil() as i32).div_euclid(tile),
            )
        };
        for ty in min_ty..=max_ty {
            for tx in min_tx..=max_tx {
                let Some((cx, cy, cw, ch)) = clip_region(
                    tx * tile,
                    ty * tile,
                    TILE_PX,
                    TILE_PX,
                    self.doc.width(),
                    self.doc.height(),
                ) else {
                    continue;
                };
                let clipped = PaintRect {
                    x: cx as i32,
                    y: cy as i32,
                    width: cw,
                    height: ch,
                };
                let Ok((pixels, hash)) = self.doc.composite_region_hashed(cx, cy, cw, ch) else {
                    continue;
                };
                let min =
                    self.canvas_to_screen(egui::pos2(clipped.x as f32, clipped.y as f32), view);
                let max = self.canvas_to_screen(
                    egui::pos2(
                        (clipped.x + clipped.width as i32) as f32,
                        (clipped.y + clipped.height as i32) as f32,
                    ),
                    view,
                );
                let entry = self.tiles.entry((tx, ty)).or_insert_with(|| TileImage {
                    hash: [0u8; 32],
                    texture: ui.ctx().load_texture(
                        format!("paint-tile-{tx}-{ty}"),
                        egui::ColorImage::new(
                            [clipped.width as usize, clipped.height as usize],
                            vec![
                                egui::Color32::TRANSPARENT;
                                clipped.width as usize * clipped.height as usize
                            ],
                        ),
                        egui::TextureOptions::NEAREST,
                    ),
                });
                if entry.hash != hash {
                    entry.texture.set(
                        egui::ColorImage::from_rgba_unmultiplied(
                            [clipped.width as usize, clipped.height as usize],
                            &pixels,
                        ),
                        egui::TextureOptions::NEAREST,
                    );
                    entry.hash = hash;
                }
                painter.image(
                    entry.texture.id(),
                    egui::Rect::from_min_max(min, max),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
        // Canvas border.
        let doc_min = self.canvas_to_screen(egui::Pos2::ZERO, view);
        let doc_max = self.canvas_to_screen(
            egui::pos2(self.doc.width() as f32, self.doc.height() as f32),
            view,
        );
        painter.rect_stroke(
            egui::Rect::from_min_max(doc_min, doc_max),
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 90, 100)),
            egui::StrokeKind::Outside,
        );
    }

    fn paint_reference(
        &self,
        painter: &egui::Painter,
        reference: &ReferenceImage,
        view: egui::Rect,
        opacity: f32,
    ) {
        let min = self.canvas_to_screen(egui::Pos2::ZERO, view);
        let max = self.canvas_to_screen(
            egui::pos2(reference.width as f32, reference.height as f32),
            view,
        );
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
        painter.image(
            reference.texture.id(),
            egui::Rect::from_min_max(min, max),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::from_white_alpha(alpha),
        );
    }

    /// The mouse state machine: press starts a stroke (or
    /// picks, for the eyedropper), motion appends samples,
    /// release commits the stroke as one op.
    fn mouse_input(&mut self, ctx: &egui::Context, response: &egui::Response, view: egui::Rect) {
        let Some(pointer) = response.hover_pos().or(response.interact_pointer_pos()) else {
            if self.mouse_down {
                self.commit_mouse_stroke();
            }
            return;
        };
        let down = ctx.input(|input| input.pointer.primary_down());
        let canvas_pos = self.screen_to_canvas(pointer, view);
        if down && !self.mouse_down {
            self.mouse_down = true;
            match self.tool {
                Tool::Eyedropper => self.pick(canvas_pos),
                Tool::Brush | Tool::Eraser => {
                    self.mouse_stroke.clear();
                    self.mouse_stroke.push(StrokeSample {
                        x_px: canvas_pos.0,
                        y_px: canvas_pos.1,
                        pressure: 1.0,
                    });
                }
            }
        } else if down && self.mouse_down && self.tool != Tool::Eyedropper {
            self.mouse_stroke.push(StrokeSample {
                x_px: canvas_pos.0,
                y_px: canvas_pos.1,
                pressure: 1.0,
            });
        } else if !down && self.mouse_down {
            self.mouse_down = false;
            self.commit_mouse_stroke();
        }
    }

    fn commit_mouse_stroke(&mut self) {
        let samples = std::mem::take(&mut self.mouse_stroke);
        let tool = self.tool;
        self.commit_stroke(samples, tool);
    }

    /// Shared stroke commit for mouse and pen (DP-3: the
    /// log records stamp placements, not gestures — both
    /// paths land in the same `stamp_run` shape).
    fn commit_stroke(&mut self, samples: Vec<StrokeSample>, tool: Tool) {
        if samples.is_empty() {
            return;
        }
        let Some(layer) = self.active_layer else {
            self.status = "no active layer".to_string();
            return;
        };
        let mode = match tool {
            Tool::Eraser => StampMode::Erase,
            _ => StampMode::Paint,
        };
        let mut params = self.params;
        params.color = [self.color[0], self.color[1], self.color[2], 255];
        match torogao_paint::build_stamp_run(&samples, &params) {
            Ok(stamps) if !stamps.is_empty() => {
                self.apply(Op::StampRun {
                    layer,
                    mode,
                    color: [self.color[0], self.color[1], self.color[2], 255],
                    opacity: self.params.opacity,
                    flow: self.params.flow,
                    stamps,
                });
            }
            Ok(_) => {}
            Err(err) => {
                self.status = format!("stroke refused: {err}");
            }
        }
    }

    /// The eyedropper: average the CPU composite over the
    /// selected 1×1 / 3×3 / 5×5 footprint (§4.3).
    fn pick(&mut self, at: (f64, f64)) {
        let radius = (self.eyedropper_radius / 2) as i32;
        let region = PaintRect {
            x: at.0.round() as i32 - radius,
            y: at.1.round() as i32 - radius,
            width: self.eyedropper_radius,
            height: self.eyedropper_radius,
        };
        let Some((cx, cy, cw, ch)) = clip_region(
            region.x,
            region.y,
            region.width,
            region.height,
            self.doc.width(),
            self.doc.height(),
        ) else {
            return;
        };
        let Ok(pixels) = self.doc.composite_region(cx, cy, cw, ch) else {
            return;
        };
        let (mut r, mut g, mut b, mut w) = (0f64, 0f64, 0f64, 0f64);
        for px in pixels.chunks_exact(4) {
            let weight = f64::from(px[3]) / 255.0;
            r += f64::from(px[0]) * weight;
            g += f64::from(px[1]) * weight;
            b += f64::from(px[2]) * weight;
            w += weight;
        }
        if w > 0.0 {
            self.color = [
                (r / w).round() as u8,
                (g / w).round() as u8,
                (b / w).round() as u8,
            ];
        }
    }

    /// Drain the pen channel: position AND pressure from
    /// the shim stream itself; touch edges open and close
    /// the pen stroke.
    fn drain_pen(&mut self) {
        let Some(rx) = self.pen_rx.take() else { return };
        let mut samples: Vec<PenSample> = Vec::new();
        for sample in rx.try_iter().take(PEN_DRAIN_MAX) {
            samples.push(sample);
        }
        self.pen_rx = Some(rx);
        let Some(info) = self.pen_info.clone() else {
            return;
        };
        for sample in samples {
            let canvas = (
                f64::from(sample.x_units) * f64::from(self.doc.width())
                    / f64::from(info.x_max.max(1)),
                f64::from(sample.y_units) * f64::from(self.doc.height())
                    / f64::from(info.y_max.max(1)),
            );
            if sample.touch && !self.pen_touch {
                self.pen_touch = true;
                match self.tool {
                    Tool::Eyedropper => self.pick(canvas),
                    Tool::Brush | Tool::Eraser => {
                        self.pen_stroke.clear();
                        self.pen_stroke.push(StrokeSample {
                            x_px: canvas.0,
                            y_px: canvas.1,
                            pressure: f64::from(sample.pressure) / 65_535.0,
                        });
                    }
                }
            } else if sample.touch && self.pen_touch && self.tool != Tool::Eyedropper {
                self.pen_stroke.push(StrokeSample {
                    x_px: canvas.0,
                    y_px: canvas.1,
                    pressure: f64::from(sample.pressure) / 65_535.0,
                });
            } else if !sample.touch && self.pen_touch {
                self.pen_touch = false;
                let samples = std::mem::take(&mut self.pen_stroke);
                self.commit_stroke(samples, self.tool);
            }
        }
    }

    fn canvas_to_screen(&self, pos: egui::Pos2, view: egui::Rect) -> egui::Pos2 {
        view.min + self.pan + pos.to_vec2() * self.zoom
    }

    fn screen_to_canvas(&self, pos: egui::Pos2, view: egui::Rect) -> (f64, f64) {
        let local = (pos - view.min - self.pan) / self.zoom;
        (f64::from(local.x), f64::from(local.y))
    }
}

/// The blend mode's panel name.
fn blend_name(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Overlay => "overlay",
    }
}

/// Clip a (possibly off-canvas) region to the canvas,
/// returning (x, y, width, height) in canvas coordinates
/// or None when nothing survives.
fn clip_region(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    canvas_width: u32,
    canvas_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + width as i32).min(canvas_width as i32);
    let y1 = (y + height as i32).min(canvas_height as i32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}
