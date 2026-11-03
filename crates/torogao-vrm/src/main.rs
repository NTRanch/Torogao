//! The torogao-vrm command line: `export` and `validate`.
//!
//! Exit status is 0 on success and 1 on any refusal or failure,
//! with the named error on stderr. Reports print to stdout in a
//! stable field order so gate logs diff cleanly.

use std::process::ExitCode;

use torogao_vrm::{
    ExportOptions, ExpressionMap, VrmError, VrmProfile, export_vrm, prepare_spring, validate_vrm,
};

/// Largest input file the CLI will read, in bytes (256 MiB,
/// matching the library's GLB bound).
const INPUT_BYTES_MAX: u64 = 268_435_456;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    match args.first().map(String::as_str) {
        Some("export") => export(&args[1..]),
        Some("validate") => validate(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  torogao-vrm export --glb FILE [--profile FILE] [--expressions FILE] [--spring FILE] --out FILE\n  torogao-vrm validate --vrm FILE [--profile FILE]"
            );
            Err("no subcommand".into())
        }
    }
}

/// `export`: GLB + profile → VRM. `--profile` is deliberately
/// optional at the flag level so a missing profile reaches the
/// library and is refused by name there.
fn export(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let glb_path = required_flag(args, "--glb")?;
    let out_path = required_flag(args, "--out")?;
    let glb_bytes = read_bounded(&glb_path)?;
    let profile = match flag(args, "--profile") {
        Some(path) => Some(VrmProfile::load(&read_text(&path)?)?),
        None => None,
    };
    let expressions = match flag(args, "--expressions") {
        Some(path) => Some(ExpressionMap::parse(&read_text(&path)?)?),
        None => None,
    };
    let spring = match flag(args, "--spring") {
        Some(path) => Some(prepare_spring(&read_text(&path)?, &glb_bytes)?),
        None => None,
    };
    let options = ExportOptions {
        expressions: expressions.as_ref(),
        spring: spring.as_ref(),
    };
    let bytes = export_vrm(&glb_bytes, profile.as_ref(), &options)?;
    std::fs::write(&out_path, &bytes)?;
    let report = validate_vrm(&bytes, profile.as_ref())?;
    println!("exported: {out_path} ({} bytes)", bytes.len());
    print_report(&report);
    Ok(())
}

/// `validate`: re-parse a VRM and census it.
fn validate(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let vrm_path = required_flag(args, "--vrm")?;
    let bytes = read_bounded(&vrm_path)?;
    let profile = match flag(args, "--profile") {
        Some(path) => Some(VrmProfile::load(&read_text(&path)?)?),
        None => None,
    };
    let report = validate_vrm(&bytes, profile.as_ref())?;
    println!("valid: {vrm_path}");
    print_report(&report);
    Ok(())
}

/// Print a validation report, one field per line, stable order.
fn print_report(report: &torogao_vrm::ValidationReport) {
    let census = &report.census;
    println!("meta name: {}", report.meta_name);
    println!("humanoid bones: {}", report.humanoid_bones.len());
    println!(
        "preset expressions: {}",
        report.preset_expressions.join(", ")
    );
    println!(
        "custom expressions: {}",
        report.custom_expressions.join(", ")
    );
    println!("spring chains: {}", report.spring_chains);
    println!("nodes: {}", census.node_count);
    println!("meshes: {}", census.mesh_count);
    println!("bin bytes: {}", census.bin_bytes);
    println!("bin sha256: {}", census.bin_sha256);
}

/// The value of `--flag`, when present.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

/// The value of a required `--flag`.
fn required_flag(args: &[String], name: &str) -> Result<String, Box<dyn std::error::Error>> {
    flag(args, name).ok_or_else(|| format!("missing required flag {name}").into())
}

/// Read a file with the byte bound enforced before allocation
/// of the full contents (metadata length first).
fn read_bounded(path: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > INPUT_BYTES_MAX {
        return Err(VrmError::CensusMismatch {
            detail: format!(
                "{path} is {} bytes, over the {INPUT_BYTES_MAX}-byte bound",
                metadata.len()
            ),
        }
        .into());
    }
    Ok(std::fs::read(path)?)
}

/// Read a text input (profile, expression map, spring record).
fn read_text(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = read_bounded(path)?;
    Ok(String::from_utf8(bytes)?)
}
