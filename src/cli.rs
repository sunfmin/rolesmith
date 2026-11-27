//! The frame every command runs in: parsing, `--help` from the manual, `--json`,
//! stderr, exit codes.

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use serde_json::{Value, json};

use crate::commands::{Failure, Output, Registered, groups};

/// Success.
pub const EXIT_OK: u8 = 0;
/// The command refused, or something it needed failed.
pub const EXIT_FAILED: u8 = 1;
/// The command line was not one rolesmith accepts.
pub const EXIT_USAGE: u8 = 2;

/// Roles for any coding agent: keeps them in git libraries, installs them into any agent, and opens the lessons people distill into them.
///
/// A role is what an agent of one kind of work must know: a core always in force, prohibitions, and skills loaded when a task needs them. rolesmith installs a role as skills in the directory an agent reads (`--into`), records it at a commit, and checks what an agent writes into a library. It never calls a model and never writes a secret.
///
/// It needs `git` on `PATH`; `gh` for lessons and pull requests, `optokens` for credentials, `brew` for installing tools.
///
/// Run `rolesmith <command> --help` before a command's first run. To start on a machine: `rolesmith source add <name> <git-url>`, e.g. `rolesmith source add felix https://github.com/sunfmin/lore`, then `rolesmith list`. To give a project's agents roles: `rolesmith suggest`, then `rolesmith use <role>... --into <target>`. To work under a role: `rolesmith doctor <role>`, then `rolesmith env <role>`.
///
/// ## Rules to know first
///
/// - Every command runs without asking anything. With `--json` it prints one JSON object on stdout; without, human text. Diagnostics go to stderr. It exits {const:EXIT_OK} on success, {const:EXIT_FAILED} when it refused or something failed, {const:EXIT_USAGE} on a bad command line (see [how a run ends](help:errors)).
/// - A role is addressed as `<role>` or `<source>/<role>` and read at a commit, never from a working tree (see [sources and revs](help:sources)).
/// - A project's `rolesmith.toml` is the one record of its roles; every other file rolesmith writes there is generated from it, and `rolesmith sync` writes it again. Never edit a generated file.
/// - Changes to a library land only as a pull request a person merges (`rolesmith propose`).
#[derive(Parser)]
#[command(
    name = "rolesmith",
    version = env!("ROLESMITH_VERSION"),
    arg_required_else_help = true,
    disable_help_subcommand = true,
    verbatim_doc_comment
)]
pub struct Cli {
    /// Print one JSON object on stdout instead of human text.
    #[arg(long, global = true, verbatim_doc_comment)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Top,
}

/// The commands: every registered one, and `help`.
#[derive(Subcommand)]
pub enum Top {
    /// Show one topic of this manual, or all of it.
    ///
    /// `rolesmith help <topic>` shows a topic, `rolesmith help all` the whole manual, every command and topic in full; without a topic it shows what `rolesmith --help` does. It answers Markdown on stdout, even with `--json`. An unknown topic is a usage error naming the topics.
    #[command(next_help_heading = groups::LEARNING, verbatim_doc_comment)]
    Help {
        /// The topic, such as `composing`, or `all`.
        #[arg(verbatim_doc_comment)]
        topic: Option<String>,
    },
    #[command(flatten)]
    Run(Registered),
}

/// The kind of a refusal, `error.kind` with `--json`.
#[derive(agent_help::Help, Clone, Copy)]
pub enum ErrorKind {
    /// The command refused, or something it needed failed; exit 1.
    Failed,
    /// The command line is not one rolesmith accepts; exit 2.
    Usage,
}

impl ErrorKind {
    /// Its name, as `error.kind` carries it.
    pub fn name(self) -> &'static str {
        agent_help::Table::entry(&self).name
    }
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
    let cli = match parse(args) {
        Ok(cli) => cli,
        Err(e) => return usage(&e, wants_json(args), out, err),
    };
    match cli.command {
        Top::Run(command) => report(command.run(), cli.json, out, err),
        Top::Help { topic } => match help_answer(topic.as_deref()) {
            Ok(human) => report(
                Ok(Output {
                    json: Value::Null,
                    human,
                }),
                false,
                out,
                err,
            ),
            Err(e) => usage(&e, cli.json, out, err),
        },
    }
}

/// Parse without the manual when the line needs none; `--help`, `-h`, `help [<topic>]` and a
/// line clap refuses go through the manual, so every help answer and usage error is its text.
fn parse(args: &[OsString]) -> Result<Cli, clap::Error> {
    match Cli::try_parse_from(args) {
        Ok(cli) if matches!(cli.command, Top::Run(_)) => Ok(cli),
        _ => {
            let command = crate::help::manual()
                .clap()
                .expect("checked by the_manual_is_whole");
            Cli::from_arg_matches(&command.try_get_matches_from(args)?)
        }
    }
}

/// What `help [<topic>]` answers: the index, a topic, or `all`; an unknown topic is a usage error.
fn help_answer(topic: Option<&str>) -> Result<String, clap::Error> {
    let manual = crate::help::manual();
    match topic {
        None => Ok(manual.index().expect("checked by the_manual_is_whole")),
        Some(topic) => manual
            .topic_help(topic)
            .map_err(|e| clap::Error::raw(clap::error::ErrorKind::InvalidValue, format!("{e}\n"))),
    }
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
                let mut doc = error_document(ErrorKind::Failed, &failure.message);
                if let Some((key, items)) = failure.list {
                    doc["error"][key] = items;
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
        let _ = writeln!(out, "{}", error_document(ErrorKind::Usage, text.trim_end()));
    }
    EXIT_USAGE
}

/// The one JSON document a failed `--json` run prints.
fn error_document(kind: ErrorKind, message: &str) -> Value {
    json!({ "error": { "kind": kind.name(), "message": message } })
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
