//! The Wayland presentation (the primary route, primo's
//! Hyprland): a `zwlr_layer_shell_v1` overlay surface via
//! smithay-client-toolkit.
//!
//! Why layer-shell and not winit here is PROGRAM.md §3.1:
//! under Wayland a client cannot position its own window,
//! demand always-on-top, or make screen regions
//! click-through through winit; layer-shell is the
//! wlroots-native mechanism for all three (anchored
//! surface, overlay layer, and a `wl_surface` input region
//! restricted to her alpha silhouette). The surface is
//! drawn with the app crate's wgpu renderer through the
//! `Renderer::create_surface_from_target` seam, on a
//! handle built from the layer surface's own pointers —
//! the connection outlives the surface by construction
//! (both live in this module's state for the whole run).
//!
//! Placement is anchor + margins (layer-shell surfaces are
//! compositor-placed; the drag is margin arithmetic, §3.1),
//! persisted through `state`. Keyboard interactivity is
//! on-demand only: she never takes the keyboard by being
//! present.
//!
//! Click-through lock note (an honest seam): while the
//! lock is on, her input region is empty and *no* pointer
//! event reaches her — including the click that would lift
//! the lock. The lock can be lifted from the fallback
//! presentation, by editing the state file, or with
//! `--click-through-lock off` on the command line (which
//! rewrites the persisted toggle). That is stated here
//! and in the gate report rather than hidden.

use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle as RawWaylandWindowHandle,
    WindowHandle,
};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopSignal};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::{
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, registry_handlers,
};
use torogao_app::render::{Renderer, prepare_frame};
use torogao_app::textures::encode_png;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use wayland_client::{Connection, Proxy, QueueHandle};

use crate::core::{AMBIENT_ANIMATION, CLICK_ANIMATION, CompanionCore};
use crate::error::{CompanionError, CompanionResult};
use crate::menu::{MenuAction, MenuUi};
use crate::silhouette::{Silhouette, bbox_materially_changed, scale_rects, silhouette_of};
use crate::state::{Anchor as StateAnchor, CompanionState, drag_margins, save_state};
use crate::view::{fit_view, surface_size_for_height};

/// Linux pointer button codes (input-event-codes.h), as
/// `wl_pointer` reports them.
const BTN_LEFT: u32 = 0x110;
/// See `BTN_LEFT`.
const BTN_RIGHT: u32 = 0x111;
/// Drag threshold, pixels: below this a press+release is
/// a click (Greeting); above it, a drag (move).
const DRAG_THRESHOLD_PX: f64 = 6.0;
/// Presentation cadence: one fixed step (1/60 s) per tick.
const TICK: Duration = Duration::from_micros(16_667);
/// Silhouette re-measure cadence, in presented frames:
/// she breathes and moves, so the region is a living
/// shape, refreshed on this cadence and applied when the
/// box materially changed.
const SILHOUETTE_REFRESH_FRAMES: u64 = 30;
/// Pixels of her that a drag may park off the anchored
/// edges (the rest must stay grabbable).
const KEEP_VISIBLE_PX: i32 = 48;

/// Run options shared by both presentations (parsed in
/// `main`).
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// The repertoire model (Prima's rig fixture).
    pub model: std::path::PathBuf,
    /// Display name for the menu header (derived from the
    /// model file in `main`).
    pub character_name: String,
    /// Where the placement state lives.
    pub state_path: std::path::PathBuf,
    /// The loaded placement state.
    pub placement: CompanionState,
    /// Session directory for the hosted session server;
    /// `None` disables serving.
    pub session_dir: Option<std::path::PathBuf>,
    /// Session WebSocket port override.
    pub session_port: Option<u16>,
    /// Present this many frames, then exit (the evidence
    /// mode); `None` runs until quit.
    pub frames: Option<u64>,
    /// Evidence screenshot path (offscreen render of the
    /// final presented state), written at exit in
    /// `--frames` mode.
    pub screenshot: Option<std::path::PathBuf>,
    /// Evidence report path (JSON), written at exit in
    /// `--frames` mode.
    pub report: Option<std::path::PathBuf>,
    /// Session holder name for the hosted session.
    pub held_by: String,
}

