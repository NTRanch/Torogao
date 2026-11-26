//! Torogao Mobile — Slint phone shell.
//!
//! Thin consumer of `torogao-mobile-core`: the window collects a
//! cast pick, a scene prompt, and a seat URL; generation runs on a
//! worker thread through the core's seat client; results come back
//! through the core's gallery. The shell holds no prompt strings,
//! no checkpoint name, and no HTTP of its own.

// deny, not forbid: Slint's generated component code carries
// #[allow(unsafe_code)] on its vtable statics, which a crate-level
// forbid would overrule (E0453). Our own code holds the same line
// under deny.
#![deny(unsafe_code)]
// No crate-level missing_docs deny here either: the generated
// component API (include_modules!) is undocumented by construction.
// This crate's own public items are documented by hand; the core
// crate carries the deny for the shared surface.

use std::path::PathBuf;

use slint::{Image, ModelRc, VecModel};
use torogao_mobile_core::{Gallery, SeatClient, CAST};

slint::include_modules!();

/// Default seat URL on the phone: primo over the tailnet.
pub const SEAT_URL_ANDROID: &str = "http://100.107.128.71:8188";
/// Default seat URL on desktop: primo loopback.
pub const SEAT_URL_DESKTOP: &str = "http://127.0.0.1:8188";

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

fn show_entry(ui: &AppWindow, gallery: &Gallery, index: usize) {
    if let Ok(entries) = gallery.list() {
        if let Some(entry) = entries.iter().rev().nth(index) {
            let path = gallery.path_of(entry);
            if let Ok(img) = Image::load_from_path(&path) {
                ui.set_last_image(img);
                ui.set_has_image(true);
                ui.set_status(format!("Showing {}", entry.file).into());
            }
        }
    }
}

/// Run the shell. `data_dir` roots the gallery; `default_seat_url`
/// is the seat the UI starts pointed at (editable in the window).
pub fn run(data_dir: PathBuf, default_seat_url: &str) -> Result<(), slint::PlatformError> {
    let gallery_dir = data_dir.join("gallery");
    let ui = AppWindow::new()?;
    ui.set_seat_url(default_seat_url.into());
    ui.set_cast_names(ModelRc::new(VecModel::from(
        CAST.iter().map(|c| slint::SharedString::from(c.name)).collect::<Vec<_>>(),
    )));

    if let Ok(gallery) = Gallery::open(&gallery_dir) {
        ui.set_gallery_items(ModelRc::new(gallery_model(&gallery)));
        show_entry(&ui, &gallery, 0);
    }

    let ui_weak = ui.as_weak();
    let dir_for_generate = gallery_dir.clone();
    ui.on_generate(move |cast_index, prompt, seat_url| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let Some(character) = CAST.get(cast_index as usize) else {
            ui.set_status("Unknown cast member.".into());
            return;
        };
        ui.set_busy(true);
        ui.set_status(format!("Generating {}…", character.name).into());
        let weak = ui.as_weak();
        let dir = dir_for_generate.clone();
        let prompt = prompt.to_string();
        let seat_url = seat_url.to_string();
        std::thread::spawn(move || {
            let outcome = Gallery::open(&dir).and_then(|gallery| {
                let client = SeatClient::new(&seat_url);
                client
                    .generate(character, &prompt, None, &gallery)
                    .map(|entry| (gallery, entry))
            });
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                ui.set_busy(false);
                match outcome {
                    Ok((gallery, entry)) => {
                        ui.set_status(format!("Saved {}", entry.file).into());
                        ui.set_gallery_items(ModelRc::new(gallery_model(&gallery)));
                        show_entry(&ui, &gallery, 0);
                    }
                    Err(e) => {
                        ui.set_status(format!("{e}").into());
                    }
                }
            });
        });
    });

    let ui_weak = ui.as_weak();
    ui.on_gallery_picked(move |index| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if let Ok(gallery) = Gallery::open(&gallery_dir) {
            show_entry(&ui, &gallery, index as usize);
        }
    });

    ui.run()
}

/// Android entry point (NativeActivity). The activity hands us the
/// app's private data path, which roots the gallery.
///
/// The item-level allow is the crate's one unsafe surface: the FFI
/// entry point itself. In edition 2024 the `unsafe_code` lint also
/// covers the `#[unsafe(no_mangle)]` attribute that exports it.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    let data_dir = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/torogao-mobile"));
    slint::android::init(app).expect("slint android backend init");
    let _ = run(data_dir, SEAT_URL_ANDROID);
}

