//! The vendor extension document (states.d): the
//! `io.ntranch.torogao` EXT section and everything that
//! reads it — rating verdict (single-sourced), state and
//! variant recipes, consumer data, and the validation
//! layer over all of it.

use crate::controllers::validate_controllers;
use crate::criteria::Finding;
use crate::packs::{load_pack, pack_str};
use crate::util::{fmt_4f, fmt_g, is_num, jget, jstr, num_of};
use crate::walk::Walk;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use torogao_engine::Puppet;
use torogao_format::container::TOROGAO_EXT_SECTION;

/// Extension document format identifier.
pub const EXT_FORMAT: &str = "torogao-ext/1";
/// TG-BLEND-UNKNOWN-PARAM.
pub const TG_BLEND_UNKNOWN_PARAM: &str = "TG-BLEND-UNKNOWN-PARAM";
/// TG-BLEND-UNKNOWN-PART.
pub const TG_BLEND_UNKNOWN_PART: &str = "TG-BLEND-UNKNOWN-PART";
/// TG-CONTAINER-TRAILING.
pub const TG_CONTAINER_TRAILING: &str = "TG-CONTAINER-TRAILING";
/// TG-EVENT-BEYOND-DURATION.
pub const TG_EVENT_BEYOND_DURATION: &str = "TG-EVENT-BEYOND-DURATION";
/// TG-EVENT-SHAPE.
pub const TG_EVENT_SHAPE: &str = "TG-EVENT-SHAPE";
/// TG-EVENT-UNKNOWN-MOTION.
pub const TG_EVENT_UNKNOWN_MOTION: &str = "TG-EVENT-UNKNOWN-MOTION";
/// TG-EXT-DUPLICATE.
pub const TG_EXT_DUPLICATE: &str = "TG-EXT-DUPLICATE";
/// TG-EXPR-MISSING.
pub const TG_EXPR_MISSING: &str = "TG-EXPR-MISSING";
/// TG-EXT-MALFORMED.
pub const TG_EXT_MALFORMED: &str = "TG-EXT-MALFORMED";
/// TG-EXT-TYPE.
pub const TG_EXT_TYPE: &str = "TG-EXT-TYPE";
/// TG-GLUE-UNMAPPED.
pub const TG_GLUE_UNMAPPED: &str = "TG-GLUE-UNMAPPED";
/// TG-HITAREA-SHAPE.
pub const TG_HITAREA_SHAPE: &str = "TG-HITAREA-SHAPE";
/// TG-HITAREA-UNKNOWN-PART.
pub const TG_HITAREA_UNKNOWN_PART: &str = "TG-HITAREA-UNKNOWN-PART";
/// TG-RATING-LANE-MISMATCH.
pub const TG_RATING_LANE_MISMATCH: &str = "TG-RATING-LANE-MISMATCH";
/// TG-RATING-STAMP-MISSING.
pub const TG_RATING_STAMP_MISSING: &str = "TG-RATING-STAMP-MISSING";
/// TG-RATING-UNKNOWN.
pub const TG_RATING_UNKNOWN: &str = "TG-RATING-UNKNOWN";
/// TG-STATE-DUPLICATE.
pub const TG_STATE_DUPLICATE: &str = "TG-STATE-DUPLICATE";
/// TG-STATE-UNKNOWN-PARAM.
pub const TG_STATE_UNKNOWN_PARAM: &str = "TG-STATE-UNKNOWN-PARAM";
/// TG-TRACK-MISSING.
pub const TG_TRACK_MISSING: &str = "TG-TRACK-MISSING";
/// TG-TRACK-RANGE.
pub const TG_TRACK_RANGE: &str = "TG-TRACK-RANGE";
/// TG-VARIANT-UNKNOWN-PARAM.
pub const TG_VARIANT_UNKNOWN_PARAM: &str = "TG-VARIANT-UNKNOWN-PARAM";
/// TG-VARIANT-UNKNOWN-PART.
pub const TG_VARIANT_UNKNOWN_PART: &str = "TG-VARIANT-UNKNOWN-PART";

/// How the extension presents for one project.
#[derive(Debug, Clone)]
pub struct ExtView {
    /// The parsed document (null when absent/malformed).
    pub doc: Value,
    /// Whether a document object is present.
    pub present: bool,
    /// Whether a non-empty section failed to parse.
    pub malformed: bool,
    /// Whether a section exists but is not an object.
    pub wrong_type: bool,
}

