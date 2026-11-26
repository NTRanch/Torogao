//! torogao-qc — render-QC checker for frozen scene specs
//! (RENDER-QC-CONTRACT step 1).
//!
//! Usage:
//!   torogao-qc inspect <spec.json> [--manifest <manifest.json>]
//!                      [--known-options <options.json>]
//!   torogao-qc diff <reference_dir> <candidate_dir>
//!                      [--threshold <0-255>] [--region-gap <0-16>]
//!                      [--exclude-aa]
//!
//! The JSON report goes to stdout. Exit codes: 0 = the report
//! passes (no fail-severity findings), 1 = fail-severity
//! findings present, 2 = the run itself failed (a typed
//! `QC-E-*` error as JSON on stderr).

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use torogao_qc::error::QcError;
use torogao_qc::limits::PIXEL_THRESHOLD_DEFAULT;
use torogao_qc::report::Report;

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            println!("{}", report.to_json_string());
            if report.failed() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(err) => {
            eprintln!(
                "{}",
                serde_json::json!({"error": {"code": err.code(), "message": err.message()}})
            );
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<Report, QcError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (verb, rest) = args.split_first().ok_or_else(usage)?;
    match verb.as_str() {
        "inspect" => run_inspect(rest),
        "diff" => run_diff(rest),
        _ => Err(usage()),
    }
}

fn usage() -> QcError {
    QcError::new(
        "QC-E-USAGE",
        "usage: torogao-qc inspect <spec.json> [--manifest <m>] [--known-options <o>] \
         | torogao-qc diff <reference_dir> <candidate_dir> [--threshold <0-255>] \
         [--region-gap <0-16>] [--exclude-aa]",
    )
}

/// Parsed `--flag value` pairs, in argument order.
type FlagPairs = Vec<(String, String)>;

/// Splits positionals from `--flag value` pairs.
fn split_flags(args: &[String]) -> Result<(Vec<String>, FlagPairs), QcError> {
    let mut positionals = Vec::new();
    let mut flags = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(flag) = arg.strip_prefix("--") {
            let value = iter.next().ok_or_else(|| {
                QcError::new("QC-E-USAGE", format!("flag --{flag} needs a value"))
            })?;
            flags.push((flag.to_string(), value.clone()));
        } else {
            positionals.push(arg.clone());
        }
    }
    Ok((positionals, flags))
}

fn flag<'a>(flags: &'a FlagPairs, name: &str) -> Option<&'a str> {
    flags
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn run_inspect(args: &[String]) -> Result<Report, QcError> {
    let (positionals, flags) = split_flags(args)?;
    let [spec] = positionals.as_slice() else {
        return Err(usage());
    };
    let known = match flag(&flags, "known-options") {
        Some(path) => torogao_qc::options::load_set(&PathBuf::from(path))?,
        None => torogao_qc::options::builtin_set(),
    };
    torogao_qc::inspect::inspect(
        &PathBuf::from(spec),
        flag(&flags, "manifest").map(PathBuf::from).as_deref(),
        &known,
    )
}

fn run_diff(args: &[String]) -> Result<Report, QcError> {
    // `--exclude-aa` is a boolean flag; the pair splitter
    // only handles `--flag value`, so extract it first.
    let exclude_aa = args.iter().any(|a| a == "--exclude-aa");
    let rest: Vec<String> = args
        .iter()
        .filter(|a| *a != "--exclude-aa")
        .cloned()
        .collect();
    let (positionals, flags) = split_flags(&rest)?;
    let [reference, candidate] = positionals.as_slice() else {
        return Err(usage());
    };
    let threshold = match flag(&flags, "threshold") {
        Some(raw) => raw.parse::<u8>().map_err(|_| {
            QcError::new(
                "QC-E-USAGE",
                format!("--threshold must be an integer 0-255, got {raw:?}"),
            )
        })?,
        None => PIXEL_THRESHOLD_DEFAULT,
    };
    let region_gap = match flag(&flags, "region-gap") {
        Some(raw) => raw.parse::<u32>().map_err(|_| {
            QcError::new(
                "QC-E-USAGE",
                format!("--region-gap must be an integer 0-16, got {raw:?}"),
            )
        })?,
        None => 0,
    };
    let options = torogao_qc::diff::DiffOptions {
        threshold,
        region_gap,
        exclude_aa,
    };
    torogao_qc::diff::diff_dirs_with_options(
        &PathBuf::from(reference),
        &PathBuf::from(candidate),
        &options,
    )
}
