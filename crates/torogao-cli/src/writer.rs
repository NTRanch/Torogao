//! The writer core (writer.d port): canon-grounded
//! scene scaffolds. A request whose kink is not in the
//! character's vector — or whose canon inputs are
//! missing — produces a loud gap report naming the
//! missing canon. It never produces invented content.

use crate::packs::{pack_expression, pack_kink_vocabulary, pack_str, pack_word_bounds};
use crate::paths::find_data_file;
use crate::util::jget;
use serde_json::{Value, json};
use sha2::Digest;
use torogao_format::jsonfmt::to_d_json;

/// TG-WRITER-GAP.
pub const TG_WRITER_GAP: &str = "TG-WRITER-GAP";
/// TG-WRITER-DENIED.
pub const TG_WRITER_DENIED: &str = "TG-WRITER-DENIED";
/// TG-VOICE-PROVENANCE.
pub const TG_VOICE_PROVENANCE: &str = "TG-VOICE-PROVENANCE";
/// TG-VOICE-ANCHOR.
pub const TG_VOICE_ANCHOR: &str = "TG-VOICE-ANCHOR";

/// Character record format identifier.
pub const CHARACTER_FORMAT: &str = "torogao-character/1";
/// Voice record format identifier.
pub const VOICE_FORMAT: &str = "torogao-voice/1";

/// Loads a JSON document from a file; on failure returns
/// the reference's error string.
pub fn load_json_file(path: &str) -> Result<Value, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("file '{path}' is unreadable: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("file '{path}' is unreadable: {e}"))
}

/// Whether a value counts as present canon: a non-empty
/// string, an object, or an array.
fn non_empty(value: &Value) -> bool {
    match value {
        Value::String(s) => !s.is_empty(),
        Value::Object(_) | Value::Array(_) => true,
        _ => false,
    }
}

/// Whether a value is a positive integer (>= 1) in either
/// integer storage.
fn positive_int(value: &Value) -> bool {
    if let Some(n) = value.as_i64() {
        n >= 1
    } else if let Some(u) = value.as_u64() {
        u >= 1
    } else {
        false
    }
}

/// Whether a value is an integer equal to `stage`.
fn int_equals(value: &Value, stage: i64) -> bool {
    if let Some(n) = value.as_i64() {
        n == stage
    } else if let Some(u) = value.as_u64() {
        u == stage as u64
    } else {
        false
    }
}

/// One SFX library entry by id; null when absent.
pub fn sfx_entry(index: &Value, id: &str) -> Value {
    let entries = jget(index, "entries");
    if let Some(list) = entries.as_array() {
        for entry in list {
            if pack_str(entry, "id") == id {
                return entry.clone();
            }
        }
    }
    Value::Null
}

/// Loads the SFX library index (the sfx/ data
/// directory).
pub fn load_sfx_index() -> Result<Value, String> {
    let Some(path) = find_data_file("TOROGAO_SFX_DIR", "sfx", "index.json") else {
        return Err("SFX library index not found".to_string());
    };
    let doc = load_json_file(&path.to_string_lossy())?;
    if pack_str(&doc, "format") != "torogao-sfx-index/1" {
        return Err("SFX library index is not a torogao-sfx-index/1 document".to_string());
    }
    Ok(doc)
}

