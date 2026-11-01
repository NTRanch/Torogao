//! End-to-end gate (design §8.5): a stand-in binary
//! honouring the whisper.cpp CLI/JSON contract feeds a
//! spoken-form command through the real adapter → parser →
//! registry path against a fixture project, and the tool's
//! payload comes back. Also covers the dispatch laws: the
//! confirmation hold, the registry's own refusal of the
//! disabled tool, `TG-VOICE-NOT-BOUND` for unbuilt surfaces,
//! and unresolved spoken references.
//!
//! The stand-in is a contract double, not the real engine:
//! whisper.cpp is not installed on primo yet, and the
//! real-engine bench is slice V2's gate (design §9).

use std::path::PathBuf;
use torogao_voice::dispatch::{ExecStatus, Session, execute, stop_report};
use torogao_voice::embedded_grammar;
use torogao_voice::parse::{ParseOutcome, Transcript, parse};
use torogao_voice::stt::{SttEngine, WhisperCppEngine};

/// A scratch directory unique to this test process.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("torogao-voice-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The workspace fixture root holding `rich.inx`.
fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// Writes the whisper.cpp contract double: a shell script
/// that answers `-of <prefix>` with a canned full-JSON
/// document, exactly where the real binary would put it.
fn fake_whisper(dir: &std::path::Path) -> PathBuf {
    let script = dir.join("whisper-cli");
    let body = r#"#!/bin/sh
prefix=""
prev=""
for a in "$@"; do
  if [ "$prev" = "-of" ]; then prefix="$a"; fi
  prev="$a"
done
cat > "$prefix.json" <<'JSON'
{"transcription":[{"text":" inspect project rich","tokens":[{"text":" inspect","p":0.98},{"text":" project","p":0.97},{"text":" rich","p":0.99}]}]}
JSON
exit 0
"#;
    std::fs::write(&script, body).expect("write fake whisper-cli");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake whisper-cli");
    }
    script
}

/// Parses an utterance at high confidence, expecting a
/// command.
fn command(text: &str) -> torogao_voice::parse::ParsedCommand {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    match parse(
        &grammar,
        &Transcript {
            text: text.to_string(),
            confidence: Some(0.95),
        },
    ) {
        ParseOutcome::Command(command) => command,
        other => panic!("expected command for {text:?}, got {other:?}"),
    }
}

#[test]
fn whisper_contract_double_drives_a_real_tool_call() {
    let dir = scratch("whisper");
    let bin = fake_whisper(&dir);
    let model = dir.join("ggml-tiny.en.bin");
    std::fs::write(&model, b"model placeholder").expect("model placeholder");
    let engine = WhisperCppEngine::new(bin, model);
    let transcript = engine
        .transcribe(&dir.join("utterance.wav"))
        .expect("contract double transcribes");
    assert_eq!(transcript.text, "inspect project rich");
    let confidence = transcript.confidence.expect("confidence reported");
    assert!(
        (confidence - 0.98).abs() < 0.01,
        "mean token p: {confidence}"
    );

    let grammar = embedded_grammar().expect("embedded grammar parses");
    let ParseOutcome::Command(parsed) = parse(&grammar, &transcript) else {
        panic!("expected a command from the transcript");
    };
    assert_eq!(parsed.entry_id, "tool.inspect");

    let session = Session::scan(&fixtures_root());
    let report = execute(&parsed, false, &session);
    assert_eq!(report.status, ExecStatus::Executed, "{}", report.message);
    assert!(
        report.payload.is_object(),
        "inspect returns its census document: {}",
        report.payload
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_engine_binary_is_a_typed_refusal() {
    let dir = scratch("missing");
    let engine = WhisperCppEngine::new(dir.join("no-such-whisper"), dir.join("no-model"));
    let err = engine
        .transcribe(&dir.join("x.wav"))
        .expect_err("absent binary refuses");
    assert_eq!(err.code, "TG-VOICE-ENGINE-MISSING");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_is_held_until_confirmed_then_executes() {
    let dir = scratch("export");
    let out_stem = dir.join("spoken-export");
    let utterance = format!("export rig project rich out {}", out_stem.display());
    let parsed = command(&utterance);
    let session = Session::scan(&fixtures_root());

    let held = execute(&parsed, false, &session);
    assert_eq!(held.status, ExecStatus::ConfirmationRequired);

    let done = execute(&parsed, true, &session);
    assert_eq!(done.status, ExecStatus::Executed, "{}", done.message);
    assert!(done.payload.is_object());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn disabled_tool_is_refused_by_the_registry_itself() {
    let parsed = command("apply fixes project rich");
    let session = Session::scan(&fixtures_root());
    let report = execute(&parsed, false, &session);
    assert_eq!(report.status, ExecStatus::Refused);
    assert_eq!(report.code, "tool_disabled");
}

#[test]
fn unbuilt_surfaces_refuse_not_bound() {
    let session = Session::default();
    let verb = execute(&command("author keyform set"), true, &session);
    assert_eq!(verb.status, ExecStatus::Refused);
    assert_eq!(verb.code, "TG-VOICE-NOT-BOUND");

    let app = execute(&command("open settings"), false, &session);
    assert_eq!(app.status, ExecStatus::Refused);
    assert_eq!(app.code, "TG-VOICE-NOT-BOUND");

    let mode = execute(&command("start mcp server"), true, &session);
    assert_eq!(mode.status, ExecStatus::Refused);
    assert_eq!(mode.code, "TG-VOICE-NOT-BOUND");
}

#[test]
fn unresolved_project_name_refuses() {
    let parsed = command("inspect project zzz-no-such-project");
    let report = execute(&parsed, false, &Session::default());
    assert_eq!(report.status, ExecStatus::Refused);
    assert_eq!(report.code, "TG-VOICE-UNRESOLVED-REF");
}

#[test]
fn bound_cli_modes_execute() {
    let session = Session::default();
    let dump = execute(&command("dump registry"), false, &session);
    assert_eq!(dump.status, ExecStatus::Executed, "{}", dump.message);
    assert_eq!(dump.payload["version"], "5.0.0");
    let tools = dump.payload["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 57);

    let paths = execute(&command("show paths"), false, &session);
    assert_eq!(paths.status, ExecStatus::Executed, "{}", paths.message);
}

#[test]
fn stop_report_shape() {
    let report = stop_report();
    assert_eq!(report.status, ExecStatus::Stopped);
}
