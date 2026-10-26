//! Live-tracking take validation, baking and gaze
//! generation, ported from the D `torogao.cli.tracking`.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::criteria::Finding;
use crate::curves::{CurveSeries, CurveSet};
use crate::fluids::{round4, round6};
use crate::packs::{pack_num, pack_str};
use crate::util::jget;

pub const TAKE_FORMAT: &str = "torogao-tracking-take/1";
pub const TG_TRACK_FORMAT: &str = "TG-TRACK-FORMAT";
pub const TG_TRACK_RANGE: &str = "TG-TRACK-RANGE";
pub const TG_TRACK_TYPE: &str = "TG-TRACK-TYPE";
pub const TG_TRACK_UNKNOWN_ID: &str = "TG-TRACK-UNKNOWN-ID";

#[derive(Debug, Clone, Copy)]
pub struct TrackRange {
    pub found: bool,
    pub max: f64,
    pub min: f64,
}

pub fn find_tracking_set(pack: &Value, set_id: &str) -> Value {
    if let Some(sets) = jget(pack, "trackingSets").as_array() {
        for entry in sets {
            if pack_str(entry, "id") == set_id {
                return entry.clone();
            }
        }
    }
    Value::Null
}

pub fn tracking_ranges(set: &Value) -> BTreeMap<String, TrackRange> {
    let mut out = BTreeMap::new();
    if let Some(ids) = jget(set, "ids").as_array() {
        for entry in ids {
            let id = pack_str(entry, "id");
            if id.is_empty() {
                continue;
            }
            out.insert(
                id,
                TrackRange {
                    found: true,
                    max: pack_num(entry, "max", 1.0),
                    min: pack_num(entry, "min", 0.0),
                },
            );
        }
    }
    out
}

