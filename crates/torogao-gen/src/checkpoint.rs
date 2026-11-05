//! Single-file SDXL checkpoint loading (native-stack
//! P1): a ComfyUI/LDM-layout `.safetensors` checkpoint
//! (the format every estate checkpoint actually ships in)
//! is read directly into the four component tensor maps
//! the pipeline builds from — UNet, VAE, CLIP-L, CLIP-G —
//! with no on-disk conversion step.
//!
//! The name mapping is the Stage 1/2 converters' mapping
//! (proven tensor-for-tensor against the oracle on Iris
//! and WAI: 1,680 UNet tensors, zero unused), re-implemented
//! here as pure key plans over the checkpoint's header
//! census, so the plan itself is unit-testable without a
//! 6 GB fixture. The mapping is a format fact (LDM module
//! names vs diffusers module names), not oracle code.
//!
//! Strictness contract, mirroring the converters: every
//! tensor under `model.diffusion_model.` and
//! `first_stage_model.` must be consumed by the plan
//! exactly once — an unconsumed tensor means the mapping
//! and the file disagree, and the load is refused rather
//! than silently dropping weights. The two CLIP prefixes
//! are exempt from the consumption rule (they carry
//! non-transformer extras such as `logit_scale`); their
//! unconsumed names are reported instead. Names outside
//! all four prefixes are likewise reported, never loaded.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use candle::safetensors::MmapedSafetensors;
use candle::{Device, Tensor};
use serde::Serialize;

use crate::error::{GenError, Result};

/// LDM prefix of the diffusion model (UNet) tensors.
const P_UNET: &str = "model.diffusion_model.";
/// LDM prefix of the VAE tensors.
const P_VAE: &str = "first_stage_model.";
/// LDM prefix of the CLIP-L conditioner (HF layout inside).
const P_CLIP_L: &str = "conditioner.embedders.0.transformer.";
/// LDM prefix of the CLIP-G conditioner (OpenAI layout inside).
const P_CLIP_G: &str = "conditioner.embedders.1.model.";

/// What a checkpoint load consumed and what it left over.
#[derive(Debug, Clone, Serialize)]
pub struct CheckpointReport {
    /// Tensors routed to the UNet map.
    pub unet_tensors: usize,
    /// Tensors routed to the VAE map.
    pub vae_tensors: usize,
    /// Tensors routed to the CLIP-L map.
    pub clip_tensors: usize,
    /// Tensors routed to the CLIP-G map (an in_proj split
    /// counts once here, three times in the map).
    pub clip2_tensors: usize,
    /// Names under the CLIP prefixes the transformer does
    /// not consume (e.g. `logit_scale`), sorted.
    pub clip_unused: Vec<String>,
    /// Names outside all four component prefixes, sorted.
    pub outside_prefix: Vec<String>,
}

/// The four component tensor maps of one checkpoint,
/// keyed by the diffusers names the builders expect.
/// Tensors are CPU-resident in their stored dtype; the
/// builders' VarBuilders own the device/dtype transfer.
#[derive(Debug)]
pub struct CheckpointMaps {
    /// UNet tensors (diffusers names).
    pub unet: HashMap<String, Tensor>,
    /// VAE tensors (diffusers names).
    pub vae: HashMap<String, Tensor>,
    /// CLIP-L tensors (HF names).
    pub clip: HashMap<String, Tensor>,
    /// CLIP-G tensors (HF names; in_proj already split).
    pub clip2: HashMap<String, Tensor>,
    /// Consumption record of the load.
    pub report: CheckpointReport,
}

/// One planned routing: a source tensor lands under a
/// destination name, or a fused QKV tensor splits into
/// its three projections.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PlanEntry {
    /// `src` is loaded and stored under `dst`.
    Rename {
        /// Source name in the checkpoint.
        src: String,
        /// Destination name in the component map.
        dst: String,
    },
    /// `src` (CLIP-G `in_proj_weight`/`in_proj_bias`) is
    /// split into equal thirds along dim 0.
    SplitThirds {
        /// Source name in the checkpoint.
        src: String,
        /// Destination names for the q, k, v thirds.
        dst: [String; 3],
    },
    /// `src` is loaded, transposed, and stored under
    /// `dst` (the CLIP-G text projection: the checkpoint
    /// stores the OpenAI matrix; the diffusers-layout
    /// Linear consumes its transpose — proven by the
    /// Stage 1 Iris converter, whose transposed layout
    /// generates Iris-exact).
    Transpose {
        /// Source name in the checkpoint.
        src: String,
        /// Destination name in the component map.
        dst: String,
    },
}

