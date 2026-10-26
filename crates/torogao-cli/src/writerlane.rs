//! The writer model lane: fills a governed scaffold's slots
//! through a fixture transcript or an HTTP endpoint, then
//! gates the draft (word bounds, deny scan, refrain closing).
//! Ported from the D `torogao.cli.writerlane` module. Any gate
//! failure refuses the WHOLE draft; the lane never re-prompts
//! or repairs inside one call.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::packs::pack_kink_vocabulary;
use crate::util::{jget, jstr};
use crate::writer::{ScaffoldOutcome, build_scaffold};

pub const LANE_FORMAT: &str = "torogao-writer-lane/1";
pub const TRANSCRIPT_FORMAT: &str = "torogao-writer-transcript/1";
pub const DRAFT_FORMAT: &str = "torogao-draft/1";
pub const TG_WRITER_DRAFT_INCOMPLETE: &str = "TG-WRITER-DRAFT-INCOMPLETE";
pub const TG_WRITER_DRAFT_WORDS: &str = "TG-WRITER-DRAFT-WORDS";
pub const TG_WRITER_DRAFT_DENIED: &str = "TG-WRITER-DRAFT-DENIED";
pub const TG_WRITER_DRAFT_REFRAIN: &str = "TG-WRITER-DRAFT-REFRAIN";
pub const TG_WRITER_LANE: &str = "TG-WRITER-LANE";

#[derive(Clone)]
pub struct LaneConfig {
    pub mode: String,
    pub transcript: String,
    pub endpoint: String,
    pub model: String,
}

pub fn load_lane_config(path: &str) -> Result<LaneConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("lane config unreadable: {e}"))?;
    let doc: Value =
        serde_json::from_str(&text).map_err(|e| format!("lane config unreadable: {e}"))?;
    if jstr(&doc, "format") != LANE_FORMAT {
        return Err("lane config format must be \"torogao-writer-lane/1\"".to_string());
    }
    let mut transcript = jstr(&doc, "transcript").to_string();
    if !transcript.is_empty() && !std::path::Path::new(&transcript).is_absolute() {
        let dir = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !dir.is_empty() {
            transcript = format!("{dir}/{transcript}");
        }
    }
    let cfg = LaneConfig {
        mode: jstr(&doc, "mode").to_string(),
        transcript,
        endpoint: jstr(&doc, "endpoint").to_string(),
        model: jstr(&doc, "model").to_string(),
    };
    match cfg.mode.as_str() {
        "fixture" => {
            if cfg.transcript.is_empty() {
                return Err("fixture lane requires a 'transcript' path".to_string());
            }
        }
        "http" => {
            if cfg.endpoint.is_empty() || cfg.model.is_empty() {
                return Err("http lane requires 'endpoint' and 'model'".to_string());
            }
        }
        _ => return Err("lane mode must be \"fixture\" or \"http\"".to_string()),
    }
    Ok(cfg)
}

pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

fn word_bounds_for(scaffold: &Value, class: &str) -> (i64, i64) {
    let bounds = jget(jget(scaffold, "wordCounts"), class);
    let int_or_zero = |v: &Value| -> i64 {
        match v {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    i
                } else if let Some(u) = n.as_u64() {
                    u as i64
                } else {
                    0
                }
            }
            _ => 0,
        }
    };
    (
        int_or_zero(jget(bounds, "minWords")),
        int_or_zero(jget(bounds, "maxWords")),
    )
}

struct Slot {
    id: String,
    class: String,
    function: String,
    closing: bool,
}

fn slot_prompt(scaffold: &Value, slot: &Slot, min: i64, max: i64) -> String {
    let function = if slot.class == "prose" {
        jstr(jget(scaffold, "proseStructure"), "structure").to_string()
    } else {
        slot.function.clone()
    };
    let stage = match &jget(scaffold, "stage") {
        Value::Number(n) => n.as_i64().unwrap_or(0),
        _ => 0,
    };
    let mut prompt = format!(
        "You are filling one slot of a governed scene scaffold.\nCharacter: {}\nKink: {}, stage: {}\nSlot function: {}\nStage scaffold: {}\nWrite between {} and {} words. Stay inside the character's canon; use no kink vocabulary outside the scaffold. Reply with the slot text only.",
        jstr(scaffold, "character"),
        jstr(scaffold, "kink"),
        stage,
        function,
        jstr(scaffold, "stageScaffold"),
        min,
        max
    );
    if slot.closing {
        prompt.push_str(&format!(
            "\nEnd with this line verbatim: {}",
            jstr(scaffold, "refrain")
        ));
    }
    prompt
}