/// The outcome of a run (the evidence numbers Gate G5/G6
/// read).
#[derive(Debug, Default)]
pub struct RunSummary {
    /// Frames presented.
    pub frames_presented: u64,
    /// Chain hash over the presented steps' evaluated
    /// state hashes.
    pub trace_hash: u64,
    /// Evaluated state hash of the final presented frame.
    pub final_state_hash: u64,
    /// Median interval between presented frames, ms.
    pub frame_interval_median_ms: f64,
    /// Presented steps whose evaluated state changed
    /// versus the previous presented step.
    pub moving_steps: usize,
    /// The hosted session's id, when serving.
    pub session_id: Option<String>,
}

/// The raw handles wgpu needs for the layer surface: the
/// `wl_display` and `wl_surface` pointers of *this*
/// process's connection. Stored as addresses so the struct
/// is `Send + Sync` for wgpu's target box; the pointers are
/// valid for the whole run because the `Connection` and the
/// `LayerSurface` outlive the wgpu surface (both are owned
/// by the presentation state and dropped after it).
struct LayerHandles {
    display: usize,
    surface: usize,
}

impl HasWindowHandle for LayerHandles {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let surface =
            NonNull::new(self.surface as *mut std::ffi::c_void).ok_or(HandleError::Unavailable)?;
        let raw = RawWindowHandle::Wayland(RawWaylandWindowHandle::new(surface));
        // SAFETY: the wl_surface proxy lives in the
        // presentation state for the surface's whole
        // lifetime, and this handle is used only to create
        // that surface.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for LayerHandles {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let display =
            NonNull::new(self.display as *mut std::ffi::c_void).ok_or(HandleError::Unavailable)?;
        let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display));
        // SAFETY: as `window_handle` — the connection
        // outlives every surface created from it here.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

struct DragState {
    start_pos: (f64, f64),
    start_margins: (i32, i32),
    moved: bool,
}

struct Presentation {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor_state: CompositorState,
    layer: Option<LayerSurface>,
    core: CompanionCore,
    placement: CompanionState,
    options: RunOptions,
    desired_size: (u32, u32),
    display_ptr: usize,
    surface_ptr: usize,
    renderer: Option<Renderer>,
    gpu_surface: Option<wgpu::Surface<'static>>,
    surface_format: Option<wgpu::TextureFormat>,
    gpu_configured_size: Option<(u32, u32)>,
    configured_size: Option<(u32, u32)>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_pos: (f64, f64),
    drag: Option<DragState>,
    menu: Option<MenuUi>,
    region: Option<Region>,
    current_silhouette: Option<(Silhouette, (u32, u32))>,
    last_silhouette_bbox: Option<(u32, u32, u32, u32)>,
    frames_since_silhouette: u64,
    chain: u64,
    summary: RunSummary,
    frame_times: Vec<Instant>,
    last_hash: Option<u64>,
    exit: bool,
    placement_dirty: bool,
    loop_signal: Option<LoopSignal>,
}

impl Presentation {
    fn layer(&self) -> &LayerSurface {
        self.layer
            .as_ref()
            .expect("layer surface is created before the event loop runs")
    }

    fn layer_anchor(anchor: StateAnchor) -> Anchor {
        match anchor {
            StateAnchor::BottomLeft => Anchor::BOTTOM | Anchor::LEFT,
            StateAnchor::BottomRight => Anchor::BOTTOM | Anchor::RIGHT,
            StateAnchor::TopLeft => Anchor::TOP | Anchor::LEFT,
            StateAnchor::TopRight => Anchor::TOP | Anchor::RIGHT,
        }
    }

    fn apply_layer_placement(&self) {
        let layer = self.layer();
        layer.set_anchor(Self::layer_anchor(self.placement.anchor));
        layer.set_margin(
            if self.placement.anchor.is_bottom() {
                0
            } else {
                self.placement.margin_edge
            },
            if self.placement.anchor.is_right() {
                self.placement.margin_side
            } else {
                0
            },
            if self.placement.anchor.is_bottom() {
                self.placement.margin_edge
            } else {
                0
            },
            if self.placement.anchor.is_right() {
                0
            } else {
                self.placement.margin_side
            },
        );
    }

    fn persist_placement(&mut self) {
        if !self.placement_dirty {
            return;
        }
        match self
            .placement
            .validate()
            .and_then(|()| save_state(&self.options.state_path, &self.placement))
        {
            Ok(()) => self.placement_dirty = false,
            Err(err) => eprintln!("torogao-companion: state save refused: {err}"),
        }
    }

