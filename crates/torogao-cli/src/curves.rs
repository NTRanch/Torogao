//! Canonical curve documents (`torogao-curves/1`): one
//! curve shape for every baked or simulated parameter
//! track. Written by a dedicated serializer — fixed key
//! order and six-decimal floats — so identical curve data
//! produces byte-identical files. Negative zero is
//! normalized away.

use serde_json::Value;

/// One parameter axis sampled once per step.
#[derive(Debug, Clone)]
pub struct CurveSeries {
    /// Parameter the values belong to.
    pub parameter: String,
    /// 0 = x, 1 = y.
    pub axis: i32,
    /// One value per step, in step order.
    pub values: Vec<f64>,
}

/// A complete curve document.
#[derive(Debug, Clone, Default)]
pub struct CurveSet {
    /// Producer name, e.g. "physics-preview".
    pub source: String,
    /// Seconds per step.
    pub dt: f64,
    /// Series in emission order.
    pub series: Vec<CurveSeries>,
    /// Extra header fields (key, raw JSON text), in order.
    pub extra: Vec<(String, String)>,
    /// Explicit step count; -1 derives it from the series.
    pub step_count: i64,
}

impl CurveSet {
    /// A curve set with derived step count.
    pub fn new(source: &str, dt: f64) -> CurveSet {
        CurveSet {
            source: source.to_string(),
            dt,
            series: Vec::new(),
            extra: Vec::new(),
            step_count: -1,
        }
    }

    /// Number of steps in the document.
    pub fn steps(&self) -> usize {
        if self.step_count >= 0 {
            return self.step_count as usize;
        }
        self.series.iter().map(|s| s.values.len()).max().unwrap_or(0)
    }

    /// The canonical JSON text (no trailing newline).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"format\": {},\n", quote("torogao-curves/1")));
        out.push_str(&format!("  \"source\": {},\n", quote(&self.source)));
        for (key, value) in &self.extra {
            out.push_str(&format!("  {}: {},\n", quote(key), value));
        }
        out.push_str(&format!("  \"dt\": {},\n", format_float(self.dt)));
        out.push_str(&format!("  \"steps\": {},\n", self.steps()));
        out.push_str("  \"series\": [");
        for (i, series) in self.series.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"parameter\": {}, \"axis\": {}, \"values\": [",
                quote(&series.parameter),
                series.axis
            ));
            for (j, value) in series.values.iter().enumerate() {
                if j > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format_float(*value));
            }
            out.push_str("]}");
        }
        out.push_str(if !self.series.is_empty() {
            "\n  ]\n}"
        } else {
            "]\n}"
        });
        out
    }

    /// The same document as a JSON value (tool payloads).
    pub fn to_json_value(&self) -> Value {
        serde_json::from_str(&self.to_json()).unwrap_or(Value::Null)
    }
}

/// Fixed six-decimal float formatting; -0 collapses to 0.
/// Mirrors C `%.6f` rounding on the f64 value.
pub fn format_float(value: f64) -> String {
    if value == 0.0 {
        return "0.000000".to_string();
    }
    let text = format!("{value:.6}");
    if text == "-0.000000" {
        "0.000000".to_string()
    } else {
        text
    }
}

/// Minimal JSON string quoting for identifiers and paths.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{CurveSeries, CurveSet, format_float};

    #[test]
    fn float_format_matches_the_reference() {
        assert_eq!(format_float(0.0), "0.000000");
        assert_eq!(format_float(-0.0), "0.000000");
        assert_eq!(format_float(-0.0000001), "0.000000");
        assert_eq!(format_float(1.0 / 60.0), "0.016667");
        assert_eq!(format_float(0.3), "0.300000");
    }

    #[test]
    fn empty_curve_set_serializes_like_the_reference() {
        let set = CurveSet::new("physics-preview", 0.016667);
        assert_eq!(
            set.to_json(),
            "{\n  \"format\": \"torogao-curves/1\",\n  \"source\": \"physics-preview\",\n  \"dt\": 0.016667,\n  \"steps\": 0,\n  \"series\": []\n}"
        );
    }

    #[test]
    fn series_layout_matches_the_reference() {
        let mut set = CurveSet::new("x", 0.5);
        set.series.push(CurveSeries {
            parameter: "P".to_string(),
            axis: 0,
            values: vec![1.0, 2.5],
        });
        let text = set.to_json();
        assert!(text.contains("\n    {\"parameter\": \"P\", \"axis\": 0, \"values\": [1.000000, 2.500000]}"));
        assert!(text.ends_with("\n  ]\n}"));
    }
}
