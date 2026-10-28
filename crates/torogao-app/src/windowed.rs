//! The windowed shell: a winit window with egui panels —
//! a node tree (selection writes the shared app selection
//! state), a parameter panel (edits write through the
//! engine and re-evaluate), and the viewport showing the
//! renderer's output. The renderer and the evaluation path
//! are the same ones the headless mode uses; only the
//! window is extra.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use torogao_engine::Puppet;
use torogao_session::host::LiveBridge;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::error::{AppError, AppResult};
use crate::persona::{CommandKind, NativeCommand};
use crate::player::PlayHost;
use crate::render::{Renderer, VIEWPORT_SIDE_MAX, cap_canvas, prepare_frame};
use crate::scene::{STEP_SECONDS, Scene};
use crate::session_host::{AppBridge, BridgeRequest};
use crate::state::{Selection, ShellState};

/// Deepest tree depth the node panel renders.
const TREE_DEPTH_MAX: usize = 64;
/// Largest catch-up steps one frame may take while playing
/// (the fixed-step clock never bursts unboundedly).
const CATCH_UP_STEPS_MAX: u64 = 4;

/// Options for attaching the session server to the
/// windowed shell (windowed co-presence).
#[derive(Debug, Clone)]
pub struct WindowSessionOptions {
    /// The session dir (capability token file home).
    pub dir: std::path::PathBuf,
    /// The loopback WebSocket port.
    pub port: u16,
    /// Provenance label recorded in the session record.
    pub held_by: String,
}

/// One transport edit collected from the transport row.
enum TransportEdit {
    Play,
    Pause,
    Stop,
    Step,
    Seek(i32),
    SetLoop(bool),
}

/// The offscreen texture behind the viewport widget.
struct ViewportTexture {
    id: egui::TextureId,
    texture: wgpu::Texture,
    size: [u32; 2],
}

/// The windowed shell's full state.
struct Shell {
    state: ShellState,
    play: PlayHost,
    model_path_input: String,
    pending_model: Option<std::path::PathBuf>,
    expanded: BTreeSet<usize>,
    viewport_dirty: bool,
    renderer: Option<Renderer>,
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    surface_format: wgpu::TextureFormat,
    egui_context: egui::Context,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    viewport: Option<ViewportTexture>,
    session_options: Option<WindowSessionOptions>,
    session_rx: Option<std::sync::mpsc::Receiver<BridgeRequest>>,
    session_note: String,
    last_step: std::time::Instant,
    /// Whether the physics debug overlay (anchor-to-bob
    /// segments) draws over the viewport.
    debug_physics: bool,
    /// The save-path text field (session/scene saves).
    save_path_input: String,
    /// A queued save request from the panel.
    pending_save: Option<std::path::PathBuf>,
    /// A queued load request from the panel.
    pending_load: Option<std::path::PathBuf>,
    /// The Tools panel's selected registry tool.
    tool_selected: Option<String>,
    /// The Tools panel's arguments editor text.
    tool_args: String,
    /// The Tools panel's result text (verbatim registry
    /// output or the typed refusal).
    tool_result: String,
    /// The Persona presentation state (Addition Y): the
    /// default UI. `persona.nerd_mode` selects the
    /// traditional panels instead.
    persona: crate::persona::PersonaState,
    /// The Persona motion clock's last-step instant (the
    /// fixed-step accumulator `advance_persona` runs on).
    persona_last_step: std::time::Instant,
}

impl Shell {
    fn new(model_path: Option<&Path>, session_options: Option<WindowSessionOptions>) -> Shell {
        // The persisted presentation preference (Addition
        // S state): Nerd mode only when a saved preference
        // says so — the Persona stage is the default.
        let preference = crate::persona::preference_path()
            .map(|path| crate::persona::load_preference(&path))
            .unwrap_or_default();
        let mut persona = crate::persona::PersonaState::new();
        persona.nerd_mode = preference.nerd_mode;
        Shell {
            state: ShellState::new(),
            play: PlayHost::new(),
            model_path_input: model_path
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            pending_model: model_path.map(Path::to_path_buf),
            expanded: BTreeSet::new(),
            viewport_dirty: true,
            renderer: None,
            window: None,
            surface: None,
            surface_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            egui_context: egui::Context::default(),
            egui_state: None,
            egui_renderer: None,
            viewport: None,
            session_options,
            session_rx: None,
            session_note: String::new(),
            last_step: std::time::Instant::now(),
            debug_physics: false,
            save_path_input: String::new(),
            pending_save: None,
            pending_load: None,
            tool_selected: None,
            tool_args: "{}".to_string(),
            tool_result: String::new(),
            persona,
            persona_last_step: std::time::Instant::now(),
        }
    }

    /// Loads a model file into the shell and evaluates it at
    /// its default parameters.
    fn load_model(&mut self, path: &Path) {
        match Scene::load_file(path).and_then(|mut scene| {
            scene.evaluate_fresh(&[])?;
            Ok(scene)
        }) {
            Ok(scene) => {
                self.state.install_scene(scene);
                self.play = PlayHost::new();
                self.last_step = std::time::Instant::now();
                self.expanded.clear();
                if let Some(scene) = &self.state.scene {
                    self.expanded.insert(scene.puppet.nodes.root);
                }
                self.viewport_dirty = true;
                self.maybe_start_session();
            }
            Err(err) => {
                self.state.status = format!("load failed: {err}");
            }
        }
    }

    /// Attaches the session server once, when session
    /// options were given and a scene is installed: the
    /// genesis projects from the standing scene, and the
    /// WebSocket bridge forwards into this frame loop (see
    /// `drain_session_requests`).
    fn maybe_start_session(&mut self) {
        if self.session_rx.is_some() {
            return;
        }
        let Some(options) = self.session_options.clone() else {
            return;
        };
        match crate::session_host::start_window_session(
            &self.state,
            &options.dir,
            options.port,
            &options.held_by,
        ) {
            Ok((rx, session_id)) => {
                self.session_rx = Some(rx);
                self.session_note =
                    format!("session {session_id} on ws://127.0.0.1:{}", options.port);
            }
            Err(err) => {
                self.session_note = format!("session attach failed: {err}");
            }
        }
    }

