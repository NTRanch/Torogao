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

/// Problems with one tool's input schema, stated for a
/// gate report; empty when the schema is a well-formed
/// object schema. Downstream consumers (the model server's
/// tool format, strict MCP clients) require `properties`
/// to be an object: the registry once shipped
/// `properties: null` on its two no-argument tools, and
/// every toolset-carrying request then failed at the
/// server. This is the check that keeps that defect class
/// out of the registry.
pub fn input_schema_problems(schema: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(object) = schema.as_object() else {
        return vec!["input schema is not a JSON object".to_string()];
    };
    if object.get("type").and_then(Value::as_str) != Some("object") {
        problems.push("input schema \"type\" is not \"object\"".to_string());
    }
    let properties = object.get("properties");
    match properties {
        Some(Value::Object(entries)) => {
            for (name, entry) in entries {
                if !entry.is_object() {
                    problems.push(format!("property '{name}' schema is not an object"));
                }
            }
        }
        _ => problems.push("input schema \"properties\" is not an object".to_string()),
    }
    if let Some(required) = object.get("required") {
        let Some(required) = required.as_array() else {
            problems.push("input schema \"required\" is not an array".to_string());
            return problems;
        };
        let declared = properties.and_then(Value::as_object);
        for entry in required {
            let Some(name) = entry.as_str() else {
                problems.push("input schema \"required\" holds a non-string".to_string());
                continue;
            };
            if declared.is_none_or(|props| !props.contains_key(name)) {
                problems.push(format!("required property '{name}' is not declared"));
            }
        }
    }
    problems
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
    use super::{call_tool, dump_json, input_schema_problems, list_tools};
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

    #[test]
    fn every_registered_input_schema_is_a_valid_object_schema() {
        for tool in list_tools() {
            let schema: serde_json::Value =
                serde_json::from_str(tool.input_schema_json).expect("schema parses");
            let problems = input_schema_problems(&schema);
            assert!(
                problems.is_empty(),
                "tool '{}' schema problems: {problems:?}",
                tool.name
            );
        }
    }

    #[test]
    fn input_schema_problems_flags_the_null_properties_defect_class() {
        // The historical defect: no-argument tools shipped
        // "properties": null, which strict consumers reject
        // at request time.
        let null_props = json!({"type": "object", "properties": null});
        assert!(!input_schema_problems(&null_props).is_empty());
        let array_props = json!({"type": "object", "properties": []});
        assert!(!input_schema_problems(&array_props).is_empty());
        let missing_props = json!({"type": "object"});
        assert!(!input_schema_problems(&missing_props).is_empty());
        let wrong_type = json!({"type": "array", "properties": {}});
        assert!(!input_schema_problems(&wrong_type).is_empty());
        let null_property = json!({"type": "object", "properties": {"x": null}});
        assert!(!input_schema_problems(&null_property).is_empty());
        let undeclared_required =
            json!({"type": "object", "properties": {}, "required": ["ghost"]});
        assert!(!input_schema_problems(&undeclared_required).is_empty());
        let not_an_object = json!(["properties"]);
        assert!(!input_schema_problems(&not_an_object).is_empty());
        // The cured shape: an empty properties object is a
        // valid no-argument schema.
        let cured = json!({"type": "object", "properties": {}});
        assert!(input_schema_problems(&cured).is_empty());
    }
}
