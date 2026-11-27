//! Regression tests for the f64 canonical round-trip defect
//! (mocap lane finding, 2026-11-27): serde_json's default float
//! parser is best-effort and landed 1 ulp off on hard decimals,
//! so a document parsed from disk, written with the canonical
//! writer, and parsed again did not return the same values —
//! frozen documents failed TG-CLIP-FREEZE-MISMATCH on
//! revalidation. The writer (`jsonfmt::fmt_f64`, C `%.18g`) was
//! bisected clean against correctly-rounded references; the cure
//! is serde_json's `float_roundtrip` feature, enabled on the
//! workspace dependency in the root Cargo.toml. These tests pin
//! the contract: parse -> canonical write -> parse is the
//! identity on f64 bit patterns, and write -> parse -> write is
//! byte-identical.

use torogao_format::jsonfmt::to_d_json;

/// Deterministic xorshift64* so the sweep is the same on every
/// run and every machine.
struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn round_trip(value: f64) -> (String, f64) {
    let doc = serde_json::json!({ "v": value });
    let text = to_d_json(&doc);
    let back: serde_json::Value =
        serde_json::from_str(&text).expect("canonical text must re-parse");
    let parsed = back["v"].as_f64().expect("value stays a float");
    (text, parsed)
}

#[test]
fn minimal_repro_value_round_trips() {
    // The finding's minimal repro: this decimal denotes the f64
    // with bits bfef074d605993fd (Python and Rust std agree).
    let doc: serde_json::Value =
        serde_json::from_str("{\"v\":-0.9696413881143823}").expect("source parses");
    let value = doc["v"].as_f64().unwrap();
    assert_eq!(value.to_bits(), 0xbfef_074d_6059_93fd);
    // The canonical writer prints the true %.18g of that value.
    let (text, parsed) = round_trip(value);
    assert_eq!(text, "{\"v\":-0.969641388114382274}");
    assert_eq!(parsed.to_bits(), value.to_bits());
    // A second full pass is byte-identical (idempotent).
    let (text2, _) = round_trip(parsed);
    assert_eq!(text2, text);
}

#[test]
fn edge_values_round_trip() {
    let edges = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.3,
        1.0 / 3.0,
        f64::MAX,
        f64::MIN_POSITIVE,
        f64::from_bits(1),                     // smallest subnormal
        f64::from_bits(0x000f_ffff_ffff_ffff), // largest subnormal
        1e17,
        1e-5,
        123_456_789.123_456_79,
        -0.9696413881143823,
    ];
    for value in edges {
        let (text, parsed) = round_trip(value);
        assert_eq!(
            parsed.to_bits(),
            value.to_bits(),
            "edge value {value} ({text}) shifted"
        );
        let (text2, _) = round_trip(parsed);
        assert_eq!(text2, text, "edge value {value} not idempotent");
    }
}

#[test]
fn randomized_f64_round_trip_sweep() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    let mut checked = 0u32;
    for _ in 0..100_000 {
        let value = f64::from_bits(rng.next_u64());
        if !value.is_finite() {
            continue;
        }
        checked += 1;
        let (text, parsed) = round_trip(value);
        assert_eq!(
            parsed.to_bits(),
            value.to_bits(),
            "random value {value} ({text}) shifted by one write/read pass"
        );
        let (text2, _) = round_trip(parsed);
        assert_eq!(text2, text, "random value {value} not idempotent");
    }
    assert!(checked > 90_000, "sweep covered only {checked} values");
}

#[test]
fn real_valued_curve_document_round_trips_byte_identically() {
    // A clip-shaped document whose channel keys carry
    // captured-curve-style values (full-precision decimals, the
    // shape that froze fine in memory and failed from disk).
    // The freeze hash covers exactly this canonical form, so
    // byte-stability across a disk round trip is the freeze
    // contract.
    let mut rng = XorShift(0x1234_5678_9ABC_DEF0);
    let mut keys = Vec::new();
    for frame in 0..120 {
        let raw = rng.next_u64();
        let unit = (raw >> 11) as f64 / (1u64 << 53) as f64; // [0, 1)
        let value = unit * 2.0 - 1.0;
        keys.push(serde_json::json!({
            "frame": frame,
            "interp": "linear",
            "key_type": "keyframe",
            "value": value,
        }));
    }
    let doc = serde_json::json!({
        "format": "torogao-motion-clip/2",
        "name": "roundtrip-probe",
        "channels": [{
            "target": {"kind": "parameter", "name": "Probe", "property": "value"},
            "keys": keys,
        }],
    });
    let text1 = to_d_json(&doc);
    let reparsed: serde_json::Value =
        serde_json::from_str(&text1).expect("canonical text must re-parse");
    let text2 = to_d_json(&reparsed);
    assert_eq!(
        text1, text2,
        "curve document changed across one disk round trip"
    );
    // And the values themselves are bit-identical after the trip.
    let before = &doc["channels"][0]["keys"];
    let after = &reparsed["channels"][0]["keys"];
    for (b, a) in before
        .as_array()
        .unwrap()
        .iter()
        .zip(after.as_array().unwrap())
    {
        assert_eq!(
            b["value"].as_f64().unwrap().to_bits(),
            a["value"].as_f64().unwrap().to_bits(),
        );
    }
}
