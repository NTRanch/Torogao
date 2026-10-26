//! Procedural audio: recipe validation, PRNG synthesis,
//! BS.1770 loudness and true-peak measurement, rendering —
//! ported from the D `torogao.audio.proc` module.

use serde_json::{Map, Value, json};

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
        if !["sine", "binaural", "noise", "tone"].contains(&ty) {
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
    findings
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

pub fn synthesize(recipe: &Value, frames: usize) -> Vec<Vec<f64>> {
    let mut channels = vec![vec![0.0f64; frames], vec![0.0f64; frames]];
    let duration = get_d(recipe, "durationSeconds", 0.0);
    let seed = get_i(recipe, "seed", 0) as u64;
    let stems_v = jget(recipe, "stems");
    let Some(stems) = stems_v.as_array() else {
        return channels;
    };
    let rate = AUDIO_SAMPLE_RATE as f64;
    for stem in stems {
        let ty = jstr(stem, "type");
        let gain = get_d(stem, "gain", 0.0);
        let start = get_d(stem, "startSeconds", 0.0);
        let stem_dur = get_d(stem, "durationSeconds", duration - start);
        let n0 = (start * rate) as usize;
        let n1 = (((start + stem_dur) * rate) as usize).min(frames);
        match ty {
            "sine" | "tone" => {
                let f = get_d(stem, "freqHz", 0.0);
                for n in n0..n1 {
                    let t = n as f64 / rate;
                    let v = gain
                        * (2.0 * std::f64::consts::PI * f * t).sin()
                        * envelope_at(stem, t, stem_dur);
                    channels[0][n] += v;
                    channels[1][n] += v;
                }
            }
            "binaural" => {
                let base = get_d(stem, "baseHz", 0.0);
                let beat = get_d(stem, "beatHz", 0.0);
                let fl = base - beat / 2.0;
                let fr = base + beat / 2.0;
                for n in n0..n1 {
                    let t = n as f64 / rate;
                    let e = envelope_at(stem, t, stem_dur);
                    channels[0][n] += gain * (2.0 * std::f64::consts::PI * fl * t).sin() * e;
                    channels[1][n] += gain * (2.0 * std::f64::consts::PI * fr * t).sin() * e;
                }
            }
            "noise" => {
                let cutoff = get_d(stem, "cutoffHz", 1000.0);
                let a = 1.0 - (-2.0 * std::f64::consts::PI * cutoff / rate).exp();
                for (ch_idx, channel) in channels.iter_mut().enumerate() {
                    let mut rng =
                        Xoshiro256::from_seed(seed.wrapping_add(7919 * (ch_idx as u64 + 1)));
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
    }
    channels
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
    let mut channels = synthesize(recipe, frames);
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
