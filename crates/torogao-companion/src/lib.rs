//! Torogao's desktop companion (Torogao 1.0, Stage 1.1):
//! a third presentation of the one session host — Persona
//! and the panel shell are the other two (PROGRAM.md
//! §3.4). The companion presents Prima performing her
//! landed repertoire (Daydream ambient, Greeting on click)
//! in a transparent, frameless, always-on-top window whose
//! pointer input region is her own alpha silhouette.
//!
//! Presentations:
//! - `wayland` — the primary route on primo's Hyprland: a
//!   `zwlr_layer_shell_v1` overlay surface via
//!   smithay-client-toolkit, anchor + margins for
//!   placement, `wl_surface.set_input_region` for the
//!   silhouette, keyboard interactivity on demand only.
//! - `fallback` — winit on X11/Windows: transparent,
//!   frameless, always-on-top, same silhouette hit
//!   discipline (XShape is a post-1.1 refinement; the
//!   fallback gates pointer actions on the silhouette
//!   in-process, named honestly in the gate report).
//!
//! The companion core (`core`) owns the session host's
//! live state and is shared by both presentations; there
//! is no companion-private scene or player. The companion
//! ships no audio code path at all (Gate G8).

pub mod core;
pub mod error;
pub mod fallback;
pub mod menu;
pub mod perf;
pub mod silhouette;
pub mod state;
pub mod view;
pub mod wayland;

pub use error::{CompanionError, CompanionResult};
