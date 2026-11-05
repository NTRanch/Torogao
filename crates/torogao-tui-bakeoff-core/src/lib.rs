//! Shared workload core for the Torogao TUI bake-off (D-926).
//!
//! Both rendering routes consume the same measured scenario:
//! an ML-KEM-768 session handshake with a tamper rejection, one
//! OpenAPPA-style gated MCP census call and one expected refusal,
//! one signed-card A2A task over loopback, and the real rig and
//! scene records measured from the Torogao tree.

#![forbid(unsafe_code)]

pub mod a2a;
pub mod data;
pub mod error;
pub mod mcp;
pub mod policy;
pub mod pqc;
pub mod scenario;
