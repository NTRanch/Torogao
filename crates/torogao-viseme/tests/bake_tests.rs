//! End-to-end tests for `torogao-viseme/1`: cue text in,
//! baked track out, read back and evaluated independently
//! (the read-back habit: a converter's output is judged
//! by re-reading it, not by trusting the bake).

use torogao_viseme::{VisemeMap, bake, parse_cues_json, sha256_hex};

/// Evaluates one channel of a baked track document at a
/// frame, by reading the emitted JSON's stepped keys —
/// deliberately independent of the bake's internals.
fn sample(track_json: &str, channel: &str, frame: u32) -> f64 {
    let parsed: serde_json::Value = serde_json::from_str(track_json).expect("track parses");
    for track in parsed["group"]["tracks"].as_array().expect("tracks array") {
        if track["channel"] == channel {
            let mut value = 0.0;
            for key in track["keys"].as_array().expect("keys array") {
                if key["frame"].as_u64().expect("frame is a number") <= u64::from(frame) {
                    value = key["value"].as_f64().expect("value is a number");
                }
            }
            return value;
        }
    }
    0.0
}

/// A cue sheet shaped like the real Rhubarb output for
/// the estate's proof recording (same timestamps at the
/// spot-checked points; the middle is elided into a gap,
/// which the bake law covers as rest).
const PROOF_SHAPED_CUES: &str = r#"{"metadata":{"soundFile":"proof.wav","duration":5.57},
"mouthCues":[
 {"start":0.00,"end":0.31,"value":"X"},
 {"start":0.31,"end":0.57,"value":"B"},
 {"start":0.57,"end":0.64,"value":"G"},
 {"start":0.64,"end":0.92,"value":"B"},
 {"start":0.92,"end":0.99,"value":"C"},
 {"start":0.99,"end":1.34,"value":"B"},
 {"start":3.37,"end":3.51,"value":"E"},
 {"start":3.51,"end":3.58,"value":"F"},
 {"start":5.09,"end":5.57,"value":"X"}
]}"#;

#[test]
fn proof_shaped_bake_aligns_at_named_timestamps() {
    let cues = parse_cues_json(PROOF_SHAPED_CUES.as_bytes()).expect("cues parse");
    let map = VisemeMap::load().expect("map loads");
    let track = bake(&cues, &map, "00", None).expect("bake");
    // 5.57 s at 60 Hz -> 334 frames.
    assert_eq!(track.frames, 334);
    let json = track.to_json();
    // t = 1.00 s (frame 60) sits in the B cue [0.99, 1.34):
    // B drives Fac_Mth_Aa at 0.2.
    assert_eq!(sample(json, "Fac_Mth_Aa", 60), 0.2);
    // t = 0.60 s (frame 36) sits in the G cue [0.57, 0.64):
    // G drives Fac_Mth_L_Down at 0.45.
    assert_eq!(sample(json, "Fac_Mth_L_Down", 36), 0.45);
    // t = 3.50 s (frame 210) sits in the E cue [3.37, 3.51):
    // E drives Fac_Mth_Oo at 0.8.
    assert_eq!(sample(json, "Fac_Mth_Oo", 210), 0.8);
    // t = 5.30 s (frame 318) sits in the closing X cue: rest.
    assert_eq!(sample(json, "Fac_Mth_Aa", 318), 0.0);
    assert_eq!(sample(json, "Fac_Mth_Uu", 318), 0.0);
}

#[test]
fn double_run_is_byte_identical() {
    let cues = parse_cues_json(PROOF_SHAPED_CUES.as_bytes()).expect("cues parse");
    let map = VisemeMap::load().expect("map loads");
    let first = bake(&cues, &map, "00", None).expect("bake");
    let second = bake(&cues, &map, "00", None).expect("bake");
    assert_eq!(first.to_json(), second.to_json());
    assert_eq!(
        sha256_hex(first.to_json().as_bytes()),
        sha256_hex(second.to_json().as_bytes())
    );
}

#[test]
fn cue_straddling_a_frame_boundary_lands_on_the_rounded_frame() {
    // The D cue [0.020, 0.030) straddles the frame 1/2
    // boundary at 0.025 s: round(1.2) = 1 and round(1.8) = 2,
    // so it covers frame 1 exactly.
    let bytes = br#"{"mouthCues":[
        {"start":0.00,"end":0.02,"value":"X"},
        {"start":0.02,"end":0.03,"value":"D"},
        {"start":0.03,"end":0.10,"value":"X"}]}"#;
    let cues = parse_cues_json(bytes).expect("cues parse");
    let map = VisemeMap::load().expect("map loads");
    let track = bake(&cues, &map, "00", None).expect("bake");
    assert_eq!(track.frames, 6);
    let json = track.to_json();
    assert_eq!(sample(json, "Fac_Mth_Aa", 0), 0.0);
    assert_eq!(sample(json, "Fac_Mth_Aa", 1), 1.0);
    assert_eq!(sample(json, "Fac_Mth_Aa", 2), 0.0);
}

#[test]
fn provenance_names_the_converter_and_the_source_hash() {
    let cues = parse_cues_json(PROOF_SHAPED_CUES.as_bytes()).expect("cues parse");
    let map = VisemeMap::load().expect("map loads");
    let track = bake(&cues, &map, "cafe", None).expect("bake");
    let parsed: serde_json::Value = serde_json::from_str(track.to_json()).expect("track parses");
    assert_eq!(parsed["format"], "torogao-viseme-track/1");
    assert_eq!(parsed["provenance"]["converter"], "torogao-viseme/1");
    assert_eq!(parsed["provenance"]["source_class"], "converted");
    assert_eq!(parsed["provenance"]["source_sha256"], "cafe");
    assert_eq!(parsed["group"]["kind"], "face");
}
