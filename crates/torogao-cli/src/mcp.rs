//! The MCP server mode (`torogao-cli mcp`): the tool registry
//! over stdio, modern protocol era only (2026-07-28).
//!
//! As of WS2-W5 (MCP standardization program, decision D-113)
//! the framing is rmcp 3.8.0 behind a byte-level era guard
//! (`mcp_guard`); the handler (`mcp_server`) delegates every
//! call to the one registry, exactly as the hand-rolled
//! NDJSON server did. There is no `initialize` handshake:
//! sessions open with `server/discover`, every request carries
//! its protocol version in `_meta`, and legacy contact is
//! refused by the guard with the typed `-32022` error. Tool
//! execution failures remain normal tool results with
//! `isError` set; protocol problems remain JSON-RPC errors
//! with the stable machine code in `error.data.code`.

use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::transport::async_rw::AsyncRwTransport;
use tokio::sync::Mutex;

use crate::mcp_guard;
use crate::mcp_server::CliServer;

/// Duplex buffer between the era guard and the SDK service.
/// Request lines can reach the 4 MiB line bound; the SDK
/// consumes incrementally, so this only needs to cover
/// response bursts.
const DUPLEX_BUFFER_BYTES: usize = 1024 * 1024;

/// Wire the pieces together: the guard owns stdio, the SDK
/// service sits behind an in-process duplex, and a pump copies
/// its responses back to stdout through the same lock the
/// guard writes with.
async fn serve() -> std::io::Result<()> {
    let (client_side, server_side) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let (server_read, server_write) = tokio::io::split(server_side);
    let (client_read, client_write) = tokio::io::split(client_side);
    let stdout: mcp_guard::SharedStdout = Arc::new(Mutex::new(tokio::io::stdout()));

    let ordering = mcp_guard::Ordering::new();
    let handler = CliServer;
    let serve = tokio::spawn(async move {
        let transport = AsyncRwTransport::new_server(server_read, server_write);
        match handler.serve(transport).await {
            Ok(running) => running
                .waiting()
                .await
                .map(|_| ())
                .map_err(|e| std::io::Error::other(e.to_string())),
            // The client closed the connection before selecting a
            // lifecycle (a probe, a spawn check, a client that only
            // ever wanted the refusal): the baseline treated bare
            // EOF as a clean end of session, and so do we.
            Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => Ok(()),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    });
    let mut pump = tokio::spawn(mcp_guard::pump(
        client_read,
        Arc::clone(&stdout),
        Arc::clone(&ordering),
    ));

    // When stdin reaches EOF the guard shuts the duplex write
    // side down, which ends the SDK service; the pump ends when
    // the service's transport drops. The pump wait is bounded
    // as a backstop: a response still in flight when the service
    // ends gets its chance to drain, but a wedged pump cannot
    // hang exit.
    let guard_result = mcp_guard::run(tokio::io::stdin(), client_write, stdout, ordering).await;
    let serve_result = match serve.await {
        Ok(result) => result,
        Err(e) => Err(std::io::Error::other(e.to_string())),
    };
    if tokio::time::timeout(std::time::Duration::from_secs(2), &mut pump)
        .await
        .is_err()
    {
        pump.abort();
    }
    guard_result.and(serve_result)
}

/// Runs the MCP server on stdio until the input reaches EOF;
/// returns 0, or 1 if the runtime or serve loop failed.
pub fn run_mcp_server() -> i32 {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("torogao mcp: cannot start runtime: {e}");
            return 1;
        }
    };
    match runtime.block_on(serve()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("torogao mcp: serve loop ended: {e}");
            1
        }
    }
}