struct FixtureCache {
    doc: Option<Value>,
}

impl FixtureCache {
    fn fill(&mut self, cfg: &LaneConfig, slot_id: &str) -> Result<String, String> {
        if self.doc.is_none() {
            let text = std::fs::read_to_string(&cfg.transcript)
                .map_err(|e| format!("transcript unreadable: {e}"))?;
            let doc: Value =
                serde_json::from_str(&text).map_err(|e| format!("transcript unreadable: {e}"))?;
            if jstr(&doc, "format") != TRANSCRIPT_FORMAT {
                return Err("transcript format must be \"torogao-writer-transcript/1\"".to_string());
            }
            self.doc = Some(doc);
        }
        let doc = self.doc.as_ref().unwrap();
        match jget(jget(doc, "responses"), slot_id) {
            Value::String(s) => Ok(s.clone()),
            _ => Err(format!(
                "transcript carries no response for slot '{slot_id}'"
            )),
        }
    }
}

fn http_fill(cfg: &LaneConfig, prompt: &str) -> Result<String, String> {
    // Minimal HTTP/1.1 POST (the estate has no HTTP client
    // dependency; the endpoint is a local model server).
    let endpoint = cfg.endpoint.trim_end_matches('/');
    let url = format!("{endpoint}/api/generate");
    let stripped = url
        .strip_prefix("http://")
        .ok_or_else(|| "endpoint call failed: only http:// endpoints are supported".to_string())?;
    let (host_port, path) = match stripped.find('/') {
        Some(i) => (&stripped[..i], &stripped[i..]),
        None => (stripped, "/"),
    };
    let mut body = Map::new();
    body.insert("model".to_string(), json!(cfg.model));
    body.insert("prompt".to_string(), json!(prompt));
    body.insert("stream".to_string(), json!(false));
    let body_text = torogao_format::jsonfmt::to_d_json(&Value::Object(body));
    let mut stream =
        TcpStream::connect(host_port).map_err(|e| format!("endpoint call failed: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(180)))
        .map_err(|e| format!("endpoint call failed: {e}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(180)))
        .map_err(|e| format!("endpoint call failed: {e}"))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_text}",
        body_text.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("endpoint call failed: {e}"))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("endpoint call failed: {e}"))?;
    let raw_text = String::from_utf8_lossy(&raw);
    let body_start = raw_text
        .find("\r\n\r\n")
        .ok_or_else(|| "endpoint call failed: malformed HTTP response".to_string())?;
    let response_text = &raw_text[body_start + 4..];
    // Chunked transfer-encoding is not decoded; the model
    // servers this lane targets answer with Content-Length.
    let doc: Value =
        serde_json::from_str(response_text).map_err(|e| format!("endpoint call failed: {e}"))?;
    match jget(&doc, "response") {
        Value::String(s) => Ok(s.clone()),
        _ => Err("endpoint returned no 'response' text".to_string()),
    }
}

pub enum LaneOutcome {
    Draft(Value),
    Gap(Value),
    Refusal(Value),
    Error(String),
}

struct SlotFill {
    id: String,
    class: String,
    text: String,
}

fn refusal_doc(code: &'static str, detail: String, cfg: &LaneConfig) -> Value {
    let mut doc = Map::new();
    doc.insert("format".to_string(), json!(DRAFT_FORMAT));
    doc.insert("result".to_string(), json!("refused"));
    doc.insert("code".to_string(), json!(code));
    doc.insert("detail".to_string(), json!(detail));
    doc.insert("transport".to_string(), json!(cfg.mode));
    if cfg.mode == "http" {
        doc.insert("model".to_string(), json!(cfg.model));
    }
    doc.insert(
        "note".to_string(),
        json!("The lane refuses the whole draft and never re-prompts or repairs inside one call."),
    );
    Value::Object(doc)
}

pub fn run_writer_lane(
    character: &Value,
    pack: &Value,
    kink: &str,
    stage: i64,
    sfx_ids: &[String],
    cfg: &LaneConfig,
) -> LaneOutcome {
    let scaffold = match build_scaffold(character, pack, kink, stage, sfx_ids) {
        ScaffoldOutcome::Scaffold(doc) => doc,
        ScaffoldOutcome::Gap(doc) => return LaneOutcome::Gap(doc),
    };
    // Slots: the dialogue slots in order, then the prose slot.
    let mut slots: Vec<Slot> = Vec::new();
    let dialogue = jget(&scaffold, "dialogueSlots");
    if let Some(arr) = dialogue.as_array() {
        let n = arr.len();
        for (i, slot) in arr.iter().enumerate() {
            slots.push(Slot {
                id: jstr(slot, "id").to_string(),
                class: "dialogue".to_string(),
                function: jstr(slot, "function").to_string(),
                closing: i + 1 == n,
            });
        }
    }
    slots.push(Slot {
        id: "prose".to_string(),
        class: "prose".to_string(),
        function: String::new(),
        closing: false,
    });
    let mut fixture = FixtureCache { doc: None };
    let mut fills: Vec<SlotFill> = Vec::new();
    for slot in &slots {
        let text = if cfg.mode == "fixture" {
            match fixture.fill(cfg, &slot.id) {
                Ok(t) => t,
                Err(e) => return LaneOutcome::Error(e),
            }
        } else {
            let (min, max) = word_bounds_for(&scaffold, &slot.class);
            let prompt = slot_prompt(&scaffold, slot, min, max);
            match http_fill(cfg, &prompt) {
                Ok(t) => t,
                Err(e) => return LaneOutcome::Error(e),
            }
        };
        fills.push(SlotFill {
            id: slot.id.clone(),
            class: slot.class.clone(),
            text,
        });
    }
    // Gate 1: per-slot completeness and word bounds.
    for fill in &fills {
        if fill.text.is_empty() {
            return LaneOutcome::Refusal(refusal_doc(
                TG_WRITER_DRAFT_INCOMPLETE,
                format!("slot '{}' came back empty", fill.id),
                cfg,
            ));
        }
        let (min, max) = word_bounds_for(&scaffold, &fill.class);
        let words = count_words(&fill.text) as i64;
        if words < min || words > max {
            return LaneOutcome::Refusal(refusal_doc(
                TG_WRITER_DRAFT_WORDS,
                format!(
                    "slot '{}' holds {} words; the pack bound for class '{}' is {}..{}",
                    fill.id, words, fill.class, min, max
                ),
                cfg,
            ));
        }
    }
    // Gate 2: deny scan over the joined fills.
    let joined = fills
        .iter()
        .map(|f| f.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let vector = jget(character, "kinkVector");
    let in_vector = |term: &str| -> bool {
        vector
            .as_array()
            .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(term)))
    };
    for term in pack_kink_vocabulary(pack) {
        if term == kink || in_vector(&term) {
            continue;
        }
        if joined.contains(&term.to_lowercase()) {
            return LaneOutcome::Refusal(refusal_doc(
                TG_WRITER_DRAFT_DENIED,
                format!("draft text uses '{term}', which is outside the character's kink vector"),
                cfg,
            ));
        }
    }
    // Gate 3: the closing dialogue line carries the refrain.
    let refrain = jstr(&scaffold, "refrain");
    let closing_text = fills
        .iter()
        .rfind(|f| f.class == "dialogue")
        .map(|f| f.text.as_str())
        .unwrap_or("");
    if !closing_text.contains(refrain) {
        return LaneOutcome::Refusal(refusal_doc(
            TG_WRITER_DRAFT_REFRAIN,
            "the closing dialogue line does not carry the refrain verbatim".to_string(),
            cfg,
        ));
    }
    let mut fills_doc = Map::new();
    for fill in &fills {
        let mut entry = Map::new();
        entry.insert("text".to_string(), json!(fill.text));
        entry.insert("words".to_string(), json!(count_words(&fill.text)));
        fills_doc.insert(fill.id.clone(), Value::Object(entry));
    }
    let mut checks = Map::new();
    checks.insert("denyScan".to_string(), json!("pass"));
    checks.insert("wordBounds".to_string(), json!("pass"));
    checks.insert("refrainClosing".to_string(), json!("pass"));
    let mut doc = Map::new();
    doc.insert("format".to_string(), json!(DRAFT_FORMAT));
    doc.insert("result".to_string(), json!("draft"));
    doc.insert("transport".to_string(), json!(cfg.mode));
    if cfg.mode == "http" {
        doc.insert("model".to_string(), json!(cfg.model));
    }
    doc.insert("scaffold".to_string(), scaffold);
    doc.insert("fills".to_string(), Value::Object(fills_doc));
    doc.insert("checks".to_string(), Value::Object(checks));
    LaneOutcome::Draft(Value::Object(doc))
}
