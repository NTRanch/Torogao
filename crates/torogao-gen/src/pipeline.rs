//! The SDXL inference pipeline: load the four components of
//! a converted checkpoint (see the converter notes in the
//! Stage 1 proof report), encode prompts through CLIP-L and
//! CLIP-G, run DPM++ 2M with the SDXL added conditioning,
//! decode through the VAE. Batch convention matches
//! ComfyUI: row 0 is the unconditional (negative) prompt,
//! row 1 the conditional prompt; sampler state stays f32,
//! UNet compute runs f16 — ComfyUI's own dtype split.

use std::path::{Path, PathBuf};

use candle::{DType, Device, IndexOp, Tensor};
use candle_nn::{Linear, Module, VarBuilder};
use candle_transformers::models::stable_diffusion as sd;
use tokenizers::Tokenizer;

use crate::error::{GenError, Result};
use crate::limits;
use crate::rng::GaussianStream;
use crate::sampler::dpmpp_2m;
use crate::schedule::Schedule;
use crate::sd::unet_2d::{BlockConfig, UNet2DConditionModel, UNet2DConditionModelConfig};

/// EOS/pad token id for both CLIP tokenizers.
const PAD_ID: u32 = 49407;
/// SDXL VAE scaling factor.
const VAE_SCALE: f64 = 0.18215;

/// One generation request.
#[derive(Debug, Clone)]
pub struct GenParams {
    /// The positive prompt.
    pub prompt: String,
    /// The negative prompt (may be empty).
    pub negative: String,
    /// Seed for the counter-based noise stream.
    pub seed: u64,
    /// Sampler steps (DPM++ 2M, Karras ladder).
    pub steps: usize,
    /// Classifier-free guidance scale.
    pub cfg: f32,
    /// Output width in pixels (multiple of 8).
    pub width: u32,
    /// Output height in pixels (multiple of 8).
    pub height: u32,
}

impl GenParams {
    /// Check the request against the limits module.
    ///
    /// # Errors
    /// Returns `gen_limit` naming the first violated bound.
    pub fn validate(&self) -> Result<()> {
        let bad = |field: &'static str, detail: String| GenError::Limit { field, detail };
        if self.width == 0 || self.width > limits::AXIS_MAX_PX || self.width % 8 != 0 {
            return Err(bad(
                "width",
                format!("{} not in 8..={} step 8", self.width, limits::AXIS_MAX_PX),
            ));
        }
        if self.height == 0 || self.height > limits::AXIS_MAX_PX || self.height % 8 != 0 {
            return Err(bad(
                "height",
                format!("{} not in 8..={} step 8", self.height, limits::AXIS_MAX_PX),
            ));
        }
        if self.width.saturating_mul(self.height) > limits::TOTAL_MAX_PX {
            return Err(bad(
                "pixels",
                format!(
                    "{} exceeds {}",
                    self.width * self.height,
                    limits::TOTAL_MAX_PX
                ),
            ));
        }
        if !(limits::STEPS_MIN..=limits::STEPS_MAX).contains(&self.steps) {
            return Err(bad(
                "steps",
                format!(
                    "{} not in {}..={}",
                    self.steps,
                    limits::STEPS_MIN,
                    limits::STEPS_MAX
                ),
            ));
        }
        if !(limits::CFG_MIN..=limits::CFG_MAX).contains(&self.cfg) {
            return Err(bad(
                "cfg",
                format!(
                    "{} not in {}..={}",
                    self.cfg,
                    limits::CFG_MIN,
                    limits::CFG_MAX
                ),
            ));
        }
        Ok(())
    }
}

/// A generated image plus the facts its run record needs.
#[derive(Debug)]
pub struct GeneratedImage {
    /// RGB pixels, row-major, `width * height * 3` bytes.
    pub rgb: Vec<u8>,
    /// Image width.
    pub width: u32,
    /// Image height.
    pub height: u32,
    /// Wall-clock generation time in milliseconds.
    pub elapsed_ms: u64,
    /// SHA-256 of the latent noise buffer (pre-sampler),
    /// a replay fingerprint of the RNG stream.
    pub noise_sha256: String,
}

