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

use crate::error::{GenError, Result};
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

    /// Sigma at a fractional timestep: log-space linear
    /// interpolation between the neighbouring discrete
    /// sigmas (ComfyUI `ModelSamplingDiscrete.sigma` rule,
    /// read from the oracle source as spec). Out-of-range
    /// timesteps clamp to the schedule ends.
    pub fn sigma_at(&self, timestep: f64) -> f64 {
        let t = timestep.clamp(0.0, (SCHEDULE_LEN - 1) as f64);
        let low = t.floor() as usize;
        let high = t.ceil() as usize;
        let w = t.fract();
        ((1.0 - w) * self.log_sigmas[low] + w * self.log_sigmas[high]).exp()
    }

    /// The "normal" ladder (ComfyUI `normal_scheduler`,
    /// read from the oracle source as spec): timesteps
    /// spaced linearly from the timestep of sigma_max down
    /// to the timestep of sigma_min, each mapped through
    /// the fractional `sigma_at`, plus a terminal zero.
    /// For SDXL the endpoints are timesteps 999 and 0.
    pub fn normal(&self, steps: usize) -> Vec<f64> {
        assert!(steps >= 1, "a ladder needs at least one step");
        let start = self.timestep(self.sigma_max()) as f64;
        let end = self.timestep(self.sigma_min()) as f64;
        let mut out = Vec::with_capacity(steps + 1);
        for i in 0..steps {
            let ramp = if steps == 1 {
                0.0
            } else {
                i as f64 / (steps - 1) as f64
            };
            out.push(self.sigma_at(start + ramp * (end - start)));
        }
        out.push(0.0);
        out
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

/// Which sigma ladder a run walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerKind {
    /// The Karras ladder (rho 7) — the Stage 1/2 default.
    Karras,
    /// ComfyUI's "normal" ladder: linearly spaced
    /// timesteps mapped through the discrete schedule.
    Normal,
}

impl SchedulerKind {
    /// The stable name used in run records and CLI flags.
    pub fn name(self) -> &'static str {
        match self {
            Self::Karras => "karras",
            Self::Normal => "normal",
        }
    }

    /// Parse a scheduler name (the CLI/record spelling).
    ///
    /// # Errors
    /// Returns `gen_limit` on an unknown name.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "karras" => Ok(Self::Karras),
            "normal" => Ok(Self::Normal),
            other => Err(GenError::Limit {
                field: "scheduler",
                detail: format!("unknown scheduler {other:?}"),
            }),
        }
    }

    /// Build this scheduler's ladder at `steps` steps.
    pub fn ladder(self, schedule: &Schedule, steps: usize) -> Vec<f64> {
        match self {
            Self::Karras => schedule.karras(steps),
            Self::Normal => schedule.normal(steps),
        }
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
    fn normal_ladder_shape() {
        let s = Schedule::sdxl();
        let n = s.normal(30);
        assert_eq!(n.len(), 31);
        assert_eq!(n[30], 0.0);
        assert!((n[0] - s.sigma_max()).abs() < 1e-9);
        assert!((n[29] - s.sigma_min()).abs() < 1e-9);
        for w in n.windows(2) {
            assert!(w[0] > w[1], "ladder must descend");
        }
        // The normal ladder falls faster at the top than
        // Karras: linear-in-timestep spacing takes a full
        // 1/29th of the timestep range in step one, while
        // the rho-7 Karras curve crowds its resolution at
        // the low-sigma end and so stays higher early.
        let k = s.karras(30);
        assert!(n[1] < k[1], "normal {} vs karras {}", n[1], k[1]);
    }

    #[test]
    fn sigma_at_hits_discrete_points_and_midpoints() {
        let s = Schedule::sdxl();
        for t in [0usize, 3, 500, 999] {
            assert!((s.sigma_at(t as f64) - s.sigma(t)).abs() < 1e-12);
        }
        // Log-space midpoint: geometric mean of neighbours.
        let mid = s.sigma_at(500.5);
        let expect = (s.sigma(500) * s.sigma(501)).sqrt();
        assert!((mid - expect).abs() / expect < 1e-9, "{mid} vs {expect}");
        // Clamps at both ends.
        assert!((s.sigma_at(-5.0) - s.sigma_min()).abs() < 1e-12);
        assert!((s.sigma_at(5000.0) - s.sigma_max()).abs() < 1e-12);
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
