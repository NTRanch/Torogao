//! The rmcp server for `torogao-cli mcp`: the tool registry
//! surface and handler dispatch.
//!
//! Domain logic does not move in the WS2-W5 migration — every
//! call delegates to the one registry (`registry::call_tool`),
//! exactly as the hand-rolled dispatcher did, and result shapes
//! are the baseline's: success payloads as canonical d-JSON text
//! content plus `structuredContent`; domain failures as `isError`
//! text carrying `{"error":{"code","message"}}`; the registry's
//! protocol-class failures (`unknown_tool`, `tool_disabled`,
//! `invalid_arguments`) as JSON-RPC `-32602` errors with the
//! stable code in `error.data.code`. Only the framing changes
//! sides of the seam: rmcp owns the envelope, the era guard (see
//! `mcp_guard`) owns the stdio front door, and this type owns
//! the surface.
//!
//! Modern-only posture lives in two places: the era guard
//! refuses `initialize` before the SDK sees it, and
//! [`ServerHandler::supported_protocol_versions`] pins this
//! handler to 2026-07-28, so `server/discover` advertises
//! exactly one version and the SDK validates every request's
//! `_meta` against it.
//!
//! `tools/list` parity note: the baseline's tool entries carried
//! the registry's `torogao:namespace` / `torogao:posture` (and,
//! for disabled tools, `torogao:gate`) keys inside `annotations`.
//! rmcp's `ToolAnnotations` is a closed type — it cannot carry
//! custom keys — so those three keys ride in the tool's `_meta`
//! instead, with the four standard hints in `annotations`
//! carrying the baseline's exact values. No information is lost;
//! its location moves (itemized in the WS2-W5 gate report).

use std::borrow::Cow;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorCode,
    Implementation, InitializeResult, ListToolsResult, MetaObject, PaginatedRequestParams,
    ProtocolVersion, ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Value, json};
use torogao_format::jsonfmt::to_d_json;

use crate::registry;
use crate::registry_data::ToolMeta;

/// The registry's protocol-class error codes: the baseline
/// answered these as JSON-RPC `-32602` protocol errors; every
/// other registry failure is a domain `isError` tool result.
const PROTOCOL_ERROR_CODES: [&str; 3] = ["unknown_tool", "tool_disabled", "invalid_arguments"];

/// One registry entry as an rmcp `Tool`: the baseline's
/// hand-written schema served raw, so the schema delta is zero
/// by construction.
fn tool_definition(tool: &ToolMeta) -> Tool {
    let schema: Value = serde_json::from_str(tool.input_schema_json).unwrap_or(Value::Null);
    let Value::Object(schema_map) = schema else {
        unreachable!("registry schemas are objects");
    };
    let mut definition = Tool::new(
        tool.name.to_string(),
        tool.description.to_string(),
        Arc::new(schema_map),
    );
    definition.title = Some(tool.title.to_string());
    definition.annotations = Some(ToolAnnotations::from_raw(
        None,
        Some(tool.posture == "read-only"),
        Some(false),
        Some(tool.posture != "export"),
        Some(false),
    ));
    let mut meta = serde_json::Map::new();
    meta.insert(
        "torogao:namespace".to_string(),
        Value::String(tool.namespace.to_string()),
    );
    meta.insert(
        "torogao:posture".to_string(),
        Value::String(tool.posture.to_string()),
    );
    if tool.posture == "disabled" {
        meta.insert(
            "torogao:gate".to_string(),
            Value::String(tool.gate.to_string()),
        );
    }
    definition.meta = Some(MetaObject(meta));
    definition
}

/// The baseline's dispatch, unchanged: one registry call, with
/// the result re-shaped into the baseline's wire classes.
fn call_tool(name: &str, arguments: &Value) -> Result<CallToolResult, ErrorData> {
    let result = registry::call_tool(name, arguments);
    if result.is_error {
        if PROTOCOL_ERROR_CODES.contains(&result.error_code.as_str()) {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                result.error_message.clone(),
                Some(json!({"code": result.error_code})),
            ));
        }
        let content = vec![ContentBlock::text(to_d_json(&result.error_json()))];
        return Ok(CallToolResult::error(content));
    }
    let content = vec![ContentBlock::text(to_d_json(&result.payload))];
    let mut outcome = CallToolResult::success(content);
    outcome.structured_content = Some(result.payload);
    Ok(outcome)
}

/// The server handler. The baseline served sequentially; the
/// registry is stateless per call (handlers read project files
/// from disk), so dispatch runs on a blocking thread and calls
/// may proceed concurrently without shared state.
pub struct CliServer;

impl ServerHandler for CliServer {
    fn get_info(&self) -> InitializeResult {
        // The baseline's serverInfo: name "torogao", title
        // "Torogao", version = the registry version (a domain
        // constant, 5.0.0 — not the crate version).
        let mut server_info = Implementation::default();
        server_info.name = "torogao".to_string();
        server_info.title = Some("Torogao".to_string());
        server_info.version = registry::REGISTRY_VERSION.to_string();
        let mut info = InitializeResult::default();
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = server_info;
        info
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: registry::list_tools()
                .iter()
                .map(|t| tool_definition(t))
                .collect(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let outcome = tokio::task::spawn_blocking(move || call_tool(&name, &arguments)).await;
        match outcome {
            Ok(result) => result.map(CallToolResponse::from),
            Err(join) => Err(ErrorData::internal_error(
                format!("tool task failed: {join}"),
                None,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_cover_the_registry_with_baseline_hints() {
        let tools = registry::list_tools();
        assert_eq!(tools.len(), 57);
        for tool in &tools {
            let definition = tool_definition(tool);
            assert_eq!(definition.name.as_ref(), tool.name);
            assert_eq!(definition.title.as_deref(), Some(tool.title));
            let annotations = definition.annotations.expect("annotations");
            assert_eq!(
                annotations.read_only_hint,
                Some(tool.posture == "read-only")
            );
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.idempotent_hint, Some(tool.posture != "export"));
            assert_eq!(annotations.open_world_hint, Some(false));
            let meta = definition.meta.expect("meta");
            assert_eq!(meta["torogao:namespace"], json!(tool.namespace));
            assert_eq!(meta["torogao:posture"], json!(tool.posture));
            assert_eq!(
                meta.contains_key("torogao:gate"),
                tool.posture == "disabled"
            );
            // The schema is the baseline's own, byte-for-byte as a
            // canonical value.
            let baseline_schema: Value =
                serde_json::from_str(tool.input_schema_json).expect("schema parses");
            assert_eq!(
                Value::Object((*definition.input_schema).clone()),
                baseline_schema
            );
        }
    }

    #[test]
    fn protocol_error_classes_map_to_invalid_params_with_code_data() {
        let outcome = call_tool("no-such-tool", &json!({}));
        let err = outcome.expect_err("unknown tool is a protocol error");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(err.data, Some(json!({"code": "unknown_tool"})));
        let outcome = call_tool("apply-fixes", &json!({"project": "x.inx"}));
        let err = outcome.expect_err("disabled tool is a protocol error");
        assert_eq!(err.data, Some(json!({"code": "tool_disabled"})));
    }

    #[test]
    fn domain_failures_are_is_error_results() {
        let outcome = call_tool("inspect", &json!({"project": "/nonexistent/nope.inx"}))
            .expect("domain failure is a result, not a protocol error");
        assert_eq!(outcome.is_error, Some(true));
        assert_eq!(outcome.structured_content, None);
    }
}
