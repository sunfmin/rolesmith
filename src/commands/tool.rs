//! `rolesmith tool show <tool>`: a tool's tool.toml on its source's local `main`, and whether its
//! check passes on this machine. `rolesmith tool install <role> | --tool <tool>`: each program
//! whose check fails, installed by its pinned method into the user's own prefix (SPEC.md §3.5, §8).

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::install::{self, Done, Installed};
use crate::library::ToolToml;
use crate::machine;
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
    /// Run it when `rolesmith doctor` names a program missing. A tool whose check passes is left alone; a `script` tool's check passes only when its stdout shows the pinned `version`. Everything runs as you, never with `sudo`. A `brew` tool is installed into brew's own prefix: `<formula>@<version>` where brew has it, else `<formula>` when brew's stable version is the pinned one. A `script` tool's script is run with `sh -c`. After installing, the tool's check must pass. Tools are installed in order; the first refusal stops it and names what was installed before it.
    ///
    /// ## Refusals
    ///
    /// - a tool without a pinned version, with no method or with both `brew` and `script`;
    /// - a formula whose stable version is not the pinned one (before installing anything);
    /// - a script that exits non-zero, with its exit code and the end of its stderr;
    /// - a check that still fails, or does not show the pinned version, after installing.
    #[command(verbatim_doc_comment)]
    Install {
        /// The role whose programs to install: <role> or <source>/<role>.
        #[arg(required_unless_present = "tool", conflicts_with = "tool")]
        role: Option<String>,
        /// One tool instead of a role's programs: <tool> or <source>/<tool>.
        #[arg(long)]
        tool: Option<String>,
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
                  "required": ["source", "rev", "tools"],
                  "properties": {
                    "source": { "type": "string" },
                    "rev": { "type": "string", "description": "the full SHA the tools were read at" },
                    "tools": { "type": "array", "items": {
                        "type": "object", "required": ["name", "method", "formula"],
                        "properties": {
                            "name": { "type": "string" },
                            "method": { "enum": ["brew", "script", null],
                                        "description": "how it was installed; null when its check already passed" },
                            "formula": { "type": ["string", "null"],
                                         "description": "the formula brew installed; null otherwise" }
                        }
                    } }
                  } }
            ]
        })
    }

    fn run(self) -> Result<Output, Failure> {
        match self.action {
            Action::Show { tool } => show(&tool),
            Action::Install { role, tool } => install_all(role, tool),
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
    let check = machine::check_tool(&toml);
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
fn install_all(role: Option<String>, tool: Option<String>) -> Result<Output, Failure> {
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

    let mut done: Vec<Done> = Vec::new();
    for (name, toml) in &tools {
        match install::install(name, toml) {
            Ok(d) => done.push(d),
            Err(mut f) => {
                let installed: Vec<String> = done
                    .iter()
                    .filter(|d| d.installed.is_some())
                    .map(installed_what)
                    .collect();
                if !installed.is_empty() {
                    f.message
                        .push_str(&format!("; installed before it: {}", installed.join(", ")));
                }
                return Err(f);
            }
        }
    }

    let mut human = format!("{} at {}:\n", found.qualified(), found.rev);
    for d in &done {
        match &d.installed {
            Some(_) => human.push_str(&format!("- {}: installed {}\n", d.name, installed_what(d))),
            None => human.push_str(&format!("- {}: its check passes, left alone\n", d.name)),
        }
    }
    Ok(Output {
        json: json!({
            "source": found.source,
            "rev": found.rev,
            "tools": done.iter().map(|d| {
                let (method, formula) = match &d.installed {
                    None => (None, None),
                    Some(Installed::Brew(formula)) => (Some("brew"), Some(formula)),
                    Some(Installed::Script) => (Some("script"), None),
                };
                json!({ "name": d.name, "method": method, "formula": formula })
            }).collect::<Vec<_>>(),
        }),
        human,
    })
}

/// What an install did, as a refusal or the human answer names it: the formula brew installed,
/// or the tool by its script.
fn installed_what(d: &Done) -> String {
    match &d.installed {
        Some(Installed::Brew(formula)) => formula.clone(),
        _ => format!("{} by its script", d.name),
    }
}
