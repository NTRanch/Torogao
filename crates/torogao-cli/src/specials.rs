//! The special subcommands (everything outside the
//! generic project tail): each family lands here as its
//! port completes. Returns None when the invocation is
//! not a special command.

use std::io::Write;

/// Runs a special subcommand, if `args` names one.
pub fn run_special(
    _args: &[String],
    _out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Option<i32> {
    None
}

/// The P5 subcommand family's interception (runs before
/// the other specials, as in the reference's main).
/// Returns None when the invocation is not a P5 command.
pub fn run_p5_cli(
    _args: &[String],
    _out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Option<i32> {
    None
}
