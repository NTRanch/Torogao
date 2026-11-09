//! torogao-coin-android: the Lane D S6b spike shell.
//!
//! The smallest honest Android host for the coin systems:
//! a winit window under `android.app.NativeActivity`
//! (Route 1, Hands-On configuration), which on startup runs
//! the canonical coin fixture from `torogao_coin::trajectory`
//! exactly once, reports the outcome line to logcat, and
//! fills the window green on success / red on refusal. No
//! wgpu, no egui, no text rendering — rendering is the
//! BRIEF-A1 commission's scope; a created window and an
//! event loop that survives pause/resume are this spike's
//! route evidence.
//!
//! Safety discipline: `#![forbid(unsafe_code)]` holds for
//! everything this crate writes. The single exception point
//! is the `android_main` entry below: the symbol export the
//! android-activity runtime calls by name, written with the
//! edition-2024 `#[unsafe(no_mangle)]` attribute. No unsafe
//! block, function, or impl exists in this crate.

#![forbid(unsafe_code)]

use torogao_coin::run_measured_fixture;

/// The spike outcome line, in the exact shape the spike
/// report compares against the Linux `coin-smoke` output:
/// one line, hash first, the fixture's headline values
/// after.
pub fn spike_line() -> String {
    spike_report().0
}

/// Run the fixture once and render the report: the logcat
/// line and whether the run succeeded (the window fill
/// colour's input). A refused fixture is a red window and
/// an `error=` line — reported, never hidden.
fn spike_report() -> (String, bool) {
    match run_measured_fixture() {
        Ok(run) => {
            let outcome = run.outcome;
            (
                format!(
                    "coin-android-smoke: hash={:016x} success={} gauge={:.4} points={:.4} level={} clock={}",
                    run.trajectory_hash,
                    outcome.success,
                    outcome.gauge,
                    outcome.points,
                    outcome.level_after,
                    run.clock_minute
                ),
                outcome.success,
            )
        }
        Err(e) => (format!("coin-android-smoke: error={e}"), false),
    }
}

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

/// The Android entry point. The android-activity runtime
/// calls this symbol by name; it is the crate's single
/// `#[unsafe(no_mangle)]` point (see the crate docs).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("torogao-coin-android"),
    );
    let event_loop = winit::event_loop::EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("spike event loop builds");
    let mut spike = SpikeApp::default();
    if event_loop.run_app(&mut spike).is_err() {
        log::error!("coin-android-smoke: event loop exited with an error");
    }
}

/// The spike's winit application: one window, one fixture
/// run, one fill colour. On suspend the window and surface
/// are dropped (the native window is gone); on resume they
/// are recreated and the fill re-presented — the fixture
/// itself runs exactly once per process.
#[derive(Default)]
struct SpikeApp {
    context: Option<softbuffer::Context<std::rc::Rc<winit::window::Window>>>,
    surface: Option<
        softbuffer::Surface<
            std::rc::Rc<winit::window::Window>,
            std::rc::Rc<winit::window::Window>,
        >,
    >,
    window: Option<std::rc::Rc<winit::window::Window>>,
    success: bool,
    ran: bool,
}

impl winit::application::ApplicationHandler for SpikeApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = std::rc::Rc::new(
            event_loop
                .create_window(winit::window::Window::default_attributes())
                .expect("spike window creates"),
        );
        if !self.ran {
            self.ran = true;
            let (line, success) = spike_report();
            self.success = success;
            log::info!("{line}");
        }
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        self.context = Some(context);
        self.surface = Some(surface);
        window.request_redraw();
        self.window = Some(window);
    }

    fn suspended(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        self.surface = None;
        self.context = None;
        self.window = None;
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::CloseRequested => event_loop.exit(),
            winit::event::WindowEvent::RedrawRequested => {
                let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else {
                    return;
                };
                let size = window.inner_size();
                let (Some(width), Some(height)) = (
                    std::num::NonZeroU32::new(size.width),
                    std::num::NonZeroU32::new(size.height),
                ) else {
                    return;
                };
                if surface.resize(width, height).is_err() {
                    return;
                }
                if let Ok(mut buffer) = surface.buffer_mut() {
                    // softbuffer's pixel word is 0x00RRGGBB.
                    let colour: u32 = if self.success { 0x0022_cc66 } else { 0x00cc_2222 };
                    buffer.fill(colour);
                    let _ = buffer.present();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn spike_line_names_the_fixture_result() {
        // Host-side proof of the line the Android run will
        // emit: same fixture, same format, recorded values.
        let line = super::spike_line();
        assert!(line.starts_with("coin-android-smoke: hash="), "{line}");
        assert!(line.contains("success=true"), "{line}");
        assert!(line.contains("gauge=219.5479"), "{line}");
        assert!(line.contains("points=172.5000"), "{line}");
        assert!(line.contains("level=1"), "{line}");
        assert!(line.contains("clock=595"), "{line}");
    }
}
