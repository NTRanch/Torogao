//! Small shared helpers: forgiving JSON access (the
//! criteria module's `jget` family, reused estate-wide)
//! and C-printf-style float formatting (`%g`, `%.4f`)
//! that several reference messages are built with.

use serde_json::Value;

/// Object member access yielding null for a missing key.
pub fn jget<'a>(obj: &'a Value, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    match obj {
        Value::Object(map) => map.get(key).unwrap_or(&NULL),
        _ => &NULL,
    }
}

/// String member access yielding "" when absent/mistyped.
pub fn jstr<'a>(obj: &'a Value, key: &str) -> &'a str {
    match jget(obj, key) {
        Value::String(text) => text,
        _ => "",
    }
}

/// Whether a value is numeric (float or integer).
pub fn is_num(value: &Value) -> bool {
    value.is_number()
}

/// Numeric value as f64 (0 for non-numbers).
pub fn num_of(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

/// Numeric member access with a fallback.
pub fn jnum(obj: &Value, key: &str, fallback: f64) -> f64 {
    let value = jget(obj, key);
    if is_num(value) {
        num_of(value)
    } else {
        fallback
    }
}

/// Whether a value is a JSON string.
pub fn is_str(value: &Value) -> bool {
    value.is_string()
}

/// Formats a float like C `printf("%g")` (D std.format's
/// `%g`): 6 significant digits, trailing zeros stripped,
/// exponent form outside [1e-4, 1e6).
pub fn fmt_g(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if !value.is_finite() {
        return if value.is_nan() {
            "nan".to_string()
        } else if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    let magnitude = value.abs();
    let exponent = magnitude.log10().floor() as i32;
    if exponent < -4 || exponent >= 6 {
        // Exponent form: %.5e with trailing zeros stripped.
        let raw = format!("{value:.5e}");
        let (mantissa, exp) = raw.split_once('e').expect("exponent form");
        let mantissa = strip_zeros(mantissa);
        let exp: i32 = exp.parse().unwrap_or(0);
        format!("{mantissa}e{exp:+03}")
    } else {
        // Fixed form with (6 - 1 - exponent) decimals.
        let decimals = (5 - exponent).max(0) as usize;
        let raw = format!("{value:.decimals$}");
        strip_zeros(&raw)
    }
}

fn strip_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

/// Formats like C `printf("%.4f")`.
pub fn fmt_4f(value: f64) -> String {
    format!("{value:.4}")
}

/// A double widened from an f32, as a JSON number (the
/// census and findings widen f32 fields exactly this way).
pub fn f64_of_f32(value: f32) -> Value {
    Value::from(value as f64)
}

#[cfg(test)]
mod tests {
    use super::fmt_g;

    #[test]
    fn g_format_matches_c_printf() {
        assert_eq!(fmt_g(0.0), "0");
        assert_eq!(fmt_g(-5.0), "-5");
        assert_eq!(fmt_g(0.3), "0.3");
        assert_eq!(fmt_g(45.0), "45");
        assert_eq!(fmt_g(100000.0), "100000");
        assert_eq!(fmt_g(1000000.0), "1e+06");
        assert_eq!(fmt_g(0.00001), "1e-05");
        assert_eq!(fmt_g(0.0001), "0.0001");
        assert_eq!(fmt_g(1234567.0), "1.23457e+06");
        assert_eq!(fmt_g(3.14159265), "3.14159");
    }
}
