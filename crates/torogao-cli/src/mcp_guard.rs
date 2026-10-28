//! Era guard: the byte-level front door of `torogao-cli mcp`.
//!
//! rmcp's service layer enforces the modern era at the message
//! level (discover negotiation, per-request `_meta` validation)
//! once the handler pins `supported_protocol_versions` to
//! 2026-07-28 — but it silently drops lines it cannot parse, and
//! it will still *answer* an `initialize` request (with the
//! unsupported-version refusal) only by failing the whole serve.
//! This guard sits in front of the SDK on stdio and owns the
//! framing contract, preserving the hand-rolled baseline's exact
//! error taxonomy (stable machine codes ride in `error.data.code`,
//! as the baseline had them):
//!
//! - malformed JSON / non-UTF-8 → `-32700` `parse_error` with the
//!   baseline's message (`frame is not valid JSON: {err}`), id null
//! - non-object frame → `-32600` `invalid_request`
//!   ("frame must be a JSON object"), id null
//! - object without a string `method` → `-32600` `invalid_request`
//!   ("frame has no string method"), id null — the baseline
//!   answered this class before it ever read the id, so the id is
//!   null even when the request carried one; preserved verbatim
//! - `initialize` (any version) → the designed modern-only
//!   refusal, `-32022` naming the supported versions; the
//!   connection and the process stay up
//! - unknown methods → `-32601` `method_not_found`
//!   ("unknown method: {method}", the baseline's message)
//! - malformed `tools/call` shapes are rejected here with the
//!   baseline's own `-32602` `invalid_arguments` messages —
//!   this baseline *rejected* bad call shapes in framing (the
//!   WS2-W1 rule: the guard variant matches the baseline's own
//!   argument handling — rejection here, not normalization)
//! - oversized line → `-32700` "request line exceeds size bound"
//!   with the remainder drained (the fleet 4 MiB bound replaces
//!   the baseline's 1 MiB cap and its `-32600 frame_too_large`
//!   shape; itemized in the WS2-W5 gate report)
//! - notifications (no `id`) are dropped silently — and never
//!   executed: the baseline executed and answered
//!   notification-shaped `tools/call` frames with id null; the
//!   fleet drops them unexecuted (itemized in the gate report)
//!
//! Only `server/discover`, `ping`, `tools/list`, and
//! shape-validated `tools/call` frames reach the SDK. Guard
//! frames are written only after every earlier forwarded request
//! has been answered (the WS2-W1 ordering fix, bounded at 5 s):
//! rmcp dispatches concurrently, so an instantly-answered guard
//! frame could otherwise overtake the SDK's first response.
//!
//! The guard is also the SEQUENCER (fleet finding from the
//! ache migration gate, WS2-W5): the same concurrent dispatch
//! means pipelined requests execute out of wire order unless
//! the guard serializes them — ache's stateful [arm, vibrate]
//! drew a spurious refusal that way. This registry's calls
//! commute in the common case, but not in principle
//! (instantiate-template, then inspect what it wrote), and the
//! fleet shape is one guard doing both jobs: at most one
//! forwarded request in flight, in wire order, and EOF waits
//! for the in-flight request to drain. Notifications are
//! dropped by this guard's design, so requests are the only
//! forwarded class there is to sequence.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, Stdin, Stdout,
    WriteHalf,
};
use tokio::sync::Mutex;

/// The one protocol version this server speaks.
pub const MODERN_VERSION: &str = "2026-07-28";
/// Maximum bytes in one JSON-RPC line (the fleet guard's bound).
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_ARGUMENTS: i32 = -32602;
const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;

/// Stdout shared by the guard (its own error frames) and the pump
/// (SDK responses); one line is written per lock, never interleaved.
pub type SharedStdout = Arc<Mutex<Stdout>>;

/// Response-ordering state shared by the guard and the pump: the
/// ids of forwarded requests whose SDK response has not yet been
/// written to stdout. The guard never writes one of its own frames
/// while an earlier forwarded request is unanswered.
pub struct Ordering {
    outstanding: Mutex<Vec<Value>>,
    drained: tokio::sync::Notify,
}

