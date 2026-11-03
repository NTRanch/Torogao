//! Session validation and session-to-recipe compilation,
//! ported from the D `torogao.cli.sessionbuild` module.

use serde_json::{Map, Value, json};

use crate::audioproc::validate_recipe;
use crate::criteria::Finding;
use crate::util::{jget, jstr};

pub const SESSION_FORMAT: &str = "torogao-session/1";
/// The v2 session format (TOROGAO-HYPNO-CAPABILITY §d): a
/// superset of v1 — typed blocks, affirmation-set records,
/// triggers as data, voice placements, visual cues, and
/// encoded subliminal stems. V1 documents keep compiling
/// exactly as before; untyped v1 phases read as `plateau`.
pub const SESSION_FORMAT_V2: &str = "torogao-session/2";
pub const TG_SESSION_CARRIER: &str = "TG-SESSION-CARRIER";
pub const TG_SESSION_BED: &str = "TG-SESSION-BED";
pub const TG_SESSION_CROSSFADE: &str = "TG-SESSION-CROSSFADE";
pub const TG_SESSION_FORMAT: &str = "TG-SESSION-FORMAT";
pub const TG_SESSION_PHASE: &str = "TG-SESSION-PHASE";
pub const TG_SESSION_SUBLIMINAL_LEVEL: &str = "TG-SESSION-SUBLIMINAL-LEVEL";
pub const TG_SESSION_SUBLIMINAL_PEAK: &str = "TG-SESSION-SUBLIMINAL-PEAK";
pub const TG_SESSION_AFFIRMATION: &str = "TG-SESSION-AFFIRMATION";
pub const TG_SESSION_CUE: &str = "TG-SESSION-CUE";
pub const TG_SESSION_FRAME: &str = "TG-SESSION-FRAME";
pub const TG_SESSION_KIND: &str = "TG-SESSION-KIND";
pub const TG_SESSION_PROVENANCE: &str = "TG-SESSION-PROVENANCE";
pub const TG_SESSION_TRIGGER: &str = "TG-SESSION-TRIGGER";

/// The block kinds a v2 phase may declare (TECHNIQUE-SPEC
/// T6's session architecture, made data).
pub const BLOCK_KINDS: [&str; 6] = [
    "induction",
    "deepener",
    "suggestion",
    "anchor",
    "emergence",
    "plateau",
];

/// The provenance an affirmation set must assert: sets are
/// estate-original text authored for the game. Corpus
/// material is method evidence only and never enters a
/// project record, so a provenance naming anything else —
/// a studied creator above all — is refused.
pub const ESTATE_ORIGINAL_PREFIX: &str = "estate-original";

/// Derives a cue's frame from its sample position at the
/// declared frame rate (TOROGAO-HYPNO-CAPABILITY §c's
/// synchrony law): audio and visual cues share the session's
/// 48 kHz sample clock, and the derivation is recorded in
/// the compiled recipe so drift is a build error.
pub fn cue_frame(at_sample: u64, frame_rate: u32) -> u64 {
    at_sample * u64::from(frame_rate) / 48_000
}

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
        num(bed, "gain", 0.0)
    } else {
        num(jget(session, "carrier"), "gain", 0.0)
    }
}

pub fn subliminal_gain(session: &Value) -> f64 {
    let sub = jget(session, "subliminal");
    if !sub.is_object() {
        return 0.0;
    }
    session_reference_gain(session) * 10f64.powf(num(sub, "levelDb", 0.0) / 20.0)
}

