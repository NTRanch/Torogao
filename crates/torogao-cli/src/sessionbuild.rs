//! Session validation and session-to-recipe compilation,
//! ported from the D `torogao.cli.sessionbuild` module.

use serde_json::{json, Map, Value};

use crate::audioproc::validate_recipe;
use crate::criteria::Finding;
use crate::util::{jget, jstr};

pub const SESSION_FORMAT: &str = "torogao-session/1";
pub const TG_SESSION_CARRIER: &str = "TG-SESSION-CARRIER";
pub const TG_SESSION_BED: &str = "TG-SESSION-BED";
pub const TG_SESSION_CROSSFADE: &str = "TG-SESSION-CROSSFADE";
pub const TG_SESSION_FORMAT: &str = "TG-SESSION-FORMAT";
pub const TG_SESSION_PHASE: &str = "TG-SESSION-PHASE";
pub const TG_SESSION_SUBLIMINAL_LEVEL: &str = "TG-SESSION-SUBLIMINAL-LEVEL";
pub const TG_SESSION_SUBLIMINAL_PEAK: &str = "TG-SESSION-SUBLIMINAL-PEAK";

fn num(v: &Value, key: &str, fallback: f64) -> f64 {
    match &jget(v, key) {
        Value::Number(n) => n.as_f64().unwrap_or(fallback),
        _ => fallback,
    }
}

fn is_int_like(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.is_i64() || n.is_u64())
}

fn finding(code: &'static str, message: String) -> Finding {
    Finding::new(code, "error", "session", message)
}

pub fn session_reference_gain(session: &Value) -> f64 {
    let bed = jget(session, "bed");
    if bed.is_object() {
        num(&bed, "gain", 0.0)
    } else {
        num(&jget(session, "carrier"), "gain", 0.0)
    }
}

pub fn subliminal_gain(session: &Value) -> f64 {
    let sub = jget(session, "subliminal");
    if !sub.is_object() {
        return 0.0;
    }
    session_reference_gain(session) * 10f64.powf(num(&sub, "levelDb", 0.0) / 20.0)
}

pub fn validate_session(session: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !session.is_object() || jstr(session, "format") != SESSION_FORMAT {
        findings.push(finding(
            TG_SESSION_FORMAT,
            "format must be \"torogao-session/1\"".to_string(),
        ));
        return findings;
    }
    if jstr(session, "name").is_empty() {
        findings.push(finding(TG_SESSION_FORMAT, "name is required".to_string()));
    }
    if !is_int_like(&jget(session, "seed")) {
        findings.push(finding(
            TG_SESSION_FORMAT,
            "seed (integer) is required — sessions are deterministic".to_string(),
        ));
    }
    let carrier = jget(session, "carrier");
    let base = num(&carrier, "baseHz", 0.0);
    if !carrier.is_object() || !(base > 0.0) || base > 20000.0 {
        findings.push(finding(
            TG_SESSION_CARRIER,
            "carrier.baseHz must be in (0, 20000]".to_string(),
        ));
    }
    let cgain = num(&carrier, "gain", -1.0);
    if !(cgain > 0.0) || cgain > 1.0 {
        findings.push(finding(
            TG_SESSION_CARRIER,
            "carrier.gain must be in (0, 1]".to_string(),
        ));
    }
    let bed = jget(session, "bed");
    if bed.is_object() {
        let cutoff = num(&bed, "cutoffHz", 0.0);
        if !(cutoff > 0.0) || cutoff > 20000.0 {
            findings.push(finding(
                TG_SESSION_BED,
                "bed.cutoffHz must be in (0, 20000]".to_string(),
            ));
        }
        let bgain = num(&bed, "gain", -1.0);
        if !(bgain > 0.0) || bgain > 1.0 {
            findings.push(finding(
                TG_SESSION_BED,
                "bed.gain must be in (0, 1]".to_string(),
            ));
        }
    }
    let phases_v = jget(session, "phases");
    let mut min_seconds = 0.0f64;
    match phases_v.as_array().filter(|a| !a.is_empty()) {
        None => findings.push(finding(
            TG_SESSION_PHASE,
            "phases must be a non-empty array".to_string(),
        )),
        Some(phases) => {
            for (i, phase) in phases.iter().enumerate() {
                if jstr(phase, "name").is_empty() {
                    findings.push(finding(
                        TG_SESSION_PHASE,
                        format!("phase {i}: name is required"),
                    ));
                }
                let seconds = num(phase, "seconds", 0.0);
                if !(seconds > 0.0) {
                    findings.push(finding(
                        TG_SESSION_PHASE,
                        format!("phase {i}: seconds must be positive"),
                    ));
                } else if min_seconds == 0.0 || seconds < min_seconds {
                    min_seconds = seconds;
                }
                let beat = num(phase, "beatHz", -1.0);
                if !(0.0..=100.0).contains(&beat) {
                    findings.push(finding(
                        TG_SESSION_PHASE,
                        format!("phase {i}: beatHz must be in [0, 100]"),
                    ));
                }
            }
        }
    }
    let crossfade = num(session, "crossfadeSeconds", 0.0);
    if crossfade < 0.0 || (min_seconds > 0.0 && crossfade >= min_seconds) {
        findings.push(finding(
            TG_SESSION_CROSSFADE,
            "crossfadeSeconds must be >= 0 and shorter than every phase".to_string(),
        ));
    }
    let sub = jget(session, "subliminal");
    if sub.is_object() {
        let freq = num(&sub, "freqHz", 0.0);
        if !(freq > 0.0) || freq > 20000.0 {
            findings.push(finding(
                TG_SESSION_SUBLIMINAL_LEVEL,
                "subliminal.freqHz must be in (0, 20000]".to_string(),
            ));
        }
        let level = num(&sub, "levelDb", 0.0);
        if level >= 0.0 {
            findings.push(finding(
                TG_SESSION_SUBLIMINAL_LEVEL,
                "a subliminal stem must sit BELOW the bed (levelDb < 0); it is specified by its depth, never by an absolute level at or above the bed".to_string(),
            ));
        } else {
            let gain = subliminal_gain(session);
            let ceiling = num(session, "truePeakDb", -1.0);
            let peak_db = if gain > 0.0 {
                20.0 * gain.log10()
            } else {
                -400.0
            };
            if peak_db > ceiling {
                findings.push(finding(
                    TG_SESSION_SUBLIMINAL_PEAK,
                    format!(
                        "subliminal solo peak {peak_db:.2} dB exceeds the session ceiling {ceiling:.2} dB; refused, not clipped into compliance"
                    ),
                ));
            }
        }
    }
    findings
}

