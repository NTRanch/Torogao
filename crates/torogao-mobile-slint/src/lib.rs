//! Torogao Mobile — Slint phone shell.
//!
//! Thin consumer of `torogao-mobile-core`: the shell opens a
//! [`Session`] over the app's data directory and mirrors its
//! state into the window — attestation, seats, recipe, queue,
//! gallery — and renders the core helper state machine (Prima:
//! seat setup, Lana: generation, Sirena: gallery) in the helper
//! card. Every behaviour lives in the core exactly once; this
//! crate owns presentation and event forwarding only.
//!
//! A 1-second timer polls the queue: job transitions refresh
//! the gallery and feed the helper machine (a finished job is
//! Lana's success beat; a seat that stops answering mid-flow is
//! her comfort beat).

// deny, not forbid: Slint's generated component code carries
// #[allow(unsafe_code)] on its vtable statics, which a crate-level
// forbid would overrule (E0453). Our own code holds the same line
// under deny.
#![deny(unsafe_code)]
// No crate-level missing_docs deny here either: the generated
// component API (include_modules!) is undocumented by construction.
// This crate's own public items are documented by hand; the core
// crate carries the deny for the shared surface.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use slint::{Image, ModelRc, Timer, TimerMode, VecModel};
use torogao_mobile_core::helper::{
    FlowId, GateContext, Helper, HelperArea, HelperEvent, HelperState, ProposedAction,
};
use torogao_mobile_core::queue::JobStatus;
use torogao_mobile_core::{CAST, Gallery, RecipeSettings, SeatConfig, Session};

slint::include_modules!();

/// Default seat on the phone: primo's facade over the tailnet.
pub const SEAT_URL_ANDROID: &str = "http://100.107.128.71:8190";
/// Default seat name on the phone.
pub const SEAT_NAME_ANDROID: &str = "Primo (tailnet)";
/// Default seat on desktop: primo's facade on loopback.
pub const SEAT_URL_DESKTOP: &str = "http://127.0.0.1:8190";
/// Default seat name on desktop.
pub const SEAT_NAME_DESKTOP: &str = "Primo (this device)";

/// Shared shell state: the session (if it opened), the helper
/// machine, and the set of jobs already seen at a terminal
/// state (for transition detection in the poll loop).
struct Shell {
    session: Option<Arc<Session>>,
    helper: Mutex<Helper>,
    seen_terminal: Mutex<HashSet<u64>>,
    exports_dir: PathBuf,
    default_seat_url: String,
    default_seat_name: String,
}

impl Shell {
    fn gate(&self) -> GateContext {
        GateContext {
            attested: self.session.as_ref().is_some_and(|s| s.is_attested()),
        }
    }

    fn feed(&self, event: HelperEvent) {
        let gate = self.gate();
        if let Ok(mut helper) = self.helper.lock() {
            helper.on_event(event, &gate);
        }
    }

    fn helper_state(&self) -> Option<HelperState> {
        self.helper.lock().ok().map(|h| h.state().clone())
    }
}

fn gallery_model(gallery: &Gallery) -> VecModel<GalleryItem> {
    let mut items = Vec::new();
    if let Ok(entries) = gallery.list() {
        for e in entries.iter().rev() {
            let mut label = format!("{} · seed {}", e.character_id, e.seed);
            if !e.prompt.is_empty() {
                label.push_str(" · ");
                label.push_str(&e.prompt);
            }
            items.push(GalleryItem {
                label: label.into(),
                path: gallery.path_of(e).to_string_lossy().into_owned().into(),
            });
        }
    }
    VecModel::from(items)
}

fn entry_at(
    entries: &[torogao_mobile_core::GalleryEntry],
    index: usize,
) -> Option<&torogao_mobile_core::GalleryEntry> {
    entries.iter().rev().nth(index)
}

fn show_entry(ui: &AppWindow, gallery: &Gallery, index: usize) {
    if let Ok(entries) = gallery.list() {
        if let Some(entry) = entry_at(&entries, index) {
            let path = gallery.path_of(entry);
            if let Ok(img) = Image::load_from_path(&path) {
                ui.set_last_image(img);
                ui.set_has_image(true);
                ui.set_gallery_index(index as i32);
            }
        }
    }
}

