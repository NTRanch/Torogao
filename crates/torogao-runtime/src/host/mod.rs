//! Hosted-core implementations shipped with the runtime.
//!
//! A host in this module is a [`crate::GameCore`]
//! implementation, not a platform host crate: platform shape
//! (windowing, GPU, audio, OS) stays quarantined in the
//! per-platform host crates (plan SS1.3) and never appears
//! here.

pub mod author_game;