/// The loaded SDXL model set.
pub struct SdxlModel {
    dir: std::path::PathBuf,
    vae: sd::vae::AutoEncoderKL,
    clip: sd::clip::ClipTextTransformer,
    clip2: sd::clip::ClipTextTransformer,
    text_projection: Tensor,
    tokenizer: Tokenizer,
    tokenizer2: Tokenizer,
    device: Device,
    schedule: Schedule,
}

fn load_tensor(path: &Path, name: &str, device: &Device) -> Result<Tensor> {
    // SAFETY: read-only mapping of a weights file this
    // process owns for the run's duration; never mutated.
    #[allow(unsafe_code)]
    let mmap = unsafe { candle::safetensors::MmapedSafetensors::new(path) }.map_err(|e| {
        GenError::Weights {
            detail: format!("{}: {e}", path.display()),
        }
    })?;
    mmap.load(name, device).map_err(|e| GenError::Weights {
        detail: format!("{name}: {e}"),
    })
}

impl SdxlModel {
    /// Load all components from a converted checkpoint
    /// directory (unet/vae/clip/clip2 safetensors plus the
    /// two tokenizer.json files).
    ///
    /// # Errors
    /// Returns `gen_weights` on any missing file or tensor.
    pub fn load(dir: &Path, device: &Device) -> Result<Self> {
        let cfg = sd::StableDiffusionConfig::sdxl(None, None, None);
        let path = |n: &str| -> Result<PathBuf> {
            let p = dir.join(n);
            if p.exists() {
                Ok(p)
            } else {
                Err(GenError::Weights {
                    detail: format!("missing {}", p.display()),
                })
            }
        };
        let vae = cfg
            .build_vae(path("vae.safetensors")?, device, DType::F32)
            .map_err(GenError::from)?;
        // Residency discipline: the text encoders live on
        // CPU. Encoding 77 tokens costs milliseconds and
        // keeps ~3.3 GiB of fp32 CLIP weights off the GPU;
        // only the UNet and VAE are GPU-resident.
        let cpu = Device::Cpu;
        let clip =
            sd::build_clip_transformer(&cfg.clip, path("clip.safetensors")?, &cpu, DType::F32)
                .map_err(GenError::from)?;
        let clip2_cfg = cfg.clip2.as_ref().ok_or_else(|| GenError::Weights {
            detail: "sdxl config has no clip2".into(),
        })?;
        let clip2 =
            sd::build_clip_transformer(clip2_cfg, path("clip2.safetensors")?, &cpu, DType::F32)
                .map_err(GenError::from)?;
        let text_projection =
            load_tensor(&path("clip2.safetensors")?, "text_projection.weight", &cpu)?
                .to_dtype(DType::F32)?;
        let tokenizer =
            Tokenizer::from_file(path("tokenizer.json")?).map_err(|e| GenError::Tokenizer {
                detail: e.to_string(),
            })?;
        let tokenizer2 =
            Tokenizer::from_file(path("tokenizer2.json")?).map_err(|e| GenError::Tokenizer {
                detail: e.to_string(),
            })?;
        Ok(Self {
            dir: dir.to_path_buf(),
            vae,
            clip,
            clip2,
            text_projection,
            tokenizer,
            tokenizer2,
            device: device.clone(),
            schedule: Schedule::sdxl(),
        })
    }

