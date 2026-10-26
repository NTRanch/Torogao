//! The A2A HTTP surface (a2a/server.d port): an Agent
//! Card generated from the tool registry, plus JSON-RPC
//! task methods (message/send, tasks/get, tasks/cancel)
//! delegating to the same registry the MCP server
//! serves. Transport is a small bounded HTTP server over
//! std::net — two routes, one request shape, size caps
//! on headers and body.

use crate::registry::{REGISTRY_VERSION, call_tool, list_tools};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
use torogao_format::jsonfmt::to_d_json;

/// The default A2A port.
pub const A2A_DEFAULT_PORT: u16 = 8377;

/// Header size cap (the reference's maxHeaderBytes).
const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Body size cap (the reference's maxBodyBytes).
const MAX_BODY_BYTES: usize = 1_048_576;

/// Builds the Agent Card JSON from the registry.
fn build_agent_card(base_url: &str) -> Value {
    let skills: Vec<Value> = list_tools()
        .iter()
        .map(|tool| {
            json!({
                "description": tool.description,
                "id": tool.name,
                "inputModes": ["application/json"],
                "name": tool.title,
                "outputModes": ["application/json"],
                "tags": [tool.namespace, tool.posture],
            })
        })
        .collect();
    json!({
        "capabilities": {
            "pushNotifications": false,
            "stateTransitionHistory": false,
            "streaming": false,
        },
        "defaultInputModes": ["application/json"],
        "defaultOutputModes": ["application/json"],
        "description": "Torogao puppet tooling for agents: inspect, validate, and convert realtime 2D puppet projects. Every skill delegates to the one tool registry; tools that would write into a project are listed as disabled skills with their gate named.",
        "name": "Torogao",
        "provider": { "organization": "Nice Thoughts Ranch" },
        "skills": skills,
        "url": base_url,
        "version": REGISTRY_VERSION,
    })
}

/// A JSON-RPC error response.
fn rpc_error(id: &Value, code: i32, data_code: &str, message: &str) -> Value {
    json!({
        "error": {
            "code": code,
            "data": { "code": data_code },
            "message": message,
        },
        "id": id.clone(),
        "jsonrpc": "2.0",
    })
}

/// A JSON-RPC result response.
fn rpc_result(id: &Value, result: &Value) -> Value {
    json!({
        "id": id.clone(),
        "jsonrpc": "2.0",
        "result": result.clone(),
    })
}

/// The mutable server state: stored tasks by id.
#[derive(Default)]
struct A2aServer {
    /// Stored task objects, as served.
    tasks: HashMap<String, Value>,
    /// Monotonic task counter.
    task_counter: usize,
    /// The base URL the card advertises.
    base_url: String,
}

impl A2aServer {
    /// Creates a fresh task object in "working" state.
    fn new_task(&mut self, context_id: &str) -> Value {
        self.task_counter += 1;
        json!({
            "artifacts": [],
            "contextId": context_id,
            "id": format!("task-{}", self.task_counter),
            "status": { "state": "working" },
        })
    }

    /// Handles one JSON-RPC request object.
    fn handle_rpc(&mut self, msg: &Value) -> Value {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            return rpc_error(
                &id,
                -32600,
                "invalid_request",
                "request has no string method",
            );
        };
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        match method {
            "message/send" => self.message_send(&id, &params),
            "tasks/get" => self.task_lookup(&id, &params, false),
            "tasks/cancel" => self.task_lookup(&id, &params, true),
            other => rpc_error(
                &id,
                -32601,
                "method_not_found",
                &format!("unknown method: {other}"),
            ),
        }
    }

    /// message/send: run the named tool synchronously and
    /// store the completed (or failed) task.
    fn message_send(&mut self, id: &Value, params: &Value) -> Value {
        let Some(message) = params.get("message") else {
            return rpc_error(
                id,
                -32602,
                "invalid_arguments",
                "message/send requires params.message",
            );
        };
        let mut tool_name = String::new();
        let mut arguments = json!({});
        let mut found = false;
        if let Some(parts) = message.get("parts").and_then(Value::as_array) {
            for part in parts {
                if !part.is_object() {
                    continue;
                }
                let kind = part.get("kind");
                let data = part.get("data");
                if kind.and_then(Value::as_str) == Some("data")
                    && data.is_some_and(Value::is_object)
                {
                    let data = data.unwrap();
                    if let Some(tool) = data.get("tool").and_then(Value::as_str) {
                        tool_name = tool.to_string();
                        found = true;
                        if let Some(args) = data.get("arguments") {
                            arguments = args.clone();
                        }
                    }
                }
            }
        }
        if !found {
            return rpc_error(
                id,
                -32602,
                "invalid_arguments",
                "message must carry a data part {\"tool\": ..., \"arguments\": {...}}",
            );
        }
        let context_id = message
            .get("contextId")
            .and_then(Value::as_str)
            .unwrap_or("torogao");
        let mut task = self.new_task(context_id);
        let task_id = task["id"].as_str().unwrap_or("").to_string();
        let result = call_tool(&tool_name, &arguments);
        let (data, state) = if result.is_error {
            (result.error_json(), "failed")
        } else {
            (result.payload.clone(), "completed")
        };
        task["status"]["state"] = json!(state);
        task["artifacts"] = json!([{
            "artifactId": format!("{task_id}-0"),
            "name": tool_name,
            "parts": [{
                "data": data,
                "kind": "data",
            }],
        }]);
        self.tasks.insert(task_id, task.clone());
        rpc_result(id, &task)
    }

    /// tasks/get (cancel = false) and tasks/cancel (true).
    fn task_lookup(&mut self, id: &Value, params: &Value, cancel: bool) -> Value {
        let method = if cancel { "tasks/cancel" } else { "tasks/get" };
        let Some(task_id) = params.get("id").and_then(Value::as_str) else {
            return rpc_error(
                id,
                -32602,
                "invalid_arguments",
                &format!("{method} requires params.id"),
            );
        };
        let Some(task) = self.tasks.get(task_id).cloned() else {
            return rpc_error(
                id,
                -32001,
                "task_not_found",
                &format!("no task with id {task_id}"),
            );
        };
        if cancel {
            let state = task["status"]["state"].as_str().unwrap_or("");
            if state != "completed" && state != "failed" && state != "canceled" {
                let mut task = task;
                task["status"]["state"] = json!("canceled");
                self.tasks.insert(task_id.to_string(), task.clone());
                return rpc_result(id, &task);
            }
        }
        rpc_result(id, &task)
    }

    /// Serves one accepted connection to completion.
    fn serve(&mut self, conn: &mut TcpStream) {
        let _ = conn.set_read_timeout(Some(Duration::from_secs(30)));
        let mut buffer: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut header_end: Option<usize> = None;
        while header_end.is_none() {
            match conn.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    buffer.extend_from_slice(&chunk[..n]);
                    if buffer.len() > MAX_HEADER_BYTES {
                        respond(conn, 431, "{\"error\":\"headers too large\"}");
                        return;
                    }
                    if buffer.len() >= 4 {
                        header_end = buffer
                            .windows(4)
                            .position(|w| w == b"\r\n\r\n")
                            .map(|i| i + 4);
                    }
                }
            }
        }
        let header_end = header_end.unwrap();
        let head = String::from_utf8_lossy(&buffer[..header_end - 4]).into_owned();
        let mut body: Vec<u8> = buffer[header_end..].to_vec();
        let mut lines = head.split("\r\n");
        let request_line = lines.next().unwrap_or("");
        let mut request_parts = request_line.split(' ');
        let method = request_parts.next().unwrap_or("");
        let path = request_parts.next().unwrap_or("");
        let mut content_length: usize = 0;
        for line in lines {
            let Some(colon) = line.find(':') else {
                continue;
            };
            if line[..colon].trim().eq_ignore_ascii_case("content-length") {
                content_length = line[colon + 1..].trim().parse().unwrap_or(0);
            }
        }
        if content_length > MAX_BODY_BYTES {
            respond(conn, 413, "{\"error\":\"body too large\"}");
            return;
        }
        while body.len() < content_length {
            match conn.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => body.extend_from_slice(&chunk[..n]),
            }
        }

        if method == "GET" && path == "/.well-known/agent-card.json" {
            respond(conn, 200, &to_d_json(&build_agent_card(&self.base_url)));
            return;
        }
        if method == "POST" && (path == "/" || path == "/rpc") {
            let text = String::from_utf8_lossy(&body).into_owned();
            let parsed: Result<Value, _> = serde_json::from_str(&text);
            match parsed {
                Err(e) => {
                    respond(
                        conn,
                        400,
                        &to_d_json(&rpc_error(
                            &Value::Null,
                            -32700,
                            "parse_error",
                            &e.to_string(),
                        )),
                    );
                }
                Ok(msg) if !msg.is_object() => {
                    respond(
                        conn,
                        400,
                        &to_d_json(&rpc_error(
                            &Value::Null,
                            -32600,
                            "invalid_request",
                            "body must be a JSON object",
                        )),
                    );
                }
                Ok(msg) => {
                    let response = self.handle_rpc(&msg);
                    respond(conn, 200, &to_d_json(&response));
                }
            }
            return;
        }
        respond(conn, 404, "{\"error\":\"not found\"}");
    }
}

/// Writes one HTTP response (Connection: close).
fn respond(conn: &mut TcpStream, status: u32, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Status",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = conn.write_all(head.as_bytes());
    let _ = conn.write_all(body.as_bytes());
}

/// Serves A2A over HTTP until killed; returns the exit
/// code.
pub fn run_a2a_server(bind: &str, port: u16) -> i32 {
    let base_url = format!("http://{bind}:{port}/");
    let listener = match TcpListener::bind((bind, port)) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("torogao a2a: cannot bind {base_url}: {e}");
            return 1;
        }
    };
    eprintln!("torogao a2a: listening on {base_url}");
    let mut server = A2aServer {
        base_url,
        ..Default::default()
    };
    loop {
        match listener.accept() {
            Ok((mut conn, _)) => {
                server.serve(&mut conn);
            }
            Err(_) => continue,
        }
    }
}