    /// Serves every queued session request through an
    /// `AppBridge` over this shell's own state + play host,
    /// so admitted ops and play drives land in the same
    /// puppet the window evaluates. Runs at the top of
    /// every redraw; requests never wait longer than a
    /// frame.
    fn drain_session_requests(&mut self) {
        let Some(rx) = self.session_rx.take() else {
            return;
        };
        let mut pending: Vec<BridgeRequest> = Vec::new();
        while let Ok(request) = rx.try_recv() {
            pending.push(request);
        }
        self.session_rx = Some(rx);
        if pending.is_empty() {
            return;
        }
        let shell = std::mem::replace(&mut self.state, ShellState::new());
        let play = std::mem::replace(&mut self.play, PlayHost::new());
        let mut bridge = AppBridge::with_player(shell, play);
        for request in pending {
            match request {
                BridgeRequest::Apply { op, reply } => {
                    let _ = reply.send(bridge.apply_live(&op));
                }
                BridgeRequest::EvalHash { reply } => {
                    let _ = reply.send(bridge.evaluated_hash());
                }
                BridgeRequest::PlayState { reply } => {
                    let _ = reply.send(bridge.play_state());
                }
                BridgeRequest::PlayControl {
                    action,
                    args,
                    reply,
                } => {
                    let _ = reply.send(bridge.play_control(&action, &args));
                }
            }
        }
        let (shell, play) = bridge.into_parts();
        self.state = shell;
        self.play = play;
        self.viewport_dirty = true;
    }

    /// Advances playback on the fixed-step clock while the
    /// transport is running: whole steps only, bounded
    /// catch-up, the remainder carried in `last_step`.
    fn advance_playback(&mut self) {
        if !self.play.is_advancing() {
            self.last_step = std::time::Instant::now();
            return;
        }
        let elapsed = self.last_step.elapsed().as_secs_f64();
        let mut steps = (elapsed / STEP_SECONDS).floor() as u64;
        if steps == 0 {
            return;
        }
        if steps > CATCH_UP_STEPS_MAX {
            steps = CATCH_UP_STEPS_MAX;
            self.last_step = std::time::Instant::now();
        } else {
            self.last_step += std::time::Duration::from_secs_f64(steps as f64 * STEP_SECONDS);
        }
        if let Some(scene) = self.state.scene.as_mut() {
            match self.play.step(scene, steps) {
                Ok(()) => self.viewport_dirty = true,
                Err(err) => {
                    self.state.status = format!("playback failed: {err}");
                }
            }
        }
    }

    /// Advances the Persona motion state on the same
    /// fixed-step clock as playback (STEP_SECONDS, bounded
    /// catch-up): menu motion is stepped, never wall-clock
    /// free-running — the determinism contract covers the
    /// stage as well as the puppet.
    fn advance_persona(&mut self) {
        let elapsed = self.persona_last_step.elapsed().as_secs_f64();
        let mut steps = (elapsed / STEP_SECONDS).floor() as u64;
        if steps == 0 {
            return;
        }
        if steps > CATCH_UP_STEPS_MAX {
            steps = CATCH_UP_STEPS_MAX;
            self.persona_last_step = std::time::Instant::now();
        } else {
            self.persona_last_step +=
                std::time::Duration::from_secs_f64(steps as f64 * STEP_SECONDS);
        }
        for _ in 0..steps {
            self.persona.tick();
        }
    }

    /// Flips the presentation between the Persona stage
    /// and Nerd mode, persisting the choice as Addition S
    /// state (the preference file is written only here,
    /// when the toggle changes).
    fn toggle_nerd_mode(&mut self) {
        self.persona.nerd_mode = !self.persona.nerd_mode;
        if let Some(path) = crate::persona::preference_path() {
            let pref = crate::persona::PreferenceState {
                nerd_mode: self.persona.nerd_mode,
            };
            if let Err(err) = crate::persona::save_preference(&path, &pref) {
                self.state.status = format!("preference save failed: {err}");
            }
        }
    }

