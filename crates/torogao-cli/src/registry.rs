//! The tool registry: the single tool layer for every
//! surface (CLI, MCP, A2A). Listings sort by tool name;
//! unknown names and disabled tools are typed failures; a
//! handler failure is contained as `internal_error`.

use crate::handlers;
use crate::registry_data::{TOOL_META, ToolMeta};
use serde_json::{Value, json};

/// The agent tool surface version.
pub const REGISTRY_VERSION: &str = "5.0.0";

/// Outcome of a tool call: a payload, or a typed failure.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Whether the call failed.
    pub is_error: bool,
    /// Stable machine code when failed.
    pub error_code: String,
    /// Human-readable detail when failed.
    pub error_message: String,
    /// Structured result when ok.
    pub payload: Value,
}

impl ToolResult {
    /// A successful result.
    pub fn ok(payload: Value) -> ToolResult {
        ToolResult {
            is_error: false,
            error_code: String::new(),
            error_message: String::new(),
            payload,
        }
    }

    /// A typed failure.
    pub fn fail(code: &str, message: &str) -> ToolResult {
        ToolResult {
            is_error: true,
            error_code: code.to_string(),
            error_message: message.to_string(),
            payload: Value::Null,
        }
    }

    /// The failure as a JSON object (surface error
    /// payloads).
    pub fn error_json(&self) -> Value {
        json!({
            "error": {
                "code": self.error_code,
                "message": self.error_message,
            }
        })
    }
}

/// All tool specs, sorted by name.
pub fn list_tools() -> Vec<&'static ToolMeta> {
    let mut tools: Vec<&ToolMeta> = TOOL_META.iter().collect();
    tools.sort_by_key(|tool| tool.name);
    tools
}

/// One tool's spec as JSON (registry dumps, cards).
pub fn spec_json(tool: &ToolMeta) -> Value {
    let mut doc = json!({
        "name": tool.name,
        "namespace": tool.namespace,
        "title": tool.title,
        "description": tool.description,
        "inputSchema": serde_json::from_str::<Value>(tool.input_schema_json)
            .unwrap_or(Value::Null),
        "posture": tool.posture,
    });
    if !tool.gate.is_empty() {
        doc["gate"] = Value::String(tool.gate.to_string());
    }
    doc
}

/// The whole registry as JSON (the dump the gates diff
/// against).
pub fn dump_json() -> Value {
    let tools: Vec<Value> = list_tools().iter().map(|tool| spec_json(tool)).collect();
    json!({
        "version": REGISTRY_VERSION,
        "tools": tools,
    })
}

/// Finds one tool's metadata by name.
pub fn find_tool(name: &str) -> Option<&'static ToolMeta> {
    TOOL_META.iter().find(|tool| tool.name == name)
}

/// Calls a tool by name through the one handler table.
pub fn call_tool(name: &str, args: &Value) -> ToolResult {
    let Some(tool) = find_tool(name) else {
        return ToolResult::fail(
            "unknown_tool",
            &format!("no tool named '{name}' is registered"),
        );
    };
    if tool.posture == "disabled" {
        return ToolResult::fail(
            "tool_disabled",
            &format!(
                "tool '{}' is declared but disabled; gate: {}",
                name, tool.gate
            ),
        );
    }
    handlers::dispatch(name, args)
}

/// Pulls a required non-empty string argument out of a
/// tool call (the reference `stringArg`).
pub fn string_arg(args: &Value, key: &str) -> Result<String, ToolResult> {
    if !args.is_object() {
        return Err(ToolResult::fail(
            "invalid_arguments",
            "arguments must be a JSON object",
        ));
    }
    match args.get(key) {
        Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
        _ => Err(ToolResult::fail(
            "invalid_arguments",
            &format!("argument '{key}' (non-empty string) is required"),
        )),
    }
}

/// Pulls an optional string argument (empty when absent).
pub fn opt_string_arg(args: &Value, key: &str) -> String {
    match args.get(key) {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{call_tool, dump_json, list_tools};
    use serde_json::json;

    #[test]
    fn registry_has_57_tools_sorted() {
        let tools = list_tools();
        assert_eq!(tools.len(), 57);
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        assert_eq!(dump_json()["version"], json!("5.0.0"));
    }

    #[test]
    fn unknown_and_disabled_tools_fail_typed() {
        let unknown = call_tool("nope", &json!({}));
        assert_eq!(unknown.error_code, "unknown_tool");
        let disabled = call_tool("apply-fixes", &json!({"project": "x.inx"}));
        assert_eq!(disabled.error_code, "tool_disabled");
        assert!(disabled.error_message.contains("mutation gate"));
    }
}