/// SHA-256 (hex) of an SFX recipe file in the library.
pub fn sfx_recipe_hash(recipe_file: &str) -> Result<String, String> {
    let Some(path) = find_data_file("TOROGAO_SFX_DIR", "sfx", recipe_file) else {
        return Err(format!("SFX recipe '{recipe_file}' not found"));
    };
    let bytes =
        std::fs::read(&path).map_err(|e| format!("SFX recipe '{recipe_file}' unreadable: {e}"))?;
    let digest = sha2::Sha256::digest(&bytes);
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// The outcome of a scaffold build: exactly one of the
/// two documents is meaningful.
pub enum ScaffoldOutcome {
    /// A scaffold that passed every gate.
    Scaffold(Value),
    /// A gap report (missing canon) or a denial.
    Gap(Value),
}

/// Builds a scaffold for (character x kink x stage), or
/// a gap report when the canon does not cover the
/// request.
pub fn build_scaffold(
    character: &Value,
    pack: &Value,
    kink: &str,
    stage: i64,
    sfx_ids: &[String],
) -> ScaffoldOutcome {
    let mut missing: Vec<String> = Vec::new();

    if pack_str(character, "format") != CHARACTER_FORMAT {
        missing.push(format!(
            "character record format (expected {CHARACTER_FORMAT})"
        ));
    }
    const REQUIRED_CANON: [(&str, &str); 6] = [
        ("name", "character name"),
        ("anchors", "identity anchors"),
        ("kinkVector", "kink vector"),
        ("refrain", "refrain"),
        ("voiceRules", "voice rules"),
        ("voice", "voice record (anchor id + version + provenance)"),
    ];
    for (field, label) in REQUIRED_CANON {
        let value = jget(character, field);
        let ok = match field {
            "kinkVector" => value.as_array().is_some_and(|a| !a.is_empty()),
            "anchors" | "voiceRules" | "voice" => value.is_object(),
            _ => non_empty(value),
        };
        if !ok {
            missing.push(label.to_string());
        }
    }

    // Canon-vector check: the requested kink must be IN
    // the vector.
    let vector = jget(character, "kinkVector");
    let mut in_vector = false;
    if let Some(list) = vector.as_array() {
        for entry in list {
            if entry.as_str() == Some(kink) {
                in_vector = true;
            }
        }
    }
    if !in_vector {
        missing.push(format!(
            "kink '{kink}' is not in the character's kink vector"
        ));
    }

    // Ladder coverage: the stage must be a stage the
    // pack's ladder defines.
    let mut ladder_entry = Value::Null;
    if let Some(ladder) = jget(pack, "arcLadder").as_array() {
        for entry in ladder {
            if int_equals(jget(entry, "stage"), stage) {
                ladder_entry = entry.clone();
            }
        }
    }
    if !ladder_entry.is_object() {
        missing.push(format!(
            "stage {stage} is not a stage of the pack's arc ladder"
        ));
    }

    // The rupture line is required canon at its slot.
    let rupture_field = jget(pack, "ruptureStage");
    let rupture_stage: i64 = rupture_field.as_i64().unwrap_or(-1);
    if stage == rupture_stage && pack_str(character, "ruptureLine").is_empty() {
        missing.push("rupture line (required at the rupture stage)".to_string());
    }

    // Word-count bounds must exist for every scene class.
    for cls in ["dialogue", "prose", "script"] {
        if !pack_word_bounds(pack, cls).is_object() {
            missing.push(format!("word-count bounds for scene class '{cls}'"));
        }
    }

    // Voice canon: provenance + anchor shape.
    let voice = jget(character, "voice");
    if voice.is_object() {
        if pack_str(voice, "provenance") != "estate-synthetic" {
            missing.push("voice provenance (must be estate-synthetic)".to_string());
        }
        if !positive_int(jget(voice, "anchorVersion")) {
            missing.push("voice anchor version (a positive integer)".to_string());
        }
        if pack_str(voice, "anchorId").is_empty() {
            missing.push("voice anchor id".to_string());
        }
    }

    // SFX canon: every requested cue must be a library
    // entry. An unloadable index means no entry exists.
    let index = load_sfx_index().unwrap_or(Value::Null);
    let mut sfx_entries: Vec<Value> = Vec::new();
    for id in sfx_ids {
        let entry = sfx_entry(&index, id);
        if !entry.is_object() {
            missing.push(format!("sfx entry '{id}' is not in the SFX library"));
        } else {
            sfx_entries.push(entry);
        }
    }

    if !missing.is_empty() {
        return ScaffoldOutcome::Gap(json!({
            "character": pack_str(character, "name"),
            "code": TG_WRITER_GAP,
            "format": "torogao-gap-report/1",
            "kink": kink,
            "missingCanon": missing,
            "note": "The writer does not invent canon. Supply the named fields (character record / lane pack) and re-run.",
            "result": "gap-report",
            "stage": stage,
        }));
    }

    // -- scaffold construction (every element traces) ---
    let mut doc = json!({
        "anchors": jget(character, "anchors").clone(),
        "character": pack_str(character, "name"),
        "conditioningTracks": jget(character, "conditioningTracks").clone(),
        "format": "torogao-scaffold/1",
        "kink": kink,
        "ladder": ladder_entry.clone(),
        "pack": pack_str(pack, "name"),
        "refrain": pack_str(character, "refrain"),
        "result": "scaffold",
        "stage": stage,
    });
    if stage == rupture_stage {
        doc["ruptureLine"] = json!(pack_str(character, "ruptureLine"));
    }

    let mut bounds = json!({});
    for cls in ["dialogue", "prose", "script"] {
        bounds[cls] = pack_word_bounds(pack, cls);
    }
    doc["wordCounts"] = bounds;

    let scaffold_text = pack_str(&ladder_entry, "scaffold");
    let slot_functions = [
        "the opening ask, in her register",
        "the answer that moves the stage one rung",
        "the closing line, which carries the refrain's shape",
    ];
    let mut slots = Vec::new();
    for (i, function) in slot_functions.iter().enumerate() {
        slots.push(json!({
            "function": function,
            "id": format!("d{}", i + 1),
            "speaker": pack_str(character, "name"),
            "stageScaffold": scaffold_text,
            "wordClass": "dialogue",
        }));
    }
    doc["dialogueSlots"] = json!(slots);

    doc["proseStructure"] = json!({
        "structure": scaffold_text,
        "voiceRules": jget(character, "voiceRules").clone(),
        "wordClass": "prose",
    });

    let stage_expr = jget(jget(character, "stageExpressions"), &format!("{stage}")).clone();
    let mut cue = json!({ "beat": "open" });
    if let Some(expression) = stage_expr.as_str() {
        cue["expression"] = json!(expression);
        cue["governed"] = json!(pack_expression(pack, expression).is_object());
    } else {
        cue["expressionRole"] = json!("baseline");
        cue["note"] = json!(
            "no stage expression recorded for this stage; the baseline governed expression applies"
        );
    }
    let mut audio_cues = Vec::new();
    for entry in &sfx_entries {
        audio_cues.push(json!({
            "atBeat": "open",
            "name": pack_str(entry, "name"),
            "recipe": pack_str(entry, "recipe"),
            "recipeSha256": sfx_recipe_hash(&pack_str(entry, "recipe")).unwrap_or_default(),
            "sfx": pack_str(entry, "id"),
        }));
    }
    doc["sceneScript"] = json!({
        "audioCues": audio_cues,
        "parameterCues": [cue],
        "voiceRef": {
            "anchorId": pack_str(voice, "anchorId"),
            "anchorVersion": jget(voice, "anchorVersion").clone(),
            "provenance": pack_str(voice, "provenance"),
        },
        "wordClass": "script",
    });

    // -- the gates, run before the scaffold is offered --
    let refrain_pass = pack_str(&doc, "refrain") == pack_str(character, "refrain");
    // Deny scan: no vocabulary term outside the
    // character's vector (other than the requested kink,
    // which is in it) may appear anywhere in the scaffold
    // text. The scan runs over the document BEFORE the
    // checks block joins it, exactly as the reference
    // orders it.
    let text = to_d_json(&doc).to_lowercase();
    let mut denied_hits: Vec<String> = Vec::new();
    for term in pack_kink_vocabulary(pack) {
        if term == kink {
            continue;
        }
        let mut allowed = false;
        if let Some(list) = vector.as_array() {
            for entry in list {
                if entry.as_str() == Some(term.as_str()) {
                    allowed = true;
                }
            }
        }
        if allowed {
            continue;
        }
        if text.contains(&term.to_lowercase()) {
            denied_hits.push(term);
        }
    }
    doc["checks"] = json!({
        "canonVector": "pass",
        "denyScan": if denied_hits.is_empty() { "pass" } else { "fail" },
        "refrainVerbatim": if refrain_pass { "pass" } else { "fail" },
        "wordBounds": "pass",
    });

    if !denied_hits.is_empty() {
        return ScaffoldOutcome::Gap(json!({
            "character": pack_str(character, "name"),
            "code": TG_WRITER_DENIED,
            "deniedTerms": denied_hits,
            "format": "torogao-gap-report/1",
            "note": "The scaffold would contain a kink term outside the character's vector; it is refused, not shipped.",
            "result": "denied",
        }));
    }
    if !refrain_pass {
        return ScaffoldOutcome::Gap(json!({
            "character": pack_str(character, "name"),
            "code": TG_WRITER_DENIED,
            "format": "torogao-gap-report/1",
            "note": "The refrain did not survive verbatim; refused.",
            "result": "denied",
        }));
    }

    ScaffoldOutcome::Scaffold(doc)
}

/// One voice-validation finding.
#[derive(Debug, Clone)]
pub struct VoiceFinding {
    /// Stable finding code.
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
}

impl VoiceFinding {
    /// The finding as a JSON document.
    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
        })
    }
}

