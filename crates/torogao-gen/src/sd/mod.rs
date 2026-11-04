//! Vendored stable-diffusion building blocks, adapted from
//! candle-transformers 0.11.1 (Hugging Face; Apache-2.0 OR MIT).
//! Vendored — rather than used through the candle-transformers
//! API — for exactly one reason, recorded in the D-867 Stage 1
//! proof report: candle 0.11.1's UNet2DConditionModel exposes no
//! SDXL added-conditioning input (pooled text embedding plus
//! size/crop time ids through the checkpoint's label embedding),
//! which ComfyUI and diffusers do apply. unet_2d here adds
//! forward_with_added, restoring that path; everything else is
//! upstream code, kept close so upstream fixes can be
//! re-vendored by diff.

/// Upstream attention blocks (vendored).
pub mod attention;
/// Upstream timestep embeddings (vendored).
pub mod embeddings;
/// Upstream resnet blocks (vendored).
pub mod resnet;
/// Upstream UNet plus the forward_with_added patch (vendored).
pub mod unet_2d;
/// Upstream UNet down/up blocks (vendored).
pub mod unet_2d_blocks;
/// Shim for candle's with_tracing helpers: upstream wraps
/// candle_nn constructors in tracing spans; the vendored blocks
/// only need the constructors themselves.
pub mod with_tracing;
