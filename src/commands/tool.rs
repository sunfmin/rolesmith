//! `rolesmith tool show <tool>`: a tool's tool.toml on its source's local `main`, and whether its
//! check passes on this machine. `rolesmith tool install <role> | --tool <tool>`: each program
//! whose check fails, installed by its pinned method into the user's own prefix (SPEC.md §3.5, §8).

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::install::{self, Done};
use crate::library::ToolToml;
use crate::machine;
use crate::sources::{self, Found, Kind, Sources};

/// The tools of a library, as this machine has them.
#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub action: Action,
}

#[derive(clap::Subcommand)]
pub enum Action {
    /// The tool.toml, and whether its check passes here.
    Show {
        /// The tool: <tool> when one source has it, <source>/<tool> otherwise.
        tool: String,
    },
    /// Install each program of a role (or one tool) whose check fails, by its pinned method.
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
                        "type": "object", "required": ["name", "formula"],
                        "properties": {
                            "name": { "type": "string" },
                            "formula": { "type": ["string", "null"],
                                         "description": "the formula installed; null when its check already passed" }
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
    let file = format!("tools/{}/tool.toml", found.name);
    let clone = sources::clone_dir(&found.source)?;
    let text = sources::file_at(&clone, &found.rev, &file).ok_or_else(|| {
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
    let check = machine::check(&toml.check);
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
                let installed: Vec<&str> =
                    done.iter().filter_map(|d| d.formula.as_deref()).collect();
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
        match &d.formula {
            Some(formula) => human.push_str(&format!("- {}: installed {formula}\n", d.name)),
            None => human.push_str(&format!("- {}: its check passes, left alone\n", d.name)),
        }
    }
    Ok(Output {
        json: json!({
            "source": found.source,
            "rev": found.rev,
            "tools": done.iter().map(|d| json!({ "name": d.name, "formula": d.formula })).collect::<Vec<_>>(),
        }),
        human,
    })
}
