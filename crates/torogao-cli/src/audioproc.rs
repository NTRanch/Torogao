//! Procedural audio: recipe validation, PRNG synthesis,
//! BS.1770 loudness and true-peak measurement, rendering —
//! ported from the D `torogao.audio.proc` module.

use serde_json::{json, Map, Value};

use crate::criteria::Finding;
use crate::util::{jget, jstr};
use crate::wav::{sha256_hex, write_wav24};

pub const AUDIO_ENGINE: &str = "torogao-audio/1";
pub const AUDIO_LICENSE: &str =
    "Original work of the studio; procedural synthesis from the recipe, no third-party audio.";
pub const AUDIO_SAMPLE_RATE: u32 = 48000;

pub const TG_AUDIO_DURATION: &str = "TG-AUDIO-DURATION";
pub const TG_AUDIO_ENVELOPE: &str = "TG-AUDIO-ENVELOPE";
pub const TG_AUDIO_FORMAT: &str = "TG-AUDIO-FORMAT";
pub const TG_AUDIO_FREQ: &str = "TG-AUDIO-FREQ";
pub const TG_AUDIO_GAIN: &str = "TG-AUDIO-GAIN";
pub const TG_AUDIO_SEED: &str = "TG-AUDIO-SEED";
pub const TG_AUDIO_STEM_TYPE: &str = "TG-AUDIO-STEM-TYPE";
pub const TG_AUDIO_SUBLIMINAL_SOURCE: &str = "TG-AUDIO-SUBLIMINAL-SOURCE";
pub const TG_AUDIO_TARGET: &str = "TG-AUDIO-TARGET";

fn get_d(v: &Value, key: &str, fallback: f64) -> f64 {
    match &jget(v, key) {
        Value::Number(n) => n.as_f64().unwrap_or(fallback),
        _ => fallback,
    }
}

/// D `getI`: integer, unsigned integer, or float cast; else fallback.
pub fn get_i(v: &Value, key: &str, fallback: i64) -> i64 {
    match &jget(v, key) {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i
            } else if let Some(u) = n.as_u64() {
                u as i64
            } else if let Some(f) = n.as_f64() {
                f as i64
            } else {
                fallback
            }
        }
        _ => fallback,
    }
}

fn is_int_like(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.is_i64() || n.is_u64())
}

// -- PRNG ------------------------------------------------------

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub struct Xoshiro256 {
    s: [u64; 4],
}

impl Xoshiro256 {
    /// Builds a generator from a seed.
    pub fn from_seed(seed: u64) -> Self {
        let mut state = seed;
        let mut s = [0u64; 4];
        for slot in &mut s {
            *slot = splitmix64(&mut state);
        }
        Xoshiro256 { s }
    }

