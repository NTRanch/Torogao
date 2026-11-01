//! The Android shell proper: the winit event loop, the
//! surface lifecycle, the session-host boot, and the
//! Persona frame drawing (portrait reflow).
//!
//! Lifecycle discipline is Android's, not the desktop's:
//! the surface is destroyed and recreated as the activity
//! suspends and resumes, so every GPU object bound to the
//! window is dropped on `suspended` and rebuilt on
//! `resumed`, while the session state (scene, play host,
//! persona state, the session host itself) survives — it
//! was never the window's to own. A frame that arrives
//! with no surface, a zero-size surface, or a lost surface
//! is skipped as a typed no-op, never a panic (the
//! surface-loss adversarial cases of BRIEF-A1 gate B2).
//!
//! The frame the shell draws is the layout model's frame
//! (crate::layout): stage header pinned top, one scrolling
//! content column, area rail, standing bar pinned to the
//! bottom edge — the §3.3 rules as geometry, with the
//! Settings area live and the other areas' frames rendered
//! but not claimed.

use std::sync::Arc;
use std::time::Instant;

use torogao_app::persona::{self, Area, PersonaState, StageContext};
use torogao_app::player::PlayHost;
use torogao_app::render::{Renderer, VIEWPORT_SIDE_MAX, cap_canvas, prepare_frame};
use torogao_app::scene::STEP_SECONDS;
use torogao_app::session_host::project_genesis;
use torogao_app::state::ShellState;
use torogao_session::host::Host;
use torogao_session::session::Session;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::audit_phone::phone_audit_json;
use crate::evidence::{AUDIT_NAME, DETERMINISM_NAME, EvidenceLog, PERF_NAME, write_json_atomic};
use crate::fixture::{PRIMA_REPERTOIRE_BYTES, run_greeting_stream};
use crate::layout::{
    COMMAND_ROW_HEIGHT_DP, FrameLayout, RAIL_CHIP_MIN_WIDTH_DP, ViewportDp, frame_layout,
};
use crate::perf::FrameTimes;
use crate::settings::SettingsStore;
use crate::storage::Storage;

/// Largest catch-up steps one frame may take (the
/// windowed shell's bound; the fixed-step clock never
/// bursts unboundedly).
const CATCH_UP_STEPS_MAX: u64 = 4;
/// Presented frames between performance-record writes.
const PERF_WRITE_INTERVAL: u64 = 240;
/// The animation the stage presents ambiently (Prima's
/// idle repertoire performance).
const AMBIENT_ANIMATION: &str = "Daydream";

/// What one frame of UI asked the shell to do. Collected
/// during drawing, applied after — the windowed shell's
/// borrow discipline.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PhoneAction {
    /// Navigate the frame to an area.
    SwitchArea(Area),
    /// Flip the Persona/Nerd presentation (persisted).
    ToggleNerdMode,
    /// Collapse or expand the stage header.
    ToggleStageCollapse,
    /// Play or pause the stage's ambient performance.
    TogglePlayPause,
    /// Settings: save state under the field's name.
    SaveState,
    /// Settings: load state from the field's name.
    LoadState,
    /// Settings: adopt a listed save's name into the field.
    FillSaveName(String),
}

/// The offscreen texture behind the stage viewport.
struct ViewportTexture {
    /// The egui texture id the stage image draws.
    id: egui::TextureId,
    /// The GPU texture (kept alive for the id).
    texture: wgpu::Texture,
    /// Its pixel size.
    size: [u32; 2],
}

