//! IP-Adapter conditioning (Stage 2, D-936): the
//! plus-model resampler that turns CLIP-ViT-H penultimate
//! tokens into 16 reference tokens at the UNet context
//! width, and the per-site key/value projections the
//! vendored cross-attention consumes.
//!
//! Clean-room from the weights file's measured layout
//! (191 tensors, header-censused) and the trials'
//! measured behaviour. IPAdapter_plus itself is GPL-3.0
//! and was used as oracle/specimen ONLY — no code taken.
//!
//! Measured facts the implementation rests on:
//! - `image_proj` is a Perceiver resampler: 16 learned
//!   latents (1280), four layers of cross-attention
//!   (latents query the image tokens) + feed-forward,
//!   `proj_in`/`proj_out` around them, `norm_out` at the
//!   end; output is 16 tokens x 2048.
//! - `ip_adapter.{odd 1..=139}` holds 70 to_k_ip/to_v_ip
//!   pairs and NO to_out: the image branch shares the
//!   text attention's output projection (the decoupled
//!   cross-attention design; see sd::attention).
//! - The pairs' channel sequence, read off the file's
//!   shape transitions (640 at pair 1, 1280 from pair 9,
//!   640 again at pair 109, 1280 at pair 121), fixes the
//!   site order: down blocks (4x640 + 20x1280), up blocks
//!   (30x1280 + 6x640), mid block LAST (10x1280). The
//!   loader asserts every pair's shape against the
//!   expected site channel, so a wrong order fails at
//!   load, not in the image.
//!
//! Embed-magnitude discipline (the trials' central
//! finding, TRIAL-REPORT + ROUND2-REPORT): the naive path
//! saturates at scale >= 0.5; the oracle's cure is its
//! `K+V w/ C penalty` embeds scaling. The estate's
//! gencond seam names per-reference embed normalisation a
//! first-class step, and gencond's own design for it is
//! measurement over assumption (recorded magnitudes,
//! regime flags — never an invented threshold).
//!
//! `normalize_reference_embed` applies the token-count
//! penalty `1 / sqrt(token_count)` — the Stage 2
//! calibration sweep's strongest fully-healthy point
//! (see the function's docs for the measured regime
//! boundary). Since `to_k_ip`/`to_v_ip` are bias-free, a
//! token factor IS a K+V factor, which is what makes
//! this a faithful seam for the oracle's penalty shape.
//! The raw RMS, the factor, and the resulting RMS ride
//! the run's provenance. This is a clean-room
//! reconstruction of the penalty, not a transcription:
//! the oracle's within-stack effect at equal weight is
//! larger, so numerical parity stays REVIEW (D-713).

use std::path::Path;

use candle::{DType, Device, Tensor};
use candle_nn::{self as nn, Module, VarBuilder};

use crate::error::{GenError, Result};

/// Reference token count the resampler emits.
pub const IP_TOKENS: usize = 16;
/// UNet context width (the resampler's output width).
pub const CONTEXT_DIM: usize = 2048;
/// Vision width (the resampler's internal width).
const VISION_DIM: usize = 1280;
/// Resampler layer count in the file.
const RESAMPLER_LAYERS: usize = 4;
/// Resampler attention heads (named calibration point:
/// head partition of the 1280-wide attention; the parity
/// gate judges the whole path, D-713 raw reporting).
const RESAMPLER_HEADS: usize = 8;

/// Expected channel of each IP site in the weights
/// file's enumeration order (down, up, mid last). The
/// sequence is the file's measured shape-transition
/// sequence; the loader asserts against it per site.
pub const SITE_CHANNELS: [usize; 70] = [
    640, 640, 640, 640, // down block 1: 2 resnets x depth 2
    1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, // down block 2 (10)
    1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, // down block 2 (20)
    1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, // up block 0 (10)
    1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, // up block 0 (20)
    1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, // up block 0 (30)
    640, 640, 640, 640, 640, 640, // up block 1: 3 resnets x depth 2
    1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, 1280, // mid block
];

struct ResamplerLayer {
    norm1: nn::LayerNorm,
    norm2: nn::LayerNorm,
    to_q: nn::Linear,
    to_kv: nn::Linear,
    to_out: nn::Linear,
    ff_norm: nn::LayerNorm,
    ff_up: nn::Linear,
    ff_down: nn::Linear,
}

impl ResamplerLayer {
    fn new(vb: VarBuilder) -> Result<Self> {
        let attn = vb.pp("0");
        let ff = vb.pp("1");
        Ok(Self {
            norm1: nn::layer_norm(VISION_DIM, 1e-5, attn.pp("norm1"))?,
            norm2: nn::layer_norm(VISION_DIM, 1e-5, attn.pp("norm2"))?,
            to_q: nn::linear_no_bias(VISION_DIM, VISION_DIM, attn.pp("to_q"))?,
            to_kv: nn::linear_no_bias(VISION_DIM, VISION_DIM * 2, attn.pp("to_kv"))?,
            to_out: nn::linear_no_bias(VISION_DIM, VISION_DIM, attn.pp("to_out"))?,
            ff_norm: nn::layer_norm(VISION_DIM, 1e-5, ff.pp("0"))?,
            ff_up: nn::linear_no_bias(VISION_DIM, VISION_DIM * 4, ff.pp("1"))?,
            ff_down: nn::linear_no_bias(VISION_DIM * 4, VISION_DIM, ff.pp("3"))?,
        })
    }

