//! The named subliminal converters (TOROGAO-HYPNO-CAPABILITY
//! §a; TECHNIQUE-SPEC T1/T2 for the encoding, T11 for the
//! verification method).
//!
//! `subliminal-encode/1` transplants a spoken affirmation stem
//! into the ultrasonic-adjacent band by single-sideband AM:
//! band-limit to the speech core band, multiply by a
//! seeded-phase sine carrier, keep only the upper sideband,
//! then a lookahead limiter so the sideband never clips.
//! `subliminal-decode-verify/1` is the exact inverse, used
//! only as a gate instrument: decode a rendered stem and
//! require agreement with the source above a null-calibrated
//! floor (a bed-only null render decoded the same way must
//! not agree). Both are pure sample-domain functions — same
//! input and parameters, byte-identical output.

use crate::audioproc::Xoshiro256;

/// The encoder converter's format token.
pub const ENCODE_FORMAT: &str = "subliminal-encode/1";
/// The decode-verify converter's format token.
pub const DECODE_FORMAT: &str = "subliminal-decode-verify/1";

/// Refusal code: a carrier whose transplanted sideband would
/// cross Nyquist minus the guard band.
pub const TG_SUBLIMINAL_NYQUIST: &str = "TG-SUBLIMINAL-NYQUIST";
/// Refusal code: an encoding parameter outside its range.
pub const TG_SUBLIMINAL_PARAM: &str = "TG-SUBLIMINAL-PARAM";
/// Refusal code: a rendered subliminal stem whose measured
/// spectrum escapes its transplanted band.
pub const TG_SUBLIMINAL_SPECTRUM: &str = "TG-SUBLIMINAL-SPECTRUM";

/// Lowest carrier the encoding admits (T2's window).
pub const CARRIER_MIN_HZ: f64 = 14_000.0;
/// Highest carrier the encoding admits (T2's window).
pub const CARRIER_MAX_HZ: f64 = 20_000.0;
/// Default speech-band low edge (T1).
pub const BAND_LOW_DEFAULT_HZ: f64 = 300.0;
/// Default speech-band high edge (T1).
pub const BAND_HIGH_DEFAULT_HZ: f64 = 3_000.0;
/// Default encoder gain (T1's ×5).
pub const ENCODER_GAIN_DEFAULT: f64 = 5.0;
/// The decoder's fixed gain (T1's ×4).
pub const DECODER_GAIN: f64 = 4.0;
/// The lookahead limiter's ceiling (T1).
pub const LIMITER_CEILING: f64 = 0.8;
/// Guard band below Nyquist the sideband must stay clear of:
/// at 48 kHz a 20,000 Hz carrier with the default band tops
/// out at 23,000 Hz, inside the guard, and is refused rather
/// than aliased.
pub const NYQUIST_GUARD_HZ: f64 = 1_500.0;
/// Iterated sideband-isolation passes (T1 iterates its
/// high-pass at carrier + band low).
pub const SIDEBAND_PASSES: usize = 8;
/// The render-path spectrum law: at least this fraction of an
/// encoded stem's energy sits inside its transplanted band.
pub const SPECTRUM_BAND_MIN: f64 = 0.99;

/// Encoding parameters. The carrier phase derives from
/// `seed`, so a recipe's seed fully determines the render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubliminalParams {
    /// Carrier frequency in Hz, in [14,000, 20,000].
    pub carrier_hz: f64,
    /// Speech-band low edge in Hz (default 300).
    pub band_low_hz: f64,
    /// Speech-band high edge in Hz (default 3,000).
    pub band_high_hz: f64,
    /// Encoder gain applied before the limiter (default ×5).
    pub encoder_gain: f64,
    /// Seed for the carrier phase.
    pub seed: u64,
}

impl Default for SubliminalParams {
    fn default() -> Self {
        SubliminalParams {
            carrier_hz: 16_000.0,
            band_low_hz: BAND_LOW_DEFAULT_HZ,
            band_high_hz: BAND_HIGH_DEFAULT_HZ,
            encoder_gain: ENCODER_GAIN_DEFAULT,
            seed: 0,
        }
    }
}