/// The Android shell's full state.
pub struct AndroidShell {
    /// The phone storage seam.
    storage: Storage,
    /// The Settings area's store.
    settings: SettingsStore,
    /// The boot/evidence log.
    boot_log: EvidenceLog,
    /// The Settings walk record.
    walk_log: EvidenceLog,
    /// The shared app state (the scene lives here).
    state: ShellState,
    /// The play host over the standing scene.
    play: PlayHost,
    /// The Persona presentation state.
    persona: PersonaState,
    /// The hosted session (torogao-session), resident from
    /// boot: genesis projected from the standing scene.
    host: Option<Host>,
    /// The GPU renderer (survives surface loss).
    renderer: Option<Renderer>,
    /// The platform window, while resumed.
    window: Option<Arc<Window>>,
    /// The window surface, while resumed.
    surface: Option<wgpu::Surface<'static>>,
    /// The surface's chosen format.
    surface_format: wgpu::TextureFormat,
    /// The egui context (one for the shell's life).
    egui_context: egui::Context,
    /// The egui-winit state, bound to the window.
    egui_state: Option<egui_winit::State>,
    /// The egui-wgpu renderer, bound to the surface format.
    egui_renderer: Option<egui_wgpu::Renderer>,
    /// The stage viewport texture.
    viewport: Option<ViewportTexture>,
    /// Whether the viewport needs a re-render.
    viewport_dirty: bool,
    /// Whether the stage header is collapsed.
    stage_collapsed: bool,
    /// The Settings save-name field (IME text entry).
    save_name_input: String,
    /// The last user-visible status line.
    status: String,
    /// Whether the status line is a refusal (rule 11).
    status_is_refusal: bool,
    /// Playback's fixed-step clock anchor.
    last_step: Instant,
    /// The Persona motion clock anchor.
    persona_last_step: Instant,
    /// Frame-time samples for the performance smoke.
    perf: FrameTimes,
    /// Presented-frame count (drives evidence writes).
    frames_presented: u64,
    /// Whether the boot sequence (host, determinism,
    /// audit) has run for this process.
    booted: bool,
}

impl AndroidShell {
    /// A fresh shell over `storage`. The persisted
    /// presentation preference is honoured from the first
    /// frame (a relaunch restores Nerd mode — gate B7's
    /// visible half).
    pub fn new(storage: Storage, boot_log: EvidenceLog, walk_log: EvidenceLog) -> AndroidShell {
        let settings = SettingsStore::from_storage(&storage);
        let mut persona = PersonaState::new();
        persona.nerd_mode = settings.nerd_mode();
        AndroidShell {
            storage,
            settings,
            boot_log,
            walk_log,
            state: ShellState::new(),
            play: PlayHost::new(),
            persona,
            host: None,
            renderer: None,
            window: None,
            surface: None,
            surface_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            egui_context: egui::Context::default(),
            egui_state: None,
            egui_renderer: None,
            viewport: None,
            viewport_dirty: true,
            stage_collapsed: false,
            save_name_input: "session-1".to_string(),
            status: "starting".to_string(),
            status_is_refusal: false,
            last_step: Instant::now(),
            persona_last_step: Instant::now(),
            perf: FrameTimes::new(),
            frames_presented: 0,
            booted: false,
        }
    }

    /// Sets the status line (confirmation register).
    fn confirm(&mut self, message: String) {
        self.status = message;
        self.status_is_refusal = false;
    }

    /// Sets the status line in the refusal register
    /// (persona rule 11: typed, visible, distinct).
    fn refuse(&mut self, message: String) {
        self.status = format!("REFUSED: {message}");
        self.status_is_refusal = true;
    }

    /// The boot sequence, once per process, on the first
    /// resume: load Prima, start her ambient performance,
    /// host the session, and write the boot evidence
    /// (determinism record + phone audit). Every step logs
    /// its outcome; a failed step degrades the shell to a
    /// status line, never a dead process.
    fn boot(&mut self) {
        if self.booted {
            return;
        }
        self.booted = true;
        self.boot_log
            .line("boot: loading embedded Prima repertoire");
        match crate::fixture::load_prima(PRIMA_REPERTOIRE_BYTES) {
            Ok(scene) => {
                self.state.install_scene(scene);
                self.boot_log.line(&format!("boot: {}", self.state.status));
            }
            Err(err) => {
                self.boot_log
                    .line(&format!("boot: fixture load failed: {err}"));
                self.refuse(format!("fixture load failed: {err}"));
                return;
            }
        }
        if let Some(scene) = self.state.scene.as_ref() {
            match self.play.play(scene, Some(AMBIENT_ANIMATION), true, None) {
                Ok(()) => self
                    .boot_log
                    .line(&format!("boot: playing {AMBIENT_ANIMATION} (looping)")),
                Err(err) => self.boot_log.line(&format!("boot: play failed: {err}")),
            }
        }
        self.start_session_host();
        self.write_determinism_record();
        write_json_atomic(
            &self.storage.evidence_dir().join(AUDIT_NAME),
            &phone_audit_json(),
        );
        self.boot_log
            .line("boot: evidence written (audit, determinism)");
    }