    fn forward(&self, latents: &Tensor, context: &Tensor) -> Result<Tensor> {
        // Cross-attention: latents query the image tokens.
        let (b, t, _) = latents.dims3()?;
        let tc = context.dim(1)?;
        let head_dim = VISION_DIM / RESAMPLER_HEADS;
        let split = |x: Tensor, seq: usize| -> Result<Tensor> {
            Ok(x.reshape((b, seq, RESAMPLER_HEADS, head_dim))?
                .transpose(1, 2)?
                .to_dtype(DType::F32)?)
        };
        let q = split(self.to_q.forward(&self.norm1.forward(latents)?)?, t)?;
        let kv = self.to_kv.forward(&self.norm2.forward(context)?)?;
        let k = split(kv.narrow(2, 0, VISION_DIM)?, tc)?;
        let v = split(kv.narrow(2, VISION_DIM, VISION_DIM)?, tc)?;
        let scale = 1.0 / f64::sqrt(head_dim as f64);
        let scores = (q.matmul(&k.transpose(2, 3)?)? * scale)?;
        let attn = nn::ops::softmax(&scores, candle::D::Minus1)?;
        let out = attn
            .matmul(&v)?
            .transpose(1, 2)?
            .reshape((b, t, VISION_DIM))?
            .to_dtype(latents.dtype())?;
        let xs = (latents + self.to_out.forward(&out)?)?;
        let h = self.ff_up.forward(&self.ff_norm.forward(&xs)?)?.gelu()?;
        Ok((xs + self.ff_down.forward(&h)?)?)
    }
}

/// The loaded resampler.
pub struct Resampler {
    latents: Tensor,
    proj_in: nn::Linear,
    layers: Vec<ResamplerLayer>,
    proj_out: nn::Linear,
    norm_out: nn::LayerNorm,
}

impl Resampler {
    /// Run the resampler over vision tokens
    /// `(1, seq, 1280)` f32; returns `(1, 16, 2048)` f32
    /// reference tokens (pre-normalisation).
    ///
    /// # Errors
    /// Returns tensor errors typed by the crate enum.
    pub fn forward(&self, vision_tokens: &Tensor) -> Result<Tensor> {
        let ctx = self.proj_in.forward(&vision_tokens.to_dtype(DType::F16)?)?;
        let mut lat = self
            .latents
            .to_dtype(DType::F16)?
            .broadcast_as((1, IP_TOKENS, VISION_DIM))?;
        for layer in &self.layers {
            lat = layer.forward(&lat, &ctx)?;
        }
        let out = self.proj_out.forward(&lat)?;
        Ok(self.norm_out.forward(&out)?.to_dtype(DType::F32)?)
    }
}

/// Everything the IP path loads from the adapter file.
pub struct IpWeights {
    /// The resampler.
    pub resampler: Resampler,
    /// Per-site (to_k_ip, to_v_ip) projections in
    /// SITE_CHANNELS order, ready for the UNet's attach.
    pub sites: Vec<(nn::Linear, nn::Linear)>,
}

/// Load the IP-Adapter file (f16, on the given device):
/// the resampler plus the 70 per-site projections, each
/// shape-asserted against SITE_CHANNELS.
///
/// # Errors
/// Returns `gen_conditioning` when the file is missing,
/// `gen_weights` when a tensor is absent, and
/// `gen_conditioning` when a site's shape contradicts the
/// measured enumeration (a different adapter variant must
/// refuse, not mis-attach).
pub fn load_ip_weights(path: &Path, device: &Device) -> Result<IpWeights> {
    if !path.exists() {
        return Err(GenError::Conditioning {
            detail: format!("ip-adapter weights {} do not exist", path.display()),
        });
    }
    // SAFETY: read-only mapping of a weights file this
    // process owns for the run's duration; never mutated.
    #[allow(unsafe_code)]
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[path], DType::F16, device) }.map_err(
        |e| GenError::Weights {
            detail: format!("{}: {e}", path.display()),
        },
    )?;
    let proj = vb.pp("image_proj");
    let resampler = Resampler {
        latents: proj.get((1, IP_TOKENS, VISION_DIM), "latents")?,
        proj_in: nn::linear(VISION_DIM, VISION_DIM, proj.pp("proj_in"))?,
        layers: (0..RESAMPLER_LAYERS)
            .map(|i| ResamplerLayer::new(proj.pp(format!("layers.{i}"))))
            .collect::<Result<Vec<_>>>()?,
        proj_out: nn::linear(VISION_DIM, CONTEXT_DIM, proj.pp("proj_out"))?,
        norm_out: nn::layer_norm(CONTEXT_DIM, 1e-5, proj.pp("norm_out"))?,
    };
    let mut sites = Vec::with_capacity(SITE_CHANNELS.len());
    for (i, channels) in SITE_CHANNELS.iter().enumerate() {
        let key = 2 * i + 1;
        let vb_site = vb.pp(format!("ip_adapter.{key}"));
        let w_k = vb_site.get((*channels, CONTEXT_DIM), "to_k_ip.weight")?;
        let w_v = vb_site.get((*channels, CONTEXT_DIM), "to_v_ip.weight")?;
        sites.push((nn::Linear::new(w_k, None), nn::Linear::new(w_v, None)));
    }
    Ok(IpWeights { resampler, sites })
}

