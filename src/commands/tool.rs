//! `rolesmith tool show <tool>`: a tool's tool.toml on its source's local `main`, and whether its
//! check passes on this machine. `rolesmith tool install <role> | --tool <tool>`: each program
//! whose check fails, installed by its pinned method into the user's own prefix, on this machine or
//! on `--host` over ssh (SPEC.md §3.5, §8).

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::host::Host;
use crate::install::{self, Done, Installed};
use crate::library::ToolToml;
use crate::machine;
use crate::needs;
use crate::sources::{self, Found, Kind, Sources};

/// Show and install the tools of a library, as this machine has them.
///
/// A tool is a program a role runs, described by its library's `tools/<tool>/tool.toml`: how to check it, how to install it at a pinned version (by `brew` or by a `script`), and the credential it needs. A tool is addressed as a role is.
///
/// Each subcommand's `rolesmith tool <subcommand> --help` says when to run it and what it answers.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    #[command(subcommand)]
    pub action: Action,
}

#[derive(clap::Subcommand)]
pub enum Action {
    /// Show a tool's `tool.toml` on its source's local `main`, and whether its check passes here.
    ///
    /// It runs the tool's `check` with `sh -c` and writes nothing. It refuses an unknown tool and a `tool.toml` that does not parse.
    #[command(verbatim_doc_comment)]
    Show {
        /// The tool: <tool> when one source has it, <source>/<tool> otherwise.
        tool: String,
    },
    /// Install each program of a role (or one tool) whose check fails, by its pinned method.
    ///
    /// Run it when `rolesmith doctor` names a program missing. A tool whose check passes is left alone; a `script` tool's check passes only when its stdout shows the pinned `version`. Everything runs as you, never with `sudo`. A `brew` tool is installed into brew's own prefix: `<formula>@<version>` where brew has it, else `<formula>` when brew's stable version is the pinned one. A `brew` tool with `version = "stable"` follows its formula's stable version instead: it passes only when brew has that version installed (its tap, `<user>/<repo>` of `<user>/<repo>/<formula>`, refreshed first with a `git pull` at most once a day, never `brew update`), and is otherwise installed (`brew install <formula>`) or, when an older one is installed, upgraded (`brew upgrade <formula>`). While another brew holds the formula's lock it waits and tries again. A `script` tool's script is run with `sh -c`. After installing, the tool's check must pass. Tools are installed in order; the first refusal stops it and names what was installed before it.
    ///
    /// ## Refusals
    ///
    /// - a tool without a pinned version, with no method or with both `brew` and `script`;
    /// - a formula whose stable version is not the pinned one (before installing anything);
    /// - a `script` tool with `version = "stable"` (only `brew` follows a stable version);
    /// - a script that exits non-zero, with its exit code and the end of its stderr;
    /// - a check that still fails, or does not show the pinned version, after installing;
    /// - with `--host`, a host ssh cannot reach (naming it) or one without `brew` for a `brew` tool.
    ///
    /// ## Another host
    ///
    /// With `--host <host>` every check, every `brew` command and every script runs on that machine, as `ssh -o BatchMode=yes -- <host> sh -c '<command>'`: a `brew` tool goes into the host's own brew and prefix, and the host needs `sh` and ssh access by key, not rolesmith. It runs with the `PATH` that a command run over ssh gets there, so brew must be on it. This is the fix `rolesmith doctor <role> --host <host>` names.
    ///
    /// ## A project's roles
    ///
    /// With `--project <dir>` in place of a role it installs the programs of every role the project's `rolesmith.toml` records, each at its recorded rev, and of every role each requires, at that same rev; a tool two roles name is checked and installed once. Each installed tool adds `from`, the roles that name it, and the answer names the `project` and the `roles` it read. This is the fix `rolesmith doctor --project <dir>` names. A directory without `rolesmith.toml` is refused.
    #[command(verbatim_doc_comment)]
    Install {
        /// The role whose programs to install: <role> or <source>/<role>.
        #[arg(required_unless_present_any = ["tool", "project"], conflicts_with_all = ["tool", "project"])]
        role: Option<String>,
        /// One tool instead of a role's programs: <tool> or <source>/<tool>.
        #[arg(long, conflicts_with = "project")]
        tool: Option<String>,
        /// The programs of every role this project's rolesmith.toml records, at its rev.
        #[arg(long, value_name = "DIR")]
        project: Option<PathBuf>,
        /// Check and install on this host over ssh instead of on this machine.
        #[arg(long)]
        host: Option<String>,
    },
}

