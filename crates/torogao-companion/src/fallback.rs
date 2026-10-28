//! The fallback presentation (X11 / Windows, and the
//! build-proof route everywhere): winit with a
//! transparent, decorationless, always-on-top window,
//! reusing the app crate's surface seam (the same one the
//! panel shell uses).
//!
//! What the fallback cannot do is stated plainly
//! (PROGRAM.md §3.1): winit exposes noway to restrict the
//! OS-level hit region to her silhouette, and X11's
//! SHAPE extension binding is a post-1.1 refinement. So
//! the fallback gates its *own* pointer actions on the
//! silhouette (a click off her body does nothing — no
//! Greeting, no drag), while the window rectangle itself
//! still receives the events at OS level. On Wayland this
//! fallback is not used at all (no client positioning
//! exists there); it exists for X11/Windows desktops and
//! to prove the companion core presentation-independent.
//!
//! Placement: winit can position its own window on
//! X11/Windows, so the persisted anchor + margins become
//! absolute coordinates against the primary monitor.

use std::time::{Duration, Instant};

use torogao_app::render::{Renderer, prepare_frame};
use torogao_app::textures::encode_png;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{WindowAttributes, WindowId, WindowLevel};

use crate::core::{AMBIENT_ANIMATION, CLICK_ANIMATION, CompanionCore};
use crate::error::{CompanionError, CompanionResult};
use crate::menu::{MenuAction, MenuUi};
use crate::silhouette::{Silhouette, bbox_materially_changed, point_in_silhouette, silhouette_of};
use crate::state::save_state;
use crate::view::{fit_view, surface_size_for_height};
use crate::wayland::{RunOptions, RunSummary};

/// Left-press distance (px) under which a release is a
/// click rather than a drag — mirrors the Wayland route.
const DRAG_THRESHOLD_PX: f64 = 6.0;
/// Silhouette re-measure cadence, presented frames.
const SILHOUETTE_REFRESH_FRAMES: u64 = 30;

struct FallbackApp {
    options: RunOptions,
    core: CompanionCore,
    window: Option<std::sync::Arc<winit::window::Window>>,
    renderer: Option<Renderer>,
    gpu_surface: Option<wgpu::Surface<'static>>,
    surface_format: Option<wgpu::TextureFormat>,
    size: (u32, u32),
    cursor: (f64, f64),
    press: Option<((f64, f64), bool)>,
    menu: Option<MenuUi>,
    current_silhouette: Option<(Silhouette, (u32, u32))>,
    last_silhouette_bbox: Option<(u32, u32, u32, u32)>,
    frames_since_silhouette: u64,
    chain: u64,
    summary: RunSummary,
    frame_times: Vec<Instant>,
    last_hash: Option<u64>,
    placement_dirty: bool,
    failed: Option<CompanionError>,
}

impl FallbackApp {
    fn window_position(&self) -> (i32, i32) {
        // Against the primary monitor when known at window
        // creation; the run() prologue positions after
        // creation with the real monitor size. Here the
        // persisted margins alone anchor the first frame.
        let p = &self.options.placement;
        let x = if p.anchor.is_right() {
            -0
        } else {
            p.margin_side
        };
        let y = if p.anchor.is_bottom() {
            0
        } else {
            p.margin_edge
        };
        (x, y)
    }

    fn tick_and_present(&mut self) {
        if self.failed.is_some() {
            return;
        }
        let hash = match self.core.tick() {
            Ok(hash) => hash,
            Err(err) => {
                self.failed = Some(err);
                return;
            }
        };
        if let Some(previous) = self.last_hash {
            if previous != hash {
                self.summary.moving_steps += 1;
            }
        }
        self.last_hash = Some(hash);
        self.chain = crate::perf::chain_step(self.chain, hash);
        self.summary.trace_hash = self.chain;
        self.summary.final_state_hash = hash;
        self.present();
        self.summary.frames_presented += 1;
        self.frame_times.push(Instant::now());
        self.frames_since_silhouette += 1;
        if self.frames_since_silhouette >= SILHOUETTE_REFRESH_FRAMES {
            self.frames_since_silhouette = 0;
            self.refresh_silhouette();
        }
        if let Some(budget) = self.options.frames {
            if self.summary.frames_presented >= budget {
                self.finish_evidence();
            }
        }
    }