    /// Hosts the session in-process: genesis projects from
    /// the standing scene and a `torogao-session` Host
    /// takes residence — the windowed co-presence
    /// arrangement minus the socket (the socket is a
    /// desktop transport; the host is the same).
    fn start_session_host(&mut self) {
        match project_genesis(&self.state) {
            Ok((genesis, defaults)) => {
                let genesis_hash = torogao_session::state_hash(&genesis);
                let session = Session::new(genesis, &defaults, crate::settings::HELD_BY);
                let session_id = session.id.clone();
                self.host = Some(Host::new(session));
                self.boot_log.line(&format!(
                    "session host up: session {session_id} genesis_hash {genesis_hash}"
                ));
            }
            Err(err) => {
                self.boot_log
                    .line(&format!("session host failed to start: {err}"));
            }
        }
    }

    /// The determinism smoke (Scope 5) from inside the
    /// app: the Greeting stream evaluated over the embedded
    /// bytes by this build, recorded as JSON evidence. The
    /// desktop counterpart is the `greeting-hash` tool over
    /// the canonical file's bytes; the gate compares them.
    fn write_determinism_record(&self) {
        let record = match run_greeting_stream(PRIMA_REPERTOIRE_BYTES) {
            Ok(report) => serde_json::json!({
                "kind": "torogao-android-determinism/1",
                "run_target": "android-app",
                "fixture_sha256": crate::fixture::PRIMA_REPERTOIRE_SHA256,
                "animation": crate::fixture::STREAM_ANIMATION,
                "steps": report.steps,
                "trajectory_hash": format!("{:016x}", report.trajectory_hash),
                "final_hash": format!("{:016x}", report.final_hash),
                "distinct_states": report.distinct_states,
            }),
            Err(err) => serde_json::json!({
                "kind": "torogao-android-determinism/1",
                "run_target": "android-app",
                "error": format!("{err}"),
            }),
        };
        write_json_atomic(&self.storage.evidence_dir().join(DETERMINISM_NAME), &record);
    }

