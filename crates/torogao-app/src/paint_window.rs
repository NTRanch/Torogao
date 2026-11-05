// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! The paint editor's window shell: the stage Shell's
//! proven winit/egui-wgpu boilerplate (windowed.rs),
//! carrying a `PaintSession` instead of a scene. Same
//! renderer, same surface discipline, no eframe — the
//! pinned stack as D-970 ruled it.

use std::path::Path;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::error::{AppError, AppResult};
use crate::paint::PaintSession;
use crate::render::Renderer;

/// Runs the paint editor. Returns the process exit code.
/// `open_path`, when given, is a `.tpaint` container the
/// session opens at startup.
pub fn run_paint(open_path: Option<&Path>) -> AppResult<i32> {
    let event_loop = EventLoop::new()
        .map_err(|err| AppError::Cli(format!("event loop creation failed: {err}")))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = PaintShell::new(open_path);
    event_loop
        .run_app(&mut shell)
        .map_err(|err| AppError::Cli(format!("event loop failed: {err}")))?;
    Ok(0)
}

/// The editor's application state.
struct PaintShell {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    surface: Option<wgpu::Surface<'static>>,
    surface_format: wgpu::TextureFormat,
    egui_context: egui::Context,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    session: PaintSession,
    status: String,
}

impl PaintShell {
    fn new(open_path: Option<&Path>) -> PaintShell {
        PaintShell {
            window: None,
            renderer: None,
            surface: None,
            surface_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            egui_context: egui::Context::default(),
            egui_state: None,
            egui_renderer: None,
            session: PaintSession::new(open_path),
            status: String::new(),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Torogao Paint")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
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
                egui::Panel::top("paint-shell-status").show(ui, |ui| {
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
                    label: Some("torogao paint encoder"),
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
                    label: Some("torogao paint pass"),
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

impl ApplicationHandler for PaintShell {
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
