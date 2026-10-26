//! The Torogao command-line tools.
//!
//! This crate owns the tool registry and every surface
//! that drives it: the subcommand-line interface, the MCP
//! server, and the A2A server. The phase harness suite
//! under `tests/` is its behavior contract.

#![forbid(unsafe_code)]

pub mod a2a;
pub mod app;
pub mod controllers;
pub mod criteria;
pub mod curves;
pub mod extdoc;
pub mod families;
pub mod guards;
pub mod handlers;
pub mod inspect;
pub mod mcp;
pub mod packs;
pub mod paths;
pub mod preview;
pub mod registry;
pub mod registry_data;
pub mod rigexport;
pub mod specials;
pub mod util;
pub mod walk;

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
