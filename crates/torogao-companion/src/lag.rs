//! The Stage 2.1 measurement instruments: peak-spacing
//! statistics on an evaluated series, envelope extraction,
//! the regional-MAD series, and the cross-correlation lag
//! instrument the PROGRAM names for this stage — built
//! here, and run on companion performances.
//!
//! Cross-correlation method: Pearson correlation of the
//! two mean-removed series at every integer lag in
//! ±`max_lag_steps`; the peak lag is refined to sub-step
//! precision by parabolic interpolation over the three
//! correlations around the peak, so a one-frame lag at
//! 60 fps reads 16.7 ms and not "somewhere in 0–33".
//! Regional MAD: over a named region of the parameter
//! vector (the body region, the hair region), the
//! per-step mean absolute delta — the activity series of
//! that region, whose cross-correlation with another
//! region's gives the motion-propagation lag in the
//! same units.

/// Steps per second of the fixed-step clock.
pub const STEP_RATE_HZ: f64 = 60.0;
/// Milliseconds per fixed step.
pub const STEP_MS: f64 = 1000.0 / STEP_RATE_HZ;

/// Peak-spacing statistics over an evaluated series.
#[derive(Debug, Clone)]
pub struct PeakStats {
    /// Number of positive peaks found.
    pub peaks: usize,
    /// Mean peak spacing, seconds.
    pub mean_period_seconds: f64,
    /// The frequency the mean spacing implies, Hz.
    pub frequency_hz: f64,
    /// Standard deviation of successive peak spacings,
    /// milliseconds.
    pub jitter_sigma_ms: f64,
}

/// Finds positive peaks in `series` (a sample is a peak
/// when it is at least its two neighbours and strictly
/// above zero), refines each peak's time by parabolic
/// interpolation, and reports spacing statistics.
/// `window` restricts the analysis to `[start, end)`
/// sample indices (the sustain region of a performance).
pub fn peak_spacing(series: &[f64], window: (usize, usize)) -> PeakStats {
    let (start, end) = (
        window.0.max(1),
        window.1.min(series.len().saturating_sub(1)),
    );
    let mut peak_times: Vec<f64> = Vec::new();
    for i in start..end {
        let (prev, value, next) = (series[i - 1], series[i], series[i + 1]);
        if value > 0.0 && value >= prev && value >= next && (value > prev || value > next) {
            // Parabolic vertex offset, in samples.
            let denom = prev - 2.0 * value + next;
            let offset = if denom.abs() > f64::EPSILON {
                (0.5 * (prev - next) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            peak_times.push(i as f64 + offset);
        }
    }
    if peak_times.len() < 2 {
        return PeakStats {
            peaks: peak_times.len(),
            mean_period_seconds: 0.0,
            frequency_hz: 0.0,
            jitter_sigma_ms: 0.0,
        };
    }
    let spacings: Vec<f64> = peak_times
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) / STEP_RATE_HZ)
        .collect();
    let mean = spacings.iter().sum::<f64>() / spacings.len() as f64;
    let variance = spacings
        .iter()
        .map(|spacing| (spacing - mean) * (spacing - mean))
        .sum::<f64>()
        / spacings.len() as f64;
    PeakStats {
        peaks: peak_times.len(),
        mean_period_seconds: mean,
        frequency_hz: if mean > 0.0 { 1.0 / mean } else { 0.0 },
        jitter_sigma_ms: variance.sqrt() * 1000.0,
    }
}

/// Extracts a series' amplitude envelope by peak-hold:
/// at each sample, the largest |value| inside a centred
/// window of `window_steps` samples (one nominal period
/// by default at the corpus frequency).
pub fn peak_hold_envelope(series: &[f64], window_steps: usize) -> Vec<f64> {
    let half = window_steps / 2;
    series
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let from = i.saturating_sub(half);
            let to = (i + half + 1).min(series.len());
            series[from..to]
                .iter()
                .map(|value| value.abs())
                .fold(0.0_f64, f64::max)
        })
        .collect()
}

