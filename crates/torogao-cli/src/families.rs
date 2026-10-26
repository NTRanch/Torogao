//! Tool-family dispatch for the non-core registry tools,
//! plus the shared layer helpers the CLI tail uses
//! (viseme baking, ambient driver configs). Each family
//! lands here as its port completes; a tool with no
//! ported handler yet fails as `internal_error`, never
//! as a fabricated success.

use crate::curves::CurveSet;
use crate::registry::ToolResult;
use serde_json::Value;

/// How a CLI layer file can fail: a usage-class problem
/// (stderr, exit 3) or a bake failure (error document,
/// exit 3).
pub enum LayerError {
    /// Usage-class failure; message goes to stderr.
    Usage(String),
    /// Bake failure; message goes into an error document.
    Bake(String),
}

/// Loads a viseme contract and bakes a viseme track into
/// curves (the CLI tail's --viseme layer).
pub fn bake_viseme_layer(
    _viseme_path: &str,
    _contract_path: &str,
) -> Result<CurveSet, LayerError> {
    Err(LayerError::Bake(
        "viseme baking is not ported yet".to_string(),
    ))
}

/// Parses an ambient driver configuration document.
pub fn parse_driver_config(_doc: &Value) -> Result<Value, String> {
    Err("ambient drivers are not ported yet".to_string())
}

/// Dispatches a non-core tool by name.
pub fn dispatch_family(name: &str, _args: &Value) -> ToolResult {
    ToolResult::fail(
        "internal_error",
        &format!("tool '{name}' is not ported yet"),
    )
}
