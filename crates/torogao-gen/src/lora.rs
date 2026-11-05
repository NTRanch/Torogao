//! LoRA loading and weight-space merging (native-stack
//! P3): kohya-format LoRA files (`networks.lora`, the
//! format every estate LoRA ships in) are merged into
//! the base component maps at load time —
//! `merged = base + (alpha / rank) * scale * (up @ down)`
//! per targeted module, computed in f32 and cast back to
//! the base tensor's dtype. Weight-space merge at a fixed
//! strength is the same linear map the oracle's dynamic
//! patching applies per forward; the merge is done once,
//! here, so the sampler never knows LoRAs exist.
//!
//! Name routing is a format fact, censused from the
//! estate files (Crossed_Rolled_Eyes_SDXL: 986 modules;
//! orgasmic_heartblast: 722): UNet modules use LDM names
//! (`lora_unet_input_blocks_4_1_transformer_blocks_0_attn1_to_q`)
//! and route through the same LDM -> diffusers block
//! structure as the checkpoint mapping; text modules use
//! `lora_te1_` / `lora_te2_` over HF names. Only
//! `lora_down`/`lora_up` factorisations (LoRA / LoCon)
//! are supported; any other factorisation is refused by
//! name. A module whose target weight is absent from the
//! component map is skipped and named in the report —
//! never silently dropped, never an error: LoRA files
//! legitimately carry modules for blocks a given base
//! may not expose (e.g. stage-0 attention that SDXL's
//! first down block does not have).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use candle::safetensors::MmapedSafetensors;
use candle::{DType, Device, Tensor};
use serde::Serialize;

use crate::conditioning::sha256_file;
use crate::error::{GenError, Result};
use crate::limits;

/// One LoRA file's participation in a load.
#[derive(Debug, Clone)]
pub struct LoraSpec {
    /// The LoRA safetensors file.
    pub path: PathBuf,
    /// Merge strength (the oracle's LoRA weight).
    pub scale: f64,
}

/// Which component map a LoRA module targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoraComponent {
    /// The UNet map.
    Unet,
    /// The CLIP-L map (`lora_te1_` modules).
    ClipL,
    /// The CLIP-G map (`lora_te2_` modules).
    ClipG,
}

/// One factorised LoRA module.
#[derive(Debug)]
struct LoraModule {
    /// The kohya module name (no `.lora_down.weight`
    /// suffix) as written in the file.
    name: String,
    /// Which prefix family the module came from.
    component: LoraComponent,
    /// The down projection, `(rank, in...)`.
    down: Tensor,
    /// The up projection, `(out..., rank)`.
    up: Tensor,
    /// The module's alpha, when the file carries one.
    alpha: Option<f64>,
}

/// A parsed LoRA file, ready to merge.
#[derive(Debug)]
pub struct LoadedLora {
    /// SHA-256 of the file bytes.
    pub sha256: String,
    modules: Vec<LoraModule>,
}

/// The outcome of merging one LoRA into one component.
#[derive(Debug, Clone, Serialize)]
pub struct LoraApplyReport {
    /// LoRA file path as given.
    pub path: String,
    /// SHA-256 of the file bytes.
    pub sha256: String,
    /// Merge strength applied.
    pub scale: f64,
    /// Modules the file carries.
    pub modules_total: usize,
    /// Modules merged into a base weight.
    pub applied: usize,
    /// Module names (kohya spelling) whose target weight
    /// was absent from the component map.
    pub skipped: Vec<String>,
}

