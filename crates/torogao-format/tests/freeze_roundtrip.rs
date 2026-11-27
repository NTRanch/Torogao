//! Freeze-path regression for the f64 round-trip defect
//! (mocap lane finding, 2026-11-27): a clip carrying
//! captured-curve values froze cleanly in memory but its frozen
//! text failed TG-CLIP-FREEZE-MISMATCH when revalidated from
//! disk, because serde_json's default float parser landed 1 ulp
//! off on hard decimals and repeated canonical passes kept
//! shifting values. With `float_roundtrip` enabled on the
//! workspace serde_json dependency, the frozen text revalidates.
//!
//! The key set is generated, not hand-picked: a handful of
//! decimals can sit on fixed points of even a broken parser
//! (the finding's minimal repro value does), while a few
//! hundred converter-shaped values cannot all sit on fixed
//! points — the measured draft shifted 2,146 of ~14,220 values
//! per canonical pass.
//!
//! This test belongs with the motion_clip lineage; it was
//! authored by the torogao-format f64 fix lane
//! (pax/torogao-format-f64-roundtrip-20261127) and verified
//! against this tree — land it with the merge.

use torogao_format::jsonfmt::to_d_json;
use torogao_format::motion_clip::{freeze_document, validate_clip};

/// Deterministic xorshift64*, same generator as the writer-side
/// round-trip sweep.
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

#[test]
fn frozen_real_valued_clip_revalidates_from_disk_text() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    let mut keys = Vec::new();
    // The finding's minimal repro value leads, then generated
    // converter-shaped samples in [-1, 1).
    keys.push(serde_json::json!({
        "frame": 0, "interp": "linear", "key_type": "keyframe",
        "value": -0.9696413881143823,
    }));
    for frame in 1..240 {
        let raw = rng.next_u64();
        let unit = (raw >> 11) as f64 / (1u64 << 53) as f64;
        keys.push(serde_json::json!({
            "frame": frame,
            "interp": if frame % 2 == 0 || frame == 239 { "step" } else { "linear" },
            "key_type": "keyframe",
            "value": unit * 2.0 - 1.0,
        }));
    }
    let doc = serde_json::json!({
        "channels": [
            {"keys": keys,
             "target": {"kind": "parameter", "name": "Probe", "property": "value"}},
            {"keys": [
                {"frame": 0, "interp": "linear", "key_type": "keyframe",
                 "value": [0.9696413881143823, 1.0312499999999998, -0.20207095791797864]},
                {"frame": 239, "interp": "step", "key_type": "keyframe",
                 "value": [0.0, 1.0, 0.0]}
            ],
             "target": {"kind": "joint", "name": "Hips", "property": "translation"}}
        ],
        "format": "torogao-motion-clip/2",
        "loop": null,
        "name": "freeze-roundtrip-probe",
        "op_log": [{"actor": "converter", "op": "author freeze roundtrip probe",
                    "seq": 0, "session": "f64-fix-20261127"}],
        "range": {"end": 239, "start": 0},
        "rate": {"den": 1, "num": 24},
        "rig": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    });
    let source_text = to_d_json(&doc);
    validate_clip(source_text.as_bytes()).expect("probe source validates");

    let frozen = freeze_document(&doc, "torogao-format f64 regression", "tier2-1e-6")
        .expect("real-valued document freezes");
    // The disk form of a frozen document is its canonical text.
    let disk_text = to_d_json(&frozen);
    let report = validate_clip(disk_text.as_bytes())
        .expect("frozen document must revalidate from its disk text");
    let recorded = report
        .freeze_sha256
        .expect("revalidated document carries its freeze hash");
    assert_eq!(recorded.len(), 64);
}