    fn next(&mut self) -> u64 {
        let result = (self.s[1].wrapping_mul(5)).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Next uniform sample in [-1, 1).
    pub fn uniform(&mut self) -> f64 {
        ((self.next() >> 11) as f64 / 9_007_199_254_740_992.0) * 2.0 - 1.0
    }
}

// -- Recipe validation ------------------------------------------

pub fn validate_recipe(recipe: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    let add = |code: &'static str, message: String, findings: &mut Vec<Finding>| {
        findings.push(Finding::new(code, "error", String::new(), message));
    };
    if !recipe.is_object() {
        add(
            TG_AUDIO_FORMAT,
            "recipe must be a JSON object".to_string(),
            &mut findings,
        );
        return findings;
    }
    if jstr(recipe, "format") != "torogao-audio-recipe/1" {
        add(
            TG_AUDIO_FORMAT,
            "format must be \"torogao-audio-recipe/1\"".to_string(),
            &mut findings,
        );
    }
    if !is_int_like(jget(recipe, "seed")) {
        add(
            TG_AUDIO_SEED,
            "seed (integer) is required; rendering must be seeded".to_string(),
            &mut findings,
        );
    }
    let duration = get_d(recipe, "durationSeconds", 0.0);
    if duration <= 0.0 || duration > 600.0 {
        add(
            TG_AUDIO_DURATION,
            "durationSeconds must be in (0, 600]".to_string(),
            &mut findings,
        );
    }
    let target = get_d(recipe, "targetLufs", -16.0);
    if !(-60.0..=-6.0).contains(&target) {
        add(
            TG_AUDIO_TARGET,
            "targetLufs must be in [-60, -6]".to_string(),
            &mut findings,
        );
    }
    let ceiling = get_d(recipe, "truePeakDb", -1.0);
    if ceiling > 0.0 || ceiling < -20.0 {
        add(
            TG_AUDIO_TARGET,
            "truePeakDb must be in [-20, 0]".to_string(),
            &mut findings,
        );
    }
    let stems_v = jget(recipe, "stems");
    let Some(stems) = stems_v.as_array().filter(|a| !a.is_empty()) else {
        add(
            TG_AUDIO_STEM_TYPE,
            "stems must be a non-empty array".to_string(),
            &mut findings,
        );
        return findings;
    };
    for (i, stem) in stems.iter().enumerate() {
        let prefix = format!("stem {i}: ");
        let ty = jstr(stem, "type");
        if !["sine", "binaural", "noise", "tone", "subliminal"].contains(&ty) {
            add(
                TG_AUDIO_STEM_TYPE,
                format!("{prefix}unknown stem type \"{ty}\""),
                &mut findings,
            );
        }
        let gain = get_d(stem, "gain", -1.0);
        if !(0.0..=1.0).contains(&gain) {
            add(
                TG_AUDIO_GAIN,
                format!("{prefix}gain must be in [0, 1]"),
                &mut findings,
            );
        }
        match ty {
            "sine" | "tone" => {
                let f = get_d(stem, "freqHz", 0.0);
                if f <= 0.0 || f > 20000.0 {
                    add(
                        TG_AUDIO_FREQ,
                        format!("{prefix}freqHz must be in (0, 20000]"),
                        &mut findings,
                    );
                }
                // The glide endpoint (frequency-glide stems):
                // optional; when present it carries the same
                // range law as the start frequency.
                if stem.get("freqHzEnd").is_some() {
                    let f_end = get_d(stem, "freqHzEnd", 0.0);
                    if f_end <= 0.0 || f_end > 20000.0 {
                        add(
                            TG_AUDIO_FREQ,
                            format!("{prefix}freqHzEnd must be in (0, 20000]"),
                            &mut findings,
                        );
                    }
                }
            }
            "binaural" => {
                let f = get_d(stem, "baseHz", 0.0);
                if f <= 0.0 || f > 20000.0 {
                    add(
                        TG_AUDIO_FREQ,
                        format!("{prefix}baseHz must be in (0, 20000]"),
                        &mut findings,
                    );
                }
                let beat = get_d(stem, "beatHz", -1.0);
                if !(0.0..=100.0).contains(&beat) {
                    add(
                        TG_AUDIO_FREQ,
                        format!("{prefix}beatHz must be in [0, 100]"),
                        &mut findings,
                    );
                }
                // Glide endpoints for the carrier pair: the
                // base and/or the beat may travel across the
                // stem's window (the measured sweep-program
                // shape). Same range laws as the start values.
                if stem.get("baseHzEnd").is_some() {
                    let f_end = get_d(stem, "baseHzEnd", 0.0);
                    if f_end <= 0.0 || f_end > 20000.0 {
                        add(
                            TG_AUDIO_FREQ,
                            format!("{prefix}baseHzEnd must be in (0, 20000]"),
                            &mut findings,
                        );
                    }
                }
                if stem.get("beatHzEnd").is_some() {
                    let beat_end = get_d(stem, "beatHzEnd", -1.0);
                    if !(0.0..=100.0).contains(&beat_end) {
                        add(
                            TG_AUDIO_FREQ,
                            format!("{prefix}beatHzEnd must be in [0, 100]"),
                            &mut findings,
                        );
                    }
                }
            }
            "noise" => {
                let f = get_d(stem, "cutoffHz", 0.0);
                if f <= 0.0 || f > 20000.0 {
                    add(
                        TG_AUDIO_FREQ,
                        format!("{prefix}cutoffHz must be in (0, 20000]"),
                        &mut findings,
                    );
                }
            }
            "subliminal" => {
                let params = subliminal_params(stem, get_i(recipe, "seed", 0));
                if let Err(message) = params.check(AUDIO_SAMPLE_RATE) {
                    let code: &'static str =
                        if message.starts_with(crate::subliminal::TG_SUBLIMINAL_NYQUIST) {
                            crate::subliminal::TG_SUBLIMINAL_NYQUIST
                        } else {
                            crate::subliminal::TG_SUBLIMINAL_PARAM
                        };
                    let text = message
                        .split_once(": ")
                        .map_or(message.as_str(), |(_, t)| t);
                    findings.push(Finding::new(
                        code,
                        "error",
                        String::new(),
                        format!("{prefix}{text}"),
                    ));
                }
                if jstr(stem, "sourceStem").is_empty() {
                    add(
                        TG_AUDIO_SUBLIMINAL_SOURCE,
                        format!(
                            "{prefix}subliminal stem needs a sourceStem naming the stem it encodes"
                        ),
                        &mut findings,
                    );
                }
            }
            _ => {}
        }
        let env = jget(stem, "envelope");
        if env.is_object() {
            let attack = get_d(env, "attackSeconds", 0.0);
            let release = get_d(env, "releaseSeconds", 0.0);
            if attack < 0.0 || release < 0.0 {
                add(
                    TG_AUDIO_ENVELOPE,
                    format!("{prefix}envelope times must be >= 0"),
                    &mut findings,
                );
            }
        }
        let start = get_d(stem, "startSeconds", 0.0);
        if start < 0.0 || start >= duration {
            add(
                TG_AUDIO_DURATION,
                format!("{prefix}startSeconds must be in [0, duration)"),
                &mut findings,
            );
        }
    }
    // A subliminal stem encodes a named sibling stem: names
    // are unique, and the source must exist and not itself be
    // a subliminal stem.
    let mut named: Vec<String> = Vec::new();
    for stem in stems {
        let name = jstr(stem, "name");
        if !name.is_empty() {
            if named.iter().any(|n| n.as_str() == name) {
                add(
                    TG_AUDIO_STEM_TYPE,
                    format!("stem name \"{name}\" is used twice"),
                    &mut findings,
                );
            }
            named.push(name.to_string());
        }
    }
    for (i, stem) in stems.iter().enumerate() {
        if jstr(stem, "type") == "subliminal" {
            let source = jstr(stem, "sourceStem");
            let found = stems
                .iter()
                .find(|s| jstr(s, "name") == source && jstr(s, "type") != "subliminal");
            if !source.is_empty() && found.is_none() {
                add(
                    TG_AUDIO_SUBLIMINAL_SOURCE,
                    format!(
                        "stem {i}: sourceStem \"{source}\" names no non-subliminal stem in this recipe"
                    ),
                    &mut findings,
                );
            }
        }
    }
    findings
}

