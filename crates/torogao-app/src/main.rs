//! The torogao-app binary: the windowed shell by default,
//! the headless verification mode under `--headless-eval`,
//! and the session host under `--session-serve`.
//!
//! Usage:
//!   torogao-app [model.inx]
//!   torogao-app --headless-eval <model.inx>
//!       [--param name=value ...] [--render out.png]
//!       [--state-hash]
//!   torogao-app --session-serve <model.inx> --session-dir <dir>
//!       [--session-port <port>] [--held-by <name>]
//!   torogao-app --session-replay <record-dir>
//!   torogao-app --play-export <model.inx> --animation <name>
//!       --out <dir> [--force]
//!   torogao-app --persona-audit

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use torogao_app::error::AppError;
use torogao_app::headless::{HeadlessOptions, run_headless};
use torogao_app::scene::NamedAssignment;
use torogao_app::session_host::run_session_host;
use torogao_app::windowed::run_windowed;

const USAGE: &str = "usage: torogao-app [model.inx] \
                     [--session-dir <dir>] [--session-port <port>] [--held-by <name>]\n       torogao-app --headless-eval <model.inx> \
                     [--param name=value ...] [--render out.png] [--state-hash] \
                     [--debug-physics]\n       torogao-app --session-serve <model.inx> --session-dir <dir> \
                     [--session-port <port>] [--held-by <name>]\n       torogao-app --session-replay <record-dir>\n       torogao-app --play-export <model.inx> --animation <name> --out <dir> [--force]";

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(0)),
        Err(err) => {
            eprintln!("torogao-app: {err}");
            if matches!(err, AppError::Cli(_)) {
                eprintln!("{USAGE}");
            }
            ExitCode::from(3)
        }
    }
}

fn run() -> Result<i32, AppError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut headless = false;
    let mut session_serve = false;
    let mut session_replay: Option<PathBuf> = None;
    let mut session_dir: Option<PathBuf> = None;
    let mut session_port: Option<u16> = None;
    let mut held_by = "torogao-app session host".to_string();
    let mut model: Option<PathBuf> = None;
    let mut play_export = false;
    let mut persona_audit = false;
    let mut animation: Option<String> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut force = false;
    let mut options = HeadlessOptions::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--headless-eval" => headless = true,
            "--session-serve" => session_serve = true,
            "--session-replay" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    AppError::Cli("--session-replay requires a record directory".to_string())
                })?;
                session_replay = Some(PathBuf::from(value));
            }
            "--session-dir" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    AppError::Cli("--session-dir requires a directory argument".to_string())
                })?;
                session_dir = Some(PathBuf::from(value));
            }
            "--session-port" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    AppError::Cli("--session-port requires a port argument".to_string())
                })?;
                session_port = Some(value.parse().map_err(|_| {
                    AppError::Cli(format!("--session-port: '{value}' is not a port number"))
                })?);
            }
            "--held-by" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    AppError::Cli("--held-by requires a name argument".to_string())
                })?;
                held_by = value.clone();
            }
            "--state-hash" => options.state_hash = true,
            "--debug-physics" => options.debug_physics = true,
            "--persona-audit" => persona_audit = true,
            "--play-export" => play_export = true,
            "--animation" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    AppError::Cli("--animation requires a name argument".to_string())
                })?;
                animation = Some(value.clone());
            }
            "--out" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    AppError::Cli("--out requires a directory argument".to_string())
                })?;
                out_dir = Some(PathBuf::from(value));
            }
            "--force" => force = true,
            "--param" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    AppError::Cli("--param requires a name=value argument".to_string())
                })?;
                options.params.push(NamedAssignment::parse(value)?);
            }
            "--render" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| AppError::Cli("--render requires an output path".to_string()))?;
                options.render = Some(PathBuf::from(value));
            }
            other if other.starts_with("--") => {
                return Err(AppError::Cli(format!("unknown option '{other}'")));
            }
            other => {
                if model.is_some() {
                    return Err(AppError::Cli(format!(
                        "unexpected extra argument '{other}'"
                    )));
                }
                model = Some(PathBuf::from(other));
            }
        }
        index += 1;
    }
    if persona_audit {
        // The RP5 gate surface: the twelve readability
        // rules, parity, and accent distinctness over the
        // Persona model, as one JSON document. Exit 0
        // only when every check passes.
        let report = torogao_app::persona::audit_json();
        let passed = report["checks_passed"].as_u64().unwrap_or(0);
        let total = report["checks_total"].as_u64().unwrap_or(0);
        println!("{}", torogao_format::jsonfmt::to_d_json(&report));
        return Ok(if passed == total && total > 0 { 0 } else { 3 });
    }
    if let Some(record_dir) = session_replay {
        return match torogao_session::replay_record(&record_dir) {
            Ok(hash) => {
                println!("final_hash {hash}");
                Ok(0)
            }
            Err(r) => {
                eprintln!("REFUSAL {} {}", r.code, r.message);
                Ok(3)
            }
        };
    }
    if play_export {
        let (Some(model), Some(animation), Some(out_dir)) = (model, animation, out_dir) else {
            return Err(AppError::Cli(
                "--play-export requires a model path, --animation <name> and --out <dir>"
                    .to_string(),
            ));
        };
        return torogao_app::video_bridge::run_play_export(
            &torogao_app::video_bridge::BridgeOptions {
                model,
                animation,
                out_dir,
                force,
            },
        );
    }
    if session_serve {
        let Some(model) = model else {
            return Err(AppError::Cli(
                "--session-serve requires a model path".to_string(),
            ));
        };
        let Some(session_dir) = session_dir else {
            return Err(AppError::Cli(
                "--session-serve requires --session-dir".to_string(),
            ));
        };
        if headless || !options.params.is_empty() || options.render.is_some() || options.state_hash
        {
            return Err(AppError::Cli(
                "--session-serve does not combine with --headless-eval options".to_string(),
            ));
        }
        return run_session_host(&model, &session_dir, session_port, &held_by);
    }
    if headless {
        let Some(model) = model else {
            return Err(AppError::Cli(
                "--headless-eval requires a model path".to_string(),
            ));
        };
        options.model = model;
        run_headless(&options)
    } else {
        if !options.params.is_empty() || options.render.is_some() || options.state_hash {
            return Err(AppError::Cli(
                "--param/--render/--state-hash belong to --headless-eval".to_string(),
            ));
        }
        // Windowed co-presence: --session-dir (with an
        // optional --session-port / --held-by) attaches the
        // session server to the running window instead of
        // serving headless.
        let window_session = match session_dir {
            Some(dir) => Some(torogao_app::windowed::WindowSessionOptions {
                dir,
                port: session_port.unwrap_or(torogao_session::transport::DEFAULT_WS_PORT),
                held_by: held_by.clone(),
            }),
            None => {
                if session_port.is_some() {
                    return Err(AppError::Cli(
                        "--session-port requires --session-dir (windowed) or --session-serve"
                            .to_string(),
                    ));
                }
                None
            }
        };
        run_windowed(model.as_deref(), window_session)
    }
}