fn refresh_settings(ui: &AppWindow, session: &Session) {
    let settings = session.settings();
    ui.set_attested(settings.is_attested());
    let seats: Vec<SeatItem> = settings
        .seats
        .iter()
        .enumerate()
        .map(|(i, s)| SeatItem {
            name: s.name.clone().into(),
            url: s.base_url.clone().into(),
            active: i == settings.active_seat,
        })
        .collect();
    ui.set_seats(ModelRc::new(VecModel::from(seats)));
    if let Some(active) = settings.active_seat() {
        ui.set_seat_name(active.name.clone().into());
        ui.set_seat_url(active.base_url.clone().into());
    }
    ui.set_recipe_steps(settings.recipe.steps as i32);
    ui.set_recipe_cfg(format!("{}", settings.recipe.cfg).into());
    ui.set_recipe_width(settings.recipe.width as i32);
    ui.set_recipe_height(settings.recipe.height as i32);
}

fn refresh_jobs(ui: &AppWindow, shell: &Shell) {
    let Some(session) = &shell.session else {
        return;
    };
    let jobs = session.jobs();
    // Transition detection: terminal jobs feed the helper and
    // the gallery exactly once.
    let mut gallery_dirty = false;
    for job in &jobs {
        let terminal = matches!(
            job.status,
            JobStatus::Done | JobStatus::Failed(_) | JobStatus::Cancelled
        );
        if !terminal {
            continue;
        }
        let fresh = shell
            .seen_terminal
            .lock()
            .map(|mut seen| seen.insert(job.id))
            .unwrap_or(false);
        if !fresh {
            continue;
        }
        if job.status == JobStatus::Done {
            gallery_dirty = true;
        }
        if let Some(HelperState::WaitingOnSeat {
            flow: FlowId::FirstGeneration,
        }) = shell.helper_state()
        {
            match job.status {
                JobStatus::Done => shell.feed(HelperEvent::JobSucceeded),
                JobStatus::Failed(_) => shell.feed(HelperEvent::JobFailed),
                _ => {}
            }
        }
    }
    if gallery_dirty {
        ui.set_gallery_items(ModelRc::new(gallery_model(session.gallery())));
        show_entry(ui, session.gallery(), 0);
    }
    let items: Vec<JobItem> = jobs
        .iter()
        .rev()
        .map(|job| {
            let who = CAST
                .iter()
                .find(|c| c.id == job.request.character_id)
                .map_or(job.request.character_id.as_str(), |c| c.name);
            let state = match &job.status {
                JobStatus::Queued => "queued".to_string(),
                JobStatus::Running => "generating…".to_string(),
                JobStatus::Done => "done".to_string(),
                JobStatus::Failed(m) => format!("failed: {m}"),
                JobStatus::Cancelled => "cancelled".to_string(),
            };
            let mut text = format!("#{} · {} · {}", job.id, who, state);
            if !job.request.prompt.is_empty() {
                text.push_str(" · ");
                text.push_str(&job.request.prompt);
            }
            JobItem {
                id: job.id as i32,
                text: text.into(),
                cancellable: matches!(job.status, JobStatus::Queued | JobStatus::Running),
            }
        })
        .collect();
    ui.set_jobs(ModelRc::new(VecModel::from(items)));
}

fn refresh_helper(ui: &AppWindow, shell: &Shell) {
    let Ok(helper) = shell.helper.lock() else {
        return;
    };
    ui.set_helper_name(helper.character().name.into());
    ui.set_helper_line(helper.line().into());
    ui.set_helper_area(match helper.area() {
        HelperArea::SeatSetup => 0,
        HelperArea::Generation => 1,
        HelperArea::Gallery => 2,
    });
    let (label, has) = match helper.proposed_action() {
        Some(ProposedAction::UseDefaultSeat) => ("Use the usual seat", true),
        Some(ProposedAction::TestSeatConnection) => ("Test the connection", true),
        Some(ProposedAction::SaveSeatAndFinish) => ("Save this seat", true),
        Some(ProposedAction::RunGeneration) => ("Cue it on the seat", true),
        Some(ProposedAction::OpenGallery) => ("Open the gallery", true),
        None => ("", false),
    };
    ui.set_helper_action(label.into());
    ui.set_helper_has_action(has);
}

