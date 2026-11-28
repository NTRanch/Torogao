//! emit_job — the crate's command-line face for proofs and
//! bench work (not shipped in the app).
//!
//! ```text
//! emit_job derive <record.json> <doc.json>
//!     Build a derive job document from a character record
//!     (runs the derive gate; refuses ineligible records).
//! emit_job judge <record.json> <battery.json> <census.json>
//!                <glb_sha> <twin_sha> <builder_sha> <iso_time>
//!     Print the GateResults JSON for one derivation.
//! ```

use std::process::ExitCode;

use torogao_mobile_rig::gate::judge;
use torogao_mobile_rig::jobs::RigJobDocument;
use torogao_mobile_rig::schema::CharacterRecord;

fn read_json(path: &str) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {path}: {e}"))
}

fn read_record(path: &str) -> Result<CharacterRecord, String> {
    let value = read_json(path)?;
    serde_json::from_value(value).map_err(|e| format!("record {path}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("derive") if args.len() == 3 => {
            let run = || -> Result<(), String> {
                let record = read_record(&args[1])?;
                let doc = RigJobDocument::derive(&record)
                    .map_err(|problems| format!("derive refused:\n- {}", problems.join("\n- ")))?;
                let bytes = doc.canonical_bytes();
                std::fs::write(&args[2], &bytes).map_err(|e| format!("write: {e}"))?;
                println!("job sha256: {}", doc.sha256());
                println!("record sha256: {}", doc.record_sha256);
                if let Some(sha) = &doc.derived_spec_sha256 {
                    println!("derived spec sha256: {sha}");
                }
                Ok(())
            };
            run()
        }
        Some("judge") if args.len() == 8 => {
            let run = || -> Result<(), String> {
                let record = read_record(&args[1])?;
                let battery = read_json(&args[2])?;
                let census = read_json(&args[3])?;
                let results = judge(
                    &record,
                    args[4].clone(),
                    args[5].clone(),
                    args[6].clone(),
                    args[7].clone(),
                    &battery,
                    &census,
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?
                );
                Ok(())
            };
            run()
        }
        _ => Err(
            "usage: emit_job derive <record.json> <doc.json> | emit_job judge <record.json> \
             <battery.json> <census.json> <glb_sha> <twin_sha> <builder_sha> <iso_time>"
                .to_string(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
