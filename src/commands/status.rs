//! `rolesmith status [--project <dir> | --global]`: per role of a project (or of the global
//! installs), its pinned rev, its source's `main`, and whether its generated files are what the
//! record generates (SPEC.md §5.4).

use serde_json::{Value, json};

use super::{Command, Failure, Output, sync};
use crate::project::{self, Place, Record, RoleEntry};
use crate::sources;

/// Per role: the pinned rev, the source's main, and whether the generated files match.
#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    pub place: sync::Where,
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
                "project": { "type": ["string", "null"], "description": "the project's root; null for --global" },
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
        let place = self.place.place()?;
        let record = Record::load(&place)?;
        let (roles, lines): (Vec<Value>, Vec<String>) = record
            .role
            .iter()
            .map(|entry| role_status(&place, entry))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();
        let human = if lines.is_empty() {
            format!("no roles in {}\n", place.record_name())
        } else {
            lines.concat()
        };
        Ok(Output {
            json: json!({ "project": sync::project_json(&place), "roles": roles }),
            human,
        })
    }
}

/// A role's status as JSON, and its line of human text.
fn role_status(place: &Place, entry: &RoleEntry) -> Result<(Value, String), Failure> {
    // Reading what the record generates adds a missing source and fetches a missing rev, as sync.
    let drift = project::drift(place, entry)?;
    let clone = sources::clone_dir(&entry.source)?;
    let main = sources::main_commit(&clone);
    let (mut ahead, mut behind) = (None, None);
    let at = match &main {
        None => "its source has no main".to_owned(),
        Some(main) => {
            let range = format!("{}...{main}", entry.rev);
            let counts = sources::git(
                Some(&clone),
                &["rev-list", "--left-right", "--count", &range],
            )?;
            let mut n = counts.split_whitespace().map(|c| c.parse::<u64>().ok());
            (ahead, behind) = (n.next().flatten(), n.next().flatten());
            match (behind, ahead) {
                (Some(0), Some(0)) => "at main".to_owned(),
                _ => format!(
                    "main {main}: {} behind, {} ahead",
                    behind.unwrap_or(0),
                    ahead.unwrap_or(0)
                ),
            }
        }
    };
    let files = if drift.is_empty() {
        "files match".to_owned()
    } else {
        format!("drift (run `rolesmith sync`): {}", drift.join(", "))
    };
    let line = format!(
        "{}/{} at {}; {at}; {files}\n",
        entry.source, entry.name, entry.rev
    );
    let json = json!({
        "name": entry.name,
        "source": entry.source,
        "rev": entry.rev,
        "main": main,
        "behind": behind,
        "ahead": ahead,
        "drift": drift,
    });
    Ok((json, line))
}
