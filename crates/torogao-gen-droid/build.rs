//! Link wiring for the `native` backend.
//!
//! The seam links against a stable-diffusion.cpp build that the
//! builder provides out-of-tree (estate law: third-party native code
//! is a screened dependency with recorded provenance, never vendored
//! into this tree — see PROVENANCE.md). `SDCPP_LIB_DIR` names the
//! directory holding `libstable-diffusion.so`; when it is unset the
//! crate still builds (stub backend only) and the native backend
//! reports itself unavailable at run time.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=SDCPP_LIB_DIR");
    if env::var_os("CARGO_FEATURE_NATIVE").is_none() {
        return;
    }
    if let Some(dir) = env::var_os("SDCPP_LIB_DIR") {
        let dir = dir.to_string_lossy().into_owned();
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rustc-link-lib=dylib=stable-diffusion");
        // The shared library's own ggml dependencies sit beside it in
        // every build layout this lane produces (host build/bin and
        // the Android packaging both stage them together).
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }
}
