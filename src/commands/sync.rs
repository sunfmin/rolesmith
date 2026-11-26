//! `rolesmith sync [--project <dir> | --global]`: make the generated files equal what the record
//! (`rolesmith.toml` or `global.toml`) says, adding a missing source from its `url`.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::project::{self, Place, Record, Synced};

/// Make the generated files equal what the record (`rolesmith.toml`, or `global.toml` with `--global`) says.
///
/// Run it after cloning a project, after editing `rolesmith.toml` by hand, or when `rolesmith status` shows drift. It writes only the files that differ and deletes only files the manifest lists that the record no longer generates. A source the record names that this machine lacks is added from the record's `url`. It writes the managed section of each target's entry file again when it differs, and takes it out of an entry file no target names. A record with no roles is an answer that writes nothing.
///
/// ## Refusals
///
/// - a `--project` that is not a directory, or a record that does not parse or names an unknown target;
/// - a file in an install directory that the manifest does not list;
/// - an entry file whose managed section has its first line but no last line after it.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    #[command(flatten)]
    pub place: Where,
}

/// Where `use`, `sync`, `status`, `update` and `remove` work: a project, or the user's global
/// installs.
#[derive(clap::Args)]
pub struct Where {
    /// The project's root (default: the current directory).
    #[arg(long, value_name = "DIR", conflicts_with = "global")]
    pub project: Option<PathBuf>,

    /// The user's own installs, recorded in global.toml; nothing is written into a project.
    #[arg(long)]
    pub global: bool,
}

impl Where {
    /// The place these flags name.
    pub fn place(self) -> Result<Place, Failure> {
        if self.global {
            Place::global()
        } else {
            project_dir(self.project).map(Place::project)
        }
    }
}

impl Command for Args {
    const NAME: &'static str = "sync";

    fn output_shape() -> Value {
        shape()
    }

    fn run(self) -> Result<Output, Failure> {
        let place = self.place.place()?;
        let record = Record::load(&place)?;
        let synced = project::sync(&place, &record)?;
        Ok(output(&place, &record, &synced))
    }
}

/// What `use` and `sync` print: the project, its roles, and what changed.
pub fn shape() -> Value {
    let strings = json!({ "type": "array", "items": { "type": "string" } });
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["project", "roles", "required", "written", "removed"],
        "properties": {
            "project": { "type": ["string", "null"], "description": "the project's root; null for --global" },
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
            "required": { "type": "array",
                          "description": "roles installed only because recorded roles require them, at their rev",
                          "items": {
                "type": "object", "required": ["name", "source", "rev", "by"],
                "properties": {
                    "name": { "type": "string" },
                    "source": { "type": "string" },
                    "rev": { "type": "string", "description": "full SHA" },
                    "by": { "type": "array", "items": { "type": "string" },
                            "description": "the recorded roles that require it" }
                }
            } },
            "written": { "type": "array", "items": { "type": "string" },
                         "description": "files written because they differed, relative to the project (to HOME for --global)" },
            "removed": { "type": "array", "items": { "type": "string" },
                         "description": "generated files deleted, relative to the project (to HOME for --global)" }
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

/// Save `record` at `place`, sync it, and say what changed (`use`, `update`, `remove`).
pub fn install(place: &Place, record: &Record) -> Result<Output, Failure> {
    let synced = project::install(place, record)?;
    Ok(output(place, record, &synced))
}

/// The output of `use` and `sync`.
pub fn output(place: &Place, record: &Record, synced: &Synced) -> Output {
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
    for r in &synced.required {
        human.push_str(&format!(
            "{}/{} at {}, required by {}\n",
            r.source,
            r.name,
            r.rev,
            r.by.join(", ")
        ));
    }
    if record.role.is_empty() {
        human.push_str(&format!("no roles in {}\n", place.record_name()));
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
    Output {
        json: json!({
            "project": place.project_name(),
            "roles": record.role,
            "required": synced.required,
            "written": synced.written,
            "removed": synced.removed,
        }),
        human,
    }
}
