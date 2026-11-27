// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! The workbench's window shell and session UI.
//!
//! Host choice, recorded per the commission: the workbench
//! lives in `torogao-app`'s egui editor surface, mirroring
//! `paint_window`'s proven winit/egui-wgpu shell, rather
//! than in Slint. Slint is the estate's one GAME face
//! (runtime pin, D-457 line); `torogao-app` is the estate's
//! desktop AUTHORING surface — the paint editor, the stage
//! shell, and now this workbench — and the art loop is
//! authoring work. Building here reuses the pinned
//! egui 0.35 stack with zero new UI dependencies.
//!
//! UI discipline:
//!
//! - Fire buttons are honest: originate disables without a
//!   facade token or while a job is in flight; edit fire
//!   disables while the shared edit queue reads busy (the
//!   button says why). The workbench never pretends a
//!   queue is free.
//! - Admission is a button only the Owner presses, in this
//!   window. Nothing in the job plumbing calls it.
//! - Worker threads do network and subprocess work; all
//!   store writes happen back on the UI thread from the
//!   drained messages, so the record files have one writer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::error::{AppError, AppResult};
use crate::render::Renderer;
use crate::workbench::{
    CLASS_ADMITTED, Candidate, CharacterRecord, MaskCanvas, ROUTE_EDIT_FAITHFUL, ROUTE_EDIT_TURBO,
    RecipeSettings, RgbaImage, Store, Stroke,
};
use crate::workbench_client::{
    EditConfig, EditOutcome, EditQueue, EditSpec, FacadeClient, FacadeQueue, JobPoll, SeatHealth,
    WbConfig, probe_edit_queue, run_edit,
};

/// Runs the desktop art workbench. Returns the process
/// exit code.
pub fn run_workbench() -> AppResult<i32> {
    let store = Store::open(&Store::default_root())
        .map_err(|e| AppError::Cli(format!("workbench store: {e}")))?;
    let event_loop = EventLoop::new()
        .map_err(|err| AppError::Cli(format!("event loop creation failed: {err}")))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = WorkbenchShell::new(store);
    event_loop
        .run_app(&mut shell)
        .map_err(|err| AppError::Cli(format!("event loop failed: {err}")))?;
    Ok(0)
}

// ---------------------------------------------------------------------
// Job plumbing: worker threads report back; the UI thread applies.
// ---------------------------------------------------------------------

struct OriginDone {
    temp_path: PathBuf,
    composed_prompt: String,
    user_prompt: String,
    seed: u64,
    config: serde_json::Value,
}

struct EditDone {
    outcome: EditOutcome,
    canvas_id: String,
    canvas_sha: String,
    prompt: String,
    route: &'static str,
    seed: Option<u64>,
    config: serde_json::Value,
}

enum JobMsg {
    Originated {
        char_id: String,
        result: Result<OriginDone, String>,
    },
    Edited {
        char_id: String,
        result: Result<EditDone, String>,
    },
    Polled {
        health: SeatHealth,
        facade_queue: Option<FacadeQueue>,
        edit_queue: EditQueue,
    },
    Woke(Result<(), String>),
}

/// The workbench session: all state behind the window.
pub struct WorkbenchSession {
    store: Store,
    cfg: WbConfig,
    records: Vec<CharacterRecord>,
    selected: usize,
    prompt_buf: String,
    edit_prompt_buf: String,
    seed: u64,
    status: String,
    log: Vec<String>,
    health: Option<SeatHealth>,
    facade_queue: Option<FacadeQueue>,
    edit_queue: EditQueue,
    last_poll: Instant,
    poll_in_flight: bool,
    originate_in_flight: bool,
    edit_in_flight: bool,
    view_sel: Option<String>,
    canvas_sel: Option<String>,
    compare: bool,
    mask: Option<MaskCanvas>,
    mask_rev: u64,
    stroke_active: bool,
    brush_radius: f32,
    feather: u32,
    edit_config: EditConfig,
    paint_mask: bool,
    tx: Sender<JobMsg>,
    rx: Receiver<JobMsg>,
    textures: HashMap<String, egui::TextureHandle>,
    overlay: Option<(String, u64, egui::TextureHandle)>,
}