    fn apply_input_region(&mut self) {
        let Some((width, height)) = self.configured_size else {
            return;
        };
        let Ok(region) = Region::new(&self.compositor_state) else {
            return;
        };
        if self.placement.click_through_locked {
            // Empty region: everything passes through.
        } else if self.menu.is_some() {
            region.add(0, 0, width as i32, height as i32);
        } else if let Some((silhouette, space)) = &self.current_silhouette {
            for rect in scale_rects(&silhouette.rects, *space, (width, height)) {
                region.add(rect[0], rect[1], rect[2], rect[3]);
            }
        } else {
            // No silhouette measured yet: accept the full
            // surface until the first refresh lands (the
            // first tick).
            region.add(0, 0, width as i32, height as i32);
        }
        self.layer()
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
        self.region = Some(region);
    }

    /// One presentation tick: advance the performance one
    /// fixed step, present it, and keep the evidence
    /// bookkeeping. Driven by the calloop timer.
    fn tick(&mut self, qh: &QueueHandle<Self>) {
        if self.exit {
            return;
        }
        let Some((width, height)) = self.configured_size else {
            return; // No configure yet: nothing to present into.
        };
        if width == 0 || height == 0 {
            return;
        }
        let hash = match self.core.tick() {
            Ok(hash) => hash,
            Err(err) => {
                eprintln!("torogao-companion: evaluation failed: {err}");
                self.exit = true;
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

        if self.gpu_surface.is_none() && !self.create_gpu_surface() {
            self.exit = true;
            return;
        }
        self.present_frame(width, height, qh);
        self.summary.frames_presented += 1;
        self.frame_times.push(Instant::now());
        if self.frame_times.len() > 8192 {
            self.frame_times.remove(0);
        }
        self.frames_since_silhouette += 1;
        if self.frames_since_silhouette >= SILHOUETTE_REFRESH_FRAMES {
            self.frames_since_silhouette = 0;
            self.refresh_silhouette();
        }
        if let Some(budget) = self.options.frames {
            if self.summary.frames_presented >= budget {
                self.finish_evidence();
                self.exit = true;
            }
        }
    }

    fn create_gpu_surface(&mut self) -> bool {
        let renderer = match Renderer::new() {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("torogao-companion: adapter/device failed: {err}");
                return false;
            }
        };
        let handles = LayerHandles {
            display: self.display_ptr,
            surface: self.surface_ptr,
        };
        let target = wgpu::SurfaceTarget::DisplayAndWindow(
            Box::new(handles) as Box<dyn wgpu::DisplayAndWindowHandle>
        );
        let surface = match renderer.create_surface_from_target(target) {
            Ok(surface) => surface,
            Err(err) => {
                eprintln!("torogao-companion: surface creation failed: {err}");
                return false;
            }
        };
        self.surface_format = Some(renderer.surface_format(&surface));
        self.renderer = Some(renderer);
        self.gpu_surface = Some(surface);
        true
    }

    fn configure_gpu_surface(&mut self, width: u32, height: u32) {
        let (Some(renderer), Some(surface)) = (self.renderer.as_ref(), self.gpu_surface.as_ref())
        else {
            return;
        };
        crate::view::configure_surface(renderer, surface, (width, height));
        self.gpu_configured_size = Some((width, height));
    }

    fn present_frame(&mut self, width: u32, height: u32, qh: &QueueHandle<Self>) {
        if self.gpu_configured_size != Some((width, height)) {
            self.configure_gpu_surface(width, height);
        }
        let (Some(renderer), Some(surface)) = (self.renderer.as_mut(), self.gpu_surface.as_ref())
        else {
            return;
        };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.gpu_configured_size = None;
                return;
            }
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let scene = match self.core.scene_mut() {
            Ok(scene) => scene,
            Err(err) => {
                eprintln!("torogao-companion: scene vanished: {err}");
                self.exit = true;
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
        prepared.canvas.view = fit_view(&prepared.canvas, width, height);
        renderer.draw_into(
            scene,
            &prepared,
            &view,
            (width, height),
            wgpu::Color::TRANSPARENT,
            &[],
        );
        let mut actions: Vec<MenuAction> = Vec::new();
        if let Some(menu) = &mut self.menu {
            actions = menu.draw(
                renderer.device(),
                renderer.queue(),
                &view,
                (width, height),
                &self.options.character_name,
            );
        }
        frame.present();
        self.layer()
            .wl_surface()
            .frame(qh, self.layer().wl_surface().clone());
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
        if changed {
            self.apply_input_region();
        }
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
        if let Some(path) = self.options.screenshot.clone() {
            self.write_screenshot(&path);
        }
        if let Some(path) = &self.options.report {
            let report = serde_json::json!({
                "frames_presented": self.summary.frames_presented,
                "trace_hash": format!("{:#018x}", self.summary.trace_hash),
                "final_state_hash": format!("{:#018x}", self.summary.final_state_hash),
                "frame_interval_median_ms": self.summary.frame_interval_median_ms,
                "moving_steps": self.summary.moving_steps,
                "session_id": self.summary.session_id,
                "presentation": "wayland-layer-shell",
            });
            match serde_json::to_string_pretty(&report)
                .map_err(|err| err.to_string())
                .and_then(|text| std::fs::write(path, text).map_err(|err| err.to_string()))
            {
                Ok(()) => {}
                Err(err) => eprintln!("torogao-companion: report write failed: {err}"),
            }
        }
    }

    fn write_screenshot(&mut self, path: &std::path::Path) {
        let (Ok(scene), Some(renderer)) = (self.core.scene_mut(), self.renderer.as_mut()) else {
            return;
        };
        match renderer.render(scene) {
            Ok((rgba, _report)) => match encode_png(&rgba) {
                Ok(png) => {
                    if let Err(err) = std::fs::write(path, png) {
                        eprintln!("torogao-companion: screenshot write failed: {err}");
                    }
                }
                Err(err) => eprintln!("torogao-companion: screenshot encode failed: {err}"),
            },
            Err(err) => eprintln!("torogao-companion: screenshot render failed: {err}"),
        }
    }

    fn persist_and_exit(&mut self) {
        self.persist_placement();
        if let Some(signal) = &self.loop_signal {
            signal.stop();
        }
    }
}

impl Presentation {
    fn on_press(&mut self, button: u32) {
        if self.menu.is_some() {
            if let Some(menu) = &mut self.menu {
                menu.push_pointer_button(button == BTN_LEFT, button == BTN_RIGHT, true);
            }
            return;
        }
        if self.placement.click_through_locked {
            return; // No events should arrive; a stray one changes nothing.
        }
        match button {
            BTN_LEFT => {
                self.drag = Some(DragState {
                    start_pos: self.pointer_pos,
                    start_margins: (self.placement.margin_side, self.placement.margin_edge),
                    moved: false,
                });
            }
            BTN_RIGHT => self.open_menu(),
            _ => {}
        }
    }

    fn on_motion(&mut self) {
        if let Some(menu) = &mut self.menu {
            menu.push_pointer_move(self.pointer_pos);
            return;
        }
        let Some(drag) = &mut self.drag else {
            return;
        };
        let delta = (
            self.pointer_pos.0 - drag.start_pos.0,
            self.pointer_pos.1 - drag.start_pos.1,
        );
        if delta.0.abs() > DRAG_THRESHOLD_PX || delta.1.abs() > DRAG_THRESHOLD_PX {
            drag.moved = true;
        }
        if drag.moved {
            if let Some((width, height)) = self.configured_size {
                let (side, edge) = drag_margins(
                    &self.placement,
                    drag.start_margins,
                    delta,
                    (width, height),
                    KEEP_VISIBLE_PX,
                );
                self.placement.margin_side = side;
                self.placement.margin_edge = edge;
                self.apply_layer_placement();
                self.layer().commit();
            }
        }
    }

    fn on_release(&mut self, button: u32) {
        if self.menu.is_some() {
            if let Some(menu) = &mut self.menu {
                menu.push_pointer_button(button == BTN_LEFT, button == BTN_RIGHT, false);
            }
            return;
        }
        if button != BTN_LEFT {
            return;
        }
        if let Some(drag) = self.drag.take() {
            if drag.moved {
                self.placement_dirty = true;
                self.persist_placement();
            } else if let Err(err) = self.core.play(CLICK_ANIMATION, false) {
                // A click on her: Greeting through the
                // bridge — the same entry the session host's
                // play_control tool lands in.
                eprintln!("torogao-companion: greeting refused: {err}");
            }
        }
    }

    fn open_menu(&mut self) {
        let (Some(renderer), Some(format)) = (self.renderer.as_ref(), self.surface_format) else {
            eprintln!("torogao-companion: menu unavailable before the first frame");
            return;
        };
        let mut menu = MenuUi::new(renderer.device(), format, self.pointer_pos);
        menu.click_through_locked = self.placement.click_through_locked;
        self.menu = Some(menu);
        self.apply_input_region();
    }

    fn apply_menu_actions(&mut self, actions: Vec<MenuAction>) {
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
                    self.placement.click_through_locked = !self.placement.click_through_locked;
                    if let Some(menu) = &mut self.menu {
                        menu.click_through_locked = self.placement.click_through_locked;
                    }
                    self.placement_dirty = true;
                    self.persist_placement();
                }
                MenuAction::Quit => {
                    self.exit = true;
                    self.menu = None;
                }
                MenuAction::Close => {
                    self.menu = None;
                }
            }
        }
        if self.menu.is_none() {
            self.apply_input_region();
        }
    }

    fn open_torogao(&self) {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|dir| dir.join("torogao-app")));
        let Some(exe) = exe else {
            eprintln!("torogao-companion: cannot locate the torogao-app sibling binary");
            return;
        };
        match std::process::Command::new(&exe)
            .arg(self.core.model_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => {}
            Err(err) => eprintln!(
                "torogao-companion: opening Torogao via {} failed: {err}",
                exe.display()
            ),
        }
    }
}

