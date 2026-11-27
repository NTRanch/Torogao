//! torogao-gen-droid — the on-device generation engine for Torogao
//! on Android.
//!
//! Torogao Mobile's core speaks a bounded contract to its generation
//! seat (submit → poll → fetch, a prompt byte cap, and a typed
//! "seat unreachable" status). The primo seat is one engine behind
//! that contract; this crate is the second: the same semantics,
//! in-process, on the phone, backed by stable-diffusion.cpp's ggml
//! Vulkan pipeline running the estate's generator of record (wai,
//! D-1525) in GGUF-quantized form. Which engine serves a request is
//! the caller's routing decision; the contract does not change.
//!
//! The native library is reached through a small hand-written FFI
//! surface (see [`sys`]); everything above that surface is safe
//! Rust. Without the `native` feature only the stub backend builds,
//! which is what the unit tests drive.

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod engine;
#[cfg(feature = "native")]
#[allow(unsafe_code)]
pub mod native;
pub mod params;
pub mod stub;
#[cfg(feature = "native")]
#[allow(unsafe_code)]
pub mod sys;

pub use engine::{Backend, Engine, JobId, JobState};
pub use params::{DroidError, GenParams, GeneratedImage, MAX_PROMPT_BYTES, RawImage};
