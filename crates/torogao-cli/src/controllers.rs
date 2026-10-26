//! Performance controllers (Addition N): a playable
//! named slider fanning out to parameters through gain
//! and offset. Two controllers binding one parameter is
//! a refusal, not a blend.

use crate::criteria::Finding;
use crate::packs::{pack_num, pack_str};
use crate::util::jget;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Wire format tag for a controller values document.
pub const CONTROLLER_VALUES_FORMAT: &str = "torogao-controller-values/1";
/// TG-CONTROLLER-DUPLICATE.
pub const TG_CONTROLLER_DUPLICATE: &str = "TG-CONTROLLER-DUPLICATE";
/// TG-CONTROLLER-RANGE.
pub const TG_CONTROLLER_RANGE: &str = "TG-CONTROLLER-RANGE";
/// TG-CONTROLLER-TYPE.
pub const TG_CONTROLLER_TYPE: &str = "TG-CONTROLLER-TYPE";
/// TG-CONTROLLER-UNKNOWN.
pub const TG_CONTROLLER_UNKNOWN: &str = "TG-CONTROLLER-UNKNOWN";
/// TG-CONTROLLER-UNKNOWN-PARAM.
pub const TG_CONTROLLER_UNKNOWN_PARAM: &str = "TG-CONTROLLER-UNKNOWN-PARAM";

/// Validates the controllers array of an EXT document
/// against the puppet's parameter names.
pub fn validate_controllers(ext: &Value, param_names: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let controllers = jget(ext, "controllers");
    if controllers.is_null() {
        return findings;
    }
    let Value::Array(controllers) = controllers else {
        findings.push(Finding::new(
            TG_CONTROLLER_TYPE,
            "error",
            "controllers",
            "controllers must be an array",
        ));
        return findings;
    };
    let param_set: HashSet<&str> = param_names.iter().map(String::as_str).collect();
    let mut ids: HashSet<String> = HashSet::new();
    let mut param_owner: HashMap<String, String> = HashMap::new();
    for (i, controller) in controllers.iter().enumerate() {
        let at = format!("controllers[{i}]");
        if !controller.is_object() {
            findings.push(Finding::new(
                TG_CONTROLLER_TYPE,
                "error",
                at.clone(),
                "controller entry is not an object",
            ));
            continue;
        }
        let id = pack_str(controller, "id");
        if id.is_empty() {
            findings.push(Finding::new(
                TG_CONTROLLER_TYPE,
                "error",
                format!("{at}.id"),
                "controller requires an id",
            ));
            continue;
        }
        if !ids.insert(id.clone()) {
            findings.push(Finding::new(
                TG_CONTROLLER_DUPLICATE,
                "error",
                format!("{at}.id"),
                format!("duplicate controller id '{id}'"),
            ));
        }
        let min = pack_num(controller, "min", 0.0);
        let max = pack_num(controller, "max", 0.0);
        if max <= min {
            findings.push(Finding::new(
                TG_CONTROLLER_RANGE,
                "error",
                at.clone(),
                format!("controller '{id}' has max <= min"),
            ));
        }
        let default = pack_num(controller, "default", min);
        if default < min || default > max {
            findings.push(Finding::new(
                TG_CONTROLLER_RANGE,
                "error",
                format!("{at}.default"),
                format!("controller '{id}' default is outside [min, max]"),
            ));
        }
        let bindings = jget(controller, "bindings");
        let Value::Array(bindings) = bindings else {
            findings.push(Finding::new(
                TG_CONTROLLER_TYPE,
                "error",
                format!("{at}.bindings"),
                format!("controller '{id}' binds no parameters"),
            ));
            continue;
        };
        if bindings.is_empty() {
            findings.push(Finding::new(
                TG_CONTROLLER_TYPE,
                "error",
                format!("{at}.bindings"),
                format!("controller '{id}' binds no parameters"),
            ));
            continue;
        }
        for (j, binding) in bindings.iter().enumerate() {
            let param = pack_str(binding, "parameter");
            let bat = format!("{at}.bindings[{j}]");
            if param.is_empty() || !param_set.contains(param.as_str()) {
                findings.push(Finding::new(
                    TG_CONTROLLER_UNKNOWN_PARAM,
                    "error",
                    format!("{bat}.parameter"),
                    format!(
                        "controller '{id}' binds parameter '{param}', which the puppet does not have"
                    ),
                ));
                continue;
            }
            match param_owner.get(&param) {
                Some(owner) if *owner != id => {
                    findings.push(Finding::new(
                        TG_CONTROLLER_DUPLICATE,
                        "error",
                        format!("{bat}.parameter"),
                        format!(
                            "parameter '{param}' is bound by both '{owner}' and '{id}'; one owner per parameter"
                        ),
                    ));
                }
                Some(_) => {}
                None => {
                    param_owner.insert(param.clone(), id.clone());
                }
            }
        }
    }
    findings
}

/// Resolves controller values against an EXT document
/// into a parameter -> value map (in puppet parameter
/// order at the call site; the map itself is unordered).
/// Values not supplied fall back to each controller's
/// default. Unknown controller ids in the values doc are
/// reported as problems.
pub fn resolve_controllers(
    ext: &Value,
    values_doc: &Value,
) -> (Vec<(String, f64)>, Vec<String>) {
    let mut out: Vec<(String, f64)> = Vec::new();
    let mut problems = Vec::new();
    let Value::Array(controllers) = jget(ext, "controllers") else {
        return (out, problems);
    };
    let values = if values_doc.is_object() {
        jget(values_doc, "values").clone()
    } else {
        Value::Null
    };
    let mut known: HashSet<String> = HashSet::new();
    for controller in controllers {
        let id = pack_str(controller, "id");
        if id.is_empty() {
            continue;
        }
        known.insert(id.clone());
        let min = pack_num(controller, "min", 0.0);
        let max = pack_num(controller, "max", 1.0);
        let mut value = pack_num(controller, "default", min);
        if values.is_object() {
            let supplied = jget(&values, &id);
            if supplied.is_number() {
                value = pack_num(&values, &id, value);
            }
        }
        value = value.clamp(min, max);
        if let Value::Array(bindings) = jget(controller, "bindings") {
            for binding in bindings {
                let param = pack_str(binding, "parameter");
                if param.is_empty() {
                    continue;
                }
                let resolved = pack_num(binding, "gain", 1.0) * value
                    + pack_num(binding, "offset", 0.0);
                // Later bindings overwrite (reference AA assignment).
                if let Some(slot) = out.iter_mut().find(|(name, _)| *name == param) {
                    slot.1 = resolved;
                } else {
                    out.push((param, resolved));
                }
            }
        }
    }
    if let Value::Object(values) = &values {
        for id in values.keys() {
            if !known.contains(id) {
                problems.push(format!("values name unknown controller '{id}'"));
            }
        }
    }
    (out, problems)
}