impl ExtView {
    /// Builds the view from a loaded project.
    pub fn of_project(loaded: &crate::guards::LoadedProject) -> ExtView {
        let section = loaded.puppet.ext_section(TOROGAO_EXT_SECTION);
        ExtView::of_section(section)
    }

    /// Builds the view from raw section bytes (absent slot
    /// = no section at all).
    pub fn of_section(section: Option<&[u8]>) -> ExtView {
        let absent = ExtView {
            doc: Value::Null,
            present: false,
            malformed: false,
            wrong_type: false,
        };
        let Some(bytes) = section else {
            return absent;
        };
        if bytes.is_empty() {
            return absent;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return ExtView {
                malformed: true,
                ..absent
            };
        };
        let Ok(doc) = serde_json::from_str::<Value>(text) else {
            return ExtView {
                malformed: true,
                ..absent
            };
        };
        if !doc.is_object() {
            return ExtView {
                doc,
                present: false,
                malformed: false,
                wrong_type: true,
            };
        }
        ExtView {
            present: true,
            malformed: false,
            wrong_type: false,
            doc,
        }
    }
}

/// Counts "glue" keys recursively through objects and
/// arrays (the INP1 model has no glue construct).
pub fn count_glue_keys(value: &Value) -> usize {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, child)| (key == "glue") as usize + count_glue_keys(child))
            .sum(),
        Value::Array(items) => items.iter().map(count_glue_keys).sum(),
        _ => 0,
    }
}

/// Parameter name -> [min.x, max.x] in puppet order.
pub fn param_ranges(puppet: &Puppet) -> Vec<(String, [f64; 2])> {
    puppet
        .params
        .iter()
        .map(|param| {
            (
                param.name.clone(),
                [param.min.x as f64, param.max.x as f64],
            )
        })
        .collect()
}

/// All node names of the puppet (walk names).
pub fn node_names(puppet: &Puppet, walk: &Walk) -> HashSet<String> {
    walk.nodes
        .iter()
        .map(|w| puppet.nodes.nodes[w.node].name.clone())
        .collect()
}

/// Blend shape name -> the parameter it folds into
/// (entries count only when both fields are strings).
pub fn blend_param_by_name(ext: &Value) -> HashMap<String, String> {
    let mut blends = HashMap::new();
    if let Value::Array(items) = jget(ext, "blendshapes") {
        for item in items {
            if let (Value::String(name), Value::String(parameter)) =
                (jget(item, "name"), jget(item, "parameter"))
            {
                blends.insert(name.clone(), parameter.clone());
            }
        }
    }
    blends
}

/// Resolves a recipe (blend-shape names fold to their
/// parameters) against the project's parameter ranges.
/// Returns the resolved parameter -> value list (later
/// keys overwrite) and any problems.
pub fn resolve_recipe(
    ext: &Value,
    recipe: &Value,
    ranges: &[(String, [f64; 2])],
) -> (Vec<(String, f64)>, Vec<String>) {
    let mut out: Vec<(String, f64)> = Vec::new();
    let mut problems = Vec::new();
    if !recipe.is_object() {
        problems.push("recipe is not an object".to_string());
        return (out, problems);
    }
    let blends = blend_param_by_name(ext);
    let range_names: HashSet<&str> = ranges.iter().map(|(name, _)| name.as_str()).collect();
    if let Value::Object(entries) = recipe {
        for (key, value) in entries {
            let param = blends.get(key).map_or(key.as_str(), String::as_str);
            if !range_names.contains(param) {
                problems.push(format!(
                    "recipe key '{key}' is neither a parameter nor a blend shape"
                ));
            } else if !is_num(value) {
                problems.push(format!("recipe value for '{key}' is not a number"));
            } else {
                let number = num_of(value);
                if let Some(slot) = out.iter_mut().find(|(name, _)| name == param) {
                    slot.1 = number;
                } else {
                    out.push((param.to_string(), number));
                }
            }
        }
    }
    (out, problems)
}