/// The encoding parameters a `subliminal` stem declares,
/// with the recipe seed as the carrier-phase seed.
pub fn subliminal_params(stem: &Value, seed: i64) -> crate::subliminal::SubliminalParams {
    crate::subliminal::SubliminalParams {
        carrier_hz: get_d(stem, "carrierHz", 16_000.0),
        band_low_hz: get_d(stem, "bandLowHz", crate::subliminal::BAND_LOW_DEFAULT_HZ),
        band_high_hz: get_d(stem, "bandHighHz", crate::subliminal::BAND_HIGH_DEFAULT_HZ),
        encoder_gain: get_d(stem, "encoderGain", crate::subliminal::ENCODER_GAIN_DEFAULT),
        seed: seed as u64,
    }
}

// -- Loudness ---------------------------------------------------

struct KWeight {
    x1: [f64; 2],
    y1: [f64; 2],
    x2: [f64; 2],
    y2: [f64; 2],
}

impl KWeight {
    fn new() -> Self {
        KWeight {
            x1: [0.0; 2],
            y1: [0.0; 2],
            x2: [0.0; 2],
            y2: [0.0; 2],
        }
    }

    fn process(&mut self, x0: f64) -> f64 {
        const B1: [f64; 3] = [
            1.535_124_859_586_97,
            -2.691_696_189_406_38,
            1.198_392_810_852_85,
        ];
        const A1: [f64; 3] = [1.0, -1.690_659_293_182_41, 0.732_480_774_215_85];
        const B2: [f64; 3] = [1.0, -2.0, 1.0];
        const A2: [f64; 3] = [1.0, -1.990_047_454_833_98, 0.990_072_250_366_21];
        let y0 = B1[0] * x0 + B1[1] * self.x1[0] + B1[2] * self.x1[1]
            - A1[1] * self.y1[0]
            - A1[2] * self.y1[1];
        self.x1[1] = self.x1[0];
        self.x1[0] = x0;
        self.y1[1] = self.y1[0];
        self.y1[0] = y0;
        let z = B2[0] * y0 + B2[1] * self.x2[0] + B2[2] * self.x2[1]
            - A2[1] * self.y2[0]
            - A2[2] * self.y2[1];
        self.x2[1] = self.x2[0];
        self.x2[0] = y0;
        self.y2[1] = self.y2[0];
        self.y2[0] = z;
        z
    }
}

fn block_loudness(mean_squares: &[f64]) -> f64 {
    let sum: f64 = mean_squares.iter().sum();
    -0.691 + 10.0 * sum.log10()
}

pub fn measure_lufs(channels: &[Vec<f64>], sample_rate: u32) -> f64 {
    let frames = if channels.is_empty() {
        0
    } else {
        channels[0].len()
    };
    let block_len = (0.4 * sample_rate as f64) as usize;
    let hop = (0.1 * sample_rate as f64) as usize;
    if block_len == 0 || hop == 0 || frames < block_len {
        return f64::NAN;
    }
    // K-weight each channel once.
    let weighted: Vec<Vec<f64>> = channels
        .iter()
        .map(|ch| {
            let mut kw = KWeight::new();
            ch.iter().map(|&s| kw.process(s)).collect()
        })
        .collect();
    let mut block_ms: Vec<Vec<f64>> = Vec::new();
    let mut start = 0usize;
    while start + block_len <= frames {
        let mut ms: Vec<f64> = Vec::with_capacity(channels.len());
        for ch in &weighted {
            let mut sum = 0.0f64;
            for &s in &ch[start..start + block_len] {
                sum += s * s;
            }
            ms.push(sum / block_len as f64);
        }
        block_ms.push(ms);
        start += hop;
    }
    let kept: Vec<&Vec<f64>> = block_ms
        .iter()
        .filter(|ms| block_loudness(ms) > -70.0)
        .collect();
    if kept.is_empty() {
        return f64::NAN;
    }
    let mean_of = |blocks: &[&Vec<f64>]| -> Vec<f64> {
        let mut out = vec![0.0; channels.len()];
        for b in blocks {
            for (c, v) in b.iter().enumerate() {
                out[c] += v;
            }
        }
        for v in &mut out {
            *v /= blocks.len() as f64;
        }
        out
    };
    let threshold = block_loudness(&mean_of(&kept)) - 10.0;
    let kept2: Vec<&Vec<f64>> = kept
        .iter()
        .copied()
        .filter(|ms| block_loudness(ms) > threshold)
        .collect();
    let final_blocks = if kept2.is_empty() { &kept } else { &kept2 };
    block_loudness(&mean_of(final_blocks))
}

// -- True peak ----------------------------------------------------

fn blackman32(u: f64) -> f64 {
    if !(-16.0..=16.0).contains(&u) {
        return 0.0;
    }
    let p = (u + 16.0) / 32.0;
    0.42 - 0.5 * (2.0 * std::f64::consts::PI * p).cos()
        + 0.08 * (4.0 * std::f64::consts::PI * p).cos()
}

fn true_peak_weights() -> [[f64; 4]; 32] {
    // weights[k + 15][q - 1] for k in -15..=16, q in 1..4
    let mut out = [[0.0f64; 4]; 32];
    for q in 1..4usize {
        for k in -15..=16i32 {
            let u = q as f64 * 0.25 - k as f64;
            let sinc = if u == 0.0 {
                1.0
            } else {
                (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u)
            };
            out[(k + 15) as usize][q - 1] = sinc * blackman32(u);
        }
    }
    out
}

pub fn measure_true_peak_db(channels: &[Vec<f64>]) -> f64 {
    let weights = true_peak_weights();
    let mut peak = 0.0f64;
    for ch in channels {
        let n = ch.len() as i64;
        for i in 0..n {
            peak = peak.max(ch[i as usize].abs());
            let mut accs = [0.0f64; 3];
            for k in -15..=16i64 {
                let idx = i + k;
                let s = if idx < 0 || idx >= n {
                    0.0
                } else {
                    ch[idx as usize]
                };
                for (acc, w) in accs
                    .iter_mut()
                    .zip(weights[(k + 15) as usize].iter().take(3))
                {
                    *acc += s * w;
                }
            }
            for acc in accs {
                peak = peak.max(acc.abs());
            }
        }
    }
    if peak <= 0.0 {
        -120.0
    } else {
        20.0 * peak.log10()
    }
}

