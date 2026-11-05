//! The samplers: DPM-Solver++(2M) and Euler ancestral.
//! Both are transcribed from the algorithms' recurrences
//! as ComfyUI's k-diffusion port executes them
//! (`sample_dpmpp_2m`, `sample_euler_ancestral` — the
//! oracle source read as spec, no code taken). The
//! denoiser closure returns the model's x0 (denoised)
//! prediction; CFG composition happens inside the closure.

use candle::Tensor;

use crate::error::{GenError, Result};

/// Which sampler walks the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplerKind {
    /// DPM-Solver++(2M) — the Stage 1/2 sampler.
    Dpmpp2m,
    /// Euler ancestral (eta 1) — the D-555 recipe's
    /// sampler; needs a per-step fresh-noise source.
    EulerAncestral,
}

impl SamplerKind {
    /// The stable name used in run records and CLI flags
    /// (the oracle's own spellings).
    pub fn name(self) -> &'static str {
        match self {
            Self::Dpmpp2m => "dpmpp_2m",
            Self::EulerAncestral => "euler_ancestral",
        }
    }

    /// Parse a sampler name (the CLI/record spelling).
    ///
    /// # Errors
    /// Returns `gen_limit` on an unknown name.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "dpmpp_2m" => Ok(Self::Dpmpp2m),
            "euler_ancestral" => Ok(Self::EulerAncestral),
            other => Err(GenError::Limit {
                field: "sampler",
                detail: format!("unknown sampler {other:?}"),
            }),
        }
    }
}

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

/// The ancestral step split (k-diffusion
/// `get_ancestral_step`, eta = 1): from `sigma_from` toward
/// `sigma_to`, returns `(sigma_down, sigma_up)` — the
/// deterministic step lands at sigma_down and fresh noise
/// at sigma_up restores the marginal. At sigma_to = 0 the
/// split is (0, 0): the step lands exactly on the
/// denoised prediction.
pub fn ancestral_step(sigma_from: f64, sigma_to: f64) -> (f64, f64) {
    assert!(sigma_from > 0.0, "ancestral step needs sigma_from > 0");
    assert!(sigma_to >= 0.0, "sigma_to cannot be negative");
    if sigma_to == 0.0 {
        return (0.0, 0.0);
    }
    let sigma_up = (sigma_to * sigma_to * (sigma_from * sigma_from - sigma_to * sigma_to)
        / (sigma_from * sigma_from))
        .sqrt()
        .min(sigma_to);
    let sigma_down = (sigma_to * sigma_to - sigma_up * sigma_up).sqrt();
    (sigma_down, sigma_up)
}

/// Run Euler ancestral over `sigmas` (descending, terminal
/// zero included), starting from `x` (already
/// noise-scaled). `fresh_noise` supplies one unit-Gaussian
/// tensor shaped like `x` per step that needs one; it is
/// called only when the step's sigma_up is positive, so
/// the noise stream's draw count is a function of the
/// ladder alone. The stream itself is the caller's (the
/// pipeline's counter-based stream, seeded from the run
/// seed — the oracle draws from its own RNG, so cross-stack
/// noise identity is not claimed, only the recurrence).
///
/// # Errors
/// Returns the first error the denoiser or the noise
/// source raises, or a tensor error from the algebra.
pub fn euler_ancestral<F, N>(
    mut x: Tensor,
    sigmas: &[f64],
    mut denoise: F,
    mut fresh_noise: N,
) -> Result<Tensor>
where
    F: FnMut(&Tensor, f64) -> Result<Tensor>,
    N: FnMut() -> Result<Tensor>,
{
    assert!(sigmas.len() >= 2, "a ladder needs at least one step");
    for i in 0..sigmas.len() - 1 {
        let denoised = denoise(&x, sigmas[i])?;
        let (sigma_down, sigma_up) = ancestral_step(sigmas[i], sigmas[i + 1]);
        if sigma_down == 0.0 {
            // Terminal step: land on the prediction, no
            // noise drawn (the oracle's explicit branch).
            x = denoised;
        } else {
            let d = ((&x - &denoised)? / sigmas[i])?;
            x = (x + (d * (sigma_down - sigmas[i]))?)?;
            if sigma_up > 0.0 {
                x = (x + (fresh_noise()? * sigma_up)?)?;
            }
        }
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

    #[test]
    fn ancestral_step_facts() {
        // Terminal: no down, no up.
        assert_eq!(ancestral_step(3.0, 0.0), (0.0, 0.0));
        // Marginal preservation: down^2 + up^2 == to^2.
        for (from, to) in [(14.6146, 9.0), (2.5, 0.9), (0.5, 0.0292)] {
            let (down, up) = ancestral_step(from, to);
            assert!(
                (down * down + up * up - to * to).abs() < 1e-12,
                "{from}->{to}"
            );
            assert!(down > 0.0 && up > 0.0 && up < to);
        }
        // Equal sigmas: no movement at all — the split
        // is (to, 0), the step is a pure re-evaluation.
        let (down, up) = ancestral_step(2.0, 2.0);
        assert!((down - 2.0).abs() < 1e-12 && up.abs() < 1e-12);
    }

    #[test]
    fn euler_terminal_step_returns_denoised() {
        // A single step straight to sigma 0: sigma_down is
        // 0, the state lands on the prediction, and the
        // noise source is never drawn from.
        let dev = Device::Cpu;
        let x = Tensor::new(vec![2.0f32; 4], &dev)
            .unwrap()
            .reshape((1, 4))
            .unwrap();
        let sigmas = vec![7.0, 0.0];
        let out = euler_ancestral(
            x,
            &sigmas,
            |_x, _s| Ok(Tensor::new(vec![0.5f32; 4], &dev)?.reshape((1, 4))?),
            || panic!("no noise may be drawn on the terminal step"),
        )
        .unwrap();
        let v = out.flatten_all().unwrap().to_vec1::<f32>().unwrap();
        assert!(v.iter().all(|y| (*y - 0.5).abs() < 1e-6), "{v:?}");
    }

    #[test]
    fn euler_matches_scalar_recurrence() {
        // Scalar reference of the same recurrence with a
        // constant denoiser and a constant noise source;
        // guards the tensor algebra line by line.
        let dev = Device::Cpu;
        let (x0, c, n) = (1.25f64, 0.3f64, 0.7f64);
        let sigmas = vec![14.6146f64, 9.0, 5.0, 2.5, 0.9, 0.0];
        let mut x = x0;
        for i in 0..sigmas.len() - 1 {
            let (down, up) = ancestral_step(sigmas[i], sigmas[i + 1]);
            if down == 0.0 {
                x = c;
            } else {
                let d = (x - c) / sigmas[i];
                x += d * (down - sigmas[i]);
                if up > 0.0 {
                    x += n * up;
                }
            }
        }
        let xt = Tensor::new(vec![x0 as f32], &dev)
            .unwrap()
            .reshape((1, 1))
            .unwrap();
        let out = euler_ancestral(
            xt,
            &sigmas,
            |_x, _s| Ok(Tensor::new(vec![c as f32], &dev)?.reshape((1, 1))?),
            || Ok(Tensor::new(vec![n as f32], &dev)?.reshape((1, 1))?),
        )
        .unwrap();
        let v = out.flatten_all().unwrap().to_vec1::<f32>().unwrap()[0] as f64;
        assert!((v - x).abs() < 1e-5, "tensor {v} vs scalar {x}");
    }
}
