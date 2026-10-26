//! The CLI boundary guards: guarded project loading
//! (every loader failure becomes one typed, path-free
//! TG-CONTAINER-MALFORMED message) and the output guard
//! (no silent overwrites; an output that is also an input
//! is refused unconditionally).

use crate::paths::canonical_for_compare;
use serde_json::Value;
use std::path::Path;
use torogao_engine::Puppet;
use torogao_format::{InpFile, PayloadDoc, TorogaoError, decode_payload, read_inp};

/// Stable code for a container the loader rejects.
pub const TG_CONTAINER_MALFORMED: &str = "TG-CONTAINER-MALFORMED";
/// Stable code: output exists.
pub const TG_OUT_EXISTS: &str = "TG-OUT-EXISTS";
/// Stable code: output is an input.
pub const TG_OUT_IS_INPUT: &str = "TG-OUT-IS-INPUT";

/// A guard refusal: stable code plus a human-readable
/// reason.
#[derive(Debug, Clone)]
pub struct OutRefusal {
    /// Stable machine code.
    pub code: &'static str,
    /// Human-readable reason.
    pub message: String,
}

/// Guards one output file path (the reference
/// `guardOutput`).
pub fn guard_output(out_path: &str, inputs: &[&str], force: bool) -> Result<(), OutRefusal> {
    let cout = canonical_for_compare(Path::new(out_path));
    for input in inputs {
        if input.is_empty() {
            continue;
        }
        if canonical_for_compare(Path::new(input)) == cout {
            return Err(OutRefusal {
                code: TG_OUT_IS_INPUT,
                message: format!(
                    "output path '{out_path}' is also an input ('{input}'); \
                     refusing to overwrite an input, unconditionally"
                ),
            });
        }
    }
    if !force && Path::new(out_path).exists() {
        return Err(OutRefusal {
            code: TG_OUT_EXISTS,
            message: format!(
                "output path '{out_path}' already exists; refusing to \
                 overwrite (pass --force to allow it)"
            ),
        });
    }
    Ok(())
}

/// Guards every file an operation writes into an output
/// directory (the reference `guardOutputFiles`).
pub fn guard_output_files(
    out_dir: &str,
    file_names: &[&str],
    inputs: &[&str],
    force: bool,
) -> Result<(), OutRefusal> {
    for name in file_names {
        let path = Path::new(out_dir).join(name);
        guard_output(&path.to_string_lossy(), inputs, force)?;
    }
    Ok(())
}

/// A fully loaded project: container, payload document,
/// engine puppet, and the raw payload value (criteria read
/// the payload separately, as the reference does).
pub struct LoadedProject {
    /// The container.
    pub inp: InpFile,
    /// The decoded payload document.
    pub doc: PayloadDoc,
    /// The loaded puppet.
    pub puppet: Puppet,
    /// The raw payload JSON (null when unextractable).
    pub payload: Value,
}

/// Loads a project with the load guard: any failure is a
/// TG-CONTAINER-MALFORMED TorogaoError with a path-free
/// structural detail.
pub fn load_project(path: &str) -> Result<LoadedProject, TorogaoError> {
    let bytes = std::fs::read(path).map_err(|err| {
        TorogaoError::new(
            TG_CONTAINER_MALFORMED,
            format!(
                "project could not be loaded: malformed INP container (unreadable file: {})",
                err.kind()
            ),
        )
    })?;
    let inp = read_inp(&bytes)?;
    let doc = decode_payload(&inp.payload)?;
    let puppet = Puppet::load(&doc, &inp)?;
    let payload = payload_guarded(path);
    Ok(LoadedProject {
        inp,
        doc,
        puppet,
        payload,
    })
}

/// Loads only the puppet (guarded), for callers that do
/// not need the payload split.
pub fn load_puppet_guarded(path: &str) -> Result<Puppet, TorogaoError> {
    Ok(load_project(path)?.puppet)
}

/// The INP payload of a file, or null when it cannot be
/// extracted (never fails).
pub fn payload_guarded(path: &str) -> Value {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Value::Null,
    };
    if bytes.len() < 12 || &bytes[0..8] != b"TRNSRTS\0" {
        return Value::Null;
    }
    let payload_len = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if 12 + payload_len > bytes.len() {
        return Value::Null;
    }
    match std::str::from_utf8(&bytes[12..12 + payload_len]) {
        Ok(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

/// The file's payload as the format layer parses it
/// (used by container-level reads that must tolerate a
/// payload the engine would reject).
pub fn read_container(path: &str) -> Result<InpFile, TorogaoError> {
    let bytes = std::fs::read(path).map_err(|err| {
        TorogaoError::new(
            TG_CONTAINER_MALFORMED,
            format!(
                "project could not be loaded: malformed INP container (unreadable file: {})",
                err.kind()
            ),
        )
    })?;
    read_inp(&bytes)
}

/// The tool layer's `toolForce`: true only when the
/// arguments carry `"force": true`.
pub fn tool_force(args: &Value) -> bool {
    matches!(args.get("force"), Some(Value::Bool(true)))
}

#[cfg(test)]
mod tests {
    use super::{guard_output, payload_guarded};

    #[test]
    fn guard_refuses_output_that_is_an_input_even_with_force() {
        let refusal = guard_output("/tmp/x.json", &["/tmp/x.json"], true).unwrap_err();
        assert_eq!(refusal.code, "TG-OUT-IS-INPUT");
    }

    #[test]
    fn payload_guarded_reads_null_for_missing_files() {
        assert!(payload_guarded("/nonexistent/file.inx").is_null());
    }
}
