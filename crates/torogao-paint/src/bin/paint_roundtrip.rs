// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! The Phase 1 gate harness: run a session script, save
//! the document, close it, reopen it, replay the log,
//! export the PNG, and print the hashes. Two fresh
//! processes over the same script must print identical
//! `output_sha256` and `png_sha256` lines.
//!
//! Usage: paint-roundtrip <script.ron> <doc-dir> <out.png>
//! Exit codes: 0 ok, 2 usage, 3 failure.

use std::path::Path;
use std::process::ExitCode;

use torogao_paint::hash::to_hex;
use torogao_paint::{SessionScript, load_document, save_document};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: paint-roundtrip <script.ron> <doc-dir> <out.png>");
        return ExitCode::from(2);
    }
    match run(&args[1], &args[2], &args[3]) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("paint-roundtrip: {message}");
            ExitCode::from(3)
        }
    }
}

fn run(script_path: &str, doc_dir: &str, out_png: &str) -> Result<String, String> {
    let script = SessionScript::load(Path::new(script_path)).map_err(|err| err.to_string())?;
    let mut doc = script.run().map_err(|err| err.to_string())?;
    save_document(&mut doc, Path::new(doc_dir)).map_err(|err| err.to_string())?;
    let applied = doc.ops().len();
    drop(doc);
    let loaded = load_document(Path::new(doc_dir)).map_err(|err| err.to_string())?;
    let png = loaded.export_png().map_err(|err| err.to_string())?;
    std::fs::write(out_png, &png).map_err(|err| format!("write {out_png}: {err}"))?;
    Ok(format!(
        "ops_applied={applied}\noutput_sha256={}\npng_sha256={}\nresident_bytes={}\n",
        to_hex(&loaded.composite_hash()),
        to_hex(&torogao_paint::hash::sha256_bytes(&png)),
        loaded.resident_bytes(),
    ))
}