impl PlanEntry {
    fn rename(src: String, dst: String) -> Self {
        Self::Rename { src, dst }
    }

    fn src(&self) -> &str {
        match self {
            Self::Rename { src, .. }
            | Self::SplitThirds { src, .. }
            | Self::Transpose { src, .. } => src,
        }
    }
}

/// Append `weight`/`bias` renames for one module pair.
fn push_wb(plan: &mut Vec<PlanEntry>, src_base: &str, dst_base: &str) {
    for wb in ["weight", "bias"] {
        plan.push(PlanEntry::rename(
            format!("{src_base}.{wb}"),
            format!("{dst_base}.{wb}"),
        ));
    }
}

/// The resnet routing shared by the UNet and VAE plans
/// (in the VAE the member names coincide, so the pair
/// list differs — the caller passes it).
fn plan_resnet(
    plan: &mut Vec<PlanEntry>,
    names: &HashSet<&str>,
    lp: &str,
    dp: &str,
    pairs: &[(&str, &str)],
    shortcut_src: &str,
    shortcut_dst: &str,
) {
    for (a, b) in pairs {
        push_wb(plan, &format!("{lp}.{a}"), &format!("{dp}.{b}"));
    }
    if names.contains(format!("{lp}.{shortcut_src}.weight").as_str()) {
        push_wb(
            plan,
            &format!("{lp}.{shortcut_src}"),
            &format!("{dp}.{shortcut_dst}"),
        );
    }
}

/// Transformer block indices present under `lp` in the
/// source census (`{lp}.transformer_blocks.{j}.`).
fn transformer_blocks(names: &HashSet<&str>, lp: &str) -> Vec<usize> {
    let prefix = format!("{lp}.transformer_blocks.");
    let mut idx: Vec<usize> = names
        .iter()
        .filter_map(|n| n.strip_prefix(&prefix))
        .filter_map(|rest| rest.split('.').next())
        .filter_map(|head| head.parse::<usize>().ok())
        .collect();
    idx.sort_unstable();
    idx.dedup();
    idx
}

/// The UNet transformer routing (LDM -> diffusers).
fn plan_transformer(plan: &mut Vec<PlanEntry>, names: &HashSet<&str>, lp: &str, dp: &str) {
    push_wb(plan, &format!("{lp}.norm"), &format!("{dp}.norm"));
    push_wb(plan, &format!("{lp}.proj_in"), &format!("{dp}.proj_in"));
    push_wb(plan, &format!("{lp}.proj_out"), &format!("{dp}.proj_out"));
    for j in transformer_blocks(names, lp) {
        let lq = format!("{lp}.transformer_blocks.{j}");
        let dq = format!("{dp}.transformer_blocks.{j}");
        for att in ["attn1", "attn2"] {
            for proj in ["to_q", "to_k", "to_v"] {
                plan.push(PlanEntry::rename(
                    format!("{lq}.{att}.{proj}.weight"),
                    format!("{dq}.{att}.{proj}.weight"),
                ));
            }
            push_wb(
                plan,
                &format!("{lq}.{att}.to_out.0"),
                &format!("{dq}.{att}.to_out.0"),
            );
        }
        for n in ["norm1", "norm2", "norm3"] {
            push_wb(plan, &format!("{lq}.{n}"), &format!("{dq}.{n}"));
        }
        push_wb(
            plan,
            &format!("{lq}.ff.net.0.proj"),
            &format!("{dq}.ff.net.0.proj"),
        );
        push_wb(plan, &format!("{lq}.ff.net.2"), &format!("{dq}.ff.net.2"));
    }
}