impl SubliminalParams {
    /// The refusal a parameter set earns, if any: parameter
    /// ranges first, then the Nyquist guard.
    pub fn check(&self, sample_rate: u32) -> Result<(), String> {
        if !(CARRIER_MIN_HZ..=CARRIER_MAX_HZ).contains(&self.carrier_hz) {
            return Err(format!(
                "{TG_SUBLIMINAL_PARAM}: carrierHz {} outside [{CARRIER_MIN_HZ}, {CARRIER_MAX_HZ}]",
                self.carrier_hz
            ));
        }
        if self.band_low_hz <= 0.0 || self.band_low_hz >= self.band_high_hz {
            return Err(format!(
                "{TG_SUBLIMINAL_PARAM}: speech band [{}, {}] is empty or inverted",
                self.band_low_hz, self.band_high_hz
            ));
        }
        if self.band_high_hz > 6_000.0 {
            return Err(format!(
                "{TG_SUBLIMINAL_PARAM}: bandHighHz {} over the 6000 Hz encoding bound",
                self.band_high_hz
            ));
        }
        if self.encoder_gain <= 0.0 || self.encoder_gain > 20.0 {
            return Err(format!(
                "{TG_SUBLIMINAL_PARAM}: encoderGain {} outside (0, 20]",
                self.encoder_gain
            ));
        }
        let top = self.carrier_hz + self.band_high_hz;
        let limit = f64::from(sample_rate) / 2.0 - NYQUIST_GUARD_HZ;
        if top > limit {
            return Err(format!(
                "{TG_SUBLIMINAL_NYQUIST}: sideband top {top} Hz crosses Nyquist minus guard ({limit} Hz at {sample_rate} Hz); refused, not aliased"
            ));
        }
        Ok(())
    }

    fn carrier_phase(&self) -> f64 {
        let mut rng = Xoshiro256::from_seed(self.seed ^ 0x5355_424C_494D_4E41);
        (rng.uniform() + 1.0) * std::f64::consts::PI
    }
}

// ---- Filters --------------------------------------------------
// RBJ biquads, Butterworth Q per section; four sections in
// cascade make the 8th-order edges T1 codes.

#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn new(lowpass: bool, freq: f64, rate: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * freq / rate;
        let alpha = w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let cos = w.cos();
        let (b0, b1, b2, a0) = if lowpass {
            (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
            )
        } else {
            (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
            )
        };
        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn step(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

struct Chain {
    sections: [Biquad; 4],
}

impl Chain {
    fn new(lowpass: bool, freq: f64, rate: f64) -> Self {
        Chain {
            sections: [Biquad::new(lowpass, freq, rate); 4],
        }
    }

    fn apply(&mut self, samples: &[f64]) -> Vec<f64> {
        samples
            .iter()
            .map(|&x| {
                let mut v = x;
                for section in &mut self.sections {
                    v = section.step(v);
                }
                v
            })
            .collect()
    }
}

fn band_limit(samples: &[f64], params: &SubliminalParams, rate: f64) -> Vec<f64> {
    let mut hp = Chain::new(false, params.band_low_hz, rate);
    let stage = hp.apply(samples);
    let mut lp = Chain::new(true, params.band_high_hz, rate);
    lp.apply(&stage)
}

/// The lookahead limiter (T1, ceiling 0.8): instant attack
/// through the lookahead window, linear release over one
/// window. Deterministic in the input buffer.
fn limit(samples: &[f64]) -> Vec<f64> {
    const WINDOW: usize = 128;
    let n = samples.len();
    let mut out = Vec::with_capacity(n);
    let mut gain = 1.0f64;
    for i in 0..n {
        let ahead = (i + WINDOW).min(n);
        let mut target = 1.0f64;
        for &x in &samples[i..ahead] {
            if x.abs() > f64::EPSILON {
                target = target.min(LIMITER_CEILING / x.abs());
            }
        }
        gain = if target < gain {
            target
        } else {
            (gain + 1.0 / WINDOW as f64).min(target)
        };
        out.push(samples[i] * gain);
    }
    out
}

// ---- The converters -------------------------------------------

/// `subliminal-encode/1`: encode a mono source stem at
/// `sample_rate` into its upper-sideband transplant.
pub fn encode_subliminal(
    source: &[f64],
    params: &SubliminalParams,
    sample_rate: u32,
) -> Result<Vec<f64>, String> {
    params.check(sample_rate)?;
    let rate = f64::from(sample_rate);
    let banded = band_limit(source, params, rate);
    let phase = params.carrier_phase();
    let w = 2.0 * std::f64::consts::PI * params.carrier_hz / rate;
    let modulated: Vec<f64> = banded
        .iter()
        .enumerate()
        .map(|(i, &x)| x * (w * i as f64 + phase).sin())
        .collect();
    let mut stage = modulated;
    for _ in 0..SIDEBAND_PASSES {
        let mut hp = Chain::new(false, params.carrier_hz + params.band_low_hz, rate);
        stage = hp.apply(&stage);
    }
    let gained: Vec<f64> = stage.iter().map(|&x| x * params.encoder_gain).collect();
    Ok(limit(&gained))
}

/// `subliminal-decode-verify/1`: the symmetric decode —
/// multiply by the same carrier, low-pass back to the speech
/// band. A gate instrument; nothing ships it.
pub fn decode_subliminal(encoded: &[f64], params: &SubliminalParams, sample_rate: u32) -> Vec<f64> {
    let rate = f64::from(sample_rate);
    let phase = params.carrier_phase();
    let w = 2.0 * std::f64::consts::PI * params.carrier_hz / rate;
    let demodulated: Vec<f64> = encoded
        .iter()
        .enumerate()
        .map(|(i, &x)| x * (w * i as f64 + phase).sin() * DECODER_GAIN)
        .collect();
    band_limit(&demodulated, params, rate)
}

// ---- Spectrum census and agreement ----------------------------

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for i in (0..n).step_by(len) {
            let (mut cwr, mut cwi) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (re[i + k + len / 2], im[i + k + len / 2]);
                let (tr, ti) = (vr * cwr - vi * cwi, vr * cwi + vi * cwr);
                re[i + k] = ur + tr;
                im[i + k] = ui + ti;
                re[i + k + len / 2] = ur - tr;
                im[i + k + len / 2] = ui - ti;
                let nwr = cwr * wr - cwi * wi;
                cwi = cwr * wi + cwi * wr;
                cwr = nwr;
            }
        }
        len <<= 1;
    }
}