// -- Synthesis ----------------------------------------------------

fn envelope_at(stem: &Value, t: f64, stem_dur: f64) -> f64 {
    let start = get_d(stem, "startSeconds", 0.0);
    let local = t - start;
    if local < 0.0 || local >= stem_dur {
        return 0.0;
    }
    let env = jget(stem, "envelope");
    let attack = get_d(env, "attackSeconds", 0.0);
    let release = get_d(env, "releaseSeconds", 0.0);
    let mut g = 1.0f64;
    if attack > 0.0 && local < attack {
        g = local / attack;
    }
    if release > 0.0 && local > stem_dur - release {
        g = (stem_dur - local) / release;
    }
    g
}

/// The render census of one `subliminal` stem: its declared
/// encoding parameters and the measured fraction of its
/// energy inside the transplanted band. The render refuses
/// a stem whose measured spectrum escapes the band law.
#[derive(Clone, Debug)]
pub struct SubliminalCensus {
    /// Index of the stem in the recipe.
    pub stem: usize,
    /// The stem's name, when it carries one.
    pub name: String,
    /// Carrier frequency in Hz.
    pub carrier_hz: f64,
    /// Speech-band low edge in Hz.
    pub band_low_hz: f64,
    /// Speech-band high edge in Hz.
    pub band_high_hz: f64,
    /// Encoder gain.
    pub encoder_gain: f64,
    /// Measured in-band energy fraction of the encoded stem.
    pub band_fraction: f64,
}

/// The phase, in cycles, of a tone whose frequency glides
/// linearly from `f0` to `f1` across `dur` seconds of local
/// stem time: the integral of the frequency line, so the
/// glide is phase-continuous by construction (no clicks at
/// any sample). At `f0 == f1` it reduces to the constant
/// tone's phase. Deterministic in its arguments.
fn glide_phase_cycles(f0: f64, f1: f64, tau: f64, dur: f64) -> f64 {
    if dur <= 0.0 {
        return f0 * tau;
    }
    f0 * tau + (f1 - f0) * tau * tau / (2.0 * dur)
}

/// Renders one non-subliminal stem into its own stereo
/// buffer. The per-type math is the renderer's standing
/// math; rendering per stem (instead of straight into the
/// mix) is what lets a `subliminal` stem encode a named
/// sibling's samples.
fn synthesize_stem(stem: &Value, seed: u64, duration: f64, frames: usize) -> [Vec<f64>; 2] {
    let mut channels = [vec![0.0f64; frames], vec![0.0f64; frames]];
    let ty = jstr(stem, "type");
    let gain = get_d(stem, "gain", 0.0);
    let start = get_d(stem, "startSeconds", 0.0);
    let stem_dur = get_d(stem, "durationSeconds", duration - start);
    let rate = AUDIO_SAMPLE_RATE as f64;
    let n0 = (start * rate) as usize;
    let n1 = (((start + stem_dur) * rate) as usize).min(frames);
    match ty {
        "sine" | "tone" => {
            let f = get_d(stem, "freqHz", 0.0);
            let f_end = get_d(stem, "freqHzEnd", f);
            if f_end == f {
                for n in n0..n1 {
                    let t = n as f64 / rate;
                    let v = gain
                        * (2.0 * std::f64::consts::PI * f * t).sin()
                        * envelope_at(stem, t, stem_dur);
                    channels[0][n] += v;
                    channels[1][n] += v;
                }
            } else {
                // Frequency glide: phase is the integral of
                // the frequency line over local stem time.
                for n in n0..n1 {
                    let t = n as f64 / rate;
                    let tau = t - start;
                    let v = gain
                        * (2.0
                            * std::f64::consts::PI
                            * glide_phase_cycles(f, f_end, tau, stem_dur))
                        .sin()
                        * envelope_at(stem, t, stem_dur);
                    channels[0][n] += v;
                    channels[1][n] += v;
                }
            }
        }
        "binaural" => {
            let base = get_d(stem, "baseHz", 0.0);
            let beat = get_d(stem, "beatHz", 0.0);
            let base_end = get_d(stem, "baseHzEnd", base);
            let beat_end = get_d(stem, "beatHzEnd", beat);
            let fl = base - beat / 2.0;
            let fr = base + beat / 2.0;
            if base_end == base && beat_end == beat {
                for n in n0..n1 {
                    let t = n as f64 / rate;
                    let e = envelope_at(stem, t, stem_dur);
                    channels[0][n] += gain * (2.0 * std::f64::consts::PI * fl * t).sin() * e;
                    channels[1][n] += gain * (2.0 * std::f64::consts::PI * fr * t).sin() * e;
                }
            } else {
                // Carrier glide: each ear's frequency travels
                // its own line (base and beat both glide), and
                // each ear's phase is that line's integral.
                let fl_end = base_end - beat_end / 2.0;
                let fr_end = base_end + beat_end / 2.0;
                for n in n0..n1 {
                    let t = n as f64 / rate;
                    let tau = t - start;
                    let e = envelope_at(stem, t, stem_dur);
                    channels[0][n] += gain
                        * (2.0
                            * std::f64::consts::PI
                            * glide_phase_cycles(fl, fl_end, tau, stem_dur))
                        .sin()
                        * e;
                    channels[1][n] += gain
                        * (2.0
                            * std::f64::consts::PI
                            * glide_phase_cycles(fr, fr_end, tau, stem_dur))
                        .sin()
                        * e;
                }
            }
        }
        "noise" => {
            let cutoff = get_d(stem, "cutoffHz", 1000.0);
            let a = 1.0 - (-2.0 * std::f64::consts::PI * cutoff / rate).exp();
            for (ch_idx, channel) in channels.iter_mut().enumerate() {
                let mut rng = Xoshiro256::from_seed(seed.wrapping_add(7919 * (ch_idx as u64 + 1)));
                let mut lp = 0.0f64;
                for (n, sample) in channel.iter_mut().enumerate().take(n1) {
                    lp += a * (rng.uniform() - lp);
                    if n >= n0 {
                        let t = n as f64 / rate;
                        *sample += gain * lp * envelope_at(stem, t, stem_dur);
                    }
                }
            }
        }
        _ => {}
    }
    channels
}

