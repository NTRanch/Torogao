//! One real MCP exchange over the estate's rmcp path.
//!
//! The allowed flow is decided before dispatch. Only then does
//! an rmcp client connect, over an in-process duplex transport,
//! to an rmcp server whose one tool returns the rig census. The
//! violating flow is decided from its event record and never
//! reaches the client or server.

use std::borrow::Cow;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities,
    Tool,
};
use rmcp::service::{ClientServiceExt, RequestContext};
use rmcp::{ClientLifecycleMode, ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Map, Value, json};

use crate::data::RigData;
use crate::error::{BakeoffError, Result};
use crate::policy::{GateDecision, allowed_event, decide, violating_event};

const TOOL_NAME: &str = "rig_census";
const DUPLEX_BUFFER_BYTES: usize = 64 * 1024;

/// The two policy outcomes and the one actual MCP result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpOutcome {
    /// Parameter count returned by the MCP tool.
    pub parameter_count: usize,
    /// Tool name returned over MCP.
    pub tool_name: String,
    /// Stable refusal code for the violating flow.
    pub refusal_code: String,
    /// The remedy action for the violating flow.
    pub refusal_action: String,
}

/// Runs the mediated MCP exchange synchronously.
///
/// # Errors
///
/// Returns a typed error if either decision is not the measured
/// scenario outcome, the runtime cannot start, or the rmcp
/// client/server exchange fails.
pub fn run_mcp_exchange(rig: &RigData) -> Result<McpOutcome> {
    let allowed = allowed_event();
    if decide(allowed) != GateDecision::Allowed {
        return Err(BakeoffError::Protocol(
            "the allowed flow event was refused by the gate".to_string(),
        ));
    }
    let GateDecision::Refused { code, remedy } = decide(violating_event()) else {
        return Err(BakeoffError::Protocol(
            "the violating flow event was allowed by the gate".to_string(),
        ));
    };
    let refusal_action = remedy
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            BakeoffError::Protocol("label refusal remedy omitted its action".to_string())
        })?
        .to_string();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| BakeoffError::Protocol(format!("cannot start MCP runtime: {error}")))?;
    let parameter_count = runtime.block_on(call_rig_census(rig.to_json()))?;
    if parameter_count != rig.parameter_count {
        return Err(BakeoffError::Protocol(format!(
            "MCP returned {parameter_count} parameters; the author census measured {}",
            rig.parameter_count
        )));
    }
    Ok(McpOutcome {
        parameter_count,
        tool_name: TOOL_NAME.to_string(),
        refusal_code: code.to_string(),
        refusal_action,
    })
}

async fn call_rig_census(payload: Value) -> Result<usize> {
    let (client_transport, server_transport) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let server = CensusServer { payload };
    let server_task = tokio::spawn(async move {
        let running = server
            .serve(server_transport)
            .await
            .map_err(|error| error.to_string())?;
        running.waiting().await.map_err(|error| error.to_string())?;
        Ok::<(), String>(())
    });

    let client = ()
        .serve_with_lifecycle(
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .map_err(|error| BakeoffError::Protocol(format!("rmcp client startup: {error}")))?;
    let mut arguments = Map::new();
    arguments.insert("view".to_string(), json!("summary"));
    let result = client
        .call_tool(CallToolRequestParams::new(TOOL_NAME).with_arguments(arguments))
        .await
        .map_err(|error| BakeoffError::Protocol(format!("rmcp tools/call: {error}")))?;
    client
        .cancel()
        .await
        .map_err(|error| BakeoffError::Protocol(format!("rmcp client shutdown: {error}")))?;
    match tokio::time::timeout(std::time::Duration::from_secs(2), server_task).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(message))) => {
            return Err(BakeoffError::Protocol(format!(
                "rmcp server shutdown: {message}"
            )));
        }
        Ok(Err(error)) => {
            return Err(BakeoffError::Protocol(format!(
                "rmcp server task failed: {error}"
            )));
        }
        Err(_) => {
            return Err(BakeoffError::Protocol(
                "rmcp server did not stop within 2 seconds".to_string(),
            ));
        }
    }

    let structured = result.structured_content.ok_or_else(|| {
        BakeoffError::Protocol("MCP tool result omitted structured content".to_string())
    })?;
    structured
        .get("parameters")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| BakeoffError::Protocol("MCP census omitted parameter count".to_string()))
}

#[derive(Clone)]
struct CensusServer {
    payload: Value,
}

impl ServerHandler for CensusServer {
    fn get_info(&self) -> InitializeResult {
        let mut server_info = Implementation::default();
        server_info.name = "torogao-tui-bakeoff".to_string();
        server_info.title = Some("Torogao TUI Bake-off".to_string());
        server_info.version = "1.0.0".to_string();
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
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        let mut schema = Map::new();
        schema.insert("type".to_string(), json!("object"));
        schema.insert(
            "properties".to_string(),
            json!({"view": {"enum": ["summary"], "type": "string"}}),
        );
        schema.insert("additionalProperties".to_string(), json!(false));
        let tool = Tool::new(
            TOOL_NAME,
            "Return the measured Torogao rig census",
            Arc::new(schema),
        );
        Ok(ListToolsResult {
            tools: vec![tool],
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        if request.name.as_ref() != TOOL_NAME {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {}", request.name),
                Some(json!({"code": "unknown_tool"})),
            ));
        }
        let text = serde_json::to_string(&self.payload)
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
        result.structured_content = Some(self.payload.clone());
        Ok(CallToolResponse::from(result))
    }
}