impl CompositorHandler for Presentation {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Presentation {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for Presentation {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let width = NonZeroU32::new(configure.new_size.0)
            .map(NonZeroU32::get)
            .unwrap_or(self.desired_size.0);
        let height = NonZeroU32::new(configure.new_size.1)
            .map(NonZeroU32::get)
            .unwrap_or(self.desired_size.1);
        self.configured_size = Some((width, height));
    }
}

impl SeatHandler for Presentation {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(err) => eprintln!("torogao-companion: pointer unavailable: {err}"),
            }
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(err) => eprintln!("torogao-companion: keyboard unavailable: {err}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Presentation {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::Escape {
            if self.menu.is_some() {
                self.menu = None;
                self.apply_input_region();
            } else {
                self.exit = true;
            }
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _event: KeyEvent,
    ) {
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _event: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
    }
}

impl PointerHandler for Presentation {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if let Some(layer) = &self.layer {
                if &event.surface != layer.wl_surface() {
                    continue;
                }
            }
            self.pointer_pos = event.position;
            match event.kind {
                PointerEventKind::Enter { .. } => {}
                PointerEventKind::Leave { .. } => {}
                PointerEventKind::Motion { .. } => self.on_motion(),
                PointerEventKind::Press { button, .. } => self.on_press(button),
                PointerEventKind::Release { button, .. } => self.on_release(button),
                PointerEventKind::Axis { .. } => {}
            }
        }
    }
}