/// The UNet key plan: every `model.diffusion_model.`
/// tensor routed to its diffusers name.
fn plan_unet(names: &HashSet<&str>) -> Vec<PlanEntry> {
    const RESNET_PAIRS: [(&str, &str); 5] = [
        ("in_layers.0", "norm1"),
        ("in_layers.2", "conv1"),
        ("emb_layers.1", "time_emb_proj"),
        ("out_layers.0", "norm2"),
        ("out_layers.3", "conv2"),
    ];
    let p = "model.diffusion_model";
    let mut plan = Vec::new();
    push_wb(
        &mut plan,
        &format!("{p}.time_embed.0"),
        "time_embedding.linear_1",
    );
    push_wb(
        &mut plan,
        &format!("{p}.time_embed.2"),
        "time_embedding.linear_2",
    );
    push_wb(
        &mut plan,
        &format!("{p}.label_emb.0.0"),
        "add_embedding.linear_1",
    );
    push_wb(
        &mut plan,
        &format!("{p}.label_emb.0.2"),
        "add_embedding.linear_2",
    );
    push_wb(&mut plan, &format!("{p}.input_blocks.0.0"), "conv_in");
    push_wb(&mut plan, &format!("{p}.out.0"), "conv_norm_out");
    push_wb(&mut plan, &format!("{p}.out.2"), "conv_out");
    // Down path: (stage, input block) in walk order; the
    // per-stage resnet counter is implicit in the order.
    let seq = [(0usize, 1usize), (0, 2), (1, 4), (1, 5), (2, 7), (2, 8)];
    let mut res_i = [0usize; 3];
    for (stage, ib) in seq {
        let r = res_i[stage];
        res_i[stage] += 1;
        plan_resnet(
            &mut plan,
            names,
            &format!("{p}.input_blocks.{ib}.0"),
            &format!("down_blocks.{stage}.resnets.{r}"),
            &RESNET_PAIRS,
            "skip_connection",
            "conv_shortcut",
        );
        if stage > 0 {
            plan_transformer(
                &mut plan,
                names,
                &format!("{p}.input_blocks.{ib}.1"),
                &format!("down_blocks.{stage}.attentions.{r}"),
            );
        }
    }
    for (stage, ib) in [(0usize, 3usize), (1, 6)] {
        push_wb(
            &mut plan,
            &format!("{p}.input_blocks.{ib}.0.op"),
            &format!("down_blocks.{stage}.downsamplers.0.conv"),
        );
    }
    plan_resnet(
        &mut plan,
        names,
        &format!("{p}.middle_block.0"),
        "mid_block.resnets.0",
        &RESNET_PAIRS,
        "skip_connection",
        "conv_shortcut",
    );
    plan_transformer(
        &mut plan,
        names,
        &format!("{p}.middle_block.1"),
        "mid_block.attentions.0",
    );
    plan_resnet(
        &mut plan,
        names,
        &format!("{p}.middle_block.2"),
        "mid_block.resnets.1",
        &RESNET_PAIRS,
        "skip_connection",
        "conv_shortcut",
    );
    // Up path: output block ob = stage * 3 + resnet.
    for stage in 0..3usize {
        for r in 0..3usize {
            let ob = stage * 3 + r;
            plan_resnet(
                &mut plan,
                names,
                &format!("{p}.output_blocks.{ob}.0"),
                &format!("up_blocks.{stage}.resnets.{r}"),
                &RESNET_PAIRS,
                "skip_connection",
                "conv_shortcut",
            );
            if stage < 2 {
                plan_transformer(
                    &mut plan,
                    names,
                    &format!("{p}.output_blocks.{ob}.1"),
                    &format!("up_blocks.{stage}.attentions.{r}"),
                );
                if r == 2 {
                    push_wb(
                        &mut plan,
                        &format!("{p}.output_blocks.{ob}.2.conv"),
                        &format!("up_blocks.{stage}.upsamplers.0.conv"),
                    );
                }
            }
        }
    }
    plan
}