/// The root-mean-square error between two equal-length
/// series (0 when either is empty).
pub fn rmse(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let sum: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum();
    (sum / a.len() as f64).sqrt()
}

/// The regional-MAD activity series of a parameter
/// trace: at each step, the mean absolute delta over
/// `region` (indices into the parameter vector) versus
/// the previous step, summed across the [x, y] pair.
/// Step 0 reports 0 (no predecessor).
pub fn regional_mad_series(params: &[Vec<[f32; 2]>], region: &[usize]) -> Vec<f64> {
    if params.is_empty() || region.is_empty() {
        return vec![0.0; params.len()];
    }
    let mut out = Vec::with_capacity(params.len());
    out.push(0.0);
    for pair in params.windows(2) {
        let sum: f64 = region
            .iter()
            .map(|&index| {
                let (a, b) = (pair[0][index], pair[1][index]);
                f64::from((a[0] - b[0]).abs() + (a[1] - b[1]).abs())
            })
            .sum();
        out.push(sum / region.len() as f64);
    }
    out
}

/// The fraction of samples at or below `epsilon` — the
/// near-zero-shift fraction when the series is a motion
/// (MAD) series and epsilon is the no-visible-change
/// floor.
pub fn near_zero_fraction(series: &[f64], epsilon: f64) -> f64 {
    if series.is_empty() {
        return 0.0;
    }
    let count = series.iter().filter(|value| **value <= epsilon).count();
    count as f64 / series.len() as f64
}

/// A cross-correlation lag measurement.
#[derive(Debug, Clone, Copy)]
pub struct LagReport {
    /// The refined lag of `secondary` behind `primary`,
    /// in steps (positive = secondary trails).
    pub lag_steps: f64,
    /// The same lag in milliseconds.
    pub lag_ms: f64,
    /// The Pearson correlation at the peak.
    pub peak_correlation: f64,
}

/// Measures how far `secondary` trails `primary` by
/// normalized cross-correlation over ±`max_lag_steps`,
/// with parabolic sub-step refinement of the peak.
pub fn cross_correlation_lag(primary: &[f64], secondary: &[f64], max_lag_steps: i64) -> LagReport {
    let n = primary.len().min(secondary.len());
    if n < 8 {
        return LagReport {
            lag_steps: 0.0,
            lag_ms: 0.0,
            peak_correlation: 0.0,
        };
    }
    let (primary, secondary) = (&primary[..n], &secondary[..n]);
    let mut best_lag = 0_i64;
    let mut best_corr = f64::MIN;
    let mut corrs: Vec<(i64, f64)> = Vec::new();
    for lag in -max_lag_steps..=max_lag_steps {
        let corr = pearson_at_lag(primary, secondary, lag);
        corrs.push((lag, corr));
        if corr > best_corr {
            best_corr = corr;
            best_lag = lag;
        }
    }
    // Parabolic refinement over the peak's neighbours.
    let mut refined = best_lag as f64;
    if best_lag > -max_lag_steps && best_lag < max_lag_steps {
        let at = |lag: i64| {
            corrs
                .iter()
                .find(|(l, _)| *l == lag)
                .map(|(_, c)| *c)
                .unwrap_or(0.0)
        };
        let (y0, y1, y2) = (at(best_lag - 1), at(best_lag), at(best_lag + 1));
        let denom = y0 - 2.0 * y1 + y2;
        if denom.abs() > f64::EPSILON {
            refined += (0.5 * (y0 - y2) / denom).clamp(-0.5, 0.5);
        }
    }
    LagReport {
        lag_steps: refined,
        lag_ms: refined * STEP_MS,
        peak_correlation: best_corr,
    }
}