/// The rating stamp readers report: the meta fields when
/// a stamp object exists, else the unrated default.
pub fn rating_stamp(ext: &Value) -> Value {
    let meta = jget(ext, "meta");
    if !meta.is_object() {
        return serde_json::json!({
            "basis": "no torogao rating metadata in the project",
            "lane": "none",
            "pack": "",
            "rating": "unrated",
            "stamped": true,
        });
    }
    serde_json::json!({
        "basis": jget(meta, "basis").clone(),
        "lane": jget(meta, "lane").clone(),
        "pack": jget(meta, "pack").clone(),
        "rating": jget(meta, "rating").clone(),
        "stamped": true,
    })
}

/// The outcome of the single rating verdict.
#[derive(Debug, Clone)]
pub struct RatingVerdict {
    /// Whether the stamp is complete (or there is no ext).
    pub complete: bool,
    /// Whether the verdict blocks export.
    pub blocked: bool,
    /// Whether the project carries extension data.
    pub has_ext: bool,
    /// Finding code when incomplete or blocked.
    pub code: &'static str,
    /// The verdict's reason (empty when ok).
    pub reason: String,
    /// The stamp the verdict reports.
    pub stamp: Value,
}

/// THE rating verdict — the one function every surface
/// (validate, rating-check, the export gate) reads, so
/// the surfaces cannot drift.
pub fn rating_verdict(ext: &ExtView) -> RatingVerdict {
    let stamp = rating_stamp(&ext.doc);
    if !ext.doc.is_object() {
        return RatingVerdict {
            complete: true,
            blocked: false,
            has_ext: false,
            code: "",
            reason: String::new(),
            stamp,
        };
    }
    let incomplete = |code: &'static str, reason: String| RatingVerdict {
        complete: false,
        blocked: false,
        has_ext: true,
        code,
        reason,
        stamp: stamp.clone(),
    };
    let blocked = |code: &'static str, reason: String| RatingVerdict {
        complete: true,
        blocked: true,
        has_ext: true,
        code,
        reason,
        stamp: stamp.clone(),
    };
    let meta = jget(&ext.doc, "meta");
    if !meta.is_object() {
        return incomplete(
            TG_RATING_STAMP_MISSING,
            "project carries torogao data but no rating meta; a stamped project must carry pack, lane, rating, and basis"
                .to_string(),
        );
    }
    for field in ["pack", "lane", "rating", "basis"] {
        let value = jget(meta, field);
        let ok = matches!(value, Value::String(text) if !text.is_empty());
        if !ok {
            return incomplete(
                TG_RATING_STAMP_MISSING,
                format!(
                    "rating stamp field '{field}' is missing, not a string, or empty"
                ),
            );
        }
    }
    let pack_name = jstr(meta, "pack");
    let lane = jstr(meta, "lane");
    let rating = jstr(meta, "rating");
    let pack = match load_pack(pack_name) {
        Ok(pack) => pack,
        Err(_) => {
            return blocked(
                TG_RATING_UNKNOWN,
                format!(
                    "rating meta names pack '{pack_name}', which is not a shipped lane pack"
                ),
            );
        }
    };
    let pack_lane = jstr(&pack, "lane");
    if !pack_lane.is_empty() && lane != pack_lane {
        return blocked(
            TG_RATING_LANE_MISMATCH,
            format!(
                "stamp lane '{lane}' does not match pack '{pack_name}', whose lane is '{pack_lane}'"
            ),
        );
    }
    if !crate::packs::pack_rating_known(&pack, rating) {
        return blocked(
            TG_RATING_UNKNOWN,
            format!("rating '{rating}' is not a rating value of pack '{pack_name}'"),
        );
    }
    if !crate::packs::pack_rating_allowed(&pack, lane, rating) {
        return blocked(
            TG_RATING_LANE_MISMATCH,
            format!(
                "rating '{rating}' is not permitted in lane '{lane}' by pack '{pack_name}'"
            ),
        );
    }
    RatingVerdict {
        complete: true,
        blocked: false,
        has_ext: true,
        code: "",
        reason: String::new(),
        stamp,
    }
}

/// Whether the extension's rating state blocks export
/// (an incomplete stamp does NOT block).
pub fn rating_blocks_export(ext: &ExtView) -> Option<(&'static str, String)> {
    let verdict = rating_verdict(ext);
    if verdict.blocked {
        Some((verdict.code, verdict.reason))
    } else {
        None
    }
}

