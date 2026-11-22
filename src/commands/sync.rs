//! `rolesmith sync [--project <dir>]`: make a project's generated files equal what its
//! `rolesmith.toml` says, adding a missing source from its `url` (SPEC.md §4, §8).

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::project::{self, Record, Synced};

/// Make the generated files equal what rolesmith.toml says; add missing sources from their url.
#[derive(clap::Args)]
pub struct Args {
    /// The project's root (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,
}

impl Command for Args {
    const NAME: &'static str = "sync";

    fn output_shape() -> Value {
        shape()
    }

    fn run(self) -> Result<Output, Failure> {
        let dir = project_dir(self.project)?;
        let synced = project::sync(&dir)?;
        output(&dir, &Record::load(&dir)?, &synced)
    }
}

/// What `use` and `sync` print: the project, its roles, and what changed.
pub fn shape() -> Value {
    let strings = json!({ "type": "array", "items": { "type": "string" } });
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["project", "roles", "written", "removed"],
        "properties": {
            "project": { "type": "string", "description": "the project's root" },
            "roles": { "type": "array", "items": {
                "type": "object", "required": ["name", "source", "url", "rev", "into"],
                "properties": {
                    "name": { "type": "string" },
                    "source": { "type": "string" },
                    "url": { "type": "string" },
                    "rev": { "type": "string", "description": "full SHA" },
                    "into": strings.clone()
                }
            } },
            "written": { "type": "array", "items": { "type": "string" },
                         "description": "files written because they differed, relative to the project" },
            "removed": { "type": "array", "items": { "type": "string" },
                         "description": "generated files deleted, relative to the project" }
        }
    })
}

/// The project root `--project` names, or the current directory.
pub fn project_dir(project: Option<PathBuf>) -> Result<PathBuf, Failure> {
    let dir = match project {
        Some(dir) => dir,
        None => std::env::current_dir()
            .map_err(|e| Failure::new(format!("cannot read the current directory: {e}")))?,
    };
    if !dir.is_dir() {
        return Err(Failure::new(format!(
            "the project {} is not a directory",
            dir.display()
        )));
    }
    Ok(dir)
}

/// The output of `use` and `sync`.
pub fn output(dir: &std::path::Path, record: &Record, synced: &Synced) -> Result<Output, Failure> {
    let mut human = String::new();
    for r in &record.role {
        human.push_str(&format!(
            "{}/{} at {} into {}\n",
            r.source,
            r.name,
            r.rev,
            r.into.join(", ")
        ));
    }
    if record.role.is_empty() {
        human.push_str("no roles in rolesmith.toml\n");
    }
    for rel in &synced.written {
        human.push_str(&format!("wrote {rel}\n"));
    }
    for rel in &synced.removed {
        human.push_str(&format!("removed {rel}\n"));
    }
    if synced.written.is_empty() && synced.removed.is_empty() {
        human.push_str("up to date\n");
    }
    Ok(Output {
        json: json!({
            "project": dir.display().to_string(),
            "roles": record.role,
            "written": synced.written,
            "removed": synced.removed,
        }),
        human,
    })
}