/// The VAE key plan: every `first_stage_model.` tensor
/// routed to its diffusers name. Note the decoder up
/// reversal: LDM lists decoder.up finest-first, diffusers
/// up_blocks run coarsest-first (the Stage 1 converter's
/// measured orientation).
fn plan_vae(names: &HashSet<&str>) -> Vec<PlanEntry> {
    const RESNET_PAIRS: [(&str, &str); 4] = [
        ("norm1", "norm1"),
        ("conv1", "conv1"),
        ("norm2", "norm2"),
        ("conv2", "conv2"),
    ];
    let p = "first_stage_model";
    let mut plan = Vec::new();
    for (src, dst) in [
        ("encoder.conv_in", "encoder.conv_in"),
        ("encoder.norm_out", "encoder.conv_norm_out"),
        ("encoder.conv_out", "encoder.conv_out"),
        ("decoder.conv_in", "decoder.conv_in"),
        ("decoder.norm_out", "decoder.conv_norm_out"),
        ("decoder.conv_out", "decoder.conv_out"),
        ("quant_conv", "quant_conv"),
        ("post_quant_conv", "post_quant_conv"),
    ] {
        push_wb(&mut plan, &format!("{p}.{src}"), dst);
    }
    for i in 0..4usize {
        for j in 0..2usize {
            plan_resnet(
                &mut plan,
                names,
                &format!("{p}.encoder.down.{i}.block.{j}"),
                &format!("encoder.down_blocks.{i}.resnets.{j}"),
                &RESNET_PAIRS,
                "nin_shortcut",
                "conv_shortcut",
            );
        }
        if i < 3 {
            push_wb(
                &mut plan,
                &format!("{p}.encoder.down.{i}.downsample.conv"),
                &format!("encoder.down_blocks.{i}.downsamplers.0.conv"),
            );
        }
    }
    for side in ["encoder", "decoder"] {
        plan_resnet(
            &mut plan,
            names,
            &format!("{p}.{side}.mid.block_1"),
            &format!("{side}.mid_block.resnets.0"),
            &RESNET_PAIRS,
            "nin_shortcut",
            "conv_shortcut",
        );
        plan_resnet(
            &mut plan,
            names,
            &format!("{p}.{side}.mid.block_2"),
            &format!("{side}.mid_block.resnets.1"),
            &RESNET_PAIRS,
            "nin_shortcut",
            "conv_shortcut",
        );
        let a = format!("{p}.{side}.mid.attn_1");
        let d = format!("{side}.mid_block.attentions.0");
        push_wb(&mut plan, &format!("{a}.norm"), &format!("{d}.group_norm"));
        push_wb(&mut plan, &format!("{a}.q"), &format!("{d}.query"));
        push_wb(&mut plan, &format!("{a}.k"), &format!("{d}.key"));
        push_wb(&mut plan, &format!("{a}.v"), &format!("{d}.value"));
        push_wb(
            &mut plan,
            &format!("{a}.proj_out"),
            &format!("{d}.proj_attn"),
        );
    }
    for i in 0..4usize {
        let src_i = 3 - i;
        for j in 0..3usize {
            plan_resnet(
                &mut plan,
                names,
                &format!("{p}.decoder.up.{src_i}.block.{j}"),
                &format!("decoder.up_blocks.{i}.resnets.{j}"),
                &RESNET_PAIRS,
                "nin_shortcut",
                "conv_shortcut",
            );
        }
        if names.contains(format!("{p}.decoder.up.{src_i}.upsample.conv.weight").as_str()) {
            push_wb(
                &mut plan,
                &format!("{p}.decoder.up.{src_i}.upsample.conv"),
                &format!("decoder.up_blocks.{i}.upsamplers.0.conv"),
            );
        }
    }
    plan
}

/// The CLIP-L key plan: strip the conditioner prefix;
/// the contents are already HF layout. The trailing
/// `position_ids` buffer is not a transformer weight and
/// is left unconsumed (reported, like the converters).
fn plan_clip_l(names: &HashSet<&str>) -> Vec<PlanEntry> {
    names
        .iter()
        .filter(|n| n.starts_with(P_CLIP_L) && !n.ends_with("position_ids"))
        .map(|n| PlanEntry::rename((*n).to_string(), n[P_CLIP_L.len()..].to_string()))
        .collect()
}

/// CLIP-G resblock indices present under
/// `conditioner.embedders.1.model.transformer.resblocks.`.
fn resblock_indices(names: &HashSet<&str>) -> Vec<usize> {
    let prefix = "conditioner.embedders.1.model.transformer.resblocks.";
    let mut idx: Vec<usize> = names
        .iter()
        .filter_map(|n| n.strip_prefix(prefix))
        .filter_map(|rest| rest.split('.').next())
        .filter_map(|head| head.parse::<usize>().ok())
        .collect();
    idx.sort_unstable();
    idx.dedup();
    idx
}