impl Ordering {
    pub fn new() -> Arc<Self> {
        Arc::new(Ordering {
            outstanding: Mutex::new(Vec::new()),
            drained: tokio::sync::Notify::new(),
        })
    }

    async fn track(&self, id: Value) {
        self.outstanding.lock().await.push(id);
    }

    async fn complete(&self, id: &Value) {
        {
            let mut queue = self.outstanding.lock().await;
            if let Some(pos) = queue.iter().position(|q| q == id) {
                queue.remove(pos);
            }
        }
        self.drained.notify_waiters();
    }

    /// Wait until no forwarded request is unanswered. Bounded: if
    /// the SDK died mid-request its response will never come, and
    /// the guard must go on answering framing errors regardless.
    async fn wait_drained(&self) {
        let deadline = std::time::Duration::from_secs(5);
        let _ = tokio::time::timeout(deadline, async {
            loop {
                let notified = self.drained.notified();
                if self.outstanding.lock().await.is_empty() {
                    return;
                }
                notified.await;
            }
        })
        .await;
    }

    /// Wait until no forwarded request is unanswered — unbounded.
    /// This is the sequencer's wait (see the module docs): wire
    /// order is a correctness property, not a latency budget, so
    /// a slow call holds the line for as long as it takes. The
    /// wait cannot wedge on a dead SDK: when the pump ends it
    /// calls `abandon`, which settles every outstanding id.
    async fn wait_settled(&self) {
        loop {
            let notified = self.drained.notified();
            if self.outstanding.lock().await.is_empty() {
                return;
            }
            notified.await;
        }
    }

    /// Settle every outstanding id at once. The pump calls this
    /// when it ends: no further SDK responses can arrive, so
    /// nothing may keep waiting for one.
    async fn abandon(&self) {
        {
            let mut queue = self.outstanding.lock().await;
            queue.clear();
        }
        self.drained.notify_waiters();
    }
}

/// One baseline-shaped error frame: the stable machine code rides
/// in `error.data.code`, exactly as the hand-rolled server had it.
fn error_frame(id: Value, code: i32, data_code: &str, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "data": {"code": data_code}, "message": message}
    })
}

/// The designed refusal of legacy-era contact (CHECKLIST 1.8 shape:
/// the SDK's own `unsupported_protocol_version` payload).
fn unsupported_version_frame(id: Value, requested: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": UNSUPPORTED_PROTOCOL_VERSION,
            "message": "Unsupported protocol version",
            "data": {"requested": requested, "supported": [MODERN_VERSION]}
        }
    })
}

/// What the guard does with one inbound line.
#[derive(Debug, PartialEq)]
enum Action {
    /// Forward this (possibly shape-completed) frame to the SDK.
    Forward(Value),
    /// Answer it here; the SDK never sees it.
    Respond(Value),
    /// Drop silently (blank line, notification).
    Drop,
}

/// Classify one line. Pure: unit-tested without any transport.
fn classify(line: &[u8]) -> Action {
    let text = match std::str::from_utf8(line) {
        Ok(text) => text,
        Err(err) => {
            return Action::Respond(error_frame(
                Value::Null,
                PARSE_ERROR,
                "parse_error",
                &format!("frame is not valid JSON: {err}"),
            ));
        }
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Action::Drop;
    }
    let value: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(err) => {
            return Action::Respond(error_frame(
                Value::Null,
                PARSE_ERROR,
                "parse_error",
                &format!("frame is not valid JSON: {err}"),
            ));
        }
    };
    let Some(object) = value.as_object() else {
        return Action::Respond(error_frame(
            Value::Null,
            INVALID_REQUEST,
            "invalid_request",
            "frame must be a JSON object",
        ));
    };
    // The baseline rejected a method-less frame before reading
    // the id, so this class answers with id null unconditionally.
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Action::Respond(error_frame(
            Value::Null,
            INVALID_REQUEST,
            "invalid_request",
            "frame has no string method",
        ));
    };
    // No `id` key: a notification. Dropped silently — and, unlike
    // the baseline, never executed and never answered.
    if !object.contains_key("id") {
        return Action::Drop;
    }
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => {
            let requested = object
                .get("params")
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Value::as_str)
                .map_or(Value::Null, |v| Value::String(v.to_string()));
            Action::Respond(unsupported_version_frame(id, requested))
        }
        "server/discover" | "ping" | "tools/list" => Action::Forward(normalize_params(value)),
        "tools/call" => classify_call(value, id),
        other => Action::Respond(error_frame(
            id,
            METHOD_NOT_FOUND,
            "method_not_found",
            &format!("unknown method: {other}"),
        )),
    }
}

