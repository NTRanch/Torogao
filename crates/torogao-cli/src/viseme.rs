//! Viseme contract, track baking, authoring and validation,
//! ported from the D `torogao.cli.viseme` module.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use torogao_engine::Puppet;

pub use crate::criteria::Finding;
use crate::curves::{quote, CurveSet};
use crate::paths::find_data_file;
use crate::util::{jget, jstr};

pub const TG_VISEME_PARAM_MISSING: &str = "TG-VISEME-PARAM-MISSING";
pub const TG_VISEME_PARAM_UNBOUND: &str = "TG-VISEME-PARAM-UNBOUND";
pub const TG_VISEME_TRACK_FORMAT: &str = "TG-VISEME-TRACK-FORMAT";
pub const TG_VISEME_TRACK_ORDER: &str = "TG-VISEME-TRACK-ORDER";
pub const TG_VISEME_TRACK_OVERLAP: &str = "TG-VISEME-TRACK-OVERLAP";
pub const TG_VISEME_TRACK_UNKNOWN: &str = "TG-VISEME-TRACK-UNKNOWN";

pub struct VisemeContract {
    pub set_name: String,
    pub parameters: Vec<String>,
    /// viseme -> parameter -> target value
    pub targets: BTreeMap<String, BTreeMap<String, f64>>,
    pub path: String,
}

fn num_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// D `getD`: numeric field or fallback.
fn get_d(v: &Value, key: &str, fallback: f64) -> f64 {
    num_of(&jget(v, key)).unwrap_or(fallback)
}

