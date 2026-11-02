//! torogao-vmc: the live VTuber transport of Torogao
//! (Stage 3.3) — VMC over OSC, in both directions.
//!
//! A tracker (or a peer performer) streams VMC frames;
//! the ARKit channels in each frame resolve through the
//! governed mapping (`arkit`) onto the same parameter
//! names the editor, the companion, and the session host
//! use; the tracking driver (`tracker`) applies one frame
//! per 60 fps tick and holds the last governed state when
//! the feed stops. Torogao also sends: its governed state
//! goes back out as VMC blend values under the governed
//! names, for OBS-side consumers.
//!
//! The crate is transport and mapping only — it owns no
//! scene and evaluates nothing. The app crate applies the
//! governed values to the puppet and records mimicry.

#![forbid(unsafe_code)]

pub mod arkit;
pub mod net;
pub mod osc;
pub mod tracker;
pub mod vmc;
