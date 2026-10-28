//! Era guard: the byte-level front door of the session host,
//! shared by both transports (stdio and WebSocket — the guard
//! works on any async reader/writer pair; the transports in
//! `transport` adapt their framing to bytes).
//!
//! rmcp's service layer enforces the modern era at the message
//! level once the handler pins `supported_protocol_versions`
//! to 2026-07-28, but it silently drops lines it cannot parse
//! and it has no notion of this surface's capability token.
//! The guard therefore owns the framing contract, preserving
//! the Phase C baseline's error taxonomy wherever the era
//! allows:
//!
//! - malformed JSON → `-32700` "parse error: {err}" (the
//!   baseline host's exact message), id null, no data
//! - a non-object frame → `-32600` "frame must be a JSON
//!   object" (the fleet framing contract; the baseline host
//!   conflated this with method routing and answered
//!   `-32601` — the one framing shape the migration does not
//!   preserve, itemized in the WS2-W5 gate report)
//! - an object with no string method → `-32601`
//!   "method  not found" — the baseline host read the method
//!   before anything else and answered such frames this way;
//!   preserved verbatim, including the empty method name
//! - unknown methods → `-32601` "method {method} not found",
//!   answered regardless of auth, as the baseline did
//! - `initialize` (any version) → the designed modern-only
//!   refusal, `-32022` naming the supported versions. This
//!   retires the baseline's token-handshake `initialize`:
//!   in the modern era there is no handshake frame at all
//! - `notifications/*` and every id-less frame are dropped
//!   silently and never executed (the baseline answered
//!   id-less requests with id null; the fleet drops them —
//!   itemized in the WS2-W5 gate report)
//! - oversized line → `-32700` "request line exceeds size
//!   bound" with the remainder drained (fleet 4 MiB bound;
//!   the baseline host had no line bound)
//!
//! Auth (WS2-W5's explicit-handle design): the capability
//! token rides in every forwarded request's `_meta` under
//! `torogao/capability_token` — never in tool arguments, so
//! it never reaches the op log or a record. A forwarded
//! method (`server/discover`, `ping`, `tools/list`,
//! `tools/call`) whose `_meta` lacks the token or presents
//! the wrong one is refused here with the baseline's auth
//! shape: `-32001` "capability token refused", data
//! `{"refusal": "TG-SES-AUTH"}`. Auth is per-request: no
//! connection state exists to lean on, so any connection can
//! serve any request — the session itself is identified by
//! the handle the discover result returns.
//!
//! Guard variant (WS2-W1 rule): this baseline *degraded*
//! malformed `tools/call` shapes in its dispatcher (missing
//! name → the unknown-tool refusal, non-object arguments →
//! fieldless reads), so the guard *normalizes*: params →
//! object, name → `""`, non-object arguments → `{}` — and the
//! handler reproduces the baseline's exact refusals.
//!
//! Guard frames are written only after every earlier
//! forwarded request has been answered (the WS2-W1 ordering
//! fix, bounded at 5 s).
//!
//! The guard is also the SEQUENCER (fleet finding from the
//! ache migration gate, WS2-W5): rmcp dispatches pipelined
//! requests concurrently, and this surface's calls do not
//! commute — a `session_apply_op`'s `base_hash` names the
//! state the previous op produced, so a pipelined
//! [op, op] pair dispatched out of order draws a spurious
//! TG-SES-STALE, the same failure ache saw as a spurious
//! not_armed on [arm, vibrate]. Id-correlation (WS2-W1)
//! makes out-of-order *responses* readable; it does not
//! restore wire-order *execution* on a stateful server. So
//! the guard forwards at most one request at a time, in
//! wire order, and EOF waits for the in-flight request to
//! drain. Notifications are dropped by this guard's design,
//! so requests are the only forwarded class there is to
//! sequence. No handler changes were needed — the
//! serialization lives entirely here.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf,
};
use tokio::sync::Mutex;

/// The one protocol version this server speaks.
pub const MODERN_VERSION: &str = "2026-07-28";
/// The `_meta` key carrying the capability token.
pub const TOKEN_META_KEY: &str = "torogao/capability_token";
/// Maximum bytes in one JSON-RPC line (the fleet guard's bound).
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const AUTH_REFUSED: i32 = -32001;
const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;