/// Validates a voice record: either a standalone
/// torogao-voice/1 document or the "voice" block of a
/// character record. Estate-synthetic provenance only;
/// anchors are versioned, never edited in place.
pub fn validate_voice(doc: &Value) -> Vec<VoiceFinding> {
    let mut findings = Vec::new();
    let format = pack_str(doc, "format");
    let voice: Value = if format == CHARACTER_FORMAT
        || (format != VOICE_FORMAT && jget(doc, "voice").is_object())
    {
        jget(doc, "voice").clone()
    } else {
        doc.clone()
    };
    if !voice.is_object() {
        findings.push(VoiceFinding {
            code: TG_VOICE_ANCHOR,
            message: "no voice record found in the document".to_string(),
        });
        return findings;
    }
    if pack_str(&voice, "provenance") != "estate-synthetic" {
        findings.push(VoiceFinding {
            code: TG_VOICE_PROVENANCE,
            message: "voice provenance must be estate-synthetic; any other source is not permitted in a voice record".to_string(),
        });
    }
    if pack_str(&voice, "anchorId").is_empty() {
        findings.push(VoiceFinding {
            code: TG_VOICE_ANCHOR,
            message: "voice anchor id is empty".to_string(),
        });
    }
    if !positive_int(jget(&voice, "anchorVersion")) {
        findings.push(VoiceFinding {
            code: TG_VOICE_ANCHOR,
            message: "voice anchor version must be a positive integer (anchors are versioned, never edited in place)".to_string(),
        });
    }
    findings
}
