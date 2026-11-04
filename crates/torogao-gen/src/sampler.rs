//! DPM-Solver++(2M) — the sampler the estate's ComfyUI
//! workflows run — transcribed from the algorithm's
//! recurrence as ComfyUI's k-diffusion port executes it
//! (`sample_dpmpp_2m`): t = -ln(sigma); single-step form for
//! the first step and the terminal sigma; multistep form
//! with the h-ratio correction otherwise. The denoiser
//! closure returns the model's x0 (denoised) prediction; CFG
//! composition happens inside the closure.

use candle::Tensor;

use crate::error::Result;

/// Run DPM++ 2M over `sigmas` (descending, terminal zero
/// included), starting from `x` (already noise-scaled).
///
/// # Errors
/// Returns the first error the denoiser raises, or a tensor
/// error from the sampler algebra.
pub fn dpmpp_2m<F>(mut x: Tensor, sigmas: &[f64], mut denoise: F) -> Result<Tensor>
where
    F: FnMut(&Tensor, f64) -> Result<Tensor>,
{
    assert!(sigmas.len() >= 2, "a ladder needs at least one step");
    let mut old_denoised: Option<Tensor> = None;
    let mut prev_t = 0.0f64;
    for i in 0..sigmas.len() - 1 {
        let denoised = denoise(&x, sigmas[i])?;
        let t = -sigmas[i].ln();
        let t_next = if sigmas[i + 1] == 0.0 {
            f64::INFINITY
        } else {
            -sigmas[i + 1].ln()
        };
        let h = t_next - t;
        let sigma_ratio = if sigmas[i + 1] == 0.0 {
            0.0
        } else {
            sigmas[i + 1] / sigmas[i]
        };
        if sigmas[i + 1] == 0.0 {
            // x = (sigma_next / sigma) * x - expm1(-h) * denoised
            let coeff = (-h).exp_m1();
            x = ((x * sigma_ratio)? - (denoised.clone() * coeff)?)?;
        } else if let Some(old) = old_denoised.as_ref() {
            let h_last = t - prev_t;
            let r = h_last / h;
            let d = ((denoised.clone() * (1.0 + 1.0 / (2.0 * r)))? - (old * (1.0 / (2.0 * r)))?)?;
            let coeff = (-h).exp_m1();
            x = ((x * sigma_ratio)? - (d * coeff)?)?;
        } else {
            // First step: no previous denoised estimate yet.
            let coeff = (-h).exp_m1();
            x = ((x * sigma_ratio)? - (denoised.clone() * coeff)?)?;
        }
        prev_t = t;
        old_denoised = Some(denoised);
    }
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle::{DType, Device};

    // A constant denoiser makes the trajectory closed-form:
    // with denoised = c everywhere, both the single-step and
    // the multistep recursions telescope to
    // x_final = x_0 - c * (something the ladder fixes); the
    // exact invariant checked here is the terminal-step one:
    // at sigma_next = 0 the update returns denoised itself.
    #[test]
    fn terminal_step_returns_denoised() {
        let dev = Device::Cpu;
        let x = Tensor::new(vec![2.0f32; 4], &dev)
            .unwrap()
            .reshape((1, 4))
            .unwrap();
        let sigmas = vec![14.6146, 7.0, 0.0];
        let out = dpmpp_2m(x, &sigmas, |_x, _s| {
            Ok(Tensor::new(vec![0.5f32; 4], &dev)?.reshape((1, 4))?)
        })
        .unwrap();
        let v = out.flatten_all().unwrap().to_vec1::<f32>().unwrap();
        assert!(v.iter().all(|y| (*y - 0.5).abs() < 1e-6), "{v:?}");
        let _ = DType::F32;
    }

    #[test]
    fn constant_denoiser_matches_reference_recurrence() {
        // Reference recurrence evaluated in plain f64 scalar
        // math, mirroring the implementation line for line;
        // guards against transcription slips in tensor ops.
        let dev = Device::Cpu;
        let x0 = 1.25f64;
        let c = 0.3f64;
        let sigmas = vec![14.6146f64, 9.0, 5.0, 2.5, 0.9, 0.0];
        let mut x = x0;
        let mut old: Option<f64> = None;
        let mut prev_t = 0.0;
        for i in 0..sigmas.len() - 1 {
            let t = -sigmas[i].ln();
            let t_next = if sigmas[i + 1] == 0.0 {
                f64::INFINITY
            } else {
                -sigmas[i + 1].ln()
            };
            let h = t_next - t;
            let ratio = if sigmas[i + 1] == 0.0 {
                0.0
            } else {
                sigmas[i + 1] / sigmas[i]
            };
            match (sigmas[i + 1] == 0.0, old) {
                (false, Some(old)) => {
                    let r = (t - prev_t) / h;
                    let d = (1.0 + 1.0 / (2.0 * r)) * c - (1.0 / (2.0 * r)) * old;
                    x = ratio * x - (-h).exp_m1() * d;
                }
                _ => {
                    x = ratio * x - (-h).exp_m1() * c;
                }
            }
            prev_t = t;
            old = Some(c);
        }
        let xt = Tensor::new(vec![x0 as f32], &dev)
            .unwrap()
            .reshape((1, 1))
            .unwrap();
        let out = dpmpp_2m(xt, &sigmas, |_x, _s| {
            Ok(Tensor::new(vec![c as f32], &dev)?.reshape((1, 1))?)
        })
        .unwrap();
        let v = out.flatten_all().unwrap().to_vec1::<f32>().unwrap()[0] as f64;
        assert!((v - x).abs() < 1e-5, "tensor {v} vs scalar {x}");
    }
}
