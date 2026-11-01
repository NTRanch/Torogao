//! Speech recognition: local only, behind a trait.
//!
//! No cloud STT exists in this design: recognition is a local
//! model file and local compute. The primary engine is
//! whisper.cpp, driven as a bounded subprocess (separate argv,
//! drained pipes, a deadline, a reaped child — the estate's
//! OS-boundary rules). The trait is the seam the slice-V2
//! bench measures engines through, so a failed engine is
//! swapped, never debugged into place.

use crate::parse::Transcript;
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Refusal code: the configured engine binary is absent.
pub const TG_VOICE_ENGINE_MISSING: &str = "TG-VOICE-ENGINE-MISSING";
/// Refusal code: the engine ran and failed, or its output
/// could not be read.
pub const TG_VOICE_ENGINE_FAILED: &str = "TG-VOICE-ENGINE-FAILED";

/// Hard deadline for one transcription.
pub const TRANSCRIBE_DEADLINE: Duration = Duration::from_secs(120);
/// Bounded drain for the engine's stderr (diagnostics only).
const STDERR_BYTES_MAX: usize = 8192;

/// An STT failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceError {
    /// Stable `TG-VOICE-ENGINE-*` machine code.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for VoiceError {}

/// A local speech-recognition engine.
pub trait SttEngine {
    /// Transcribes one WAV file. Audio is read from disk by
    /// the engine; nothing is uploaded anywhere — there is no
    /// network path in this layer.
    fn transcribe(&self, wav: &Path) -> Result<Transcript, VoiceError>;
}

/// A scripted engine returning a fixed transcript. Used by
/// tests and by integrations that hand over recognized text;
/// it is the engine-double the end-to-end gate runs on until
/// the real engine is benched (slice V2).
#[derive(Debug, Clone)]
pub struct ScriptedEngine {
    /// The transcript text to report.
    pub text: String,
    /// The confidence to report.
    pub confidence: Option<f32>,
}

impl SttEngine for ScriptedEngine {
    fn transcribe(&self, _wav: &Path) -> Result<Transcript, VoiceError> {
        Ok(Transcript {
            text: self.text.clone(),
            confidence: self.confidence,
        })
    }
}

/// The whisper.cpp engine: spawns `whisper-cli` with the
/// full-JSON output flag and reads the result document.
///
/// Expected CLI contract (whisper.cpp):
/// `whisper-cli -m <model.ggml> -f <wav> -ojf -of <prefix>`
/// writes `<prefix>.json` with a `transcription` array of
/// segments carrying `text` and per-token `p` probabilities.
#[derive(Debug, Clone)]
pub struct WhisperCppEngine {
    /// Path to the `whisper-cli` binary.
    pub bin: PathBuf,
    /// Path to the ggml model file.
    pub model: PathBuf,
    /// Per-call deadline.
    pub deadline: Duration,
}

impl WhisperCppEngine {
    /// An engine with the standard deadline.
    pub fn new(bin: PathBuf, model: PathBuf) -> WhisperCppEngine {
        WhisperCppEngine {
            bin,
            model,
            deadline: TRANSCRIBE_DEADLINE,
        }
    }
}

impl SttEngine for WhisperCppEngine {
    fn transcribe(&self, wav: &Path) -> Result<Transcript, VoiceError> {
        if !self.bin.exists() {
            return Err(VoiceError {
                code: TG_VOICE_ENGINE_MISSING.to_string(),
                message: format!(
                    "whisper-cli not found at {} (slice V2 installs it on primo)",
                    self.bin.display()
                ),
            });
        }
        if !self.model.exists() {
            return Err(VoiceError {
                code: TG_VOICE_ENGINE_MISSING.to_string(),
                message: format!("whisper model not found at {}", self.model.display()),
            });
        }
        let scratch = std::env::temp_dir().join(format!("torogao-voice-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&scratch);
        let prefix = scratch.join("transcript");
        let mut child = Command::new(&self.bin)
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(wav)
            .arg("-ojf")
            .arg("-of")
            .arg(&prefix)
            .arg("-nt")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| VoiceError {
                code: TG_VOICE_ENGINE_FAILED.to_string(),
                message: format!("could not spawn whisper-cli: {err}"),
            })?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if started.elapsed() > self.deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(VoiceError {
                            code: TG_VOICE_ENGINE_FAILED.to_string(),
                            message: "whisper-cli exceeded its deadline and was killed".to_string(),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(err) => {
                    return Err(VoiceError {
                        code: TG_VOICE_ENGINE_FAILED.to_string(),
                        message: format!("whisper-cli wait failed: {err}"),
                    });
                }
            }
        };
        let mut stderr_text = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let mut buf = vec![0u8; STDERR_BYTES_MAX];
            let read = stderr.read(&mut buf).unwrap_or(0);
            stderr_text = String::from_utf8_lossy(&buf[..read]).to_string();
        }
        if !status.success() {
            return Err(VoiceError {
                code: TG_VOICE_ENGINE_FAILED.to_string(),
                message: format!(
                    "whisper-cli exited {status}: {}",
                    stderr_text.trim().chars().take(200).collect::<String>()
                ),
            });
        }
        let json_path = prefix.with_extension("json");
        let raw = std::fs::read_to_string(&json_path).map_err(|err| VoiceError {
            code: TG_VOICE_ENGINE_FAILED.to_string(),
            message: format!("whisper-cli wrote no JSON output: {err}"),
        })?;
        let _ = std::fs::remove_dir_all(&scratch);
        parse_whisper_json(&raw)
    }
}

/// Reads a whisper.cpp full-JSON document into a transcript:
/// segment texts concatenated; confidence = the mean token
/// probability over non-special tokens, when reported.
pub fn parse_whisper_json(raw: &str) -> Result<Transcript, VoiceError> {
    let doc: Value = serde_json::from_str(raw).map_err(|err| VoiceError {
        code: TG_VOICE_ENGINE_FAILED.to_string(),
        message: format!("whisper JSON unreadable: {err}"),
    })?;
    let segments = doc
        .get("transcription")
        .and_then(Value::as_array)
        .ok_or_else(|| VoiceError {
            code: TG_VOICE_ENGINE_FAILED.to_string(),
            message: "whisper JSON has no transcription array".to_string(),
        })?;
    let mut text = String::new();
    let mut prob_sum = 0.0f64;
    let mut prob_count = 0u64;
    for segment in segments {
        if let Some(segment_text) = segment.get("text").and_then(Value::as_str) {
            text.push_str(segment_text);
        }
        if let Some(tokens) = segment.get("tokens").and_then(Value::as_array) {
            for token in tokens {
                let token_text = token.get("text").and_then(Value::as_str).unwrap_or("");
                if token_text.starts_with("[_") {
                    continue;
                }
                if let Some(p) = token.get("p").and_then(Value::as_f64) {
                    prob_sum += p;
                    prob_count += 1;
                }
            }
        }
    }
    Ok(Transcript {
        text: text.trim().to_string(),
        confidence: if prob_count > 0 {
            Some((prob_sum / prob_count as f64) as f32)
        } else {
            None
        },
    })
}
