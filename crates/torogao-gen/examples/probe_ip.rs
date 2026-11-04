//! probe_ip — Stage 2 diagnostic (D-936): dump the real
//! CLIP-ViT-H penultimate tokens and the resampler's
//! output tokens for one reference image: RMS, per-token
//! norms, and pairwise cosine spread. Token collapse
//! (pairwise cosine near 1) would mean the vision path
//! is broken; healthy tokens are diverse.
//!
//! Usage: cargo run --release --example probe_ip -- \
//!   CLIP_VISION.safetensors IPADAPTER.safetensors REF.png

use candle::{DType, Device, Tensor};
use torogao_gen::clip_vision::ClipVisionH;
use torogao_gen::ipadapter::{load_ip_weights, rms};

fn token_stats(name: &str, t: &Tensor) -> torogao_gen::Result<()> {
    let t = t.to_dtype(DType::F32)?;
    let (_, seq, dim) = t.dims3()?;
    let flat = t.reshape((seq, dim))?;
    let norms = flat
        .sqr()?
        .sum_keepdim(1)?
        .sqrt()?
        .reshape(seq)?
        .to_vec1::<f32>()?;
    let mean_norm = norms.iter().sum::<f32>() / seq as f32;
    let min_norm = norms.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_norm = norms.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    // Pairwise cosine over the first min(seq, 64) tokens.
    let n = seq.min(64);
    let sub = flat.narrow(0, 0, n)?;
    let unit = sub.broadcast_div(&sub.sqr()?.sum_keepdim(1)?.sqrt()?)?;
    let cos = unit.matmul(&unit.t()?)?;
    let cos_v = cos.flatten_all()?.to_vec1::<f32>()?;
    let mut sum = 0.0f64;
    let mut cnt = 0u64;
    let mut maxc = f32::NEG_INFINITY;
    for i in 0..n {
        for j in (i + 1)..n {
            let c = cos_v[i * n + j];
            sum += f64::from(c);
            cnt += 1;
            maxc = maxc.max(c);
        }
    }
    println!(
        "{name}: seq={seq} dim={dim} rms={:.4} token-norm mean={mean_norm:.3} min={min_norm:.3} max={max_norm:.3} pairwise-cos mean={:.4} max={maxc:.4}",
        rms(&t)?,
        sum / cnt as f64,
    );
    Ok(())
}

fn main() -> torogao_gen::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (vision_path, ipa_path, reference) = (&args[1], &args[2], &args[3]);
    let device = Device::new_cuda(0)?;
    let vision = ClipVisionH::load(std::path::Path::new(vision_path), &device)?;
    let tokens = vision.embed(std::path::Path::new(reference))?;
    token_stats("vision penultimate", &tokens)?;
    let weights = load_ip_weights(std::path::Path::new(ipa_path), &device)?;
    let out = weights.resampler.forward(&tokens)?;
    token_stats("resampler out", &out)?;
    Ok(())
}
