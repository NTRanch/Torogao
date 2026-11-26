//! Host binary for the Slint shell — off-device checks and the
//! desktop-side development seat. The phone build uses the cdylib
//! `android_main` entry in the library instead.

use std::path::PathBuf;

fn main() -> Result<(), slint::PlatformError> {
    let data_dir = std::env::var_os("TOROGAO_MOBILE_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".local/share/torogao-mobile")
        });
    torogao_mobile_slint::run(data_dir, torogao_mobile_slint::SEAT_URL_DESKTOP)
}
