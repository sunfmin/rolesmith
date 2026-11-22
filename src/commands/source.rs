//! `rolesmith source add|list|update|remove`: the libraries this machine knows (SPEC.md §2, §8).

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::sources::{self, Entry, MAIN, Sources};

/// Add, list, update and remove the libraries this machine knows.
#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub action: Action,
}

#[derive(clap::Subcommand)]
pub enum Action {
    /// Clone a library and record it; refuse a name in use or a repo without library.toml.
    Add {
        /// The local name of the source (lowercase letters, digits and single hyphens).
        name: String,
        /// The library's git URL.
        url: String,
    },
    /// Each source: name, URL, local main commit, roles count.
    List,
    /// Fetch and fast-forward each source's clone (or only this one); never force.
    Update {
        /// Only this source.
        name: Option<String>,
    },
    /// Forget a source and delete its clone.
    Remove {
        /// The source to remove.
        name: String,
    },
}

impl Command for Args {
    const NAME: &'static str = "source";

    fn output_shape() -> Value {
        let source = json!({
            "type": "object",
            "required": ["name", "url", "main", "roles"],
            "properties": {
                "name": { "type": "string" },
                "url": { "type": "string" },
                "main": { "type": ["string", "null"], "description": "full SHA of the clone's local main" },
                "roles": { "type": "integer", "minimum": 0, "description": "roles on main" }
            }
        });
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "by subcommand: add prints the source added; list its sources; update each source moved; remove the source removed",
            "oneOf": [
                { "title": "add", "type": "object", "required": ["added"], "properties": { "added": source.clone() } },
                { "title": "list", "type": "object", "required": ["sources"], "properties": { "sources": { "type": "array", "items": source } } },
                { "title": "update", "type": "object", "required": ["updated"], "properties": { "updated": { "type": "array", "items": {
                    "type": "object",
                    "required": ["name", "from", "to"],
                    "properties": {
                        "name": { "type": "string" },
                        "from": { "type": ["string", "null"] },
                        "to": { "type": "string" }
                    }
                } } } },
                { "title": "remove", "type": "object", "required": ["removed"], "properties": { "removed": { "type": "string" } } }
            ]
        })
    }

    fn run(self) -> Result<Output, Failure> {
        match self.action {
            Action::Add { name, url } => add(&name, &url),
            Action::List => list(),
            Action::Update { name } => update(name.as_deref()),
            Action::Remove { name } => remove(&name),
        }
    }
}

/// A source as `list` and `add` print it.
fn describe(name: &str, entry: &Entry) -> Result<Value, Failure> {
    let clone = sources::clone_dir(name)?;
    let main = sources::main_commit(&clone);
    let roles = main
        .as_deref()
        .map_or(0, |sha| sources::roles_at(&clone, sha).len());
    Ok(json!({ "name": name, "url": entry.url, "main": main, "roles": roles }))
}

fn line(s: &Value) -> String {
    format!(
        "{}\t{}\t{}\t{} role(s)\n",
        s["name"].as_str().unwrap_or_default(),
        s["url"].as_str().unwrap_or_default(),
        s["main"].as_str().unwrap_or("(no main)"),
        s["roles"]
    )
}

fn add(name: &str, url: &str) -> Result<Output, Failure> {
    let mut record = Sources::load()?;
    sources::add(&mut record, name, url)?;
    let added = describe(name, &record.source[name])?;
    Ok(Output {
        human: format!("added {}", line(&added)),
        json: json!({ "added": added }),
    })
}

fn list() -> Result<Output, Failure> {
    let record = Sources::load()?;
    let listed = record
        .source
        .iter()
        .map(|(name, entry)| describe(name, entry))
        .collect::<Result<Vec<_>, _>>()?;
    let human = if listed.is_empty() {
        "no sources; add one with `rolesmith source add <name> <git-url>`\n".to_owned()
    } else {
        listed.iter().map(line).collect()
    };
    Ok(Output {
        json: json!({ "sources": listed }),
        human,
    })
}

fn update(only: Option<&str>) -> Result<Output, Failure> {
    let record = Sources::load()?;
    let names: Vec<&str> = match only {
        Some(name) => {
            record.get(name)?;
            vec![name]
        }
        None => record.source.keys().map(String::as_str).collect(),
    };

    let mut updated = Vec::new();
    let mut failed = Vec::new();
    for name in names {
        match update_one(name) {
            Ok(moved) => updated.push(moved),
            Err(e) => failed.push(format!("{name}: {}", e.message)),
        }
    }
    if !failed.is_empty() {
        return Err(Failure::new(format!(
            "{} source(s) not updated:\n  {}",
            failed.len(),
            failed.join("\n  ")
        )));
    }
    let human: String = updated
        .iter()
        .map(|u: &Value| {
            let (from, to) = (u["from"].as_str().unwrap_or("(none)"), &u["to"]);
            if u["from"] == u["to"] {
                format!("{}: up to date at {from}\n", u["name"].as_str().unwrap())
            } else {
                format!(
                    "{}: {from} -> {}\n",
                    u["name"].as_str().unwrap(),
                    to.as_str().unwrap()
                )
            }
        })
        .collect();
    Ok(Output {
        json: json!({ "updated": updated }),
        human,
    })
}

/// Fetch the source's origin and fast-forward its local `main` to it; a `main` that has moved
/// apart from origin's is refused, never forced.
fn update_one(name: &str) -> Result<Value, Failure> {
    let clone = sources::clone_dir(name)?;
    if !clone.join(".git").exists() {
        return Err(Failure::new(format!(
            "its clone {} is missing; remove the source and add it again",
            clone.display()
        )));
    }
    let from = sources::main_commit(&clone);
    let dir = Some(clone.as_path());
    let on_main = sources::git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .is_ok_and(|branch| branch == MAIN);
    if on_main {
        sources::git(dir, &["fetch", "--quiet", "origin"])?;
        sources::git(
            dir,
            &[
                "merge",
                "--ff-only",
                "--quiet",
                &format!("refs/remotes/origin/{MAIN}"),
            ],
        )?;
    } else {
        // Off main (a branch being distilled), git refuses a non-fast-forward without `+`.
        sources::git(
            dir,
            &[
                "fetch",
                "--quiet",
                "origin",
                &format!("refs/heads/{MAIN}:refs/heads/{MAIN}"),
            ],
        )?;
    }
    let to = sources::main_commit(&clone)
        .ok_or_else(|| Failure::new(format!("its clone has no {MAIN} after fetching")))?;
    Ok(json!({ "name": name, "from": from, "to": to }))
}

fn remove(name: &str) -> Result<Output, Failure> {
    let mut record = Sources::load()?;
    record.get(name)?;
    let clone = sources::clone_dir(name)?;
    if clone.exists() {
        std::fs::remove_dir_all(&clone)
            .map_err(|e| Failure::new(format!("cannot delete {}: {e}", clone.display())))?;
    }
    record.source.remove(name);
    record.save()?;
    Ok(Output {
        json: json!({ "removed": name }),
        human: format!("removed {name} and deleted {}\n", clone.display()),
    })
}
