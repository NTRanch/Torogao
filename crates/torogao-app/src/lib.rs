//! Torogao's graphical application: the live shell over the
//! evaluation engine.
//!
//! The shell loads an `.inx` project through the format
//! layer, builds the engine puppet, evaluates it, and draws
//! the evaluated parts with a wgpu renderer owned by this
//! crate. All deformation math stays in the engine; this
//! crate owns presentation state only — the loaded scene,
//! the node selection (a plain data structure the panels
//! read and a later session host can serve), and the GPU
//! resources.
//!
//! Two entry modes share every code path except the window:
//! the windowed shell (winit + egui panels + viewport) and
//! `--headless-eval`, which loads, evaluates, hashes, and
//! renders offscreen without creating a window.

#![forbid(unsafe_code)]

pub mod error;
pub mod font;
pub mod headless;
pub mod persona;
pub mod player;
pub mod render;
pub mod save;
pub mod scene;
pub mod session_host;
pub mod state;
pub mod state_hash;
pub mod textures;
pub mod tools;
pub mod video_bridge;
pub mod windowed;

pub use error::{AppError, AppResult};
pub use scene::Scene;
pub use state::{Selection, ShellState};
pub use state_hash::evaluated_state_hash;

#[cfg(test)]
mod tests {
    /// The crate builds under its contracted name.
    #[test]
    fn workspace_member_builds_under_its_contracted_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-app");
    }
}