/// fps fields: integer or float casts count; unsigned integers do not.
fn fps_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => {
            if n.is_i64() {
                n.as_i64()
            } else if n.is_f64() {
                n.as_f64().map(|f| f as i64)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn load_viseme_contract(explicit_path: &str) -> Result<VisemeContract, String> {
    let path = if explicit_path.is_empty() {
        match find_data_file("TOROGAO_CONTRACTS_DIR", "contracts", "visemes.json") {
            Some(p) => p.to_string_lossy().into_owned(),
            None => return Err("viseme contract not found (contracts/visemes.json)".to_string()),
        }
    } else {
        explicit_path.to_string()
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("viseme contract unreadable: {e}"))?;
    let doc: Value = serde_json::from_str(&text)
        .map_err(|e| format!("viseme contract unreadable: {e}"))?;
    if jstr(&doc, "format") != "torogao-visemes/1" {
        return Err("viseme contract format must be \"torogao-visemes/1\"".to_string());
    }
    let mut parameters: Vec<String> = Vec::new();
    if let Some(arr) = jget(&doc, "parameters").as_array() {
        for p in arr {
            if let Some(s) = p.as_str() {
                parameters.push(s.to_string());
            }
        }
    }
    let mut targets: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    if let Some(obj) = jget(&doc, "visemes").as_object() {
        for (viseme, map) in obj {
            let mut inner: BTreeMap<String, f64> = BTreeMap::new();
            if let Some(m) = map.as_object() {
                for (param, value) in m {
                    if let Some(n) = num_of(value) {
                        inner.insert(param.clone(), n);
                    }
                }
            }
            targets.insert(viseme.clone(), inner);
        }
    }
    if parameters.is_empty() || targets.is_empty() {
        return Err("viseme contract is empty".to_string());
    }
    Ok(VisemeContract {
        set_name: jstr(&doc, "set").to_string(),
        parameters,
        targets,
        path,
    })
}

pub fn covered_parameters(puppet: &Puppet, contract: &VisemeContract) -> Vec<String> {
    contract
        .parameters
        .iter()
        .filter(|p| puppet.param_index(p).is_some())
        .cloned()
        .collect()
}

pub fn check_visemes(puppet: &Puppet, contract: &VisemeContract) -> Vec<Finding> {
    let mut findings = Vec::new();
    for name in &contract.parameters {
        match puppet.param_index(name) {
            None => findings.push(Finding::new(
                TG_VISEME_PARAM_MISSING,
                "error",
                name.clone(),
                format!(
                    "contract mouth parameter '{name}' is absent; the puppet cannot answer the viseme contract"
                ),
            )),
            Some(idx) => {
                let param = &puppet.params[idx];
                if param.bindings.is_empty() && !puppet.is_driven(idx) {
                    findings.push(Finding::new(
                        TG_VISEME_PARAM_UNBOUND,
                        "warning",
                        name.clone(),
                        format!(
                            "contract mouth parameter '{name}' has no bindings and no driver; viseme curves would move nothing"
                        ),
                    ));
                }
            }
        }
    }
    findings
}

struct BakeEvent {
    viseme: String,
    start: f64,
}

pub fn bake_visemes(track: &Value, contract: &VisemeContract) -> Result<CurveSet, String> {
    if !track.is_object() || jstr(track, "format") != "torogao-viseme-track/1" {
        return Err("track format must be \"torogao-viseme-track/1\"".to_string());
    }
    let fps: i64 = fps_of(&jget(track, "fps")).unwrap_or(60);
    if ![15, 30, 60, 120].contains(&fps) {
        return Err("track fps must be one of 15, 30, 60, 120".to_string());
    }
    let duration = get_d(track, "durationSeconds", 0.0);
    if !(duration > 0.0) {
        return Err("track durationSeconds (positive) is required".to_string());
    }
    let blend = get_d(track, "blendSeconds", 0.06);
    if blend < 0.0 {
        return Err("blendSeconds must be >= 0".to_string());
    }
    let mut events: Vec<BakeEvent> = Vec::new();
    let events_v = jget(track, "events");
    if !events_v.is_null() {
        let arr = events_v
            .as_array()
            .ok_or_else(|| "track events must be an array".to_string())?;
        for e in arr {
            let viseme = jstr(e, "viseme").to_string();
            if !contract.targets.contains_key(&viseme) {
                return Err(format!("unknown viseme '{viseme}' (not in the contract set)"));
            }
            events.push(BakeEvent {
                viseme,
                start: get_d(e, "start", 0.0),
            });
        }
    }
    events.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    let steps = (duration * fps as f64 + 0.5) as usize;
    let dt = 1.0 / fps as f64;
    let sil = &contract.targets["sil"];
    let mut held: BTreeMap<String, f64> = BTreeMap::new();
    let mut blend_from: BTreeMap<String, f64> = BTreeMap::new();
    for p in &contract.parameters {
        let v = sil.get(p).copied().unwrap_or(0.0);
        held.insert(p.clone(), v);
        blend_from.insert(p.clone(), v);
    }
    let mut blend_start = 0.0f64;
    let mut blending = false;
fn value_at(
    held: &BTreeMap<String, f64>,
    blend_from: &BTreeMap<String, f64>,
    blending: bool,
    blend: f64,
    blend_start: f64,
    p: &str,
    t: f64,
) -> f64 {
    if !blending || blend == 0.0 {
        return held[p];
    }
    let k = (t - blend_start) / blend;
    if k >= 1.0 {
        held[p]
    } else if k <= 0.0 {
        blend_from[p]
    } else {
        blend_from[p] + (held[p] - blend_from[p]) * k
    }
}
    let mut series_values: Vec<Vec<f64>> =
        contract.parameters.iter().map(|_| Vec::with_capacity(steps)).collect();
    let mut next = 0usize;
    for step in 0..steps {
        let t = step as f64 * dt;
        while next < events.len() && events[next].start <= t {
            let ev = &events[next];
            // Snapshot the in-flight value of every parameter first.
            let snapshot: Vec<f64> = contract
                .parameters
                .iter()
                .map(|p| value_at(&held, &blend_from, blending, blend, blend_start, p, ev.start))
                .collect();
            for (i, p) in contract.parameters.iter().enumerate() {
                blend_from.insert(p.clone(), snapshot[i]);
                held.insert(
                    p.clone(),
                    contract.targets[&ev.viseme].get(p).copied().unwrap_or(0.0),
                );
            }
            blend_start = ev.start;
            blending = blend > 0.0;
            next += 1;
        }
        for (i, p) in contract.parameters.iter().enumerate() {
            let v = value_at(&held, &blend_from, blending, blend, blend_start, p, t);
            series_values[i].push(v);
        }
    }
    let mut set = CurveSet::new("viseme-bake", dt);
    for (i, p) in contract.parameters.iter().enumerate() {
        set.series.push(crate::curves::CurveSeries {
            parameter: p.clone(),
            axis: 0,
            values: std::mem::take(&mut series_values[i]),
        });
    }
    set.step_count = steps as i64;
    set.extra
        .push(("contract".to_string(), quote(&contract.set_name)));
    Ok(set)
}

pub fn author_viseme_track(spec: &Value, contract: &VisemeContract) -> Result<Value, String> {
    if !spec.is_object() || jstr(spec, "format") != "torogao-viseme-authoring/1" {
        return Err("spec format must be \"torogao-viseme-authoring/1\"".to_string());
    }
    let events_v = jget(spec, "events");
    let arr = events_v.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
        "spec events must be a non-empty array".to_string()
    })?;
    let mut events: Vec<(String, f64, f64)> = Vec::new();
    for e in arr {
        let name = jstr(e, "viseme").to_string();
        if !contract.targets.contains_key(&name) {
            return Err(format!("unknown viseme '{name}' (not in the contract set)"));
        }
        let start = get_d(e, "start", -1.0);
        if start < 0.0 {
            return Err(format!("event '{name}' needs a start >= 0"));
        }
        let end = get_d(e, "end", -1.0);
        if end <= start {
            return Err(format!("event '{name}' needs an end after its start"));
        }
        events.push((name, start, end));
    }
    events.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    for i in 1..events.len() {
        if events[i].1 < events[i - 1].2 {
            return Err(format!(
                "events '{}' and '{}' overlap",
                events[i - 1].0, events[i].0
            ));
        }
    }
    let last_end = events.last().map_or(0.0, |e| e.2);
    let mut duration = get_d(spec, "durationSeconds", 0.0);
    if duration <= 0.0 {
        duration = last_end;
    }
    if duration < last_end {
        return Err("durationSeconds ends before the last event".to_string());
    }
    let fps: i64 = fps_of(&jget(spec, "fps")).unwrap_or(60);
    if ![15, 30, 60, 120].contains(&fps) {
        return Err("fps must be one of 15, 30, 60, 120".to_string());
    }
    let mut out_events: Vec<Value> = Vec::new();
    for (name, start, end) in &events {
        let mut m = Map::new();
        m.insert("viseme".to_string(), json!(name));
        m.insert("start".to_string(), json!(start));
        m.insert("end".to_string(), json!(end));
        out_events.push(Value::Object(m));
    }
    let mut doc = Map::new();
    doc.insert("format".to_string(), json!("torogao-viseme-track/1"));
    doc.insert("fps".to_string(), json!(fps));
    doc.insert("durationSeconds".to_string(), json!(duration));
    doc.insert(
        "blendSeconds".to_string(),
        json!(get_d(spec, "blendSeconds", 0.06)),
    );
    doc.insert("events".to_string(), Value::Array(out_events));
    Ok(Value::Object(doc))
}