/// Renders one `subliminal` stem: encodes the named source
/// stem's mono sum through `subliminal-encode/1`, then
/// applies this stem's gain, envelope and window. A source
/// that cannot be encoded (validation refuses such recipes
/// before render) renders silence.
fn synthesize_subliminal_stem(
    stem: &Value,
    source: &[Vec<f64>; 2],
    seed: i64,
    duration: f64,
    frames: usize,
) -> ([Vec<f64>; 2], f64) {
    let mut channels = [vec![0.0f64; frames], vec![0.0f64; frames]];
    let params = subliminal_params(stem, seed);
    let mono: Vec<f64> = source[0]
        .iter()
        .zip(source[1].iter())
        .map(|(l, r)| (l + r) * 0.5)
        .collect();
    let Ok(encoded) = crate::subliminal::encode_subliminal(&mono, &params, AUDIO_SAMPLE_RATE)
    else {
        return (channels, 0.0);
    };
    // The spectrum census measures the encoded stem itself,
    // before this stem's gain and envelope shape it.
    let band_fraction = crate::subliminal::band_energy_fraction(
        &encoded,
        AUDIO_SAMPLE_RATE,
        params.carrier_hz + params.band_low_hz,
        params.carrier_hz + params.band_high_hz,
    );
    let gain = get_d(stem, "gain", 0.0);
    let start = get_d(stem, "startSeconds", 0.0);
    let stem_dur = get_d(stem, "durationSeconds", duration - start);
    let rate = AUDIO_SAMPLE_RATE as f64;
    let n0 = (start * rate) as usize;
    let n1 = (((start + stem_dur) * rate) as usize).min(frames);
    for n in n0..n1 {
        let t = n as f64 / rate;
        let v = gain * encoded[n] * envelope_at(stem, t, stem_dur);
        channels[0][n] += v;
        channels[1][n] += v;
    }
    (channels, band_fraction)
}

/// Synthesizes the mix and the per-stem subliminal census.
/// Buffers are summed in recipe order, so a recipe without
/// subliminal stems renders exactly what it always did.
pub fn synthesize_mix(recipe: &Value, frames: usize) -> (Vec<Vec<f64>>, Vec<SubliminalCensus>) {
    let duration = get_d(recipe, "durationSeconds", 0.0);
    let seed = get_i(recipe, "seed", 0);
    let stems_v = jget(recipe, "stems");
    let Some(stems) = stems_v.as_array() else {
        return (vec![vec![0.0f64; frames], vec![0.0f64; frames]], Vec::new());
    };
    let mut buffers: Vec<Option<[Vec<f64>; 2]>> = Vec::with_capacity(stems.len());
    let mut census: Vec<SubliminalCensus> = Vec::new();
    for stem in stems {
        if jstr(stem, "type") == "subliminal" {
            buffers.push(None);
            continue;
        }
        buffers.push(Some(synthesize_stem(stem, seed as u64, duration, frames)));
    }
    for (index, stem) in stems.iter().enumerate() {
        if jstr(stem, "type") != "subliminal" {
            continue;
        }
        let source_name = jstr(stem, "sourceStem");
        let source = stems.iter().enumerate().find_map(|(i, s)| {
            if jstr(s, "name") == source_name && jstr(s, "type") != "subliminal" {
                buffers[i].as_ref()
            } else {
                None
            }
        });
        let Some(source) = source else {
            buffers[index] = Some([vec![0.0f64; frames], vec![0.0f64; frames]]);
            continue;
        };
        let (rendered, band_fraction) =
            synthesize_subliminal_stem(stem, source, seed, duration, frames);
        let params = subliminal_params(stem, seed);
        census.push(SubliminalCensus {
            stem: index,
            name: jstr(stem, "name").to_string(),
            carrier_hz: params.carrier_hz,
            band_low_hz: params.band_low_hz,
            band_high_hz: params.band_high_hz,
            encoder_gain: params.encoder_gain,
            band_fraction,
        });
        buffers[index] = Some(rendered);
    }
    let mut channels = vec![vec![0.0f64; frames], vec![0.0f64; frames]];
    for buffer in buffers.into_iter().flatten() {
        for (channel, stem_channel) in channels.iter_mut().zip(buffer.iter()) {
            for (sample, value) in channel.iter_mut().zip(stem_channel.iter()) {
                *sample += value;
            }
        }
    }
    (channels, census)
}

pub fn synthesize(recipe: &Value, frames: usize) -> Vec<Vec<f64>> {
    synthesize_mix(recipe, frames).0
}

// -- Rendering ----------------------------------------------------

