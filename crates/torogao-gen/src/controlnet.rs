//! ControlNet conditioning (Stage 2, D-936): the xinsir
//! ControlNet-Union (promax) weights in their native
//! format, run as a residual branch beside the vendored
//! UNet, exactly where the architecture puts it: nine
//! zero-convolution outputs added index-wise to the
//! UNet's down-block residual list (conv_in output first),
//! one added to the mid-block output — the injection the
//! vendored `forward_with_additional_residuals` already
//! implements.
//!
//! Clean-room from the weights file's measured layout
//! (863 tensors, header-censused) and the trials'
//! measured behaviour. The encoder reuses the vendored
//! UNet block types with the UNet's own configuration, so
//! the branch cannot drift from the trunk's block math.
//!
//! Two pieces are estate RECONSTRUCTIONS, named as such
//! (the union file ships the tensors but no runnable
//! reference implementation in a license this tree may
//! read):
//! - `control_add_embedding` (2048 -> 1280) consumes the
//!   mean-pooled encoder context and is added to the time
//!   embedding beside the file's own `add_embedding`.
//! - The union type path — `task_embedding` row ->
//!   `transformer_layes.0` -> `spatial_ch_projs` — produces
//!   a per-channel bias added to the conditioning
//!   embedding's output. With a single token, attention is
//!   head-count-invariant, so the block is exact under any
//!   head split; the parity gate judges the whole path.

use std::path::Path;

use candle::{DType, Device, Tensor};
use candle_nn::{self as nn, Module, VarBuilder};

use crate::conditioning::CnType;
use crate::error::{GenError, Result};
use crate::sd::embeddings::{TimestepEmbedding, Timesteps};
use crate::sd::unet_2d_blocks::{
    CrossAttnDownBlock2D, CrossAttnDownBlock2DConfig, DownBlock2D, DownBlock2DConfig,
    UNetMidBlock2DCrossAttn, UNetMidBlock2DCrossAttnConfig,
};

/// Zero-conv output channels, in residual-list order:
/// conv_in, down0 resnets+downsample, down1, down2.
const ZERO_CHANNELS: [usize; 9] = [320, 320, 320, 320, 640, 640, 640, 1280, 1280];
/// Conditioning-embedding conv stack: (out, in, stride).
const COND_BLOCKS: [(usize, usize, usize); 6] = [
    (16, 16, 1),
    (32, 16, 2),
    (32, 32, 1),
    (96, 32, 2),
    (96, 96, 1),
    (256, 96, 2),
];
/// Time-embedding width (matches the UNet's).
const TIME_EMBED_DIM: usize = 1280;
/// Sinusoidal timestep channels (matches the UNet's).
const TIME_CHANNELS: usize = 320;

/// The union type transformer block (the file's
/// `transformer_layes.0`): a single OpenAI-style block over
/// one task token.
struct TypeBlock {
    ln_1: nn::LayerNorm,
    in_proj_weight: Tensor,
    in_proj_bias: Tensor,
    out_proj: nn::Linear,
    ln_2: nn::LayerNorm,
    c_fc: nn::Linear,
    c_proj: nn::Linear,
}

impl TypeBlock {
    fn new(vb: VarBuilder) -> Result<Self> {
        let vb = vb.pp("transformer_layes.0");
        Ok(Self {
            ln_1: nn::layer_norm(320, 1e-5, vb.pp("ln_1"))?,
            in_proj_weight: vb.get((960, 320), "attn.in_proj_weight")?,
            in_proj_bias: vb.get(960, "attn.in_proj_bias")?,
            out_proj: nn::linear(320, 320, vb.pp("attn.out_proj"))?,
            ln_2: nn::layer_norm(320, 1e-5, vb.pp("ln_2"))?,
            c_fc: nn::linear(320, 1280, vb.pp("mlp.c_fc"))?,
            c_proj: nn::linear(1280, 320, vb.pp("mlp.c_proj"))?,
        })
    }

    /// Forward over a `(1, 320)` task token. Attention over
    /// a single key is the identity on values under any
    /// head split, so q/k are computed only to keep the
    /// op sequence honest; the result is exact.
    fn forward(&self, token: &Tensor) -> Result<Tensor> {
        let h = self.ln_1.forward(token)?;
        let qkv = h
            .matmul(&self.in_proj_weight.t()?)?
            .broadcast_add(&self.in_proj_bias)?;
        let v = qkv.narrow(1, 640, 320)?;
        let xs = (token + self.out_proj.forward(&v)?)?;
        let h = self.ln_2.forward(&xs)?;
        let h = self.c_fc.forward(&h)?.gelu()?;
        Ok((xs + self.c_proj.forward(&h)?)?)
    }
}

/// The loaded ControlNet.
pub struct ControlNet {
    time_proj: Timesteps,
    time_embedding: TimestepEmbedding,
    add_1: nn::Linear,
    add_2: nn::Linear,
    control_add_1: nn::Linear,
    control_add_2: nn::Linear,
    cond_conv_in: nn::Conv2d,
    cond_blocks: Vec<nn::Conv2d>,
    cond_conv_out: nn::Conv2d,
    task_embedding: Tensor,
    type_block: TypeBlock,
    spatial_ch_projs: nn::Linear,
    conv_in: nn::Conv2d,
    down_0: DownBlock2D,
    down_1: CrossAttnDownBlock2D,
    down_2: CrossAttnDownBlock2D,
    mid: UNetMidBlock2DCrossAttn,
    zero_convs: Vec<nn::Conv2d>,
    zero_mid: nn::Conv2d,
}

