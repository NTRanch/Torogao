//! mmd-ingest-check — validate a `torogao-mmd-ingest/1` record
//! against the committed `mmd-profile/1` data (Stage 2, D-633).
//!
//! Usage: `mmd-ingest-check <profile.ron> <record.json>`
//! Exit codes: 0 = PASS (verdict line on stdout), 1 = validation
//! failures (each named on stdout), 2 = usage error.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: mmd-ingest-check <profile.ron> <record.json>".to_string());
    }
    let profile_text =
        std::fs::read_to_string(&args[1]).map_err(|err| format!("cannot read profile: {err}"))?;
    let record_text =
        std::fs::read_to_string(&args[2]).map_err(|err| format!("cannot read record: {err}"))?;
    match torogao_format::mmd_ingest::validate_texts(&profile_text, &record_text) {
        Ok(summary) => {
            println!(
                "mmd-ingest-check: PASS skeleton={} face={} accounting_rows={} \
                 profile_bones_consumed={} profile_morphs_consumed={}",
                summary.skeleton_entries,
                summary.face_entries,
                summary.accounting_rows,
                summary.profile_bones_consumed,
                summary.profile_morphs_consumed
            );
            Ok(())
        }
        Err(failures) => {
            for failure in &failures {
                println!("mmd-ingest-check: FAIL {failure}");
            }
            Err(format!("{} validation failure(s)", failures.len()))
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if err.starts_with("usage:") => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
        Err(err) => {
            eprintln!("mmd-ingest-check: {err}");
            ExitCode::FAILURE
        }
    }
}
