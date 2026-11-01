//! greeting-hash — the desktop half of the A-1
//! determinism smoke (BRIEF-A1 Scope 5, gate B6).
//!
//! Runs the play-parity Greeting stream (fixture module:
//! play looping → 45 steps → pause → seek 60 → 1 step)
//! over a fixture file's bytes with the desktop build and
//! prints the trajectory record as JSON. The Android app
//! writes the same record from inside the shell over the
//! embedded bytes; the gate compares the two — equal
//! hashes, or the divergence is a STOP-4 finding, never a
//! tolerance.
//!
//! Usage: greeting-hash <fixture.inx>

use std::process::ExitCode;

use torogao_android::fixture::{PRIMA_REPERTOIRE_SHA256, STREAM_ANIMATION, run_greeting_stream};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: greeting-hash <fixture.inx>");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("greeting-hash: cannot read {path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    match run_greeting_stream(&bytes) {
        Ok(report) => {
            let record = serde_json::json!({
                "kind": "torogao-android-determinism/1",
                "run_target": "desktop host",
                "fixture": path,
                "fixture_sha256_expected": PRIMA_REPERTOIRE_SHA256,
                "animation": STREAM_ANIMATION,
                "steps": report.steps,
                "trajectory_hash": format!("{:016x}", report.trajectory_hash),
                "final_hash": format!("{:016x}", report.final_hash),
                "distinct_states": report.distinct_states,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&record).expect("record serializes")
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("greeting-hash: stream refused: {err}");
            ExitCode::FAILURE
        }
    }
}