/// Response-ordering state shared by the guard and the pump: the
/// ids of forwarded requests whose SDK response has not yet been
/// written out. The guard never writes one of its own frames
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

/// One baseline-shaped plain error frame (no data member, as
/// the Phase C host's framing errors had it).
fn error_frame(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The auth refusal, in the baseline's exact shape.
fn auth_frame(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": AUTH_REFUSED,
            "message": "capability token refused",
            "data": {"refusal": "TG-SES-AUTH"}
        }
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
    /// Forward this (possibly shape-normalized) frame to the SDK.
    Forward(Value),
    /// Answer it here; the SDK never sees it.
    Respond(Value),
    /// Drop silently (blank line, notification).
    Drop,
}

/// Whether the frame's `_meta` presents the capability token.
fn token_ok(frame: &Value, token: &str) -> bool {
    frame
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(TOKEN_META_KEY))
        .and_then(Value::as_str)
        .is_some_and(|presented| presented == token)
}

/// Classify one line against the session token. Pure:
/// unit-tested without any transport.
fn classify(line: &[u8], token: &str) -> Action {
    let value: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(err) => {
            // A whitespace-only slice parses as an error too;
            // blank input is a drop, as on every fleet guard.
            if line.iter().all(|b| b.is_ascii_whitespace()) {
                return Action::Drop;
            }
            return Action::Respond(error_frame(
                Value::Null,
                PARSE_ERROR,
                &format!("parse error: {err}"),
            ));
        }
    };
    let Some(object) = value.as_object() else {
        return Action::Respond(error_frame(
            Value::Null,
            INVALID_REQUEST,
            "frame must be a JSON object",
        ));
    };
    // The baseline read the method before the id and before
    // auth: an object with no string method is method-not-found
    // (with the empty name), whatever else it carries.
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Action::Respond(error_frame(
            Value::Null,
            METHOD_NOT_FOUND,
            "method  not found",
        ));
    };
    if method.starts_with("notifications/") {
        return Action::Drop;
    }
    // No `id` key: a notification. Dropped silently — and,
    // unlike the baseline, never executed and never answered.
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
        "server/discover" | "ping" | "tools/list" => {
            if token_ok(&value, token) {
                Action::Forward(normalize_params(value))
            } else {
                Action::Respond(auth_frame(id))
            }
        }
        "tools/call" => {
            if token_ok(&value, token) {
                Action::Forward(normalize_call(value))
            } else {
                Action::Respond(auth_frame(id))
            }
        }
        other => Action::Respond(error_frame(
            id,
            METHOD_NOT_FOUND,
            &format!("method {other} not found"),
        )),
    }
}

/// A missing or non-object `params` becomes `{}` so the SDK's
/// typed envelope always parses; the token has already been
/// verified inside the original `_meta` at that point — the
/// params of forwarded discover/ping/list frames carry no
/// other meaning.
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

/// Reproduce the baseline's `tools/call` degradation: a
/// missing/non-string name becomes `""` (the dispatch then
/// answers the unknown-tool refusal, as it did), and a
/// non-object `arguments` becomes `{}` (the baseline's arg
/// reads on a non-object found no fields, tool by tool).
fn normalize_call(mut value: Value) -> Value {
    let object = value.as_object_mut().expect("classified object");
    match object.get("params") {
        Some(p) if p.is_object() => {}
        _ => {
            object.insert("params".to_string(), json!({}));
        }
    }
    let params = object
        .get_mut("params")
        .and_then(Value::as_object_mut)
        .expect("params normalized to an object");
    match params.get("name") {
        Some(n) if n.is_string() => {}
        _ => {
            params.insert("name".to_string(), Value::String(String::new()));
        }
    }
    if let Some(arguments) = params.get("arguments")
        && !arguments.is_object()
    {
        params.insert("arguments".to_string(), json!({}));
    }
    value
}

async fn write_frame<W: AsyncWrite + Unpin>(out: &Arc<Mutex<W>>, frame: &Value) {
    let mut line = serde_json::to_string(frame).unwrap_or_default();
    line.push('\n');
    let mut out = out.lock().await;
    let _ = out.write_all(line.as_bytes()).await;
    let _ = out.flush().await;
}