/// Finds one state by name (null when absent).
pub fn find_state(ext: &Value, name: &str) -> Value {
    if let Value::Array(states) = jget(ext, "states") {
        for state in states {
            if pack_str(state, "name") == name {
                return state.clone();
            }
        }
    }
    Value::Null
}

/// How many states carry this name.
pub fn count_states_named(ext: &Value, name: &str) -> usize {
    match jget(ext, "states") {
        Value::Array(states) => states
            .iter()
            .filter(|state| pack_str(state, "name") == name)
            .count(),
        _ => 0,
    }
}

/// Finds one variant by id (null when absent).
pub fn find_variant(ext: &Value, id: &str) -> Value {
    if let Value::Array(variants) = jget(ext, "variants") {
        for variant in variants {
            if pack_str(variant, "id") == id {
                return variant.clone();
            }
        }
    }
    Value::Null
}

/// How many variants carry this id.
pub fn count_variants_named(ext: &Value, id: &str) -> usize {
    match jget(ext, "variants") {
        Value::Array(variants) => variants
            .iter()
            .filter(|variant| pack_str(variant, "id") == id)
            .count(),
        _ => 0,
    }
}

/// A compact summary of the extension for inspect/list
/// tools.
pub fn ext_summary(ext: &Value) -> Value {
    if !ext.is_object() {
        return serde_json::json!({"present": false});
    }
    let names_of = |value: &Value, key: &str| -> Vec<Value> {
        match value {
            Value::Array(items) => items
                .iter()
                .map(|item| Value::String(pack_str(item, key)))
                .collect(),
            _ => Vec::new(),
        }
    };
    let character = jget(ext, "character");
    let mut doc = serde_json::json!({
        "blendshapes": names_of(jget(ext, "blendshapes"), "name"),
        "controllers": names_of(jget(ext, "controllers"), "id"),
        "format": jget(ext, "format").clone(),
        "hitAreas": names_of(jget(jget(ext, "consumer"), "hitAreas"), "name"),
        "meta": jget(ext, "meta").clone(),
        "present": true,
        "states": names_of(jget(ext, "states"), "name"),
        "trackingClaims": jget(ext, "trackingClaims").clone(),
        "variants": names_of(jget(ext, "variants"), "id"),
    });
    if character.is_object() {
        doc["character"] = Value::String(pack_str(character, "name"));
    }
    doc
}

/// Expression coverage: the governed names of `pack`
/// that the project's states do NOT implement.
pub fn expression_missing(ext: &Value, pack: &Value) -> Vec<String> {
    let mut have: HashSet<String> = HashSet::new();
    if let Value::Array(states) = jget(ext, "states") {
        for state in states {
            have.insert(pack_str(state, "name"));
        }
    }
    let mut missing = Vec::new();
    for expression in crate::packs::pack_expressions(pack) {
        let name = pack_str(&expression, "name");
        if !have.contains(&name) {
            missing.push(name);
        }
    }
    missing
}

/// Tracking coverage for one set of a pack against a
/// puppet: (missing ids, range violations).
pub fn tracking_coverage(
    puppet: &Puppet,
    set: &Value,
) -> (Vec<String>, Vec<String>) {
    let ranges = param_ranges(puppet);
    let range_map: HashMap<&str, [f64; 2]> = ranges
        .iter()
        .map(|(name, range)| (name.as_str(), *range))
        .collect();
    let mut missing = Vec::new();
    let mut range_bad = Vec::new();
    if let Value::Array(ids) = jget(set, "ids") {
        for entry in ids {
            let id = pack_str(entry, "id");
            let Some(range) = range_map.get(id.as_str()) else {
                missing.push(id);
                continue;
            };
            let set_min = num_of(jget(entry, "min"));
            let set_max = num_of(jget(entry, "max"));
            if range[0] > set_min + 1e-9 || range[1] < set_max - 1e-9 {
                range_bad.push(format!(
                    "{} (rig {}..{}, set {}..{})",
                    id,
                    fmt_g(range[0]),
                    fmt_g(range[1]),
                    fmt_g(set_min),
                    fmt_g(set_max)
                ));
            }
        }
    }
    (missing, range_bad)
}

/// The puppet's parameter names, in order.
pub fn puppet_param_names(puppet: &Puppet) -> Vec<String> {
    puppet
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect()
}