/// The fraction of spectral energy inside `[low_hz, high_hz]`
/// (Hann-windowed DFT, bins above DC). Deterministic.
pub fn band_energy_fraction(samples: &[f64], sample_rate: u32, low_hz: f64, high_hz: f64) -> f64 {
    let n = samples.len();
    if n < 16 {
        return 0.0;
    }
    let size = n.next_power_of_two();
    let mut re = vec![0.0f64; size];
    let mut im = vec![0.0f64; size];
    for (i, &x) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
        re[i] = x * window;
    }
    fft(&mut re, &mut im);
    let rate = f64::from(sample_rate);
    let mut total = 0.0f64;
    let mut inside = 0.0f64;
    for bin in 1..size / 2 {
        let energy = re[bin] * re[bin] + im[bin] * im[bin];
        let freq = bin as f64 * rate / size as f64;
        total += energy;
        if (low_hz..=high_hz).contains(&freq) {
            inside += energy;
        }
    }
    if total <= 0.0 {
        0.0
    } else {
        inside / total
    }
}

/// The amplitude envelope of a buffer: mean absolute value
/// over a sliding window. The iterated sideband filter is
/// phase-distorting by construction, so the decode-verify
/// gate measures content agreement on envelopes (the
/// transcription the spec names as its instrument is likewise
/// phase-insensitive), with waveform correlation kept as a
/// reported secondary figure.
pub fn envelope(samples: &[f64], half_window: usize) -> Vec<f64> {
    samples
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let lo = i.saturating_sub(half_window);
            let hi = (i + half_window).min(samples.len());
            samples[lo..hi].iter().map(|x| x.abs()).sum::<f64>() / (hi - lo) as f64
        })
        .collect()
}