/// The D `to!string(double)` rendering for range messages:
/// integral values print without a decimal point.
fn num_str(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

pub fn validate_take(take: &Value, pack: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !take.is_object() {
        findings.push(Finding::new(
            TG_TRACK_TYPE,
            "error",
            "take",
            "take document is not a JSON object",
        ));
        return findings;
    }
    if pack_str(take, "format") != TAKE_FORMAT {
        findings.push(Finding::new(
            TG_TRACK_FORMAT,
            "error",
            "take.format",
            format!("format is not '{TAKE_FORMAT}'"),
        ));
        return findings;
    }
    let set_id = pack_str(take, "set");
    let set = find_tracking_set(pack, &set_id);
    if !set.is_object() {
        findings.push(Finding::new(
            TG_TRACK_UNKNOWN_ID,
            "error",
            "take.set",
            format!("take names tracking set '{set_id}', which the pack does not declare"),
        ));
        return findings;
    }
    let frames = jget(take, "frames");
    let Some(frames) = frames.as_array().filter(|a| !a.is_empty()) else {
        findings.push(Finding::new(
            TG_TRACK_TYPE,
            "error",
            "take.frames",
            "take carries no frames",
        ));
        return findings;
    };
    let ranges = tracking_ranges(&set);
    let mut last_time = f64::NEG_INFINITY;
    for (i, frame) in frames.iter().enumerate() {
        let at = format!("frames[{i}]");
        let t = pack_num(frame, "time", -1.0);
        if t < 0.0 {
            findings.push(Finding::new(
                TG_TRACK_TYPE,
                "error",
                format!("{at}.time"),
                "frame time missing or negative",
            ));
            continue;
        }
        if t < last_time {
            findings.push(Finding::new(
                TG_TRACK_TYPE,
                "error",
                format!("{at}.time"),
                "frame times must be non-decreasing",
            ));
        }
        last_time = t;
        let values = jget(frame, "values");
        let Some(values) = values.as_object() else {
            continue;
        };
        for (id, v) in values {
            let range = ranges.get(id);
            match range {
                None => {
                    findings.push(Finding::new(
                        TG_TRACK_UNKNOWN_ID,
                        "error",
                        format!("{at}.values.{id}"),
                        format!("tracking ID '{id}' is not in set '{set_id}'"),
                    ));
                    continue;
                }
                Some(range) => {
                    let Some(value) = v.as_f64() else {
                        findings.push(Finding::new(
                            TG_TRACK_TYPE,
                            "error",
                            format!("{at}.values.{id}"),
                            "tracking value is not a number",
                        ));
                        continue;
                    };
                    if value < range.min || value > range.max {
                        findings.push(Finding::new(
                            TG_TRACK_RANGE,
                            "error",
                            format!("{at}.values.{id}"),
                            format!(
                                "value {} is outside the declared range [{}, {}] for '{id}'",
                                num_str(value),
                                num_str(range.min),
                                num_str(range.max)
                            ),
                        ));
                    }
                }
            }
        }
    }
    findings
}

fn value_of(frame: &Value, id: &str) -> Option<f64> {
    jget(&jget(frame, "values"), id).as_f64()
}

fn sample_at(frames: &[Value], id: &str, t: f64) -> f64 {
    let mut have0 = false;
    let mut have1 = false;
    let mut t0 = 0.0;
    let mut v0 = 0.0;
    let mut t1 = 0.0;
    let mut v1 = 0.0;
    for frame in frames {
        let Some(v) = value_of(frame, id) else { continue };
        let ft = pack_num(frame, "time", 0.0);
        if ft <= t {
            t0 = ft;
            v0 = v;
            have0 = true;
        }
        if ft >= t && !have1 {
            t1 = ft;
            v1 = v;
            have1 = true;
        }
    }
    if have0 && have1 {
        if t1 <= t0 {
            return v0;
        }
        return v0 + (v1 - v0) * ((t - t0) / (t1 - t0));
    }
    if have0 {
        return v0;
    }
    if have1 {
        return v1;
    }
    0.0
}

pub fn bake_take(take: &Value, fps: i32, steps: i32) -> CurveSet {
    let dt = if fps > 0 { 1.0 / f64::from(fps) } else { 0.0 };
    let mut out = CurveSet::new("tracking-bake", dt);
    let empty: Vec<Value> = Vec::new();
    let frames = jget(take, "frames").as_array().unwrap_or(&empty);
    let mut ids: Vec<String> = Vec::new();
    for frame in frames {
        if let Some(values) = jget(frame, "values").as_object() {
            for id in values.keys() {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
    }
    ids.sort();
    for id in &ids {
        let mut values: Vec<f64> = Vec::new();
        for step in 0..steps {
            let t = if fps > 0 {
                f64::from(step) / f64::from(fps)
            } else {
                0.0
            };
            values.push(f64::from(sample_at(frames, id, t) as f32));
        }
        out.series.push(CurveSeries {
            axis: 0,
            parameter: id.clone(),
            values,
        });
    }
    out
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn sample_path(target_path: &Value, t: f64) -> (f64, f64) {
    let points = jget(target_path, "targets");
    let Some(points) = points.as_array().filter(|a| !a.is_empty()) else {
        return (0.0, 0.0);
    };
    let mut have_prev = false;
    let mut pt = 0.0;
    let mut px = 0.0;
    let mut py = 0.0;
    for point in points {
        let ct = pack_num(point, "time", 0.0);
        let cx = pack_num(point, "x", 0.0);
        let cy = pack_num(point, "y", 0.0);
        if ct >= t {
            if !have_prev {
                return (cx, cy);
            }
            let a = if ct > pt { (t - pt) / (ct - pt) } else { 0.0 };
            return (px + (cx - px) * a, py + (cy - py) * a);
        }
        pt = ct;
        px = cx;
        py = cy;
        have_prev = true;
    }
    let last = points.last().unwrap_or(&Value::Null);
    (pack_num(last, "x", 0.0), pack_num(last, "y", 0.0))
}

pub fn generate_gaze_take(target_path: &Value, set_id: &str, seed: u64, seconds: f64) -> Value {
    let mut state = seed;
    let mut sac_x = 0.0f64;
    let mut sac_y = 0.0f64;
    let mut next_saccade: i64 = 0;
    let steps = (seconds * 60.0 + 0.5) as i64;
    let mut frames: Vec<Value> = Vec::new();
    for step in 0..=steps {
        let t = step as f64 / 60.0;
        if step >= next_saccade {
            let u = splitmix64(&mut state);
            sac_x = ((u & 0xFFFF) as f64 / 65535.0 - 0.5) * 0.8;
            let w = splitmix64(&mut state);
            sac_y = ((w & 0xFFFF) as f64 / 65535.0 - 0.5) * 0.8;
            let gap = splitmix64(&mut state);
            next_saccade = step + 7 + ((gap % 11) + 7) as i64;
        }
        let (gx, gy) = sample_path(target_path, t);
        frames.push(json!({
            "time": round6(t),
            "values": {
                "ParamAngleX": round4((gx + sac_x).clamp(-30.0, 30.0)),
                "ParamAngleY": round4((gy + sac_y).clamp(-30.0, 30.0)),
            },
        }));
    }
    json!({
        "format": TAKE_FORMAT,
        "fps": 60,
        "frames": frames,
        "set": set_id,
        "source": "gaze-generator",
    })
}
