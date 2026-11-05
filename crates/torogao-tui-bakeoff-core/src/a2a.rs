//! A minimal signed Agent Card and task exchange over loopback.
//!
//! The estate's existing A2A card is unsigned, so this prototype
//! supplies the missing signature seam itself: an Ed25519
//! signature over a canonical JSON Agent Card. The server and
//! client are two local TCP endpoints; the task asks for the real
//! rig census and returns a completed task with that artifact.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{Value, json};

use crate::data::RigData;
use crate::error::{BakeoffError, Result};

const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 64 * 1024;
const SIGNING_SEED: [u8; 32] = [0x42; 32];
const TASK_ID: &str = "bakeoff-task-1";

/// The measured A2A outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct A2aOutcome {
    /// Whether the client verified the signed Agent Card.
    pub card_verified: bool,
    /// The completed task identifier.
    pub task_id: String,
    /// The task state returned by the server endpoint.
    pub task_state: String,
    /// The tool named by the returned task artifact.
    pub artifact_tool: String,
}

/// Runs one signed-card verification and one completed task over
/// two loopback TCP connections.
///
/// # Errors
///
/// Returns a typed error for a bind, HTTP, JSON, signature, or
/// task-contract failure.
pub fn run_a2a_exchange(rig: &RigData) -> Result<A2aOutcome> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| BakeoffError::Network(format!("A2A bind failed: {error}")))?;
    let address = listener
        .local_addr()
        .map_err(|error| BakeoffError::Network(format!("A2A address failed: {error}")))?;
    let base_url = format!("http://{address}");
    let server_rig = rig.clone();
    let server = thread::spawn(move || serve_two_requests(listener, &server_rig, &base_url));

    let card_response = http_get(address, "/.well-known/agent-card.json")?;
    let (card_verified, card) = verify_card_document(&card_response)?;
    if !card_verified {
        return Err(BakeoffError::Crypto(
            "the loopback Agent Card signature did not verify".to_string(),
        ));
    }
    if card.get("name").and_then(Value::as_str) != Some("Torogao Bake-off Agent") {
        return Err(BakeoffError::Protocol(
            "the verified Agent Card names a different agent".to_string(),
        ));
    }

    let request = json!({
        "id": "bakeoff-request-1",
        "jsonrpc": "2.0",
        "method": "message/send",
        "params": {
            "message": {
                "messageId": "bakeoff-message-1",
                "parts": [{"kind": "data", "data": {"tool": "rig_census"}}],
                "role": "user"
            }
        }
    });
    let task_response = http_post_json(address, "/", &request)?;
    let task = task_response
        .get("result")
        .ok_or_else(|| BakeoffError::Protocol("A2A response omitted result".to_string()))?;
    let outcome = A2aOutcome {
        card_verified,
        task_id: required_string(task, "id")?,
        task_state: task
            .get("status")
            .and_then(|status| status.get("state"))
            .and_then(Value::as_str)
            .ok_or_else(|| BakeoffError::Protocol("A2A task omitted status.state".to_string()))?
            .to_string(),
        artifact_tool: task
            .get("artifacts")
            .and_then(Value::as_array)
            .and_then(|artifacts| artifacts.first())
            .and_then(|artifact| artifact.get("parts"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(|part| part.get("data"))
            .and_then(|data| data.get("tool"))
            .and_then(Value::as_str)
            .ok_or_else(|| BakeoffError::Protocol("A2A task omitted artifact tool".to_string()))?
            .to_string(),
    };
    server
        .join()
        .map_err(|_| BakeoffError::Network("A2A server thread panicked".to_string()))??;
    if outcome.task_id != TASK_ID || outcome.task_state != "completed" {
        return Err(BakeoffError::Protocol(format!(
            "A2A task ended as {} in state {}",
            outcome.task_id, outcome.task_state
        )));
    }
    Ok(outcome)
}

/// Builds the signed card document for one loopback base URL.
pub fn signed_agent_card(base_url: &str) -> Value {
    let mut card = unsigned_agent_card(base_url);
    let signing_key = SigningKey::from_bytes(&SIGNING_SEED);
    let signature = signing_key.sign(&canonical_card_bytes(&card));
    card["signature"] = json!(hex_encode(&signature.to_bytes()));
    card["signerPublicKey"] = json!(hex_encode(signing_key.verifying_key().as_bytes()));
    card
}

fn unsigned_agent_card(base_url: &str) -> Value {
    json!({
        "capabilities": {"streaming": false},
        "defaultInputModes": ["application/json"],
        "defaultOutputModes": ["application/json"],
        "description": "Returns the measured Torogao rig census for the TUI bake-off.",
        "documentationUrl": format!("{base_url}/docs"),
        "name": "Torogao Bake-off Agent",
        "protocolVersion": "0.3.0",
        "skills": [{
            "description": "Return the rig census measured through torogao-author.",
            "id": "rig_census",
            "name": "Rig census",
            "tags": ["torogao", "rig"]
        }],
        "url": base_url,
        "version": "1.0.0"
    })
}

/// Verifies a signed card document without trusting its key or
/// signature fields as part of the signed payload.
pub fn verify_card_document(document: &Value) -> Result<(bool, Value)> {
    let signature_hex = required_string(document, "signature")?;
    let public_hex = required_string(document, "signerPublicKey")?;
    let signature_bytes = hex_decode_fixed::<64>(&signature_hex)?;
    let public_bytes = hex_decode_fixed::<32>(&public_hex)?;
    let verifying_key = VerifyingKey::from_bytes(&public_bytes)
        .map_err(|error| BakeoffError::Crypto(format!("Agent Card public key: {error}")))?;
    let signature = Signature::from_bytes(&signature_bytes);
    let mut card = document.clone();
    if let Some(object) = card.as_object_mut() {
        object.remove("signature");
        object.remove("signerPublicKey");
    }
    let verified = verifying_key
        .verify(&canonical_card_bytes(&card), &signature)
        .is_ok();
    Ok((verified, card))
}

fn canonical_card_bytes(card: &Value) -> Vec<u8> {
    serde_json::to_vec(card).expect("a serde_json::Value always serializes")
}

fn serve_two_requests(listener: TcpListener, rig: &RigData, base_url: &str) -> Result<()> {
    for request_number in 0..2 {
        let (mut stream, _) = listener
            .accept()
            .map_err(|error| BakeoffError::Network(format!("A2A accept failed: {error}")))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| BakeoffError::Network(format!("A2A timeout setup: {error}")))?;
        let (method, _path, body) = read_http_request(&mut stream)?;
        let response = if request_number == 0 && method == "GET" {
            signed_agent_card(base_url)
        } else if request_number == 1 && method == "POST" {
            task_response(&body, rig)?
        } else {
            return Err(BakeoffError::Protocol(format!(
                "A2A request {request_number} used unexpected method {method}"
            )));
        };
        write_http_json(&mut stream, &response)?;
        stream
            .shutdown(Shutdown::Both)
            .map_err(|error| BakeoffError::Network(format!("A2A shutdown failed: {error}")))?;
    }
    Ok(())
}