/// Parse a kohya LoRA file.
///
/// # Errors
/// Returns `gen_weights` when the file cannot be mapped,
/// when a module lacks its down/up pair, when a module
/// uses an unsupported factorisation, or when the module
/// count or a rank exceeds the limits module bounds.
pub fn load_lora(path: &Path) -> Result<LoadedLora> {
    let sha256 = sha256_file(path)?;
    // SAFETY: read-only mapping, as in checkpoint loads.
    #[allow(unsafe_code)]
    let mmap = unsafe { MmapedSafetensors::new(path) }.map_err(|e| GenError::Weights {
        detail: format!("{}: {e}", path.display()),
    })?;
    let names: Vec<String> = mmap.tensors().iter().map(|(n, _)| n.clone()).collect();
    let mut bases: Vec<String> = names
        .iter()
        .filter_map(|n| n.strip_suffix(".lora_down.weight").map(str::to_string))
        .collect();
    bases.sort();
    if bases.is_empty() {
        return Err(GenError::Weights {
            detail: format!(
                "{}: no lora_down modules — not a kohya LoRA file",
                path.display()
            ),
        });
    }
    if bases.len() > limits::LORA_MODULES_MAX {
        return Err(GenError::Weights {
            detail: format!(
                "{}: {} modules exceeds the {} bound",
                path.display(),
                bases.len(),
                limits::LORA_MODULES_MAX
            ),
        });
    }
    // Any *.lora_* tensor that is not a down/up pair is a
    // factorisation this loader does not implement
    // (LoHa/LoKr carry .hada_*/.lokr_* names).
    for n in &names {
        if n.contains(".lora_")
            && !n.ends_with(".lora_down.weight")
            && !n.ends_with(".lora_up.weight")
        {
            return Err(GenError::Weights {
                detail: format!("{}: unsupported LoRA factorisation at {n}", path.display()),
            });
        }
    }
    let cpu = Device::Cpu;
    let mut modules = Vec::with_capacity(bases.len());
    for base in bases {
        let component = if base.starts_with("lora_unet_") {
            LoraComponent::Unet
        } else if base.starts_with("lora_te1_") {
            LoraComponent::ClipL
        } else if base.starts_with("lora_te2_") {
            LoraComponent::ClipG
        } else {
            return Err(GenError::Weights {
                detail: format!("{}: unknown LoRA module prefix at {base}", path.display()),
            });
        };
        let load = |suffix: &str| -> Result<Tensor> {
            let key = format!("{base}.{suffix}");
            mmap.load(&key, &cpu).map_err(|e| GenError::Weights {
                detail: format!("{key}: {e}"),
            })
        };
        let down = load("lora_down.weight")?;
        let up = load("lora_up.weight")?;
        let rank = down.dim(0)?;
        if rank == 0 || rank > limits::LORA_RANK_MAX {
            return Err(GenError::Weights {
                detail: format!("{base}: rank {rank} outside 1..={}", limits::LORA_RANK_MAX),
            });
        }
        let alpha = match mmap.load(&format!("{base}.alpha"), &cpu) {
            // Alpha arrives in the file's dtype (BF16 in
            // the estate's kohya exports); read it through
            // an F32 cast, never a typed read.
            Ok(t) => Some(
                t.to_dtype(candle::DType::F32)?
                    .flatten_all()?
                    .to_vec1::<f32>()?[0] as f64,
            ),
            Err(_) => None,
        };
        modules.push(LoraModule {
            name: base,
            component,
            down,
            up,
            alpha,
        });
    }
    Ok(LoadedLora { sha256, modules })
}