/// The full extension validation: rating stamp, states,
/// variants, blend shapes, hit areas, glue visibility.
pub fn validate_ext(puppet: &Puppet, walk: &Walk, payload: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    let view = ExtView::of_section(puppet.ext_section(TOROGAO_EXT_SECTION));

    // Glue visibility applies to every project, with or
    // without Torogao data: never a silent drop.
    let mut glue = count_glue_keys(payload);
    if view.present {
        glue += count_glue_keys(jget(&view.doc, "foreign"));
    }
    if glue > 0 {
        findings.push(Finding::new(
            TG_GLUE_UNMAPPED,
            "warning",
            "payload",
            format!(
                "project carries {glue} glue record(s); the INP1 model has no glue construct, so the coupling is reported here and not consumed — it is never silently dropped"
            ),
        ));
    }

    if !view.present {
        if view.malformed {
            findings.push(Finding::new(
                TG_EXT_MALFORMED,
                "error",
                "ext",
                "the torogao EXT section is not parseable JSON",
            ));
        } else if view.wrong_type {
            findings.push(Finding::new(
                TG_EXT_TYPE,
                "error",
                "ext",
                "the torogao EXT section is well-formed JSON but not an object; extension data must be a torogao-ext/1 object",
            ));
        }
        return findings;
    }
    let ext = &view.doc;
    let ranges = param_ranges(puppet);
    let nodes = node_names(puppet, walk);

    // -- rating stamp (the single verdict)
    let verdict = rating_verdict(&view);
    if !verdict.complete || verdict.blocked {
        findings.push(Finding::new(
            verdict.code,
            "error",
            "meta",
            verdict.reason.clone(),
        ));
    }

    // -- section shape discipline
    for section in ["states", "variants", "blendshapes"] {
        let value = jget(ext, section);
        if !value.is_null() && !value.is_array() {
            findings.push(Finding::new(
                TG_EXT_TYPE,
                "error",
                section,
                format!("extension section '{section}' must be an array"),
            ));
        }
    }
    let character = jget(ext, "character");
    if !character.is_null() && !character.is_object() {
        findings.push(Finding::new(
            TG_EXT_TYPE,
            "error",
            "character",
            "extension section 'character' must be an object",
        ));
    }
    let consumer = jget(ext, "consumer");
    if !consumer.is_null() && !consumer.is_object() {
        findings.push(Finding::new(
            TG_EXT_TYPE,
            "error",
            "consumer",
            "extension section 'consumer' must be an object",
        ));
    } else if consumer.is_object() {
        for field in ["hitAreas", "motionEvents"] {
            let value = jget(consumer, field);
            if !value.is_null() && !value.is_array() {
                findings.push(Finding::new(
                    TG_EXT_TYPE,
                    "error",
                    format!("consumer.{field}"),
                    format!("consumer field '{field}' must be an array"),
                ));
            }
        }
    }

    // -- states
    if let Value::Array(states) = jget(ext, "states") {
        let mut seen_states: HashSet<String> = HashSet::new();
        let mut dup_states: HashSet<String> = HashSet::new();
        for state in states {
            let sname = pack_str(state, "name");
            if !sname.is_empty() {
                if seen_states.contains(&sname) && dup_states.insert(sname.clone()) {
                    findings.push(Finding::new(
                        TG_STATE_DUPLICATE,
                        "error",
                        sname.clone(),
                        format!(
                            "two states carry the name '{sname}'; states are applied by name, so a duplicate makes application ambiguous"
                        ),
                    ));
                }
                seen_states.insert(sname.clone());
            }
            let (_, problems) = resolve_recipe(ext, jget(state, "recipe"), &ranges);
            for problem in problems {
                findings.push(Finding::new(
                    TG_STATE_UNKNOWN_PARAM,
                    "error",
                    if sname.is_empty() {
                        "<unnamed state>".to_string()
                    } else {
                        sname.clone()
                    },
                    problem,
                ));
            }
        }
    }

    // -- variants
    if let Value::Array(variants) = jget(ext, "variants") {
        for variant in variants {
            let vid = pack_str(variant, "id");
            let subject = if vid.is_empty() {
                "<unnamed variant>".to_string()
            } else {
                vid
            };
            let (_, problems) = resolve_recipe(ext, jget(variant, "recipe"), &ranges);
            for problem in problems {
                findings.push(Finding::new(
                    TG_VARIANT_UNKNOWN_PARAM,
                    "error",
                    subject.clone(),
                    problem,
                ));
            }
            for (field, label) in [("visibility", "visibility"), ("appearance", "appearance")] {
                let entries = jget(variant, field);
                if let Value::Object(entries) = entries {
                    for part in entries.keys() {
                        if !nodes.contains(part) {
                            findings.push(Finding::new(
                                TG_VARIANT_UNKNOWN_PART,
                                "error",
                                subject.clone(),
                                format!(
                                    "variant {label} names part '{part}', which is not a node in the project"
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    // -- blend shapes
    if let Value::Array(blends) = jget(ext, "blendshapes") {
        let range_names: HashSet<&str> = ranges.iter().map(|(name, _)| name.as_str()).collect();
        for blend in blends {
            let bname = pack_str(blend, "name");
            let subject = if bname.is_empty() {
                "<unnamed blend shape>".to_string()
            } else {
                bname
            };
            let param = pack_str(blend, "parameter");
            if !range_names.contains(param.as_str()) {
                findings.push(Finding::new(
                    TG_BLEND_UNKNOWN_PARAM,
                    "error",
                    subject.clone(),
                    format!(
                        "blend shape folds into parameter '{param}', which the project does not have"
                    ),
                ));
            }
            if let Value::Array(deltas) = jget(blend, "deltas") {
                for delta in deltas {
                    let part = pack_str(delta, "part");
                    if !part.is_empty() && !nodes.contains(&part) {
                        findings.push(Finding::new(
                            TG_BLEND_UNKNOWN_PART,
                            "error",
                            subject.clone(),
                            format!(
                                "blend shape delta names part '{part}', which is not a node in the project"
                            ),
                        ));
                    }
                }
            }
        }
    }

    // -- consumer layer + performance controllers
    findings.extend(validate_hit_areas_only(ext, puppet, walk));
    findings.extend(validate_motion_events_only(ext, puppet));
    findings.extend(validate_controllers(ext, &puppet_param_names(puppet)));

    findings
}

/// Validates only the consumer layer's motion event
/// tracks of an extension document against a puppet.
pub fn validate_motion_events_only(ext: &Value, puppet: &Puppet) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Value::Array(events) = jget(jget(ext, "consumer"), "motionEvents") else {
        return findings;
    };
    for event in events {
        let ename = pack_str(event, "name");
        let subject = if ename.is_empty() {
            "<unnamed event>".to_string()
        } else {
            ename.clone()
        };
        if ename.is_empty() {
            findings.push(Finding::new(
                TG_EVENT_SHAPE,
                "error",
                subject.clone(),
                "event needs a name",
            ));
        }
        let motion = pack_str(event, "motion");
        let Some((_, animation)) = puppet
            .animations
            .iter()
            .find(|(name, _)| name == &motion)
        else {
            findings.push(Finding::new(
                TG_EVENT_UNKNOWN_MOTION,
                "error",
                subject.clone(),
                format!(
                    "event names motion '{motion}', which is not an animation of the project"
                ),
            ));
            continue;
        };
        let duration = animation.length as f64 * animation.timestep as f64;
        let time_value = jget(event, "timeSeconds");
        let t = if time_value.is_number() {
            num_of(time_value)
        } else {
            -1.0
        };
        if t < 0.0 {
            findings.push(Finding::new(
                TG_EVENT_SHAPE,
                "error",
                subject.clone(),
                "event needs timeSeconds >= 0",
            ));
        } else if t > duration + 1e-9 {
            findings.push(Finding::new(
                TG_EVENT_BEYOND_DURATION,
                "error",
                subject.clone(),
                format!(
                    "event time {} s is beyond motion '{motion}' duration {} s",
                    fmt_4f(t),
                    fmt_4f(duration)
                ),
            ));
        }
        let payload_class = pack_str(event, "payloadClass");
        if !["sfx", "voice", "haptic", "script-beat"].contains(&payload_class.as_str()) {
            findings.push(Finding::new(
                TG_EVENT_SHAPE,
                "error",
                subject.clone(),
                "payloadClass must be one of sfx, voice, haptic, script-beat",
            ));
        }
        if pack_str(event, "payload").is_empty() {
            findings.push(Finding::new(
                TG_EVENT_SHAPE,
                "error",
                subject.clone(),
                "event needs a payload reference",
            ));
        }
    }
    findings
}

/// Validates only the consumer layer's hit areas of an
/// extension document against a puppet.
pub fn validate_hit_areas_only(ext: &Value, puppet: &Puppet, walk: &Walk) -> Vec<Finding> {
    let mut findings = Vec::new();
    let nodes = node_names(puppet, walk);
    let Value::Array(hit_areas) = jget(jget(ext, "consumer"), "hitAreas") else {
        return findings;
    };
    for hit_area in hit_areas {
        let hname = pack_str(hit_area, "name");
        let subject = if hname.is_empty() {
            "<unnamed hit area>".to_string()
        } else {
            hname
        };
        let part = pack_str(jget(hit_area, "binding"), "part");
        if part.is_empty() || !nodes.contains(&part) {
            findings.push(Finding::new(
                TG_HITAREA_UNKNOWN_PART,
                "error",
                subject.clone(),
                format!(
                    "hit area binds part '{part}', which is not a node in the project"
                ),
            ));
        }
        let shape = jget(hit_area, "shape");
        let kind = pack_str(shape, "kind");
        let (required, extents): (&[&str], &[&str]) = match kind.as_str() {
            "rect" => (&["x", "y", "w", "h"], &["w", "h"]),
            "circle" => (&["x", "y", "r"], &["r"]),
            _ => {
                findings.push(Finding::new(
                    TG_HITAREA_SHAPE,
                    "error",
                    subject.clone(),
                    format!(
                        "hit area shape kind '{kind}' is not 'rect' or 'circle'"
                    ),
                ));
                continue;
            }
        };
        for field in required {
            let value = jget(shape, field);
            if !is_num(value) || !num_of(value).is_finite() {
                findings.push(Finding::new(
                    TG_HITAREA_SHAPE,
                    "error",
                    subject.clone(),
                    format!(
                        "hit area shape '{kind}' field '{field}' is missing or not a finite number"
                    ),
                ));
                continue;
            }
            if extents.contains(field) && num_of(value) < 0.0 {
                findings.push(Finding::new(
                    TG_HITAREA_SHAPE,
                    "error",
                    subject.clone(),
                    format!(
                        "hit area shape '{kind}' field '{field}' must be >= 0 (got {})",
                        fmt_g(num_of(value))
                    ),
                ));
            }
        }
    }
    findings
}

/// Container-structure findings that only the raw bytes
/// can show: a duplicate Torogao EXT section and trailing
/// bytes after the final EXT entry. An unreadable or
/// unparseable file yields no findings here.
pub fn validate_container_file(path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Ok(bytes) = std::fs::read(path) else {
        return findings;
    };
    let scan = torogao_format::container::scan_container(&bytes);
    if !scan.ok {
        return findings;
    }
    if scan.torogao_sections > 1 {
        findings.push(Finding::new(
            TG_EXT_DUPLICATE,
            "error",
            "ext",
            format!(
                "container carries the torogao EXT section {} times; the section is single by contract and no reader picks a winner between duplicates",
                scan.torogao_sections
            ),
        ));
    }
    if scan.trailing_bytes > 0 {
        findings.push(Finding::new(
            TG_CONTAINER_TRAILING,
            "error",
            "ext",
            format!(
                "{} trailing byte(s) follow the final EXT section entry; the container ends at its last entry",
                scan.trailing_bytes
            ),
        ));
    }
    findings
}


/// Writes a NEW project file: the source container's
/// bytes with its Torogao EXT section replaced by `ext`
/// (extdata.d's writeProjectWithExt). The source is
/// never modified.
pub fn write_project_with_ext(
    in_path: &str,
    out_path: &str,
    ext: &Value,
) -> Result<(), String> {
    let bytes = std::fs::read(in_path)
        .map_err(|e| format!("project unreadable: {e}"))?;
    let source = torogao_format::container::read_inp(&bytes)
        .map_err(|e| e.message().to_string())?;
    let out = torogao_format::container::write_with_ext(&source, ext);
    std::fs::write(out_path, out)
        .map_err(|e| format!("could not write project: {e}"))?;
    Ok(())
}