    /// Runs the currently selected registry tool with the
    /// arguments editor's text — the same call and the
    /// same verbatim result format the Nerd Tools panel
    /// uses (one skeleton under both presentations).
    fn run_selected_tool(&mut self) {
        match &self.tool_selected {
            None => {
                self.tool_result = "select a tool first".to_string();
            }
            Some(name) => {
                let name = name.clone();
                match serde_json::from_str::<serde_json::Value>(&self.tool_args) {
                    Ok(args) => {
                        let result = crate::tools::call(&name, &args);
                        self.tool_result = if result.is_error {
                            format!("REFUSED {}: {}", result.error_code, result.error_message)
                        } else {
                            serde_json::to_string_pretty(&result.payload)
                                .unwrap_or_else(|_| result.payload.to_string())
                        };
                    }
                    Err(err) => {
                        self.tool_result = format!("arguments are not valid JSON: {err}");
                    }
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Torogao")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.state.status = format!("window creation failed: {err}");
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
                    self.state.status = format!("{err}");
                }
            },
            Err(err) => {
                // No GPU: the panels still work; the viewport
                // reports the renderer's absence.
                self.state.status = format!("renderer unavailable: {err}");
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
        if let Some(path) = self.pending_model.take() {
            self.load_model(&path);
        }
    }

    fn build_ui(&mut self, ui: &mut egui::Ui) {
        // The presentation hotkey (F9), live in both
        // presentations whenever no text field holds the
        // keyboard: the Nerd-mode toggle must be reachable
        // from both modes (Addition Y).
        let text_focused = ui.ctx().memory(|memory| memory.focused().is_some());
        if !text_focused && ui.ctx().input(|input| input.key_pressed(egui::Key::F9)) {
            self.toggle_nerd_mode();
        }
        if !self.persona.nerd_mode {
            self.build_persona_ui(ui);
            return;
        }
        let mut scene = self.state.scene.take();
        let mut new_status: Option<String> = None;
        let mut transport: Option<TransportEdit> = None;
        egui::Panel::top("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Torogao");
                if ui
                    .button("Persona stage (F9)")
                    .on_hover_text("Return to the Persona presentation (Addition Y)")
                    .clicked()
                {
                    self.toggle_nerd_mode();
                }
                ui.separator();
                ui.label(&self.state.status);
                if !self.session_note.is_empty() {
                    ui.separator();
                    ui.label(&self.session_note);
                }
            });
        });
        egui::Panel::left("nodes")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui| {
                ui.heading("Model");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.model_path_input);
                    if ui.button("Load").clicked() {
                        let path = std::path::PathBuf::from(self.model_path_input.trim());
                        if !path.as_os_str().is_empty() {
                            self.pending_model = Some(path);
                        }
                    }
                });
                if ui
                    .checkbox(&mut self.debug_physics, "Physics debug overlay")
                    .changed()
                {
                    self.viewport_dirty = true;
                }
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.save_path_input);
                    if ui.button("Save state").clicked() {
                        let path = std::path::PathBuf::from(self.save_path_input.trim());
                        if !path.as_os_str().is_empty() {
                            self.pending_save = Some(path);
                        }
                    }
                    if ui.button("Load state").clicked() {
                        let path = std::path::PathBuf::from(self.save_path_input.trim());
                        if !path.as_os_str().is_empty() {
                            self.pending_load = Some(path);
                        }
                    }
                });
                ui.separator();
                match &scene {
                    Some(scene) => {
                        ui.label(format!(
                            "{} — {} nodes, {} parameters",
                            scene.model_name(),
                            scene.puppet.nodes.nodes.len(),
                            scene.puppet.params.len(),
                        ));
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_salt("node-tree")
                            .max_height(ui.available_height() * 0.6)
                            .show(ui, |ui| {
                                let root = scene.puppet.nodes.root;
                                tree_node(
                                    ui,
                                    &scene.puppet,
                                    root,
                                    0,
                                    &mut self.state.selection,
                                    &mut self.expanded,
                                );
                            });
                        if let Some(index) = self.state.selection.node_index {
                            ui.separator();
                            node_details(ui, &scene.puppet, index);
                        }
                    }
                    None => {
                        ui.label("No model loaded. Enter an .inx path and choose Load.");
                    }
                }
            });
        egui::Panel::right("parameters")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                ui.heading("Parameters");
                match &mut scene {
                    Some(scene) => {
                        let mut edit: Option<(usize, torogao_engine::Vec2)> = None;
                        egui::ScrollArea::vertical()
                            .id_salt("parameter-list")
                            .show(ui, |ui| {
                                for index in 0..scene.puppet.params.len() {
                                    if let Some(result) = parameter_row(ui, scene, index) {
                                        edit = Some(result);
                                    }
                                }
                            });
                        if let Some((index, value)) = edit {
                            match scene.set_parameter(index, value) {
                                Ok(()) => self.viewport_dirty = true,
                                Err(err) => {
                                    new_status = Some(format!("evaluation failed: {err}"));
                                }
                            }
                        }
                    }
                    None => {
                        ui.label("Parameters appear once a model is loaded.");
                    }
                }
            });
        egui::Panel::bottom("tools")
            .resizable(true)
            .default_size(170.0)
            .show(ui, |ui| {
                ui.heading("Tools (registry)");
                ui.horizontal(|ui| {
                    // The tool list: every registry tool,
                    // with its posture; selecting one shows
                    // its gate note (the disabled tool's
                    // included) and arms the Run button.
                    egui::ScrollArea::vertical()
                        .id_salt("tool-list")
                        .max_width(260.0)
                        .show(ui, |ui| {
                            for meta in crate::tools::tools() {
                                let label = format!("{} [{}]", meta.name, meta.posture);
                                let selected = self.tool_selected.as_deref() == Some(meta.name);
                                if ui.selectable_label(selected, label).clicked() {
                                    self.tool_selected = Some(meta.name.to_string());
                                    self.tool_result = if meta.gate.is_empty() {
                                        meta.description.to_string()
                                    } else {
                                        format!("{}\nGate: {}", meta.description, meta.gate)
                                    };
                                }
                            }
                        });
                    ui.vertical(|ui| {
                        ui.label("Arguments (JSON):");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.tool_args)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                        if ui.button("Run tool").clicked() {
                            match &self.tool_selected {
                                None => {
                                    self.tool_result = "select a tool first".to_string();
                                }
                                Some(name) => {
                                    let name = name.clone();
                                    match serde_json::from_str::<serde_json::Value>(&self.tool_args)
                                    {
                                        Ok(args) => {
                                            let result = crate::tools::call(&name, &args);
                                            self.tool_result = if result.is_error {
                                                format!(
                                                    "REFUSED {}: {}",
                                                    result.error_code, result.error_message
                                                )
                                            } else {
                                                serde_json::to_string_pretty(&result.payload)
                                                    .unwrap_or_else(|_| result.payload.to_string())
                                            };
                                        }
                                        Err(err) => {
                                            self.tool_result =
                                                format!("arguments are not valid JSON: {err}");
                                        }
                                    }
                                }
                            }
                        }
                        egui::ScrollArea::vertical()
                            .id_salt("tool-result")
                            .show(ui, |ui| {
                                ui.label(&self.tool_result);
                            });
                    });
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Viewport");
            if let Some(scene_ref) = &scene {
                transport = transport_row(ui, &self.play, scene_ref);
            }
            if self.renderer.is_none() {
                ui.label("Renderer unavailable on this machine; panels remain usable.");
            } else if scene.is_none() {
                ui.label("Load a model to see it here.");
            } else if let Some(viewport) = &self.viewport {
                let available = ui.available_size();
                let aspect = viewport.size[0] as f32 / viewport.size[1] as f32;
                let mut width = available.x;
                let mut height = width / aspect;
                if height > available.y {
                    height = available.y;
                    width = height * aspect;
                }
                ui.image((viewport.id, egui::vec2(width, height)));
            } else {
                ui.label("Preparing the first frame…");
            }
        });
        if let (Some(edit), Some(scene_ref)) = (transport, &mut scene) {
            let outcome = match edit {
                TransportEdit::Play => self.play.play(scene_ref, None, false, None),
                TransportEdit::Pause => {
                    self.play.pause();
                    Ok(())
                }
                TransportEdit::Stop => {
                    self.play.stop(scene_ref, false);
                    Ok(())
                }
                TransportEdit::Step => self.play.step(scene_ref, 1),
                TransportEdit::Seek(frame) => self.play.seek(scene_ref, frame),
                TransportEdit::SetLoop(looping) => {
                    self.play.set_looping(looping);
                    Ok(())
                }
            };
            match outcome {
                Ok(()) => self.viewport_dirty = true,
                Err(err) => {
                    new_status = Some(format!("playback failed: {err}"));
                }
            }
        }
        self.state.scene = scene;
        if let Some(status) = new_status {
            self.state.status = status;
        }
        if let Some(path) = self.pending_model.take() {
            self.load_model(&path);
        }
        if let Some(path) = self.pending_save.take() {
            self.save_state(&path);
        }
        if let Some(path) = self.pending_load.take() {
            self.load_state(&path);
        }
    }

    /// Saves the standing scene + play state to `path`
    /// (the save path: genesis projects from the standing
    /// scene, the play record captures the transport; the
    /// document is the one `save` module defines).
    fn save_state(&mut self, path: &Path) {
        if self.state.scene.is_none() {
            self.state.status = "save failed: no model loaded".to_string();
            return;
        }
        let result = (|| -> AppResult<String> {
            let (genesis, defaults) = crate::session_host::project_genesis(&self.state)?;
            let scene = self.state.scene.as_mut().expect("scene checked");
            let evaluated = format!(
                "{:016x}",
                crate::state_hash::evaluated_state_hash(&mut scene.puppet)
            );
            let play = self.play.snapshot(scene);
            let model_label = scene.source_label.clone();
            let session =
                torogao_session::session::Session::new(genesis, &defaults, "torogao-app window");
            let document = crate::save::capture_with_defaults(
                &session,
                &play,
                &model_label,
                &evaluated,
                &defaults,
            );
            crate::save::write_save(path, &document, Some(Path::new(&model_label)), false)?;
            Ok(format!(
                "saved {} (session {})",
                path.display(),
                document.session_id
            ))
        })();
        self.state.status = match result {
            Ok(message) => message,
            Err(err) => format!("save failed: {err}"),
        };
    }

    /// Loads a save document: the model reloads, the op
    /// log replays through a fresh session + bridge over a
    /// fresh play host (the same flow the round-trip gate
    /// proves), and the play record restores.
    fn load_state(&mut self, path: &Path) {
        let result = (|| -> AppResult<(ShellState, PlayHost, String)> {
            let document = crate::save::read_save(path)?;
            let mut scene = Scene::load_file(Path::new(&document.model))?;
            scene.evaluate_fresh(&[])?;
            let mut shell = ShellState::new();
            shell.install_scene(scene);
            let genesis = torogao_session::from_canon_ron(&document.genesis)
                .map_err(|r| AppError::Format(format!("{}: {}", r.code, r.message)))?;
            let session = torogao_session::session::Session::new(
                genesis,
                &document.defaults,
                &document.held_by,
            );
            let mut host = torogao_session::host::Host::new(session);
            let mut bridge = AppBridge::with_player(shell, PlayHost::new());
            let mut base = torogao_session::state_hash(&host.session.genesis);
            for op in &document.ops {
                let applied = host
                    .call_tool(
                        "session_apply_op",
                        &serde_json::json!({"base_hash": base, "op": op}),
                        &mut bridge,
                    )
                    .map_err(|r| AppError::Format(format!("{}: {}", r.code, r.message)))?;
                base = applied["post"].as_str().unwrap_or_default().to_string();
            }
            if base != document.session_state_hash {
                return Err(AppError::Format(format!(
                    "restored session hash {base} != saved hash {}",
                    document.session_state_hash
                )));
            }
            let (mut shell, mut player) = bridge.into_parts();
            let report = {
                let scene = shell
                    .scene
                    .as_mut()
                    .ok_or_else(|| AppError::Format("save model scene missing".to_string()))?;
                crate::save::restore_play(&document, scene, &mut player)?
            };
            Ok((
                shell,
                player,
                format!(
                    "loaded save {} (session {}, play steps {})",
                    path.display(),
                    document.session_id,
                    report.steps_taken
                ),
            ))
        })();
        match result {
            Ok((shell, player, message)) => {
                self.state = shell;
                self.play = player;
                self.viewport_dirty = true;
                self.state.status = message;
            }
            Err(err) => {
                self.state.status = format!("load save failed: {err}");
            }
        }
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(mut egui_state) = self.egui_state.take() else {
            return;
        };
        self.drain_session_requests();
        self.advance_playback();
        self.advance_persona();
        if self.viewport_dirty {
            if let (Some(renderer), Some(egui_renderer), Some(scene)) = (
                self.renderer.as_mut(),
                self.egui_renderer.as_mut(),
                self.state.scene.as_mut(),
            ) {
                refresh_viewport(
                    renderer,
                    egui_renderer,
                    &mut self.viewport,
                    scene,
                    &mut self.state.status,
                    self.debug_physics,
                );
            }
            self.viewport_dirty = false;
        }
        let raw_input = egui_state.take_egui_input(&window);
        let full_output = self
            .egui_context
            .clone()
            .run_ui(raw_input, |ui| self.build_ui(ui));
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
                    label: Some("torogao UI encoder"),
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
                    label: Some("torogao UI pass"),
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

/// An `[u8; 3]` palette triple as an egui colour.
fn persona_rgb(rgb: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// A mix of two palette triples (`t` = weight of `b`).
fn persona_mix(a: [u8; 3], b: [u8; 3], t: f32) -> egui::Color32 {
    let channel = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t) as u8;
    egui::Color32::from_rgb(
        channel(a[0], b[0]),
        channel(a[1], b[1]),
        channel(a[2], b[2]),
    )
}

/// Paints a parallelogram (the slanted ribbon / skewed
/// selection block of the Persona stage) over `rect`,
/// sheared by `skew` points at the top edge.
fn persona_skew(ui: &egui::Ui, rect: egui::Rect, skew: f32, fill: egui::Color32) {
    let points = vec![
        rect.left_top() + egui::vec2(skew, 0.0),
        rect.right_top(),
        rect.right_bottom() - egui::vec2(skew, 0.0),
        rect.left_bottom(),
    ];
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        fill,
        egui::Stroke::NONE,
    ));
}

