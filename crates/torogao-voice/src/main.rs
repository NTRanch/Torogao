//! The `torogao-voice` command line: grammar generation,
//! coverage audit, and one-shot voice-command runs (from
//! text, or from a WAV through the local STT adapter).
//!
//! Exit codes: 0 the command executed (or the audit is
//! clean); 1 a typed refusal; 2 usage error; 3 the command
//! is held for confirmation (re-run with `--confirm`).
//!
//! Nothing here plays sound: feedback is the JSON report on
//! stdout, per the estate's audio silence rule.

#![forbid(unsafe_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use torogao_voice::dispatch::{self, ExecReport, ExecStatus, Session};
use torogao_voice::grammar::GrammarDoc;
use torogao_voice::parse::{ParseOutcome, Transcript};
use torogao_voice::stt::{ScriptedEngine, SttEngine, WhisperCppEngine};

/// Parsed command-line options for the run-shaped commands.
#[derive(Default)]
struct Opts {
    text: Option<String>,
    wav: Option<PathBuf>,
    whisper_bin: Option<PathBuf>,
    model: Option<PathBuf>,
    confidence: Option<f32>,
    confirm: bool,
    root: Option<PathBuf>,
    project: Option<String>,
    grammar: Option<PathBuf>,
    out: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args, &mut std::io::stdout().lock());
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}

/// Runs one invocation; returns the process exit code.
fn run(args: &[String], out: &mut dyn Write) -> i32 {
    let Some(command) = args.first() else {
        usage(out);
        return 2;
    };
    let opts = parse_opts(&args[1..]);
    match command.as_str() {
        "gen-grammar" => gen_grammar(&opts, out),
        "coverage" => coverage(&opts, out),
        "parse" => parse_only(&opts, out),
        "run" => run_command(&opts, out),
        _ => {
            usage(out);
            2
        }
    }
}

/// Prints the usage document.
fn usage(out: &mut dyn Write) {
    let doc = serde_json::json!({
        "usage": [
            "torogao-voice gen-grammar [--out <file>]",
            "torogao-voice coverage [--grammar <file>]",
            "torogao-voice parse --text <utterance> [--confidence <0..1>] [--grammar <file>]",
            "torogao-voice run --text <utterance> [--confirm] [--root <dir>] [--project <path>]",
            "torogao-voice run --wav <file> --whisper-bin <bin> --model <ggml> [--confirm] [--root <dir>]",
        ],
    });
    let _ = writeln!(out, "{doc}");
}

/// Parses the flag tail shared by the subcommands.
fn parse_opts(args: &[String]) -> Opts {
    let mut opts = Opts::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--text" => opts.text = take_string(args, &mut i),
            "--confidence" => {
                let raw = take_string(args, &mut i);
                opts.confidence = raw.and_then(|text| text.parse::<f32>().ok());
            }
            "--confirm" => opts.confirm = true,
            "--wav" => opts.wav = take_path(args, &mut i),
            "--whisper-bin" => opts.whisper_bin = take_path(args, &mut i),
            "--model" => opts.model = take_path(args, &mut i),
            "--root" => opts.root = take_path(args, &mut i),
            "--project" => opts.project = take_string(args, &mut i),
            "--grammar" => opts.grammar = take_path(args, &mut i),
            "--out" => opts.out = take_path(args, &mut i),
            _ => {}
        }
        i += 1;
    }
    opts
}

/// Takes the next argument as a string.
fn take_string(args: &[String], i: &mut usize) -> Option<String> {
    if *i + 1 < args.len() {
        *i += 1;
        Some(args[*i].clone())
    } else {
        None
    }
}

/// Takes the next argument as a path.
fn take_path(args: &[String], i: &mut usize) -> Option<PathBuf> {
    if *i + 1 < args.len() {
        *i += 1;
        Some(PathBuf::from(&args[*i]))
    } else {
        None
    }
}

/// Loads the grammar: the `--grammar` file when named, else
/// the embedded checked-in document.
fn load_grammar(opts: &Opts) -> Result<GrammarDoc, String> {
    match &opts.grammar {
        Some(path) => torogao_voice::load_grammar_file(path).map_err(|err| err.to_string()),
        None => torogao_voice::embedded_grammar().map_err(|err| err.to_string()),
    }
}

/// Writes one JSON document line.
fn emit(out: &mut dyn Write, doc: &serde_json::Value) {
    let _ = writeln!(out, "{doc}");
}