impl Command for Args {
    const NAME: &'static str = "tool";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "by subcommand: show prints the tool and its check here; install what it installed",
            "oneOf": [
                { "title": "show", "type": "object",
                  "required": ["source", "tool", "rev", "description", "tool_toml", "check", "passes", "reason"],
                  "properties": {
                    "source": { "type": "string" },
                    "tool": { "type": "string" },
                    "rev": { "type": "string", "description": "the full SHA the tool was read at" },
                    "description": { "type": "string" },
                    "tool_toml": { "type": "string", "description": "the text of tool.toml" },
                    "check": { "type": "string", "description": "the check command" },
                    "passes": { "type": "boolean", "description": "whether the check exits 0 here" },
                    "reason": { "type": "string", "description": "\"passes\", or why it does not" }
                  } },
                { "title": "install", "type": "object",
                  "required": ["host", "tools"],
                  "properties": {
                    "source": { "type": "string", "description": "with a role or --tool" },
                    "rev": { "type": "string", "description": "with a role or --tool: the full SHA the tools were read at" },
                    "project": { "type": "string", "description": "with --project: the project's root" },
                    "roles": { "type": "array", "description": "with --project: each role read, recorded or required, at its rev",
                               "items": {
                        "type": "object", "required": ["source", "role", "rev"],
                        "properties": { "source": { "type": "string" }, "role": { "type": "string" },
                                        "rev": { "type": "string" } }
                    } },
                    "host": { "type": ["string", "null"], "description": "the host it installed on; null for this machine" },
                    "tools": { "type": "array", "items": {
                        "type": "object", "required": ["name", "method", "formula"],
                        "properties": {
                            "name": { "type": "string" },
                            "method": { "enum": ["brew", "script", null],
                                        "description": "how it was installed; null when its check already passed" },
                            "formula": { "type": ["string", "null"],
                                         "description": "the formula brew installed; null otherwise" },
                            "from": { "type": "array", "items": { "type": "string" },
                                      "description": "with --project: the roles that name it, as <source>/<role>" }
                        }
                    } }
                  } }
            ]
        })
    }

    fn run(self) -> Result<Output, Failure> {
        match self.action {
            Action::Show { tool } => show(&tool),
            Action::Install {
                role,
                tool,
                project,
                host,
            } => {
                let host = Host::from_arg(host);
                match project {
                    Some(dir) => install_project(dir, &host),
                    None => install_all(role, tool, &host),
                }
            }
        }
    }
}

/// The tool.toml of `found`, as text and parsed.
fn read_tool(found: &Found) -> Result<(String, ToolToml), Failure> {
    let file = Kind::Tool.toml(&found.name);
    let clone = sources::clone_dir(&found.source)?;
    let text = sources::toml_at(&clone, &found.rev, Kind::Tool, &found.name).ok_or_else(|| {
        Failure::new(format!(
            "{} at {} has no {file}",
            found.qualified(),
            found.rev
        ))
    })?;
    let toml = toml::from_str(&text).map_err(|e| {
        Failure::new(format!(
            "{file} of {} at {} does not parse ({}); `rolesmith check` in the library says more",
            found.source,
            found.rev,
            e.message().trim()
        ))
    })?;
    Ok((text, toml))
}

fn show(address: &str) -> Result<Output, Failure> {
    let record = Sources::load()?;
    let found = sources::resolve_kind(&record, Kind::Tool, address, None)?;
    let (text, toml) = read_tool(&found)?;
    let check = machine::check_tool(&Host::Here, &toml)?;
    let human = format!(
        "# {} ({} at {})\n\n{}\n\n## tool.toml\n\n{}\n\n## Here\n\n`{}` {}\n",
        found.name,
        found.source,
        found.rev,
        toml.description,
        text.trim_end(),
        toml.check,
        check.reason
    );
    Ok(Output {
        json: json!({
            "source": found.source,
            "tool": found.name,
            "rev": found.rev,
            "description": toml.description,
            "tool_toml": text,
            "check": toml.check,
            "passes": check.passes,
            "reason": check.reason,
        }),
        human,
    })
}

