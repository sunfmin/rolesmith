//! `rolesmith source add|list|update|remove`: the libraries this machine knows (SPEC.md §2, §8).

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::project;
use crate::sources::{self, Entry, MAIN, Sources};

/// Add, list, update and remove the libraries this machine knows.
///
/// A source is this machine's name for a library, a git repository of roles (see [sources and revs](help:sources)). Run `source add` once per library on a machine, and `source update` to bring in what a library has merged since.
///
/// Each subcommand's `rolesmith source <subcommand> --help` says when to run it and what it answers.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    #[command(subcommand)]
    pub action: Action,
}

#[derive(clap::Subcommand)]
pub enum Action {
    /// Clone a library and record it as a source.
    ///
    /// Run it once per library on a machine. It clones `<url>` into rolesmith's data directory and records `<name>` in `sources.toml`; the name is the `<source>` of `<source>/<role>`.
    ///
    /// ## Refusals
    ///
    /// - a name that is not lowercase letters, digits and single hyphens, or one already recorded;
    /// - a URL `git clone` cannot clone;
    /// - a repository without `library.toml` on its `main`.
    #[command(verbatim_doc_comment)]
    Add {
        /// The local name of the source (lowercase letters, digits and single hyphens).
        name: String,
        /// The library's git URL.
        url: String,
    },
    /// List each source: its name, URL, local `main` commit and how many roles it has there.
    ///
    /// It reads the clones only; it fetches nothing, so `main` is what the last `source add` or `source update` brought in. No sources is an answer, not a refusal.
    #[command(verbatim_doc_comment)]
    List,
    /// Fetch and fast-forward each source's clone (or only the one named); never force.
    ///
    /// Run it before `rolesmith update`, which moves a project's roles to the local `main` this brings in and fetches nothing itself. It updates every source it can and then refuses, naming each that failed: a missing clone (remove the source and add it again), a fetch that failed, or a local `main` that moved apart from the library's.
    #[command(verbatim_doc_comment)]
    Update {
        /// Only this source.
        name: Option<String>,
    },
    /// Forget a source and delete its clone.
    ///
    /// It refuses while an install still records the source: a project's `rolesmith.toml` (of every project rolesmith has synced into that still exists) or the user's `global.toml`. The refusal lists each, with `--json` as `error.projects`; run `rolesmith remove` of its roles there first.
    #[command(verbatim_doc_comment)]
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

/// A project whose `rolesmith.toml`, or the global installs whose `global.toml` (`project: None`),
/// records roles of a source.
struct User {
    project: Option<String>,
    roles: Vec<String>,
}

impl std::fmt::Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let place = self.project.as_deref().unwrap_or("--global");
        write!(f, "{place}: {}", self.roles.join(", "))
    }
}

/// The projects rolesmith has installed into whose record still names the source `name`, then the
/// global installs when they do; a project that is gone, or no longer records it, is not one.
fn recorded_by(name: &str) -> Result<Vec<User>, Failure> {
    let projects = project::known_projects()?
        .into_iter()
        .filter(|dir| dir.is_dir());
    let mut users = Vec::new();
    for place in projects
        .map(project::Place::project)
        .chain([project::Place::global()?])
    {
        let roles: Vec<String> = project::Record::load(&place)?
            .role
            .into_iter()
            .filter(|r| r.source == name)
            .map(|r| r.name)
            .collect();
        if !roles.is_empty() {
            users.push(User {
                project: place.project_name(),
                roles,
            });
        }
    }
    Ok(users)
}

fn remove(name: &str) -> Result<Output, Failure> {
    let mut record = Sources::load()?;
    record.get(name)?;
    let users = recorded_by(name)?;
    if !users.is_empty() {
        return Err(Failure::listing(
            format!(
                "source {name} is recorded by {} record(s); `rolesmith remove` its roles there first:",
                users.len()
            ),
            "projects",
            &users,
            |u| json!({ "project": u.project, "roles": u.roles }),
        ));
    }
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
