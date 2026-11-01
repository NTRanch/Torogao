//! Torogao's Android shell (AMENDMENT-ANDROID track T5,
//! stage A-1).
//!
//! The shell is a *presentation* of the one session host,
//! exactly as the desktop windowed shell is (PROGRAM §3.4):
//! it owns no session state of its own. It hosts the
//! existing `torogao-session` host over the existing
//! `torogao-engine` evaluator, renders Prima's rig through
//! the app's renderer, and presents the Persona frame in
//! the portrait reflow of AMENDMENT-ANDROID §3.3 with one
//! area — Settings — operable end-to-end (BRIEF-A1 §Scope
//! 4). The other eleven areas render their frames and are
//! honestly not claimed operable at this stage.
//!
//! Platform seams only: the engines are untouched. The
//! seams are the storage root (app-private storage standing
//! in for the Addition S base directories), the winit
//! Android entry (`android_main`), and this crate's own
//! layout/settings/audit logic, all of which is
//! platform-neutral and host-tested.

// `deny`, not `forbid`, in this crate only: the Android
// entry contract requires `#[unsafe(no_mangle)]` on
// `android_main` (edition 2024 makes no_mangle an unsafe
// attribute), and `forbid` cannot be overridden at item
// level. The single sanctioned exception is the
// `android_main` export below, carrying its own
// item-level allow — the Hands-On crate's precedent,
// adopted verbatim in shape. Everywhere else the crate
// is unsafe-free, and the lint proves it.
#![deny(unsafe_code)]

pub mod audit_phone;
pub mod evidence;
pub mod fixture;
pub mod layout;
pub mod perf;
pub mod settings;
pub mod shell;
pub mod storage;

/// The Android entry point the NativeActivity glue calls:
/// hands the shell the platform application object and runs
/// its event loop. Desktop and host targets never compile
/// this; the cdylib exports exactly this one symbol.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    shell::android_main(app);
}

#[cfg(test)]
mod tests {
    /// The crate builds under its contracted name.
    #[test]
    fn workspace_member_builds_under_its_contracted_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-android");
    }
}
