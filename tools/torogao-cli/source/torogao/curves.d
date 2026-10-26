/*
    Torogao canonical curve documents

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    One curve shape for every baked or simulated parameter
    track in Torogao: the physics preview, the audio baker
    and the viseme baker all emit this document so curves
    from different producers compose and compare directly.

    The document is written by a dedicated serializer, not
    std.json: key order is fixed by construction and every
    value is printed with a fixed six-decimal format, so
    identical curve data produces byte-identical files.
    Negative zero is normalized away. A SHA-256 over the
    emitted bytes is the replay identity of a curve set.
*/
module torogao.curves;

import std.format : format;
import std.json : JSONValue;
import std.string : replace;

/// Curve document format identifier.
enum curveFormat = "torogao-curves/1";

/// One parameter axis sampled once per step.
struct CurveSeries {
    string parameter; /// Parameter the values belong to.
    int axis;         /// 0 = x (the only axis of scalar parameters), 1 = y.
    double[] values;  /// One value per step, in step order.
}

/**
    A complete curve document: a fixed step size, the number
    of steps, and one series per tracked parameter axis.
*/
struct CurveSet {
    string source;        /// Producer name, e.g. "physics-preview".
    double dt;            /// Seconds per step.
    CurveSeries[] series; /// Series in emission order.
    string[2][] extra;    /// Extra header fields (key, JSON text), in order.
    long stepCount = -1;  /// Explicit step count; -1 derives it from the series.

    /// Number of steps in the document.
    size_t steps() const {
        if (stepCount >= 0) return cast(size_t) stepCount;
        size_t n = 0;
        foreach (s; series)
            if (s.values.length > n) n = s.values.length;
        return n;
    }

    /// The canonical JSON text (no trailing newline).
    string toJson() const {
        string out_;
        out_ ~= "{\n";
        out_ ~= "  \"format\": " ~ quote(curveFormat) ~ ",\n";
        out_ ~= "  \"source\": " ~ quote(source) ~ ",\n";
        foreach (kv; extra)
            out_ ~= "  " ~ quote(kv[0]) ~ ": " ~ kv[1] ~ ",\n";
        out_ ~= "  \"dt\": " ~ formatFloat(dt) ~ ",\n";
        out_ ~= "  \"steps\": " ~ format("%d", steps()) ~ ",\n";
        out_ ~= "  \"series\": [";
        foreach (i, s; series) {
            if (i > 0) out_ ~= ",";
            out_ ~= "\n    {\"parameter\": " ~ quote(s.parameter)
                ~ ", \"axis\": " ~ format("%d", s.axis) ~ ", \"values\": [";
            foreach (j, v; s.values) {
                if (j > 0) out_ ~= ", ";
                out_ ~= formatFloat(v);
            }
            out_ ~= "]}";
        }
        out_ ~= series.length > 0 ? "\n  ]\n}" : "]\n}";
        return out_;
    }

    /// The same document as a std.json value (tool payloads).
    JSONValue toJsonValue() const {
        import std.json : parseJSON;
        return parseJSON(toJson());
    }
}

/// Fixed six-decimal float formatting; -0 collapses to 0.
string formatFloat(double v) {
    if (v == 0) return "0.000000";
    string s = format("%.6f", v);
    return s == "-0.000000" ? "0.000000" : s;
}

/// Minimal JSON string quoting for identifiers and paths.
string quote(string s) {
    string out_ = "\"";
    foreach (char c; s) {
        switch (c) {
            case '"':  out_ ~= "\\\""; break;
            case '\\': out_ ~= "\\\\"; break;
            case '\n': out_ ~= "\\n"; break;
            case '\r': out_ ~= "\\r"; break;
            case '\t': out_ ~= "\\t"; break;
            default:
                if (c < 0x20) out_ ~= format("\\u%04x", c);
                else out_ ~= c;
        }
    }
    return out_ ~ "\"";
}