    /// Load the UNet and its added-conditioning projection
    /// from the retained weights directory. Kept out of
    /// `load` so `generate` can drop both before the VAE
    /// decode phase: residency is staged (UNet resident only
    /// while sampling), which is what lets a run fit beside
    /// other tenants on a shared card.
    fn load_unet(&self) -> Result<(UNet2DConditionModel, (Linear, Linear))> {
        let device = &self.device;
        let unet_path = self.dir.join("unet.safetensors");
        // SAFETY: read-only weights mapping, as in `load`.
        #[allow(unsafe_code)]
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[&unet_path], DType::F16, device) }
            .map_err(GenError::from)?;
        // The UNet is the vendored build (sd module): candle
        // upstream loads the same weights but cannot accept
        // the added conditioning.
        let unet_cfg = UNet2DConditionModelConfig {
            blocks: vec![
                BlockConfig {
                    out_channels: 320,
                    use_cross_attn: None,
                    attention_head_dim: 5,
                },
                BlockConfig {
                    out_channels: 640,
                    use_cross_attn: Some(2),
                    attention_head_dim: 10,
                },
                BlockConfig {
                    out_channels: 1280,
                    use_cross_attn: Some(10),
                    attention_head_dim: 20,
                },
            ],
            center_input_sample: false,
            cross_attention_dim: 2048,
            downsample_padding: 1,
            flip_sin_to_cos: true,
            freq_shift: 0.0,
            layers_per_block: 2,
            mid_block_scale_factor: 1.0,
            norm_eps: 1e-5,
            norm_num_groups: 32,
            sliced_attention_size: None,
            use_linear_projection: true,
        };
        let unet =
            UNet2DConditionModel::new(vb.clone(), 4, 4, false, unet_cfg).map_err(GenError::from)?;
        let add1 = candle_nn::linear(2816, 1280, vb.pp("add_embedding.linear_1"))
            .map_err(GenError::from)?;
        let add2 = candle_nn::linear(1280, 1280, vb.pp("add_embedding.linear_2"))
            .map_err(GenError::from)?;
        Ok((unet, (add1, add2)))
    }

    fn tokenize(&self, tok: &Tokenizer, text: &str, pad_id: u32) -> Result<Vec<u32>> {
        let enc = tok.encode(text, true).map_err(|e| GenError::Tokenizer {
            detail: e.to_string(),
        })?;
        let mut ids: Vec<u32> = enc.get_ids().to_vec();
        ids.resize(limits::TOKEN_LEN, pad_id);
        ids.truncate(limits::TOKEN_LEN);
        Ok(ids)
    }

    /// Encode one prompt through both CLIPs; returns the
    /// concatenated sequence embedding (f32) and the pooled
    /// embedding (f32, 1280).
    fn encode(&self, text: &str) -> Result<(Tensor, Tensor)> {
        let ids = self.tokenize(&self.tokenizer, text, PAD_ID)?;
        // ComfyUI's SDXL pair pads asymmetrically: CLIP-L
        // pads with the end token, CLIP-G pads with token 0
        // (sdxl_clip.SDXLClipGTokenizer, pad_with_end=False,
        // special_tokens pad=0). The padding positions are
        // attended by the UNet, so this is load-bearing.
        let ids2 = self.tokenize(&self.tokenizer2, text, 0)?;
        // Encoders are CPU-resident (see load).
        let dev = &Device::Cpu;
        let t = Tensor::new(ids.clone(), dev)?.reshape((1, limits::TOKEN_LEN))?;
        let t2 = Tensor::new(ids2.clone(), dev)?.reshape((1, limits::TOKEN_LEN))?;
        // SDXL consumes the penultimate layer's hidden
        // states (pre-final-norm) as the sequence context —
        // the diffusers/ComfyUI convention; the final layer
        // output serves only the pooled embedding.
        let (final1, hidden) = self.clip.forward_until_encoder_layer(&t, usize::MAX, -2)?;
        let _ = final1;
        let (hidden2_final, hidden2) =
            self.clip2
                .forward_until_encoder_layer(&t2, usize::MAX, -2)?;
        // The first EOS id is the sequence end (padding
        // after it repeats the same id).
        let eos = ids
            .iter()
            .position(|id| *id == PAD_ID)
            .unwrap_or(limits::TOKEN_LEN - 1);
        let pooled_src = hidden2_final.i((0, eos))?;
        let pooled = pooled_src
            .unsqueeze(0)?
            .matmul(&self.text_projection.t()?)?
            .squeeze(0)?;
        let context = Tensor::cat(&[&hidden, &hidden2], 2)?;
        Ok((context, pooled))
    }

    /// The added-conditioning vector (f16, 1280) for one
    /// prompt: add_embedding([pooled | time-id embedding]).
    fn added(
        &self,
        add: &(Linear, Linear),
        pooled: &Tensor,
        width: u32,
        height: u32,
    ) -> Result<Tensor> {
        let ids = crate::schedule::time_ids_embedding(height, width);
        let ids_t = Tensor::new(ids, &self.device)?.reshape((1, 1536))?;
        let pooled = pooled.reshape((1, 1280))?.to_device(&self.device)?;
        let joined = Tensor::cat(&[&pooled, &ids_t], 1)?.to_dtype(DType::F16)?;
        let h = add.0.forward(&joined)?;
        let h = candle_nn::ops::silu(&h)?;
        Ok(add.1.forward(&h)?)
    }

    /// Generate one image.
    ///
    /// # Errors
    /// Returns limit, tokenizer, and tensor errors typed by
    /// the crate error enum.
    pub fn generate(&self, params: &GenParams) -> Result<GeneratedImage> {
        params.validate()?;
        let started = std::time::Instant::now();
        let dev = &self.device;
        let (ctx_uncond, pooled_uncond) = self.encode(&params.negative)?;
        let (ctx_cond, pooled_cond) = self.encode(&params.prompt)?;
        let context = Tensor::cat(&[&ctx_uncond, &ctx_cond], 0)?
            .to_dtype(DType::F16)?
            .to_device(dev)?;
        let (unet, add_embedding) = self.load_unet()?;
        let added = Tensor::cat(
            &[
                &self.added(&add_embedding, &pooled_uncond, params.width, params.height)?,
                &self.added(&add_embedding, &pooled_cond, params.width, params.height)?,
            ],
            0,
        )?;
        let lw = (params.width / 8) as usize;
        let lh = (params.height / 8) as usize;
        let n = 4 * lw * lh;
        let mut noise = vec![0f32; n];
        GaussianStream::new(params.seed).fill_gaussian(&mut noise);
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        for v in &noise {
            hasher.update(v.to_le_bytes());
        }
        let noise_sha256 = format!("{:x}", hasher.finalize());
        let noise_t = Tensor::from_vec(noise, (1, 4, lh, lw), &Device::Cpu)?.to_device(dev)?;
        let sigmas = self.schedule.karras(params.steps);
        let x0_latent = (noise_t * sigmas[0])?;
        let context_ref = &context;
        let added_ref = &added;
        let schedule = &self.schedule;
        let cfg_scale = params.cfg;
        let x = dpmpp_2m(x0_latent, &sigmas, |x_in, sigma| {
            // EPS preconditioning (ComfyUI model_sampling
            // EPS.calculate_input, sigma_data = 1): the UNet
            // sees x / sqrt(sigma^2 + 1); the denoised
            // conversion below uses the unscaled state.
            let xb = (Tensor::cat(&[x_in, x_in], 0)? / (sigma * sigma + 1.0).sqrt())?
                .to_dtype(DType::F16)?;
            let timestep = schedule.timestep(sigma) as f64;
            let eps = unet
                .forward_with_added(&xb, timestep, context_ref, Some(added_ref))?
                .to_dtype(DType::F32)?;
            let eps_uncond = eps.narrow(0, 0, 1)?;
            let eps_cond = eps.narrow(0, 1, 1)?;
            let eps = (eps_uncond.clone() + ((eps_cond - eps_uncond)? * cfg_scale as f64)?)?;
            let x0 = (x_in - (eps.clone() * sigma)?)?;
            Ok(x0)
        })?;
        // Staged residency: the UNet and its projection are
        // released before the decode phase (see load_unet).
        drop(unet);
        drop(add_embedding);
        let decoded = self.vae.decode(&(&x / VAE_SCALE)?)?;
        let img = (((decoded / 2.0)? + 0.5)?.clamp(0.0, 1.0)? * 255.0)?;
        let img = img.to_dtype(DType::U8)?.i(0)?.permute((1, 2, 0))?;
        let rgb = img.flatten_all()?.to_vec1::<u8>()?;
        Ok(GeneratedImage {
            rgb,
            width: params.width,
            height: params.height,
            elapsed_ms: started.elapsed().as_millis() as u64,
            noise_sha256,
        })
    }
}

/// Write an RGB buffer as a PNG.
///
/// # Errors
/// Returns `gen_image` on encode failure.
pub fn write_png(img: &GeneratedImage, path: &Path) -> Result<()> {
    let buf =
        image::RgbImage::from_raw(img.width, img.height, img.rgb.clone()).ok_or_else(|| {
            GenError::Image {
                detail: "buffer size mismatch".into(),
            }
        })?;
    buf.save(path).map_err(|e| GenError::Image {
        detail: e.to_string(),
    })?;
    Ok(())
}
