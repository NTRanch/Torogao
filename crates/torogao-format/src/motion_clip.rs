//! The `torogao-motion-clip/2` validator and freeze pipeline
//! (hand-animation programme Rung H0; the format's specification
//! of record is this crate's `FORMATS.md`).
//!
//! A motion clip is a canonical JSON document: hand-authored
//! keys on integer frames, explicit Hermite tangents, pose
//! provenance, shader modules under a closed uniform contract,
//! and an append-only op log. A *working* document is fluid
//! authoring state and may carry edit-time tangent modes; a
//! *frozen* document carries a freeze block whose SHA-256
//! covers the canonical form of the document without it, and
//! may not carry anything the freeze was required to resolve
//! (DESIGN.md sections 2.3, 3.3, 3.4).
//!
//! Checks run in specification order and stop at the first
//! failure, so the reported error is deterministic. Nothing is
//! repaired, dropped, or clamped here — invalid input is a
//! typed [`ClipError`], per the loud-error law.

use std::fmt;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::jsonfmt::to_d_json;
use crate::limits::{
    CLIP_CHANNELS_MAX, CLIP_KEYS_PER_CHANNEL_MAX, CLIP_NAME_BYTES_MAX, CLIP_OP_LOG_MAX,
    CLIP_SHADER_MODULES_MAX, CLIP_SHADER_SOURCE_BYTES_MAX, CLIP_UNIFORMS_PER_MODULE_MAX,
    INPUT_BYTES_MAX,
};

/// The format stamp. This constant is the one match site for
/// the token; every comparison goes through it.
pub const MOTION_CLIP_FORMAT: &str = "torogao-motion-clip/2";

/// The closed list of shader stage hooks (DESIGN.md section
/// 7.2), in the renderer's composition order.
pub const SHADER_HOOKS: [&str; 3] = ["skinning_assist", "morph_assist", "post_skin_displacement"];

/// The key types: the craft vocabulary, stored as data
/// (DESIGN.md section 2.1).
pub const KEY_TYPES: [&str; 4] = ["keyframe", "breakdown", "moving_hold", "extreme"];

/// Tangent modes a document may name. Automatic modes are
/// edit-time only (DESIGN.md section 3.3): a working document
/// may carry them, a frozen one may not.
pub const TANGENT_MODES: [&str; 6] = [
    "explicit",
    "free",
    "aligned",
    "vector",
    "auto",
    "auto_clamped",
];

/// A typed refusal from the motion-clip validator: a stable
/// `TG-CLIP-` code plus a human message, the shape this crate's
/// surfaces report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipError {
    code: &'static str,
    message: String,
}

impl ClipError {
    fn new(code: &'static str, message: impl Into<String>) -> ClipError {
        ClipError {
            code,
            message: message.into(),
        }
    }

    /// The stable machine-readable code (for example
    /// `TG-CLIP-FLOAT-FRAME`).
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The human-readable explanation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ClipError {}

/// The result type every motion-clip operation returns.
pub type ClipResult<T> = Result<T, ClipError>;

/// What a successful validation established about the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipReport {
    /// The clip's name.
    pub name: String,
    /// The target rig identity (lowercase hex SHA-256).
    pub rig: String,
    /// Frame-rate numerator.
    pub rate_num: u64,
    /// Frame-rate denominator.
    pub rate_den: u64,
    /// First frame of the clip range.
    pub range_start: i64,
    /// Last frame of the clip range.
    pub range_end: i64,
    /// The loop range, when the clip loops.
    pub loop_range: Option<(i64, i64)>,
    /// Channel count.
    pub channels: usize,
    /// Total key count across channels.
    pub keys: usize,
    /// Shader module names, in document order.
    pub shader_modules: Vec<String>,
    /// The freeze hash, when the document is frozen.
    pub freeze_sha256: Option<String>,
}

/// The canonical byte form of a motion-clip document: the
/// estate canonical JSON writer over the document with its
/// freeze block removed. The freeze hash covers exactly these
/// bytes.
pub fn canonical_bytes(doc: &Value) -> String {
    let mut stripped = doc.clone();
    if let Some(object) = stripped.as_object_mut() {
        object.remove("freeze");
    }
    to_d_json(&stripped)
}