/// The baseline ignored `params` on discover/ping/list entirely; a
/// missing or non-object value is normalized to `{}` so the SDK's
/// typed envelope always parses.
fn normalize_params(mut value: Value) -> Value {
    let object = value.as_object_mut().expect("classified object");
    match object.get("params") {
        Some(p) if p.is_object() => {}
        _ => {
            object.insert("params".to_string(), json!({}));
        }
    }
    value
}

/// Reproduce the baseline's `tools/call` shape rejections in
/// framing: the same `-32602 invalid_arguments` frames the
/// hand-rolled dispatcher answered, in the same order (params
/// object, then name, then arguments shape). A missing `arguments`
/// key was accepted as `{}` and is forwarded so.
fn classify_call(value: Value, id: Value) -> Action {
    let Some(params) = value.get("params").filter(|p| p.is_object()) else {
        return Action::Respond(error_frame(
            id,
            INVALID_ARGUMENTS,
            "invalid_arguments",
            "tools/call requires a params object",
        ));
    };
    if params.get("name").and_then(Value::as_str).is_none() {
        return Action::Respond(error_frame(
            id,
            INVALID_ARGUMENTS,
            "invalid_arguments",
            "tools/call requires a tool name",
        ));
    }
    if let Some(arguments) = params.get("arguments")
        && !arguments.is_object()
    {
        return Action::Respond(error_frame(
            id,
            INVALID_ARGUMENTS,
            "invalid_arguments",
            "tool arguments must be a JSON object",
        ));
    }
    Action::Forward(value)
}

async fn write_frame(stdout: &SharedStdout, frame: &Value) {
    let mut line = serde_json::to_string(frame).unwrap_or_default();
    line.push('\n');
    let mut out = stdout.lock().await;
    let _ = out.write_all(line.as_bytes()).await;
    let _ = out.flush().await;
}

async fn write_bytes(stdout: &SharedStdout, bytes: &[u8]) {
    let mut out = stdout.lock().await;
    let _ = out.write_all(bytes).await;
    let _ = out.flush().await;
}

/// One inbound line, bounded: the error trips when the content
/// (excluding the newline) reaches [`MAX_LINE_BYTES`]; an oversized
/// line's remainder is drained.
enum LineOutcome {
    Line(Vec<u8>),
    Oversized,
    Eof,
}

async fn next_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<LineOutcome> {
    let mut out: Vec<u8> = Vec::new();
    let mut content_len = 0usize;
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(if oversized {
                LineOutcome::Oversized
            } else if out.is_empty() && content_len == 0 {
                LineOutcome::Eof
            } else {
                LineOutcome::Line(out)
            });
        }
        match chunk.iter().position(|&b| b == b'\n') {
            Some(pos) => {
                content_len += pos;
                if content_len >= MAX_LINE_BYTES {
                    oversized = true;
                }
                if !oversized {
                    out.extend_from_slice(&chunk[..pos]);
                }
                reader.consume(pos + 1);
                return Ok(if oversized {
                    LineOutcome::Oversized
                } else {
                    LineOutcome::Line(out)
                });
            }
            None => {
                let len = chunk.len();
                content_len += len;
                if content_len >= MAX_LINE_BYTES {
                    oversized = true;
                    out.clear();
                } else {
                    out.extend_from_slice(chunk);
                }
                reader.consume(len);
            }
        }
    }
}

/// Run the guard until stdin reaches EOF, then shut the duplex
/// write side down: a split write half signals end-of-input to the
/// SDK service only through an explicit shutdown, never by being
/// dropped (both halves share ownership of the stream).
pub async fn run(
    stdin: Stdin,
    mut to_server: WriteHalf<DuplexStream>,
    stdout: SharedStdout,
    ordering: Arc<Ordering>,
) -> std::io::Result<()> {
    let result = forward_loop(stdin, &mut to_server, &stdout, &ordering).await;
    let _ = to_server.shutdown().await;
    result
}

