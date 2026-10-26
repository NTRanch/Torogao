//! `rig-check` — the rig-model validator binary.
//!
//! Contract (tests/RIG-CHECK.md): `rig-check <model.json>`
//! prints a `VALID` line carrying the model's census
//! (parameters, parts, textures, deformers, meshes, physics)
//! and exits 0 when the model is valid; any failure prints an
//! `INVALID` line naming the typed `LRF-` code and exits
//! non-zero. Model parsing and validation live in
//! torogao-format; this binary is the harness-facing shell.
//!
//! Under ruling RP1-D1 a file carrying the predecessor format
//! token is accepted: the parse raises one typed warning per
//! file, printed to stderr so the stdout verdict stays clean,
//! and the verdict is VALID. Writers emit the current token
//! only.

#![forbid(unsafe_code)]

use std::io::Write;
use std::process::ExitCode;

use torogao_format::parse_model;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    ExitCode::from(run(&args, &mut stdout, &mut stderr))
}

/// Runs one validation, returning the process exit code:
/// 0 valid, 1 invalid, 2 usage or read error.
fn run(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    if args.len() != 1 {
        let _ = writeln!(stderr, "usage: rig-check <model.json>");
        return 2;
    }
    let path = &args[0];
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            let _ = writeln!(stderr, "rig-check: cannot read {path}: {err}");
            return 2;
        }
    };
    match parse_model(&bytes) {
        Ok(parsed) => {
            for warning in &parsed.warnings {
                let _ = writeln!(stderr, "{}: {}", warning.code(), warning.message());
            }
            let model = &parsed.model;
            // The canonical round-trip law: the validated
            // model re-emits in canonical form and re-parses
            // to an identical model. A failure here is a
            // validator defect, reported as INVALID, never
            // silently passed.
            let canonical = model.to_canonical_json();
            let round_trips = parse_model(canonical.as_bytes())
                .map(|again| again.model == *model && again.warnings.is_empty())
                .unwrap_or(false);
            if !round_trips {
                let _ = writeln!(
                    stdout,
                    "INVALID {path}: LRF-103 LRF-103: canonical round-trip mismatch"
                );
                return 1;
            }
            let _ = writeln!(
                stdout,
                "VALID {path} (parameters={}, parts={}, textures={}, deformers={}, \
                 meshes={}, physics={}; canonical round-trip OK)",
                model.parameters.len(),
                model.parts.len(),
                model.textures.len(),
                model.deformers.len(),
                model.meshes.len(),
                model.physics.len(),
            );
            0
        }
        Err(err) => {
            let _ = writeln!(stdout, "INVALID {path}: {} {err}", err.code());
            1
        }
    }
}