impl Shell {
    /// The Persona stage (Addition Y): the default UI.
    /// One stage, rearranged per area — a pinned context
    /// strip on top (rule 5), the area rail at the left
    /// (slanted ribbons; frozen areas in the mauve tier),
    /// the host's cut-in at the fixed right edge (rule
    /// 10), the area's single-axis command stage at the
    /// bottom, and the puppet's viewport in the centre —
    /// live in the Playroom, dimmed behind the stage
    /// everywhere else (freeze, don't erase). The model
    /// in `crate::persona` carries the rules; this is its
    /// rendering.
    fn build_persona_ui(&mut self, ui: &mut egui::Ui) {
        use crate::persona::{
            Area, CommandKind, NativeCommand, StageContext, commands, display_treatment,
            event_accent,
        };
        let mut scene = self.state.scene.take();
        let mut new_status: Option<String> = None;
        let mut transport: Option<TransportEdit> = None;
        let area = self.persona.current;
        let info = area.info();
        let accent_rgb = event_accent(area);
        let accent = persona_rgb(accent_rgb);
        let area_commands = commands(area);
        let focus_index = self.persona.focus_index(area);

        // The pinned comparison context (rule 5): the
        // same values in every area, so nothing a user
        // needs for a comparison lives in their memory.
        let context = match &scene {
            Some(scene) => StageContext {
                model_label: scene.model_name().to_string(),
                node_count: scene.puppet.nodes.nodes.len(),
                param_count: scene.puppet.params.len(),
                selected_node: self.state.selection.node_index,
                play_state: match self.play.current() {
                    Some(name) => format!(
                        "{} · step {} · {}",
                        name,
                        self.play.step_count(),
                        if self.play.is_advancing() {
                            "playing"
                        } else {
                            "paused"
                        }
                    ),
                    None => "stopped".to_string(),
                },
            },
            None => StageContext::default(),
        };

        // Keyboard navigation, live whenever no text
        // field holds the keyboard: Up/Down move the
        // focus along the single axis, letters jump
        // (rule 8), Enter activates, Esc returns to the
        // Playroom stage.
        let text_focused = ui.ctx().memory(|memory| memory.focused().is_some());
        if !text_focused {
            let mut activate = false;
            ui.ctx().input(|input| {
                if input.key_pressed(egui::Key::ArrowDown) {
                    self.persona.focus_next();
                }
                if input.key_pressed(egui::Key::ArrowUp) {
                    self.persona.focus_prev();
                }
                if input.key_pressed(egui::Key::Enter) {
                    activate = true;
                }
                if input.key_pressed(egui::Key::Escape) {
                    self.persona.switch_area(Area::Playroom);
                }
                for key in [
                    egui::Key::A,
                    egui::Key::B,
                    egui::Key::C,
                    egui::Key::D,
                    egui::Key::E,
                    egui::Key::F,
                    egui::Key::G,
                    egui::Key::H,
                    egui::Key::I,
                    egui::Key::J,
                    egui::Key::K,
                    egui::Key::L,
                    egui::Key::M,
                    egui::Key::N,
                    egui::Key::O,
                    egui::Key::P,
                    egui::Key::Q,
                    egui::Key::R,
                    egui::Key::S,
                    egui::Key::T,
                    egui::Key::U,
                    egui::Key::V,
                    egui::Key::W,
                    egui::Key::X,
                    egui::Key::Y,
                    egui::Key::Z,
                ] {
                    if input.key_pressed(key) {
                        let letter = format!("{key:?}").chars().next().unwrap_or(' ');
                        self.persona.jump_to(letter);
                    }
                }
            });
            if activate {
                self.persona_activate_focused();
            }
            // Keep the tool selection under the focus, the
            // way a click arms the Run button in Nerd mode.
            self.persona_sync_selection();
        }

        // ---- pinned strip: treated area title, the
        // pinned context, the model furniture, status.
        egui::Panel::top("persona-pinned").show(ui, |ui| {
            ui.horizontal(|ui| {
                // The area title in the procedural display
                // treatment (per-letter rotation + offset,
                // a pure function of the title string).
                let title = info.title;
                let (title_rect, _) = ui.allocate_exact_size(
                    egui::vec2(15.0 * title.chars().count() as f32 + 8.0, 30.0),
                    egui::Sense::hover(),
                );
                let mut x = title_rect.left() + 4.0;
                for letter in display_treatment(title) {
                    let galley = ui.painter().layout_no_wrap(
                        letter.ch.to_string(),
                        egui::FontId::proportional(24.0),
                        persona_rgb(crate::persona::TEXT_HEADING),
                    );
                    let mut shape = egui::epaint::TextShape::new(
                        egui::pos2(x, title_rect.top() + 2.0 + letter.y_offset),
                        galley,
                        persona_rgb(crate::persona::TEXT_HEADING),
                    );
                    shape.angle = letter.rotation_deg.to_radians();
                    ui.painter().add(egui::Shape::Text(shape));
                    x += 15.0;
                }
                ui.separator();
                ui.label(format!(
                    "{} — {} nodes, {} parameters",
                    context.model_label, context.node_count, context.param_count
                ));
                if let Some(node) = context.selected_node {
                    ui.label(format!("· node {node}"));
                }
                ui.label(format!("· {}", context.play_state));
                ui.separator();
                ui.text_edit_singleline(&mut self.model_path_input);
                if ui.button("Load").clicked() {
                    let path = std::path::PathBuf::from(self.model_path_input.trim());
                    if !path.as_os_str().is_empty() {
                        self.pending_model = Some(path);
                    }
                }
                ui.separator();
                ui.label(&self.state.status);
                if !self.session_note.is_empty() {
                    ui.separator();
                    ui.label(&self.session_note);
                }
            });
        });

        // ---- the area rail: slanted ribbons, one per
        // area; the current area carries the event
        // colour, frozen areas the mauve tier.
        egui::Panel::left("persona-rail")
            .resizable(false)
            .default_size(158.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                for rail_area in Area::ALL {
                    let rail_info = rail_area.info();
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 30.0),
                        egui::Sense::click(),
                    );
                    let current = rail_area == area;
                    let frozen = self.persona.is_frozen(rail_area);
                    let fill = if current {
                        persona_rgb(event_accent(rail_area))
                    } else if frozen {
                        persona_rgb(crate::persona::FRAME_BG)
                    } else {
                        egui::Color32::from_rgb(0x2A, 0x0F, 0x1D)
                    };
                    persona_skew(ui, rect, 9.0, fill);
                    let text_color = if current {
                        persona_rgb(crate::persona::BLACK)
                    } else if frozen {
                        persona_rgb(crate::persona::TEXT_DISABLED)
                    } else {
                        persona_rgb(crate::persona::TEXT_HEADING)
                    };
                    ui.painter().text(
                        rect.left_center() + egui::vec2(14.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        if frozen {
                            format!("{} · held", rail_info.title)
                        } else {
                            rail_info.title.to_string()
                        },
                        egui::FontId::proportional(17.0),
                        text_color,
                    );
                    if response.clicked() {
                        self.persona.switch_area(rail_area);
                        self.persona_sync_selection();
                    }
                    ui.add_space(4.0);
                }
                // The brushstroke wipe, drawn as a growing
                // accent band under the rail while a
                // transition runs (the one moving element).
                if let Some(transition) = self.persona.transition() {
                    let progress = self.persona.wipe_progress();
                    let width = ui.available_width() * progress;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 10.0),
                        egui::Sense::hover(),
                    );
                    let band = egui::Rect::from_min_size(rect.min, egui::vec2(width, 10.0));
                    persona_skew(ui, band, 5.0, accent);
                    let _ = transition;
                }
            });

        // ---- the host cut-in: fixed at the right edge,
        // pulled forward once on focus, idle motion
        // capped below the cursor's salience (rule 10).
        egui::Panel::right("persona-host")
            .resizable(false)
            .default_size(168.0)
            .show(ui, |ui| {
                if area == Area::Video {
                    // The one lighter ground: black-plum,
                    // never white (the cutting room's
                    // light-table).
                    ui.painter().rect_filled(
                        ui.max_rect(),
                        0.0,
                        egui::Color32::from_rgb(0x24, 0x10, 0x26),
                    );
                }
                ui.add_space(8.0);
                match info.host_slot {
                    Some(slot) => {
                        ui.label(
                            egui::RichText::new(slot)
                                .size(15.0)
                                .color(persona_rgb(crate::persona::TEXT_HEADING)),
                        );
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("HOSTLESS — the meta register")
                                .size(15.0)
                                .color(persona_rgb(crate::persona::TEXT_HEADING)),
                        );
                    }
                }
                let pull = self.persona.host_pull();
                let monogram_size = 46.0 + 12.0 * pull;
                let monogram_color = persona_mix(
                    accent_rgb,
                    crate::persona::TEXT_DISABLED,
                    1.0 - crate::persona::HOST_IDLE_SALIENCE,
                );
                ui.label(
                    egui::RichText::new(info.title.chars().next().unwrap_or('?').to_string())
                        .size(monogram_size)
                        .color(if area == Area::Settings {
                            persona_rgb(crate::persona::TEXT_HEADING)
                        } else {
                            monogram_color
                        }),
                );
                ui.label(
                    egui::RichText::new(info.register)
                        .size(13.0)
                        .color(persona_rgb(crate::persona::TEXT_RESTING)),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(info.accent_note)
                        .size(12.0)
                        .color(persona_rgb(crate::persona::TEXT_DISABLED)),
                );
            });

        // ---- the area's command stage: single-axis list,
        // focus detail, activation, result.
        let focused = area_commands.get(focus_index).cloned();
        egui::Panel::bottom("persona-stage")
            .resizable(true)
            .default_size(232.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        ui.label(
                            egui::RichText::new(format!("{} — commands", info.title))
                                .size(15.0)
                                .color(persona_rgb(crate::persona::TEXT_HEADING)),
                        );
                        egui::ScrollArea::vertical()
                            .id_salt("persona-commands")
                            .show(ui, |ui| {
                                for (index, command) in area_commands.iter().enumerate() {
                                    let is_focused = index == focus_index;
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), 25.0),
                                        egui::Sense::click(),
                                    );
                                    if is_focused {
                                        // The skewed selection
                                        // block: full accent
                                        // fill, black text —
                                        // never a mere outline.
                                        persona_skew(ui, rect, 7.0, accent);
                                    }
                                    ui.painter().text(
                                        rect.left_center() + egui::vec2(12.0, 0.0),
                                        egui::Align2::LEFT_CENTER,
                                        if is_focused {
                                            command.label.clone()
                                        } else {
                                            command.summary.clone()
                                        },
                                        egui::FontId::proportional(16.0),
                                        if is_focused {
                                            persona_rgb(crate::persona::BLACK)
                                        } else {
                                            persona_rgb(crate::persona::TEXT_RESTING)
                                        },
                                    );
                                    if response.clicked() {
                                        self.persona.set_focus(index);
                                        self.persona_sync_selection();
                                    }
                                    if response.double_clicked() {
                                        self.persona.set_focus(index);
                                        self.persona_sync_selection();
                                        self.persona_activate_focused();
                                    }
                                }
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        if let Some(command) = &focused {
                            ui.label(
                                egui::RichText::new(&command.label)
                                    .size(17.0)
                                    .color(persona_rgb(crate::persona::TEXT_HEADING)),
                            );
                            ui.label(
                                egui::RichText::new(&command.detail)
                                    .size(14.0)
                                    .color(persona_rgb(crate::persona::TEXT_RESTING)),
                            );
                            ui.label(
                                egui::RichText::new(&command.affordance)
                                    .size(13.0)
                                    .color(persona_rgb(crate::persona::TEXT_DISABLED)),
                            );
                            ui.add_space(4.0);
                            match command.kind {
                                CommandKind::Tool(_) => {
                                    ui.label("Arguments (JSON):");
                                    ui.add(
                                        egui::TextEdit::multiline(&mut self.tool_args)
                                            .desired_rows(3)
                                            .desired_width(f32::INFINITY),
                                    );
                                    if ui.button("Run").clicked() {
                                        self.run_selected_tool();
                                    }
                                }
                                CommandKind::Native(NativeCommand::TogglePhysicsOverlay) => {
                                    let label = if self.debug_physics {
                                        "Physics debug overlay: ON — Enter turns it off"
                                    } else {
                                        "Physics debug overlay: OFF — Enter turns it on"
                                    };
                                    if ui.button(label).clicked() {
                                        self.debug_physics = !self.debug_physics;
                                        self.viewport_dirty = true;
                                    }
                                }
                                CommandKind::Native(NativeCommand::ToggleNerdMode) => {
                                    if ui.button("Switch to Nerd mode (F9)").clicked() {
                                        self.toggle_nerd_mode();
                                    }
                                }
                                CommandKind::Native(
                                    NativeCommand::SaveSessionState
                                    | NativeCommand::LoadSessionState,
                                ) => {
                                    ui.label("State file:");
                                    ui.text_edit_singleline(&mut self.save_path_input);
                                    let save = matches!(
                                        command.kind,
                                        CommandKind::Native(NativeCommand::SaveSessionState)
                                    );
                                    if ui
                                        .button(if save { "Save state" } else { "Load state" })
                                        .clicked()
                                    {
                                        let path =
                                            std::path::PathBuf::from(self.save_path_input.trim());
                                        if !path.as_os_str().is_empty() {
                                            if save {
                                                self.pending_save = Some(path);
                                            } else {
                                                self.pending_load = Some(path);
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            ui.label("This area has no commands.");
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Result")
                                .size(15.0)
                                .color(persona_rgb(crate::persona::TEXT_HEADING)),
                        );
                        // Rule 11 in the rendering: a
                        // refusal sits in the emergency
                        // register (accent on the emergency
                        // ground); a confirmation rests.
                        let refusal = self.tool_result.starts_with("REFUSED ");
                        let frame = egui::Frame::default()
                            .fill(if refusal {
                                persona_rgb(crate::persona::EMERGENCY_BG)
                            } else {
                                persona_rgb(crate::persona::FRAME_BG)
                            })
                            .inner_margin(6.0);
                        frame.show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("persona-result")
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(&self.tool_result).size(14.0).color(
                                            if refusal {
                                                accent
                                            } else {
                                                persona_rgb(crate::persona::TEXT_RESTING)
                                            },
                                        ),
                                    );
                                });
                        });
                    });
                });
            });

        // ---- the centre: the puppet stays on stage.
        egui::CentralPanel::default().show(ui, |ui| {
            if area == Area::Playroom {
                if let Some(scene_ref) = &scene {
                    transport = transport_row(ui, &self.play, scene_ref);
                }
            } else {
                ui.label(
                    egui::RichText::new(format!("{} stage — {}", info.title, info.register))
                        .size(16.0)
                        .color(persona_rgb(crate::persona::TEXT_HEADING)),
                );
            }
            if self.renderer.is_none() {
                ui.label("Renderer unavailable on this machine; the stage remains usable.");
            } else if scene.is_none() {
                ui.label("Load a model to bring the puppet on stage.");
            } else if let Some(viewport) = &self.viewport {
                let available = ui.available_size();
                let aspect = viewport.size[0] as f32 / viewport.size[1] as f32;
                let mut width = available.x;
                let mut height = width / aspect;
                if height > available.y {
                    height = available.y;
                    width = height * aspect;
                }
                let response = ui.image((viewport.id, egui::vec2(width, height)));
                if area != Area::Playroom {
                    // The stage furniture changes; the
                    // puppet is frozen behind it, dimmed
                    // toward the frame ground.
                    ui.painter().rect_filled(
                        response.rect,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(0x1C, 0x0B, 0x14, 165),
                    );
                }
            } else {
                ui.label("Preparing the first frame…");
            }
        });

        if let (Some(edit), Some(scene_ref)) = (transport, &mut scene) {
            let outcome = match edit {
                TransportEdit::Play => self.play.play(scene_ref, None, false, None),
                TransportEdit::Pause => {
                    self.play.pause();
                    Ok(())
                }
                TransportEdit::Stop => {
                    self.play.stop(scene_ref, false);
                    Ok(())
                }
                TransportEdit::Step => self.play.step(scene_ref, 1),
                TransportEdit::Seek(frame) => self.play.seek(scene_ref, frame),
                TransportEdit::SetLoop(looping) => {
                    self.play.set_looping(looping);
                    Ok(())
                }
            };
            match outcome {
                Ok(()) => self.viewport_dirty = true,
                Err(err) => {
                    new_status = Some(format!("playback failed: {err}"));
                }
            }
        }
        self.state.scene = scene;
        if let Some(status) = new_status {
            self.state.status = status;
        }
        if let Some(path) = self.pending_model.take() {
            self.load_model(&path);
        }
        if let Some(path) = self.pending_save.take() {
            self.save_state(&path);
        }
        if let Some(path) = self.pending_load.take() {
            self.load_state(&path);
        }
    }

    /// Keeps the shell's tool selection under the Persona
    /// focus: focusing a tool command arms it exactly as
    /// clicking it in the Nerd Tools panel does (selection
    /// + description/gate preview in the result pane).
    fn persona_sync_selection(&mut self) {
        if let Some(command) = self.persona.focused_command() {
            if let CommandKind::Tool(name) = command.kind {
                if self.tool_selected.as_deref() != Some(name) {
                    self.tool_selected = Some(name.to_string());
                    if let Some(meta) = crate::tools::find(name) {
                        self.tool_result = if meta.gate.is_empty() {
                            meta.description.to_string()
                        } else {
                            format!("{}\nGate: {}", meta.description, meta.gate)
                        };
                    }
                }
            }
        }
    }

    /// Activates the Persona-focused command (Enter, or a
    /// double-click): a tool runs with the arguments
    /// editor's text; a native performs its shell action.
    fn persona_activate_focused(&mut self) {
        let Some(command) = self.persona.focused_command() else {
            return;
        };
        match command.kind {
            CommandKind::Tool(_) => {
                self.persona_sync_selection();
                self.run_selected_tool();
            }
            CommandKind::Native(NativeCommand::TogglePhysicsOverlay) => {
                self.debug_physics = !self.debug_physics;
                self.viewport_dirty = true;
            }
            CommandKind::Native(NativeCommand::ToggleNerdMode) => {
                self.toggle_nerd_mode();
            }
            CommandKind::Native(NativeCommand::SaveSessionState) => {
                let path = std::path::PathBuf::from(self.save_path_input.trim());
                if !path.as_os_str().is_empty() {
                    self.pending_save = Some(path);
                }
            }
            CommandKind::Native(NativeCommand::LoadSessionState) => {
                let path = std::path::PathBuf::from(self.save_path_input.trim());
                if !path.as_os_str().is_empty() {
                    self.pending_load = Some(path);
                }
            }
        }
    }
}