pub fn build_session_recipe(session: &Value) -> Result<Value, String> {
    let phases_v = jget(session, "phases");
    let phases: &[Value] = phases_v.as_array().map_or(&[], |a| a);
    let total: f64 = phases.iter().map(|p| num(p, "seconds", 0.0)).sum();
    let carrier = jget(session, "carrier");
    let carrier_gain = num(&carrier, "gain", 0.0);
    let base_hz = num(&carrier, "baseHz", 0.0);
    let mut stems: Vec<Value> = Vec::new();
    let bed = jget(session, "bed");
    if bed.is_object() {
        let mut stem = Map::new();
        stem.insert("type".to_string(), json!("noise"));
        stem.insert("cutoffHz".to_string(), json!(num(&bed, "cutoffHz", 800.0)));
        stem.insert("gain".to_string(), json!(num(&bed, "gain", 0.0)));
        stem.insert("startSeconds".to_string(), json!(0.0));
        stem.insert("durationSeconds".to_string(), json!(total));
        stems.push(Value::Object(stem));
    }
    let n = phases.len();
    let crossfade = num(session, "crossfadeSeconds", 0.0);
    let mut t = 0.0f64;
    for (i, phase) in phases.iter().enumerate() {
        let seconds = num(phase, "seconds", 0.0);
        let last = i + 1 == n;
        let mut stem = Map::new();
        stem.insert("type".to_string(), json!("binaural"));
        stem.insert("baseHz".to_string(), json!(base_hz));
        stem.insert("beatHz".to_string(), json!(num(phase, "beatHz", 0.0)));
        stem.insert("gain".to_string(), json!(carrier_gain));
        stem.insert("startSeconds".to_string(), json!(t));
        stem.insert(
            "durationSeconds".to_string(),
            json!(seconds + if last { 0.0 } else { crossfade }),
        );
        let mut env = Map::new();
        env.insert(
            "attackSeconds".to_string(),
            json!(if i == 0 { 2.0 } else { crossfade }),
        );
        env.insert(
            "releaseSeconds".to_string(),
            json!(if last { 2.0 } else { crossfade }),
        );
        stem.insert("envelope".to_string(), Value::Object(env));
        stems.push(Value::Object(stem));
        t += seconds;
    }
    let sub = jget(session, "subliminal");
    if sub.is_object() {
        let mut stem = Map::new();
        stem.insert("type".to_string(), json!("tone"));
        stem.insert("freqHz".to_string(), json!(num(&sub, "freqHz", 0.0)));
        stem.insert("gain".to_string(), json!(subliminal_gain(session)));
        stem.insert("startSeconds".to_string(), json!(0.0));
        stem.insert("durationSeconds".to_string(), json!(total));
        stems.push(Value::Object(stem));
    }
    let mut doc = Map::new();
    doc.insert("format".to_string(), json!("torogao-audio-recipe/1"));
    doc.insert("name".to_string(), json!(jstr(session, "name")));
    doc.insert("seed".to_string(), jget(session, "seed").clone());
    doc.insert("durationSeconds".to_string(), json!(total));
    doc.insert(
        "targetLufs".to_string(),
        json!(num(session, "targetLufs", -16.0)),
    );
    doc.insert("truePeakDb".to_string(), json!(num(session, "truePeakDb", -1.0)));
    doc.insert("stems".to_string(), Value::Array(stems));
    let doc = Value::Object(doc);
    for f in validate_recipe(&doc) {
        if f.severity == "error" {
            return Err(format!(
                "compiled recipe failed validation ({}): {}",
                f.code, f.message
            ));
        }
    }
    Ok(doc)
}
