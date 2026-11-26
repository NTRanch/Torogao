//! torogao-mobile-core — the shell-agnostic core of Torogao Mobile.
//!
//! Owns everything both shells share, exactly once:
//! - [`cast`]: the canonical cast table (Prima, Lana, Sirena), each
//!   record citing the lock/style sheet it was taken from.
//! - [`style`]: the canonical style record — the wai checkpoint and
//!   the estate's proven stills recipe (D-1525: wai is the single
//!   generator; this crate is a *client* of the seat that runs it).
//! - [`workflow`]: the pure builder from a cast member + prompt to a
//!   ComfyUI workflow document.
//! - [`client`]: the blocking seat client (submit, poll, fetch).
//! - [`gallery`]: the local gallery — generated images plus a JSON
//!   index, owned by whichever data directory the shell hands in.
//!
//! The core has no UI assumptions and imports no shell crate; the
//! Slint phone shell and the GPUI desktop shell are thin consumers.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod cast;
pub mod client;
pub mod gallery;
pub mod style;
pub mod workflow;

pub use cast::{character, Character, CAST};
pub use client::{GenError, SeatClient};
pub use gallery::{Gallery, GalleryEntry};
pub use workflow::{build_workflow, compose_negative, compose_positive};
