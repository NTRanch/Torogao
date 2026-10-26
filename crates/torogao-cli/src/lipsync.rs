//! Lip-sync analysis: per-frame RMS / zero-crossing / spectral
//! centroid features classified into visemes, ported from the
//! D `torogao.cli.lipsync` module. Output is a PROPOSAL only.

use serde_json::{Map, Value, json};

use crate::viseme::{VisemeContract, bake_visemes};
use crate::wav::{parse_wav, sha256_hex};

pub const TG_LIPSYNC_INPUT: &str = "TG-LIPSYNC-INPUT";

struct Features {
    rms: f64,
    zcr: f64,
    centroid: f64,
    viseme: &'static str,
}

fn measure_frame(samples: &[f64], rate: u32) -> Features {
    if samples.is_empty() {
        return Features {
            rms: 0.0,
            zcr: 0.0,
            centroid: 0.0,
            viseme: "sil",
        };
    }
    let n = samples.len();
    let mut sum = 0.0f64;
    for &s in samples {
        sum += s * s;
    }
    let rms = (sum / n as f64).sqrt();
    let mut crossings = 0usize;
    for i in 1..n {
        if (samples[i] < 0.0) != (samples[i - 1] < 0.0) {
            crossings += 1;
        }
    }
    let zcr = crossings as f64 / n as f64;
    // Hann-windowed DFT magnitudes over the lower half spectrum.
    let half = n / 2;
    let mut weighted = 0.0f64;
    let mut total = 0.0f64;
    for k in 1..half {
        let step = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
        let mut re = 0.0f64;
        let mut im = 0.0f64;
        for (j, &s) in samples.iter().enumerate() {
            let window =
                0.5 * (1.0 - (2.0 * std::f64::consts::PI * j as f64 / (n as f64 - 1.0)).cos());
            let angle = step * j as f64;
            re += s * window * angle.cos();
            im -= s * window * angle.sin();
        }
        let mag = (re * re + im * im).sqrt();
        let freq = k as f64 * rate as f64 / n as f64;
        weighted += freq * mag;
        total += mag;
    }
    let centroid = if total > 0.0 { weighted / total } else { 0.0 };
    let viseme = classify(rms, zcr, centroid);
    Features {
        rms,
        zcr,
        centroid,
        viseme,
    }
}

fn classify(rms: f64, zcr: f64, centroid: f64) -> &'static str {
    if rms < 0.01 {
        "sil"
    } else if zcr > 0.25 && centroid > 4000.0 {
        "SS"
    } else if zcr > 0.18 && centroid > 3000.0 {
        "FF"
    } else if centroid < 800.0 {
        "oh"
    } else if centroid < 1300.0 {
        "ou"
    } else if centroid < 2200.0 {
        "aa"
    } else if centroid < 3000.0 {
        "E"
    } else {
        "ih"
    }
}