delegate_compositor!(Presentation);
delegate_output!(Presentation);
delegate_seat!(Presentation);
delegate_keyboard!(Presentation);
delegate_pointer!(Presentation);
delegate_layer!(Presentation);
delegate_registry!(Presentation);

impl ProvidesRegistryState for Presentation {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

/// Runs the Wayland presentation until quit or the
/// `--frames` budget is spent. Typed refusals (never
/// panics) for the absent-compositor cases Gate G1 names:
/// no Wayland connection, no compositor global, no
/// layer-shell global, unknown pinned output.
pub fn run(options: RunOptions) -> CompanionResult<RunSummary> {
    let conn = Connection::connect_to_env().map_err(|err| {
        CompanionError::new(
            "TG-COMP-NO-WAYLAND",
            format!("no Wayland session is reachable: {err}"),
        )
    })?;
    let (globals, mut event_queue) = registry_queue_init(&conn).map_err(|err| {
        CompanionError::new(
            "TG-COMP-NO-WAYLAND",
            format!("Wayland registry init failed: {err}"),
        )
    })?;
    let qh = event_queue.handle();
    let compositor_state = CompositorState::bind(&globals, &qh).map_err(|err| {
        CompanionError::new(
            "TG-COMP-NO-COMPOSITOR",
            format!("wl_compositor is not advertised: {err}"),
        )
    })?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|err| {
        CompanionError::new(
            "TG-COMP-NO-LAYER-SHELL",
            format!(
                "zwlr_layer_shell_v1 is not advertised by this compositor; \
                 the companion requires a wlroots-class compositor (on \
                 X11/Windows use --fallback): {err}"
            ),
        )
    })?;
    let mut core = CompanionCore::load(
        &options.model,
        options.session_dir.as_deref(),
        options.session_port,
        &options.held_by,
    )?;
    let desired_size = {
        let scene = core.scene_mut()?;
        let prepared =
            prepare_frame(scene).map_err(|err| CompanionError::model(err.to_string()))?;
        surface_size_for_height(options.placement.display_height, &prepared.canvas)
    };
    let mut state = Presentation {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor_state,
        layer: None,
        core,
        placement: options.placement.clone(),
        desired_size,
        display_ptr: conn.backend().display_ptr() as usize,
        surface_ptr: 0,
        renderer: None,
        gpu_surface: None,
        surface_format: None,
        gpu_configured_size: None,
        configured_size: None,
        keyboard: None,
        pointer: None,
        pointer_pos: (0.0, 0.0),
        drag: None,
        menu: None,
        region: None,
        current_silhouette: None,
        last_silhouette_bbox: None,
        frames_since_silhouette: SILHOUETTE_REFRESH_FRAMES - 1,
        chain: crate::perf::chain_hash(&[]),
        summary: RunSummary::default(),
        frame_times: Vec::new(),
        last_hash: None,
        exit: false,
        placement_dirty: false,
        loop_signal: None,
        options,
    };
    // One dispatch so outputs are known before pinning.
    event_queue
        .blocking_dispatch(&mut state)
        .map_err(|err| CompanionError::new("TG-COMP-NO-WAYLAND", format!("dispatch: {err}")))?;
    let output = match &state.placement.output_name {
        Some(name) => {
            let found = state.output_state.outputs().find_map(|output| {
                state
                    .output_state
                    .info(&output)
                    .filter(|info| info.name.as_deref() == Some(name.as_str()))
                    .map(|_| output)
            });
            match found {
                Some(output) => Some(output),
                None => {
                    return Err(CompanionError::new(
                        "TG-COMP-OUTPUT-UNKNOWN",
                        format!(
                            "the persisted state pins output '{name}', which this \
                             session does not advertise; refusing to place her \
                             somewhere she was not put"
                        ),
                    ));
                }
            }
        }
        None => None,
    };
    let wl_surface = state.compositor_state.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        wl_surface,
        Layer::Overlay,
        Some("torogao-companion"),
        output.as_ref(),
    );
    layer.set_anchor(Presentation::layer_anchor(state.placement.anchor));
    layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
    layer.set_exclusive_zone(-1);
    layer.set_size(state.desired_size.0, state.desired_size.1);
    state.surface_ptr = layer.wl_surface().id().as_ptr() as usize;
    state.layer = Some(layer);
    state.apply_layer_placement();
    state.layer().commit();

    let mut event_loop: EventLoop<Presentation> = EventLoop::try_new()
        .map_err(|err| CompanionError::new("TG-COMP-EVENT-LOOP", format!("event loop: {err}")))?;
    let loop_handle = event_loop.handle();
    state.loop_signal = Some(event_loop.get_signal());
    loop_handle
        .insert_source(
            WaylandSource::new(conn.clone(), event_queue),
            // calloop-wayland-source 0.4 hands the queue to
            // the callback: dispatching pending events is
            // the callback's job. Returning without
            // dispatching leaves every compositor event
            // (configure included) stranded in the queue —
            // the loop idles and no frame is ever presented.
            |_, queue, state| queue.dispatch_pending(state),
        )
        .map_err(|err| {
            CompanionError::new("TG-COMP-EVENT-LOOP", format!("wayland source: {err}"))
        })?;
    loop_handle
        .insert_source(Timer::from_duration(TICK), |_deadline, _, state| {
            state.tick(&qh);
            if state.exit {
                state.persist_and_exit();
            }
            TimeoutAction::ToDuration(TICK)
        })
        .map_err(|err| CompanionError::new("TG-COMP-EVENT-LOOP", format!("timer: {err}")))?;
    event_loop
        .run(None, &mut state, |state| {
            if state.exit {
                state.persist_and_exit();
            }
        })
        .map_err(|err| CompanionError::new("TG-COMP-EVENT-LOOP", format!("run: {err}")))?;
    Ok(state.summary)
}
