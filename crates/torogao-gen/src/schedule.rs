//! The SDXL discrete noise schedule and the sampler
//! schedules built on it, as data: scaled-linear betas
//! (0.00085..0.012 over 1000 steps — the schedule recorded in
//! the Iris checkpoint's provenance and used by ComfyUI's
//! ModelSamplingDiscrete defaults), sigmas derived from the
//! cumulative alpha product, sigma-to-timestep inversion by
//! nearest log-sigma (ComfyUI's rule), and the Karras sigma
//! ladder (ComfyUI `get_sigmas_karras`, rho = 7, zero
//! appended).
//!
//! Also the SDXL time-id embedding (ComfyUI `Timestep(256)`:
//! cos block then sin block over
//! exp(-ln(10000) * arange(128) / 128)) used for the added
//! conditioning vector.

use crate::limits::SCHEDULE_LEN;

/// Beta at the schedule start (scaled-linear).
pub const BETA_START: f64 = 0.00085;
/// Beta at the schedule end (scaled-linear).
pub const BETA_END: f64 = 0.012;
/// Karras rho (ComfyUI default).
pub const KARRAS_RHO: f64 = 7.0;

/// The discrete schedule: sigmas ascending with timestep.
#[derive(Debug, Clone)]
pub struct Schedule {
    sigmas: Vec<f64>,
    log_sigmas: Vec<f64>,
}

impl Schedule {
    /// Build the SDXL scaled-linear schedule.
    pub fn sdxl() -> Self {
        let n = SCHEDULE_LEN;
        let lo = BETA_START.sqrt();
        let hi = BETA_END.sqrt();
        let mut alpha_bar = 1.0f64;
        let mut sigmas = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / (n - 1) as f64;
            let beta = (lo + t * (hi - lo)).powi(2);
            alpha_bar *= 1.0 - beta;
            sigmas.push(((1.0 - alpha_bar) / alpha_bar).sqrt());
        }
        let log_sigmas = sigmas.iter().map(|s| s.ln()).collect();
        Self { sigmas, log_sigmas }
    }

    /// Sigma at a discrete timestep index.
    pub fn sigma(&self, timestep: usize) -> f64 {
        self.sigmas[timestep]
    }

    /// Smallest sigma (timestep 0).
    pub fn sigma_min(&self) -> f64 {
        self.sigmas[0]
    }

    /// Largest sigma (final timestep).
    pub fn sigma_max(&self) -> f64 {
        self.sigmas[SCHEDULE_LEN - 1]
    }

    /// The discrete timestep whose sigma is nearest to
    /// `sigma` in log space (ComfyUI's `timestep()` rule).
    pub fn timestep(&self, sigma: f64) -> usize {
        let target = sigma.ln();
        let mut best = 0usize;
        let mut best_d = f64::MAX;
        for (i, ls) in self.log_sigmas.iter().enumerate() {
            let d = (target - ls).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }

    /// The Karras ladder: `steps` sigmas from sigma_max down
    /// to sigma_min at rho, plus a terminal zero.
    pub fn karras(&self, steps: usize) -> Vec<f64> {
        let lo = self.sigma_min().powf(1.0 / KARRAS_RHO);
        let hi = self.sigma_max().powf(1.0 / KARRAS_RHO);
        let mut out = Vec::with_capacity(steps + 1);
        for i in 0..steps {
            let ramp = if steps == 1 {
                0.0
            } else {
                i as f64 / (steps - 1) as f64
            };
            out.push((hi + ramp * (lo - hi)).powf(KARRAS_RHO));
        }
        out.push(0.0);
        out
    }
}

/// The sinusoidal embedding ComfyUI's `Timestep(dim)` builds:
/// cos block concatenated with sin block.
pub fn timestep_embedding(value: f64, dim: usize) -> Vec<f32> {
    let half = dim / 2;
    let mut out = Vec::with_capacity(dim);
    let freqs: Vec<f64> = (0..half)
        .map(|i| (-10000f64.ln() * i as f64 / half as f64).exp())
        .collect();
    for f in &freqs {
        out.push((value * f).cos() as f32);
    }
    for f in &freqs {
        out.push((value * f).sin() as f32);
    }
    out
}

/// The SDXL time-id half of the added conditioning: the six
/// ids [height, width, crop_h, crop_w, target_height,
/// target_width] each embedded at dim 256 and concatenated
/// (ComfyUI `SDXL.encode_adm` order).
pub fn time_ids_embedding(height: u32, width: u32) -> Vec<f32> {
    let ids = [
        height as f64,
        width as f64,
        0.0,
        0.0,
        height as f64,
        width as f64,
    ];
    let mut out = Vec::with_capacity(6 * 256);
    for id in ids {
        out.extend(timestep_embedding(id, 256));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_endpoints_match_sdxl_facts() {
        let s = Schedule::sdxl();
        // Known SDXL scaled-linear endpoints (also what
        // ComfyUI reports as sigma_min / sigma_max).
        assert!((s.sigma_min() - 0.0292).abs() < 0.001, "{}", s.sigma_min());
        assert!((s.sigma_max() - 14.6146).abs() < 0.01, "{}", s.sigma_max());
    }

    #[test]
    fn timestep_round_trips() {
        let s = Schedule::sdxl();
        for t in [0usize, 1, 500, 998, 999] {
            assert_eq!(s.timestep(s.sigma(t)), t);
        }
    }

    #[test]
    fn karras_ladder_shape() {
        let s = Schedule::sdxl();
        let k = s.karras(30);
        assert_eq!(k.len(), 31);
        assert_eq!(k[30], 0.0);
        assert!((k[0] - s.sigma_max()).abs() < 1e-9);
        assert!((k[29] - s.sigma_min()).abs() < 1e-6);
        for w in k.windows(2) {
            assert!(w[0] > w[1], "ladder must descend");
        }
    }

    #[test]
    fn time_id_embedding_shape_and_zero_crop() {
        let e = time_ids_embedding(1024, 1024);
        assert_eq!(e.len(), 1536);
        // crop ids are 0: cos(0)=1 block then sin(0)=0 block.
        let crop = &e[512..768];
        assert!(crop[..128].iter().all(|v| (*v - 1.0).abs() < 1e-6));
        assert!(crop[128..].iter().all(|v| v.abs() < 1e-6));
    }
}
