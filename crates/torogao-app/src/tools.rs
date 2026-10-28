//! Registry reachability from the app (RP4b): the full
//! tool registry, callable in-process through the CLI
//! crate's one handler table — the same dispatch the CLI,
//! MCP, and A2A surfaces serve. The D editor's panels did
//! exactly this through a process hop (spawn the CLI, show
//! the result verbatim); the app panel does it in-process.
//! Posture is the registry's own: read-only and export
//! tools run; the one disabled tool (`apply-fixes`)
//! surfaces its typed refusal, never a fake success — its
//! gate state is reported by `apply_fixes_gate_state`.

use serde_json::Value;
use torogao_cli::registry::{self, ToolResult};
use torogao_cli::registry_data::ToolMeta;

/// Every registered tool, in registry (sorted) order.
pub fn tools() -> Vec<&'static ToolMeta> {
    registry::list_tools()
}

/// One tool's registry entry, when it exists.
pub fn find(name: &str) -> Option<&'static ToolMeta> {
    registry::find_tool(name)
}

/// Calls a tool through the registry's handler table.
/// The result is the registry's own typed outcome —
/// payload on success, code + message on refusal.
pub fn call(name: &str, args: &Value) -> ToolResult {
    registry::call_tool(name, args)
}

/// The C01 disposition evidence: `apply-fixes`' registry
/// entry (posture + gate text) plus what a call returns
/// today. The §5.4 gate now exists (the AUTH-D2 decide()
/// the session surface runs), so the remaining blocker is
/// the one this reports verbatim: the registry's own
/// enablement has not been granted — the tool stays
/// disabled by design, and this surface never pretends
/// otherwise.
pub fn apply_fixes_gate_state() -> Value {
    let meta = registry::find_tool("apply-fixes");
    let probe = registry::call_tool("apply-fixes", &serde_json::json!({}));
    serde_json::json!({
        "tool": "apply-fixes",
        "posture": meta.map(|m| m.posture),
        "gate": meta.map(|m| m.gate),
        "call_is_error": probe.is_error,
        "call_error_code": probe.error_code,
        "call_error_message": probe.error_message,
        "write_gate_exists": true,
        "write_gate": "torogao_author::decide::decide, live in the session surface since Phase C",
    })
}

#[cfg(test)]
mod tests {
    use super::{apply_fixes_gate_state, call, tools};
    use serde_json::json;

    #[test]
    fn the_registry_is_fully_reachable_from_the_app() {
        let all = tools();
        assert_eq!(all.len(), 57);
        // A read-only tool answers in-process.
        let info = call("list-templates", &json!({}));
        assert!(
            !info.is_error,
            "list-templates refused: {}",
            info.error_message
        );
        // An unknown tool refuses typed, never panics.
        let unknown = call("no-such-tool", &json!({}));
        assert!(unknown.is_error);
        assert_eq!(unknown.error_code, "unknown_tool");
    }

    #[test]
    fn apply_fixes_surfaces_its_disabled_refusal() {
        let state = apply_fixes_gate_state();
        assert_eq!(state["posture"], json!("disabled"));
        assert_eq!(state["call_is_error"], json!(true));
        assert_eq!(state["call_error_code"], json!("tool_disabled"));
        assert_eq!(state["write_gate_exists"], json!(true));
    }
}
