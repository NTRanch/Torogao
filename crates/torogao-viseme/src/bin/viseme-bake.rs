//! viseme-bake: the `torogao-viseme/1` command-line
//! converter. Reads a Rhubarb cue file (JSON or TSV,
//! chosen by extension) and writes the baked 60 Hz track
//! document.
//!
//! Usage:
//!   viseme-bake <cue-file> <track-file> [--duration <seconds>]
//!
//! `--duration` is the source audio's true duration in
//! seconds; absent one, the last cue's end is the baked
//! duration (see the bake law in `torogao_viseme::bake`).
//! The bake report is printed on stdout; failures print
//! their `TV-` code on stderr and exit non-zero. The
//! track file is written via a temporary sibling and a
//! rename, so a failed bake never leaves a partial track.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use torogao_viseme::error::{VisemeError, VisemeResult};
use torogao_viseme::{VisemeMap, bake, parse_cues_json, parse_cues_tsv, sha256_hex};

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("viseme-bake: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> VisemeResult<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cue_path, track_path, duration) = parse_args(&args)?;
    let bytes = std::fs::read(&cue_path).map_err(|err| {
        VisemeError::new(
            "TV-IO",
            format!("cannot read cue file {}: {err}", cue_path.display()),
        )
    })?;
    let cues = match cue_path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => parse_cues_json(&bytes)?,
        Some("tsv" | "txt") => parse_cues_tsv(&bytes)?,
        _ => {
            return Err(VisemeError::new(
                "TV-CUE-MALFORMED",
                "cue file extension must be .json, .tsv, or .txt",
            ));
        }
    };
    let map = VisemeMap::load()?;
    let source_sha256 = sha256_hex(&bytes);
    let track = bake(&cues, &map, &source_sha256, duration)?;
    write_atomic(&track_path, track.to_json().as_bytes())?;
    let report = track.report;
    Ok(format!(
        "baked {} frame(s) from {} cue(s): {} dropped as sub-frame, {} channel track(s) -> {}",
        report.frames,
        report.cues_in,
        report.cues_dropped_empty,
        report.tracks_emitted,
        track_path.display()
    ))
}

fn parse_args(args: &[String]) -> VisemeResult<(PathBuf, PathBuf, Option<f64>)> {
    let usage = "usage: viseme-bake <cue-file> <track-file> [--duration <seconds>]";
    if args.len() < 2 {
        return Err(VisemeError::new("TV-USAGE", usage));
    }
    let mut duration: Option<f64> = None;
    let mut rest = &args[2..];
    while !rest.is_empty() {
        match rest[0].as_str() {
            "--duration" => {
                let value = rest.get(1).ok_or_else(|| {
                    VisemeError::new("TV-USAGE", "--duration needs a value in seconds")
                })?;
                duration = Some(value.parse().map_err(|_| {
                    VisemeError::new(
                        "TV-USAGE",
                        format!("--duration value {value:?} is not a number"),
                    )
                })?);
                rest = &rest[2..];
            }
            other => {
                return Err(VisemeError::new(
                    "TV-USAGE",
                    format!("unknown argument {other:?}; {usage}"),
                ));
            }
        }
    }
    Ok((PathBuf::from(&args[0]), PathBuf::from(&args[1]), duration))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> VisemeResult<()> {
    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".tmp");
    let temp = PathBuf::from(temp_name);
    std::fs::write(&temp, bytes).map_err(|err| {
        VisemeError::new("TV-IO", format!("cannot write {}: {err}", temp.display()))
    })?;
    std::fs::rename(&temp, path).map_err(|err| {
        VisemeError::new(
            "TV-IO",
            format!(
                "cannot rename {} to {}: {err}",
                temp.display(),
                path.display()
            ),
        )
    })?;
    Ok(())
}
