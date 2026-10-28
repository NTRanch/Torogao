//! Entry point for the `torogao-cli` binary.
//!
//! All behavior lives in the library (`torogao_cli::run`) so the
//! skeleton's contract is unit-tested in-process; this function
//! only wires argv and the standard streams to it.
//!
//! The streams are passed as unlocked handles, deliberately:
//! a `StdoutLock` held for the process lifetime keeps std's
//! global stdout lock acquired, and the `mcp` mode's async
//! writer (tokio's stdout, a separate handle to the same fd)
//! would block on that lock forever. Per-call locking keeps
//! output identical.

#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let code = torogao_cli::run(&args, &mut stdout, &mut stderr);
    // The skeleton's exit codes are 0 (help/version) or 2 (refused);
    // both fit the ExitCode conversion without loss.
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
