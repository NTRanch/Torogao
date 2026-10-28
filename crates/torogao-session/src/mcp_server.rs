//! The modern-era MCP handler for the session host (rmcp 3.8.0):
//! the registry's tools and the discover-time session handle,
//! over the owner thread in `dispatch`.
//!
//! Session handle (explicit-handles rule): the `server/discover`
//! result carries the session's identity in its `_meta` —
//! `torogao/session`, `torogao/genesis_hash`,
//! `torogao/state_hash`, `torogao/tools` — the same values the
//! Phase C handshake result carried, in the modern position.
//! Clients present the capability token in every request's
//! `_meta` (checked by `mcp_guard` before anything reaches
//! here) and take their bearings from the handle; the handler
//! itself keeps no per-connection state.
//!
//! Tool results reproduce the baseline's payload shapes: a
//! successful call returns the tool's payload as the text
//! content AND as structured content (the baseline returned the
//! bare payload as the result; the wrapping is the era's shape,
//! itemized in the WS2-W5 gate report). A refusal is the
//! baseline's error frame exactly: JSON-RPC `-32000`, the
//! refusal's message, and `data: {"refusal": <code>}`.

use std::sync::Arc;

use std::borrow::Cow;

use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode, ErrorData, Implementation,
    InitializeResult, JsonObject, ListToolsResult, MetaObject, ProtocolVersion, ServerCapabilities,
    Tool, ToolAnnotations,
};
use serde_json::{Map, Value, json};

use crate::dispatch::DispatchHandle;
use crate::registry::{ToolDef, registry};

/// The modern-era handler. Clone-cheap: the dispatch handle is
/// a channel plus a snapshot behind `Arc`s.
#[derive(Clone)]
pub struct SessionServer {
    dispatch: DispatchHandle,
}

impl SessionServer {
    /// A handler backed by the session's owner thread.
    pub fn new(dispatch: DispatchHandle) -> Self {
        SessionServer { dispatch }
    }
}

/// The rmcp tool for a registry entry. As on the CLI surface,
/// the baseline's registry metadata (`namespace`, `posture`,
/// and `gate` for gated tools — plain fields in the baseline's
/// tool JSON) rides on in the tool's `_meta`, since rmcp's
/// annotations type carries only the five standard hints.
fn tool_for(def: &ToolDef) -> Tool {
    let schema: JsonObject = def.schema.as_object().cloned().unwrap_or_default();
    let mut tool = Tool::new(def.name, def.description, Arc::new(schema));
    tool.title = Some(def.title.to_string());
    tool.annotations = Some(ToolAnnotations::from_raw(
        None,
        Some(def.posture == "read-only"),
        Some(false),
        Some(def.posture != "export"),
        Some(false),
    ));
    let mut meta = Map::new();
    meta.insert("torogao:namespace".to_string(), json!(def.namespace));
    meta.insert("torogao:posture".to_string(), json!(def.posture));
    if !def.gate.is_empty() {
        meta.insert("torogao:gate".to_string(), json!(def.gate));
    }
    tool.meta = Some(MetaObject(meta));
    tool
}

impl ServerHandler for SessionServer {
    fn get_info(&self) -> InitializeResult {
        let info = self.dispatch.info();
        let mut meta: JsonObject = Map::new();
        meta.insert("torogao/session".to_string(), json!(info.session_id));
        meta.insert("torogao/genesis_hash".to_string(), json!(info.genesis_hash));
        meta.insert("torogao/state_hash".to_string(), json!(info.state_hash));
        meta.insert("torogao/tools".to_string(), json!(info.tool_count));
        let mut result = InitializeResult::default();
        result.protocol_version = ProtocolVersion::V_2026_07_28;
        result.capabilities = ServerCapabilities::builder().enable_tools().build();
        let mut server_info = Implementation::default();
        // The A2A card's agent identity: the interop surface
        // keeps its name across the migration.
        server_info.name = "torogao-editor-interop".to_string();
        server_info.version = "0.1.0".to_string();
        result.server_info = server_info;
        result.meta = Some(MetaObject(meta));
        result
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: registry().iter().map(tool_for).collect(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = match request.arguments {
            Some(map) => Value::Object(map),
            None => json!({}),
        };
        match self.dispatch.call_tool(&request.name, arguments).await {
            Ok(payload) => Ok(CallToolResponse::from(CallToolResult::structured(payload))),
            Err(refusal) => Err(ErrorData::new(
                ErrorCode(-32000),
                refusal.message.clone(),
                Some(json!({"refusal": refusal.code})),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, NoLiveBridge};
    use crate::registry::agent_card;
    use crate::session::Session;
    use crate::state::{EditorState, ProjectMeta, STATE_FORMAT, Scene, Viewport};
    use std::collections::BTreeMap;

    fn server() -> SessionServer {
        let genesis = EditorState {
            format: STATE_FORMAT.to_string(),
            project: ProjectMeta {
                name: "server-test".to_string(),
                source: "test".to_string(),
            },
            scene: Scene {
                name: "server-test".to_string(),
                nodes: Vec::new(),
            },
            selection: Vec::new(),
            viewport: Viewport {
                cam_x: 0.0,
                cam_y: 0.0,
                zoom: 1.0,
            },
            params: BTreeMap::new(),
            bindings: Vec::new(),
            uncapturable: Vec::new(),
        };
        let session = Session::new(genesis, &BTreeMap::new(), "server test");
        let dispatch = crate::dispatch::spawn(Host::new(session), Box::new(NoLiveBridge));
        SessionServer::new(dispatch)
    }

    #[test]
    fn discover_carries_the_session_handle() {
        let info = server().get_info();
        assert_eq!(info.server_info.name, "torogao-editor-interop");
        let meta = &info.meta.expect("handle meta").0;
        assert!(
            meta["torogao/session"]
                .as_str()
                .expect("session id")
                .starts_with("ses-")
        );
        assert_eq!(meta["torogao/tools"], json!(9));
        assert_eq!(meta["torogao/genesis_hash"], meta["torogao/state_hash"]);
    }

    #[test]
    fn tools_match_the_registry_and_the_a2a_card() {
        // The drift check (WS2-W5 brief): the tools served, the
        // registry table, and the A2A card's skills name the same
        // nine tools. The card groups its skills by namespace
        // (its own order), so the comparison is over name sets.
        let names: Vec<String> = registry().iter().map(|d| d.name.to_string()).collect();
        assert_eq!(names.len(), 9);
        let card = agent_card();
        assert_eq!(card["name"], json!("torogao-editor-interop"));
        let mut card_names: Vec<String> = card["skills"]
            .as_array()
            .expect("card skills")
            .iter()
            .flat_map(|s| {
                s["tools"]
                    .as_array()
                    .expect("skill tools")
                    .iter()
                    .map(|t| t.as_str().expect("tool name").to_string())
                    .collect::<Vec<String>>()
            })
            .collect();
        card_names.sort();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(sorted, card_names);
        let tools: Vec<Tool> = registry().iter().map(tool_for).collect();
        for (tool, def) in tools.iter().zip(registry().iter()) {
            assert_eq!(tool.name, def.name);
            assert_eq!(
                tool.input_schema.as_ref(),
                def.schema
                    .as_object()
                    .expect("registry schemas are objects")
            );
            let meta = &tool.meta.as_ref().expect("tool meta").0;
            assert_eq!(meta["torogao:namespace"], json!(def.namespace));
            assert_eq!(meta["torogao:posture"], json!(def.posture));
        }
    }
}
