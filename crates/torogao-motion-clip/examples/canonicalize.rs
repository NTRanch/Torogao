//! Read a torogao-motion-clip/1 document from a file and
//! write its canonical serialization (SPEC.md section
//! 1.2) to stdout. Used to produce checked-in fixtures;
//! handy for inspecting what the parser resolved.
//!
//! Usage: `canonicalize <path-to-clip.json>`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: canonicalize <path-to-clip.json>");
        return ExitCode::from(2);
    }
    let bytes = match std::fs::read(&args[1]) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::FAILURE;
        }
    };
    match torogao_motion_clip::parse_document(&bytes) {
        Ok(clip) => {
            print!("{}", clip.to_canonical_json());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