    /// Recreates the window-bound GPU stack on resume.
    /// The renderer (device) is created once and kept; the
    /// surface, egui state, and egui renderer are bound to
    /// the window and rebuilt every resume.
    fn resume_gpu(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title("Torogao");
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.boot_log
                    .line(&format!("window creation failed: {err}"));
                event_loop.exit();
                return;
            }
        };
        if self.renderer.is_none() {
            match Renderer::new() {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(err) => {
                    self.boot_log.line(&format!("renderer unavailable: {err}"));
                    self.refuse(format!("renderer unavailable: {err}"));
                }
            }
        }
        if let Some(renderer) = self.renderer.as_ref() {
            match renderer.create_surface(window.clone()) {
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
                    self.boot_log.line(&format!(
                        "surface up: {}x{} format {format:?}",
                        size.width, size.height
                    ));
                }
                Err(err) => self
                    .boot_log
                    .line(&format!("surface creation failed: {err}")),
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
        self.viewport_dirty = true;
    }

    /// Drops every window-bound GPU object on suspend. The
    /// renderer and all session state survive.
    fn suspend_gpu(&mut self) {
        if let (Some(egui_renderer), Some(viewport)) =
            (self.egui_renderer.as_mut(), self.viewport.take())
        {
            egui_renderer.free_texture(&viewport.id);
        } else {
            self.viewport = None;
        }
        self.surface = None;
        self.egui_renderer = None;
        self.egui_state = None;
        self.window = None;
        self.boot_log.line("suspended: window surface dropped");
    }

    /// Advances playback on the fixed-step clock (whole
    /// steps, bounded catch-up — the windowed shell's
    /// clock discipline, verbatim in miniature).
    fn advance_playback(&mut self) {
        if !self.play.is_advancing() {
            self.last_step = Instant::now();
            return;
        }
        let elapsed = self.last_step.elapsed().as_secs_f64();
        let mut steps = (elapsed / STEP_SECONDS).floor() as u64;
        if steps == 0 {
            return;
        }
        if steps > CATCH_UP_STEPS_MAX {
            steps = CATCH_UP_STEPS_MAX;
            self.last_step = Instant::now();
        } else {
            self.last_step += std::time::Duration::from_secs_f64(steps as f64 * STEP_SECONDS);
        }
        if let Some(scene) = self.state.scene.as_mut() {
            match self.play.step(scene, steps) {
                Ok(()) => self.viewport_dirty = true,
                Err(err) => self.refuse(format!("playback failed: {err}")),
            }
        }
    }

    /// Advances the Persona motion state on the same
    /// fixed-step clock.
    fn advance_persona(&mut self) {
        let elapsed = self.persona_last_step.elapsed().as_secs_f64();
        let mut steps = (elapsed / STEP_SECONDS).floor() as u64;
        if steps == 0 {
            return;
        }
        if steps > CATCH_UP_STEPS_MAX {
            steps = CATCH_UP_STEPS_MAX;
            self.persona_last_step = Instant::now();
        } else {
            self.persona_last_step +=
                std::time::Duration::from_secs_f64(steps as f64 * STEP_SECONDS);
        }
        for _ in 0..steps {
            self.persona.tick();
        }
    }

    /// Re-renders the stage viewport texture from the
    /// scene's current evaluated state (the windowed
    /// shell's refresh, in this shell's keeping).
    fn refresh_viewport(&mut self) {
        let (Some(renderer), Some(egui_renderer)) =
            (self.renderer.as_mut(), self.egui_renderer.as_mut())
        else {
            return;
        };
        let Some(scene) = self.state.scene.as_mut() else {
            return;
        };
        let frame = match prepare_frame(scene) {
            Ok(frame) => frame,
            Err(err) => {
                self.refuse(format!("frame preparation failed: {err}"));
                return;
            }
        };
        let canvas = cap_canvas(frame.canvas, VIEWPORT_SIDE_MAX);
        let size = [canvas.width, canvas.height];
        let needs_new = self
            .viewport
            .as_ref()
            .map(|current| current.size != size)
            .unwrap_or(true);
        if needs_new {
            if let Some(old) = self.viewport.take() {
                egui_renderer.free_texture(&old.id);
            }
            let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
                label: Some("torogao android viewport"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let id = egui_renderer.register_native_texture(
                renderer.device(),
                &view,
                wgpu::FilterMode::Linear,
            );
            self.viewport = Some(ViewportTexture { id, texture, size });
        }
        let current = self.viewport.as_ref().expect("viewport prepared above");
        let view = current
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let id = current.id;
        renderer.draw_into(
            scene,
            &frame,
            &view,
            (size[0], size[1]),
            wgpu::Color::TRANSPARENT,
            &[],
        );
        egui_renderer.update_egui_texture_from_wgpu_texture(
            renderer.device(),
            &view,
            wgpu::FilterMode::Linear,
            id,
        );
    }
}

impl AndroidShell {
    /// The stage's pinned comparison context, from the
    /// standing scene and the play host (identical in
    /// every area — persona rule 5, as the desktop fills
    /// it).
    fn stage_context(&self) -> StageContext {
        let (model_label, node_count, param_count) = match &self.state.scene {
            Some(scene) => (
                scene.model_name().to_string(),
                scene.puppet.nodes.nodes.len(),
                scene.puppet.params.len(),
            ),
            None => ("no model loaded".to_string(), 0, 0),
        };
        let play_state = match &self.state.scene {
            Some(scene) => {
                let snapshot = self.play.snapshot(scene);
                match &snapshot.animation {
                    Some(name) => format!(
                        "playing {name} · frame {} · step {}",
                        snapshot.frame, snapshot.step_count
                    ),
                    None => "stopped".to_string(),
                }
            }
            None => "stopped".to_string(),
        };
        StageContext {
            model_label,
            node_count,
            param_count,
            selected_node: self.state.selection.node_index,
            play_state,
        }
    }

