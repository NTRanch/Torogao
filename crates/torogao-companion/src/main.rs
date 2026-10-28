//! `torogao-companion` — the desktop companion window
//! (Torogao 1.0, Stage 1.1). See the library crate docs
//! for the architecture; this binary parses the command
//! line and picks the presentation: Wayland layer-shell
//! when a Wayland session is present (primo's Hyprland),
//! the winit fallback elsewhere, either forçable by flag.
//!
//! Evidence modes (the gate instruments):
//! - `--frames N` presents N frames then exits, printing
//!   the run summary as JSON on stdout.
//! - `--screenshot PATH` / `--report PATH` (with
//!   `--frames`) write the final-frame PNG and the JSON
//!   report.
//! - `--trace-check N` runs the Daydream trace headlessly
//!   three times through the companion core and exits 0
//!   only if all three chain hashes agree (Gate G6), also
//!   printing the seam report (Gate G5's ratio) and the
//!   moving-step fraction.
//!
//! The companion ships no audio code path; nothing here
//! can play a sound (Gate G8).

use std::path::PathBuf;
use std::process::ExitCode;

use torogao_companion::error::{CompanionError, CompanionResult};
use torogao_companion::perf::{run_trace, seam_report};
use torogao_companion::state::{LoadOutcome, default_state_path, load_state, save_state};
use torogao_companion::wayland::RunOptions;

const USAGE: &str = "\
torogao-companion — Torogao's desktop companion (Stage 1.1)

USAGE:
    torogao-companion --model <repertoire.inx> [options]

OPTIONS:
    --model <path>            The repertoire model to present (required
                              unless --print-state-path).
    --session-dir <path>      Host the session server rooted here
                              (default: serving off — the panel's own
                              session co-presence detects the WS port).
    --session-port <n>        Session WebSocket port (default: the
                              session host's 9770).
    --held-by <name>          Session holder name (default
                              \"torogao-companion\").
    --state-file <path>       Placement state file (default: the
                              per-user state dir's companion.json).
    --height <px>             Display height override (persisted
                              bounds still apply).
    --wayland                 Force the Wayland layer-shell route.
    --fallback                Force the winit fallback route.
    --frames <n>              Present n frames, then exit (evidence).
    --screenshot <path>       With --frames: PNG of the final frame.
    --report <path>           With --frames: JSON evidence report.
    --click-through-lock <on|off>
                              Set the persisted click-through lock and
                              save it before running (the way to lift
                              the lock from outside the window).
    --trace-check <frames>    Headless determinism check: trace the
                              Daydream performance 3x through the
                              companion core; exit 0 iff all hashes
                              agree. Prints the seam report too.
    --print-state-path        Print the default state path and exit.
    --help                    This text.
";

struct Cli {
    model: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    session_port: Option<u16>,
    held_by: String,
    state_file: Option<PathBuf>,
    height: Option<u32>,
    force: Option<Force>,
    frames: Option<u64>,
    screenshot: Option<PathBuf>,
    report: Option<PathBuf>,
    click_through_lock: Option<bool>,
    trace_check: Option<usize>,
    print_state_path: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Force {
    Wayland,
    Fallback,
}

fn parse_cli(args: &[String]) -> Result<Cli, CompanionError> {
    let mut cli = Cli {
        model: None,
        session_dir: None,
        session_port: None,
        held_by: "torogao-companion".to_string(),
        state_file: None,
        height: None,
        force: None,
        frames: None,
        screenshot: None,
        report: None,
        click_through_lock: None,
        trace_check: None,
        print_state_path: false,
    };
    let mut index = 0_usize;
    let take = |index: &mut usize, flag: &str| -> Result<String, CompanionError> {
        *index += 1;
        args.get(*index)
            .cloned()
            .ok_or_else(|| CompanionError::new("TG-COMP-CLI", format!("{flag} needs a value")))
    };
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--model" => cli.model = Some(PathBuf::from(take(&mut index, arg)?)),
            "--session-dir" => {
                cli.session_dir = Some(PathBuf::from(take(&mut index, arg)?));
            }
            "--session-port" => {
                let value = take(&mut index, arg)?;
                cli.session_port = Some(value.parse::<u16>().map_err(|_| {
                    CompanionError::new("TG-COMP-CLI", format!("--session-port: '{value}'"))
                })?);
            }
            "--held-by" => cli.held_by = take(&mut index, arg)?,
            "--state-file" => {
                cli.state_file = Some(PathBuf::from(take(&mut index, arg)?));
            }
            "--height" => {
                let value = take(&mut index, arg)?;
                cli.height = Some(value.parse::<u32>().map_err(|_| {
                    CompanionError::new("TG-COMP-CLI", format!("--height: '{value}'"))
                })?);
            }
            "--wayland" => cli.force = Some(Force::Wayland),
            "--fallback" => cli.force = Some(Force::Fallback),
            "--frames" => {
                let value = take(&mut index, arg)?;
                cli.frames = Some(value.parse::<u64>().map_err(|_| {
                    CompanionError::new("TG-COMP-CLI", format!("--frames: '{value}'"))
                })?);
            }
            "--screenshot" => {
                cli.screenshot = Some(PathBuf::from(take(&mut index, arg)?));
            }
            "--report" => cli.report = Some(PathBuf::from(take(&mut index, arg)?)),
            "--click-through-lock" => {
                let value = take(&mut index, arg)?;
                cli.click_through_lock = Some(match value.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => {
                        return Err(CompanionError::new(
                            "TG-COMP-CLI",
                            format!("--click-through-lock: '{value}' (want on|off)"),
                        ));
                    }
                });
            }
            "--trace-check" => {
                let value = take(&mut index, arg)?;
                cli.trace_check = Some(value.parse::<usize>().map_err(|_| {
                    CompanionError::new("TG-COMP-CLI", format!("--trace-check: '{value}'"))
                })?);
            }
            "--print-state-path" => cli.print_state_path = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => {
                return Err(CompanionError::new(
                    "TG-COMP-CLI",
                    format!("unknown argument '{other}' (see --help)"),
                ));
            }
        }
        index += 1;
    }
    Ok(cli)
}

/// The menu header name from the model file: the cast
/// member's name is the fixture stem with the repertoire
/// suffix removed (`prima-repertoire.inx` → `Prima`).
fn character_name(model: &std::path::Path) -> String {
    let stem = model
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("companion");
    let base = stem.strip_suffix("-repertoire").unwrap_or(stem);
    let mut chars = base.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Companion".to_string(),
    }
}

