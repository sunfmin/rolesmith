//! The frame every command runs in (SPEC.md §8): parsing, `--json`, stderr, exit codes.

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use serde_json::{Value, json};

use crate::commands::{Failure, Output, Registered};

/// Success.
pub const EXIT_OK: u8 = 0;
/// The command refused, or something it needed failed.
pub const EXIT_FAILED: u8 = 1;
/// The command line was not one rolesmith accepts.
pub const EXIT_USAGE: u8 = 2;

/// Roles for any coding agent.
#[derive(Parser)]
#[command(name = "rolesmith", version, arg_required_else_help = true)]
pub struct Cli {
    /// Print one JSON document on stdout instead of human text.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Registered,
}

/// The parser's definition: what `agent-context` describes and what parses the command line.
pub fn command() -> clap::Command {
    Cli::command()
}

/// Run rolesmith on a command line (the first item is the program name).
pub fn main(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args: Vec<OsString> = args.into_iter().collect();
    let code = run(
        &args,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}

/// Parse, run and report; answers the exit code. Writes go to the given streams so tests can
/// watch them; a closed stdout (`| head`) is not an error of the command.
pub fn run(args: &[OsString], out: &mut impl Write, err: &mut impl Write) -> u8 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => return usage(&e, wants_json(args), out, err),
    };
    report(cli.command.run(), cli.json, out, err)
}

/// Print what a command produced or why it failed, and answer the exit code.
pub fn report(
    result: Result<Output, Failure>,
    json: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> u8 {
    match result {
        Ok(output) => {
            if json {
                let _ = writeln!(out, "{}", output.json);
            } else {
                let _ = write!(out, "{}", output.human);
            }
            EXIT_OK
        }
        Err(failure) => {
            let _ = writeln!(err, "rolesmith: {}", failure.message);
            if json {
                let mut doc = error_document("failed", &failure.message);
                if !failure.problems.is_empty() {
                    doc["error"]["problems"] =
                        failure.problems.iter().map(|p| p.to_json()).collect();
                }
                let _ = writeln!(out, "{doc}");
            }
            EXIT_FAILED
        }
    }
}

/// `--help` and `--version` are answers (stdout, 0); anything else clap rejects is a usage error
/// (stderr, 2), and with `--json` also one JSON document on stdout.
fn usage(e: &clap::Error, json: bool, out: &mut impl Write, err: &mut impl Write) -> u8 {
    let text = e.render().to_string();
    if !e.use_stderr() {
        let _ = write!(out, "{text}");
        return EXIT_OK;
    }
    let _ = write!(err, "{text}");
    if json {
        let _ = writeln!(out, "{}", error_document("usage", text.trim_end()));
    }
    EXIT_USAGE
}

/// The one JSON document a failed `--json` run prints.
fn error_document(kind: &str, message: &str) -> Value {
    json!({ "error": { "kind": kind, "message": message } })
}

/// Whether a command line that did not parse asked for `--json` (before any `--`).
fn wants_json(args: &[OsString]) -> bool {
    args.iter()
        .skip(1)
        .take_while(|a| *a != "--")
        .any(|a| a == "--json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parser_definition_is_consistent() {
        command().debug_assert();
    }

    #[test]
    fn a_failure_exits_1_with_the_reason_on_stderr_and_json_on_stdout() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let failure = Failure::new("no such role");
        assert_eq!(report(Err(failure), true, &mut out, &mut err), EXIT_FAILED);
        assert_eq!(String::from_utf8(err).unwrap(), "rolesmith: no such role\n");
        let doc: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(doc["error"]["kind"], "failed");
        assert_eq!(doc["error"]["message"], "no such role");
    }

    #[test]
    fn a_failure_without_json_prints_nothing_on_stdout() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let failure = Failure::new("x");
        assert_eq!(report(Err(failure), false, &mut out, &mut err), EXIT_FAILED);
        assert!(out.is_empty());
    }

    #[test]
    fn json_after_a_double_dash_is_not_the_flag() {
        let args: Vec<OsString> = ["rolesmith", "env", "r", "--", "x", "--json"]
            .iter()
            .map(OsString::from)
            .collect();
        assert!(!wants_json(&args));
    }
}