    fn present(&mut self) {
        let (Some(renderer), Some(surface)) = (self.renderer.as_mut(), self.gpu_surface.as_ref())
        else {
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
        let scene = match self.core.scene_mut() {
            Ok(scene) => scene,
            Err(err) => {
                self.failed = Some(err);
                return;
            }
        };
        let mut prepared = match prepare_frame(scene) {
            Ok(prepared) => prepared,
            Err(err) => {
                eprintln!("torogao-companion: frame preparation failed: {err}");
                return;
            }
        };
        prepared.canvas.view = fit_view(&prepared.canvas, self.size.0, self.size.1);
        renderer.draw_into(
            scene,
            &prepared,
            &view,
            self.size,
            wgpu::Color::TRANSPARENT,
            &[],
        );
        let mut actions: Vec<MenuAction> = Vec::new();
        if let Some(menu) = &mut self.menu {
            actions = menu.draw(
                renderer.device(),
                renderer.queue(),
                &view,
                self.size,
                &self.options.character_name,
            );
        }
        frame.present();
        if !actions.is_empty() {
            self.apply_menu_actions(actions);
        }
    }

    fn refresh_silhouette(&mut self) {
        let (Ok(scene), Some(renderer)) = (self.core.scene_mut(), self.renderer.as_mut()) else {
            return;
        };
        let Ok((rgba, _report)) = renderer.render(scene) else {
            return;
        };
        let space = (rgba.width, rgba.height);
        let silhouette = silhouette_of(&rgba);
        let changed = bbox_materially_changed(self.last_silhouette_bbox, silhouette.bbox);
        self.last_silhouette_bbox = silhouette.bbox;
        self.current_silhouette = Some((silhouette, space));
        let _ = changed; // No OS region on this route; the
        // silhouette gates pointer actions in-process.
    }

    fn pointer_on_her(&self, pos: (f64, f64)) -> bool {
        if self.options.placement.click_through_locked {
            return false;
        }
        match &self.current_silhouette {
            Some((silhouette, space)) => {
                point_in_silhouette(silhouette, *space, self.size, (pos.0, pos.1))
            }
            None => true, // Before the first measure: accept.
        }
    }

    fn open_menu(&mut self) {
        let (Some(renderer), Some(format)) = (self.renderer.as_ref(), self.surface_format) else {
            return;
        };
        let mut menu = MenuUi::new(renderer.device(), format, self.cursor);
        menu.click_through_locked = self.options.placement.click_through_locked;
        self.menu = Some(menu);
    }

    fn apply_menu_actions(&mut self, actions: Vec<MenuAction>) {
        let mut placement = self.options.placement.clone();
        for action in actions {
            match action {
                MenuAction::PlayGreeting => {
                    if let Err(err) = self.core.play(CLICK_ANIMATION, false) {
                        eprintln!("torogao-companion: greeting refused: {err}");
                    }
                    self.menu = None;
                }
                MenuAction::PlayDaydream => {
                    if let Err(err) = self.core.play(AMBIENT_ANIMATION, true) {
                        eprintln!("torogao-companion: daydream refused: {err}");
                    }
                    self.menu = None;
                }
                MenuAction::OpenTorogao => {
                    self.open_torogao();
                    self.menu = None;
                }
                MenuAction::ToggleClickThrough => {
                    placement.click_through_locked = !placement.click_through_locked;
                    if let Some(menu) = &mut self.menu {
                        menu.click_through_locked = placement.click_through_locked;
                    }
                    self.placement_dirty = true;
                }
                MenuAction::Quit => {
                    self.menu = None;
                    self.request_exit();
                }
                MenuAction::Close => {
                    self.menu = None;
                }
            }
        }
        self.options.placement = placement;
    }

    fn open_torogao(&self) {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|dir| dir.join("torogao-app")));
        let Some(exe) = exe else {
            eprintln!("torogao-companion: cannot locate the torogao-app sibling binary");
            return;
        };
        if let Err(err) = std::process::Command::new(&exe)
            .arg(self.core.model_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            eprintln!(
                "torogao-companion: opening Torogao via {} failed: {err}",
                exe.display()
            );
        }
    }

    fn request_exit(&mut self) {
        // The event loop is exited from the handler paths;
        // here we set the budget to the current count so
        // the next tick finishes evidence and the redraw
        // handler exits.
        self.options.frames = Some(self.summary.frames_presented);
    }

    fn finish_evidence(&mut self) {
        let mut intervals: Vec<f64> = self
            .frame_times
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]).as_secs_f64() * 1000.0)
            .collect();
        if !intervals.is_empty() {
            intervals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mid = intervals.len() / 2;
            self.summary.frame_interval_median_ms = if intervals.len() % 2 == 1 {
                intervals[mid]
            } else {
                (intervals[mid - 1] + intervals[mid]) / 2.0
            };
        }
        self.summary.session_id = self.core.session_id.clone();
        if let Some(path) = &self.options.screenshot {
            if let (Ok(scene), Some(renderer)) = (self.core.scene_mut(), self.renderer.as_mut()) {
                if let Ok((rgba, _report)) = renderer.render(scene) {
                    match encode_png(&rgba) {
                        Ok(png) => {
                            if let Err(err) = std::fs::write(path, png) {
                                eprintln!("torogao-companion: screenshot write failed: {err}");
                            }
                        }
                        Err(err) => {
                            eprintln!("torogao-companion: screenshot encode failed: {err}")
                        }
                    }
                }
            }
        }
        if let Some(path) = &self.options.report {
            let report = serde_json::json!({
                "frames_presented": self.summary.frames_presented,
                "trace_hash": format!("{:#018x}", self.summary.trace_hash),
                "final_state_hash": format!("{:#018x}", self.summary.final_state_hash),
                "frame_interval_median_ms": self.summary.frame_interval_median_ms,
                "moving_steps": self.summary.moving_steps,
                "session_id": self.summary.session_id,
                "presentation": "winit-fallback",
            });
            if let Ok(text) = serde_json::to_string_pretty(&report) {
                if let Err(err) = std::fs::write(path, text) {
                    eprintln!("torogao-companion: report write failed: {err}");
                }
            }
        }
    }

    fn leave(&mut self, event_loop: &ActiveEventLoop) {
        if self.placement_dirty && self.options.placement.validate().is_ok() {
            let _ = save_state(&self.options.state_path, &self.options.placement);
        }
        event_loop.exit();
    }
}