/// Root-mean-square of a tensor's elements (f64).
pub fn rms(t: &Tensor) -> Result<f64> {
    let sq = t.to_dtype(DType::F32)?.sqr()?;
    let mean = sq.mean_all()?.to_scalar::<f32>()?;
    Ok(f64::from(mean).sqrt())
}

/// Per-reference embed normalisation: apply the
/// token-count penalty `1 / sqrt(token_count)` to the
/// reference tokens and return the penalised tokens
/// with the applied factor (the run's provenance
/// records raw/resulting RMS alongside it).
///
/// The factor is the Stage 2 calibration sweep's
/// finding (S2 gate specimen, IP weight 0.7, this
/// adapter's 16 tokens): factor 1.0 (raw) collapses
/// the output (per-channel std ~4 vs the control's
/// ~46, mean |gradient| 1.7 vs 5.0); 0.5 partially
/// collapses (blue-channel std 14); 0.25 is healthy
/// (std ~39, gradient 5.2, saturation 0.109 vs the
/// control's 0.144) with a clear conditioning effect
/// (mean |delta| vs control ~26 per channel); the
/// first reading tried — rescaling to the text
/// context's RMS, a factor of ~2.15 on that specimen —
/// collapses fully and desaturates (saturation 0.040).
/// Evidence: gen-stage2-20261104/runs probes +
/// measurements on primo.
///
/// # Errors
/// Returns `gen_conditioning` on a zero-RMS or
/// token-less reference (a degenerate embed is a
/// failed reference, refused rather than amplified)
/// and tensor errors otherwise.
pub fn normalize_reference_embed(tokens: &Tensor) -> Result<(Tensor, f64)> {
    let (_, seq, _) = tokens.dims3().map_err(|_| GenError::Conditioning {
        detail: "reference embed must be (batch, tokens, channels)".to_string(),
    })?;
    if seq == 0 {
        return Err(GenError::Conditioning {
            detail: "reference embed has no tokens; refusing".to_string(),
        });
    }
    let raw = rms(tokens)?;
    if raw <= 0.0 || !raw.is_finite() {
        return Err(GenError::Conditioning {
            detail: format!("reference embed has non-positive RMS {raw}; refusing"),
        });
    }
    let factor = 1.0 / (seq as f64).sqrt();
    Ok(((tokens * factor)?, factor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_table_matches_the_measured_transitions() {
        // Validation: 70 sites; transitions at entries
        // 0 (640), 4 (1280), 60 (640), 66 (1280) — the
        // file's pair indices 1, 9, 121... measured as
        // pairs 1/9/109/121 -> entries 0/4/54/60. Assert
        // the channel runs, not just the count.
        assert_eq!(SITE_CHANNELS.len(), 70);
        assert!(SITE_CHANNELS[0..4].iter().all(|c| *c == 640));
        assert!(SITE_CHANNELS[4..54].iter().all(|c| *c == 1280));
        assert!(SITE_CHANNELS[54..60].iter().all(|c| *c == 640));
        assert!(SITE_CHANNELS[60..70].iter().all(|c| *c == 1280));
    }

    #[test]
    fn normalize_applies_the_token_count_penalty() {
        // Validation: 16 tokens of RMS 2 -> factor
        // 1/sqrt(16) = 0.25 and post-RMS 0.5.
        let t = Tensor::new(vec![2.0f32; 64], &Device::Cpu)
            .unwrap()
            .reshape((1, 16, 4))
            .unwrap();
        let (out, factor) = normalize_reference_embed(&t).unwrap();
        assert!((factor - 0.25).abs() < 1e-6);
        assert!((rms(&out).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn zero_rms_reference_is_refused() {
        // Adversarial: a degenerate (all-zero) embed must
        // refuse, never divide by zero or amplify noise.
        let t = Tensor::zeros((1, 16, 4), DType::F32, &Device::Cpu).unwrap();
        assert!(matches!(
            normalize_reference_embed(&t),
            Err(GenError::Conditioning { .. })
        ));
    }

    #[test]
    fn missing_adapter_weights_are_a_conditioning_refusal() {
        // Adversarial: missing input refuses typed, early.
        let err = load_ip_weights(
            std::path::Path::new("/nonexistent/ip-adapter.safetensors"),
            &Device::Cpu,
        )
        .err();
        match err {
            Some(e @ GenError::Conditioning { .. }) => {
                assert_eq!(e.code(), "gen_conditioning");
            }
            other => panic!("expected gen_conditioning, got {other:?}"),
        }
    }
}
