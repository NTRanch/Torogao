//! The Torogao command-line tools.
//!
//! This crate will own the tool registry and every surface that
//! drives it: the subcommand-line interface, the MCP server, and
//! the A2A server. The phase harness suite under `tests/` is its
//! behavior contract; the port is done, surface by surface, when
//! the harnesses pass against this binary.
//!
//! Port phase: RP0 ships a skeleton. It answers `--help` and
//! `--version`; every other invocation is refused with a typed
//! not-implemented error so the harnesses fail against it honestly
//! — the machinery is exercised, nothing is implemented. RP1 ports
//! the real tool surface.

#![forbid(unsafe_code)]

use std::io::Write;

/// Exit code for a refused (not-yet-implemented) invocation.
const EXIT_NOT_IMPLEMENTED: i32 = 2;

/// What one invocation asks the skeleton to do.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// Print help to stdout and exit 0.
    Help,
    /// Refuse: the named invocation is not implemented at RP0.
    NotImplemented(String),
    /// Print the version to stdout and exit 0.
    Version,
}

/// Classifies raw arguments into the skeleton's one decision.
///
/// Only the first argument is inspected: `--help` and `--version`
/// are answered; anything else (including no arguments at all)
/// names the refused invocation.
fn classify(args: &[String]) -> Action {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => Action::Help,
        Some("--version" | "-V") => Action::Version,
        Some(first) => Action::NotImplemented(first.to_owned()),
        None => Action::NotImplemented("no command".to_owned()),
    }
}

/// Help text printed for `--help` (and the skeleton's whole
/// documented surface: two flags and a promise).
fn help_text() -> String {
    format!(
        "torogao-cli {version} — the Torogao command-line tools\n\
         \n\
         USAGE:\n\
         \x20   torogao-cli --help\n\
         \x20   torogao-cli --version\n\
         \n\
         The tool surface (registry, MCP and A2A servers, scenes,\n\
         writer, tutorials, toolbox, conversion) is not implemented\n\
         in the RP0 skeleton; every other invocation is refused with\n\
         a typed not-implemented error.\n",
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// Runs one invocation, writing normal output to `stdout` and
/// refusals to `stderr`, and returns the process exit code.
///
/// Stream writes are best-effort by contract: a closed stream
/// (e.g. piping `--help` into a terminated reader) must not turn
/// an answered invocation into a panic; the exit code carries the
/// outcome.
pub fn run(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    match classify(args) {
        Action::Help => {
            let _ = stdout.write_all(help_text().as_bytes());
            0
        }
        Action::Version => {
            let _ = writeln!(stdout, "torogao-cli {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Action::NotImplemented(name) => {
            let _ = writeln!(
                stderr,
                "torogao-cli: error TG-NOT-IMPLEMENTED: not-implemented \
                 (RP0 skeleton): {name}"
            );
            EXIT_NOT_IMPLEMENTED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, classify, run};

    fn run_capture(args: &[&str]) -> (i32, String, String) {
        let owned: Vec<String> = args.iter().map(ToString::to_string).collect();
        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let code = run(&owned, &mut stdout, &mut stderr);
        (
            code,
            String::from_utf8(stdout).expect("stdout is UTF-8"),
            String::from_utf8(stderr).expect("stderr is UTF-8"),
        )
    }

    #[test]
    fn classify_answers_only_help_and_version() {
        assert_eq!(classify(&["--help".to_owned()]), Action::Help);
        assert_eq!(classify(&["--version".to_owned()]), Action::Version);
        assert_eq!(
            classify(&["validate".to_owned()]),
            Action::NotImplemented("validate".to_owned()),
        );
        assert_eq!(
            classify(&[]),
            Action::NotImplemented("no command".to_owned()),
        );
    }

    #[test]
    fn help_exits_zero_and_documents_the_refusal() {
        let (code, stdout, stderr) = run_capture(&["--help"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("torogao-cli --help"), "stdout: {stdout}");
        assert!(stdout.contains("not implemented"), "stdout: {stdout}");
        assert!(stderr.is_empty(), "stderr: {stderr}");
    }

    #[test]
    fn version_exits_zero_with_the_crate_version() {
        let (code, stdout, _stderr) = run_capture(&["--version"]);
        assert_eq!(code, 0);
        assert_eq!(
            stdout,
            format!("torogao-cli {}\n", env!("CARGO_PKG_VERSION")),
        );
    }

    #[test]
    fn any_other_invocation_is_a_typed_refusal_on_stderr() {
        for args in [&["validate", "x.inx"][..], &["mcp"][..], &[][..]] {
            let (code, stdout, stderr) = run_capture(args);
            assert_ne!(code, 0, "args: {args:?}");
            assert!(stdout.is_empty(), "args {args:?} stdout: {stdout}");
            assert!(
                stderr.contains("not-implemented (RP0 skeleton)"),
                "args {args:?} stderr: {stderr}",
            );
        }
    }
}