pub struct RenderResult {
    pub manifest: Value,
    pub error: String,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

pub fn render_recipe(recipe: &Value, recipe_text: &str, out_dir: &str) -> RenderResult {
    for f in validate_recipe(recipe) {
        if f.severity == "error" {
            return RenderResult {
                manifest: Value::Null,
                error: format!("{}: {}", f.code, f.message),
            };
        }
    }
    let duration = get_d(recipe, "durationSeconds", 0.0);
    let frames = (duration * AUDIO_SAMPLE_RATE as f64 + 0.5) as usize;
    let (mut channels, census) = synthesize_mix(recipe, frames);
    for entry in &census {
        if entry.band_fraction < crate::subliminal::SPECTRUM_BAND_MIN {
            return RenderResult {
                manifest: Value::Null,
                error: format!(
                    "{}: stem {} measured band energy fraction {:.4} below the law {}; refused, not rendered",
                    crate::subliminal::TG_SUBLIMINAL_SPECTRUM,
                    entry.stem,
                    entry.band_fraction,
                    crate::subliminal::SPECTRUM_BAND_MIN
                ),
            };
        }
    }
    let target = get_d(recipe, "targetLufs", -16.0);
    let ceiling = get_d(recipe, "truePeakDb", -1.0);
    let lufs = measure_lufs(&channels, AUDIO_SAMPLE_RATE);
    if lufs.is_nan() || !lufs.is_finite() {
        return RenderResult {
            manifest: Value::Null,
            error: "recipe renders silence (no measurable loudness)".to_string(),
        };
    }
    let mut gain_db = target - lufs;
    let scale = 10f64.powf(gain_db / 20.0);
    for ch in &mut channels {
        for s in ch.iter_mut() {
            *s *= scale;
        }
    }
    let mut tp = measure_true_peak_db(&channels);
    if tp > ceiling {
        let adjust = ceiling - tp;
        let s2 = 10f64.powf(adjust / 20.0);
        for ch in &mut channels {
            for s in ch.iter_mut() {
                *s *= s2;
            }
        }
        gain_db += adjust;
        tp = measure_true_peak_db(&channels);
    }
    let lufs_final = measure_lufs(&channels, AUDIO_SAMPLE_RATE);
    let name_v = jget(recipe, "name");
    let name = if let Some(s) = name_v.as_str().filter(|s| !s.is_empty()) {
        s.to_string()
    } else {
        "untitled".to_string()
    };
    let wav_bytes = write_wav24(&channels, AUDIO_SAMPLE_RATE);
    let wav_path = format!("{out_dir}/{name}.wav");
    if let Err(e) = std::fs::write(&wav_path, &wav_bytes) {
        return RenderResult {
            manifest: Value::Null,
            error: e.to_string(),
        };
    }
    let mut manifest = Map::new();
    manifest.insert("file".to_string(), json!(format!("{name}.wav")));
    manifest.insert("recipe".to_string(), json!(name));
    manifest.insert(
        "recipeSha256".to_string(),
        json!(sha256_hex(recipe_text.as_bytes())),
    );
    manifest.insert("seed".to_string(), json!(get_i(recipe, "seed", 0)));
    manifest.insert("engine".to_string(), json!(AUDIO_ENGINE));
    manifest.insert(
        "prng".to_string(),
        json!("xoshiro256** seeded via splitmix64"),
    );
    manifest.insert("sampleRate".to_string(), json!(AUDIO_SAMPLE_RATE));
    manifest.insert("channels".to_string(), json!(2));
    manifest.insert("bitsPerSample".to_string(), json!(24));
    manifest.insert(
        "durationSeconds".to_string(),
        json!(frames as f64 / AUDIO_SAMPLE_RATE as f64),
    );
    manifest.insert("measuredLufs".to_string(), json!(round3(lufs_final)));
    manifest.insert("measuredTruePeakDb".to_string(), json!(round3(tp)));
    manifest.insert("targetLufs".to_string(), json!(target));
    manifest.insert("truePeakCeilingDb".to_string(), json!(ceiling));
    manifest.insert("gainAppliedDb".to_string(), json!(round3(gain_db)));
    manifest.insert("license".to_string(), json!(AUDIO_LICENSE));
    if !census.is_empty() {
        let rows: Vec<Value> = census
            .iter()
            .map(|entry| {
                json!({
                    "stem": entry.stem,
                    "name": entry.name,
                    "encoder": crate::subliminal::ENCODE_FORMAT,
                    "carrierHz": entry.carrier_hz,
                    "bandLowHz": entry.band_low_hz,
                    "bandHighHz": entry.band_high_hz,
                    "encoderGain": entry.encoder_gain,
                    "bandEnergyFraction": (entry.band_fraction * 10_000.0).round() / 10_000.0,
                })
            })
            .collect();
        manifest.insert("subliminalStems".to_string(), Value::Array(rows));
    }
    let manifest_doc = Value::Object(manifest);
    let manifest_path = format!("{out_dir}/{name}.manifest.json");
    let text = torogao_format::jsonfmt::to_d_json(&manifest_doc);
    if let Err(e) = std::fs::write(&manifest_path, format!("{text}\n")) {
        return RenderResult {
            manifest: Value::Null,
            error: e.to_string(),
        };
    }
    RenderResult {
        manifest: manifest_doc,
        error: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn recipe(stems: Vec<Value>) -> Value {
        json!({
            "format": "torogao-audio-recipe/1",
            "name": "glide-test",
            "seed": 11,
            "durationSeconds": 4.0,
            "targetLufs": -16.0,
            "truePeakDb": -1.0,
            "stems": stems,
        })
    }

    fn error_codes(recipe: &Value) -> Vec<String> {
        validate_recipe(recipe)
            .iter()
            .filter(|f| f.severity == "error")
            .map(|f| f.code.to_string())
            .collect()
    }

    fn frames(seconds: f64) -> usize {
        (seconds * AUDIO_SAMPLE_RATE as f64) as usize
    }

    /// Crude but honest frequency estimate for a pure tone:
    /// sign changes per second, halved.
    fn zero_crossing_freq(samples: &[f64]) -> f64 {
        let mut crossings = 0usize;
        for pair in samples.windows(2) {
            if (pair[0] < 0.0) != (pair[1] < 0.0) {
                crossings += 1;
            }
        }
        crossings as f64 / 2.0 / (samples.len() as f64 / AUDIO_SAMPLE_RATE as f64)
    }

    // -- validation half ------------------------------------

    #[test]
    fn glide_stems_validate_clean() {
        let doc = recipe(vec![
            json!({"type": "sine", "freqHz": 100.0, "freqHzEnd": 200.0, "gain": 0.4}),
            json!({"type": "binaural", "baseHz": 200.0, "baseHzEnd": 180.0,
                   "beatHz": 10.0, "beatHzEnd": 2.0, "gain": 0.4}),
        ]);
        assert_eq!(error_codes(&doc), Vec::<String>::new());
    }

    #[test]
    fn glide_tone_travels_from_start_to_end_frequency() {
        let doc = recipe(vec![
            json!({"type": "sine", "freqHz": 100.0, "freqHzEnd": 200.0, "gain": 0.8}),
        ]);
        let (channels, _) = synthesize_mix(&doc, frames(4.0));
        let ch = &channels[0];
        let head = &ch[0..frames(0.25)];
        let tail = &ch[frames(3.75)..frames(4.0)];
        let head_low =
            crate::subliminal::band_energy_fraction(head, AUDIO_SAMPLE_RATE, 90.0, 110.0);
        let tail_high =
            crate::subliminal::band_energy_fraction(tail, AUDIO_SAMPLE_RATE, 190.0, 210.0);
        assert!(head_low > 0.9, "head energy near 100 Hz: {head_low}");
        assert!(tail_high > 0.9, "tail energy near 200 Hz: {tail_high}");
        let head_high =
            crate::subliminal::band_energy_fraction(head, AUDIO_SAMPLE_RATE, 190.0, 210.0);
        assert!(
            head_high < 0.1,
            "head must not start at the end frequency: {head_high}"
        );
    }

    #[test]
    fn descending_glide_travels_down() {
        // The measured sweep-program shape (video 89-zjkL7AFI):
        // the gliding pair spends its life descending.
        let doc = recipe(vec![
            json!({"type": "sine", "freqHz": 250.0, "freqHzEnd": 90.0, "gain": 0.8}),
        ]);
        let (channels, _) = synthesize_mix(&doc, frames(4.0));
        let tail = &channels[0][frames(3.75)..frames(4.0)];
        let near_end =
            crate::subliminal::band_energy_fraction(tail, AUDIO_SAMPLE_RATE, 85.0, 105.0);
        assert!(near_end > 0.9, "tail energy near 90 Hz: {near_end}");
    }

    #[test]
    fn binaural_beat_glide_descends() {
        // The measured session shape (video GU9k6JrAYyc):
        // carrier anchored, beat descending 10 -> 2 Hz.
        let doc = recipe(vec![
            json!({"type": "binaural", "baseHz": 200.0, "beatHz": 10.0,
                   "beatHzEnd": 2.0, "gain": 0.8}),
        ]);
        let (channels, _) = synthesize_mix(&doc, frames(4.0));
        let head_l = zero_crossing_freq(&channels[0][0..frames(0.5)]);
        let head_r = zero_crossing_freq(&channels[1][0..frames(0.5)]);
        assert!(
            (7.0..=13.0).contains(&(head_r - head_l)),
            "head beat {}",
            head_r - head_l
        );
        let tail_l = zero_crossing_freq(&channels[0][frames(3.5)..frames(4.0)]);
        let tail_r = zero_crossing_freq(&channels[1][frames(3.5)..frames(4.0)]);
        assert!(
            (0.5..=4.5).contains(&(tail_r - tail_l)),
            "tail beat {}",
            tail_r - tail_l
        );
    }

    #[test]
    fn glide_is_phase_continuous() {
        let doc = recipe(vec![
            json!({"type": "sine", "freqHz": 100.0, "freqHzEnd": 250.0, "gain": 1.0}),
        ]);
        let (channels, _) = synthesize_mix(&doc, frames(4.0));
        let max_step = channels[0]
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0f64, f64::max);
        // A phase-continuous sine at <= 250 Hz / 48 kHz steps
        // at most ~2*pi*250/48000 = 0.0327 per sample; a phase
        // break would step by up to twice the amplitude.
        assert!(
            max_step < 0.04,
            "max sample step {max_step} betrays a phase break"
        );
    }

    #[test]
    fn glide_render_is_deterministic() {
        let doc = recipe(vec![
            json!({"type": "binaural", "baseHz": 220.0, "baseHzEnd": 160.0,
                   "beatHz": 8.0, "beatHzEnd": 3.0, "gain": 0.5}),
        ]);
        let (a, _) = synthesize_mix(&doc, frames(4.0));
        let (b, _) = synthesize_mix(&doc, frames(4.0));
        assert_eq!(a, b);
    }

    #[test]
    fn constant_tone_keeps_its_standing_values() {
        // Regression anchor: without glide keys the tone path
        // is the standing math. 480 Hz at 48 kHz has a
        // 100-sample period; sample 25 sits at sin(pi/2) = 1.
        let doc = recipe(vec![json!({"type": "sine", "freqHz": 480.0, "gain": 0.5})]);
        let (channels, _) = synthesize_mix(&doc, frames(4.0));
        assert!(channels[0][0].abs() < 1e-12);
        assert!((channels[0][25] - 0.5).abs() < 1e-9, "{}", channels[0][25]);
        assert!((channels[0][50]).abs() < 1e-9, "{}", channels[0][50]);
    }

    // -- adversarial half -----------------------------------

    #[test]
    fn glide_endpoints_carry_the_range_law() {
        for (stem, what) in [
            (
                json!({"type": "sine", "freqHz": 100.0, "freqHzEnd": 0.0, "gain": 0.4}),
                "freqHzEnd 0",
            ),
            (
                json!({"type": "sine", "freqHz": 100.0, "freqHzEnd": 20001.0, "gain": 0.4}),
                "freqHzEnd over",
            ),
            (
                json!({"type": "binaural", "baseHz": 200.0, "baseHzEnd": -5.0, "beatHz": 4.0, "gain": 0.4}),
                "baseHzEnd negative",
            ),
            (
                json!({"type": "binaural", "baseHz": 200.0, "beatHz": 4.0, "beatHzEnd": 101.0, "gain": 0.4}),
                "beatHzEnd over",
            ),
            (
                json!({"type": "binaural", "baseHz": 200.0, "beatHz": 4.0, "beatHzEnd": -1.0, "gain": 0.4}),
                "beatHzEnd negative",
            ),
        ] {
            let doc = recipe(vec![stem]);
            assert!(
                error_codes(&doc).iter().any(|c| c == TG_AUDIO_FREQ),
                "{what} must refuse with TG_AUDIO_FREQ: {:?}",
                error_codes(&doc)
            );
        }
    }

    #[test]
    fn glide_stem_stays_inside_its_window() {
        let doc = recipe(vec![
            json!({"type": "sine", "freqHz": 100.0, "freqHzEnd": 200.0, "gain": 0.8,
                   "startSeconds": 2.0, "durationSeconds": 1.0}),
        ]);
        let (channels, _) = synthesize_mix(&doc, frames(4.0));
        assert!(channels[0][0..frames(2.0)].iter().all(|&s| s == 0.0));
        assert!(channels[0][frames(3.0)..].iter().all(|&s| s == 0.0));
        assert!(channels[0][frames(2.0)..frames(3.0)]
            .iter()
            .any(|&s| s != 0.0));
    }

    // -- the layer stack: edit / add / remove ---------------

    fn base_recipe() -> Value {
        recipe(vec![
            json!({"type": "noise", "cutoffHz": 800.0, "gain": 0.3}),
            json!({"type": "binaural", "baseHz": 200.0, "beatHz": 6.0, "gain": 0.4}),
        ])
    }

    fn glide_layer(freq_end: f64) -> Value {
        json!({"type": "sine", "freqHz": 250.0, "freqHzEnd": freq_end, "gain": 0.35})
    }

    #[test]
    fn adding_a_layer_changes_only_that_layer() {
        let base = base_recipe();
        let mut plus = base_recipe();
        plus["stems"]
            .as_array_mut()
            .expect("stems")
            .push(glide_layer(90.0));
        let solo = recipe(vec![glide_layer(90.0)]);
        let n = frames(4.0);
        let (mix_base, _) = synthesize_mix(&base, n);
        let (mix_plus, _) = synthesize_mix(&plus, n);
        let (mix_solo, _) = synthesize_mix(&solo, n);
        let mut max_dev = 0.0f64;
        for ch in 0..2 {
            for i in 0..n {
                let dev = ((mix_plus[ch][i] - mix_base[ch][i]) - mix_solo[ch][i]).abs();
                max_dev = max_dev.max(dev);
            }
        }
        assert!(max_dev < 1e-9, "layer contribution deviates by {max_dev}");
    }

    #[test]
    fn editing_the_layer_leaves_the_other_layers_untouched() {
        let mut plus_a = base_recipe();
        plus_a["stems"]
            .as_array_mut()
            .expect("stems")
            .push(glide_layer(90.0));
        let mut plus_b = base_recipe();
        plus_b["stems"]
            .as_array_mut()
            .expect("stems")
            .push(glide_layer(140.0));
        let solo_a = recipe(vec![glide_layer(90.0)]);
        let solo_b = recipe(vec![glide_layer(140.0)]);
        let n = frames(4.0);
        let (mix_a, _) = synthesize_mix(&plus_a, n);
        let (mix_b, _) = synthesize_mix(&plus_b, n);
        let (sa, _) = synthesize_mix(&solo_a, n);
        let (sb, _) = synthesize_mix(&solo_b, n);
        assert_ne!(sa, sb, "the edit must change the layer itself");
        let mut max_dev = 0.0f64;
        for ch in 0..2 {
            for i in 0..n {
                let rest_a = mix_a[ch][i] - sa[ch][i];
                let rest_b = mix_b[ch][i] - sb[ch][i];
                max_dev = max_dev.max((rest_a - rest_b).abs());
            }
        }
        assert!(max_dev < 1e-9, "the untouched layers moved by {max_dev}");
    }

    #[test]
    fn removing_the_layer_restores_the_base_exactly() {
        let base = base_recipe();
        let text = torogao_format::jsonfmt::to_d_json(&base);
        let root = std::env::temp_dir().join(format!("torogao-glide-{}", std::process::id()));
        let a = root.join("a");
        let b = root.join("b");
        std::fs::create_dir_all(&a).expect("dir a");
        std::fs::create_dir_all(&b).expect("dir b");
        let ra = render_recipe(&base, &text, a.to_str().expect("utf8"));
        let rb = render_recipe(&base, &text, b.to_str().expect("utf8"));
        assert!(ra.error.is_empty(), "{}", ra.error);
        assert!(rb.error.is_empty(), "{}", rb.error);
        let wav_a = std::fs::read(a.join("glide-test.wav")).expect("wav a");
        let wav_b = std::fs::read(b.join("glide-test.wav")).expect("wav b");
        assert_eq!(
            wav_a, wav_b,
            "base renders byte-identically with the layer absent"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