impl ApplicationHandler for Shell {
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

/// Re-renders the viewport texture from the scene's current
/// evaluated state, recreating the texture when the canvas
/// size changed.
fn refresh_viewport(
    renderer: &mut Renderer,
    egui_renderer: &mut egui_wgpu::Renderer,
    viewport: &mut Option<ViewportTexture>,
    scene: &mut Scene,
    status: &mut String,
    debug_physics: bool,
) {
    let frame = match prepare_frame(scene) {
        Ok(frame) => frame,
        Err(err) => {
            *status = format!("frame preparation failed: {err}");
            return;
        }
    };
    let canvas = cap_canvas(frame.canvas, VIEWPORT_SIDE_MAX);
    let size = [canvas.width, canvas.height];
    let needs_new = viewport
        .as_ref()
        .map(|current| current.size != size)
        .unwrap_or(true);
    if needs_new {
        if let Some(old) = viewport.take() {
            egui_renderer.free_texture(&old.id);
        }
        let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("torogao viewport"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let id = egui_renderer.register_native_texture(
            renderer.device(),
            &view,
            wgpu::FilterMode::Linear,
        );
        *viewport = Some(ViewportTexture { id, texture, size });
    }
    let current = viewport.as_ref().expect("viewport prepared above");
    let view = current
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let id = current.id;
    let segments = if debug_physics {
        crate::render::physics_overlay_segments(&scene.puppet)
    } else {
        Vec::new()
    };
    renderer.draw_into(
        scene,
        &frame,
        &view,
        (size[0], size[1]),
        wgpu::Color::TRANSPARENT,
        &segments,
    );
    egui_renderer.update_egui_texture_from_wgpu_texture(
        renderer.device(),
        &view,
        wgpu::FilterMode::Linear,
        id,
    );
}

/// One node row, recursively. Selection is the shared app
/// state, written here and nowhere else in the panel code.
fn tree_node(
    ui: &mut egui::Ui,
    puppet: &Puppet,
    index: usize,
    depth: usize,
    selection: &mut Selection,
    expanded: &mut BTreeSet<usize>,
) {
    if depth > TREE_DEPTH_MAX {
        ui.label("…");
        return;
    }
    let node = &puppet.nodes.nodes[index];
    let children = node.children.clone();
    let has_children = !children.is_empty();
    let is_open = expanded.contains(&index);
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);
        if has_children {
            let glyph = if is_open { "▾" } else { "▸" };
            if ui.small_button(glyph).clicked() {
                if is_open {
                    expanded.remove(&index);
                } else {
                    expanded.insert(index);
                }
            }
        } else {
            ui.add_space(18.0);
        }
        let label = format!("{} [{}]", node.name, node.kind.type_name());
        if ui
            .selectable_label(selection.is_selected(index), label)
            .clicked()
        {
            selection.select(index);
        }
    });
    if has_children && is_open {
        for child in children {
            tree_node(ui, puppet, child, depth + 1, selection, expanded);
        }
    }
}