/// The CLIP-G key plan: OpenAI resblocks layout routed
/// to HF names, with the fused in-projection split.
fn plan_clip_g(names: &HashSet<&str>) -> Vec<PlanEntry> {
    let pre = "conditioner.embedders.1.model";
    let mut plan = Vec::new();
    plan.push(PlanEntry::rename(
        format!("{pre}.token_embedding.weight"),
        "text_model.embeddings.token_embedding.weight".to_string(),
    ));
    plan.push(PlanEntry::rename(
        format!("{pre}.positional_embedding"),
        "text_model.embeddings.position_embedding.weight".to_string(),
    ));
    push_wb(
        &mut plan,
        &format!("{pre}.ln_final"),
        "text_model.final_layer_norm",
    );
    plan.push(PlanEntry::Transpose {
        src: format!("{pre}.text_projection"),
        dst: "text_projection.weight".to_string(),
    });
    for i in resblock_indices(names) {
        let lp = format!("{pre}.transformer.resblocks.{i}");
        let dp = format!("text_model.encoder.layers.{i}");
        for wb in ["weight", "bias"] {
            plan.push(PlanEntry::rename(
                format!("{lp}.ln_1.{wb}"),
                format!("{dp}.layer_norm1.{wb}"),
            ));
            plan.push(PlanEntry::rename(
                format!("{lp}.ln_2.{wb}"),
                format!("{dp}.layer_norm2.{wb}"),
            ));
            plan.push(PlanEntry::rename(
                format!("{lp}.attn.out_proj.{wb}"),
                format!("{dp}.self_attn.out_proj.{wb}"),
            ));
            plan.push(PlanEntry::rename(
                format!("{lp}.mlp.c_fc.{wb}"),
                format!("{dp}.mlp.fc1.{wb}"),
            ));
            plan.push(PlanEntry::rename(
                format!("{lp}.mlp.c_proj.{wb}"),
                format!("{dp}.mlp.fc2.{wb}"),
            ));
        }
        for (src, dsts) in [
            (
                "attn.in_proj_weight",
                [
                    "self_attn.q_proj.weight",
                    "self_attn.k_proj.weight",
                    "self_attn.v_proj.weight",
                ],
            ),
            (
                "attn.in_proj_bias",
                [
                    "self_attn.q_proj.bias",
                    "self_attn.k_proj.bias",
                    "self_attn.v_proj.bias",
                ],
            ),
        ] {
            plan.push(PlanEntry::SplitThirds {
                src: format!("{lp}.{src}"),
                dst: dsts.map(|d| format!("{dp}.{d}")),
            });
        }
    }
    plan
}