    /// Draws one frame and returns the actions it asked
    /// for. The frame is the layout model made visible:
    /// stage header (top), standing bar (bottom edge),
    /// area rail (above the bar), one scrolling content
    /// column between.
    fn build_frame(&mut self, actions: &mut Vec<PhoneAction>) -> egui::FullOutput {
        let viewport_id = self
            .viewport
            .as_ref()
            .map(|current| (current.id, current.size));
        let context = self.stage_context();
        let (win_w, win_h) = self.window_size_dp();
        let viewport = ViewportDp {
            width: win_w,
            height: win_h,
        };
        let layout = frame_layout(viewport, self.stage_collapsed);
        let area = self.persona.current;
        let nerd = self.persona.nerd_mode;
        let status = self.status.clone();
        let status_is_refusal = self.status_is_refusal;
        let save_name = &mut self.save_name_input;
        let saves = self.settings.list_saves();
        let nerd_mode_on = self.settings.nerd_mode();
        let preference_line = format!(
            "preference: nerd_mode {} (file {})",
            if nerd_mode_on { "on" } else { "off" },
            self.settings.preference_file().display()
        );
        let egui_ctx = self.egui_context.clone();
        let window = self.window.clone();
        let raw_input = match (self.egui_state.as_mut(), window.as_ref()) {
            (Some(state), Some(window)) => state.take_egui_input(window),
            _ => egui::RawInput::default(),
        };
        egui_ctx.run_ui(raw_input, |ui| {
            draw_stage_header(ui, area, &context, viewport_id, &layout, actions);
            draw_standing_bar(ui, area, nerd, &status, status_is_refusal, &layout, actions);
            draw_area_rail(ui, area, &layout, actions);
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if area == Area::Settings {
                        draw_settings(
                            ui,
                            nerd_mode_on,
                            &preference_line,
                            save_name,
                            &saves,
                            actions,
                        );
                    } else {
                        draw_area_frame(ui, area, nerd);
                    }
                });
            });
        })
    }

    /// The window's logical size in dp (the layout model's
    /// unit of account).
    fn window_size_dp(&self) -> (f32, f32) {
        match self.window.as_ref() {
            Some(window) => {
                let size = window.inner_size();
                let scale = window.scale_factor() as f32;
                (size.width as f32 / scale, size.height as f32 / scale)
            }
            None => (0.0, 0.0),
        }
    }

    /// Applies the actions one frame collected.
    fn apply_actions(&mut self, actions: Vec<PhoneAction>) {
        for action in actions {
            match action {
                PhoneAction::SwitchArea(area) => {
                    self.persona.switch_area(area);
                    self.boot_log.line(&format!("area: {}", area.info().title));
                    self.confirm(format!("area: {}", area.info().title));
                }
                PhoneAction::ToggleNerdMode => self.toggle_nerd_mode(),
                PhoneAction::ToggleStageCollapse => {
                    self.stage_collapsed = !self.stage_collapsed;
                    self.viewport_dirty = true;
                }
                PhoneAction::TogglePlayPause => self.toggle_play_pause(),
                PhoneAction::SaveState => self.save_state_action(),
                PhoneAction::LoadState => self.load_state_action(),
                PhoneAction::FillSaveName(name) => {
                    self.save_name_input = name;
                }
            }
        }
    }

    /// The Settings toggle, end to end: presentation
    /// flips and the preference is persisted (write on
    /// change); both outcomes are recorded in the walk
    /// log and the status line.
    fn toggle_nerd_mode(&mut self) {
        let next = !self.persona.nerd_mode;
        match self.settings.set_nerd_mode(next) {
            Ok(wrote) => {
                self.persona.nerd_mode = next;
                self.walk_log.line(&format!(
                    "settings: nerd mode -> {} (preference written: {wrote})",
                    if next { "on" } else { "off" }
                ));
                self.confirm(format!("nerd mode {}", if next { "on" } else { "off" }));
            }
            Err(err) => {
                self.walk_log
                    .line(&format!("settings: nerd toggle refused: {err}"));
                self.refuse(format!("{err}"));
            }
        }
    }

    /// The stage transport: pause the ambient performance
    /// or resume it.
    fn toggle_play_pause(&mut self) {
        match self.state.scene.as_mut() {
            Some(scene) => {
                if self.play.is_advancing() {
                    self.play.pause();
                    self.confirm("paused".to_string());
                } else {
                    match self.play.play(scene, Some(AMBIENT_ANIMATION), true, None) {
                        Ok(()) => self.confirm(format!("playing {AMBIENT_ANIMATION}")),
                        Err(err) => self.refuse(format!("{err}")),
                    }
                }
            }
            None => self.refuse("no model loaded".to_string()),
        }
        self.viewport_dirty = true;
    }

    /// The Settings save action: the live state goes to
    /// disk under the field's name, through the store
    /// (which is the windowed save flow).
    fn save_state_action(&mut self) {
        let name = self.save_name_input.clone();
        match self
            .settings
            .save_session(&name, &mut self.state, &self.play)
        {
            Ok(message) => {
                self.walk_log
                    .line(&format!("settings: save '{name}': {message}"));
                self.confirm(format!("saved '{name}'"));
            }
            Err(err) => {
                self.walk_log
                    .line(&format!("settings: save '{name}' refused: {err}"));
                self.refuse(format!("{err}"));
            }
        }
    }

    /// The Settings load action: the named save restores
    /// scene, play position, and session coherence. The
    /// restored state replaces the shell's only after the
    /// store's validation has completed (its refusals
    /// leave the live state untouched).
    fn load_state_action(&mut self) {
        let name = self.save_name_input.clone();
        match self.settings.load_session(&name) {
            Ok((state, play, message)) => {
                self.state = state;
                self.play = play;
                self.start_session_host();
                self.viewport_dirty = true;
                self.walk_log
                    .line(&format!("settings: load '{name}': {message}"));
                self.confirm(format!("loaded '{name}'"));
            }
            Err(err) => {
                self.walk_log
                    .line(&format!("settings: load '{name}' refused: {err}"));
                self.refuse(format!("{err}"));
            }
        }
    }

    /// One redraw: advance, rebuild the viewport when
    /// dirty, run the UI, present. No surface or a
    /// zero-size window is a typed skip.
    fn redraw(&mut self) {
        let frame_started = Instant::now();
        let Some(window) = self.window.clone() else {
            return;
        };
        if self.surface.is_none() {
            return;
        }
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.advance_playback();
        self.advance_persona();
        if self.viewport_dirty {
            self.refresh_viewport();
            self.viewport_dirty = false;
        }
        let mut actions = Vec::new();
        let full_output = self.build_frame(&mut actions);
        self.apply_actions(actions);
        let Some(egui_state) = self.egui_state.as_mut() else {
            return;
        };
        egui_state.handle_platform_output(&window, full_output.platform_output);
        let paint_jobs = self
            .egui_context
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize_surface(size.width, size.height);
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.boot_log.line("surface lost; awaiting resume");
                self.surface = None;
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.boot_log
                    .line("surface texture validation error; frame skipped");
                return;
            }
        };
        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };
        let Some(egui_renderer) = self.egui_renderer.as_mut() else {
            return;
        };
        for (id, delta) in &full_output.textures_delta.set {
            egui_renderer.update_texture(renderer.device(), renderer.queue(), *id, delta);
        }
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            renderer
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("torogao android egui encoder"),
                });
        // egui-wgpu's contract: buffers upload before the
        // render pass consumes them (its render() panics
        // otherwise — found by the first emulator run,
        // gate B1's adversarial half).
        egui_renderer.update_buffers(
            renderer.device(),
            renderer.queue(),
            &mut encoder,
            &paint_jobs,
            &descriptor,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("torogao android egui pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.05,
                                g: 0.05,
                                b: 0.07,
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
            egui_renderer.render(&mut pass, &paint_jobs, &descriptor);
        }
        for id in &full_output.textures_delta.free {
            egui_renderer.free_texture(id);
        }
        renderer.queue().submit([encoder.finish()]);
        frame.present();
        self.frames_presented += 1;
        self.perf
            .push(frame_started.elapsed().as_secs_f32() * 1000.0);
        if self.frames_presented % PERF_WRITE_INTERVAL == 0 {
            self.write_perf_record();
        }
    }

    /// Reconfigures the surface to a new pixel size.
    fn resize_surface(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let (Some(renderer), Some(surface)) = (self.renderer.as_ref(), self.surface.as_ref())
        else {
            return;
        };
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

    /// The performance smoke record (B8's input — a
    /// smoke, labelled as such in the JSON itself).
    fn write_perf_record(&self) {
        if let Some(summary) = self.perf.summary() {
            write_json_atomic(
                &self.storage.evidence_dir().join(PERF_NAME),
                &summary.to_json("android-app", AMBIENT_ANIMATION),
            );
        }
    }
}