async fn forward_loop(
    stdin: Stdin,
    to_server: &mut WriteHalf<DuplexStream>,
    stdout: &SharedStdout,
    ordering: &Ordering,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stdin);
    loop {
        match next_line(&mut reader).await? {
            LineOutcome::Eof => {
                // EOF waits for drain: an in-flight call finishes
                // and its response is written before the duplex
                // write half shuts down (the sequencer's rule).
                ordering.wait_settled().await;
                return Ok(());
            }
            LineOutcome::Oversized => {
                ordering.wait_drained().await;
                write_frame(
                    stdout,
                    &error_frame(
                        Value::Null,
                        PARSE_ERROR,
                        "parse_error",
                        "request line exceeds size bound",
                    ),
                )
                .await;
            }
            LineOutcome::Line(bytes) => match classify(&bytes) {
                Action::Drop => {}
                Action::Respond(frame) => {
                    ordering.wait_drained().await;
                    write_frame(stdout, &frame).await;
                }
                Action::Forward(frame) => {
                    // The sequencer: at most one forwarded request
                    // in flight, in wire order. rmcp dispatches
                    // concurrently; the fleet shape after the ache
                    // gate is guard = era gate + sequencer, and
                    // this registry has non-commuting pairs in
                    // principle (template instantiation followed
                    // by inspection of what it wrote), so the
                    // guard serializes exactly as the session
                    // host's does.
                    ordering.wait_settled().await;
                    if let Some(id) = frame.get("id").cloned() {
                        ordering.track(id).await;
                    }
                    let mut line = serde_json::to_string(&frame).unwrap_or_default();
                    line.push('\n');
                    to_server.write_all(line.as_bytes()).await?;
                    to_server.flush().await?;
                }
            },
        }
    }
}

