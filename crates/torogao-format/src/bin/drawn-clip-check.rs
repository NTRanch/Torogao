//! `drawn-clip-check` — the gate tool for
//! `torogao-drawn-clip/1` documents (drawn-animation
//! spike, 2026-11-27).
//!
//! Usage:
//!   drawn-clip-check <file>...
//!       Validate each file; print one line per file and
//!       exit non-zero if any file refuses.
//!   drawn-clip-check --freeze <compiler> <band> <file>
//!       Freeze one working document and print the frozen
//!       document's canonical text to stdout.
//!   drawn-clip-check --canonical <file>
//!       Print the document's canonical form (the estate
//!       canonical writer over the document without its
//!       freeze block) — the exact bytes the freeze hash
//!       covers.

use std::process::ExitCode;

use torogao_format::drawn_clip::{canonical_bytes, freeze_document, validate_clip};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() >= 4 && args[0] == "--freeze" {
        return freeze(&args[1], &args[2], &args[3]);
    }
    if args.len() == 2 && args[0] == "--canonical" {
        return canonical(&args[1]);
    }
    if args.is_empty() {
        eprintln!("usage: drawn-clip-check [--freeze <compiler> <band> | --canonical] <file>...");
        return ExitCode::from(2);
    }
    let mut failed = false;
    for path in &args {
        match std::fs::read(path) {
            Ok(bytes) => match validate_clip(&bytes) {
                Ok(report) => {
                    let hash = report.freeze_sha256.as_deref().unwrap_or("-");
                    println!(
                        "OK {path} name={} canvas={}x{} fps={} range={}..={} drawings={} tracks={} keys={} freeze={hash}",
                        report.name,
                        report.canvas.0,
                        report.canvas.1,
                        report.fps,
                        report.range.0,
                        report.range.1,
                        report.drawings,
                        report.tracks,
                        report.keys,
                    );
                }
                Err(error) => {
                    println!("ERR {path} {error}");
                    failed = true;
                }
            },
            Err(error) => {
                println!("ERR {path} TG-DRAW-READ: {error}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn freeze(compiler: &str, band: &str, path: &str) -> ExitCode {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("ERR {path} TG-DRAW-READ: {error}");
            return ExitCode::FAILURE;
        }
    };
    let doc: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(doc) => doc,
        Err(error) => {
            eprintln!("ERR {path} TG-DRAW-NOT-JSON: document is not JSON: {error}");
            return ExitCode::FAILURE;
        }
    };
    match freeze_document(&doc, compiler, band) {
        Ok(frozen) => {
            println!("{}", torogao_format::jsonfmt::to_d_json(&frozen));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ERR {path} {error}");
            ExitCode::FAILURE
        }
    }
}

fn canonical(path: &str) -> ExitCode {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("ERR {path} TG-DRAW-READ: {error}");
            return ExitCode::FAILURE;
        }
    };
    let doc: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(doc) => doc,
        Err(error) => {
            eprintln!("ERR {path} TG-DRAW-NOT-JSON: document is not JSON: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("{}", canonical_bytes(&doc));
    ExitCode::SUCCESS
}
