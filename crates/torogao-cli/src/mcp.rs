//! The MCP server mode (mcp/server.d): the tool registry
//! over stdio as newline-delimited JSON-RPC 2.0 frames —
//! a hard frame size cap, typed protocol errors, and no
//! input that can crash or hang the server. Transport
//! only; every tool call delegates to the one registry.
//!
//! Error model: transport/protocol problems (unparseable
//! frame, frame over the cap, unknown method, client-
//! caused argument problems) are JSON-RPC error responses
//! with the stable machine code in error.data.code. Tool
//! execution failures are normal tool results with
//! isError set.

use crate::registry;
use serde_json::Value;
use std::io::{Read, Write};
use torogao_format::jsonfmt::to_d_json;

/// Hard cap on one input frame, in bytes (1 MiB).
const MAX_FRAME_BYTES: usize = 1_048_576;
/// MCP protocol version this server answers with.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Reads newline-terminated frames from stdin, enforcing
/// the size cap while reading: an over-cap frame is
/// consumed through its newline (so the stream resyncs)
/// and reported as oversized instead of buffered whole.
struct FrameReader {
    pending: Vec<u8>,
    eof: bool,
    source: std::io::StdinLock<'static>,
}

impl FrameReader {
    fn new() -> FrameReader {
        FrameReader {
            pending: Vec::new(),
            eof: false,
            source: std::io::stdin().lock(),
        }
    }

    /// Returns (frame, oversized); None at end of stream
    /// with no partial frame delivered.
    fn read_frame(&mut self) -> Option<(Vec<u8>, bool)> {
        let mut overflow = false;
        let mut scanned = 0usize;
        loop {
            if let Some(pos) = self.pending[scanned..].iter().position(|b| *b == b'\n') {
                let end = scanned + pos;
                let oversized = overflow || end > MAX_FRAME_BYTES;
                let frame = if oversized {
                    Vec::new()
                } else {
                    self.pending[..end].to_vec()
                };
                self.pending.drain(..=end);
                return Some((frame, oversized));
            }
            scanned = self.pending.len();
            if scanned > MAX_FRAME_BYTES {
                overflow = true;
            }
            if overflow {
                self.pending.clear();
                scanned = 0;
            }
            if self.eof && self.pending.is_empty() {
                return None;
            }
            if self.eof {
                let frame = if overflow {
                    Vec::new()
                } else {
                    std::mem::take(&mut self.pending)
                };
                return Some((frame, overflow));
            }
            let mut chunk = [0u8; 8192];
            match self.source.read(&mut chunk) {
                Ok(0) | Err(_) => self.eof = true,
                Ok(n) => self.pending.extend_from_slice(&chunk[..n]),
            }
        }
    }
}

fn rpc_error(id: Value, code: i64, data_code: &str, message: &str) -> Value {
    serde_json::json!({
        "error": {
            "code": code,
            "data": {"code": data_code},
            "message": message,
        },
        "id": id,
        "jsonrpc": "2.0",
    })
}

fn rpc_result(id: Value, result: Value) -> Value {
    serde_json::json!({
        "id": id,
        "jsonrpc": "2.0",
        "result": result,
    })
}

/// MCP tool description, generated from the registry spec.
fn tool_json(tool: &crate::registry_data::ToolMeta) -> Value {
    let mut annotations = serde_json::json!({
        "destructiveHint": false,
        "idempotentHint": tool.posture != "export",
        "openWorldHint": false,
        "readOnlyHint": tool.posture == "read-only",
        "torogao:namespace": tool.namespace,
        "torogao:posture": tool.posture,
    });
    if tool.posture == "disabled" {
        annotations["torogao:gate"] = Value::String(tool.gate.to_string());
    }
    serde_json::json!({
        "annotations": annotations,
        "description": tool.description,
        "inputSchema": serde_json::from_str::<Value>(tool.input_schema_json)
            .unwrap_or(Value::Null),
        "name": tool.name,
        "title": tool.title,
    })
}