pub fn validate_session(session: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    let format = jstr(session, "format");
    if !session.is_object() || (format != SESSION_FORMAT && format != SESSION_FORMAT_V2) {
        findings.push(finding(
            TG_SESSION_FORMAT,
            "format must be \"torogao-session/1\" or \"torogao-session/2\"".to_string(),
        ));
        return findings;
    }
    if jstr(session, "name").is_empty() {
        findings.push(finding(TG_SESSION_FORMAT, "name is required".to_string()));
    }
    if !is_int_like(jget(session, "seed")) {
        findings.push(finding(
            TG_SESSION_FORMAT,
            "seed (integer) is required — sessions are deterministic".to_string(),
        ));
    }
    let carrier = jget(session, "carrier");
    let base = num(carrier, "baseHz", 0.0);
    if !carrier.is_object() || base <= 0.0 || base > 20000.0 {
        findings.push(finding(
            TG_SESSION_CARRIER,
            "carrier.baseHz must be in (0, 20000]".to_string(),
        ));
    }
    let cgain = num(carrier, "gain", -1.0);
    if cgain <= 0.0 || cgain > 1.0 {
        findings.push(finding(
            TG_SESSION_CARRIER,
            "carrier.gain must be in (0, 1]".to_string(),
        ));
    }
    let bed = jget(session, "bed");
    if bed.is_object() {
        let cutoff = num(bed, "cutoffHz", 0.0);
        if cutoff <= 0.0 || cutoff > 20000.0 {
            findings.push(finding(
                TG_SESSION_BED,
                "bed.cutoffHz must be in (0, 20000]".to_string(),
            ));
        }
        let bgain = num(bed, "gain", -1.0);
        if bgain <= 0.0 || bgain > 1.0 {
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
                if seconds <= 0.0 {
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
        let freq = num(sub, "freqHz", 0.0);
        if freq <= 0.0 || freq > 20000.0 {
            findings.push(finding(
                TG_SESSION_SUBLIMINAL_LEVEL,
                "subliminal.freqHz must be in (0, 20000]".to_string(),
            ));
        }
        let level = num(sub, "levelDb", 0.0);
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
    validate_v2(session, format == SESSION_FORMAT_V2, &mut findings);
    findings
}

/// Whether any v2-only key appears anywhere in the document.
fn carries_v2_keys(session: &Value) -> bool {
    if session.get("affirmationSets").is_some()
        || session.get("frame").is_some()
        || session.get("stems").is_some()
    {
        return true;
    }
    match jget(session, "phases").as_array() {
        Some(phases) => phases.iter().any(|phase| {
            [
                "kind",
                "voiceLines",
                "affirmationSet",
                "visualCues",
                "countFrom",
                "trigger",
            ]
            .iter()
            .any(|key| phase.get(key).is_some())
                || (phase.get("subliminal").is_some_and(|s| s.get("affirmationSet").is_some()))
        }),
        None => false,
    }
}

/// The named pass-through stems a v2 document declares.
fn declared_stem_names(session: &Value) -> Vec<String> {
    match jget(session, "stems").as_array() {
        Some(stems) => stems.iter().map(|stem| jstr(stem, "name").to_string()).collect(),
        None => Vec::new(),
    }
}

fn validate_v2(session: &Value, is_v2: bool, findings: &mut Vec<Finding>) {
    if !is_v2 {
        if carries_v2_keys(session) {
            findings.push(finding(
                TG_SESSION_FORMAT,
                "document carries torogao-session/2 fields (typed blocks, affirmation sets, encoded subliminal) under a torogao-session/1 format tag".to_string(),
            ));
        }
        return;
    }
    // Affirmation sets (T4's grammar made data).
    let mut set_ids: Vec<String> = Vec::new();
    if let Some(sets) = jget(session, "affirmationSets").as_array() {
        for (i, set) in sets.iter().enumerate() {
            let id = jstr(set, "id");
            if id.is_empty() || set_ids.iter().any(|s| s == id) {
                findings.push(finding(
                    TG_SESSION_AFFIRMATION,
                    format!("affirmationSets {i}: id is required and must be unique"),
                ));
            } else {
                set_ids.push(id.to_string());
            }
            let provenance = jstr(set, "provenance");
            if !provenance.starts_with(ESTATE_ORIGINAL_PREFIX) {
                findings.push(finding(
                    TG_SESSION_PROVENANCE,
                    format!(
                        "affirmation set '{id}': provenance must assert estate-original authorship; corpus material is method evidence only and never enters a project record"
                    ),
                ));
            }
            match jget(set, "statements").as_array().filter(|a| !a.is_empty()) {
                None => findings.push(finding(
                    TG_SESSION_AFFIRMATION,
                    format!("affirmation set '{id}': statements must be a non-empty array"),
                )),
                Some(statements) => {
                    for (j, statement) in statements.iter().enumerate() {
                        let text = jstr(statement, "text");
                        if text.is_empty() || text.len() > 512 {
                            findings.push(finding(
                                TG_SESSION_AFFIRMATION,
                                format!("affirmation set '{id}' statement {j}: text is required (<= 512 bytes)"),
                            ));
                        }
                        if !["first", "second"].contains(&jstr(statement, "person")) {
                            findings.push(finding(
                                TG_SESSION_AFFIRMATION,
                                format!("affirmation set '{id}' statement {j}: person must be \"first\" or \"second\""),
                            ));
                        }
                        if jstr(statement, "tense") != "present" {
                            findings.push(finding(
                                TG_SESSION_AFFIRMATION,
                                format!("affirmation set '{id}' statement {j}: tense must be \"present\" (the hidden-layer grammar is present-tense)"),
                            ));
                        }
                        if jstr(statement, "theme").is_empty() {
                            findings.push(finding(
                                TG_SESSION_AFFIRMATION,
                                format!("affirmation set '{id}' statement {j}: theme is required"),
                            ));
                        }
                    }
                    if let Some(head) = jget(set, "cueHeadIndex").as_i64() {
                        if head < 0 || head as usize >= statements.len() {
                            findings.push(finding(
                                TG_SESSION_AFFIRMATION,
                                format!("affirmation set '{id}': cueHeadIndex {head} outside the statement list"),
                            ));
                        }
                    }
                }
            }
        }
    }
    // Pass-through stems must carry names (a subliminal
    // source is addressed by name).
    for (i, stem) in declared_stem_names(session).iter().enumerate() {
        if stem.is_empty() {
            findings.push(finding(
                TG_SESSION_FORMAT,
                format!("stems {i}: a pass-through stem needs a name"),
            ));
        }
    }
    let stem_names = declared_stem_names(session);
    // Phases: typed blocks and their content references.
    if let Some(phases) = jget(session, "phases").as_array() {
        for (i, phase) in phases.iter().enumerate() {
            let name = jstr(phase, "name");
            let kind = {
                let raw = jstr(phase, "kind");
                if raw.is_empty() { "plateau" } else { raw }
            };
            if !BLOCK_KINDS.contains(&kind) {
                findings.push(finding(
                    TG_SESSION_KIND,
                    format!("phase {i} ('{name}'): kind '{kind}' is not one of {}", BLOCK_KINDS.join(", ")),
                ));
            }
            if kind == "deepener" {
                let from = jget(phase, "countFrom").as_i64();
                let to = jget(phase, "countTo").as_i64();
                match (from, to) {
                    (Some(a), Some(b)) if a != b && (1..=10_000).contains(&a) && (1..=10_000).contains(&b) => {}
                    _ => findings.push(finding(
                        TG_SESSION_KIND,
                        format!("phase {i} ('{name}'): a deepener declares its count range (countFrom/countTo, distinct, in [1, 10000])"),
                    )),
                }
            }
            if kind == "anchor" {
                let trigger = jget(phase, "trigger");
                let ok = trigger.is_object()
                    && !jstr(trigger, "cue").is_empty()
                    && !jstr(trigger, "response").is_empty()
                    && ["session", "persistent"].contains(&jstr(trigger, "scope"));
                if !ok {
                    findings.push(finding(
                        TG_SESSION_TRIGGER,
                        format!("phase {i} ('{name}'): an anchor block declares its trigger (cue, response, scope session|persistent)"),
                    ));
                }
            }
            if let Some(lines) = jget(phase, "voiceLines").as_array() {
                for (j, line) in lines.iter().enumerate() {
                    if jstr(line, "identity").is_empty() || jstr(line, "record").is_empty() {
                        findings.push(finding(
                            TG_SESSION_PHASE,
                            format!("phase {i} ('{name}') voiceLines {j}: a line is a torori delivery reference (identity, record)"),
                        ));
                    }
                }
            }
            if phase.get("affirmationSet").is_some() {
                let set_ref = jstr(phase, "affirmationSet");
                if !set_ids.iter().any(|s| s == set_ref) {
                    findings.push(finding(
                        TG_SESSION_AFFIRMATION,
                        format!("phase {i} ('{name}'): affirmationSet '{set_ref}' is not declared in this document"),
                    ));
                }
            }
            if let Some(sub) = phase.get("subliminal").filter(|s| s.is_object()) {
                let set_ref = jstr(sub, "affirmationSet");
                if set_ref.is_empty() || !set_ids.iter().any(|s| s == set_ref) {
                    findings.push(finding(
                        TG_SESSION_AFFIRMATION,
                        format!("phase {i} ('{name}'): subliminal.affirmationSet '{set_ref}' is not declared in this document"),
                    ));
                }
                let source = jstr(sub, "sourceStem");
                if source.is_empty() || !stem_names.iter().any(|s| s == source) {
                    findings.push(finding(
                        TG_SESSION_AFFIRMATION,
                        format!("phase {i} ('{name}'): subliminal.sourceStem '{source}' names no declared stem"),
                    ));
                }
                let level = num(sub, "levelDb", 0.0);
                if level >= 0.0 {
                    findings.push(finding(
                        TG_SESSION_SUBLIMINAL_LEVEL,
                        format!("phase {i} ('{name}'): an encoded subliminal is specified by depth below the bed (levelDb < 0)"),
                    ));
                }
                let carrier = num(sub, "carrierHz", 16_000.0);
                if !(crate::subliminal::CARRIER_MIN_HZ..=crate::subliminal::CARRIER_MAX_HZ)
                    .contains(&carrier)
                {
                    findings.push(finding(
                        TG_SESSION_SUBLIMINAL_LEVEL,
                        format!("phase {i} ('{name}'): subliminal.carrierHz {carrier} outside the encoding window"),
                    ));
                }
            }
            if let Some(cues) = jget(phase, "visualCues").as_array() {
                for (j, cue) in cues.iter().enumerate() {
                    let at = jget(cue, "atSample");
                    let text_ref = jstr(cue, "textRef");
                    let duration = jget(cue, "durationFrames").as_i64();
                    let fade = jget(cue, "fadeFrames").as_i64();
                    let rate = jget(cue, "frameRate").as_i64();
                    let ok = is_int_like(at)
                        && at.as_i64().is_some_and(|v| v >= 0)
                        && set_ids.iter().any(|s| s == text_ref)
                        && duration.is_some_and(|d| d >= 1)
                        && fade.is_some_and(|f| f >= 0 && Some(f) <= duration)
                        && rate.is_some_and(|r| (1..=240).contains(&r));
                    if !ok {
                        findings.push(finding(
                            TG_SESSION_CUE,
                            format!("phase {i} ('{name}') visualCues {j}: a cue is (atSample >= 0, textRef naming an affirmation set — text by reference, never a literal, durationFrames >= 1, fadeFrames <= durationFrames, frameRate in [1, 240])"),
                        ));
                    }
                }
            }
        }
    }
    // The accumulation frame (T6's everyday variant).
    if let Some(frame) = session.get("frame") {
        let mut ok = jstr(frame, "type") == "everyday";
        if let Some(rows) = jget(frame, "escalation").as_array() {
            let mut last = 0i64;
            for row in rows {
                match row.get("listens").and_then(Value::as_i64) {
                    Some(listens) if listens > last => last = listens,
                    _ => ok = false,
                }
                if jstr(row, "note").is_empty() {
                    ok = false;
                }
            }
        } else {
            ok = false;
        }
        if !ok {
            findings.push(finding(
                TG_SESSION_FRAME,
                "frame must be (type \"everyday\", escalation rows with strictly increasing listens and a note)".to_string(),
            ));
        }
    }
}

pub fn build_session_recipe(session: &Value) -> Result<Value, String> {
    let is_v2 = jstr(session, "format") == SESSION_FORMAT_V2;
    let phases_v = jget(session, "phases");
    let phases: &[Value] = phases_v.as_array().map_or(&[], |a| a);
    let total: f64 = phases.iter().map(|p| num(p, "seconds", 0.0)).sum();
    let carrier = jget(session, "carrier");
    let carrier_gain = num(carrier, "gain", 0.0);
    let base_hz = num(carrier, "baseHz", 0.0);
    let mut stems: Vec<Value> = Vec::new();
    let mut voice_placements: Vec<Value> = Vec::new();
    let mut visual_cues: Vec<Value> = Vec::new();
    if is_v2 {
        // Pass-through stems ride into the recipe verbatim:
        // they are how a session carries a source stem (an
        // affirmation voice stem, in production the torori
        // render) for its encoded subliminal to encode.
        if let Some(extra) = jget(session, "stems").as_array() {
            stems.extend(extra.iter().cloned());
        }
    }
    let bed = jget(session, "bed");
    if bed.is_object() {
        let mut stem = Map::new();
        stem.insert("type".to_string(), json!("noise"));
        stem.insert("cutoffHz".to_string(), json!(num(bed, "cutoffHz", 800.0)));
        stem.insert("gain".to_string(), json!(num(bed, "gain", 0.0)));
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
        if is_v2 {
            // Voice on the timeline: narration lines are
            // scheduled against their block at sample-accurate
            // offsets (the block's start, quantised), with any
            // per-line gain envelope carried as data.
            if let Some(lines) = jget(phase, "voiceLines").as_array() {
                for line in lines {
                    let mut placement = Map::new();
                    placement.insert("block".to_string(), json!(jstr(phase, "name")));
                    placement.insert("atSeconds".to_string(), json!(t));
                    placement.insert(
                        "atSample".to_string(),
                        json!((t * 48_000.0).round() as i64),
                    );
                    placement.insert("identity".to_string(), json!(jstr(line, "identity")));
                    placement.insert("record".to_string(), json!(jstr(line, "record")));
                    if jget(line, "envelope").is_object() {
                        placement.insert("envelope".to_string(), jget(line, "envelope").clone());
                    }
                    voice_placements.push(Value::Object(placement));
                }
            }
            // The block's encoded subliminal: its affirmation
            // set, rendered from the named source stem through
            // subliminal-encode/1, at depth below the bed.
            if let Some(sub) = phase.get("subliminal").filter(|s| s.is_object()) {
                let mut stem = Map::new();
                stem.insert("type".to_string(), json!("subliminal"));
                stem.insert("name".to_string(), json!(format!("subliminal-{i}")));
                stem.insert("sourceStem".to_string(), json!(jstr(sub, "sourceStem")));
                stem.insert(
                    "carrierHz".to_string(),
                    json!(num(sub, "carrierHz", 16_000.0)),
                );
                stem.insert(
                    "encoderGain".to_string(),
                    json!(num(sub, "encoderGain", crate::subliminal::ENCODER_GAIN_DEFAULT)),
                );
                let level = num(sub, "levelDb", 0.0);
                stem.insert(
                    "gain".to_string(),
                    json!(session_reference_gain(session) * 10f64.powf(level / 20.0)),
                );
                stem.insert("startSeconds".to_string(), json!(t));
                stem.insert("durationSeconds".to_string(), json!(seconds));
                stems.push(Value::Object(stem));
            }
            // Visual cues: the frame derivation is recorded
            // here, at compile time (the synchrony law).
            if let Some(cues) = jget(phase, "visualCues").as_array() {
                for cue in cues {
                    let at = jget(cue, "atSample").as_u64().unwrap_or(0);
                    let rate = jget(cue, "frameRate").as_u64().unwrap_or(60) as u32;
                    let mut compiled = Map::new();
                    compiled.insert("block".to_string(), json!(jstr(phase, "name")));
                    compiled.insert("atSample".to_string(), json!(at));
                    compiled.insert("frame".to_string(), json!(cue_frame(at, rate)));
                    compiled.insert("frameRate".to_string(), json!(rate));
                    compiled.insert("textRef".to_string(), json!(jstr(cue, "textRef")));
                    compiled.insert(
                        "durationFrames".to_string(),
                        jget(cue, "durationFrames").clone(),
                    );
                    compiled.insert("fadeFrames".to_string(), jget(cue, "fadeFrames").clone());
                    visual_cues.push(Value::Object(compiled));
                }
            }
        }
        t += seconds;
    }
    let sub = jget(session, "subliminal");
    if sub.is_object() {
        let mut stem = Map::new();
        stem.insert("type".to_string(), json!("tone"));
        stem.insert("freqHz".to_string(), json!(num(sub, "freqHz", 0.0)));
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
    doc.insert(
        "truePeakDb".to_string(),
        json!(num(session, "truePeakDb", -1.0)),
    );
    if is_v2 {
        // Sample-accuracy law: compilation quantises every
        // stem's placement to integer sample positions at
        // 48 kHz and records the quantised values alongside
        // the authoring-surface seconds.
        for stem in &mut stems {
            let start = num(stem, "startSeconds", 0.0);
            let duration = num(stem, "durationSeconds", total - start);
            if let Value::Object(map) = stem {
                map.insert(
                    "startSample".to_string(),
                    json!((start * 48_000.0).round() as i64),
                );
                map.insert(
                    "durationSample".to_string(),
                    json!((duration * 48_000.0).round() as i64),
                );
            }
        }
    }
    doc.insert("stems".to_string(), Value::Array(stems));
    if is_v2 {
        if !voice_placements.is_empty() {
            doc.insert("voicePlacements".to_string(), Value::Array(voice_placements));
        }
        if !visual_cues.is_empty() {
            doc.insert("visualCues".to_string(), Value::Array(visual_cues));
        }
        if jget(session, "affirmationSets").is_array() {
            doc.insert(
                "affirmationSets".to_string(),
                jget(session, "affirmationSets").clone(),
            );
        }
        if session.get("frame").is_some() {
            doc.insert("frame".to_string(), jget(session, "frame").clone());
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audioproc::{render_recipe, synthesize_mix, validate_recipe};

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/p4/session-hypno-v2.json"
    ));

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("the v2 fixture parses")
    }

    fn error_codes(session: &Value) -> Vec<String> {
        validate_session(session)
            .iter()
            .filter(|f| f.severity == "error")
            .map(|f| f.code.to_string())
            .collect()
    }

    #[test]
    fn v2_fixture_validates_clean() {
        assert_eq!(error_codes(&fixture()), Vec::<String>::new());
    }

    #[test]
    fn cue_frame_derivation_is_the_sample_clock_law() {
        assert_eq!(cue_frame(144_000, 60), 180);
        assert_eq!(cue_frame(48_000, 30), 30);
        assert_eq!(cue_frame(0, 60), 0);
    }

    #[test]
    fn v2_fixture_compiles_with_its_records() {
        let recipe = build_session_recipe(&fixture()).expect("compiles");
        assert_eq!(recipe["format"], json!("torogao-audio-recipe/1"));
        let stems = recipe["stems"].as_array().expect("stems");
        let sub = stems
            .iter()
            .find(|s| s["type"] == json!("subliminal"))
            .expect("an encoded subliminal stem");
        assert_eq!(sub["name"], json!("subliminal-2"));
        assert_eq!(sub["sourceStem"], json!("affirmation-source"));
        // The suggestion block starts at 3.0 s: 144,000 samples.
        assert_eq!(sub["startSample"], json!(144_000));
        assert_eq!(sub["durationSample"], json!(96_000));
        let placements = recipe["voicePlacements"].as_array().expect("placements");
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0]["atSample"], json!(0));
        assert_eq!(placements[1]["block"], json!("return"));
        assert_eq!(placements[1]["atSample"], json!(288_000));
        assert_eq!(placements[1]["envelope"]["attackSeconds"], json!(0.25));
        let cues = recipe["visualCues"].as_array().expect("cues");
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0]["frame"], json!(180));
        assert_eq!(cues[0]["textRef"], json!("vivienne-patterns"));
        assert!(recipe["affirmationSets"].is_array());
        assert_eq!(recipe["frame"]["type"], json!("everyday"));
        let findings: Vec<String> = validate_recipe(&recipe)
            .iter()
            .filter(|f| f.severity == "error")
            .map(|f| f.code.to_string())
            .collect();
        assert_eq!(findings, Vec::<String>::new());
    }

    #[test]
    fn v2_compiled_subliminal_carries_content() {
        // The compiled recipe's subliminal stem, decoded with
        // the symmetric converter, agrees with its source
        // stem above the null floor (T11's method, applied
        // to the session path end to end).
        let recipe = build_session_recipe(&fixture()).expect("compiles");
        let frames = (7.0 * 48_000.0) as usize;
        let (_, census) = synthesize_mix(&recipe, frames);
        assert_eq!(census.len(), 1);
        assert!(census[0].band_fraction >= crate::subliminal::SPECTRUM_BAND_MIN);
    }

    #[test]
    fn v2_render_is_byte_identical() {
        let recipe = build_session_recipe(&fixture()).expect("compiles");
        let text = torogao_format::jsonfmt::to_d_json(&recipe);
        let base = std::env::temp_dir().join(format!("torogao-v2-{}", std::process::id()));
        let a = base.join("a");
        let b = base.join("b");
        std::fs::create_dir_all(&a).expect("dir a");
        std::fs::create_dir_all(&b).expect("dir b");
        let ra = render_recipe(&recipe, &text, a.to_str().expect("utf8"));
        let rb = render_recipe(&recipe, &text, b.to_str().expect("utf8"));
        assert!(ra.error.is_empty(), "{}", ra.error);
        assert!(rb.error.is_empty(), "{}", rb.error);
        let wav_a = std::fs::read(a.join("hypno-v2-fixture.wav")).expect("wav a");
        let wav_b = std::fs::read(b.join("hypno-v2-fixture.wav")).expect("wav b");
        assert_eq!(wav_a, wav_b);
        let subs = ra.manifest["subliminalStems"].as_array().expect("census");
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0]["encoder"], json!("subliminal-encode/1"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn anchor_referencing_a_missing_set_fails_typed() {
        let mut doc = fixture();
        doc["phases"][2]["subliminal"]["affirmationSet"] = json!("no-such-set");
        let codes = error_codes(&doc);
        assert!(
            codes.iter().any(|c| c == TG_SESSION_AFFIRMATION),
            "{codes:?}"
        );
    }

    #[test]
    fn affirmation_set_without_estate_provenance_is_refused() {
        let mut doc = fixture();
        doc["affirmationSets"][0]["provenance"] = json!("studied creator, corpus import");
        let codes = error_codes(&doc);
        assert!(
            codes.iter().any(|c| c == TG_SESSION_PROVENANCE),
            "{codes:?}"
        );
    }

    #[test]
    fn v2_fields_under_a_v1_tag_are_refused() {
        let mut doc = fixture();
        doc["format"] = json!("torogao-session/1");
        let codes = error_codes(&doc);
        assert!(codes.iter().any(|c| c == TG_SESSION_FORMAT), "{codes:?}");
    }

    #[test]
    fn deepener_without_a_count_range_fails_typed() {
        let mut doc = fixture();
        doc["phases"][1]
            .as_object_mut()
            .expect("phase object")
            .remove("countFrom");
        let codes = error_codes(&doc);
        assert!(codes.iter().any(|c| c == TG_SESSION_KIND), "{codes:?}");
    }
}