fn trace_check(model: &std::path::Path, frames: usize) -> CompanionResult<bool> {
    let mut hashes = Vec::new();
    let mut last = None;
    for _ in 0..3 {
        let run = run_trace(model, frames)?;
        hashes.push(run.trace_hash);
        last = Some(run);
    }
    let run = last.expect("three runs happened");
    let seam = seam_report(&run);
    let moving_fraction = if run.step_hashes.len() > 1 {
        run.moving_steps as f64 / (run.step_hashes.len() - 1) as f64
    } else {
        0.0
    };
    let report = serde_json::json!({
        "frames": frames,
        "trace_hashes": hashes.iter().map(|h| format!("{h:#018x}")).collect::<Vec<_>>(),
        "all_equal": hashes.windows(2).all(|pair| pair[0] == pair[1]),
        "final_state_hash": format!("{:#018x}", run.step_hashes.last().copied().unwrap_or(0)),
        "moving_steps": run.moving_steps,
        "moving_fraction": moving_fraction,
        "seam_ratio_max": seam.seam_ratio_max,
        "median_step_delta": seam.median_delta,
        "seam_deltas": seam.seam_deltas,
        "seam_steps": run.seam_steps,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
    );
    Ok(hashes.windows(2).all(|pair| pair[0] == pair[1]))
}

fn run() -> CompanionResult<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = parse_cli(&args)?;
    if cli.print_state_path {
        println!("{}", default_state_path()?.display());
        return Ok(0);
    }
    let model = cli
        .model
        .clone()
        .ok_or_else(|| CompanionError::new("TG-COMP-CLI", "--model is required (see --help)"))?;
    if let Some(frames) = cli.trace_check {
        let equal = trace_check(&model, frames)?;
        return Ok(if equal { 0 } else { 3 });
    }
    let state_path = match &cli.state_file {
        Some(path) => path.clone(),
        None => default_state_path()?,
    };
    let mut placement = match load_state(&state_path) {
        LoadOutcome::Loaded(state) => state,
        LoadOutcome::Missing(state) => state,
        LoadOutcome::Corrupt(state, err) => {
            // G7: a corrupt state file is a typed refusal
            // on stderr and a defaults run — never a crash,
            // never half-parsed placement.
            eprintln!("torogao-companion: {err}");
            state
        }
    };
    if let Some(height) = cli.height {
        placement.display_height = height;
    }
    if let Some(locked) = cli.click_through_lock {
        placement.click_through_locked = locked;
        placement.validate()?;
        save_state(&state_path, &placement)?;
    }
    placement.validate()?;
    let options = RunOptions {
        model: model.clone(),
        character_name: character_name(&model),
        state_path,
        placement,
        session_dir: cli.session_dir.clone(),
        session_port: cli.session_port,
        held_by: cli.held_by.clone(),
        frames: cli.frames,
        screenshot: cli.screenshot.clone(),
        report: cli.report.clone(),
    };
    let use_wayland = match cli.force {
        Some(Force::Wayland) => true,
        Some(Force::Fallback) => false,
        None => std::env::var_os("WAYLAND_DISPLAY").is_some(),
    };
    let summary = if use_wayland {
        torogao_companion::wayland::run(options)?
    } else {
        torogao_companion::fallback::run(options)?
    };
    if cli.frames.is_some() {
        let summary_json = serde_json::json!({
            "frames_presented": summary.frames_presented,
            "trace_hash": format!("{:#018x}", summary.trace_hash),
            "final_state_hash": format!("{:#018x}", summary.final_state_hash),
            "frame_interval_median_ms": summary.frame_interval_median_ms,
            "moving_steps": summary.moving_steps,
            "session_id": summary.session_id,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&summary_json).unwrap_or_else(|_| "{}".to_string())
        );
    }
    Ok(0)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("torogao-companion: {err}");
            ExitCode::from(1)
        }
    }
}