impl WorkbenchSession {
    /// A session over an opened store.
    pub fn new(store: Store) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let cfg = WbConfig::from_env();
        let mut status = String::new();
        let records = match store.load_all() {
            Ok(records) => records,
            Err(e) => {
                status = format!("record load failed: {e}");
                Vec::new()
            }
        };
        let selected = records.iter().position(|r| r.id == "prima").unwrap_or(0);
        Self {
            store,
            cfg,
            records,
            selected,
            prompt_buf: String::new(),
            edit_prompt_buf: String::new(),
            seed: 20261127,
            status,
            log: Vec::new(),
            health: None,
            facade_queue: None,
            edit_queue: EditQueue::Unknown,
            last_poll: Instant::now() - Duration::from_secs(60),
            poll_in_flight: false,
            originate_in_flight: false,
            edit_in_flight: false,
            view_sel: None,
            canvas_sel: None,
            compare: false,
            mask: None,
            mask_rev: 0,
            stroke_active: false,
            brush_radius: 48.0,
            feather: 16,
            edit_config: EditConfig::Faithful,
            paint_mask: false,
            tx,
            rx,
            textures: HashMap::new(),
            overlay: None,
        }
    }

    fn push_log(&mut self, line: String) {
        self.log
            .push(format!("{} {line}", crate::workbench::now_iso()));
        if self.log.len() > 10 {
            self.log.drain(0..self.log.len() - 10);
        }
    }

    fn current(&self) -> Option<&CharacterRecord> {
        self.records.get(self.selected)
    }

    fn switch_to(&mut self, idx: usize) {
        if idx == self.selected && self.view_sel.is_some() {
            return;
        }
        self.selected = idx;
        self.view_sel = None;
        self.canvas_sel = None;
        self.compare = false;
        self.mask = None;
        self.mask_rev += 1;
        self.paint_mask = false;
        self.prompt_buf.clear();
        self.edit_prompt_buf.clear();
    }

    // -- job application (UI thread; the store's one writer) --

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                JobMsg::Polled {
                    health,
                    facade_queue,
                    edit_queue,
                } => {
                    self.health = Some(health);
                    self.facade_queue = facade_queue;
                    self.edit_queue = edit_queue;
                    self.poll_in_flight = false;
                }
                JobMsg::Woke(result) => {
                    match &result {
                        Ok(()) => self.push_log("seat wake accepted".to_string()),
                        Err(e) => self.push_log(format!("seat wake failed: {e}")),
                    }
                    self.last_poll = Instant::now() - Duration::from_secs(60);
                }
                JobMsg::Originated { char_id, result } => {
                    self.originate_in_flight = false;
                    self.apply_originated(&char_id, result);
                }
                JobMsg::Edited { char_id, result } => {
                    self.edit_in_flight = false;
                    self.apply_edited(&char_id, result);
                }
            }
        }
    }

    fn apply_originated(&mut self, char_id: &str, result: Result<OriginDone, String>) {
        let done = match result {
            Ok(done) => done,
            Err(e) => {
                self.status = format!("origination failed: {e}");
                self.push_log(format!("originate {char_id}: FAILED {e}"));
                return;
            }
        };
        let Some(idx) = self.records.iter().position(|r| r.id == char_id) else {
            self.push_log(format!("originate {char_id}: record vanished"));
            return;
        };
        let store = self.store.clone();
        let record = &mut self.records[idx];
        match store.add_candidate(
            record,
            crate::workbench::CandidateSpec {
                source_file: done.temp_path,
                route: crate::workbench::ROUTE_ORIGINATE.to_string(),
                prompt: done.composed_prompt,
                seed: Some(done.seed),
                config: done.config,
                canvas: None,
            },
        ) {
            Ok(cand) => {
                let _ = store.bank_prompt(record, "originate", &done.user_prompt);
                self.view_sel = Some(cand.id.clone());
                self.status = format!("{} landed as {} (candidate)", char_id, cand.id);
                self.push_log(format!(
                    "originate {char_id}: {} candidate, sha256 {}",
                    cand.id, cand.sha256
                ));
            }
            Err(e) => {
                self.status = format!("registering origination failed: {e}");
                self.push_log(format!("originate {char_id}: register FAILED {e}"));
            }
        }
    }

    fn apply_edited(&mut self, char_id: &str, result: Result<EditDone, String>) {
        let done = match result {
            Ok(done) => done,
            Err(e) => {
                self.status = format!("edit failed: {e}");
                self.push_log(format!("edit {char_id}: FAILED {e}"));
                return;
            }
        };
        let Some(idx) = self.records.iter().position(|r| r.id == char_id) else {
            self.push_log(format!("edit {char_id}: record vanished"));
            return;
        };
        let store = self.store.clone();
        let record = &mut self.records[idx];
        match store.add_candidate(
            record,
            crate::workbench::CandidateSpec {
                source_file: done.outcome.delivered.clone(),
                route: done.route.to_string(),
                prompt: done.prompt.clone(),
                seed: done.seed,
                config: done.config,
                canvas: Some((done.canvas_id.clone(), done.canvas_sha)),
            },
        ) {
            Ok(cand) => {
                let _ = store.bank_prompt(record, "edit", &done.prompt);
                self.view_sel = Some(cand.id.clone());
                self.status = format!("strike landed as {} (candidate)", cand.id);
                self.push_log(format!(
                    "edit {char_id}: {} candidate over canvas {}, sha256 {}",
                    cand.id, done.canvas_id, cand.sha256
                ));
            }
            Err(e) => {
                self.status = format!("registering edit failed: {e}");
                self.push_log(format!("edit {char_id}: register FAILED {e}"));
            }
        }
    }

    // -- job firing ---------------------------------------------------

    fn fire_edit(&mut self) {
        let Some(record) = self.current().cloned() else {
            return;
        };
        let Some(canvas_id) = self.canvas_sel.clone() else {
            return;
        };
        let Some(canvas) = record.candidate(&canvas_id).cloned() else {
            return;
        };
        let canvas_path = self.store.candidate_path(&canvas);
        let canvas_sha = match crate::workbench::sha256_file(&canvas_path) {
            Ok(sha) => sha,
            Err(e) => {
                self.status = format!("canvas hash failed: {e}");
                return;
            }
        };
        let stamp = crate::workbench::now_iso().replace(':', "");
        let job_dir = self
            .store
            .root()
            .join("jobs")
            .join(format!("ui-{}-{stamp}", record.id));
        if let Err(e) = std::fs::create_dir_all(&job_dir) {
            self.status = format!("job dir: {e}");
            return;
        }
        let use_mask = self.mask.as_ref().map(|m| !m.is_empty()).unwrap_or(false);
        let mask_path = if use_mask {
            let p = job_dir.join("mask.png");
            match self.mask.as_ref().map(|m| m.write_png(&p)) {
                Some(Ok(())) => Some(p),
                Some(Err(e)) => {
                    self.status = format!("mask write failed: {e}");
                    return;
                }
                None => None,
            }
        } else {
            None
        };
        let feather = if use_mask { self.feather } else { 0 };
        let coverage = self.mask.as_ref().map(|m| m.coverage()).unwrap_or(0.0);
        let config_json = serde_json::json!({
            "driver": "edit-local.py edit",
            "config": self.edit_config.route_label(),
            "masked": use_mask,
            "feather": feather,
            "mask_coverage": coverage,
        });
        let spec = EditSpec {
            canvas: canvas_path,
            prompt: self.edit_prompt_buf.clone(),
            out_prefix: job_dir.join("edit"),
            config: self.edit_config,
            seed: Some(self.seed as i64),
            mask: mask_path,
            feather,
        };
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        let char_id = record.id.clone();
        let prompt = self.edit_prompt_buf.clone();
        let route = self.edit_config.route_label();
        let seed = self.seed;
        self.edit_in_flight = true;
        self.status = format!("editing over {canvas_id} ({route})…");
        std::thread::spawn(move || {
            let result = run_edit(&cfg, &spec, Duration::from_secs(900)).map(|outcome| EditDone {
                outcome,
                canvas_id,
                canvas_sha,
                prompt,
                route,
                seed: Some(seed),
                config: config_json,
            });
            let _ = tx.send(JobMsg::Edited { char_id, result });
        });
    }

    fn fire_wake(&mut self) {
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let client = FacadeClient::new(cfg);
            let _ = tx.send(JobMsg::Woke(client.wake()));
        });
    }

    fn maybe_poll(&mut self) {
        if self.poll_in_flight || self.last_poll.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.poll_in_flight = true;
        self.last_poll = Instant::now();
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let client = FacadeClient::new(cfg.clone());
            let health = client.health();
            let facade_queue = client.queue().ok();
            let edit_queue = probe_edit_queue(&cfg);
            let _ = tx.send(JobMsg::Polled {
                health,
                facade_queue,
                edit_queue,
            });
        });
    }

    // -- textures ------------------------------------------------------

    fn texture_for(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        path: &Path,
        max_dim: u32,
    ) -> Option<egui::TextureHandle> {
        if let Some(tex) = self.textures.get(key) {
            return Some(tex.clone());
        }
        let img = crate::workbench::decode_png_file(path).ok()?;
        let img = crate::workbench::downscale(&img, max_dim, max_dim);
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width as usize, img.height as usize],
            &img.pixels,
        );
        let tex = ctx.load_texture(key, color, egui::TextureOptions::LINEAR);
        self.textures.insert(key.to_string(), tex.clone());
        Some(tex)
    }

    fn overlay_texture(
        &mut self,
        ctx: &egui::Context,
        canvas: &Candidate,
    ) -> Option<egui::TextureHandle> {
        let key = format!("overlay/{}", canvas.id);
        if let Some((k, rev, tex)) = &self.overlay {
            if *k == key && *rev == self.mask_rev {
                return Some(tex.clone());
            }
        }
        let mask = self.mask.as_ref()?;
        let path = self.store.candidate_path(canvas);
        let base = crate::workbench::decode_png_file(&path).ok()?;
        let base = crate::workbench::downscale(&base, 720, 720);
        let full = mask.rasterize();
        // Nearest-sample the full-res mask into the preview.
        let mut pixels = base.pixels.clone();
        for y in 0..base.height {
            let my = (y * mask.height / base.height.max(1)).min(mask.height - 1);
            for x in 0..base.width {
                let mx = (x * mask.width / base.width.max(1)).min(mask.width - 1);
                if full[(my * mask.width + mx) as usize] > 0 {
                    let i = ((y * base.width + x) * 4) as usize;
                    pixels[i] = 255;
                    pixels[i + 1] /= 3;
                    pixels[i + 2] /= 3;
                }
            }
        }
        let img = RgbaImage {
            width: base.width,
            height: base.height,
            pixels,
        };
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width as usize, img.height as usize],
            &img.pixels,
        );
        let tex = ctx.load_texture(&key, color, egui::TextureOptions::LINEAR);
        self.overlay = Some((key, self.mask_rev, tex.clone()));
        Some(tex)
    }

    // -- the frame ------------------------------------------------------

    /// Draw one frame. Called by the shell on every redraw.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.drain_messages();
        self.maybe_poll();
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(700));

        // Status / queue honesty bar.
        egui::Panel::top("wb-top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Torogao Art Workbench");
                ui.separator();
                match &self.health {
                    Some(SeatHealth::Up { engine, seat }) => {
                        ui.label(format!("seat: {engine} / {seat}"));
                        if seat != "ready" && ui.button("Wake seat").clicked() {
                            self.fire_wake();
                        }
                    }
                    Some(SeatHealth::Down(why)) => {
                        ui.label(format!("facade down: {why}"));
                    }
                    None => {
                        ui.label("seat: …");
                    }
                }
                ui.separator();
                match &self.facade_queue {
                    Some(q) => ui.label(format!(
                        "facade queue: {} queued, running={}",
                        q.queued, q.running
                    )),
                    None => ui.label("facade queue: ?"),
                };
                ui.separator();
                match self.edit_queue {
                    EditQueue::Idle => {
                        ui.label("edit queue: idle");
                    }
                    EditQueue::Busy { running, pending } => {
                        ui.label(format!(
                            "edit queue: BUSY ({running} running, {pending} pending)"
                        ));
                    }
                    EditQueue::Unknown => {
                        ui.label("edit queue: ?");
                    }
                }
                if !self.cfg.has_token() {
                    ui.separator();
                    ui.label("no facade token held — origination refuses");
                }
            });
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
        });

        // Character picker.
        let picker: Vec<(String, String, String)> = self
            .records
            .iter()
            .map(|r| (r.id.clone(), r.name.clone(), r.cast.clone()))
            .collect();
        egui::Panel::left("wb-chars").show(ui, |ui| {
            ui.heading("Characters");
            let mut clicked: Option<usize> = None;
            for (i, (id, name, cast)) in picker.iter().enumerate() {
                let label = format!("{name} · {cast}");
                if ui.selectable_label(i == self.selected, label).clicked() {
                    clicked = Some(i);
                }
                let _ = id;
            }
            if let Some(i) = clicked {
                self.switch_to(i);
            }
        });

        // Event log.
        let log = self.log.clone();
        egui::Panel::bottom("wb-log").show(ui, |ui| {
            for line in &log {
                ui.small(line);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.central(ui, &ctx);
            });
        });
    }

    fn central(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(record) = self.current().cloned() else {
            ui.label("no character records loaded");
            return;
        };
        ui.heading(format!("{} — {}", record.name, record.cast));
        ui.small(format!(
            "identity source: {}",
            record.prompt_identity.source
        ));
        ui.label(format!("identity: {}", record.prompt_identity.identity));
        ui.label(format!("signature: {}", record.prompt_identity.signature));
        if !record.prompt_identity.negative_guards.is_empty() {
            ui.label(format!(
                "negative guards: {}",
                record.prompt_identity.negative_guards
            ));
        }
        ui.separator();

        // --- Originate ------------------------------------------------
        ui.heading("Originate (wai seat, via the governed facade)");
        ui.label(format!(
            "recipe of record: {}x{}, {} steps, cfg {}, {}, {}",
            RecipeSettings::of_record().width,
            RecipeSettings::of_record().height,
            RecipeSettings::of_record().steps,
            RecipeSettings::of_record().cfg,
            crate::workbench::SAMPLER,
            crate::workbench::CHECKPOINT,
        ));
        ui.add(
            egui::TextEdit::multiline(&mut self.prompt_buf)
                .hint_text("scene prompt — appended after her identity and signature")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            ui.label("seed");
            ui.add(egui::DragValue::new(&mut self.seed).speed(1.0));
            let can_fire = !self.originate_in_flight
                && self.cfg.has_token()
                && matches!(&self.health, Some(SeatHealth::Up { .. }));
            let label = if self.originate_in_flight {
                "originating…"
            } else {
                "Originate candidate"
            };
            if ui.add_enabled(can_fire, egui::Button::new(label)).clicked() {
                self.fire_originate(&record);
            }
        });

        // --- Prompt history -------------------------------------------
        if !record.prompt_history.is_empty() {
            ui.collapsing("Banked prompt history (click to reuse)", |ui| {
                let mut load: Option<(String, String)> = None;
                for entry in record.prompt_history.iter().rev().take(24) {
                    let text: String = entry.text.chars().take(72).collect();
                    if ui
                        .small_button(format!("[{}] {text}", entry.kind))
                        .clicked()
                    {
                        load = Some((entry.kind.clone(), entry.text.clone()));
                    }
                }
                if let Some((kind, text)) = load {
                    if kind == "edit" {
                        self.edit_prompt_buf = text;
                    } else {
                        self.prompt_buf = text;
                    }
                }
            });
        }
        ui.separator();

        // --- Gallery ---------------------------------------------------
        ui.heading("Candidates");
        if record.art.candidates.is_empty() {
            ui.label("no candidates yet — originate one above");
        }
        let candidates = record.art.candidates.clone();
        ui.horizontal_wrapped(|ui| {
            for cand in &candidates {
                let key = format!("{}/{}#thumb", record.id, cand.id);
                let path = self.store.candidate_path(cand);
                ui.vertical(|ui| {
                    if let Some(tex) = self.texture_for(ctx, &key, &path, 220) {
                        let size = tex.size_vec2();
                        let scale = 150.0 / size.y.max(1.0);
                        let shown = egui::vec2(size.x * scale, 150.0);
                        if ui
                            .add(egui::Image::new((tex.id(), shown)).sense(egui::Sense::click()))
                            .clicked()
                        {
                            self.view_sel = Some(cand.id.clone());
                        }
                    } else {
                        ui.label("(unreadable png)");
                    }
                    let badge = if cand.class == CLASS_ADMITTED {
                        "ADMITTED"
                    } else {
                        "candidate"
                    };
                    ui.small(format!("{} · {}", cand.id, badge));
                    ui.small(route_short(&cand.route));
                });
            }
        });

        // --- Selected candidate: judging view + provenance -------------
        let view_id = self
            .view_sel
            .clone()
            .or_else(|| candidates.last().map(|c| c.id.clone()));
        if let Some(view_id) = view_id {
            if let Some(cand) = record.candidate(&view_id).cloned() {
                ui.separator();
                self.candidate_detail(ui, ctx, &record, &cand);
            }
        }

        // --- Edit panel --------------------------------------------------
        ui.separator();
        self.edit_panel(ui, ctx, &record);
    }

    fn candidate_detail(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        record: &CharacterRecord,
        cand: &Candidate,
    ) {
        ui.heading(format!("{} — {}", cand.id, cand.class));
        let compare_with = if self.compare {
            cand.canvas_candidate
                .as_ref()
                .and_then(|cid| record.candidate(cid).cloned())
        } else {
            None
        };
        if let Some(canvas) = compare_with {
            ui.horizontal(|ui| {
                for (label, c) in [("CANVAS", &canvas), ("STRIKE", cand)] {
                    ui.vertical(|ui| {
                        ui.strong(format!("{label} {}", c.id));
                        let key = format!("{}/{}", record.id, c.id);
                        let path = self.store.candidate_path(c);
                        if let Some(tex) = self.texture_for(ctx, &key, &path, 900) {
                            let size = tex.size_vec2();
                            let scale = (430.0 / size.x).min(620.0 / size.y).min(1.0);
                            ui.image((tex.id(), size * scale));
                        }
                    });
                }
            });
        } else {
            let key = format!("{}/{}", record.id, cand.id);
            let path = self.store.candidate_path(cand);
            if let Some(tex) = self.texture_for(ctx, &key, &path, 1100) {
                let size = tex.size_vec2();
                let avail = ui.available_width();
                let scale = (avail / size.x).min(680.0 / size.y).min(1.0);
                ui.image((tex.id(), size * scale));
            } else {
                ui.label("(candidate png unreadable)");
            }
        }
        // Provenance — the whole record, verbatim where it counts.
        ui.label(format!("route: {}", cand.route));
        if let Some(seed) = cand.seed {
            ui.label(format!("seed: {seed}"));
        }
        ui.label(format!("created: {}", cand.created_at));
        ui.label(format!("file: {} ", cand.file));
        ui.label(format!("sha256: {}", cand.sha256));
        if let (Some(cid), Some(sha)) = (&cand.canvas_candidate, &cand.canvas_sha256) {
            ui.label(format!("canvas: {cid} (sha256 {sha})"));
        }
        ui.label(format!("config: {}", cand.config));
        ui.label("prompt (verbatim):");
        let mut prompt_view = cand.prompt.clone();
        ui.add(
            egui::TextEdit::multiline(&mut prompt_view)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .interactive(false),
        );
        ui.horizontal(|ui| {
            if ui.button("Use as edit canvas").clicked() {
                self.canvas_sel = Some(cand.id.clone());
                self.mask = None;
                self.mask_rev += 1;
                self.paint_mask = false;
                self.status = format!("edit canvas: {}", cand.id);
            }
            if cand.canvas_candidate.is_some() {
                ui.checkbox(&mut self.compare, "compare canvas | strike");
            }
            let admitted = cand.class == CLASS_ADMITTED;
            let label = if admitted {
                "Un-admit (back to candidate)"
            } else {
                "Admit — the Owner's act"
            };
            if ui.button(label).clicked() {
                let store = self.store.clone();
                if let Some(rec) = self.records.get_mut(self.selected) {
                    match store.set_admitted(rec, &cand.id, !admitted) {
                        Ok(()) => {
                            self.status = format!(
                                "{} {} by Owner act",
                                cand.id,
                                if admitted { "un-admitted" } else { "ADMITTED" }
                            );
                            self.push_log(format!(
                                "admission: {} -> {}",
                                cand.id,
                                if admitted { "candidate" } else { "admitted" }
                            ));
                        }
                        Err(e) => self.status = format!("admission failed: {e}"),
                    }
                }
            }
        });
    }

    fn edit_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, record: &CharacterRecord) {
        ui.heading("Edit (primo-local edit route)");
        let Some(canvas_id) = self.canvas_sel.clone() else {
            ui.label("pick a candidate above and choose “Use as edit canvas”");
            return;
        };
        let Some(canvas) = record.candidate(&canvas_id).cloned() else {
            ui.label("the chosen canvas no longer exists on this record");
            return;
        };
        ui.label(format!("canvas: {} ({})", canvas.id, canvas.route));

        // Canvas preview with mask painting.
        let path = self.store.candidate_path(&canvas);
        let dims = crate::workbench::decode_png_file(&path)
            .map(|img| (img.width, img.height))
            .unwrap_or((0, 0));
        if self.mask.is_none() && dims.0 > 0 {
            self.mask = Some(MaskCanvas::new(dims.0, dims.1));
            self.mask_rev += 1;
        }
        let show_overlay = self.mask.as_ref().map(|m| !m.is_empty()).unwrap_or(false);
        let tex = if show_overlay {
            self.overlay_texture(ctx, &canvas)
        } else {
            let key = format!("{}/{}#edit", record.id, canvas.id);
            self.texture_for(ctx, &key, &path, 720)
        };
        if let Some(tex) = tex {
            let size = tex.size_vec2();
            let scale = (560.0 / size.x).min(560.0 / size.y).min(1.0);
            let shown = size * scale;
            let resp =
                ui.add(egui::Image::new((tex.id(), shown)).sense(egui::Sense::click_and_drag()));
            if self.paint_mask && dims.0 > 0 {
                self.paint_on(&resp, dims);
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.paint_mask, "paint mask");
            ui.label("brush");
            ui.add(egui::Slider::new(&mut self.brush_radius, 4.0..=220.0).text("px"));
            ui.label("feather");
            ui.add(egui::Slider::new(&mut self.feather, 0..=64).text("px"));
            if ui.button("Clear mask").clicked() {
                if let Some(mask) = &mut self.mask {
                    mask.strokes.clear();
                    self.mask_rev += 1;
                }
            }
            if let Some(mask) = &self.mask {
                ui.label(format!("coverage {:.1}%", mask.coverage() * 100.0));
            }
        });

        ui.add(
            egui::TextEdit::multiline(&mut self.edit_prompt_buf)
                .hint_text("edit instruction — say what changes; the route holds the rest")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut self.edit_config,
                EditConfig::Faithful,
                EditConfig::Faithful.ui_label(),
            );
            ui.radio_value(
                &mut self.edit_config,
                EditConfig::Turbo,
                EditConfig::Turbo.ui_label(),
            );
        });
        // Queue honesty: the fire button names its blocker.
        let (can_fire, why) = if self.edit_in_flight {
            (false, "an edit is already in flight")
        } else if self.edit_prompt_buf.trim().is_empty() {
            (false, "write the edit instruction first")
        } else {
            match self.edit_queue {
                EditQueue::Idle => (true, ""),
                EditQueue::Busy { .. } => (
                    false,
                    "the shared edit queue is busy — fire is disabled, honestly",
                ),
                EditQueue::Unknown => (false, "edit queue state unknown — waiting for a probe"),
            }
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_fire, egui::Button::new("Fire edit"))
                .clicked()
            {
                self.fire_edit();
            }
            if !why.is_empty() {
                ui.label(why);
            }
            if !self.cfg.driver_present() {
                ui.label("edit driver not found on this machine");
            }
        });
    }

    fn paint_on(&mut self, resp: &egui::Response, dims: (u32, u32)) {
        let Some(pos) = resp.interact_pointer_pos() else {
            return;
        };
        let rect = resp.rect;
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        let uv = (pos - rect.min) / rect.size();
        if !(0.0..=1.0).contains(&uv.x) || !(0.0..=1.0).contains(&uv.y) {
            return;
        }
        let point = (uv.x * dims.0 as f32, uv.y * dims.1 as f32);
        if resp.drag_started() || !self.stroke_active {
            if let Some(mask) = &mut self.mask {
                mask.strokes.push(Stroke {
                    points: vec![point],
                    radius: self.brush_radius,
                });
            }
            self.stroke_active = true;
        }
        if resp.dragged() {
            if let Some(mask) = &mut self.mask {
                if let Some(stroke) = mask.strokes.last_mut() {
                    let append = stroke
                        .points
                        .last()
                        .map(|(lx, ly)| {
                            let dx = point.0 - lx;
                            let dy = point.1 - ly;
                            dx * dx + dy * dy > 9.0
                        })
                        .unwrap_or(true);
                    if append {
                        stroke.points.push(point);
                    }
                }
            }
        }
        if resp.drag_stopped() {
            self.stroke_active = false;
            self.mask_rev += 1;
        }
    }
}