impl ApplicationHandler for AndroidShell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resume_gpu(event_loop);
        self.boot();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.suspend_gpu();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        let mut wants_redraw = false;
        if let Some(egui_state) = self.egui_state.as_mut() {
            wants_redraw = egui_state.on_window_event(&window, &event).repaint;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.resize_surface(size.width, size.height);
                self.viewport_dirty = true;
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {
                if wants_redraw {
                    window.request_redraw();
                }
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

/// The stage header: the viewport (aspect-fit, the
/// character always in frame — §3.3 rule 4), the area
/// title and host slot, the pinned context, and the
/// collapse + transport controls at or above the touch
/// floor.
fn draw_stage_header(
    ui: &mut egui::Ui,
    area: Area,
    pinned: &StageContext,
    viewport: Option<(egui::TextureId, [u32; 2])>,
    layout: &FrameLayout,
    actions: &mut Vec<PhoneAction>,
) {
    let info = area.info();
    egui::Panel::top("stage")
        .exact_size(layout.stage.height)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(info.title);
                ui.label(format!(
                    "host: {}",
                    info.host_slot.unwrap_or("hostless — the meta register")
                ));
            });
            if let Some((id, size)) = viewport {
                let available = ui.available_size();
                let image_height = (available.y - 56.0).max(1.0);
                let aspect = size[0] as f32 / size[1].max(1) as f32;
                let mut width = available.x;
                let mut height = width / aspect;
                if height > image_height {
                    height = image_height;
                    width = height * aspect;
                }
                ui.add(egui::Image::new((id, egui::vec2(width, height))));
            } else {
                ui.label("viewport warming up");
            }
            ui.label(format!(
                "{} · {} nodes · {} params · {}",
                pinned.model_label, pinned.node_count, pinned.param_count, pinned.play_state
            ));
            ui.horizontal(|ui| {
                if ui
                    .add_sized(
                        [120.0, COMMAND_ROW_HEIGHT_DP],
                        egui::Button::new("Persona / Nerd"),
                    )
                    .clicked()
                {
                    actions.push(PhoneAction::ToggleNerdMode);
                }
                if ui
                    .add_sized(
                        [120.0, COMMAND_ROW_HEIGHT_DP],
                        egui::Button::new("Play / Pause"),
                    )
                    .clicked()
                {
                    actions.push(PhoneAction::TogglePlayPause);
                }
                if ui
                    .add_sized(
                        [120.0, COMMAND_ROW_HEIGHT_DP],
                        egui::Button::new("Stage +/-"),
                    )
                    .clicked()
                {
                    actions.push(PhoneAction::ToggleStageCollapse);
                }
            });
        });
}

