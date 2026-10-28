//! Transports for the session host: stdio line framing
//! (spawned clients) and WebSocket on loopback (the app
//! endpoint) — both carrying the modern-era MCP byte stream
//! behind the era guard since WS2-W5.
//!
//! The two transports differ only in framing, exactly as in
//! Phase C:
//!
//! - stdio: newline-delimited bytes straight into the guard.
//! - WebSocket: text messages are the frames. rmcp 3.8.0 has
//!   no WebSocket transport, so the sync tungstenite endpoint
//!   (unchanged from Phase C: loopback bind, token file at
//!   0600 in a 0700 session dir, one connection served to
//!   completion before the next accept) adapts at the byte
//!   seam via `ws_stream`: inbound messages become lines for
//!   the guard, outbound lines become text messages.
//!
//! Both transports run the same pipeline shape: era guard +
//! rmcp service over a 1 MiB duplex, sharing one
//! `DispatchHandle` (the session owner thread). WebSocket
//! connections each get a fresh guard + service over the
//! same handle — the session persists across connections, as
//! it did under Phase C's accept loop.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rmcp::service::serve_server;
use rmcp::transport::async_rw::AsyncRwTransport;
use tokio::io::duplex;
use tokio::sync::Mutex;

use crate::dispatch::{self, DispatchHandle};
use crate::host::{Host, LiveBridge};
use crate::mcp_guard;
use crate::mcp_server::SessionServer;
use crate::refusal::{R, Refusal};
use crate::session::Session;
use crate::ws_stream::{ChannelRead, LineWrite};

/// Default loopback port for the WebSocket endpoint.
pub const DEFAULT_WS_PORT: u16 = 9770;
/// Pump-drain bound after the guard ends (WS2-W1 fix: a dead
/// service must not hold the connection open).
const PUMP_DRAIN: std::time::Duration = std::time::Duration::from_secs(2);
/// Bytes in flight between the guard and the SDK service —
/// the same duplex the CLI surface and the fleet use.
const DUPLEX_CAPACITY: usize = 1024 * 1024;
/// How long one WebSocket connection thread idles between
/// polls of its socket and the outbound channel.
const WS_POLL: std::time::Duration = std::time::Duration::from_millis(2);
/// Bound on waiting for a closing connection's pipeline to
/// settle before aborting it, so the accept loop's
/// one-at-a-time discipline cannot deadlock on a stuck peer.
const WS_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

/// Connect-time filesystem hardening for a session dir and
/// its capability token: creates the dir (0700) if needed and
/// writes a fresh 32-byte random token (0600). Returns the
/// token. The token never leaves the host process except
/// through this file: clients read the file, the guard
/// checks `_meta` against its content.
pub fn make_token_file(session_dir: &Path) -> std::io::Result<String> {
    if !session_dir.exists() {
        std::fs::create_dir_all(session_dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(session_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut raw = [0u8; 32];
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(&mut raw)?;
    }
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let token_path: PathBuf = session_dir.join("capability-token");
    std::fs::write(&token_path, format!("{token}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(token)
}

/// A fresh runtime for one serve call. Worker threads are
/// bounded at two: the guard and the pump are I/O tasks, and
/// the session's compute runs on the owner thread.
fn build_runtime() -> R<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|err| Refusal::new("TG-SES-IO", format!("tokio runtime failed: {err}")))
}

/// The pipeline both transports share: era guard + SDK
/// service + response pump over one duplex, all against the
/// shared dispatch handle.
async fn run_pipeline<ReaderT, WriterT>(
    reader: ReaderT,
    writer: WriterT,
    dispatch: DispatchHandle,
    token: Arc<str>,
) where
    ReaderT: tokio::io::AsyncBufRead + Unpin + Send + 'static,
    WriterT: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (client_side, server_side) = duplex(DUPLEX_CAPACITY);
    let (client_read, client_write) = tokio::io::split(client_side);
    let (server_read, server_write) = tokio::io::split(server_side);
    let ordering = mcp_guard::Ordering::new();
    let out = Arc::new(Mutex::new(writer));
    let serve = serve_server(
        SessionServer::new(dispatch),
        AsyncRwTransport::new_server(server_read, server_write),
    );
    let pump = tokio::spawn(mcp_guard::pump(client_read, out.clone(), ordering.clone()));
    // The serve future resolves to the RunningService once the
    // peer selects the modern lifecycle; the service lives
    // exactly as long as `waiting()` is held. Dropping the
    // handle instead would close the service after the first
    // request — the fleet pattern (lupin, torogao-cli) holds
    // it for the connection's whole life.
    tokio::spawn(async move {
        match serve.await {
            Ok(running) => {
                let _ = running.waiting().await;
            }
            // The peer closed before selecting a lifecycle (a
            // probe, a client that only wanted the -32022
            // refusal): a clean end, as on the fleet template.
            Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => {}
            Err(_) => {}
        }
    });
    let _ = mcp_guard::run(reader, client_write, out, ordering, token).await;
    let _ = tokio::time::timeout(PUMP_DRAIN, pump).await;
}

/// Serve the session over stdio until EOF. Blocks the caller;
/// returns failures as typed refusals.
pub fn serve_stdio(host: Host, bridge: Box<dyn LiveBridge + Send>, token: &str) -> R<()> {
    let dispatch = dispatch::spawn(host, bridge);
    let token: Arc<str> = Arc::from(token);
    let runtime = build_runtime()?;
    runtime.block_on(async move {
        let reader = tokio::io::BufReader::new(tokio::io::stdin());
        run_pipeline(reader, tokio::io::stdout(), dispatch, token).await;
    });
    Ok(())
}

/// Serve the session over WebSocket on loopback until the
/// listener fails. Blocks the caller. Connections are served
/// one at a time to completion, as in Phase C; the session
/// (behind the shared dispatch handle) persists across them.
pub fn serve_ws(host: Host, bridge: Box<dyn LiveBridge + Send>, token: &str, port: u16) -> R<()> {
    let dispatch = dispatch::spawn(host, bridge);
    let token: Arc<str> = Arc::from(token);
    let runtime = build_runtime()?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|err| Refusal::new("TG-SES-IO", format!("ws bind failed: {err}")))?;
    for stream in listener.incoming() {
        let stream =
            stream.map_err(|err| Refusal::new("TG-SES-IO", format!("ws accept failed: {err}")))?;
        serve_ws_connection(stream, &runtime, dispatch.clone(), token.clone());
    }
    Ok(())
}

