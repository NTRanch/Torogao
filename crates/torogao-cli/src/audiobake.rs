//! Audio envelope baking, ported from the D `torogao.audio.bake`.

use serde_json::Value;

use crate::curves::{CurveSeries, CurveSet, quote};
use crate::util::jget;
use crate::wav::WavData;

pub fn bake_envelope(
    wav: &WavData,
    recipe: &Value,
    source_label: &str,
) -> Result<CurveSet, String> {
    let mut fps: i64 = 60;
    let mut target = "AudioLevel".to_string();
    if recipe.is_object() {
        if let Value::Number(n) = &jget(recipe, "fps") {
            if let Some(i) = n.as_i64() {
                fps = i;
            } else if let Some(f) = n.as_f64() {
                fps = f as i64;
            }
        }
        if let Some(s) = jget(recipe, "target").as_str() {
            target = s.to_string();
        }
        let format = crate::util::jstr(recipe, "format");
        if !format.is_empty() && format != "torogao-audio-bake/1" {
            return Err("bake recipe format must be \"torogao-audio-bake/1\"".to_string());
        }
    }
    if ![15, 30, 60, 120].contains(&fps) {
        return Err("bake fps must be one of 15, 30, 60, 120".to_string());
    }
    let frames = wav.frames();
    if wav.sample_rate == 0 || frames == 0 {
        return Err("audio is empty".to_string());
    }
    let samples_per_step = (wav.sample_rate as i64 / fps) as usize;
    if samples_per_step as i64 * fps != wav.sample_rate as i64 {
        return Err("sample rate does not divide evenly at this fps".to_string());
    }
    let dt_sample = 1.0 / wav.sample_rate as f64;
    let alpha_attack = 1.0 - (-dt_sample / 0.010).exp();
    let alpha_release = 1.0 - (-dt_sample / 0.080).exp();
    let steps = frames / samples_per_step;
    let mut values: Vec<f64> = Vec::with_capacity(steps);
    let mut env = 0.0f64;
    let n_channels = wav.channels.len() as f64;
    for (n, _) in (0..frames).enumerate() {
        let mut mono = 0.0f64;
        for ch in &wav.channels {
            mono += ch[n];
        }
        mono /= n_channels;
        let a = mono.abs();
        let alpha = if a > env { alpha_attack } else { alpha_release };
        env += alpha * (a - env);
        if (n + 1) % samples_per_step == 0 {
            values.push(env);
        }
    }
    let mut set = CurveSet::new("audio-bake", 1.0 / fps as f64);
    set.series.push(CurveSeries {
        parameter: target,
        axis: 0,
        values,
    });
    set.step_count = steps as i64;
    set.extra.push(("file".to_string(), quote(source_label)));
    set.extra
        .push(("sampleRate".to_string(), wav.sample_rate.to_string()));
    set.extra.push((
        "attackTau".to_string(),
        torogao_format::jsonfmt::to_d_json(&serde_json::json!(0.01)),
    ));
    set.extra.push((
        "releaseTau".to_string(),
        torogao_format::jsonfmt::to_d_json(&serde_json::json!(0.08)),
    ));
    Ok(set)
}
