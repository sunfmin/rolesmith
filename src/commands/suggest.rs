//! `rolesmith suggest [--dir <dir>]`: the roles of every source whose `[match]` the project meets,
//! with the rule that matched (SPEC.md §5.5).

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Command, Failure, Output, sync};
use crate::suggestions::{self, Suggestion};

/// Roles of every source whose [match] the project meets, with the rule that matched.
#[derive(clap::Args)]
pub struct Args {
    /// The project's root (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
}

impl Command for Args {
    const NAME: &'static str = "suggest";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["project", "roles"],
            "properties": {
                "project": { "type": "string", "description": "the project's root" },
                "roles": { "type": "array", "items": {
                    "type": "object",
                    "required": ["address", "source", "role", "rule", "path"],
                    "properties": {
                        "address": { "type": "string", "description": "<source>/<role>" },
                        "source": { "type": "string" },
                        "role": { "type": "string" },
                        "rule": { "type": "string", "description": "the [match] rule that matched, as role.toml writes it" },
                        "path": { "type": "string", "description": "the project path that met it, relative to the root" }
                    }
                } }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let dir = sync::project_dir(self.dir)?;
        let found = suggestions::suggest(&dir)?;
        let mut human: String = found
            .iter()
            .map(|s| format!("{}\t{} ({})\n", s.qualified(), s.rule, s.path))
            .collect();
        if found.is_empty() {
            human.push_str("no role matches this project\n");
        }
        Ok(Output {
            json: json!({
                "project": dir.display().to_string(),
                "roles": found.iter().map(to_json).collect::<Vec<_>>(),
            }),
            human,
        })
    }
}

fn to_json(s: &Suggestion) -> Value {
    json!({
        "address": s.qualified(), "source": s.source, "role": s.role, "rule": s.rule, "path": s.path,
    })
}
