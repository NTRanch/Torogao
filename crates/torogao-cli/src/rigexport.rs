//! The rig exporter (rigexport.d port): converts a
//! loaded puppet into the rig model document, with a
//! per-construct conversion report (mapped / approximated
//! / dropped, every entry reasoned).

use crate::extdoc::ExtView;
use crate::handlers::ToolLoad;
use crate::registry::ToolResult;
use serde_json::Value;

/// How an export can fail: the conversion itself, or the
/// file writes after it.
pub enum ExportError {
    /// The exporter refused or failed mid-conversion.
    Build(String),
    /// Writing an export file failed.
    Io(String),
}

/// Runs the export: builds the model, writes the model,
/// stamp, and report files, and returns the report
/// document.
pub fn export_rig_files(
    _load: &ToolLoad,
    _out_dir: &str,
    _base: &str,
    _ext_view: &ExtView,
) -> Result<Value, ExportError> {
    Err(ExportError::Build("export-rig is not ported yet".to_string()))
}

/// The registry tool's shape over `export_rig_files`.
pub fn export_rig(
    load: &ToolLoad,
    out_dir: &str,
    base: &str,
    ext_view: &ExtView,
) -> ToolResult {
    match export_rig_files(load, out_dir, base, ext_view) {
        Ok(report) => ToolResult::ok(report),
        Err(ExportError::Build(message)) => ToolResult::fail("export_failed", &message),
        Err(ExportError::Io(message)) => ToolResult::fail("io_error", &message),
    }
}