/// The freeze hash of a document: lowercase hex SHA-256 of its
/// canonical bytes.
pub fn freeze_hash(doc: &Value) -> String {
    let digest = Sha256::digest(canonical_bytes(doc).as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Validates a motion-clip document from its bytes. A document
/// carrying a freeze block is held to the frozen rules
/// (resolution complete, hash reproduces); one without is held
/// to the working rules.
pub fn validate_clip(bytes: &[u8]) -> ClipResult<ClipReport> {
    if bytes.len() > INPUT_BYTES_MAX {
        return Err(ClipError::new(
            "TG-CLIP-TOO-LARGE",
            format!(
                "document is {} bytes, over the {INPUT_BYTES_MAX} byte limit",
                bytes.len()
            ),
        ));
    }
    let doc: Value = serde_json::from_slice(bytes).map_err(|error| {
        ClipError::new("TG-CLIP-NOT-JSON", format!("document is not JSON: {error}"))
    })?;
    validate_document(&doc)
}

/// Validates an already-parsed document (see [`validate_clip`]).
pub fn validate_document(doc: &Value) -> ClipResult<ClipReport> {
    let root = doc.as_object().ok_or_else(|| {
        ClipError::new("TG-CLIP-NOT-OBJECT", "the document root is not an object")
    })?;
    let frozen = root.contains_key("freeze");

    // The format stamp: the one match site.
    match root.get("format").and_then(Value::as_str) {
        Some(token) if token == MOTION_CLIP_FORMAT => {}
        Some(token) => {
            return Err(ClipError::new(
                "TG-CLIP-FORMAT",
                format!("format stamp is {token:?}, not {MOTION_CLIP_FORMAT:?}"),
            ));
        }
        None => {
            return Err(ClipError::new(
                "TG-CLIP-FORMAT",
                "the format field is missing or not a string",
            ));
        }
    }

    let name = required_string(root, "name", CLIP_NAME_BYTES_MAX)?;
    let rig = required_string(root, "rig", 64)?;
    if !is_lower_hex(&rig, 64) {
        return Err(ClipError::new(
            "TG-CLIP-RIG-IDENTITY",
            "rig is not a 64-digit lowercase hex SHA-256 of the hierarchy definition",
        ));
    }

    let rate = required_object(root, "rate")?;
    let rate_num = required_positive_u64(rate, "num", "rate")?;
    let rate_den = required_positive_u64(rate, "den", "rate")?;

    let range = required_object(root, "range")?;
    let range_start = required_frame(range, "start", "range")?;
    let range_end = required_frame(range, "end", "range")?;
    if range_start < 0 || range_end < range_start {
        return Err(ClipError::new(
            "TG-CLIP-RANGE",
            format!("range {range_start}..={range_end} is empty or negative"),
        ));
    }

    let loop_range = match root.get("loop") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let object = value.as_object().ok_or_else(|| {
                ClipError::new("TG-CLIP-WRONG-TYPE", "loop is not an object or null")
            })?;
            let start = required_frame(object, "start", "loop")?;
            let end = required_frame(object, "end", "loop")?;
            if start < range_start || end > range_end || start >= end {
                return Err(ClipError::new(
                    "TG-CLIP-LOOP-RANGE",
                    format!(
                        "loop {start}..={end} is not a non-empty sub-range of {range_start}..={range_end}"
                    ),
                ));
            }
            Some((start, end))
        }
    };

    // Channels, with the parameter names collected on the way:
    // shader modules may only read declared parameters.
    let channels = required_array(root, "channels")?;
    if channels.is_empty() {
        return Err(ClipError::new(
            "TG-CLIP-CHANNELS-EMPTY",
            "the clip carries no channels",
        ));
    }
    if channels.len() > CLIP_CHANNELS_MAX {
        return Err(ClipError::new(
            "TG-CLIP-TOO-MANY-CHANNELS",
            format!(
                "{} channels, over the {CLIP_CHANNELS_MAX} limit",
                channels.len()
            ),
        ));
    }
    let mut parameters: Vec<String> = Vec::new();
    let mut seen_targets: Vec<(String, String, String)> = Vec::new();
    let mut total_keys = 0usize;
    for (index, channel) in channels.iter().enumerate() {
        total_keys += validate_channel(
            channel,
            index,
            range_start,
            range_end,
            frozen,
            &mut parameters,
            &mut seen_targets,
        )?;
    }

    // Shader modules.
    let mut shader_modules = Vec::new();
    if let Some(value) = root.get("shaders") {
        let modules = value
            .as_array()
            .ok_or_else(|| ClipError::new("TG-CLIP-WRONG-TYPE", "shaders is not an array"))?;
        if modules.len() > CLIP_SHADER_MODULES_MAX {
            return Err(ClipError::new(
                "TG-CLIP-TOO-MANY-SHADERS",
                format!(
                    "{} shader modules, over the {CLIP_SHADER_MODULES_MAX} limit",
                    modules.len()
                ),
            ));
        }
        for (index, module) in modules.iter().enumerate() {
            let module_name = validate_shader_module(module, index, &parameters)?;
            if shader_modules.contains(&module_name) {
                return Err(ClipError::new(
                    "TG-CLIP-SHADER-NAME-DUP",
                    format!("shader module name {module_name:?} is used twice"),
                ));
            }
            shader_modules.push(module_name);
        }
    }

    // Op log: append-only provenance, sequence from zero.
    let op_log = required_array(root, "op_log")?;
    if op_log.is_empty() {
        return Err(ClipError::new(
            "TG-CLIP-OPLOG-EMPTY",
            "the op log is empty; a clip carries its authoring provenance",
        ));
    }
    if op_log.len() > CLIP_OP_LOG_MAX {
        return Err(ClipError::new(
            "TG-CLIP-OPLOG-TOO-LONG",
            format!(
                "{} op log entries, over the {CLIP_OP_LOG_MAX} limit",
                op_log.len()
            ),
        ));
    }
    for (index, entry) in op_log.iter().enumerate() {
        let entry = entry.as_object().ok_or_else(|| {
            ClipError::new(
                "TG-CLIP-WRONG-TYPE",
                format!("op_log[{index}] is not an object"),
            )
        })?;
        let seq = entry.get("seq").and_then(Value::as_i64);
        if seq != Some(index as i64) {
            return Err(ClipError::new(
                "TG-CLIP-OPLOG-SEQ",
                format!("op_log[{index}] has seq {seq:?}, expected {index}"),
            ));
        }
        match entry.get("actor").and_then(Value::as_str) {
            Some("human") | Some("converter") | Some("agent") => {}
            _ => {
                return Err(ClipError::new(
                    "TG-CLIP-OPLOG-ACTOR",
                    format!("op_log[{index}] actor is not human, converter, or agent"),
                ));
            }
        }
        required_string(entry, "session", CLIP_NAME_BYTES_MAX)
            .map_err(|error| prefix(error, &format!("op_log[{index}]")))?;
        required_string(entry, "op", CLIP_NAME_BYTES_MAX)
            .map_err(|error| prefix(error, &format!("op_log[{index}]")))?;
    }

    // The freeze block: resolution first, then the hash.
    let freeze_sha256 = if frozen {
        let freeze = required_object(root, "freeze")?;
        let recorded = required_string(freeze, "sha256", 64)?;
        if !is_lower_hex(&recorded, 64) {
            return Err(ClipError::new(
                "TG-CLIP-FREEZE-FIELD",
                "freeze.sha256 is not a 64-digit lowercase hex digest",
            ));
        }
        required_string(freeze, "compiler", CLIP_NAME_BYTES_MAX)
            .map_err(|error| prefix(error, "freeze"))?;
        required_string(freeze, "band", 64).map_err(|error| prefix(error, "freeze"))?;
        let computed = freeze_hash(doc);
        if computed != recorded {
            return Err(ClipError::new(
                "TG-CLIP-FREEZE-MISMATCH",
                format!(
                    "freeze hash mismatch: recorded {recorded}, computed {computed} over the canonical form"
                ),
            ));
        }
        Some(recorded)
    } else {
        None
    };

    Ok(ClipReport {
        name,
        rig,
        rate_num,
        rate_den,
        range_start,
        range_end,
        loop_range,
        channels: channels.len(),
        keys: total_keys,
        shader_modules,
        freeze_sha256,
    })
}