/// Origination worker: health → wake if needed → submit →
/// poll → fetch → write the still into the store's job dir.
/// Returns everything the UI thread needs to register the
/// candidate with full provenance.
fn originate_worker(
    cfg: WbConfig,
    record: CharacterRecord,
    user_prompt: String,
    seed: u64,
    store_root: PathBuf,
) -> Result<OriginDone, String> {
    let client = FacadeClient::new(cfg);
    match client.health() {
        SeatHealth::Up { seat, .. } => {
            if seat != "ready" {
                client.wake()?;
            }
        }
        SeatHealth::Down(why) => return Err(format!("facade health: {why}")),
    }
    let recipe = RecipeSettings::of_record();
    let prompt_id = client.submit(&record, &user_prompt, seed, &recipe)?;
    let deadline = Instant::now() + Duration::from_secs(420);
    let filename = loop {
        match client.poll(&prompt_id)? {
            JobPoll::Done(name) => break name,
            JobPoll::Failed(why) => return Err(why),
            JobPoll::Pending => {
                if Instant::now() > deadline {
                    return Err("seat timed out".to_string());
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        }
    };
    let png = client.fetch_view(&filename)?;
    let stamp = crate::workbench::now_iso().replace(':', "");
    let dir = store_root.join("jobs").join(format!("ui-origin-{stamp}"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("job dir: {e}"))?;
    let temp_path = dir.join("originated.png");
    std::fs::write(&temp_path, &png).map_err(|e| format!("write still: {e}"))?;
    let composed = crate::workbench::compose_positive(&record, &user_prompt);
    let config = serde_json::json!({
        "steps": recipe.steps, "cfg": recipe.cfg,
        "width": recipe.width, "height": recipe.height,
        "sampler": crate::workbench::SAMPLER,
        "scheduler": crate::workbench::SCHEDULER,
        "checkpoint": crate::workbench::CHECKPOINT,
        "prompt_id": prompt_id,
        "seat_file": filename,
    });
    Ok(OriginDone {
        temp_path,
        composed_prompt: composed,
        user_prompt,
        seed,
        config,
    })
}

fn route_short(route: &str) -> &'static str {
    match route {
        r if r == crate::workbench::ROUTE_ORIGINATE => "originated",
        r if r == ROUTE_EDIT_FAITHFUL => "edit · faithful",
        r if r == ROUTE_EDIT_TURBO => "edit · turbo",
        _ => "other route",
    }
}

impl WorkbenchSession {
    /// Originate for `record` (the closure passes the
    /// snapshot it rendered from; the worker re-reads the
    /// record id from it).
    fn fire_originate(&mut self, record: &CharacterRecord) {
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        let user_prompt = self.prompt_buf.clone();
        let seed = self.seed;
        let root = self.store.root().to_path_buf();
        let record = record.clone();
        let char_id = record.id.clone();
        self.originate_in_flight = true;
        self.status = format!("originating {} on the wai seat…", record.name);
        std::thread::spawn(move || {
            let result = originate_worker(cfg, record, user_prompt, seed, root);
            let _ = tx.send(JobMsg::Originated { char_id, result });
        });
    }
}

// ---------------------------------------------------------------------
// The shell: paint_window's proven winit/egui-wgpu boilerplate,
// carrying a WorkbenchSession.
// ---------------------------------------------------------------------

struct WorkbenchShell {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    surface: Option<wgpu::Surface<'static>>,
    surface_format: wgpu::TextureFormat,
    egui_context: egui::Context,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    session: WorkbenchSession,
    status: String,
}

impl WorkbenchShell {
    fn new(store: Store) -> Self {
        Self {
            window: None,
            renderer: None,
            surface: None,
            surface_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            egui_context: egui::Context::default(),
            egui_state: None,
            egui_renderer: None,
            session: WorkbenchSession::new(store),
            status: String::new(),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Torogao Art Workbench")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 1000.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.status = format!("window creation failed: {err}");
                event_loop.exit();
                return;
            }
        };
        match Renderer::new() {
            Ok(renderer) => match renderer.create_surface(window.clone()) {
                Ok(surface) => {
                    let format = renderer.surface_format(&surface);
                    let size = window.inner_size();
                    surface.configure(
                        renderer.device(),
                        &wgpu::SurfaceConfiguration {
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            format,
                            width: size.width.max(1),
                            height: size.height.max(1),
                            present_mode: wgpu::PresentMode::Fifo,
                            desired_maximum_frame_latency: 2,
                            alpha_mode: wgpu::CompositeAlphaMode::Auto,
                            view_formats: vec![],
                        },
                    );
                    self.egui_renderer = Some(egui_wgpu::Renderer::new(
                        renderer.device(),
                        format,
                        egui_wgpu::RendererOptions::default(),
                    ));
                    self.surface_format = format;
                    self.surface = Some(surface);
                    self.renderer = Some(renderer);
                }
                Err(err) => {
                    self.status = format!("{err}");
                }
            },
            Err(err) => {
                self.status = format!("renderer unavailable: {err}");
            }
        }
        self.egui_state = Some(egui_winit::State::new(
            self.egui_context.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        ));
        self.window = Some(window);
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(mut egui_state) = self.egui_state.take() else {
            return;
        };
        let raw_input = egui_state.take_egui_input(&window);
        let status = self.status.clone();
        let full_output = self.egui_context.clone().run_ui(raw_input, |ui| {
            if !status.is_empty() {
                egui::Panel::top("wb-shell-status").show(ui, |ui| {
                    ui.label(&status);
                });
            }
            self.session.show(ui);
        });
        egui_state.handle_platform_output(&window, full_output.platform_output);
        self.egui_state = Some(egui_state);
        let jobs = self
            .egui_context
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let (Some(surface), Some(renderer), Some(egui_renderer)) = (
            self.surface.as_ref(),
            self.renderer.as_ref(),
            self.egui_renderer.as_mut(),
        ) else {
            return;
        };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            renderer
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("torogao workbench encoder"),
                });
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [frame.texture.width(), frame.texture.height()],
            pixels_per_point: full_output.pixels_per_point,
        };
        for (id, delta) in &full_output.textures_delta.set {
            egui_renderer.update_texture(renderer.device(), renderer.queue(), *id, delta);
        }
        let extra_buffers = egui_renderer.update_buffers(
            renderer.device(),
            renderer.queue(),
            &mut encoder,
            &jobs,
            &screen,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("torogao workbench pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.08,
                                g: 0.08,
                                b: 0.09,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            egui_renderer.render(&mut pass, &jobs, &screen);
        }
        for id in &full_output.textures_delta.free {
            egui_renderer.free_texture(id);
        }
        let mut submissions = vec![encoder.finish()];
        submissions.extend(extra_buffers);
        renderer.queue().submit(submissions);
        frame.present();
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if let (Some(surface), Some(renderer)) = (self.surface.as_ref(), self.renderer.as_ref()) {
            surface.configure(
                renderer.device(),
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: self.surface_format,
                    width,
                    height,
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 2,
                    alpha_mode: wgpu::CompositeAlphaMode::Auto,
                    view_formats: vec![],
                },
            );
        }
    }
}

impl ApplicationHandler for WorkbenchShell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(|window| window.id()) != Some(window_id) {
            return;
        }
        if let (Some(egui_state), Some(window)) = (self.egui_state.as_mut(), self.window.clone()) {
            let response = egui_state.on_window_event(&window, &event);
            if response.repaint {
                window.request_redraw();
            }
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.resize(size.width, size.height);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
