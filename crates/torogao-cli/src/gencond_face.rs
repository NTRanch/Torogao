//! The `gencond` CLI face (D-822): a thin shell over
//! `torogao_author::run_verb` for the generation-conditioning
//! family — `torogao gencond <verb> <project-dir>
//! [inputs-json]`.
//!
//! The face holds no validation logic (the author crate's
//! contract): it parses the optional inputs JSON, resolves
//! project names by passing them through as paths, prints the
//! answer document in the canonical JSON form, and maps the
//! answer status to the author face's exit codes (ok 0,
//! findings 1, refused/unloadable 2, usage 3).

use std::io::Write;
use std::path::{Path, PathBuf};
use torogao_author::Status;
use torogao_format::jsonfmt::to_d_json;

/// Runs one `gencond` invocation.
pub fn run_gencond(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(verb) = args.get(1) else {
        return usage(
            err,
            "gencond needs a verb (conditioning_compile, conditioning_set)",
        );
    };
    let Some(dir) = args.get(2) else {
        return usage(err, "gencond needs a project directory");
    };
    if args.len() > 4 {
        return usage(err, "gencond takes at most an inputs-json argument");
    }
    let inputs: serde_json::Value = match args.get(3) {
        Some(text) => match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => {
                let _ = writeln!(err, "gencond: inputs are not valid JSON: {error}");
                return 3;
            }
        },
        None => serde_json::json!({}),
    };
    let root = Path::new(dir);
    let resolve =
        |name: &str| -> Result<PathBuf, torogao_author::Refusal> { Ok(PathBuf::from(name)) };
    let answer = torogao_author::run_verb(root, root, verb, &inputs, &resolve);
    let _ = writeln!(out, "{}", to_d_json(&answer.doc));
    match answer.status {
        Status::Ok => 0,
        Status::Findings => 1,
        Status::Refused | Status::Unloadable => 2,
    }
}

fn usage(err: &mut dyn Write, message: &str) -> i32 {
    let _ = writeln!(err, "gencond: {message}");
    let _ = writeln!(
        err,
        "usage: torogao gencond <verb> <project-dir> [inputs-json]"
    );
    3
}