/// Freezes a working document: validates it, refuses anything
/// the freeze must resolve (edit-time tangent modes, per
/// DESIGN.md section 3.3), and returns the document with its
/// freeze block inserted. The input is not mutated.
pub fn freeze_document(doc: &Value, compiler: &str, band: &str) -> ClipResult<Value> {
    let root = doc.as_object().ok_or_else(|| {
        ClipError::new("TG-CLIP-NOT-OBJECT", "the document root is not an object")
    })?;
    if root.contains_key("freeze") {
        return Err(ClipError::new(
            "TG-CLIP-ALREADY-FROZEN",
            "the document already carries a freeze block; edit a thawed copy",
        ));
    }
    // Working validation first (deterministic first error),
    // then the resolution scan the working rules tolerate.
    validate_document(doc)?;
    if let Some(channels) = root.get("channels").and_then(Value::as_array) {
        for (index, channel) in channels.iter().enumerate() {
            if let Some(keys) = channel.get("keys").and_then(Value::as_array) {
                for (key_index, key) in keys.iter().enumerate() {
                    if let Some(mode) = key.get("tangent_mode").and_then(Value::as_str) {
                        if mode != "explicit" {
                            return Err(ClipError::new(
                                "TG-CLIP-UNRESOLVED-TANGENT",
                                format!(
                                    "channel {index} key {key_index} carries edit-time tangent mode {mode:?}; the freeze resolves every mode to explicit tangents"
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }
    let mut frozen_doc = doc.clone();
    let hash = freeze_hash(&frozen_doc);
    let freeze = serde_json::json!({
        "band": band,
        "compiler": compiler,
        "sha256": hash,
    });
    frozen_doc
        .as_object_mut()
        .expect("validated as an object")
        .insert("freeze".to_string(), freeze);
    // The frozen form must pass its own validation.
    validate_document(&frozen_doc)?;
    Ok(frozen_doc)
}

fn prefix(error: ClipError, context: &str) -> ClipError {
    ClipError::new(error.code, format!("{context}: {}", error.message))
}

fn is_lower_hex(text: &str, digits: usize) -> bool {
    text.len() == digits
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn required_object<'a>(
    parent: &'a serde_json::Map<String, Value>,
    field: &str,
) -> ClipResult<&'a serde_json::Map<String, Value>> {
    match parent.get(field) {
        Some(Value::Object(object)) => Ok(object),
        Some(_) => Err(ClipError::new(
            "TG-CLIP-WRONG-TYPE",
            format!("{field} is not an object"),
        )),
        None => Err(ClipError::new(
            "TG-CLIP-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

fn required_array<'a>(
    parent: &'a serde_json::Map<String, Value>,
    field: &str,
) -> ClipResult<&'a Vec<Value>> {
    match parent.get(field) {
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(ClipError::new(
            "TG-CLIP-WRONG-TYPE",
            format!("{field} is not an array"),
        )),
        None => Err(ClipError::new(
            "TG-CLIP-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

fn required_string(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    max_bytes: usize,
) -> ClipResult<String> {
    match parent.get(field) {
        Some(Value::String(text)) => {
            if text.is_empty() {
                Err(ClipError::new(
                    "TG-CLIP-MISSING-FIELD",
                    format!("{field} is empty"),
                ))
            } else if text.len() > max_bytes {
                Err(ClipError::new(
                    "TG-CLIP-FIELD-TOO-LONG",
                    format!(
                        "{field} is {} bytes, over the {max_bytes} limit",
                        text.len()
                    ),
                ))
            } else {
                Ok(text.clone())
            }
        }
        Some(_) => Err(ClipError::new(
            "TG-CLIP-WRONG-TYPE",
            format!("{field} is not a string"),
        )),
        None => Err(ClipError::new(
            "TG-CLIP-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

fn required_positive_u64(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    context: &str,
) -> ClipResult<u64> {
    match parent.get(field) {
        Some(value) => match value.as_u64() {
            Some(number) if number > 0 => Ok(number),
            _ => Err(ClipError::new(
                "TG-CLIP-RATE",
                format!("{context}.{field} is not a positive integer"),
            )),
        },
        None => Err(ClipError::new(
            "TG-CLIP-MISSING-FIELD",
            format!("required field {context}.{field} is missing"),
        )),
    }
}

/// Reads an integer frame. A float frame — even an integral
/// one — is the typed refusal the format exists to make
/// (DESIGN.md section 2.2: time is integer frames).
fn required_frame(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    context: &str,
) -> ClipResult<i64> {
    match parent.get(field) {
        Some(value) => {
            if let Some(frame) = value.as_i64() {
                Ok(frame)
            } else if let Some(frame) = value.as_u64() {
                i64::try_from(frame).map_err(|_| {
                    ClipError::new(
                        "TG-CLIP-FRAME-OUT-OF-RANGE",
                        format!("{context}.{field} exceeds the integer frame range"),
                    )
                })
            } else if value.is_f64() {
                Err(ClipError::new(
                    "TG-CLIP-FLOAT-FRAME",
                    format!("{context}.{field} is a float; frames are integers"),
                ))
            } else {
                Err(ClipError::new(
                    "TG-CLIP-WRONG-TYPE",
                    format!("{context}.{field} is not an integer frame"),
                ))
            }
        }
        None => Err(ClipError::new(
            "TG-CLIP-MISSING-FIELD",
            format!("required field {context}.{field} is missing"),
        )),
    }
}

/// The shape a value (or tangent) must have for a property:
/// its component count, or 0 for a scalar.
fn value_components(property: &str) -> usize {
    match property {
        "translation" | "scale" => 3,
        "rotation" => 4,
        _ => 0,
    }
}

/// Checks one value or tangent against its property's shape:
/// finite components, and a rotation is never the zero
/// quaternion (evaluation normalises; zero has no normal).
fn validate_value(value: &Value, property: &str, context: &str) -> ClipResult<()> {
    let components = value_components(property);
    if components == 0 {
        return match value.as_f64() {
            Some(number) if number.is_finite() => Ok(()),
            Some(_) => Err(ClipError::new(
                "TG-CLIP-NONFINITE",
                format!("{context} is not finite"),
            )),
            None => Err(ClipError::new(
                "TG-CLIP-VALUE-SHAPE",
                format!("{context} is not a scalar number"),
            )),
        };
    }
    let items = value.as_array().ok_or_else(|| {
        ClipError::new(
            "TG-CLIP-VALUE-SHAPE",
            format!("{context} is not an array of {components} numbers"),
        )
    })?;
    if items.len() != components {
        return Err(ClipError::new(
            "TG-CLIP-VALUE-SHAPE",
            format!(
                "{context} has {} components, expected {components}",
                items.len()
            ),
        ));
    }
    let mut norm_sq = 0.0f64;
    for item in items {
        match item.as_f64() {
            Some(number) if number.is_finite() => norm_sq += number * number,
            Some(_) => {
                return Err(ClipError::new(
                    "TG-CLIP-NONFINITE",
                    format!("{context} carries a non-finite component"),
                ));
            }
            None => {
                return Err(ClipError::new(
                    "TG-CLIP-VALUE-SHAPE",
                    format!("{context} carries a non-number component"),
                ));
            }
        }
    }
    if property == "rotation" && norm_sq == 0.0 {
        return Err(ClipError::new(
            "TG-CLIP-ZERO-QUATERNION",
            format!("{context} is the zero quaternion"),
        ));
    }
    Ok(())
}

fn validate_channel(
    channel: &Value,
    index: usize,
    range_start: i64,
    range_end: i64,
    frozen: bool,
    parameters: &mut Vec<String>,
    seen_targets: &mut Vec<(String, String, String)>,
) -> ClipResult<usize> {
    let context = format!("channel {index}");
    let channel = channel.as_object().ok_or_else(|| {
        ClipError::new("TG-CLIP-WRONG-TYPE", format!("{context} is not an object"))
    })?;
    let target = required_object(channel, "target").map_err(|error| prefix(error, &context))?;
    let kind = required_string(target, "kind", 16).map_err(|error| prefix(error, &context))?;
    let target_name = required_string(target, "name", CLIP_NAME_BYTES_MAX)
        .map_err(|error| prefix(error, &context))?;
    let property =
        required_string(target, "property", 16).map_err(|error| prefix(error, &context))?;
    let property_ok = match kind.as_str() {
        "joint" => matches!(property.as_str(), "translation" | "rotation" | "scale"),
        "morph" => property == "weight",
        "parameter" => property == "value",
        _ => {
            return Err(ClipError::new(
                "TG-CLIP-TARGET",
                format!("{context} target kind {kind:?} is not joint, morph, or parameter"),
            ));
        }
    };
    if !property_ok {
        return Err(ClipError::new(
            "TG-CLIP-TARGET",
            format!("{context} property {property:?} does not belong to kind {kind:?}"),
        ));
    }
    let identity = (kind.clone(), target_name.clone(), property.clone());
    if seen_targets.contains(&identity) {
        return Err(ClipError::new(
            "TG-CLIP-DUP-CHANNEL",
            format!("{context} duplicates the channel for {kind} {target_name} {property}"),
        ));
    }
    seen_targets.push(identity);
    if kind == "parameter" {
        parameters.push(target_name.clone());
    }

    let keys = required_array(channel, "keys").map_err(|error| prefix(error, &context))?;
    if keys.is_empty() {
        return Err(ClipError::new(
            "TG-CLIP-KEYS-EMPTY",
            format!("{context} carries no keys"),
        ));
    }
    if keys.len() > CLIP_KEYS_PER_CHANNEL_MAX {
        return Err(ClipError::new(
            "TG-CLIP-TOO-MANY-KEYS",
            format!(
                "{context} carries {} keys, over the {CLIP_KEYS_PER_CHANNEL_MAX} limit",
                keys.len()
            ),
        ));
    }

    // Segment modes first: tangent obligations are defined by
    // the mode of the segment a key opens.
    let mut interps: Vec<&str> = Vec::with_capacity(keys.len());
    for (key_index, key) in keys.iter().enumerate() {
        let key = key.as_object().ok_or_else(|| {
            ClipError::new(
                "TG-CLIP-WRONG-TYPE",
                format!("{context} key {key_index} is not an object"),
            )
        })?;
        match key.get("interp").and_then(Value::as_str) {
            Some(mode @ ("step" | "linear" | "cubic")) => interps.push(mode),
            Some(other) => {
                return Err(ClipError::new(
                    "TG-CLIP-INTERP",
                    format!(
                        "{context} key {key_index} interp {other:?} is not step, linear, or cubic"
                    ),
                ));
            }
            None => {
                return Err(ClipError::new(
                    "TG-CLIP-INTERP",
                    format!("{context} key {key_index} carries no interp mode"),
                ));
            }
        }
    }
    if interps[keys.len() - 1] != "step" {
        return Err(ClipError::new(
            "TG-CLIP-INTERP",
            format!(
                "{context} final key opens no segment; its interp must be step, not {:?}",
                interps[keys.len() - 1]
            ),
        ));
    }

    let mut previous_frame: Option<i64> = None;
    for (key_index, key) in keys.iter().enumerate() {
        let key_context = format!("{context} key {key_index}");
        let key = key.as_object().expect("keys were checked as objects above");
        let frame = required_frame(key, "frame", &key_context)?;
        if frame < range_start || frame > range_end {
            return Err(ClipError::new(
                "TG-CLIP-FRAME-OUT-OF-RANGE",
                format!("{key_context} frame {frame} is outside {range_start}..={range_end}"),
            ));
        }
        if let Some(previous) = previous_frame {
            if frame <= previous {
                return Err(ClipError::new(
                    "TG-CLIP-FRAME-ORDER",
                    format!("{key_context} frame {frame} does not strictly follow {previous}"),
                ));
            }
        }
        previous_frame = Some(frame);

        let value = key.get("value").ok_or_else(|| {
            ClipError::new(
                "TG-CLIP-MISSING-FIELD",
                format!("{key_context} carries no value"),
            )
        })?;
        validate_value(value, &property, &key_context)?;

        // Tangent obligations: a key's out-tangent belongs to
        // the segment it opens; its in-tangent to the segment
        // the previous key opened. Presence anywhere else is
        // stray data in a canonical document.
        let needs_out = interps[key_index] == "cubic";
        let needs_in = key_index > 0 && interps[key_index - 1] == "cubic";
        for (field, needed) in [("in_tangent", needs_in), ("out_tangent", needs_out)] {
            match (key.get(field), needed) {
                (Some(tangent), true) => {
                    validate_value(tangent, &property, &format!("{key_context} {field}"))?;
                }
                (Some(_), false) => {
                    return Err(ClipError::new(
                        "TG-CLIP-TANGENT-STRAY",
                        format!("{key_context} carries a {field} no cubic segment uses"),
                    ));
                }
                (None, true) => {
                    return Err(ClipError::new(
                        "TG-CLIP-TANGENT-MISSING",
                        format!("{key_context} is missing its {field} for a cubic segment"),
                    ));
                }
                (None, false) => {}
            }
        }

        if let Some(mode) = key.get("tangent_mode") {
            let mode = mode.as_str().ok_or_else(|| {
                ClipError::new(
                    "TG-CLIP-WRONG-TYPE",
                    format!("{key_context} tangent_mode is not a string"),
                )
            })?;
            if !TANGENT_MODES.contains(&mode) {
                return Err(ClipError::new(
                    "TG-CLIP-TANGENT-MODE",
                    format!("{key_context} tangent_mode {mode:?} is not a known mode"),
                ));
            }
            if frozen && mode != "explicit" {
                return Err(ClipError::new(
                    "TG-CLIP-UNRESOLVED-TANGENT",
                    format!(
                        "{key_context} carries edit-time tangent mode {mode:?} in a frozen document; the freeze resolves every mode to explicit tangents"
                    ),
                ));
            }
        }
        if let Some(key_type) = key.get("key_type") {
            match key_type.as_str() {
                Some(kind) if KEY_TYPES.contains(&kind) => {}
                _ => {
                    return Err(ClipError::new(
                        "TG-CLIP-KEY-TYPE",
                        format!(
                            "{key_context} key_type is not keyframe, breakdown, moving_hold, or extreme"
                        ),
                    ));
                }
            }
        }
        if let Some(easing) = key.get("easing") {
            // An easing preset survives only as provenance
            // (DESIGN.md section 3.4): a tag, never a behaviour.
            if easing.as_str().is_none() {
                return Err(ClipError::new(
                    "TG-CLIP-WRONG-TYPE",
                    format!("{key_context} easing provenance is not a string"),
                ));
            }
        }
        if let Some(pose_ref) = key.get("pose_ref") {
            let pose_ref = pose_ref.as_object().ok_or_else(|| {
                ClipError::new(
                    "TG-CLIP-WRONG-TYPE",
                    format!("{key_context} pose_ref is not an object"),
                )
            })?;
            required_string(pose_ref, "pose", CLIP_NAME_BYTES_MAX)
                .map_err(|error| prefix(error, &key_context))?;
            match pose_ref.get("blend").and_then(Value::as_f64) {
                Some(blend) if blend.is_finite() && (0.0..=1.0).contains(&blend) => {}
                _ => {
                    return Err(ClipError::new(
                        "TG-CLIP-POSE-REF",
                        format!("{key_context} pose_ref.blend is not a number in 0..=1"),
                    ));
                }
            }
        }
    }
    Ok(keys.len())
}

/// The WGSL identifier a contract name is declared under:
/// `param:Desire` is written `param_Desire`; every other
/// contract name is already an identifier.
fn wgsl_identifier(contract_name: &str) -> String {
    contract_name.replace(':', "_")
}

/// Scans WGSL source for `var<uniform>` declarations and
/// returns the declared identifiers, in source order.
fn wgsl_uniform_identifiers(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = source;
    while let Some(position) = rest.find("var<uniform>") {
        let after = &rest[position + "var<uniform>".len()..];
        let name: String = after
            .trim_start()
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect();
        if !name.is_empty() {
            found.push(name);
        }
        rest = after;
    }
    found
}

fn validate_shader_module(
    module: &Value,
    index: usize,
    parameters: &[String],
) -> ClipResult<String> {
    let context = format!("shader module {index}");
    let module = module.as_object().ok_or_else(|| {
        ClipError::new("TG-CLIP-WRONG-TYPE", format!("{context} is not an object"))
    })?;
    let name = required_string(module, "name", CLIP_NAME_BYTES_MAX)
        .map_err(|error| prefix(error, &context))?;
    let hook = required_string(module, "hook", 32).map_err(|error| prefix(error, &context))?;
    if !SHADER_HOOKS.contains(&hook.as_str()) {
        return Err(ClipError::new(
            "TG-CLIP-HOOK",
            format!("{context} hook {hook:?} is not one of the closed stage hooks"),
        ));
    }
    let source = required_string(module, "source", CLIP_SHADER_SOURCE_BYTES_MAX)
        .map_err(|error| prefix(error, &context))?;

    let uniforms = required_array(module, "uniforms").map_err(|error| prefix(error, &context))?;
    if uniforms.len() > CLIP_UNIFORMS_PER_MODULE_MAX {
        return Err(ClipError::new(
            "TG-CLIP-TOO-MANY-UNIFORMS",
            format!(
                "{context} declares {} uniforms, over the {CLIP_UNIFORMS_PER_MODULE_MAX} limit",
                uniforms.len()
            ),
        ));
    }
    let mut declared: Vec<String> = Vec::new();
    for (uniform_index, uniform) in uniforms.iter().enumerate() {
        let uniform_context = format!("{context} uniform {uniform_index}");
        let uniform = uniform.as_object().ok_or_else(|| {
            ClipError::new(
                "TG-CLIP-WRONG-TYPE",
                format!("{uniform_context} is not an object"),
            )
        })?;
        let uniform_name = required_string(uniform, "name", CLIP_NAME_BYTES_MAX)
            .map_err(|error| prefix(error, &uniform_context))?;
        let uniform_type = required_string(uniform, "type", 16)
            .map_err(|error| prefix(error, &uniform_context))?;
        // The wall-clock lint runs before the allowlist
        // (DESIGN.md section 7.3): a time-shaped input under
        // any renaming is a refusal. Parameter inputs are clip
        // data by construction and are not linted.
        let lowered = uniform_name.to_ascii_lowercase();
        if !lowered.starts_with("param:")
            && (lowered.contains("time")
                || lowered.contains("clock")
                || lowered.contains("elapsed")
                || lowered.contains("wall"))
        {
            return Err(ClipError::new(
                "TG-CLIP-WALLCLOCK-UNIFORM",
                format!(
                    "{uniform_context} names a wall-clock-shaped input {uniform_name:?}; the only time a module may read is the integer tick"
                ),
            ));
        }
        let allowed = match uniform_name.as_str() {
            "tick" => {
                if uniform_type != "u32" {
                    return Err(ClipError::new(
                        "TG-CLIP-TICK-TYPE",
                        format!(
                            "{uniform_context} declares tick as {uniform_type:?}; the tick is a u32"
                        ),
                    ));
                }
                true
            }
            "joint_matrices" => {
                matches!(hook.as_str(), "skinning_assist" | "post_skin_displacement")
                    && uniform_type == "mat4[]"
            }
            "morph_weights" => {
                matches!(hook.as_str(), "morph_assist" | "post_skin_displacement")
                    && uniform_type == "f32[]"
            }
            _ => {
                if let Some(parameter) = uniform_name.strip_prefix("param:") {
                    parameters.iter().any(|known| known == parameter) && uniform_type == "f32"
                } else {
                    false
                }
            }
        };
        if !allowed {
            return Err(ClipError::new(
                "TG-CLIP-UNDECLARED-UNIFORM",
                format!(
                    "{uniform_context} input {uniform_name:?} ({uniform_type}) is outside the uniform contract for hook {hook:?}"
                ),
            ));
        }
        declared.push(wgsl_identifier(&uniform_name));
    }

    // Every uniform the WGSL actually declares must be in the
    // module's contract list.
    for identifier in wgsl_uniform_identifiers(&source) {
        if !declared.contains(&identifier) {
            return Err(ClipError::new(
                "TG-CLIP-UNDECLARED-UNIFORM",
                format!(
                    "{context} WGSL declares uniform {identifier:?} that its contract does not"
                ),
            ));
        }
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_working() -> Value {
        serde_json::json!({
            "format": MOTION_CLIP_FORMAT,
            "name": "minimal",
            "rig": "a".repeat(64),
            "rate": {"num": 24, "den": 1},
            "range": {"start": 0, "end": 24},
            "loop": null,
            "channels": [{
                "target": {"kind": "joint", "name": "Hips", "property": "translation"},
                "keys": [
                    {"frame": 0, "value": [0.0, 1.0, 0.0], "interp": "step"},
                    {"frame": 24, "value": [0.0, 1.0, 0.0], "interp": "step"}
                ]
            }],
            "op_log": [{"seq": 0, "actor": "converter", "session": "test", "op": "author"}]
        })
    }

    #[test]
    fn freeze_hash_of_the_empty_canonical_form_is_the_known_constant() {
        let doc = serde_json::json!({"freeze": {"sha256": "x"}});
        // Canonical form of a freeze-only document is `{}`, and
        // SHA-256 of "{}" is a published constant.
        assert_eq!(canonical_bytes(&doc), "{}");
        assert_eq!(
            freeze_hash(&doc),
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
    }

    #[test]
    fn freeze_then_validate_round_trips() {
        let frozen = freeze_document(&minimal_working(), "test-compiler 0", "tier2-1e-6")
            .expect("the minimal document freezes");
        let report = validate_document(&frozen).expect("the frozen document validates");
        assert_eq!(report.keys, 2);
        assert!(report.freeze_sha256.is_some());
        // Canonicalisation is a fixed point.
        let text = to_d_json(&frozen);
        let reparsed: Value = serde_json::from_str(&text).expect("canonical text parses");
        assert_eq!(to_d_json(&reparsed), text);
    }

    #[test]
    fn freeze_refuses_an_automatic_tangent_mode() {
        let mut doc = minimal_working();
        doc["channels"][0]["keys"][0]["tangent_mode"] = Value::String("auto".to_string());
        // Working validation tolerates the edit-time mode...
        validate_document(&doc).expect("working document validates");
        // ...the freeze does not.
        let refused = freeze_document(&doc, "test-compiler 0", "tier2-1e-6");
        assert_eq!(
            refused.expect_err("auto must refuse").code(),
            "TG-CLIP-UNRESOLVED-TANGENT"
        );
    }

    #[test]
    fn wgsl_scan_finds_uniform_declarations() {
        let source =
            "var<uniform> tick: u32;\nvar<uniform>  param_Desire: f32;\nvar<storage> other: u32;";
        assert_eq!(
            wgsl_uniform_identifiers(source),
            vec!["tick".to_string(), "param_Desire".to_string()]
        );
    }
}
