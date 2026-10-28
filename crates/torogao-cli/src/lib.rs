//! The Torogao command-line tools.
//!
//! This crate owns the tool registry and every surface
//! that drives it: the subcommand-line interface, the MCP
//! server, and the A2A server. The phase harness suite
//! under `tests/` is its behavior contract.

#![forbid(unsafe_code)]

pub mod a2a;
pub mod app;
pub mod audiobake;
pub mod audioproc;
pub mod controllers;
pub mod creator;
pub mod criteria;
pub mod curves;
pub mod drivers;
pub mod effects;
pub mod extdoc;
pub mod families;
pub mod fluids;
pub mod glb;
pub mod guards;
pub mod handlers;
pub mod inspect;
pub mod lipsync;
pub mod mcp;
pub mod mcp_guard;
pub mod mcp_server;
pub mod packs;
pub mod paths;
pub mod playroom;
pub mod preview;
pub mod registry;
pub mod registry_data;
pub mod rigexport;
pub mod scenes;
pub mod sessionbuild;
pub mod specials;
pub mod templates;
pub mod tracking;
pub mod tutorials;
pub mod util;
pub mod video;
pub mod viseme;
pub mod walk;
pub mod wav;
pub mod writer;
pub mod writerlane;

use std::io::Write;

/// Runs one invocation, writing normal output to `stdout`
/// and diagnostics to `stderr`, and returns the process
/// exit code.
///
/// Stream writes are best-effort by contract: a closed
/// stream (e.g. piping into a terminated reader) must not
/// turn an answered invocation into a panic; the exit
/// code carries the outcome.
pub fn run(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    app::run(args, stdout, stderr)
}