/// Route a UNet (LDM) module name to its diffusers
/// module name, or `None` when the module addresses a
/// block the SDXL UNet does not expose to LoRA here
/// (stage-0 attention, resnet members).
fn unet_module_dotted(name: &str) -> Option<String> {
    let rest = name.strip_prefix("lora_unet_")?;
    let transformer_rest = |tail: &str| -> Option<String> {
        if tail == "proj_in" {
            return Some("proj_in".to_string());
        }
        if tail == "proj_out" {
            return Some("proj_out".to_string());
        }
        let t = tail.strip_prefix("transformer_blocks_")?;
        let (j, tail) = t.split_once('_')?;
        if let Some(att) = tail.strip_prefix("attn") {
            let (a, proj) = att.split_once('_')?;
            let proj = match proj {
                "to_q" => "to_q".to_string(),
                "to_k" => "to_k".to_string(),
                "to_v" => "to_v".to_string(),
                "to_out_0" => "to_out.0".to_string(),
                _ => return None,
            };
            return Some(format!("transformer_blocks.{j}.attn{a}.{proj}"));
        }
        match tail {
            "ff_net_0_proj" => Some(format!("transformer_blocks.{j}.ff.net.0.proj")),
            "ff_net_2" => Some(format!("transformer_blocks.{j}.ff.net.2")),
            _ => None,
        }
    };
    if let Some(tail) = rest.strip_prefix("middle_block_1_") {
        return Some(format!(
            "mid_block.attentions.0.{}",
            transformer_rest(tail)?
        ));
    }
    for (family, stage_base) in [("input_blocks_", "down"), ("output_blocks_", "up")] {
        if let Some(tail) = rest.strip_prefix(family) {
            let (block, tail) = tail.split_once('_')?;
            let (sub, tail) = tail.split_once('_').unwrap_or((tail, ""));
            let block: usize = block.parse().ok()?;
            if sub != "1" {
                return None;
            }
            let base = if family == "input_blocks_" {
                let (stage, r) = match block {
                    4 => (1, 0),
                    5 => (1, 1),
                    7 => (2, 0),
                    8 => (2, 1),
                    _ => return None,
                };
                format!("{stage_base}_blocks.{stage}.attentions.{r}")
            } else {
                let stage = block / 3;
                let r = block % 3;
                if stage >= 2 {
                    return None;
                }
                format!("{stage_base}_blocks.{stage}.attentions.{r}")
            };
            // proj_in/proj_out ride directly on the
            // transformer container in kohya naming for
            // these blocks; everything deeper goes through
            // the transformer-rest routing.
            let dotted_tail = if tail == "proj_in" || tail == "proj_out" {
                tail.to_string()
            } else {
                transformer_rest(tail)?
            };
            return Some(format!("{base}.{dotted_tail}"));
        }
    }
    None
}

/// Route a text-encoder module name (`lora_te1_` /
/// `lora_te2_`) to its HF module name.
fn text_module_dotted(name: &str) -> Option<String> {
    let rest = name
        .strip_prefix("lora_te1_")
        .or_else(|| name.strip_prefix("lora_te2_"))?;
    let rest = rest.strip_prefix("text_model_encoder_layers_")?;
    let (layer, tail) = rest.split_once('_')?;
    let tail = tail
        .strip_prefix("self_attn_")
        .map(|p| format!("self_attn.{p}"))
        .or_else(|| tail.strip_prefix("mlp_").map(|p| format!("mlp.{p}")))?;
    Some(format!("text_model.encoder.layers.{layer}.{tail}"))
}

/// Route any module name to its dotted target.
fn module_dotted(component: LoraComponent, name: &str) -> Option<String> {
    match component {
        LoraComponent::Unet => unet_module_dotted(name),
        LoraComponent::ClipL | LoraComponent::ClipG => text_module_dotted(name),
    }
}

