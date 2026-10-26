//! Accessors over `serde_json::Value` with the reference
//! implementation's forgiving semantics: a missing key or a
//! wrong-typed value reads as null / empty / zero, never as a
//! panic (the D side's `jget`, `packStr`, and `packNum`).

use serde_json::Value;

/// A shared null value for absent members.
pub fn null_value() -> &'static Value {
    static NULL: Value = Value::Null;
    &NULL
}

/// Object member access yielding null for a missing key or a
/// non-object parent.
pub fn jget<'a>(obj: &'a Value, key: &str) -> &'a Value {
    match obj {
        Value::Object(map) => map.get(key).unwrap_or_else(|| null_value()),
        _ => null_value(),
    }
}

/// True when the value is any JSON number.
pub fn is_num(value: &Value) -> bool {
    value.is_number()
}

/// The numeric value as f64 (integers widen); 0 for
/// non-numbers.
pub fn num_of(value: &Value) -> f64 {
    match value {
        Value::Number(number) => number.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// String member access yielding "" when absent or mistyped.
pub fn jstr<'a>(obj: &'a Value, key: &str) -> &'a str {
    match jget(obj, key) {
        Value::String(text) => text,
        _ => "",
    }
}

/// Numeric member access yielding `fallback` when absent or
/// mistyped.
pub fn jnum(obj: &Value, key: &str, fallback: f64) -> f64 {
    let value = jget(obj, key);
    if is_num(value) {
        num_of(value)
    } else {
        fallback
    }
}

/// Array member access yielding an empty slice when absent
/// or mistyped.
pub fn jarr<'a>(obj: &'a Value, key: &str) -> &'a [Value] {
    match jget(obj, key) {
        Value::Array(items) => items,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::{jget, jnum, jstr, num_of};
    use serde_json::json;

    #[test]
    fn accessors_forgive_missing_and_mistyped_members() {
        let doc = json!({"name": "maren", "count": 3, "ratio": 0.5});
        assert_eq!(jstr(&doc, "name"), "maren");
        assert_eq!(jstr(&doc, "missing"), "");
        assert_eq!(jstr(&doc, "count"), "");
        assert_eq!(jnum(&doc, "count", 9.0), 3.0);
        assert_eq!(jnum(&doc, "ratio", 9.0), 0.5);
        assert_eq!(jnum(&doc, "name", 9.0), 9.0);
        assert!(jget(&doc, "missing").is_null());
        assert_eq!(num_of(&json!("text")), 0.0);
    }
}