async fn write_bytes<W: AsyncWrite + Unpin>(out: &Arc<Mutex<W>>, bytes: &[u8]) {
    let mut out = out.lock().await;
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

/// Run the guard over one connection until the reader reaches
/// EOF, then shut the duplex write side down: a split write
/// half signals end-of-input to the SDK service only through
/// an explicit shutdown, never by being dropped (both halves
/// share ownership of the stream).
pub async fn run<R, W>(
    reader: R,
    mut to_server: WriteHalf<DuplexStream>,
    out: Arc<Mutex<W>>,
    ordering: Arc<Ordering>,
    token: Arc<str>,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send,
{
    let result = forward_loop(reader, &mut to_server, &out, &ordering, &token).await;
    let _ = to_server.shutdown().await;
    result
}

async fn forward_loop<R, W>(
    mut reader: R,
    to_server: &mut WriteHalf<DuplexStream>,
    out: &Arc<Mutex<W>>,
    ordering: &Ordering,
    token: &str,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send,
{
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
                    out,
                    &error_frame(Value::Null, PARSE_ERROR, "request line exceeds size bound"),
                )
                .await;
            }
            LineOutcome::Line(bytes) => match classify(&bytes, token) {
                Action::Drop => {}
                Action::Respond(frame) => {
                    ordering.wait_drained().await;
                    write_frame(out, &frame).await;
                }
                Action::Forward(frame) => {
                    // The sequencer: at most one forwarded request
                    // in flight, in wire order. rmcp dispatches
                    // concurrently, and this surface's calls do not
                    // commute (an op's base_hash names the state
                    // the previous op produced) — forwarding the
                    // next request only after the previous response
                    // is out is what makes pipelined input behave
                    // exactly like sequential input.
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

/// Copy SDK responses from the duplex to the connection's
/// writer, one line at a time, marking each answered request
/// id complete for the guard's ordering wait. When the pump
/// ends (the SDK side is gone), it settles every outstanding
/// id so a sequencer wait can never wedge on a dead service.
pub async fn pump<W>(
    mut from_server: ReadHalf<DuplexStream>,
    out: Arc<Mutex<W>>,
    ordering: Arc<Ordering>,
) where
    W: AsyncWrite + Unpin + Send,
{
    let mut reader = tokio::io::BufReader::new(&mut from_server);
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => {
                ordering.abandon().await;
                return;
            }
            Ok(_) => {
                write_bytes(&out, &line).await;
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

    const TOK: &str = "sekrit";

    fn classify_str(s: &str) -> Action {
        classify(s.as_bytes(), TOK)
    }

    fn authed(method: &str, params: &str) -> String {
        format!(
            "{{\"id\":1,\"method\":\"{method}\",\"params\":{{\"_meta\":{{\"torogao/capability_token\":\"{TOK}\"}},\"x\":{params}}}}}"
        )
    }

    #[test]
    fn framing_errors_match_the_baseline_host() {
        match classify_str("this is not json") {
            Action::Respond(f) => {
                assert_eq!(f["error"]["code"], PARSE_ERROR);
                assert!(
                    f["error"]["message"]
                        .as_str()
                        .expect("message")
                        .starts_with("parse error: ")
                );
            }
            other => panic!("expected respond, got {other:?}"),
        }
        // Non-objects take the fleet framing contract.
        for frame in ["[1,2,3]", "42"] {
            match classify_str(frame) {
                Action::Respond(f) => {
                    assert_eq!(f["error"]["code"], INVALID_REQUEST, "{frame}");
                    assert_eq!(f["error"]["message"], "frame must be a JSON object");
                }
                other => panic!("expected respond for {frame}, got {other:?}"),
            }
        }
        // Method-less objects: the baseline read the method
        // first and answered method-not-found (empty name).
        for frame in ["{\"id\":1}", "{\"id\":1,\"method\":7}"] {
            match classify_str(frame) {
                Action::Respond(f) => {
                    assert_eq!(f["error"]["code"], METHOD_NOT_FOUND, "{frame}");
                    assert_eq!(f["error"]["message"], "method  not found");
                }
                other => panic!("expected respond for {frame}, got {other:?}"),
            }
        }
        match classify_str("{\"id\":3,\"method\":\"bogus/method\"}") {
            Action::Respond(f) => {
                assert_eq!(f["error"]["message"], "method bogus/method not found");
            }
            other => panic!("expected respond, got {other:?}"),
        }
    }

    #[test]
    fn blank_and_notification_frames_drop() {
        assert_eq!(classify_str("   "), Action::Drop);
        assert_eq!(
            classify_str("{\"method\":\"notifications/initialized\",\"params\":{}}"),
            Action::Drop
        );
        assert_eq!(
            classify_str("{\"id\":9,\"method\":\"notifications/cancelled\",\"params\":{}}"),
            Action::Drop
        );
        // An id-less tools/call is a notification: dropped,
        // unexecuted (the baseline answered it with id null).
        assert_eq!(
            classify_str("{\"method\":\"tools/call\",\"params\":{\"name\":\"editor_state\"}}"),
            Action::Drop
        );
    }

    #[test]
    fn initialize_is_refused_regardless_of_token() {
        let frame =
            "{\"id\":7,\"method\":\"initialize\",\"params\":{\"capability_token\":\"sekrit\"}}";
        match classify_str(frame) {
            Action::Respond(f) => {
                assert_eq!(f["error"]["code"], UNSUPPORTED_PROTOCOL_VERSION);
                assert_eq!(f["error"]["data"]["supported"], json!(["2026-07-28"]));
            }
            other => panic!("expected respond, got {other:?}"),
        }
    }

    #[test]
    fn auth_is_per_request_in_meta() {
        // No _meta at all.
        match classify_str("{\"id\":1,\"method\":\"tools/list\",\"params\":{}}") {
            Action::Respond(f) => {
                assert_eq!(f["error"]["code"], AUTH_REFUSED);
                assert_eq!(f["error"]["data"]["refusal"], "TG-SES-AUTH");
                assert_eq!(f["error"]["message"], "capability token refused");
            }
            other => panic!("expected respond, got {other:?}"),
        }
        // Wrong token.
        let wrong = "{\"id\":1,\"method\":\"tools/call\",\"params\":{\"_meta\":{\"torogao/capability_token\":\"wrong\"},\"name\":\"editor_state\",\"arguments\":{}}}";
        match classify_str(wrong) {
            Action::Respond(f) => assert_eq!(f["error"]["code"], AUTH_REFUSED),
            other => panic!("expected respond, got {other:?}"),
        }
        // The right token forwards, for every forwarded method.
        for method in ["server/discover", "ping", "tools/list"] {
            match classify_str(&authed(method, "{}")) {
                Action::Forward(_) => {}
                other => panic!("expected forward for {method}, got {other:?}"),
            }
        }
    }

    #[test]
    fn malformed_calls_normalize_to_baseline_degradation() {
        // Missing name becomes "" (dispatch answers the
        // unknown-tool refusal, as the baseline did).
        let frame = "{\"id\":1,\"method\":\"tools/call\",\"params\":{\"_meta\":{\"torogao/capability_token\":\"sekrit\"},\"arguments\":{}}}";
        match classify_str(frame) {
            Action::Forward(f) => assert_eq!(f["params"]["name"], ""),
            other => panic!("expected forward, got {other:?}"),
        }
        // Non-object arguments become {}.
        let frame = "{\"id\":2,\"method\":\"tools/call\",\"params\":{\"_meta\":{\"torogao/capability_token\":\"sekrit\"},\"name\":\"editor_state\",\"arguments\":\"nope\"}}";
        match classify_str(frame) {
            Action::Forward(f) => assert_eq!(f["params"]["arguments"], json!({})),
            other => panic!("expected forward, got {other:?}"),
        }
        // Params missing entirely normalizes to {} — but the
        // token check already failed on the absent _meta, so
        // this shape is refused, not forwarded.
        match classify_str("{\"id\":3,\"method\":\"tools/call\"}") {
            Action::Respond(f) => assert_eq!(f["error"]["code"], AUTH_REFUSED),
            other => panic!("expected respond, got {other:?}"),
        }
    }
}