/// Handles one frame. Returns Some(response) when a
/// response must be written; notifications return None.
fn handle_frame(frame: &[u8]) -> Option<Value> {
    let text = match std::str::from_utf8(frame) {
        Ok(text) => text,
        Err(err) => {
            return Some(rpc_error(
                Value::Null,
                -32700,
                "parse_error",
                &format!("frame is not valid JSON: {err}"),
            ));
        }
    };
    let msg: Value = match serde_json::from_str(text) {
        Ok(msg) => msg,
        Err(err) => {
            return Some(rpc_error(
                Value::Null,
                -32700,
                "parse_error",
                &format!("frame is not valid JSON: {err}"),
            ));
        }
    };
    if !msg.is_object() {
        return Some(rpc_error(
            Value::Null,
            -32600,
            "invalid_request",
            "frame must be a JSON object",
        ));
    }
    let method = match msg.get("method") {
        Some(Value::String(method)) => method.clone(),
        _ => {
            return Some(rpc_error(
                Value::Null,
                -32600,
                "invalid_request",
                "frame has no string method",
            ));
        }
    };
    let id_value = msg.get("id");
    let is_notification = id_value.is_none() || id_value == Some(&Value::Null);
    let id = if is_notification {
        Value::Null
    } else {
        id_value.cloned().unwrap_or(Value::Null)
    };

    match method.as_str() {
        "initialize" => {
            let result = serde_json::json!({
                "capabilities": {"tools": {"listChanged": false}},
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "serverInfo": {
                    "name": "torogao",
                    "title": "Torogao",
                    "version": registry::REGISTRY_VERSION,
                },
            });
            Some(rpc_result(id, result))
        }
        "ping" => Some(rpc_result(id, Value::Object(serde_json::Map::new()))),
        "tools/list" => {
            let tools: Vec<Value> = registry::list_tools()
                .iter()
                .map(|tool| tool_json(tool))
                .collect();
            Some(rpc_result(id, serde_json::json!({"tools": tools})))
        }
        "tools/call" => {
            let params = match msg.get("params") {
                Some(params) if params.is_object() => params.clone(),
                _ => {
                    return Some(rpc_error(
                        id,
                        -32602,
                        "invalid_arguments",
                        "tools/call requires a params object",
                    ));
                }
            };
            let name = match params.get("name") {
                Some(Value::String(name)) => name.clone(),
                _ => {
                    return Some(rpc_error(
                        id,
                        -32602,
                        "invalid_arguments",
                        "tools/call requires a tool name",
                    ));
                }
            };
            let arguments = match params.get("arguments") {
                Some(arguments) => {
                    if !arguments.is_object() {
                        return Some(rpc_error(
                            id,
                            -32602,
                            "invalid_arguments",
                            "tool arguments must be a JSON object",
                        ));
                    }
                    arguments.clone()
                }
                None => Value::Object(serde_json::Map::new()),
            };
            let result = registry::call_tool(&name, &arguments);
            if result.is_error {
                if ["unknown_tool", "tool_disabled", "invalid_arguments"]
                    .contains(&result.error_code.as_str())
                {
                    return Some(rpc_error(
                        id,
                        -32602,
                        &result.error_code,
                        &result.error_message,
                    ));
                }
                let tool_result = serde_json::json!({
                    "content": [{
                        "text": to_d_json(&result.error_json()),
                        "type": "text",
                    }],
                    "isError": true,
                });
                return Some(rpc_result(id, tool_result));
            }
            let tool_result = serde_json::json!({
                "content": [{
                    "text": to_d_json(&result.payload),
                    "type": "text",
                }],
                "isError": false,
                "structuredContent": result.payload,
            });
            Some(rpc_result(id, tool_result))
        }
        _ => {
            if is_notification {
                return None;
            }
            // The reference's prefix test compares a 12-byte
            // slice against the 13-byte literal, so it never
            // fires; replicated verbatim for parity.
            if method.len() > 12 && method.get(..12) == Some("notifications") {
                return None;
            }
            Some(rpc_error(
                id,
                -32601,
                "method_not_found",
                &format!("unknown method: {method}"),
            ))
        }
    }
}

/// Runs the MCP server on stdio until the input reaches
/// EOF; returns 0.
pub fn run_mcp_server() -> i32 {
    let mut reader = FrameReader::new();
    let mut stdout = std::io::stdout().lock();
    while let Some((frame, oversized)) = reader.read_frame() {
        if oversized {
            let response = rpc_error(
                Value::Null,
                -32600,
                "frame_too_large",
                &format!("frame exceeds the {MAX_FRAME_BYTES}-byte cap"),
            );
            let _ = writeln!(stdout, "{}", to_d_json(&response));
            let _ = stdout.flush();
            continue;
        }
        if frame.is_empty() {
            continue;
        }
        if let Some(response) = handle_frame(&frame) {
            let _ = writeln!(stdout, "{}", to_d_json(&response));
            let _ = stdout.flush();
        }
    }
    0
}
