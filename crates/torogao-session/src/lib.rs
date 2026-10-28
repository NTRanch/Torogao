//! torogao-session: the editor-state interop core of the
//! Torogao app — the session host's state, gate, and record.
//!
//! A session is a snapshot of the shell's live state
//! (`torogao-editor-state/1`), an append-only log of typed
//! ops, and one write gate: every authoring op is decided by
//! `torogao-author`'s AUTH-D2 `decide()` (see `gate`), never
//! by a local re-implementation. Sessions compile to a
//! canonical record (genesis RON + op log + final RON) whose
//! replay must reproduce the live state hash exactly.
//!
//! This crate is the Slice-1 harness core, productised: the
//! schema, canonical RON form, hash, and record layout are
//! unchanged; the harness's `decide()` stub is gone — the
//! gate module routes through the real authoring gate.
//!
//! Since WS2-W5 the wire is the modern MCP era (2026-07-28)
//! on rmcp 3.8.0 behind the era guard in `mcp_guard`, with
//! explicit handles: the session's handle (id + genesis and
//! state hashes) rides in the `server/discover` result's
//! `_meta`, and the capability token — the same token-file
//! discipline (0600 in a 0700 session dir) the Phase C
//! handshake used — rides in every request's `_meta`, never
//! in tool arguments. Domain state stays confined to one
//! owner thread (`dispatch`), as it was confined to Phase
//! C's single serving loop.

#![forbid(unsafe_code)]

pub mod canon;
pub mod dispatch;
pub mod gate;
pub mod host;
pub mod mcp_guard;
pub mod mcp_server;
pub mod op;
pub mod refusal;
pub mod registry;
pub mod session;
pub mod state;
pub mod transport;
pub mod ws_stream;

pub use canon::{from_canon_ron, state_hash, to_canon_ron};
pub use dispatch::{DispatchHandle, SessionInfo};
pub use gate::{GateMirror, GateVerdict};
pub use host::{Host, LiveBridge, NoLiveBridge, PlaySnapshot};
pub use op::Op;
pub use refusal::{R, Refusal};
pub use registry::{agent_card, registry, tool_json};
pub use session::{Admission, CompileOutcome, LogEntry, Session, compile_session, replay_record};
pub use state::{EditorState, STATE_FORMAT};
