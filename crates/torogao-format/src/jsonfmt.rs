//! Serialization matching the D reference's JSON writer
//! (std.json `JSONValue.toString`), byte for byte:
//!
//! - object keys in alphabetical order, compact separators;
//! - `/` escaped as `\/` in strings, alongside the standard
//!   control-character escapes;
//! - floats printed with C `%.18g` semantics, with `.0`
//!   appended when the result carries neither a decimal point
//!   nor an exponent (so `1.0` never collapses to `1`).
//!
//! Every JSON document this port emits — CLI results, model
//! exports, authored extension documents — goes through this
//! one writer, so the byte form is decided in exactly one
//! place. Measured against the D binary's own output (the
//! float probe: 0.3 prints as `0.299999999999999989`).

use serde_json::Value;

/// Serializes a value in the reference byte form.
pub fn to_d_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => {
            if let Some(v) = number.as_i64() {
                out.push_str(&v.to_string());
            } else if let Some(v) = number.as_u64() {
                out.push_str(&v.to_string());
            } else if let Some(v) = number.as_f64() {
                out.push_str(&fmt_f64(v));
            } else {
                out.push_str("null");
            }
        }
        Value::String(text) => write_string(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            // serde_json's default map is a BTreeMap, so keys
            // are already in alphabetical order.
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_value(item, out);
            }
            out.push('}');
        }
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Formats an f64 exactly as the reference writer does:
/// C `printf("%.18g")`, then `.0` appended when the result
/// has no `.` and no exponent marker.
pub fn fmt_f64(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    // 18 significant digits, correctly rounded: Rust's
    // exponential formatting with 17 fraction digits produces
    // exactly the %.18g digit sequence.
    let scientific = format!("{:.17e}", value.abs());
    let (mantissa, exponent_text) = scientific
        .split_once('e')
        .expect("exponential format always carries an exponent");
    let exponent: i32 = exponent_text
        .parse()
        .expect("exponential exponent is an integer");
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let mut body = String::new();
    if (-4..18).contains(&exponent) {
        // Fixed notation.
        if exponent >= 0 {
            let int_len = (exponent + 1) as usize;
            if digits.len() > int_len {
                body.push_str(&digits[..int_len]);
                body.push('.');
                body.push_str(&digits[int_len..]);
            } else {
                body.push_str(&digits);
                for _ in digits.len()..int_len {
                    body.push('0');
                }
            }
        } else {
            body.push_str("0.");
            for _ in 0..(-exponent - 1) {
                body.push('0');
            }
            body.push_str(&digits);
        }
    } else {
        // Scientific notation: d[.ddd]e±XX, exponent at least
        // two digits, sign always present.
        body.push_str(&digits[..1]);
        if digits.len() > 1 {
            body.push('.');
            body.push_str(&digits[1..]);
        }
        body.push('e');
        body.push(if exponent < 0 { '-' } else { '+' });
        let magnitude = exponent.unsigned_abs();
        if magnitude < 10 {
            body.push('0');
        }
        body.push_str(&magnitude.to_string());
    }
    if !body.contains('.') && !body.contains('e') {
        body.push_str(".0");
    }
    if value < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::{fmt_f64, to_d_json};
    use serde_json::json;

    #[test]
    fn float_formatting_matches_the_reference_probe() {
        // Every pair measured from the D writer itself.
        assert_eq!(fmt_f64(0.3), "0.299999999999999989");
        assert_eq!(fmt_f64(1.0 / 3.0), "0.333333333333333315");
        assert_eq!(fmt_f64(1e20), "1e+20");
        assert_eq!(fmt_f64(100.0), "100.0");
        assert_eq!(fmt_f64(-0.0), "-0.0");
        assert_eq!(fmt_f64(0.0), "0.0");
        assert_eq!(fmt_f64(f64::from(1.03f32)), "1.02999997138977051");
        assert_eq!(fmt_f64(1.23456789e-5), "1.23456789000000003e-05");
        assert_eq!(fmt_f64(123_456_789.123_456_789), "123456789.123456791");
        assert_eq!(fmt_f64(-2.5), "-2.5");
        assert_eq!(fmt_f64(0.0001), "0.000100000000000000005");
        assert_eq!(fmt_f64(1e-5), "1.00000000000000008e-05");
        assert_eq!(fmt_f64(1e17), "100000000000000000.0");
        assert_eq!(fmt_f64(0.15), "0.149999999999999994");
    }

    #[test]
    fn writer_sorts_keys_compacts_and_escapes_slashes() {
        let doc = json!({"b": 1, "a": "x/y", "c": [true, null, 2.5]});
        assert_eq!(
            to_d_json(&doc),
            "{\"a\":\"x\\/y\",\"b\":1,\"c\":[true,null,2.5]}"
        );
    }

    #[test]
    fn writer_distinguishes_integers_from_floats() {
        let doc = json!({"f": 1.0, "i": 1, "u": 18446744073709551615u64});
        assert_eq!(
            to_d_json(&doc),
            "{\"f\":1.0,\"i\":1,\"u\":18446744073709551615}"
        );
    }
}
