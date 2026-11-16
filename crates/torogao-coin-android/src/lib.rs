//! torogao-coin-android: the Lane D S6b spike shell.
//!
//! The smallest honest Android host for the coin systems:
//! a winit window under `android.app.NativeActivity`
//! (Route 1, Hands-On configuration), which on startup runs
//! the canonical coin fixture from `torogao_coin::trajectory`
//! exactly once, reports the outcome line to logcat, and
//! fills the window green on success / red on refusal. Since
//! the S5 binding it also runs the TF Jam skeleton sessions
//! through `torogao_coin::shell` and logs their combined
//! line (`bound_line`) beside the fixture line. No
//! wgpu, no egui, no text rendering — rendering is the
//! BRIEF-A1 commission's scope; a created window and an
//! event loop that survives pause/resume are this spike's
//! route evidence.
//!
//! Safety discipline: `#![deny(unsafe_code)]` holds for
//! everything this crate writes. The single exception point
//! is the `android_main` entry below: the symbol export the
//! android-activity runtime calls by name, written with the
//! edition-2024 `#[unsafe(no_mangle)]` attribute under one
//! item-level `#[allow(unsafe_code)]`. The crate originally
//! carried `forbid` here; the pinned toolchain
//! (nightly-2026-10-30) scopes `forbid(unsafe_code)` over
//! unsafe attributes themselves, which no item may
//! override, so the forbid + no_mangle shape cannot
//! compile for the Android target — `deny` plus the
//! item-level allow is the shape the estate's android
//! crates proved (fall, timestop, milkfactory). No unsafe
//! block, function, or impl exists in this crate.

#![deny(unsafe_code)]

use torogao_coin::run_measured_fixture;

/// The spike outcome line, in the exact shape the spike
/// report compares against the Linux `coin-smoke` output:
/// one line, hash first, the fixture's headline values
/// after.
pub fn spike_line() -> String {
    spike_report().0
}

/// The bound-crate line: the TF Jam skeleton sessions run
/// through `torogao_coin::shell` (the S5 shell binding) on
/// this host, summarised as one logcat line. The combined
/// hash is the FNV-1a digest of the sessions' state hashes,
/// in script order — the same values the Linux shell host
/// (`torogao-app --coin-sessions`) prints per session, so
/// the two platforms' bound runs compare directly.
pub fn bound_line() -> String {
    match torogao_coin::measured_dynamics().and_then(torogao_coin::shell::run_skeleton_sessions) {
        Ok(sessions) => {
            // FNV-1a 64 over the sessions' state hashes (the
            // estate's hash discipline; see torogao-coin's
            // trajectory module).
            let mut state: u64 = 0xcbf2_9ce4_8422_2325;
            for session in &sessions {
                for byte in session.state_hash.to_le_bytes() {
                    state ^= u64::from(byte);
                    state = state.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            format!(
                "coin-android-bound: sessions={} fixture_win={} timeout_win={} save={} hash={:016x}",
                sessions.len(),
                sessions[0].win.unwrap_or(false),
                sessions[1].win.unwrap_or(true),
                sessions[2].can_save,
                state
            )
        }
        Err(e) => format!("coin-android-bound: error={e}"),
    }
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
#[allow(unsafe_code)]
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
/// itself runs exactly once per process. Android-only: it
/// is constructed solely by `android_main`, so host builds
/// (which compile this crate for its tests) cfg it out
/// rather than carry it as dead code.
#[cfg(target_os = "android")]
#[derive(Default)]
struct SpikeApp {
    context: Option<softbuffer::Context<std::rc::Rc<winit::window::Window>>>,
    surface: Option<
        softbuffer::Surface<std::rc::Rc<winit::window::Window>, std::rc::Rc<winit::window::Window>>,
    >,
    window: Option<std::rc::Rc<winit::window::Window>>,
    success: bool,
    ran: bool,
}

#[cfg(target_os = "android")]
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
            log::info!("{}", bound_line());
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
                    let colour: u32 = if self.success {
                        0x0022_cc66
                    } else {
                        0x00cc_2222
                    };
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

    #[test]
    fn spike_line_hash_equals_the_linux_fixture_hash() {
        // The runnable host-side half of obligation
        // coin.trajectory-hash-cross-platform: the line this
        // crate logs on Android carries exactly the hash the
        // Linux coin-smoke fixture run produces, because both
        // execute torogao_coin::trajectory::run_measured_fixture.
        // The on-device half stays open (no emulator run).
        let run = torogao_coin::run_measured_fixture().expect("fixture runs");
        let expected = format!("hash={:016x}", run.trajectory_hash);
        let line = super::spike_line();
        assert!(line.contains(&expected), "{line} vs {expected}");
    }

    #[test]
    fn bound_line_names_the_skeleton_sessions() {
        // Host-side proof of the bound run the Android shell
        // logs on startup: the three skeleton sessions
        // through torogao-coin's shell binding.
        let line = super::bound_line();
        assert!(line.starts_with("coin-android-bound: sessions=3"), "{line}");
        assert!(line.contains("fixture_win=true"), "{line}");
        assert!(line.contains("timeout_win=false"), "{line}");
        assert!(line.contains("save=true"), "{line}");
    }
}