impl ControlNet {
    /// Load the union ControlNet weights (f16, on the
    /// given device).
    ///
    /// # Errors
    /// Returns `gen_conditioning` when the file is missing
    /// and `gen_weights` when a tensor is absent or
    /// misshapen (the strict VarBuilder load).
    pub fn load(path: &Path, device: &Device) -> Result<Self> {
        if !path.exists() {
            return Err(GenError::Conditioning {
                detail: format!("controlnet weights {} do not exist", path.display()),
            });
        }
        // SAFETY: read-only mapping of a weights file this
        // process owns for the run's duration; never mutated.
        #[allow(unsafe_code)]
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[path], DType::F16, device) }
            .map_err(|e| GenError::Weights {
                detail: format!("{}: {e}", path.display()),
            })?;
        let pad1 = nn::Conv2dConfig {
            padding: 1,
            ..Default::default()
        };
        let cond_conv_in = nn::conv2d(3, 16, 3, pad1, vb.pp("controlnet_cond_embedding.conv_in"))?;
        let mut cond_blocks = Vec::with_capacity(COND_BLOCKS.len());
        for (i, (out, cin, stride)) in COND_BLOCKS.iter().enumerate() {
            let cfg = nn::Conv2dConfig {
                padding: 1,
                stride: *stride,
                ..Default::default()
            };
            cond_blocks.push(nn::conv2d(
                *cin,
                *out,
                3,
                cfg,
                vb.pp(format!("controlnet_cond_embedding.blocks.{i}")),
            )?);
        }
        let cond_conv_out = nn::conv2d(
            256,
            320,
            3,
            pad1,
            vb.pp("controlnet_cond_embedding.conv_out"),
        )?;
        // Encoder blocks: the vendored types with the
        // UNet's own configuration values (pipeline.rs).
        let db_cfg = |add_downsample: bool| DownBlock2DConfig {
            num_layers: 2,
            resnet_eps: 1e-5,
            resnet_groups: 32,
            add_downsample,
            downsample_padding: 1,
            ..Default::default()
        };
        let down_0 = DownBlock2D::new(
            vb.pp("down_blocks.0"),
            320,
            320,
            Some(TIME_EMBED_DIM),
            db_cfg(true),
        )?;
        let down_1 = CrossAttnDownBlock2D::new(
            vb.pp("down_blocks.1"),
            320,
            640,
            Some(TIME_EMBED_DIM),
            false,
            CrossAttnDownBlock2DConfig {
                downblock: db_cfg(true),
                attn_num_head_channels: 10,
                cross_attention_dim: 2048,
                sliced_attention_size: None,
                use_linear_projection: true,
                transformer_layers_per_block: 2,
            },
        )?;
        let down_2 = CrossAttnDownBlock2D::new(
            vb.pp("down_blocks.2"),
            640,
            1280,
            Some(TIME_EMBED_DIM),
            false,
            CrossAttnDownBlock2DConfig {
                downblock: db_cfg(false),
                attn_num_head_channels: 20,
                cross_attention_dim: 2048,
                sliced_attention_size: None,
                use_linear_projection: true,
                transformer_layers_per_block: 10,
            },
        )?;
        let mid = UNetMidBlock2DCrossAttn::new(
            vb.pp("mid_block"),
            1280,
            Some(TIME_EMBED_DIM),
            false,
            UNetMidBlock2DCrossAttnConfig {
                resnet_eps: 1e-5,
                output_scale_factor: 1.0,
                cross_attn_dim: 2048,
                attn_num_head_channels: 20,
                resnet_groups: Some(32),
                use_linear_projection: true,
                transformer_layers_per_block: 10,
                ..Default::default()
            },
        )?;
        let mut zero_convs = Vec::with_capacity(ZERO_CHANNELS.len());
        for (i, c) in ZERO_CHANNELS.iter().enumerate() {
            zero_convs.push(nn::conv2d(
                *c,
                *c,
                1,
                Default::default(),
                vb.pp(format!("controlnet_down_blocks.{i}")),
            )?);
        }
        Ok(Self {
            time_proj: Timesteps::new(TIME_CHANNELS, true, 0.0),
            time_embedding: TimestepEmbedding::new(
                vb.pp("time_embedding"),
                TIME_CHANNELS,
                TIME_EMBED_DIM,
            )?,
            add_1: nn::linear(2816, 1280, vb.pp("add_embedding.linear_1"))?,
            add_2: nn::linear(1280, 1280, vb.pp("add_embedding.linear_2"))?,
            control_add_1: nn::linear(2048, 1280, vb.pp("control_add_embedding.linear_1"))?,
            control_add_2: nn::linear(1280, 1280, vb.pp("control_add_embedding.linear_2"))?,
            cond_conv_in,
            cond_blocks,
            cond_conv_out,
            task_embedding: vb.get((8, 320), "task_embedding")?,
            type_block: TypeBlock::new(vb.clone())?,
            spatial_ch_projs: nn::linear(320, 320, vb.pp("spatial_ch_projs"))?,
            conv_in: nn::conv2d(4, 320, 3, pad1, vb.pp("conv_in"))?,
            down_0,
            down_1,
            down_2,
            mid,
            zero_convs,
            zero_mid: nn::conv2d(
                1280,
                1280,
                1,
                Default::default(),
                vb.pp("controlnet_mid_block"),
            )?,
        })
    }

    /// The per-channel type context `(320,)` for a union
    /// type: task row through the type transformer and the
    /// spatial projection.
    fn type_context(&self, cn_type: CnType) -> Result<Tensor> {
        let row = self.task_embedding.narrow(0, cn_type.index(), 1)?;
        let out = self.type_block.forward(&row)?;
        Ok(self.spatial_ch_projs.forward(&out)?.reshape(320)?)
    }

    /// Run the branch. `xs` is the sampler's preconditioned
    /// batch latent (f16), `pose` the conditioning map
    /// batch (f16, same batch size), `context` the text
    /// context (f16), `joined_added` the SDXL added input
    /// (pooled | time ids, f16) the trunk's own projection
    /// also consumes. Returns the nine down residuals and
    /// the mid residual, UNSCALED — the caller applies the
    /// run's strength and step window.
    ///
    /// # Errors
    /// Returns tensor errors typed by the crate enum.
    pub fn forward(
        &self,
        xs: &Tensor,
        timestep: f64,
        context: &Tensor,
        joined_added: &Tensor,
        pose: &Tensor,
        cn_type: CnType,
    ) -> Result<(Vec<Tensor>, Tensor)> {
        let (bsize, _, _, _) = xs.dims4()?;
        // Time embedding + the file's two added paths.
        let emb = (Tensor::ones(bsize, xs.dtype(), xs.device())? * timestep)?;
        let emb = self.time_proj.forward(&emb)?;
        let mut temb = self.time_embedding.forward(&emb)?;
        let added = self
            .add_2
            .forward(&nn::ops::silu(&self.add_1.forward(joined_added)?)?)?;
        temb = (temb + added)?;
        let ctx_mean = context
            .to_dtype(DType::F32)?
            .mean_keepdim(1)?
            .reshape((bsize, 2048))?
            .to_dtype(DType::F16)?;
        let control_added = self
            .control_add_2
            .forward(&nn::ops::silu(&self.control_add_1.forward(&ctx_mean)?)?)?;
        temb = (temb + control_added)?;
        // Conditioning embedding + type context.
        let mut cond = self.cond_conv_in.forward(pose)?;
        cond = nn::ops::silu(&cond)?;
        for block in &self.cond_blocks {
            cond = block.forward(&cond)?;
            cond = nn::ops::silu(&cond)?;
        }
        cond = self.cond_conv_out.forward(&cond)?;
        let type_ctx = self.type_context(cn_type)?;
        cond = cond.broadcast_add(&type_ctx.reshape((1, 320, 1, 1))?)?;
        // Encoder: states mirror the trunk's residual list.
        let mut sample = (self.conv_in.forward(xs)? + cond)?;
        let mut states = vec![sample.clone()];
        let (out, res) = self.down_0.forward(&sample, Some(&temb))?;
        sample = out;
        states.extend(res);
        let (out, res) = self.down_1.forward(&sample, Some(&temb), Some(context))?;
        sample = out;
        states.extend(res);
        let (out, res) = self.down_2.forward(&sample, Some(&temb), Some(context))?;
        sample = out;
        states.extend(res);
        assert_eq!(
            states.len(),
            ZERO_CHANNELS.len(),
            "controlnet state count must match the zero-conv count"
        );
        let mid = self.mid.forward(&sample, Some(&temb), Some(context))?;
        let mut residuals = Vec::with_capacity(states.len());
        for (zero, state) in self.zero_convs.iter().zip(states.iter()) {
            residuals.push(zero.forward(state)?);
        }
        let mid_residual = self.zero_mid.forward(&mid)?;
        Ok((residuals, mid_residual))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn residual_channels_match_the_trunk_list() {
        // Validation: conv_in + 3 + 3 + 2 states, channels
        // in trunk order (the shape-level mapping proof).
        assert_eq!(ZERO_CHANNELS.len(), 9);
        assert_eq!(ZERO_CHANNELS[0], 320);
        assert_eq!(ZERO_CHANNELS[8], 1280);
        // Conditioning embedding halves resolution three
        // times: image size -> latent size.
        let strides: usize = COND_BLOCKS.iter().map(|b| b.2).product();
        assert_eq!(strides, 8);
        assert_eq!(COND_BLOCKS[5].0, 256);
    }

    #[test]
    fn missing_controlnet_weights_are_a_conditioning_refusal() {
        // Adversarial: missing input refuses typed, early.
        let err = ControlNet::load(
            std::path::Path::new("/nonexistent/controlnet.safetensors"),
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
