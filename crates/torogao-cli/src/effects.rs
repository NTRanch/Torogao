//! Preview effects stack validation and compilation,
//! ported from the D `torogao.cli.effects` module.

use serde_json::{json, Value};

use torogao_engine::Puppet;

use crate::criteria::Finding;
use crate::packs::{pack_num, pack_str};
use crate::util::jget;

pub const FX_FORMAT: &str = "torogao-effects/1";
pub const FX_MANIFEST_FORMAT: &str = "torogao-effects-manifest/1";
pub const TG_FX_BINDING: &str = "TG-FX-BINDING";
pub const TG_FX_DUPLICATE: &str = "TG-FX-DUPLICATE";
pub const TG_FX_FORMAT: &str = "TG-FX-FORMAT";
pub const TG_FX_KIND: &str = "TG-FX-KIND";
pub const TG_FX_RANGE: &str = "TG-FX-RANGE";
pub const TG_FX_TYPE: &str = "TG-FX-TYPE";

pub fn fx_kinds() -> [&'static str; 6] {
    [
        "camera-rec",
        "color-grade",
        "dashboard",
        "grain",
        "hud",
        "letterbox",
    ]
}

fn is_singleton_kind(kind: &str) -> bool {
    kind == "camera-rec" || kind == "color-grade" || kind == "letterbox"
}

pub fn validate_effects(doc: &Value, param_names: Option<&[String]>) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !doc.is_object() {
        findings.push(Finding::new(
            TG_FX_TYPE,
            "error",
            "effects",
            "effects document is not a JSON object",
        ));
        return findings;
    }
    if pack_str(doc, "format") != FX_FORMAT {
        findings.push(Finding::new(
            TG_FX_FORMAT,
            "error",
            "effects.format",
            format!("format is not '{FX_FORMAT}'"),
        ));
        return findings;
    }
    let effects = jget(doc, "effects");
    let Some(effects) = effects.as_array() else {
        findings.push(Finding::new(
            TG_FX_TYPE,
            "error",
            "effects.effects",
            "effects must be an array (order is application order)",
        ));
        return findings;
    };
    let mut seen_singletons: Vec<String> = Vec::new();
    for (i, entry) in effects.iter().enumerate() {
        let at = format!("effects[{i}]");
        if !entry.is_object() {
            findings.push(Finding::new(
                TG_FX_TYPE,
                "error",
                at.clone(),
                "effect entry is not an object",
            ));
            continue;
        }
        let kind = pack_str(entry, "kind");
        if !fx_kinds().contains(&kind.as_str()) {
            findings.push(Finding::new(
                TG_FX_KIND,
                "error",
                format!("{at}.kind"),
                format!(
                    "unknown effect kind '{kind}' (known: {})",
                    fx_kinds().join(", ")
                ),
            ));
            continue;
        }
        if is_singleton_kind(&kind) {
            if seen_singletons.contains(&kind) {
                findings.push(Finding::new(
                    TG_FX_DUPLICATE,
                    "error",
                    format!("{at}.kind"),
                    format!(
                        "singleton effect '{kind}' appears more than once; two do not compose"
                    ),
                ));
            }
            seen_singletons.push(kind.clone());
        }
        if kind == "grain" {
            let amount = pack_num(entry, "amount", -1.0);
            if amount < 0.0 || amount > 1.0 {
                findings.push(Finding::new(
                    TG_FX_RANGE,
                    "error",
                    format!("{at}.amount"),
                    "grain amount must be a number in [0, 1]",
                ));
            }
        }
        if kind == "color-grade" && pack_str(entry, "preset").is_empty() {
            findings.push(Finding::new(
                TG_FX_TYPE,
                "error",
                format!("{at}.preset"),
                "color-grade requires a named preset",
            ));
        }
        if kind == "letterbox" && pack_str(entry, "ratio").is_empty() {
            findings.push(Finding::new(
                TG_FX_TYPE,
                "error",
                format!("{at}.ratio"),
                "letterbox requires a ratio (e.g. \"2.39:1\")",
            ));
        }
        if kind == "hud" {
            let elements = jget(entry, "elements");
            if !elements.is_array() || elements.as_array().is_some_and(|a| a.is_empty()) {
                findings.push(Finding::new(
                    TG_FX_TYPE,
                    "error",
                    format!("{at}.elements"),
                    "hud requires a non-empty elements array",
                ));
            }
        }
        if kind == "dashboard" {
            let bindings = jget(entry, "bindings");
            let Some(bindings) = bindings.as_array().filter(|a| !a.is_empty()) else {
                findings.push(Finding::new(
                    TG_FX_TYPE,
                    "error",
                    format!("{at}.bindings"),
                    "dashboard requires a non-empty bindings array",
                ));
                continue;
            };
            for (j, binding) in bindings.iter().enumerate() {
                let bat = format!("{at}.bindings[{j}]");
                let label = pack_str(binding, "label");
                let param = pack_str(binding, "parameter");
                if label.is_empty() {
                    findings.push(Finding::new(
                        TG_FX_TYPE,
                        "error",
                        format!("{bat}.label"),
                        "binding label is empty",
                    ));
                }
                if param.is_empty() {
                    findings.push(Finding::new(
                        TG_FX_BINDING,
                        "error",
                        format!("{bat}.parameter"),
                        "binding names no parameter",
                    ));
                    continue;
                }
                if let Some(names) = param_names {
                    if !names.iter().any(|pn| pn == &param) {
                        findings.push(Finding::new(
                            TG_FX_BINDING,
                            "error",
                            format!("{bat}.parameter"),
                            format!(
                                "dashboard binding '{label}' names parameter '{param}', which the puppet does not have"
                            ),
                        ));
                    }
                }
            }
        }
    }
    findings
}

pub fn puppet_param_names(puppet: &Puppet) -> Vec<String> {
    puppet.params.iter().map(|p| p.name.clone()).collect()
}

/// Compiles a stack against a puppet; Err carries the
/// findings when the stack does not compile.
pub fn compile_effects(doc: &Value, puppet: &Puppet) -> Result<Value, Vec<Finding>> {
    let findings = validate_effects(doc, Some(&puppet_param_names(puppet)));
    if findings.iter().any(|f| f.severity == "error") {
        return Err(findings);
    }
    let mut entries: Vec<Value> = Vec::new();
    if let Some(effects) = jget(doc, "effects").as_array() {
        for entry in effects {
            let mut resolved = entry.clone();
            if pack_str(entry, "kind") == "dashboard" {
                let mut bindings: Vec<Value> = Vec::new();
                if let Some(list) = jget(entry, "bindings").as_array() {
                    for binding in list {
                        let mut b = binding.clone();
                        if b.is_object() {
                            b["resolved"] = json!(true);
                        }
                        bindings.push(b);
                    }
                }
                resolved["bindings"] = Value::Array(bindings);
            }
            entries.push(resolved);
        }
    }
    let count = entries.len();
    Ok(json!({
        "effectCount": count,
        "effects": entries,
        "format": FX_MANIFEST_FORMAT,
    }))
}