/// Best Pearson correlation between two buffers over lags
/// `0..=max_lag` (the filter chains' group delay is absorbed
/// by the lag search). Mean-centred: envelopes are
/// all-positive, and an uncentred measure would let any two
/// of them "agree" on their shared DC. The round-trip
/// agreement measure the decode-verify gate compares against
/// its null floor.
pub fn best_correlation(a: &[f64], b: &[f64], max_lag: usize) -> f64 {
    fn prefix(v: &[f64], square: bool) -> Vec<f64> {
        let mut out = Vec::with_capacity(v.len() + 1);
        out.push(0.0);
        for &x in v {
            let term = if square { x * x } else { x };
            out.push(out[out.len() - 1] + term);
        }
        out
    }
    let (pa, pa2) = (prefix(a, false), prefix(a, true));
    let (pb, pb2) = (prefix(b, false), prefix(b, true));
    let mut best = 0.0f64;
    for lag in 0..=max_lag.min(a.len().saturating_sub(1)) {
        let n = (a.len() - lag).min(b.len());
        if n < 64 {
            break;
        }
        let nf = n as f64;
        let ma = (pa[lag + n] - pa[lag]) / nf;
        let mb = (pb[n] - pb[0]) / nf;
        let va = (pa2[lag + n] - pa2[lag]) - nf * ma * ma;
        let vb = (pb2[n] - pb2[0]) - nf * mb * mb;
        if va <= 0.0 || vb <= 0.0 {
            continue;
        }
        let mut dot = 0.0f64;
        for i in 0..n {
            dot += (a[lag + i] - ma) * (b[i] - mb);
        }
        best = best.max((dot / (va.sqrt() * vb.sqrt())).abs());
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audioproc::AUDIO_SAMPLE_RATE;

    /// A deterministic speech-band fixture: partials inside
    /// 300–3,000 Hz gated by a seeded syllable pattern
    /// (irregular 40 ms blocks, speech-like bursts and gaps)
    /// — the estate-fixture stand-in for an affirmation stem.
    /// A smooth test envelope would let a null render agree by
    /// sharing one slow swell; the syllable gate is what the
    /// agreement gate actually has to recover.
    fn fixture_source(seconds: f64) -> Vec<f64> {
        let rate = f64::from(AUDIO_SAMPLE_RATE);
        let n = (seconds * rate) as usize;
        let block = 1920usize; // 40 ms
        let blocks = n.div_ceil(block);
        let mut rng = Xoshiro256::from_seed(42);
        let gates: Vec<f64> = (0..blocks + 1)
            .map(|_| if rng.uniform() > -0.3 { 1.0 } else { 0.1 })
            .collect();
        (0..n)
            .map(|i| {
                let t = i as f64 / rate;
                let pos = i as f64 / block as f64;
                let lo = pos.floor() as usize;
                let frac = pos - lo as f64;
                let gate = gates[lo] * (1.0 - frac) + gates[lo + 1] * frac;
                gate * (0.45 * (2.0 * std::f64::consts::PI * 420.0 * t).sin()
                    + 0.30 * (2.0 * std::f64::consts::PI * 930.0 * t).sin()
                    + 0.20 * (2.0 * std::f64::consts::PI * 1750.0 * t).sin()
                    + 0.12 * (2.0 * std::f64::consts::PI * 2600.0 * t).sin())
            })
            .collect()
    }

    fn params() -> SubliminalParams {
        SubliminalParams {
            seed: 7,
            ..SubliminalParams::default()
        }
    }

    #[test]
    fn encode_is_byte_identical_across_runs() {
        let source = fixture_source(1.0);
        let a = encode_subliminal(&source, &params(), AUDIO_SAMPLE_RATE).unwrap();
        let b = encode_subliminal(&source, &params(), AUDIO_SAMPLE_RATE).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn encoded_energy_sits_in_the_transplanted_band() {
        let source = fixture_source(1.0);
        let p = params();
        let encoded = encode_subliminal(&source, &p, AUDIO_SAMPLE_RATE).unwrap();
        let inside = band_energy_fraction(
            &encoded,
            AUDIO_SAMPLE_RATE,
            p.carrier_hz + p.band_low_hz,
            p.carrier_hz + p.band_high_hz,
        );
        assert!(
            inside >= SPECTRUM_BAND_MIN,
            "band fraction {inside} below the law"
        );
        let below = band_energy_fraction(&encoded, AUDIO_SAMPLE_RATE, 1.0, p.carrier_hz);
        assert!(below <= 0.001, "energy below carrier: {below}");
    }

    #[test]
    fn decode_round_trip_beats_the_null_floor() {
        let source = fixture_source(2.0);
        let p = params();
        let encoded = encode_subliminal(&source, &p, AUDIO_SAMPLE_RATE).unwrap();
        let decoded = decode_subliminal(&encoded, &p, AUDIO_SAMPLE_RATE);
        // Content agreement is measured on envelopes (the
        // channel is phase-distorting; see `envelope`).
        let agreement = best_correlation(&envelope(&decoded, 120), &envelope(&source, 120), 4096);
        assert!(
            agreement > 0.9,
            "round-trip envelope agreement {agreement} under the floor"
        );
        let waveform = best_correlation(&decoded, &source, 4096);
        assert!(
            waveform > 0.3,
            "round-trip waveform correlation {waveform} implausibly low"
        );
        // The null: a bed-only render (seeded noise, no encoded
        // content) decoded identically must not agree.
        let mut rng = Xoshiro256::from_seed(99);
        let bed: Vec<f64> = (0..source.len()).map(|_| rng.uniform() * 0.2).collect();
        let null_decoded = decode_subliminal(&bed, &p, AUDIO_SAMPLE_RATE);
        let null_agreement =
            best_correlation(&envelope(&null_decoded, 120), &envelope(&source, 120), 4096);
        assert!(
            null_agreement < 0.5,
            "null envelope agreement {null_agreement} over the ceiling"
        );
        let null_waveform = best_correlation(&null_decoded, &source, 4096);
        assert!(
            null_waveform < 0.25,
            "null waveform correlation {null_waveform} over the ceiling"
        );
    }

    #[test]
    fn carrier_at_the_window_edge_is_refused_not_aliased() {
        let mut p = params();
        p.carrier_hz = 20_000.0;
        let err = encode_subliminal(&fixture_source(0.1), &p, AUDIO_SAMPLE_RATE).unwrap_err();
        assert!(err.contains(TG_SUBLIMINAL_NYQUIST), "{err}");
        p.carrier_hz = 13_000.0;
        let err = encode_subliminal(&fixture_source(0.1), &p, AUDIO_SAMPLE_RATE).unwrap_err();
        assert!(err.contains(TG_SUBLIMINAL_PARAM), "{err}");
    }
}