/// Copy SDK responses from the duplex to stdout, one line at a time,
/// marking each answered request id complete for the guard's
/// ordering wait.
pub async fn pump(
    mut from_server: ReadHalf<DuplexStream>,
    stdout: SharedStdout,
    ordering: Arc<Ordering>,
) {
    let mut reader = BufReader::new(&mut from_server);
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => {
                // The SDK side is gone: settle every outstanding
                // id so a sequencer wait can never wedge on a
                // dead service.
                ordering.abandon().await;
                return;
            }
            Ok(_) => {
                write_bytes(&stdout, &line).await;
                if let Ok(frame) = serde_json::from_slice::<Value>(&line) {
                    // A response to an explicit `"id": null` request
                    // carries no id member at all (the SDK's
                    // invalid-request frame): it settles the Null
                    // entry, or the sequencer would wait on it
                    // forever.
                    let id = frame.get("id").cloned().unwrap_or(Value::Null);
                    ordering.complete(&id).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_str(s: &str) -> Action {
        classify(s.as_bytes())
    }

    #[test]
    fn blank_lines_and_notifications_drop() {
        assert_eq!(classify_str("  \n"), Action::Drop);
        assert_eq!(
            classify_str("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}"),
            Action::Drop
        );
        // A notification-shaped tools/call drops unexecuted (the
        // baseline executed and answered it; itemized divergence).
        assert_eq!(
            classify_str(
                "{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"name\":\"list-templates\",\"arguments\":{}}}"
            ),
            Action::Drop
        );
    }

    #[test]
    fn framing_errors_match_the_baseline() {
        match classify_str("this is not json") {
            Action::Respond(f) => {
                assert_eq!(f["error"]["code"], PARSE_ERROR);
                assert_eq!(f["error"]["data"]["code"], "parse_error");
                assert!(
                    f["error"]["message"]
                        .as_str()
                        .expect("message")
                        .starts_with("frame is not valid JSON: ")
                );
                assert_eq!(f["id"], Value::Null);
            }
            other => panic!("expected respond, got {other:?}"),
        }
        for frame in ["[1,2,3]", "42"] {
            match classify_str(frame) {
                Action::Respond(f) => {
                    assert_eq!(f["error"]["code"], INVALID_REQUEST);
                    assert_eq!(f["error"]["message"], "frame must be a JSON object");
                }
                other => panic!("expected respond for {frame}, got {other:?}"),
            }
        }
        // No-method frames answer id null even when an id was sent
        // (the baseline rejected them before reading the id).
        for frame in [
            "{\"jsonrpc\":\"2.0\",\"id\":5}",
            "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":42}",
        ] {
            match classify_str(frame) {
                Action::Respond(f) => {
                    assert_eq!(f["error"]["code"], INVALID_REQUEST);
                    assert_eq!(f["error"]["message"], "frame has no string method");
                    assert_eq!(f["id"], Value::Null);
                }
                other => panic!("expected respond for {frame}, got {other:?}"),
            }
        }
    }

    #[test]
    fn initialize_is_refused_with_supported_versions() {
        let frame = "{\"id\":7,\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\"}}";
        match classify_str(frame) {
            Action::Respond(f) => {
                assert_eq!(f["id"], 7);
                assert_eq!(f["error"]["code"], UNSUPPORTED_PROTOCOL_VERSION);
                assert_eq!(f["error"]["data"]["requested"], "2024-11-05");
                assert_eq!(f["error"]["data"]["supported"], json!(["2026-07-28"]));
            }
            other => panic!("expected respond, got {other:?}"),
        }
    }

    #[test]
    fn unknown_methods_are_method_not_found_with_baseline_message() {
        match classify_str("{\"id\":1,\"jsonrpc\":\"2.0\",\"method\":\"prompts/list\"}") {
            Action::Respond(f) => {
                assert_eq!(f["error"]["code"], METHOD_NOT_FOUND);
                assert_eq!(f["error"]["data"]["code"], "method_not_found");
                assert_eq!(f["error"]["message"], "unknown method: prompts/list");
            }
            other => panic!("expected respond, got {other:?}"),
        }
    }

    #[test]
    fn call_shape_rejections_match_the_baseline() {
        let cases = [
            (
                "{\"id\":1,\"method\":\"tools/call\",\"params\":\"x\"}",
                "tools/call requires a params object",
            ),
            (
                "{\"id\":2,\"method\":\"tools/call\"}",
                "tools/call requires a params object",
            ),
            (
                "{\"id\":3,\"method\":\"tools/call\",\"params\":{}}",
                "tools/call requires a tool name",
            ),
            (
                "{\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":7}}",
                "tools/call requires a tool name",
            ),
            (
                "{\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"inspect\",\"arguments\":\"nope\"}}",
                "tool arguments must be a JSON object",
            ),
        ];
        for (frame, message) in cases {
            match classify_str(frame) {
                Action::Respond(f) => {
                    assert_eq!(f["error"]["code"], INVALID_ARGUMENTS, "{frame}");
                    assert_eq!(f["error"]["data"]["code"], "invalid_arguments");
                    assert_eq!(f["error"]["message"], message, "{frame}");
                }
                other => panic!("expected respond for {frame}, got {other:?}"),
            }
        }
        // A missing arguments key was accepted by the baseline.
        match classify_str("{\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"inspect\"}}")
        {
            Action::Forward(_) => {}
            other => panic!("expected forward, got {other:?}"),
        }
    }

    #[test]
    fn valid_modern_frames_forward() {
        for frame in [
            "{\"id\":1,\"method\":\"server/discover\",\"params\":{\"_meta\":{}}}",
            "{\"id\":2,\"method\":\"tools/list\",\"params\":{\"_meta\":{}}}",
            "{\"id\":3,\"method\":\"ping\"}",
            "{\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"inspect\",\"arguments\":{},\"_meta\":{}}}",
        ] {
            match classify_str(frame) {
                Action::Forward(_) => {}
                other => panic!("expected forward for {frame}, got {other:?}"),
            }
        }
        // Non-object params on tools/list normalize to {}.
        match classify_str("{\"id\":9,\"method\":\"tools/list\",\"params\":\"x\"}") {
            Action::Forward(f) => assert_eq!(f["params"], json!({})),
            other => panic!("expected forward, got {other:?}"),
        }
    }
}