impl ApplicationHandler for FallbackApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attrs = WindowAttributes::default()
            .with_title("Torogao Companion")
            .with_decorations(false)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_resizable(false)
            .with_inner_size(LogicalSize::new(
                f64::from(self.size.0),
                f64::from(self.size.1),
            ));
        // Position from anchor + margins against the
        // primary monitor (X11/Windows allow it).
        if let Some(monitor) = event_loop.primary_monitor() {
            let msize = monitor.size();
            let scale = monitor.scale_factor();
            let (win_w, win_h) = (
                f64::from(self.size.0) * scale,
                f64::from(self.size.1) * scale,
            );
            let p = &self.options.placement;
            let x = if p.anchor.is_right() {
                f64::from(msize.width) - win_w - f64::from(p.margin_side) * scale
            } else {
                f64::from(p.margin_side) * scale
            };
            let y = if p.anchor.is_bottom() {
                f64::from(msize.height) - win_h - f64::from(p.margin_edge) * scale
            } else {
                f64::from(p.margin_edge) * scale
            };
            attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        let _ = self.window_position();
        match event_loop.create_window(attrs) {
            Ok(window) => {
                let window = std::sync::Arc::new(window);
                match Renderer::new() {
                    Ok(renderer) => {
                        match renderer.create_surface(window.clone()) {
                            Ok(surface) => {
                                self.surface_format = Some(crate::view::configure_surface(
                                    &renderer, &surface, self.size,
                                ));
                                self.gpu_surface = Some(surface);
                                self.renderer = Some(renderer);
                            }
                            Err(err) => {
                                self.failed =
                                    Some(CompanionError::new("TG-COMP-SURFACE", err.to_string()));
                            }
                        }
                        self.window = Some(window);
                    }
                    Err(err) => {
                        self.failed =
                            Some(CompanionError::new("TG-COMP-NO-ADAPTER", err.to_string()));
                    }
                }
            }
            Err(err) => {
                self.failed = Some(CompanionError::new("TG-COMP-WINDOW", err.to_string()));
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => self.leave(event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                if let Some(menu) = &mut self.menu {
                    menu.push_pointer_move(self.cursor);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                if self.menu.is_some() {
                    if let Some(menu) = &mut self.menu {
                        menu.push_pointer_button(
                            button == MouseButton::Left,
                            button == MouseButton::Right,
                            pressed,
                        );
                    }
                    return;
                }
                match (button, pressed) {
                    (MouseButton::Left, true) => {
                        self.press = Some((self.cursor, self.pointer_on_her(self.cursor)));
                    }
                    (MouseButton::Left, false) => {
                        if let Some((start, on_her)) = self.press.take() {
                            let delta = (self.cursor.0 - start.0, self.cursor.1 - start.1);
                            if on_her
                                && delta.0.abs() <= DRAG_THRESHOLD_PX
                                && delta.1.abs() <= DRAG_THRESHOLD_PX
                            {
                                if let Err(err) = self.core.play(CLICK_ANIMATION, false) {
                                    eprintln!("torogao-companion: greeting refused: {err}");
                                }
                            }
                            // Dragging the *window* itself is
                            // the OS's job on this route (the
                            // window is frameless but movable
                            // via the platform); margin
                            // persistence from drags is a
                            // Wayland-route behaviour at this
                            // slice, named in the gate report.
                        }
                    }
                    (MouseButton::Right, true) if self.pointer_on_her(self.cursor) => {
                        self.open_menu();
                    }
                    _ => {}
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && matches!(
                        event.logical_key,
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape)
                    )
                {
                    if self.menu.is_some() {
                        self.menu = None;
                    } else {
                        self.leave(event_loop);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.tick_and_present();
                if self.failed.is_some() {
                    self.leave(event_loop);
                    return;
                }
                if let Some(budget) = self.options.frames {
                    if self.summary.frames_presented >= budget {
                        self.leave(event_loop);
                        return;
                    }
                }
                // Fixed-step cadence: request the next frame
                // after ~1/60 s via about-to-wait pacing.
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_micros(16_667),
        ));
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// Runs the fallback presentation.
pub fn run(options: RunOptions) -> CompanionResult<RunSummary> {
    let mut core = CompanionCore::load(
        &options.model,
        options.session_dir.as_deref(),
        options.session_port,
        &options.held_by,
    )?;
    let size = {
        let scene = core.scene_mut()?;
        let prepared =
            prepare_frame(scene).map_err(|err| CompanionError::model(err.to_string()))?;
        surface_size_for_height(options.placement.display_height, &prepared.canvas)
    };
    let mut app = FallbackApp {
        options,
        core,
        window: None,
        renderer: None,
        gpu_surface: None,
        surface_format: None,
        size,
        cursor: (0.0, 0.0),
        press: None,
        menu: None,
        current_silhouette: None,
        last_silhouette_bbox: None,
        frames_since_silhouette: SILHOUETTE_REFRESH_FRAMES - 1,
        chain: crate::perf::chain_hash(&[]),
        summary: RunSummary::default(),
        frame_times: Vec::new(),
        last_hash: None,
        placement_dirty: false,
        failed: None,
    };
    let event_loop = EventLoop::new()
        .map_err(|err| CompanionError::new("TG-COMP-EVENT-LOOP", err.to_string()))?;
    event_loop
        .run_app(&mut app)
        .map_err(|err| CompanionError::new("TG-COMP-EVENT-LOOP", err.to_string()))?;
    if let Some(err) = app.failed {
        return Err(err);
    }
    Ok(app.summary)
}