/// The standing bar: pinned to the bottom edge, never
/// scrolled away (§3.3 rule 5) — the presentation toggle,
/// the current area, and the status line live here.
fn draw_standing_bar(
    ui: &mut egui::Ui,
    area: Area,
    nerd: bool,
    status: &str,
    status_is_refusal: bool,
    layout: &FrameLayout,
    actions: &mut Vec<PhoneAction>,
) {
    egui::Panel::bottom("standing")
        .exact_size(layout.standing_bar.height)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_sized(
                        [140.0, COMMAND_ROW_HEIGHT_DP],
                        egui::Button::new(if nerd { "Nerd: on" } else { "Persona: on" }),
                    )
                    .clicked()
                {
                    actions.push(PhoneAction::ToggleNerdMode);
                }
                ui.strong(area.info().title);
                let colour = if status_is_refusal {
                    egui::Color32::from_rgb(0xFF, 0x70, 0x70)
                } else {
                    ui.visuals().text_color()
                };
                ui.colored_label(colour, status);
            });
        });
}

/// The area rail: twelve chips, one tap each, horizontal
/// scroll — the phone form of the desktop rail. Every
/// chip meets the touch floor and carries its area's
/// accent.
fn draw_area_rail(
    ui: &mut egui::Ui,
    current: Area,
    layout: &FrameLayout,
    actions: &mut Vec<PhoneAction>,
) {
    egui::Panel::bottom("rail")
        .exact_size(layout.rail.height)
        .show(ui, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for area in Area::ALL {
                        let info = area.info();
                        let [r, g, b] = info.accent.unwrap_or([0x8A, 0x8A, 0x93]);
                        let mut button = egui::Button::new(info.title);
                        if area == current {
                            button = button.fill(egui::Color32::from_rgb(r, g, b));
                        } else {
                            button = button
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(r, g, b)));
                        }
                        if ui
                            .add_sized([RAIL_CHIP_MIN_WIDTH_DP, 48.0], button)
                            .clicked()
                        {
                            actions.push(PhoneAction::SwitchArea(area));
                        }
                    }
                });
            });
        });
}