/// Install a role's programs, or the one tool asked for, in order; the first refusal stops it and
/// says what was installed before it.
fn install_all(role: Option<String>, tool: Option<String>, host: &Host) -> Result<Output, Failure> {
    let record = Sources::load()?;
    let (found, tools) = match (role, tool) {
        (Some(role), _) => {
            let found = sources::resolve(&record, &role, None)?;
            let (lib, _tree) = sources::read_role(&found)?;
            let tools: Vec<(String, ToolToml)> =
                lib.tools.into_iter().map(|t| (t.name, t.toml)).collect();
            (found, tools)
        }
        (None, Some(tool)) => {
            let found = sources::resolve_kind(&record, Kind::Tool, &tool, None)?;
            let (_, toml) = read_tool(&found)?;
            let name = found.name.clone();
            (found, vec![(name, toml)])
        }
        (None, None) => unreachable!("clap requires a role or --tool"),
    };

    let done = install_each(host, tools.iter().map(|(name, toml)| (name.as_str(), toml)))?;
    let header = format!("{} at {}{}:\n", found.qualified(), found.rev, host.on());
    Ok(Output {
        json: json!({
            "source": found.source,
            "rev": found.rev,
            "host": host.name(),
            "tools": done.iter().map(tool_json).collect::<Vec<_>>(),
        }),
        human: header + &lines(&done),
    })
}

/// Install the programs of every role the project at `dir` records, and of the roles they
/// require, each tool once.
fn install_project(dir: PathBuf, host: &Host) -> Result<Output, Failure> {
    let (dir, read, needs) = needs::of_project(Some(dir))?;
    let done = install_each(
        host,
        needs.programs.iter().map(|p| (p.name.as_str(), &p.toml)),
    )?;
    let tools: Vec<Value> = done
        .iter()
        .zip(&needs.programs)
        .map(|(d, p)| {
            let mut doc = tool_json(d);
            doc["from"] = json!(p.from);
            doc
        })
        .collect();
    let header = format!("the roles of {}{}:\n", dir.display(), host.on());
    Ok(Output {
        json: json!({
            "project": dir.display().to_string(),
            "roles": read.iter().map(|f| json!({ "source": f.source, "role": f.name, "rev": f.rev })).collect::<Vec<_>>(),
            "host": host.name(),
            "tools": tools,
        }),
        human: header + &lines(&done),
    })
}

/// Install each tool in order; the first refusal stops it and says what was installed before it.
fn install_each<'a>(
    host: &Host,
    tools: impl Iterator<Item = (&'a str, &'a ToolToml)>,
) -> Result<Vec<Done>, Failure> {
    let mut done: Vec<Done> = Vec::new();
    for (name, toml) in tools {
        match install::install(host, name, toml) {
            Ok(d) => done.push(d),
            Err(mut f) => {
                let installed: Vec<String> = done.iter().filter_map(installed_what).collect();
                if !installed.is_empty() {
                    f.message
                        .push_str(&format!("; installed before it: {}", installed.join(", ")));
                }
                return Err(f);
            }
        }
    }
    Ok(done)
}

/// One line per tool: what was installed, or that it was left alone.
fn lines(done: &[Done]) -> String {
    done.iter()
        .map(|d| match installed_what(d) {
            Some(what) => format!("- {}: installed {what}\n", d.name),
            None => format!("- {}: its check passes, left alone\n", d.name),
        })
        .collect()
}

/// A tool as `--json` lists it: how it was installed and the formula brew installed.
fn tool_json(d: &Done) -> Value {
    let (method, formula) = match &d.installed {
        None => (None, None),
        Some(Installed::Brew(formula)) => (Some("brew"), Some(formula)),
        Some(Installed::Script) => (Some("script"), None),
    };
    json!({ "name": d.name, "method": method, "formula": formula })
}

/// What an install did, as a refusal or the human answer names it: the formula brew installed,
/// or the tool by its script; `None` when it was left alone.
fn installed_what(d: &Done) -> Option<String> {
    match d.installed.as_ref()? {
        Installed::Brew(formula) => Some(formula.clone()),
        Installed::Script => Some(format!("{} by its script", d.name)),
    }
}