pub fn validate_viseme_track(track: &Value, contract: &VisemeContract) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !track.is_object() || jstr(track, "format") != "torogao-viseme-track/1" {
        findings.push(Finding::new(
            TG_VISEME_TRACK_FORMAT,
            "error",
            "track".to_string(),
            "format must be \"torogao-viseme-track/1\"".to_string(),
        ));
        return findings;
    }
    if !(get_d(track, "durationSeconds", 0.0) > 0.0) {
        findings.push(Finding::new(
            TG_VISEME_TRACK_FORMAT,
            "error",
            "track".to_string(),
            "durationSeconds (positive) is required".to_string(),
        ));
    }
    let events_v = jget(track, "events");
    let Some(arr) = events_v.as_array() else {
        findings.push(Finding::new(
            TG_VISEME_TRACK_FORMAT,
            "error",
            "track".to_string(),
            "events must be an array".to_string(),
        ));
        return findings;
    };
    let mut prev_start = -1.0f64;
    let mut prev_end = -1.0f64;
    let mut prev_name = String::new();
    for e in arr {
        let name = jstr(e, "viseme").to_string();
        if !contract.targets.contains_key(&name) {
            findings.push(Finding::new(
                TG_VISEME_TRACK_UNKNOWN,
                "error",
                name.clone(),
                "viseme is not in the contract set".to_string(),
            ));
        }
        let start = get_d(e, "start", 0.0);
        let end = get_d(e, "end", start);
        if prev_start >= 0.0 && start < prev_start {
            findings.push(Finding::new(
                TG_VISEME_TRACK_ORDER,
                "error",
                name.clone(),
                "events are not in start order".to_string(),
            ));
        }
        if prev_end >= 0.0 && start < prev_end {
            findings.push(Finding::new(
                TG_VISEME_TRACK_OVERLAP,
                "error",
                name.clone(),
                format!("event overlaps '{prev_name}'"),
            ));
        }
        prev_start = start;
        prev_end = end;
        prev_name = name;
    }
    findings
}