/// One WebSocket connection: adapt the socket to the byte
/// seam and run the pipeline to completion.
fn serve_ws_connection(
    stream: std::net::TcpStream,
    runtime: &tokio::runtime::Runtime,
    dispatch: DispatchHandle,
    token: Arc<str>,
) {
    let Ok(mut socket) = tungstenite::accept(stream) else {
        return;
    };
    // Non-blocking so this thread can interleave socket reads
    // with outbound lines from the pipeline.
    let _ = socket.get_mut().set_nonblocking(true);
    let (inbound_tx, inbound_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let (outbound_tx, outbound_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let reader = tokio::io::BufReader::new(ChannelRead::new(inbound_rx));
    let writer = LineWrite::new(outbound_tx);
    let pipeline = runtime.spawn(run_pipeline(reader, writer, dispatch, token));
    // This thread owns the socket: it feeds inbound messages
    // to the pipeline and drains outbound lines back as text
    // messages until the client closes, the pipeline ends, or
    // the socket fails. Outbound drains after every read
    // attempt, so a finished pipeline's last line is never
    // stranded in the channel.
    loop {
        let mut inbound_broken = false;
        match socket.read() {
            Ok(msg @ (tungstenite::Message::Text(_) | tungstenite::Message::Binary(_))) => {
                let mut bytes = msg.into_data().to_vec();
                bytes.push(b'\n');
                if inbound_tx.blocking_send(bytes).is_err() {
                    inbound_broken = true;
                }
            }
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        drain_outbound(&mut socket, &outbound_rx);
        if inbound_broken || pipeline.is_finished() {
            break;
        }
        std::thread::sleep(WS_POLL);
    }
    // EOF for the guard, a courtesy close for the peer, then a
    // bounded wait so the accept loop stays one-at-a-time.
    drop(inbound_tx);
    let _ = socket.close(None);
    let deadline = std::time::Instant::now() + WS_SETTLE;
    while !pipeline.is_finished() && std::time::Instant::now() < deadline {
        std::thread::sleep(WS_POLL);
    }
    if !pipeline.is_finished() {
        pipeline.abort();
    }
}

/// Forward any queued outbound lines as text messages.
fn drain_outbound(
    socket: &mut tungstenite::WebSocket<std::net::TcpStream>,
    outbound_rx: &std::sync::mpsc::Receiver<Vec<u8>>,
) {
    while let Ok(line) = outbound_rx.try_recv() {
        let text = String::from_utf8_lossy(&line).into_owned();
        if socket
            .send(tungstenite::Message::Text(text.into()))
            .is_err()
        {
            return;
        }
    }
}

/// The full session bootstrap on stdio: token file, session,
/// stderr banner (Phase C's connect line, kept in shape —
/// embeddings and drivers scrape the session id from it),
/// then serve until EOF.
pub fn serve_stdio_session(
    genesis: crate::state::EditorState,
    param_defaults: &std::collections::BTreeMap<String, f32>,
    held_by: &str,
    session_dir: &Path,
    bridge: Box<dyn LiveBridge + Send>,
) -> std::io::Result<()> {
    let token = make_token_file(session_dir)?;
    let session = Session::new(genesis, param_defaults, held_by);
    let host = Host::new(session);
    eprintln!(
        "torogao session {} serving on stdio (token in capability-token file; protocol MCP {})",
        host.session.id,
        mcp_guard::MODERN_VERSION,
    );
    serve_stdio(host, bridge, &token).map_err(|r| std::io::Error::other(r.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_file_shape_and_permissions() {
        let dir = std::env::temp_dir().join(format!("tg-ws2w5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let token = make_token_file(&dir).expect("token file written");
        assert_eq!(token.len(), 64, "32 random bytes as hex");
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        let file = dir.join("capability-token");
        assert_eq!(
            std::fs::read_to_string(&file)
                .expect("token readable")
                .trim(),
            token,
            "the file carries the token (newline-terminated, as in Phase C)"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file)
                .expect("file meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