impl LoadedLora {
    /// Total modules the file carries.
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Merge every module addressed to `component` into
    /// that component's tensor map at `scale`.
    ///
    /// # Errors
    /// Returns `gen_weights` when a targeted module's
    /// factor shapes do not conform to its base weight.
    pub fn apply(
        &self,
        component: LoraComponent,
        map: &mut HashMap<String, Tensor>,
        scale: f64,
        path: &Path,
    ) -> Result<LoraApplyReport> {
        let mut applied = 0usize;
        let mut skipped: Vec<String> = Vec::new();
        for m in self.modules.iter().filter(|m| m.component == component) {
            let dotted = module_dotted(component, &m.name);
            let target = dotted.as_ref().map(|d| format!("{d}.weight"));
            let Some(target) = target.filter(|t| map.contains_key(t)) else {
                skipped.push(m.name.clone());
                continue;
            };
            let base = map.get(&target).expect("presence checked").clone();
            let base32 = base.to_dtype(DType::F32)?;
            let down32 = m.down.to_dtype(DType::F32)?;
            let up32 = m.up.to_dtype(DType::F32)?;
            let rank = down32.dim(0)? as f64;
            let down2 = down32.reshape((rank as usize, down32.elem_count() / rank as usize))?;
            let up2 = up32.reshape((up32.elem_count() / rank as usize, rank as usize))?;
            let delta = up2.matmul(&down2)?;
            let delta = if delta.dims() == base32.dims() {
                delta
            } else {
                delta
                    .reshape(base32.shape().clone())
                    .map_err(|_| GenError::Weights {
                        detail: format!(
                            "{}: LoRA delta for {target} has shape {:?}, base {:?}",
                            m.name,
                            delta.dims(),
                            base32.dims()
                        ),
                    })?
            };
            let factor = scale * m.alpha.unwrap_or(rank) / rank;
            let merged = (base32 + (delta * factor)?)?.to_dtype(base.dtype())?;
            map.insert(target, merged);
            applied += 1;
        }
        Ok(LoraApplyReport {
            path: path.display().to_string(),
            sha256: self.sha256.clone(),
            scale,
            modules_total: self.modules.len(),
            applied,
            skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unet_names_route_through_block_structure() {
        assert_eq!(
            unet_module_dotted("lora_unet_input_blocks_4_1_transformer_blocks_0_attn1_to_q"),
            Some("down_blocks.1.attentions.0.transformer_blocks.0.attn1.to_q".to_string())
        );
        assert_eq!(
            unet_module_dotted("lora_unet_input_blocks_8_1_transformer_blocks_0_attn2_to_out_0"),
            Some("down_blocks.2.attentions.1.transformer_blocks.0.attn2.to_out.0".to_string())
        );
        assert_eq!(
            unet_module_dotted("lora_unet_output_blocks_5_1_transformer_blocks_0_ff_net_0_proj"),
            Some("up_blocks.1.attentions.2.transformer_blocks.0.ff.net.0.proj".to_string())
        );
        assert_eq!(
            unet_module_dotted("lora_unet_middle_block_1_transformer_blocks_0_ff_net_2"),
            Some("mid_block.attentions.0.transformer_blocks.0.ff.net.2".to_string())
        );
        assert_eq!(
            unet_module_dotted("lora_unet_input_blocks_4_1_proj_in"),
            Some("down_blocks.1.attentions.0.proj_in".to_string())
        );
        // Stage-0 attention does not exist in SDXL's
        // first down block; resnet members are unrouted.
        assert_eq!(
            unet_module_dotted("lora_unet_input_blocks_1_1_proj_in"),
            None
        );
        assert_eq!(
            unet_module_dotted("lora_unet_middle_block_0_in_layers_2"),
            None
        );
    }

    #[test]
    fn text_names_route() {
        assert_eq!(
            text_module_dotted("lora_te1_text_model_encoder_layers_0_self_attn_k_proj"),
            Some("text_model.encoder.layers.0.self_attn.k_proj".to_string())
        );
        assert_eq!(
            text_module_dotted("lora_te2_text_model_encoder_layers_31_mlp_fc2"),
            Some("text_model.encoder.layers.31.mlp.fc2".to_string())
        );
    }

    #[test]
    fn merge_math_matches_hand_computation() {
        // Base identity 2x2; down = [[1, 0]] (rank 1),
        // up = [[0], [2]]; alpha 1, scale 1:
        // delta = up @ down = [[0, 0], [2, 0]].
        let dev = Device::Cpu;
        let base = Tensor::new(vec![1.0f32, 0.0, 0.0, 1.0], &dev)
            .unwrap()
            .reshape((2, 2))
            .unwrap();
        let mut map = HashMap::new();
        map.insert(
            "text_model.encoder.layers.0.self_attn.k_proj.weight".to_string(),
            base,
        );
        let lora = LoadedLora {
            sha256: "test".to_string(),
            modules: vec![LoraModule {
                name: "lora_te1_text_model_encoder_layers_0_self_attn_k_proj".to_string(),
                component: LoraComponent::ClipL,
                down: Tensor::new(vec![1.0f32, 0.0], &dev)
                    .unwrap()
                    .reshape((1, 2))
                    .unwrap(),
                up: Tensor::new(vec![0.0f32, 2.0], &dev)
                    .unwrap()
                    .reshape((2, 1))
                    .unwrap(),
                alpha: Some(1.0),
            }],
        };
        let report = lora
            .apply(
                LoraComponent::ClipL,
                &mut map,
                1.0,
                Path::new("test.safetensors"),
            )
            .unwrap();
        assert_eq!(report.applied, 1);
        let merged = map
            .get("text_model.encoder.layers.0.self_attn.k_proj.weight")
            .unwrap()
            .flatten_all()
            .unwrap()
            .to_vec1::<f32>()
            .unwrap();
        assert_eq!(merged, vec![1.0, 0.0, 2.0, 1.0]);
    }
}
