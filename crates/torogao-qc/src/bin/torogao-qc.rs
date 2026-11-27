//! torogao-qc — render-QC checker for frozen scene specs
//! (RENDER-QC-CONTRACT step 1).
//!
//! Usage:
//!   torogao-qc inspect <spec.json> [--manifest <manifest.json>]
//!                      [--known-options <options.json>]
//!   torogao-qc diff <reference_dir> <candidate_dir>
//!                      [--threshold <0-255>] [--region-gap <0-16>]
//!                      [--exclude-aa] [--artifacts <dir>]
//!   torogao-qc asset --rules <rules.json> <asset.glb>...
//!   torogao-qc export-check --manifest <manifest.json> <export.glb>
//!   torogao-qc roundtrip --manifest <manifest.json>
//!                      --blender <blender-binary>
//!                      --transform-tolerance <float>
//!                      <source.glb> <export.glb>
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
        "asset" => run_asset(rest),
        "export-check" => run_export_check(rest),
        "roundtrip" => run_roundtrip(rest),
        _ => Err(usage()),
    }
}

fn usage() -> QcError {
    QcError::new(
        "QC-E-USAGE",
        "usage: torogao-qc inspect <spec.json> [--manifest <m>] [--known-options <o>] \
         | torogao-qc diff <reference_dir> <candidate_dir> [--threshold <0-255>] \
         [--region-gap <0-16>] [--exclude-aa] [--artifacts <dir>] \
         | torogao-qc asset --rules <rules.json> <asset.glb>... \
         | torogao-qc export-check --manifest <manifest.json> <export.glb> \
         | torogao-qc roundtrip --manifest <m> --blender <bin> \
         --transform-tolerance <f> <source.glb> <export.glb>",
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
        artifacts: flag(&flags, "artifacts").map(PathBuf::from),
    };
    torogao_qc::diff::diff_dirs_with_options(
        &PathBuf::from(reference),
        &PathBuf::from(candidate),
        &options,
    )
}

fn run_asset(args: &[String]) -> Result<Report, QcError> {
    let (positionals, flags) = split_flags(args)?;
    let rules = flag(&flags, "rules").ok_or_else(usage)?;
    if positionals.is_empty() {
        return Err(usage());
    }
    let assets: Vec<PathBuf> = positionals.iter().map(PathBuf::from).collect();
    torogao_qc::asset::check_assets(&PathBuf::from(rules), &assets)
}

fn run_export_check(args: &[String]) -> Result<Report, QcError> {
    let (positionals, flags) = split_flags(args)?;
    let [glb] = positionals.as_slice() else {
        return Err(usage());
    };
    let manifest = flag(&flags, "manifest").ok_or_else(usage)?;
    torogao_qc::export_check::export_check(&PathBuf::from(glb), &PathBuf::from(manifest))
}

fn run_roundtrip(args: &[String]) -> Result<Report, QcError> {
    let (positionals, flags) = split_flags(args)?;
    let [source, export] = positionals.as_slice() else {
        return Err(usage());
    };
    let manifest = flag(&flags, "manifest").ok_or_else(usage)?;
    let blender = flag(&flags, "blender").ok_or_else(usage)?;
    // The tolerance is a required, recorded parameter (B3
    // discipline): no flag, no run — there is no default.
    let tolerance_raw = flag(&flags, "transform-tolerance").ok_or_else(|| {
        QcError::new(
            "QC-E-USAGE",
            "--transform-tolerance is required (there is no default tolerance)",
        )
    })?;
    let transform_tolerance = tolerance_raw.parse::<f64>().map_err(|_| {
        QcError::new(
            "QC-E-USAGE",
            format!("--transform-tolerance must be a finite number >= 0, got {tolerance_raw:?}"),
        )
    })?;
    if !transform_tolerance.is_finite() || transform_tolerance < 0.0 {
        return Err(QcError::new(
            "QC-E-USAGE",
            format!("--transform-tolerance must be a finite number >= 0, got {tolerance_raw:?}"),
        ));
    }
    let options = torogao_qc::roundtrip::RoundtripOptions {
        blender: PathBuf::from(blender),
        transform_tolerance,
    };
    torogao_qc::roundtrip::roundtrip(
        &PathBuf::from(source),
        &PathBuf::from(export),
        &PathBuf::from(manifest),
        &options,
    )
}