/// `gen-grammar`: regenerate the grammar document from its
/// sources of truth.
fn gen_grammar(opts: &Opts, out: &mut dyn Write) -> i32 {
    let doc = torogao_voice::generate::build_grammar();
    let text = doc.to_canonical_json();
    if let Some(path) = &opts.out {
        if let Err(err) = std::fs::write(path, &text) {
            emit(
                out,
                &serde_json::json!({"error": format!("write failed: {err}")}),
            );
            return 1;
        }
    }
    emit(
        out,
        &serde_json::json!({
            "entries": doc.entries.len(),
            "written": opts.out.as_ref().map(|path| path.display().to_string()),
        }),
    );
    0
}

/// `coverage`: audit a grammar document against the live
/// sources of truth.
fn coverage(opts: &Opts, out: &mut dyn Write) -> i32 {
    let grammar = match load_grammar(opts) {
        Ok(grammar) => grammar,
        Err(err) => {
            emit(out, &serde_json::json!({"error": err}));
            return 2;
        }
    };
    let report = torogao_voice::audit::coverage_report(&grammar);
    emit(out, &serde_json::to_value(&report).unwrap_or_default());
    if report.ok { 0 } else { 1 }
}

/// Obtains the transcript for a run: a WAV through the
/// whisper.cpp adapter, or `--text` through the scripted
/// engine (the same parse path either way).
fn obtain_transcript(opts: &Opts) -> Result<Transcript, String> {
    if let Some(wav) = &opts.wav {
        let bin = opts
            .whisper_bin
            .clone()
            .ok_or("run --wav needs --whisper-bin <whisper-cli>")?;
        let model = opts.model.clone().ok_or("run --wav needs --model <ggml>")?;
        let engine = WhisperCppEngine::new(bin, model);
        return engine.transcribe(wav).map_err(|err| err.to_string());
    }
    let text = opts.text.clone().ok_or("run needs --text or --wav")?;
    let engine = ScriptedEngine {
        text,
        confidence: opts.confidence,
    };
    engine
        .transcribe(Path::new(""))
        .map_err(|err| err.to_string())
}

/// `parse`: parse only; print the outcome; never execute.
fn parse_only(opts: &Opts, out: &mut dyn Write) -> i32 {
    let grammar = match load_grammar(opts) {
        Ok(grammar) => grammar,
        Err(err) => {
            emit(out, &serde_json::json!({"error": err}));
            return 2;
        }
    };
    let transcript = match obtain_transcript(opts) {
        Ok(transcript) => transcript,
        Err(err) => {
            emit(out, &serde_json::json!({"error": err}));
            return 2;
        }
    };
    let outcome = torogao_voice::parse::parse(&grammar, &transcript);
    emit(out, &serde_json::to_value(&outcome).unwrap_or_default());
    match outcome {
        ParseOutcome::Refusal(_) => 1,
        _ => 0,
    }
}

/// `run`: parse, then dispatch. Confirm-class commands are
/// held unless `--confirm` is given.
fn run_command(opts: &Opts, out: &mut dyn Write) -> i32 {
    let grammar = match load_grammar(opts) {
        Ok(grammar) => grammar,
        Err(err) => {
            emit(out, &serde_json::json!({"error": err}));
            return 2;
        }
    };
    let transcript = match obtain_transcript(opts) {
        Ok(transcript) => transcript,
        Err(err) => {
            emit(out, &serde_json::json!({"error": err}));
            return 2;
        }
    };
    let outcome = torogao_voice::parse::parse(&grammar, &transcript);
    let report: ExecReport = match outcome {
        ParseOutcome::Stop => dispatch::stop_report(),
        ParseOutcome::Refusal(refusal) => ExecReport {
            status: ExecStatus::Refused,
            entry_id: String::new(),
            payload: serde_json::Value::Null,
            code: refusal.code.clone(),
            message: refusal.message.clone(),
        },
        ParseOutcome::Command(command) => {
            let root = opts.root.clone().unwrap_or_else(|| PathBuf::from("."));
            let mut session = Session::scan(&root);
            if let Some(project) = &opts.project {
                session = session.with_default_project(project);
            }
            dispatch::execute(&command, opts.confirm, &session)
        }
    };
    emit(out, &serde_json::to_value(&report).unwrap_or_default());
    match report.status {
        ExecStatus::Executed | ExecStatus::Stopped => 0,
        ExecStatus::ConfirmationRequired => 3,
        ExecStatus::Refused => 1,
    }
}