/// Execute one component plan against the opened
/// checkpoint, verifying the strictness contract for
/// `prefix` when `strict` holds: every source under the
/// prefix must be planned exactly once. Returns the map
/// plus the unconsumed names under the prefix.
fn execute_plan(
    mmap: &MmapedSafetensors,
    names: &HashSet<&str>,
    plan: &[PlanEntry],
    prefix: &str,
    strict: bool,
    component: &'static str,
) -> Result<(HashMap<String, Tensor>, Vec<String>)> {
    let mut planned: HashSet<&str> = HashSet::new();
    for e in plan {
        if !names.contains(e.src()) {
            return Err(GenError::Weights {
                detail: format!("{component}: checkpoint is missing tensor {}", e.src()),
            });
        }
        if !planned.insert(e.src()) {
            return Err(GenError::Weights {
                detail: format!("{component}: plan routes {} twice", e.src()),
            });
        }
    }
    let unconsumed: Vec<String> = names
        .iter()
        .filter(|n| n.starts_with(prefix) && !planned.contains(**n))
        .map(|n| (*n).to_string())
        .collect();
    if strict && !unconsumed.is_empty() {
        return Err(GenError::Weights {
            detail: format!(
                "{component}: {} tensors under {prefix} are not routed by the mapping \
                 (first: {}) — the mapping and the checkpoint disagree",
                unconsumed.len(),
                unconsumed[0]
            ),
        });
    }
    let cpu = Device::Cpu;
    let mut map = HashMap::with_capacity(plan.len());
    for e in plan {
        match e {
            PlanEntry::Rename { src, dst } => {
                let t = mmap.load(src, &cpu).map_err(|err| GenError::Weights {
                    detail: format!("{component}: {src}: {err}"),
                })?;
                map.insert(dst.clone(), t);
            }
            PlanEntry::Transpose { src, dst } => {
                let t = mmap.load(src, &cpu).map_err(|err| GenError::Weights {
                    detail: format!("{component}: {src}: {err}"),
                })?;
                map.insert(dst.clone(), t.t()?.contiguous()?);
            }
            PlanEntry::SplitThirds { src, dst } => {
                let t = mmap.load(src, &cpu).map_err(|err| GenError::Weights {
                    detail: format!("{component}: {src}: {err}"),
                })?;
                let rows = t.dim(0)?;
                if rows % 3 != 0 {
                    return Err(GenError::Weights {
                        detail: format!("{component}: {src} has {rows} rows, not divisible by 3"),
                    });
                }
                let third = rows / 3;
                for (k, d) in dst.iter().enumerate() {
                    map.insert(d.clone(), t.narrow(0, k * third, third)?.contiguous()?);
                }
            }
        }
    }
    Ok((map, unconsumed))
}

/// Load a single-file SDXL checkpoint into its four
/// component tensor maps.
///
/// # Errors
/// Returns `gen_weights` when the file cannot be
/// memory-mapped, when a tensor the mapping requires is
/// absent, or when the strict consumption contract for
/// the UNet/VAE prefixes fails.
pub fn load_checkpoint(path: &Path) -> Result<CheckpointMaps> {
    // SAFETY: read-only mapping of a weights file this
    // process owns for the run's duration; never mutated.
    #[allow(unsafe_code)]
    let mmap = unsafe { MmapedSafetensors::new(path) }.map_err(|e| GenError::Weights {
        detail: format!("{}: {e}", path.display()),
    })?;
    let tensors = mmap.tensors();
    let names: HashSet<&str> = tensors.iter().map(|(n, _)| n.as_str()).collect();
    let (unet, _) = execute_plan(&mmap, &names, &plan_unet(&names), P_UNET, true, "unet")?;
    let (vae, _) = execute_plan(&mmap, &names, &plan_vae(&names), P_VAE, true, "vae")?;
    let (clip, mut clip_unused) =
        execute_plan(&mmap, &names, &plan_clip_l(&names), P_CLIP_L, false, "clip")?;
    let (clip2, clip2_unused) = execute_plan(
        &mmap,
        &names,
        &plan_clip_g(&names),
        P_CLIP_G,
        false,
        "clip2",
    )?;
    clip_unused.extend(clip2_unused);
    clip_unused.sort();
    let mut outside_prefix: Vec<String> = names
        .iter()
        .filter(|n| {
            !n.starts_with(P_UNET)
                && !n.starts_with(P_VAE)
                && !n.starts_with(P_CLIP_L)
                && !n.starts_with(P_CLIP_G)
        })
        .map(|n| (*n).to_string())
        .collect();
    outside_prefix.sort();
    let report = CheckpointReport {
        unet_tensors: plan_unet(&names).len(),
        vae_tensors: plan_vae(&names).len(),
        clip_tensors: plan_clip_l(&names).len(),
        clip2_tensors: plan_clip_g(&names).len(),
        clip_unused,
        outside_prefix,
    };
    Ok(CheckpointMaps {
        unet,
        vae,
        clip,
        clip2,
        report,
    })
}