/// Pearson correlation of `a[i]` with `b[i + lag]` over
/// the overlap (positive lag = b trails a).
fn pearson_at_lag(a: &[f64], b: &[f64], lag: i64) -> f64 {
    let n = a.len() as i64;
    let from = (-lag).max(0);
    let to = (n - lag.max(0)).min(n);
    if to - from < 8 {
        return 0.0;
    }
    let count = (to - from) as f64;
    let (mut sum_a, mut sum_b) = (0.0, 0.0);
    for i in from..to {
        sum_a += a[i as usize];
        sum_b += b[(i + lag) as usize];
    }
    let (mean_a, mean_b) = (sum_a / count, sum_b / count);
    let (mut cov, mut var_a, mut var_b) = (0.0, 0.0, 0.0);
    for i in from..to {
        let da = a[i as usize] - mean_a;
        let db = b[(i + lag) as usize] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    if var_a <= 0.0 || var_b <= 0.0 {
        return 0.0;
    }
    cov / (var_a.sqrt() * var_b.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(steps: usize, period_steps: f64) -> Vec<f64> {
        (0..steps)
            .map(|i| (std::f64::consts::TAU * i as f64 / period_steps).sin())
            .collect()
    }

    #[test]
    fn peak_spacing_reads_the_corpus_frequency() {
        // 2.31 Hz at 60 fps is a 25.97-step period.
        let series = sine(3600, 60.0 / 2.31);
        let stats = peak_spacing(&series, (0, 3600));
        assert!(stats.peaks > 100, "peaks {}", stats.peaks);
        assert!(
            (stats.frequency_hz - 2.31).abs() < 0.01,
            "frequency {}",
            stats.frequency_hz
        );
        assert!(
            stats.jitter_sigma_ms < 1.0,
            "a pure tone has no jitter: {}",
            stats.jitter_sigma_ms
        );
    }

    #[test]
    fn cross_correlation_finds_a_one_frame_lag() {
        let primary = sine(1200, 26.0);
        let mut secondary = vec![0.0; 1200];
        for i in 1..1200 {
            secondary[i] = primary[i - 1] * 0.8;
        }
        let report = cross_correlation_lag(&primary, &secondary, 4);
        assert!(
            (report.lag_steps - 1.0).abs() < 0.15,
            "lag {} steps",
            report.lag_steps
        );
        assert!(report.lag_ms <= 17.0, "lag {} ms", report.lag_ms);
        assert!(report.peak_correlation > 0.99);
    }

    #[test]
    fn cross_correlation_finds_zero_and_negative_lags() {
        let primary = sine(1200, 26.0);
        let same = cross_correlation_lag(&primary, &primary, 4);
        assert!(same.lag_steps.abs() < 0.15);
        let mut leading = vec![0.0; 1200];
        leading[..1199].copy_from_slice(&primary[1..1200]);
        let lead = cross_correlation_lag(&primary, &leading, 4);
        assert!(
            (lead.lag_steps + 1.0).abs() < 0.15,
            "lead lag {}",
            lead.lag_steps
        );
    }

    #[test]
    fn the_envelope_of_a_gated_tone_matches_its_gate() {
        let mut series = sine(600, 26.0);
        for (i, value) in series.iter_mut().enumerate() {
            if i < 300 {
                *value *= i as f64 / 300.0;
            }
        }
        let envelope = peak_hold_envelope(&series, 26);
        // In the sustain half the envelope sits at the
        // tone's amplitude; early it follows the ramp.
        assert!((envelope[500] - 1.0).abs() < 0.05);
        assert!(envelope[30] < 0.35, "early envelope {}", envelope[30]);
    }

    #[test]
    fn regional_mad_and_near_zero_fraction() {
        let params: Vec<Vec<[f32; 2]>> =
            (0..10).map(|i| vec![[i as f32, 0.0], [0.0, 0.0]]).collect();
        let mad = regional_mad_series(&params, &[0]);
        assert_eq!(mad[0], 0.0);
        assert!((mad[5] - 1.0).abs() < 1e-9);
        let still: Vec<Vec<[f32; 2]>> = vec![vec![[1.0, 1.0]]; 4];
        let still_mad = regional_mad_series(&still, &[0]);
        assert_eq!(near_zero_fraction(&still_mad, 1e-6), 1.0);
        assert_eq!(near_zero_fraction(&mad, 0.5), 0.1);
    }
}