fn task_response(body: &[u8], rig: &RigData) -> Result<Value> {
    let request: Value = serde_json::from_slice(body)?;
    if request.get("method").and_then(Value::as_str) != Some("message/send") {
        return Err(BakeoffError::Protocol(
            "A2A task request used a method other than message/send".to_string(),
        ));
    }
    let tool = request
        .pointer("/params/message/parts/0/data/tool")
        .and_then(Value::as_str)
        .ok_or_else(|| BakeoffError::Protocol("A2A task omitted its tool".to_string()))?;
    if tool != "rig_census" {
        return Err(BakeoffError::Protocol(format!(
            "A2A task requested unsupported tool '{tool}'"
        )));
    }
    let mut census = rig.to_json();
    census["tool"] = json!("rig_census");
    Ok(json!({
        "id": request.get("id").cloned().unwrap_or(Value::Null),
        "jsonrpc": "2.0",
        "result": {
            "artifacts": [{
                "artifactId": "bakeoff-artifact-1",
                "name": "rig-census",
                "parts": [{"data": census, "kind": "data"}]
            }],
            "id": TASK_ID,
            "kind": "task",
            "status": {"state": "completed"}
        }
    }))
}

fn http_get(address: std::net::SocketAddr, path: &str) -> Result<Value> {
    let mut stream = connect(address)?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| BakeoffError::Network(format!("A2A GET write failed: {error}")))?;
    read_http_json(&mut stream)
}