/// Load one component's tensors from a converted-layout
/// directory file (the P1 parity arm's own format, and
/// the map source when LoRAs are merged into a layout
/// load).
///
/// # Errors
/// Returns `gen_weights` on a missing or unreadable file.
pub fn load_layout_file(path: &Path) -> Result<HashMap<String, Tensor>> {
    candle::safetensors::load(path, &Device::Cpu).map_err(|e| GenError::Weights {
        detail: format!("{}: {e}", path.display()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic census carrying the structural keys
    /// the plans read (block indices), with filler names
    /// for the routed tensors the plans generate. The
    /// fixture's purpose is plan shape: counts and spot
    /// pairs, not tensor data.
    fn synthetic_census() -> HashSet<&'static str> {
        let mut names: HashSet<&'static str> = HashSet::new();
        // CLIP-G resblocks 0..=1 under the OpenAI prefix.
        names.insert("conditioner.embedders.1.model.transformer.resblocks.0.attn.in_proj_weight");
        names.insert("conditioner.embedders.1.model.transformer.resblocks.1.attn.in_proj_weight");
        names
    }

    #[test]
    fn clip_g_plan_splits_in_proj_and_routes() {
        let names = synthetic_census();
        let plan = plan_clip_g(&names);
        let splits: Vec<&PlanEntry> = plan
            .iter()
            .filter(|e| matches!(e, PlanEntry::SplitThirds { .. }))
            .collect();
        // Two resblocks x (weight + bias) splits.
        assert_eq!(splits.len(), 4);
        let dsts: Vec<&str> = plan
            .iter()
            .filter_map(|e| match e {
                PlanEntry::Rename { dst, .. } | PlanEntry::Transpose { dst, .. } => {
                    Some(dst.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(dsts.contains(&"text_model.embeddings.token_embedding.weight"));
        assert!(dsts.contains(&"text_projection.weight"));
        assert!(dsts.contains(&"text_model.encoder.layers.1.mlp.fc2.bias"));
        // The text projection is the plan's one transpose:
        // the checkpoint stores the OpenAI matrix, the
        // diffusers-layout Linear wants its transpose.
        let transposes: Vec<&PlanEntry> = plan
            .iter()
            .filter(|e| matches!(e, PlanEntry::Transpose { .. }))
            .collect();
        assert_eq!(transposes.len(), 1);
        assert_eq!(
            transposes[0].src(),
            "conditioner.embedders.1.model.text_projection"
        );
    }

    #[test]
    fn unet_plan_routes_the_fixed_points() {
        // The plan's fixed points do not depend on the
        // census (only transformer block indices do), so
        // an empty census still yields them.
        let names: HashSet<&str> = HashSet::new();
        let plan = plan_unet(&names);
        let pairs: HashSet<(&str, &str)> = plan
            .iter()
            .filter_map(|e| match e {
                PlanEntry::Rename { src, dst } => Some((src.as_str(), dst.as_str())),
                _ => None,
            })
            .collect();
        assert!(pairs.contains(&(
            "model.diffusion_model.input_blocks.0.0.weight",
            "conv_in.weight"
        )));
        assert!(pairs.contains(&(
            "model.diffusion_model.label_emb.0.2.bias",
            "add_embedding.linear_2.bias"
        )));
        assert!(pairs.contains(&(
            "model.diffusion_model.input_blocks.3.0.op.weight",
            "down_blocks.0.downsamplers.0.conv.weight"
        )));
        assert!(pairs.contains(&(
            "model.diffusion_model.output_blocks.8.0.in_layers.2.weight",
            "up_blocks.2.resnets.2.conv1.weight"
        )));
    }

    #[test]
    fn vae_plan_reverses_decoder_up() {
        let names: HashSet<&str> = HashSet::new();
        let plan = plan_vae(&names);
        let pairs: HashSet<(&str, &str)> = plan
            .iter()
            .filter_map(|e| match e {
                PlanEntry::Rename { src, dst } => Some((src.as_str(), dst.as_str())),
                _ => None,
            })
            .collect();
        // LDM up.3 (finest) lands in diffusers up_blocks.0.
        assert!(pairs.contains(&(
            "first_stage_model.decoder.up.3.block.0.conv1.weight",
            "decoder.up_blocks.0.resnets.0.conv1.weight"
        )));
        assert!(pairs.contains(&(
            "first_stage_model.decoder.up.0.block.2.conv2.bias",
            "decoder.up_blocks.3.resnets.2.conv2.bias"
        )));
        assert!(pairs.contains(&(
            "first_stage_model.encoder.mid.attn_1.q.weight",
            "encoder.mid_block.attentions.0.query.weight"
        )));
    }
}