/// Read-only facts about the selected node.
fn node_details(ui: &mut egui::Ui, puppet: &Puppet, index: usize) {
    let node = &puppet.nodes.nodes[index];
    ui.label(format!("name: {}", node.name));
    ui.label(format!("kind: {}", node.kind.type_name()));
    ui.label(format!("uuid: {}", node.uuid));
    ui.label(format!("path: {}", puppet.nodes.path_of(index)));
    ui.label(format!("children: {}", node.children.len()));
    if node.is_drawable() {
        ui.label(format!("blend mode: {}", node.blend_mode));
        ui.label(format!("opacity: {:.3}", node.opacity));
        if let Some(mesh) = &node.mesh {
            ui.label(format!(
                "mesh: {} vertices, {} indices",
                mesh.vertices.len(),
                mesh.indices.len()
            ));
        }
    }
}

/// One parameter row: name, driven marker, and value
/// editors. Returns the new value when the user edited it.
fn parameter_row(
    ui: &mut egui::Ui,
    scene: &Scene,
    index: usize,
) -> Option<(usize, torogao_engine::Vec2)> {
    let param = &scene.puppet.params[index];
    let driven = scene.puppet.is_driven(index);
    let mut value = param.value;
    let mut changed = false;
    ui.label(&param.name);
    if driven {
        ui.label(format!(
            "driven — current ({:.3}, {:.3})",
            param.last_internal.x, param.last_internal.y
        ));
        return None;
    }
    ui.horizontal(|ui| {
        ui.label("x");
        if drag_value(ui, &mut value.x, param.min.x, param.max.x) {
            changed = true;
        }
        if param.is_vec2 {
            ui.label("y");
            if drag_value(ui, &mut value.y, param.min.y, param.max.y) {
                changed = true;
            }
        }
    });
    ui.separator();
    if changed { Some((index, value)) } else { None }
}