fn http_post_json(address: std::net::SocketAddr, path: &str, body: &Value) -> Result<Value> {
    let payload = serde_json::to_vec(body)?;
    let mut stream = connect(address)?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.write_all(&payload))
        .map_err(|error| BakeoffError::Network(format!("A2A POST write failed: {error}")))?;
    read_http_json(&mut stream)
}

fn connect(address: std::net::SocketAddr) -> Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| BakeoffError::Network(format!("A2A connect failed: {error}")))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| BakeoffError::Network(format!("A2A timeout setup: {error}")))?;
    Ok(stream)
}

fn read_http_json(stream: &mut TcpStream) -> Result<Value> {
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .map_err(|error| BakeoffError::Network(format!("A2A response read failed: {error}")))?;
    let split = find_header_end(&bytes)
        .ok_or_else(|| BakeoffError::Protocol("A2A response omitted header end".to_string()))?;
    let body = &bytes[split + 4..];
    if body.len() > MAX_BODY_BYTES {
        return Err(BakeoffError::Protocol(
            "A2A response exceeded the 65536-byte workload bound".to_string(),
        ));
    }
    Ok(serde_json::from_slice(body)?)
}

fn read_http_request(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        if let Some(position) = find_header_end(&bytes) {
            break position;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(BakeoffError::Protocol(
                "A2A request header exceeded the 16384-byte bound".to_string(),
            ));
        }
        let read = stream
            .read(&mut buffer)
            .map_err(|error| BakeoffError::Network(format!("A2A request read: {error}")))?;
        if read == 0 {
            return Err(BakeoffError::Protocol(
                "A2A request ended before its headers completed".to_string(),
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
    };
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| BakeoffError::Protocol("A2A request header is not UTF-8".to_string()))?;
    let mut first = header.split_whitespace();
    let method = first.next().unwrap_or_default().to_string();
    let path = first.next().unwrap_or_default().to_string();
    let content_length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        return Err(BakeoffError::Protocol(
            "A2A request body exceeded the 65536-byte bound".to_string(),
        ));
    }
    let mut body = bytes.split_off(header_end + 4);
    while body.len() < content_length {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| BakeoffError::Network(format!("A2A body read: {error}")))?;
        if read == 0 {
            return Err(BakeoffError::Protocol(
                "A2A request body ended early".to_string(),
            ));
        }
        body.extend_from_slice(&buffer[..read]);
    }
    body.truncate(content_length);
    Ok((method, path, body))
}

fn write_http_json(stream: &mut TcpStream, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(&body))
        .map_err(|error| BakeoffError::Network(format!("A2A response write failed: {error}")))
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn required_string(document: &Value, field: &str) -> Result<String> {
    document
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| BakeoffError::Protocol(format!("record omitted string field '{field}'")))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_decode_fixed<const N: usize>(text: &str) -> Result<[u8; N]> {
    if text.len() != N * 2 {
        return Err(BakeoffError::Crypto(format!(
            "hex field carries {} characters; the bound is exactly {}",
            text.len(),
            N * 2
        )));
    }
    let mut output = [0_u8; N];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

fn hex_value(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(BakeoffError::Crypto(
            "hex field contains a non-hex character".to_string(),
        )),
    }
}
