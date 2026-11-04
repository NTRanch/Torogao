//! CLIP ViT-H/14 vision encoder (Stage 2, D-936): turns a
//! reference image into the token sequence the IP-Adapter
//! resampler consumes.
//!
//! Clean-room from the weights file's measured layout
//! (HF-format CLIPVisionModel, every tensor nested under
//! `vision_model`: embeddings.patch_embedding,
//! class_embedding, position_embedding, the file's own
//! `pre_layrnorm` spelling, 32 encoder layers with
//! separate q/k/v/out projections, post_layernorm) and
//! from the oracle's measured behaviour: the IP-Adapter
//! consumes the **penultimate** hidden states (the output
//! of encoder layer 30 of 32, pre-post-norm), the same
//! convention the SDXL text path uses for its context.
//!
//! Preprocessing is the CLIP convention: resize to
//! 224x224, normalise with the CLIP channel means/stds.
//! The resize filter (bilinear) is a named calibration
//! point: the oracle's resizer is bicubic-class, and any
//! residual difference lands inside the parity metric,
//! which is reported raw (D-713), not thresholded away.

use std::path::Path;

use candle::{DType, Device, Tensor};
use candle_nn::{self as nn, Module, VarBuilder};

use crate::error::{GenError, Result};

/// Vision input edge length.
pub const IMAGE_SIZE: u32 = 224;
/// Patch edge length.
const PATCH: usize = 14;
/// Token sequence length: 16x16 patches + class token.
const SEQ_LEN: usize = 257;
/// Embedding width.
const EMBED_DIM: usize = 1280;
/// Encoder layer count in the file.
const LAYERS: usize = 32;
/// Index of the penultimate encoder layer (0-based).
const PENULTIMATE: usize = LAYERS - 2;
/// Attention heads (1280 / 80).
const HEADS: usize = 16;
/// Per-head width.
const HEAD_DIM: usize = EMBED_DIM / HEADS;
/// CLIP channel means (RGB).
const MEAN: [f64; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
/// CLIP channel standard deviations (RGB).
const STD: [f64; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_11];

struct VitBlock {
    layer_norm1: nn::LayerNorm,
    q_proj: nn::Linear,
    k_proj: nn::Linear,
    v_proj: nn::Linear,
    out_proj: nn::Linear,
    layer_norm2: nn::LayerNorm,
    fc1: nn::Linear,
    fc2: nn::Linear,
}

impl VitBlock {
    fn new(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            layer_norm1: nn::layer_norm(EMBED_DIM, 1e-5, vb.pp("layer_norm1"))?,
            q_proj: nn::linear(EMBED_DIM, EMBED_DIM, vb.pp("self_attn.q_proj"))?,
            k_proj: nn::linear(EMBED_DIM, EMBED_DIM, vb.pp("self_attn.k_proj"))?,
            v_proj: nn::linear(EMBED_DIM, EMBED_DIM, vb.pp("self_attn.v_proj"))?,
            out_proj: nn::linear(EMBED_DIM, EMBED_DIM, vb.pp("self_attn.out_proj"))?,
            layer_norm2: nn::layer_norm(EMBED_DIM, 1e-5, vb.pp("layer_norm2"))?,
            fc1: nn::linear(EMBED_DIM, EMBED_DIM * 4, vb.pp("mlp.fc1"))?,
            fc2: nn::linear(EMBED_DIM * 4, EMBED_DIM, vb.pp("mlp.fc2"))?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        // Self-attention, scores in f32 (the vendored UNet
        // attention's own precision convention).
        let h = self.layer_norm1.forward(xs)?;
        let (b, t, _) = h.dims3()?;
        let split = |t3: Tensor| -> Result<Tensor> {
            Ok(t3
                .reshape((b, t, HEADS, HEAD_DIM))?
                .transpose(1, 2)?
                .to_dtype(DType::F32)?)
        };
        let q = split(self.q_proj.forward(&h)?)?;
        let k = split(self.k_proj.forward(&h)?)?;
        let v = split(self.v_proj.forward(&h)?)?;
        let scale = 1.0 / f64::sqrt(HEAD_DIM as f64);
        let scores = (q.matmul(&k.transpose(2, 3)?)? * scale)?;
        let attn = nn::ops::softmax(&scores, candle::D::Minus1)?;
        let out = attn
            .matmul(&v)?
            .transpose(1, 2)?
            .reshape((b, t, EMBED_DIM))?
            .to_dtype(xs.dtype())?;
        let xs = (xs + self.out_proj.forward(&out)?)?;
        let h = self.layer_norm2.forward(&xs)?;
        let h = self.fc1.forward(&h)?.gelu()?;
        let h = self.fc2.forward(&h)?;
        Ok((xs + h)?)
    }
}

/// The loaded ViT-H encoder.
pub struct ClipVisionH {
    patch_embed: nn::Conv2d,
    class_embedding: Tensor,
    position_embedding: Tensor,
    pre_ln: nn::LayerNorm,
    layers: Vec<VitBlock>,
    device: Device,
}

impl ClipVisionH {
    /// Load from a CLIP-ViT-H safetensors file (f16, on the
    /// given device).
    ///
    /// # Errors
    /// Returns `gen_conditioning` when the file is missing
    /// (a conditioning input, refused before GPU work) and
    /// `gen_weights` when a tensor is absent or misshapen.
    pub fn load(path: &Path, device: &Device) -> Result<Self> {
        if !path.exists() {
            return Err(GenError::Conditioning {
                detail: format!("clip vision weights {} do not exist", path.display()),
            });
        }
        // SAFETY: read-only mapping of a weights file this
        // process owns for the run's duration; never mutated.
        #[allow(unsafe_code)]
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[path], DType::F16, device) }
            .map_err(|e| GenError::Weights {
                detail: format!("{}: {e}", path.display()),
            })?;
        // The file nests every tensor under `vision_model`
        // (embeddings / encoder.layers / pre_layrnorm).
        let vb = vb.pp("vision_model");
        let patch_cfg = nn::Conv2dConfig {
            stride: PATCH,
            ..Default::default()
        };
        // HF patch embedding is a bias-free convolution;
        // the file carries only its weight.
        let patch_weight = vb.get(
            (EMBED_DIM, 3, PATCH, PATCH),
            "embeddings.patch_embedding.weight",
        )?;
        let patch_embed = nn::Conv2d::new(patch_weight, None, patch_cfg);
        let class_embedding = vb.get(EMBED_DIM, "embeddings.class_embedding")?;
        let position_embedding =
            vb.get((SEQ_LEN, EMBED_DIM), "embeddings.position_embedding.weight")?;
        let pre_ln = nn::layer_norm(EMBED_DIM, 1e-5, vb.pp("pre_layrnorm"))?;
        let mut layers = Vec::with_capacity(LAYERS);
        for i in 0..LAYERS {
            layers.push(VitBlock::new(vb.pp(format!("encoder.layers.{i}")))?);
        }
        Ok(Self {
            patch_embed,
            class_embedding,
            position_embedding,
            pre_ln,
            layers,
            device: device.clone(),
        })
    }

    /// Preprocess a reference image to the encoder input:
    /// `(1, 3, 224, 224)` f16 on the load device, CLIP
    /// normalisation applied.
    ///
    /// # Errors
    /// Returns `gen_conditioning` when the reference is
    /// missing or undecodable.
    pub fn preprocess(&self, reference: &Path) -> Result<Tensor> {
        if !reference.exists() {
            return Err(GenError::Conditioning {
                detail: format!("reference image {} does not exist", reference.display()),
            });
        }
        let img = image::open(reference).map_err(|e| GenError::Conditioning {
            detail: format!("cannot decode reference {}: {e}", reference.display()),
        })?;
        let img = image::imageops::resize(
            &img.to_rgb8(),
            IMAGE_SIZE,
            IMAGE_SIZE,
            image::imageops::FilterType::Triangle,
        );
        let raw = img.into_raw();
        let side = (IMAGE_SIZE * IMAGE_SIZE) as usize;
        let mut chw = vec![0f32; 3 * side];
        for (i, px) in raw.chunks_exact(3).enumerate() {
            for c in 0..3 {
                chw[c * side + i] = ((f64::from(px[c]) / 255.0 - MEAN[c]) / STD[c]) as f32;
            }
        }
        let t = Tensor::new(chw, &Device::Cpu)?.reshape((
            1,
            3,
            IMAGE_SIZE as usize,
            IMAGE_SIZE as usize,
        ))?;
        Ok(t.to_device(&self.device)?.to_dtype(DType::F16)?)
    }

    /// Run the encoder; returns the penultimate hidden
    /// states `(1, 257, 1280)` in f32 — the token sequence
    /// the IP-Adapter resampler consumes.
    ///
    /// # Errors
    /// Returns tensor errors typed by the crate enum.
    pub fn penultimate(&self, pixels: &Tensor) -> Result<Tensor> {
        let patches = self
            .patch_embed
            .forward(pixels)?
            .reshape((1, EMBED_DIM, (IMAGE_SIZE as usize / PATCH).pow(2)))?
            .transpose(1, 2)?;
        let cls = self
            .class_embedding
            .reshape((1, 1, EMBED_DIM))?
            .broadcast_as((1, 1, EMBED_DIM))?;
        let mut xs = Tensor::cat(&[&cls, &patches], 1)?;
        xs = xs.broadcast_add(&self.position_embedding.unsqueeze(0)?)?;
        xs = self.pre_ln.forward(&xs)?;
        let mut penultimate = None;
        for (i, layer) in self.layers.iter().enumerate() {
            xs = layer.forward(&xs)?;
            if i == PENULTIMATE {
                penultimate = Some(xs.clone());
                break;
            }
        }
        // PENULTIMATE < LAYERS by construction, so the loop
        // always captures it; the fallback is unreachable
        // and exists only to avoid an unwrap on tensor data.
        let out = penultimate.unwrap_or(xs);
        Ok(out.to_dtype(DType::F32)?)
    }

    /// Preprocess and encode a reference image in one call.
    ///
    /// # Errors
    /// Returns `gen_conditioning` for a missing/undecodable
    /// reference, tensor errors otherwise.
    pub fn embed(&self, reference: &Path) -> Result<Tensor> {
        let pixels = self.preprocess(reference)?;
        self.penultimate(&pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_vision_weights_are_a_conditioning_refusal() {
        // Adversarial: a missing conditioning input refuses
        // with the typed code, before any tensor work.
        let err = ClipVisionH::load(
            Path::new("/nonexistent/clip_vision.safetensors"),
            &Device::Cpu,
        )
        .err();
        match err {
            Some(e @ GenError::Conditioning { .. }) => assert_eq!(e.code(), "gen_conditioning"),
            other => panic!("expected gen_conditioning, got {other:?}"),
        }
    }

    #[test]
    fn penultimate_is_the_second_to_last_layer() {
        // Validation: the adopted convention's arithmetic.
        assert_eq!(PENULTIMATE, 30);
        assert_eq!(SEQ_LEN, 16 * 16 + 1);
        assert_eq!(HEADS * HEAD_DIM, EMBED_DIM);
    }
}
