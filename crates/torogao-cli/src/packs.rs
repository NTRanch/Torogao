//! Lane packs (format `torogao-pack/1`): loading and the
//! governed vocabularies — expressions, tracking sets,
//! rating rules, word-count bounds, kink vocabulary.

use crate::paths::find_data_file;
use crate::util::{jget, jstr};
use serde_json::Value;

/// Pack document format identifier.
pub const PACK_FORMAT: &str = "torogao-pack/1";

/// Loads and format-checks a pack by name.
pub fn load_pack(name: &str) -> Result<Value, String> {
    let Some(path) = find_data_file("TOROGAO_PACKS_DIR", "packs", &format!("{name}.json")) else {
        return Err(format!("lane pack '{name}' not found"));
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("lane pack '{name}' is unreadable: {err}"))?;
    let doc: Value = serde_json::from_str(&text)
        .map_err(|err| format!("lane pack '{name}' is unreadable: {err}"))?;
    if !doc.is_object() || jstr(&doc, "format") != PACK_FORMAT {
        return Err(format!(
            "lane pack '{name}' is not a {PACK_FORMAT} document"
        ));
    }
    Ok(doc)
}

/// String member access yielding "" when absent/mistyped
/// (packs.d `packStr`).
pub fn pack_str(obj: &Value, key: &str) -> String {
    jstr(obj, key).to_string()
}

/// Numeric member access with fallback (`packNum`).
pub fn pack_num(obj: &Value, key: &str, fallback: f64) -> f64 {
        crate::util::jnum(obj, key, fallback)
}

/// Names of all shipped packs (the v1 pair, each present
/// only when its file resolves).
pub fn pack_names() -> Vec<String> {
    ["adult", "all-ages"]
        .iter()
        .filter(|name| {
            find_data_file("TOROGAO_PACKS_DIR", "packs", &format!("{name}.json")).is_some()
        })
        .map(|name| name.to_string())
        .collect()
}

/// The pack's governed expression set.
pub fn pack_expressions(pack: &Value) -> Vec<Value> {
    match jget(pack, "expressions") {
        Value::Array(items) => items.clone(),
        _ => Vec::new(),
    }
}

/// One governed expression by name.
pub fn pack_expression(pack: &Value, name: &str) -> Value {
    for expression in pack_expressions(pack) {
        if pack_str(&expression, "name") == name {
            return expression;
        }
    }
    Value::Null
}

/// One tracking set by id.
pub fn pack_tracking_set(pack: &Value, set_id: &str) -> Value {
    if let Value::Array(sets) = jget(pack, "trackingSets") {
        for set in sets {
            if pack_str(set, "id") == set_id {
                return set.clone();
            }
        }
    }
    Value::Null
}

/// Is `rating` a value the pack knows at all?
pub fn pack_rating_known(pack: &Value, rating: &str) -> bool {
    let values = jget(jget(pack, "ratingRules"), "values");
    if let Value::Array(values) = values {
        return values.iter().any(|v| v.as_str() == Some(rating));
    }
    false
}

/// Is `rating` permitted for a project in lane `lane`?
pub fn pack_rating_allowed(pack: &Value, lane: &str, rating: &str) -> bool {
    let table = jget(jget(pack, "ratingRules"), "laneRatings");
    if let Value::Array(allowed) = jget(table, lane) {
        return allowed.iter().any(|v| v.as_str() == Some(rating));
    }
    false
}

/// The pack's word-count bounds for a scene class (null
/// when the class is not defined).
pub fn pack_word_bounds(pack: &Value, scene_class: &str) -> Value {
    jget(jget(pack, "wordCountBounds"), scene_class).clone()
}

/// The pack's kink vocabulary.
pub fn pack_kink_vocabulary(pack: &Value) -> Vec<String> {
    match jget(pack, "kinkVocabulary") {
        Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}
