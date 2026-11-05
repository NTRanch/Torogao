// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! `torogao-paint`: the deterministic raster document
//! core of the torogao paint surface (SPEC Phase 1,
//! D-991). Everything here is CPU integer math over a
//! sparse tile grid; the op log is the document's source
//! of truth, and replaying it re-derives every pixel
//! bit-identically on any machine.
//!
//! The surface crates (editor, verbs) consume this core;
//! nothing here touches a GPU, a window, or a clock.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod color;
pub mod document;
pub mod error;
pub mod format;
pub mod hash;
pub mod layer;
pub mod ops;
pub mod palette;
pub mod png_io;
pub mod script;
pub mod stroke;
pub mod tile;

pub use document::{BaseRaster, Document, SourceRef};
pub use error::{PaintError, PaintResult};
pub use format::{FORMAT_MAJOR, FORMAT_MINOR, FORMAT_TAG, load_document, save_document};
pub use layer::{BlendMode, LayerId};
pub use ops::{LayerPropsChange, Op, Rect};
pub use palette::Palette;
pub use script::SessionScript;
pub use stroke::{Stamp, StampMode, StampStyle, StrokeParams, StrokeSample, build_stamp_run};
pub use tile::{TILE_BYTES, TILE_PX};
