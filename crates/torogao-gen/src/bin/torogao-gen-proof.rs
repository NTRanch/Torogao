//! torogao-gen-proof — the D-867 Stage 1 battery driver:
//! one prompt/seed per invocation, PNG plus a D-865 run
//! record (status candidate, made_by Iris with the weights
//! hash from the census) per image.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use candle::Device;
use serde::Serialize;
use torogao_gen::pipeline::write_png;
use torogao_gen::{
    CnType, ConditioningAssets, ConditioningSpec, ControlSpec, GenParams, IpSpec, LoadReport,
    LoraSpec, SamplerKind, SchedulerKind, SdxlModel,
};

/// The D-865 run record written beside every output.
#[derive(Serialize)]
struct RunRecord {
    status: &'static str,
    made_by: MadeBy,
    prompt: String,
    negative: String,
    seed: u64,
    sampler: String,
    scheduler: String,
    steps: usize,
    cfg: f32,
    width: u32,
    height: u32,
    pipe: &'static str,
    pipe_run: String,
    backend: &'static str,
    noise_sha256: String,
    elapsed_ms: u64,
    /// Stage 2 conditioning provenance (D-936); null on
    /// unconditioned (Stage 1) runs.
    conditioning: Option<torogao_gen::conditioning::ConditioningRecord>,
    /// Map-based load provenance (single-file checkpoint
    /// or LoRA merge); null on the Stage 1 layout path.
    load: Option<LoadReport>,
}