fn drag_value(ui: &mut egui::Ui, value: &mut f32, min: f32, max: f32) -> bool {
    let mut widget = egui::DragValue::new(value).speed(0.01);
    if min.is_finite() && max.is_finite() && min <= max {
        widget = widget.range(min..=max);
    }
    ui.add(widget).changed()
}

/// The transport row: animation picker state, play /
/// pause / stop / step buttons, the seek slider, the loop
/// toggle, and the playhead readout. Returns at most one
/// edit per frame (the shell applies it against the scene
/// after the panels close).
fn transport_row(ui: &mut egui::Ui, play: &PlayHost, scene: &Scene) -> Option<TransportEdit> {
    let mut edit = None;
    let snapshot = play.snapshot(scene);
    ui.horizontal(|ui| {
        ui.label("Play");
        ui.separator();
        match &snapshot.animation {
            Some(name) => {
                ui.label(name);
            }
            None => {
                ui.label("no animation loaded");
            }
        }
        if ui.button("▶ Play").clicked() {
            edit = Some(TransportEdit::Play);
        }
        if ui.button("⏸ Pause").clicked() {
            edit = Some(TransportEdit::Pause);
        }
        if ui.button("⏹ Stop").clicked() {
            edit = Some(TransportEdit::Stop);
        }
        if ui.button("Step").clicked() {
            edit = Some(TransportEdit::Step);
        }
        let mut looping = snapshot.looping;
        if ui.checkbox(&mut looping, "Loop").changed() {
            edit = Some(TransportEdit::SetLoop(looping));
        }
        ui.label(format!(
            "frame {} · step {} · speed {:.1}",
            snapshot.frame, snapshot.step_count, snapshot.speed
        ));
    });
    if let Some(name) = &snapshot.animation {
        if let Some((_, anim)) = scene
            .puppet
            .animations
            .iter()
            .find(|(anim_name, _)| anim_name == name)
        {
            let mut frame = snapshot.frame;
            let slider = egui::Slider::new(&mut frame, 0..=anim.length.max(0)).text("seek");
            if ui.add(slider).changed() {
                edit = Some(TransportEdit::Seek(frame));
            }
        }
    }
    if scene.puppet.animations.is_empty() {
        ui.label("This model carries no animations; named states apply from the session surface.");
    }
    edit
}

/// Runs the windowed shell. Returns the process exit code.
/// With `session_options`, the session server attaches to
/// the running window once its first model loads (windowed
/// co-presence).
pub fn run_windowed(
    model_path: Option<&Path>,
    session_options: Option<WindowSessionOptions>,
) -> AppResult<i32> {
    let event_loop = EventLoop::new()
        .map_err(|err| AppError::Cli(format!("event loop creation failed: {err}")))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell::new(model_path, session_options);
    event_loop
        .run_app(&mut shell)
        .map_err(|err| AppError::Cli(format!("event loop failed: {err}")))?;
    Ok(0)
}