/// Run the shell. `data_dir` roots settings + gallery;
/// `exports_dir` receives gallery exports; the default seat is
/// the first-run seat the settings start with.
pub fn run(
    data_dir: PathBuf,
    exports_dir: PathBuf,
    default_seat_url: &str,
    default_seat_name: &str,
) -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let (session, open_error) = match Session::open(&data_dir, default_seat_url, default_seat_name)
    {
        Ok(s) => (Some(Arc::new(s)), None),
        Err(e) => (None, Some(format!("Settings could not be loaded: {e}"))),
    };
    let shell = Arc::new(Shell {
        session,
        helper: Mutex::new(Helper::new()),
        seen_terminal: Mutex::new(HashSet::new()),
        exports_dir,
        default_seat_url: default_seat_url.to_string(),
        default_seat_name: default_seat_name.to_string(),
    });

    ui.set_cast_names(ModelRc::new(VecModel::from(
        CAST.iter()
            .map(|c| slint::SharedString::from(c.name))
            .collect::<Vec<_>>(),
    )));
    if let Some(error) = &open_error {
        ui.set_status(error.clone().into());
    }
    if let Some(session) = &shell.session {
        refresh_settings(&ui, session);
        ui.set_gallery_items(ModelRc::new(gallery_model(session.gallery())));
        show_entry(&ui, session.gallery(), 0);
        refresh_jobs(&ui, &shell);
    }
    refresh_helper(&ui, &shell);

    // ---- callbacks ----

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_attest(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(session) = &shell.session {
                match session.attest() {
                    Ok(()) => {
                        ui.set_status("Attestation saved. Welcome in.".into());
                        refresh_settings(&ui, session);
                    }
                    Err(e) => ui.set_status(format!("Could not save: {e}").into()),
                }
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_nav(move |screen| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_screen(screen);
            // The area's helper takes over — unless a flow owns
            // her right now.
            let busy = matches!(
                shell.helper_state(),
                Some(HelperState::Guiding { .. }) | Some(HelperState::WaitingOnSeat { .. })
            );
            if !busy {
                let area = match screen {
                    0 => HelperArea::Generation,
                    1 => HelperArea::Gallery,
                    _ => HelperArea::SeatSetup,
                };
                shell.feed(HelperEvent::EnterArea(area));
                refresh_helper(&ui, &shell);
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_generate(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(session) = &shell.session else {
                return;
            };
            let Some(character) = CAST.get(ui.get_cast_index() as usize) else {
                ui.set_status("Unknown cast member.".into());
                return;
            };
            let prompt = ui.get_prompt().to_string();
            match session.enqueue(character.id, &prompt, None) {
                Ok(id) => {
                    ui.set_status(format!("Queued job #{id} on the seat.").into());
                    refresh_jobs(&ui, &shell);
                }
                Err(e) => ui.set_status(format!("{e}").into()),
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_cancel_job(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(session) = &shell.session {
                if session.cancel(id as u64) {
                    ui.set_status(format!("Cancelling job #{id}…").into());
                }
                refresh_jobs(&ui, &shell);
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_gallery_picked(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(session) = &shell.session {
                show_entry(&ui, session.gallery(), index as usize);
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_gallery_delete(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(session) = &shell.session else {
                return;
            };
            let index = ui.get_gallery_index();
            if index < 0 {
                ui.set_status("Pick a still first.".into());
                return;
            }
            let gallery = session.gallery();
            match gallery.list() {
                Ok(entries) => match entry_at(&entries, index as usize) {
                    Some(entry) => match gallery.delete(entry) {
                        Ok(()) => {
                            ui.set_status(format!("Deleted {}", entry.file).into());
                            ui.set_gallery_items(ModelRc::new(gallery_model(gallery)));
                            ui.set_has_image(false);
                            ui.set_gallery_index(-1);
                        }
                        Err(e) => ui.set_status(format!("{e}").into()),
                    },
                    None => ui.set_status("That still is already gone.".into()),
                },
                Err(e) => ui.set_status(format!("{e}").into()),
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_gallery_export(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(session) = &shell.session else {
                return;
            };
            let index = ui.get_gallery_index();
            if index < 0 {
                ui.set_status("Pick a still first.".into());
                return;
            }
            let gallery = session.gallery();
            match gallery.list() {
                Ok(entries) => match entry_at(&entries, index as usize) {
                    Some(entry) => match gallery.export(entry, &shell.exports_dir) {
                        Ok(dest) => {
                            ui.set_status(format!("Exported to {}", dest.to_string_lossy()).into())
                        }
                        Err(e) => ui.set_status(format!("{e}").into()),
                    },
                    None => ui.set_status("That still is already gone.".into()),
                },
                Err(e) => ui.set_status(format!("{e}").into()),
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_save_seat(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(session) = &shell.session else {
                return;
            };
            let seat = SeatConfig {
                name: ui.get_seat_name().to_string(),
                base_url: ui.get_seat_url().to_string(),
            };
            if seat.name.trim().is_empty() || seat.base_url.trim().is_empty() {
                ui.set_status("A seat needs a name and an address.".into());
                return;
            }
            match session.upsert_seat(seat) {
                Ok(i) => {
                    let _ = session.set_active_seat(i);
                    ui.set_status("Seat saved and active.".into());
                    refresh_settings(&ui, session);
                }
                Err(e) => ui.set_status(format!("{e}").into()),
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_activate_seat(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(session) = &shell.session {
                match session.set_active_seat(index as usize) {
                    Ok(()) => {
                        ui.set_status("Seat switched.".into());
                        refresh_settings(&ui, session);
                    }
                    Err(e) => ui.set_status(format!("{e}").into()),
                }
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_test_seat(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(session) = &shell.session else {
                return;
            };
            ui.set_seat_status("Testing the seat…".into());
            let session = Arc::clone(session);
            let weak2 = weak.clone();
            std::thread::spawn(move || {
                let status = session.seat_status();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak2.upgrade() {
                        ui.set_seat_status(status.describe().into());
                    }
                });
            });
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_wake_seat(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(session) = &shell.session else {
                return;
            };
            ui.set_seat_status("Waking the seat…".into());
            let session = Arc::clone(session);
            let weak2 = weak.clone();
            std::thread::spawn(move || {
                let status = session.wake_seat();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak2.upgrade() {
                        ui.set_seat_status(status.describe().into());
                    }
                });
            });
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_apply_recipe(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(session) = &shell.session else {
                return;
            };
            let cfg: f32 = match ui.get_recipe_cfg().parse() {
                Ok(v) => v,
                Err(_) => {
                    ui.set_status("CFG must be a number.".into());
                    return;
                }
            };
            let recipe = RecipeSettings {
                steps: ui.get_recipe_steps() as u32,
                cfg,
                width: ui.get_recipe_width() as u32,
                height: ui.get_recipe_height() as u32,
            };
            match session.set_recipe(recipe) {
                Ok(()) => ui.set_status("Recipe saved.".into()),
                Err(e) => ui.set_status(format!("{e}").into()),
            }
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_helper_begin(move |flow| {
            let Some(ui) = weak.upgrade() else { return };
            match flow {
                0 => {
                    ui.set_screen(2);
                    shell.feed(HelperEvent::BeginFlow(FlowId::SeatSetup));
                }
                _ => {
                    ui.set_screen(0);
                    shell.feed(HelperEvent::BeginFlow(FlowId::FirstGeneration));
                }
            }
            refresh_helper(&ui, &shell);
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_helper_dismiss(move || {
            let Some(ui) = weak.upgrade() else { return };
            // "Not now" during a flow cancels it; on a success or
            // comfort beat it dismisses. Both are the same event
            // pair here: Cancel covers flows, Dismiss the beats.
            match shell.helper_state() {
                Some(HelperState::Guiding { .. }) | Some(HelperState::WaitingOnSeat { .. }) => {
                    shell.feed(HelperEvent::Cancel);
                }
                _ => shell.feed(HelperEvent::Dismiss),
            }
            refresh_helper(&ui, &shell);
        });
    }

    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        ui.on_helper_confirm(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(session) = &shell.session else { return };
            let action = shell
                .helper
                .lock()
                .ok()
                .and_then(|h| h.proposed_action());
            match action {
                Some(ProposedAction::UseDefaultSeat) => {
                    ui.set_seat_name(shell.default_seat_name.clone().into());
                    ui.set_seat_url(shell.default_seat_url.clone().into());
                    shell.feed(HelperEvent::ConfirmStep);
                }
                Some(ProposedAction::TestSeatConnection) => {
                    shell.feed(HelperEvent::ConfirmStep);
                    ui.set_seat_status("Testing the seat…".into());
                    let session = Arc::clone(session);
                    let shell2 = Arc::clone(&shell);
                    let weak2 = weak.clone();
                    std::thread::spawn(move || {
                        // Wake first: a sleeping seat is not a
                        // failed test. Then poll for an answer.
                        let _ = session.wake_seat();
                        let mut answered = false;
                        for _ in 0..10 {
                            let status = session.seat_status();
                            if !matches!(
                                status,
                                torogao_mobile_core::SeatStatus::Unreachable(_)
                            ) {
                                answered = true;
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_secs(3));
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            let Some(ui) = weak2.upgrade() else { return };
                            if answered {
                                shell2.feed(HelperEvent::SeatTestPassed);
                                ui.set_seat_status(
                                    "The seat answered. You can save it now.".into(),
                                );
                            } else {
                                shell2.feed(HelperEvent::SeatLost);
                                ui.set_seat_status(
                                    "No answer from the seat. Check the address and that the facade is running.".into(),
                                );
                            }
                            refresh_helper(&ui, &shell2);
                        });
                    });
                }
                Some(ProposedAction::SaveSeatAndFinish) => {
                    let seat = SeatConfig {
                        name: ui.get_seat_name().to_string(),
                        base_url: ui.get_seat_url().to_string(),
                    };
                    match session.upsert_seat(seat) {
                        Ok(i) => {
                            let _ = session.set_active_seat(i);
                            refresh_settings(&ui, session);
                            shell.feed(HelperEvent::ConfirmStep);
                        }
                        Err(e) => ui.set_status(format!("{e}").into()),
                    }
                }
                Some(ProposedAction::RunGeneration) => {
                    let Some(character) = CAST.get(ui.get_cast_index() as usize) else {
                        ui.set_status("Unknown cast member.".into());
                        return;
                    };
                    let prompt = ui.get_prompt().to_string();
                    match session.enqueue(character.id, &prompt, None) {
                        Ok(id) => {
                            ui.set_status(format!("Queued job #{id} on the seat.").into());
                            shell.feed(HelperEvent::ConfirmStep);
                            refresh_jobs(&ui, &shell);
                        }
                        Err(torogao_mobile_core::GenError::AgeGate) => {
                            shell.feed(HelperEvent::AttestationMissing);
                        }
                        Err(e) => ui.set_status(format!("{e}").into()),
                    }
                }
                Some(ProposedAction::OpenGallery) => {
                    ui.set_screen(1);
                    shell.feed(HelperEvent::Dismiss);
                    shell.feed(HelperEvent::EnterArea(HelperArea::Gallery));
                }
                None => {}
            }
            refresh_helper(&ui, &shell);
        });
    }

    // ---- poll loop: queue transitions + seat status ----
    let timer = Timer::default();
    {
        let shell = Arc::clone(&shell);
        let weak = ui.as_weak();
        let mut tick: u64 = 0;
        timer.start(
            TimerMode::Repeated,
            std::time::Duration::from_secs(1),
            move || {
                tick += 1;
                let Some(ui) = weak.upgrade() else { return };
                refresh_jobs(&ui, &shell);
                refresh_helper(&ui, &shell);
                if tick % 5 == 0 {
                    if let Some(session) = &shell.session {
                        let session = Arc::clone(session);
                        let shell2 = Arc::clone(&shell);
                        let weak2 = weak.clone();
                        std::thread::spawn(move || {
                            let status = session.seat_status();
                            let _ = slint::invoke_from_event_loop(move || {
                                let Some(ui) = weak2.upgrade() else { return };
                                ui.set_seat_status(status.describe().into());
                                if matches!(status, torogao_mobile_core::SeatStatus::Unreachable(_))
                                    && matches!(
                                        shell2.helper_state(),
                                        Some(HelperState::WaitingOnSeat { .. })
                                    )
                                {
                                    shell2.feed(HelperEvent::SeatLost);
                                    refresh_helper(&ui, &shell2);
                                }
                            });
                        });
                    }
                }
            },
        );
    }

    let result = ui.run();
    drop(timer);
    result
}

/// Android entry point (NativeActivity). The activity hands us
/// the app's private data path, which roots settings + gallery;
/// exports go to the app-specific external directory when the
/// system offers one (visible over USB, no extra permission).
///
/// The item-level allow is the crate's one unsafe surface: the
/// FFI entry point itself. In edition 2024 the `unsafe_code` lint
/// also covers the `#[unsafe(no_mangle)]` attribute that exports it.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    let data_dir = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/torogao-mobile"));
    let exports_dir = app
        .external_data_path()
        .map(|p| p.join("exports"))
        .unwrap_or_else(|| data_dir.join("exports"));
    slint::android::init(app).expect("slint android backend init");
    let _ = run(data_dir, exports_dir, SEAT_URL_ANDROID, SEAT_NAME_ANDROID);
}