#[derive(Serialize)]
struct MadeBy {
    name: &'static str,
    category: &'static str,
    model: String,
    weights_file: String,
    weights_sha256: String,
    license_alert: bool,
    cleared_for_production: bool,
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

/// Every value of a repeatable flag, in argument order.
fn args_all(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (weights, out, record_path, run_id) = match (
        arg(&args, "--weights"),
        arg(&args, "--out"),
        arg(&args, "--record"),
        arg(&args, "--run-id"),
    ) {
        (w, Some(o), Some(r), Some(i)) => (w, o, r, i),
        _ => {
            eprintln!(
                "usage: torogao-gen-proof (--weights DIR | --checkpoint FILE [--tokenizers DIR]) --out PNG --record JSON --run-id ID [--prompt S] [--negative S] [--seed N] [--steps N] [--sampler dpmpp_2m|euler_ancestral] [--scheduler karras|normal] [--cfg F] [--width N] [--height N] [--lora FILE[@WEIGHT]]... [--controlnet F --pose PNG [--cn-strength F] [--cn-type T] [--cn-start F] [--cn-end F]] [--ipadapter F --clip-vision F --reference PNG [--ip-scale F]]"
            );
            return ExitCode::from(2);
        }
    };
    // Native-stack load selection: exactly one weight
    // source. --checkpoint is the single-file path;
    // --weights is the converted-layout directory (with
    // --lora it loads through tensor maps instead).
    let checkpoint = arg(&args, "--checkpoint");
    if checkpoint.is_none() && weights.is_none() {
        eprintln!("one of --weights or --checkpoint is required");
        return ExitCode::from(2);
    }
    if checkpoint.is_some() && weights.is_some() && arg(&args, "--tokenizers").is_none() {
        eprintln!("--weights with --checkpoint is ambiguous; pass --tokenizers DIR instead");
        return ExitCode::from(2);
    }
    let tokenizers = arg(&args, "--tokenizers").or_else(|| weights.clone());
    // Repeatable --lora FILE[@WEIGHT] (default weight 1).
    let mut loras: Vec<LoraSpec> = Vec::new();
    for raw in args_all(&args, "--lora") {
        let (path, scale) = match raw.rsplit_once('@') {
            Some((p, w)) => match w.parse::<f64>() {
                Ok(v) => (p.to_string(), v),
                Err(_) => (raw.clone(), 1.0),
            },
            None => (raw.clone(), 1.0),
        };
        loras.push(LoraSpec {
            path: PathBuf::from(path),
            scale,
        });
    }
    let sampler = match arg(&args, "--sampler") {
        Some(name) => match SamplerKind::parse(&name) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error [{}]: {e}", e.code());
                return ExitCode::from(2);
            }
        },
        None => SamplerKind::Dpmpp2m,
    };
    let scheduler = match arg(&args, "--scheduler") {
        Some(name) => match SchedulerKind::parse(&name) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error [{}]: {e}", e.code());
                return ExitCode::from(2);
            }
        },
        None => SchedulerKind::Karras,
    };
    // Stage 2 conditioning flags (D-936): each half is
    // all-or-nothing; a partial half is a usage error.
    let cn_flags = (arg(&args, "--controlnet"), arg(&args, "--pose"));
    let ip_flags = (
        arg(&args, "--ipadapter"),
        arg(&args, "--clip-vision"),
        arg(&args, "--reference"),
    );
    let parse_f64 = |name: &str, default: f64| -> f64 {
        arg(&args, name)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let spec = match (cn_flags, ip_flags) {
        ((None, None), (None, None, None)) => None,
        ((Some(cn), Some(pose)), ip) => {
            let cn_type = match arg(&args, "--cn-type") {
                Some(name) => match CnType::parse(&name) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("error [{}]: {e}", e.code());
                        return ExitCode::from(2);
                    }
                },
                None => CnType::OpenPose,
            };
            let ip_spec = match ip {
                (None, None, None) => None,
                (Some(ipa), Some(cv), Some(rf)) => Some(IpSpec {
                    ipadapter_path: PathBuf::from(ipa),
                    clip_vision_path: PathBuf::from(cv),
                    reference_path: PathBuf::from(rf),
                    scale: parse_f64("--ip-scale", 0.7),
                }),
                _ => {
                    eprintln!("--ipadapter, --clip-vision and --reference must be given together");
                    return ExitCode::from(2);
                }
            };
            Some(ConditioningSpec {
                control: Some(ControlSpec {
                    controlnet_path: PathBuf::from(cn),
                    pose_path: PathBuf::from(pose),
                    strength: parse_f64("--cn-strength", 0.8),
                    cn_type,
                    start: parse_f64("--cn-start", 0.0),
                    end: parse_f64("--cn-end", 1.0),
                }),
                ip: ip_spec,
            })
        }
        ((None, None), (Some(ipa), Some(cv), Some(rf))) => Some(ConditioningSpec {
            control: None,
            ip: Some(IpSpec {
                ipadapter_path: PathBuf::from(ipa),
                clip_vision_path: PathBuf::from(cv),
                reference_path: PathBuf::from(rf),
                scale: parse_f64("--ip-scale", 0.7),
            }),
        }),
        _ => {
            eprintln!(
                "conditioning flags are all-or-nothing per half (--controlnet with --pose; --ipadapter with --clip-vision and --reference)"
            );
            return ExitCode::from(2);
        }
    };
    let params = GenParams {
        prompt: arg(&args, "--prompt").unwrap_or_default(),
        negative: arg(&args, "--negative").unwrap_or_default(),
        seed: arg(&args, "--seed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(20261020),
        steps: arg(&args, "--steps")
            .and_then(|v| v.parse().ok())
            .unwrap_or(30),
        sampler,
        scheduler,
        cfg: arg(&args, "--cfg")
            .and_then(|v| v.parse().ok())
            .unwrap_or(6.0),
        width: arg(&args, "--width")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024),
        height: arg(&args, "--height")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024),
    };
    let device = match Device::new_cuda(0) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cuda unavailable ({e}); refusing CPU fallback for the proof battery");
            return ExitCode::from(3);
        }
    };
    let conditioned = spec.is_some();
    let run = || -> torogao_gen::Result<String> {
        let model = match (&checkpoint, &weights) {
            (Some(cp), _) => {
                let tok_dir = tokenizers
                    .clone()
                    .ok_or_else(|| torogao_gen::GenError::Weights {
                        detail: "--checkpoint needs --tokenizers DIR (or --weights as its source)"
                            .into(),
                    })?;
                SdxlModel::load_single_file(
                    &PathBuf::from(cp),
                    &PathBuf::from(tok_dir),
                    &device,
                    &loras,
                )?
            }
            (None, Some(dir)) if !loras.is_empty() => {
                SdxlModel::load_layout_maps(&PathBuf::from(dir), &device, &loras)?
            }
            (None, Some(dir)) => SdxlModel::load(&PathBuf::from(dir), &device)?,
            (None, None) => {
                return Err(torogao_gen::GenError::Weights {
                    detail: "no weight source".into(),
                });
            }
        };
        let img = match spec {
            Some(spec) => {
                let assets = ConditioningAssets::load(&spec, params.width, params.height, &device)?;
                model.generate_conditioned(&params, assets)?
            }
            None => model.generate(&params)?,
        };
        write_png(&img, &PathBuf::from(&out))?;
        let record = RunRecord {
            status: "candidate",
            made_by: MadeBy {
                name: "Iris",
                category: "Image",
                model: arg(&args, "--model-name")
                    .unwrap_or_else(|| "Illustrious XL v1.1".to_string()),
                weights_file: arg(&args, "--weights-file").unwrap_or_else(|| {
                    "Illustrious-XL-v1.1.safetensors (Iris.safetensors in ComfyUI custody)"
                        .to_string()
                }),
                weights_sha256: arg(&args, "--weights-sha256").unwrap_or_else(|| {
                    "536863e9f0c13b0ce834e2f8a19ada425ee4f722c0ad3d0051ec7e6adaa8156c".to_string()
                }),
                license_alert: true,
                cleared_for_production: false,
            },
            prompt: params.prompt.clone(),
            negative: params.negative.clone(),
            seed: params.seed,
            sampler: params.sampler.name().to_string(),
            scheduler: params.scheduler.name().to_string(),
            steps: params.steps,
            cfg: params.cfg,
            width: params.width,
            height: params.height,
            pipe: if conditioned {
                "torogao-gen stage2 (D-936)"
            } else {
                "torogao-gen stage1 (D-867)"
            },
            pipe_run: run_id.clone(),
            backend: "torogao-gen: candle-core 0.11.1 + vendored SD UNet (added conditioning restored)",
            noise_sha256: img.noise_sha256.clone(),
            elapsed_ms: img.elapsed_ms,
            conditioning: img.conditioning.clone(),
            load: model.load_report.clone(),
        };
        let json =
            serde_json::to_string_pretty(&record).map_err(|e| torogao_gen::GenError::Image {
                detail: e.to_string(),
            })?;
        std::fs::write(&record_path, format!("{json}\n"))?;
        Ok(json)
    };
    match run() {
        Ok(json) => {
            let _ = writeln!(std::io::stdout(), "{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error [{}]: {e}", e.code());
            ExitCode::from(1)
        }
    }
}
