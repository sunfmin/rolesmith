//! `rolesmith tool show <tool>`: a tool's tool.toml on its source's local `main`, and whether its
//! check passes on this machine (SPEC.md §3.5, §8).

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::library::ToolToml;
use crate::machine;
use crate::sources::{self, Kind, Sources};

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
}

impl Command for Args {
    const NAME: &'static str = "tool";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "by subcommand: show prints the tool and its check here",
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
                  } }
            ]
        })
    }

    fn run(self) -> Result<Output, Failure> {
        match self.action {
            Action::Show { tool } => show(&tool),
        }
    }
}

fn show(address: &str) -> Result<Output, Failure> {
    let record = Sources::load()?;
    let found = sources::resolve_kind(&record, Kind::Tool, address, None)?;
    let file = format!("tools/{}/tool.toml", found.name);
    let clone = sources::clone_dir(&found.source)?;
    let text = sources::file_at(&clone, &found.rev, &file).ok_or_else(|| {
        Failure::new(format!(
            "{} at {} has no {file}",
            found.qualified(),
            found.rev
        ))
    })?;
    let toml: ToolToml = toml::from_str(&text).map_err(|e| {
        Failure::new(format!(
            "{file} of {} at {} does not parse ({}); `rolesmith check` in the library says more",
            found.source,
            found.rev,
            e.message().trim()
        ))
    })?;
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
