//! `rolesmith status [--project <dir>]`: per role of a project, its pinned rev, its source's
//! `main`, and whether its generated files are what the record generates (SPEC.md §5.4).

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Command, Failure, Output, sync};
use crate::project::{self, Record, RoleEntry};
use crate::sources;

/// Per role: the pinned rev, the source's main, and whether the generated files match.
#[derive(clap::Args)]
pub struct Args {
    /// The project's root (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,
}

impl Command for Args {
    const NAME: &'static str = "status";

    fn output_shape() -> Value {
        let count = json!({ "type": ["integer", "null"], "minimum": 0 });
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["project", "roles"],
            "properties": {
                "project": { "type": "string", "description": "the project's root" },
                "roles": { "type": "array", "items": {
                    "type": "object",
                    "required": ["name", "source", "rev", "main", "behind", "ahead", "drift"],
                    "properties": {
                        "name": { "type": "string" },
                        "source": { "type": "string" },
                        "rev": { "type": "string", "description": "the pinned full SHA" },
                        "main": { "type": ["string", "null"], "description": "full SHA of the source clone's local main" },
                        "behind": count.clone(),
                        "ahead": count,
                        "drift": { "type": "array", "items": { "type": "string" },
                                   "description": "generated files that differ from what the record generates, or are missing" }
                    }
                } }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let dir = sync::project_dir(self.project)?;
        let record = Record::load(&dir)?;
        let roles = record
            .role
            .iter()
            .map(|entry| role_status(&dir, entry))
            .collect::<Result<Vec<_>, _>>()?;
        let human = if roles.is_empty() {
            "no roles in rolesmith.toml\n".to_owned()
        } else {
            roles.iter().map(line).collect()
        };
        Ok(Output {
            json: json!({ "project": dir.display().to_string(), "roles": roles }),
            human,
        })
    }
}

fn role_status(dir: &std::path::Path, entry: &RoleEntry) -> Result<Value, Failure> {
    // Reading what the record generates adds a missing source and fetches a missing rev, as sync.
    let drift = project::drift(dir, entry)?;
    let clone = sources::clone_dir(&entry.source)?;
    let main = sources::main_commit(&clone);
    let (ahead, behind) = match &main {
        Some(main) => {
            let counts = sources::git(
                Some(&clone),
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("{}...{main}", entry.rev),
                ],
            )?;
            let mut n = counts.split_whitespace().map(|c| c.parse::<u64>().ok());
            (n.next().flatten(), n.next().flatten())
        }
        None => (None, None),
    };
    Ok(json!({
        "name": entry.name,
        "source": entry.source,
        "rev": entry.rev,
        "main": main,
        "behind": behind,
        "ahead": ahead,
        "drift": drift,
    }))
}

fn line(r: &Value) -> String {
    let main = match (
        r["main"].as_str(),
        r["behind"].as_u64(),
        r["ahead"].as_u64(),
    ) {
        (None, ..) => "its source has no main".to_owned(),
        (Some(_), Some(0), Some(0)) => "at main".to_owned(),
        (Some(main), behind, ahead) => format!(
            "main {main}: {} behind, {} ahead",
            behind.unwrap_or_default(),
            ahead.unwrap_or_default()
        ),
    };
    let drift: Vec<&str> = r["drift"]
        .as_array()
        .map(|d| d.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let files = if drift.is_empty() {
        "files match".to_owned()
    } else {
        format!("drift (run `rolesmith sync`): {}", drift.join(", "))
    };
    format!(
        "{}/{} at {}; {main}; {files}\n",
        r["source"].as_str().unwrap_or_default(),
        r["name"].as_str().unwrap_or_default(),
        r["rev"].as_str().unwrap_or_default(),
    )
}
