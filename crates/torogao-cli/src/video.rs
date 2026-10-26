//! Video timeline validation and edit-decision artifacts,
//! ported from the D `torogao.cli.video` module.

use serde_json::{Value, json};

use crate::criteria::Finding;
use crate::effects::validate_effects;
use crate::fluids::round6;
use crate::packs::{pack_num, pack_str};
use crate::util::{fmt_g, jget};

pub const TIMELINE_FORMAT: &str = "torogao-timeline/1";
pub const TG_VIDEO_FORMAT: &str = "TG-VIDEO-FORMAT";
pub const TG_VIDEO_OVERLAP: &str = "TG-VIDEO-OVERLAP";
pub const TG_VIDEO_RANGE: &str = "TG-VIDEO-RANGE";
pub const TG_VIDEO_REF: &str = "TG-VIDEO-REF";
pub const TG_VIDEO_TYPE: &str = "TG-VIDEO-TYPE";

fn num_str(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

pub fn beat_seconds(timeline: &Value) -> f64 {
    let bpm = pack_num(jget(timeline, "tempo"), "bpm", 0.0);
    if bpm > 0.0 { 60.0 / bpm } else { 0.0 }
}

pub fn validate_timeline(timeline: &Value, base_dir: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !timeline.is_object() {
        findings.push(Finding::new(
            TG_VIDEO_TYPE,
            "error",
            "timeline",
            "timeline document is not a JSON object",
        ));
        return findings;
    }
    if pack_str(timeline, "format") != TIMELINE_FORMAT {
        findings.push(Finding::new(
            TG_VIDEO_FORMAT,
            "error",
            "timeline.format",
            format!("format is not '{TIMELINE_FORMAT}'"),
        ));
        return findings;
    }
    let tracks = jget(timeline, "tracks");
    let Some(tracks) = tracks.as_array() else {
        findings.push(Finding::new(
            TG_VIDEO_TYPE,
            "error",
            "timeline.tracks",
            "timeline requires a tracks array",
        ));
        return findings;
    };
    for (ti, track) in tracks.iter().enumerate() {
        let tat = format!("tracks[{ti}]");
        let kind = pack_str(track, "kind");
        if kind != "video" && kind != "audio" && kind != "overlay" {
            findings.push(Finding::new(
                TG_VIDEO_TYPE,
                "error",
                format!("{tat}.kind"),
                format!("track kind '{kind}' is not video, audio or overlay"),
            ));
        }
        let clips = jget(track, "clips");
        let Some(clips) = clips.as_array() else {
            continue;
        };
        let mut last_end = -1.0f64;
        let mut last_id = String::new();
        for (ci, clip) in clips.iter().enumerate() {
            let at = format!("{tat}.clips[{ci}]");
            let id = pack_str(clip, "id");
            if pack_str(clip, "source").is_empty() {
                findings.push(Finding::new(
                    TG_VIDEO_TYPE,
                    "error",
                    format!("{at}.source"),
                    "clip names no source",
                ));
            }
            let in_at = pack_num(clip, "in", 0.0);
            let out_at = pack_num(clip, "out", 0.0);
            let start = pack_num(clip, "start", 0.0);
            if out_at <= in_at {
                findings.push(Finding::new(
                    TG_VIDEO_RANGE,
                    "error",
                    at.clone(),
                    format!(
                        "clip '{id}' has out <= in ({} <= {})",
                        num_str(out_at),
                        num_str(in_at)
                    ),
                ));
            }
            let end = start + (out_at - in_at);
            if start < last_end {
                findings.push(Finding::new(
                    TG_VIDEO_OVERLAP,
                    "error",
                    at.clone(),
                    format!(
                        "clip '{id}' starts at {} before clip '{last_id}' ends at {} on the same track",
                        num_str(start),
                        num_str(last_end)
                    ),
                ));
            }
            if end > last_end {
                last_end = end;
                last_id = id.clone();
            }
            let take = pack_str(clip, "motionTake");
            if !take.is_empty()
                && !base_dir.is_empty()
                && !std::path::Path::new(&format!("{base_dir}/{take}")).exists()
            {
                findings.push(Finding::new(
                    TG_VIDEO_REF,
                    "error",
                    format!("{at}.motionTake"),
                    format!("clip '{id}' motionTake '{take}' does not resolve to a file"),
                ));
            }
        }
    }
    let overlays = jget(timeline, "overlays");
    if overlays.is_object() {
        for f in validate_effects(overlays, None) {
            findings.push(Finding::new(
                f.code,
                f.severity,
                format!("overlays.{}", f.subject),
                f.message.clone(),
            ));
        }
    }
    findings
}

pub fn tempo_report(timeline: &Value) -> Value {
    let beat = beat_seconds(timeline);
    let mut rows: Vec<Value> = Vec::new();
    let mut on_beat: i64 = 0;
    let mut total: i64 = 0;
    if let Some(tracks) = jget(timeline, "tracks").as_array() {
        for track in tracks {
            let clips = jget(track, "clips");
            let Some(clips) = clips.as_array() else {
                continue;
            };
            for clip in clips {
                let start = pack_num(clip, "start", 0.0);
                let dur = pack_num(clip, "out", 0.0) - pack_num(clip, "in", 0.0);
                for edge in [start, start + dur] {
                    total += 1;
                    let mut row = json!({
                        "clip": pack_str(clip, "id"),
                        "edge": round6(edge),
                    });
                    if beat > 0.0 {
                        let nearest = (edge / beat).round() * beat;
                        let miss = (edge - nearest).abs();
                        row["missSeconds"] = round6(miss);
                        row["nearestBeat"] = round6(nearest);
                        row["onBeat"] = json!(miss <= 0.03);
                        if miss <= 0.03 {
                            on_beat += 1;
                        }
                    }
                    rows.push(row);
                }
            }
        }
    }
    json!({
        "beatSeconds": round6(beat),
        "boundaries": rows,
        "bpm": pack_num(jget(timeline, "tempo"), "bpm", 0.0),
        "onBeat": on_beat,
        "total": total,
    })
}

pub fn timeline_duration(timeline: &Value) -> f64 {
    let mut best = 0.0f64;
    if let Some(tracks) = jget(timeline, "tracks").as_array() {
        for track in tracks {
            if let Some(clips) = jget(track, "clips").as_array() {
                for clip in clips {
                    let end = pack_num(clip, "start", 0.0)
                        + (pack_num(clip, "out", 0.0) - pack_num(clip, "in", 0.0));
                    if end > best {
                        best = end;
                    }
                }
            }
        }
    }
    best
}

pub fn build_timeline(clip_list: &Value) -> Value {
    let mut clips: Vec<Value> = Vec::new();
    let mut at = 0.0f64;
    if let Some(list) = jget(clip_list, "clips").as_array() {
        for entry in list {
            let in_at = pack_num(entry, "in", 0.0);
            let out_at = pack_num(entry, "out", 0.0);
            let mut clip = json!({
                "id": pack_str(entry, "id"),
                "in": in_at,
                "out": out_at,
                "source": pack_str(entry, "source"),
                "start": at,
            });
            let take = pack_str(entry, "motionTake");
            if !take.is_empty() {
                clip["motionTake"] = json!(take);
            }
            clips.push(clip);
            at += out_at - in_at;
        }
    }
    let mut doc = json!({
        "duration": round6(at),
        "format": TIMELINE_FORMAT,
        "tracks": [{"clips": clips, "kind": "video"}],
    });
    let tempo = jget(clip_list, "tempo");
    if tempo.is_object() {
        doc["tempo"] = tempo.clone();
    }
    doc
}

pub fn concat_list(timeline: &Value) -> String {
    let mut out = String::new();
    if let Some(tracks) = jget(timeline, "tracks").as_array() {
        for track in tracks {
            if pack_str(track, "kind") != "video" {
                continue;
            }
            if let Some(clips) = jget(track, "clips").as_array() {
                for clip in clips {
                    out.push_str(&format!(
                        "file '{}'\ninpoint {}\noutpoint {}\n",
                        pack_str(clip, "source"),
                        fmt_g(pack_num(clip, "in", 0.0)),
                        fmt_g(pack_num(clip, "out", 0.0))
                    ));
                }
            }
        }
    }
    out
}

pub fn chapters_file(timeline: &Value) -> String {
    let mut out = String::from(";FFMETADATA1\n");
    if let Some(tracks) = jget(timeline, "tracks").as_array() {
        for track in tracks {
            if pack_str(track, "kind") != "video" {
                continue;
            }
            if let Some(clips) = jget(track, "clips").as_array() {
                for clip in clips {
                    let start = pack_num(clip, "start", 0.0);
                    let end = start + (pack_num(clip, "out", 0.0) - pack_num(clip, "in", 0.0));
                    out.push_str(&format!(
                        "[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={}\n",
                        (start * 1000.0) as i64,
                        (end * 1000.0) as i64,
                        pack_str(clip, "id")
                    ));
                }
            }
            break;
        }
    }
    out
}
