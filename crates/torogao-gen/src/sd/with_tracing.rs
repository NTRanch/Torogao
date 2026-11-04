//! Shim for candle's with_tracing helpers: upstream wraps
//! candle_nn constructors in tracing spans; the vendored blocks
//! only need the constructors themselves.

pub use candle_nn::{Conv2d, conv2d};