/// The Settings area, live: the presentation toggle with
/// its persisted readback, and the session save/load the
/// enumeration promises — every control at or above the
/// touch floor, every outcome in the status line and the
/// walk log.
fn draw_settings(
    ui: &mut egui::Ui,
    nerd_mode_on: bool,
    preference_line: &str,
    save_name: &mut String,
    saves: &[String],
    actions: &mut Vec<PhoneAction>,
) {
    ui.heading("Settings");
    ui.label("Frame only areas are A-2's work; Settings is live in A-1.");
    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .add_sized(
                [180.0, COMMAND_ROW_HEIGHT_DP],
                egui::Button::new(if nerd_mode_on {
                    "Nerd mode: on — tap for Persona"
                } else {
                    "Nerd mode: off — tap for Nerd"
                }),
            )
            .clicked()
        {
            actions.push(PhoneAction::ToggleNerdMode);
        }
    });
    ui.label(preference_line);
    ui.separator();
    ui.label("Session state (save / load):");
    let name_response = ui.add_sized(
        [280.0, COMMAND_ROW_HEIGHT_DP],
        egui::TextEdit::singleline(save_name).hint_text("save name"),
    );
    if name_response.has_focus() {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::IMEAllowed(true));
    }
    ui.horizontal(|ui| {
        if ui
            .add_sized(
                [120.0, COMMAND_ROW_HEIGHT_DP],
                egui::Button::new("Save state"),
            )
            .clicked()
        {
            actions.push(PhoneAction::SaveState);
        }
        if ui
            .add_sized(
                [120.0, COMMAND_ROW_HEIGHT_DP],
                egui::Button::new("Load state"),
            )
            .clicked()
        {
            actions.push(PhoneAction::LoadState);
        }
    });
    if saves.is_empty() {
        ui.label("no saves yet");
    } else {
        ui.label("existing saves (tap a name to fill the field):");
        for save in saves {
            if ui
                .add_sized(
                    [280.0, COMMAND_ROW_HEIGHT_DP],
                    egui::Button::new(save.as_str()),
                )
                .clicked()
            {
                actions.push(PhoneAction::FillSaveName(save.clone()));
            }
        }
    }
}

/// A not-yet-claimed area's frame: its homed commands as
/// rows (labels, never fake buttons — a control that does
/// nothing is a lie the audit forbids) under the standing
/// not-claimed note. A-2 takes these up area by area.
fn draw_area_frame(ui: &mut egui::Ui, area: Area, nerd: bool) {
    let info = area.info();
    ui.heading(info.title);
    ui.label(format!("register: {}", info.register));
    ui.label("frame only in A-1 — not claimed operable (A-2 takes this area up)");
    ui.separator();
    for command in persona::commands(area) {
        ui.add_sized(
            [ui.available_width(), COMMAND_ROW_HEIGHT_DP],
            egui::Label::new(format!(
                "{} · {}",
                command.label,
                if nerd { command.id.as_str() } else { "—" }
            )),
        );
    }
}

/// The Android entry point (winit's `android_main`
/// contract, reached through the crate root's exported
/// symbol). Storage roots at the activity's internal data
/// path; every fallible step is logged, and the loop's
/// exit is the process's exit.
#[cfg(target_os = "android")]
pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use std::path::PathBuf;

    use winit::event_loop::{ControlFlow, EventLoop};
    use winit::platform::android::EventLoopBuilderExtAndroid as _;

    use crate::evidence::{BOOT_LOG_NAME, SETTINGS_WALK_NAME};

    let root = app
        .internal_data_path()
        .or_else(|| app.external_data_path())
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/torogao-android"));
    let storage = Storage::new(root.clone());
    if let Err(err) = storage.create_dirs() {
        eprintln!(
            "torogao-android: storage unavailable at {}: {err}",
            root.display()
        );
        return;
    }
    let boot_log = EvidenceLog::new(&storage.evidence_dir(), BOOT_LOG_NAME);
    let walk_log = EvidenceLog::new(&storage.evidence_dir(), SETTINGS_WALK_NAME);
    boot_log.line("torogao-android starting (stage A-1 shell)");
    let mut shell = AndroidShell::new(storage, boot_log, walk_log);
    let event_loop = match EventLoop::with_user_event().with_android_app(app).build() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            shell
                .boot_log
                .line(&format!("event loop unavailable: {err}"));
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    if let Err(err) = event_loop.run_app(&mut shell) {
        shell.boot_log.line(&format!("event loop exited: {err}"));
    }
}