pub fn analyze_lipsync(wav_path: &str, contract: &VisemeContract) -> Result<Value, String> {
    let bytes =
        std::fs::read(wav_path).map_err(|e| format!("{TG_LIPSYNC_INPUT}: cannot read WAV: {e}"))?;
    let wav = parse_wav(&bytes).map_err(|e| format!("{TG_LIPSYNC_INPUT}: cannot read WAV: {e}"))?;
    let frames = wav.frames();
    if wav.channels.is_empty() || frames == 0 {
        return Err(format!("{TG_LIPSYNC_INPUT}: WAV carries no samples"));
    }
    let frame_samples = (wav.sample_rate / 60) as usize;
    if frame_samples == 0 {
        return Err(format!(
            "{TG_LIPSYNC_INPUT}: sample rate below the analysis rate"
        ));
    }
    let mut frame_count = frames / frame_samples;
    if frame_count == 0 {
        frame_count = 1;
    }
    if frame_count > 36000 {
        return Err(format!(
            "{TG_LIPSYNC_INPUT}: WAV longer than the 600 s analysis bound"
        ));
    }
    let n_channels = wav.channels.len() as f64;
    let mut mono: Vec<f64> = vec![0.0; frames];
    for ch in &wav.channels {
        for (i, &s) in ch.iter().enumerate() {
            mono[i] += s / n_channels;
        }
    }
    // Measure frames and merge consecutive equal visemes.
    struct Event {
        viseme: &'static str,
        first: usize,
        last: usize,
        rms_sum: f64,
        zcr_sum: f64,
        centroid_sum: f64,
        count: usize,
    }
    let mut events: Vec<Event> = Vec::new();
    for f in 0..frame_count {
        let start = f * frame_samples;
        let end = ((f + 1) * frame_samples).min(frames);
        let feat = measure_frame(&mono[start..end], wav.sample_rate);
        match events.last_mut() {
            Some(ev) if ev.viseme == feat.viseme => {
                ev.last = f + 1;
                ev.rms_sum += feat.rms;
                ev.zcr_sum += feat.zcr;
                ev.centroid_sum += feat.centroid;
                ev.count += 1;
            }
            _ => events.push(Event {
                viseme: feat.viseme,
                first: f,
                last: f + 1,
                rms_sum: feat.rms,
                zcr_sum: feat.zcr,
                centroid_sum: feat.centroid,
                count: 1,
            }),
        }
    }
    let duration = frame_count as f64 / 60.0;
    let mut track_events: Vec<Value> = Vec::new();
    let mut feature_docs: Vec<Value> = Vec::new();
    for ev in &events {
        let start = ev.first as f64 / 60.0;
        let end = ev.last as f64 / 60.0;
        let mut te = Map::new();
        te.insert("viseme".to_string(), json!(ev.viseme));
        te.insert("start".to_string(), json!(start));
        te.insert("end".to_string(), json!(end));
        track_events.push(Value::Object(te));
        let mut fd = Map::new();
        fd.insert("viseme".to_string(), json!(ev.viseme));
        fd.insert("start".to_string(), json!(start));
        fd.insert("end".to_string(), json!(end));
        fd.insert("meanRms".to_string(), json!(ev.rms_sum / ev.count as f64));
        fd.insert("meanZcr".to_string(), json!(ev.zcr_sum / ev.count as f64));
        fd.insert(
            "meanCentroidHz".to_string(),
            json!(ev.centroid_sum / ev.count as f64),
        );
        feature_docs.push(Value::Object(fd));
    }
    let mut track = Map::new();
    track.insert("format".to_string(), json!("torogao-viseme-track/1"));
    track.insert("fps".to_string(), json!(60));
    track.insert("durationSeconds".to_string(), json!(duration));
    track.insert("blendSeconds".to_string(), json!(0.06));
    track.insert("events".to_string(), Value::Array(track_events));
    let track = Value::Object(track);
    let curves = bake_visemes(&track, contract)
        .map_err(|e| format!("{TG_LIPSYNC_INPUT}: proposal track failed to bake: {e}"))?;
    let mut doc = Map::new();
    doc.insert("format".to_string(), json!("torogao-lipsync-proposal/1"));
    doc.insert("result".to_string(), json!("proposal"));
    doc.insert("source".to_string(), json!(wav_path));
    doc.insert("wavSha256".to_string(), json!(sha256_hex(&bytes)));
    doc.insert("sampleRate".to_string(), json!(wav.sample_rate));
    doc.insert("fps".to_string(), json!(60));
    doc.insert("durationSeconds".to_string(), json!(duration));
    doc.insert("track".to_string(), track);
    doc.insert("curves".to_string(), curves.to_json_value());
    doc.insert("features".to_string(), Value::Array(feature_docs));
    doc.insert("note".to_string(), json!("PROPOSAL ONLY — analysis output for human review. Nothing applies it automatically; fold it into a preview by naming the proposal's track through physics-preview --viseme, or author it into a project deliberately."));
    Ok(Value::Object(doc))
}
