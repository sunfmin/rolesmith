//! `rolesmith new <role> --need "<text>" [--source <name>]`: scaffold a role in a source's working
//! copy on a new branch and print the drafting brief (SPEC.md §8, §8.1).

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::library;
use crate::sources::{self, MAIN, Sources};

/// Scaffold roles/<role>/ on a new branch rolesmith/new-<role> and print the drafting brief.
#[derive(clap::Args)]
pub struct Args {
    /// The new role's name (lowercase letters, digits and single hyphens).
    pub role: String,

    /// What the role is for, in the asker's words; the draft description and the brief's need.
    #[arg(long, value_name = "TEXT")]
    pub need: String,

    /// The source to draft in; may be left out when there is only one.
    #[arg(long, value_name = "NAME")]
    pub source: Option<String>,
}

impl Command for Args {
    const NAME: &'static str = "new";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["source", "role", "branch", "dir", "brief"],
            "properties": {
                "source": { "type": "string" },
                "role": { "type": "string" },
                "branch": { "type": "string", "description": "the branch made from main" },
                "dir": { "type": "string", "description": "the source's working copy" },
                "brief": { "type": "string", "description": "the drafting brief, Markdown" }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let role = &self.role;
        if !library::valid_name(role) {
            return Err(Failure::new(format!(
                "{role} is not a valid role name: lowercase letters, digits and single hyphens, 2-{} characters",
                library::MAX_NAME
            )));
        }
        let record = Sources::load()?;
        let source = record.pick(self.source)?;
        let clone = sources::clone_dir(&source)?;
        let dir = Some(clone.as_path());
        let main = sources::main_commit(&clone).ok_or_else(|| {
            Failure::new(format!(
                "source {source} has no local {MAIN} at {}",
                clone.display()
            ))
        })?;

        let dirty = sources::git(dir, &["status", "--porcelain"])?;
        if !dirty.is_empty() {
            return Err(Failure::new(format!(
                "the working copy of {source} ({}) has uncommitted changes; commit, propose or discard them first:\n{dirty}",
                clone.display()
            )));
        }
        if sources::roles_at(&clone, &main).contains(role) {
            return Err(Failure::new(format!(
                "source {source} already has a role {role} on {MAIN}; pick another name or distill it"
            )));
        }
        let branch = format!("rolesmith/new-{role}");
        sources::git(dir, &["checkout", "--quiet", "-b", &branch, MAIN])?;

        let role_dir = clone.join("roles").join(role);
        let description = toml::Value::String(self.need.clone());
        let files = [
            ("role.toml", format!("description = {description}\n")),
            ("core.md", String::new()),
            ("prohibitions.toml", String::new()),
        ];
        std::fs::create_dir_all(&role_dir)
            .map_err(|e| Failure::new(format!("cannot create {}: {e}", role_dir.display())))?;
        for (name, text) in files {
            let path = role_dir.join(name);
            std::fs::write(&path, text)
                .map_err(|e| Failure::new(format!("cannot write {}: {e}", path.display())))?;
        }

        let shown_dir = clone.display().to_string();
        let brief = brief(role, &source, &branch, &shown_dir, &self.need);
        Ok(Output {
            json: json!({
                "source": source,
                "role": role,
                "branch": branch,
                "dir": shown_dir,
                "brief": brief,
            }),
            human: brief,
        })
    }
}

/// The drafting brief, kept as a file so it is reviewed like code (§8.1), its blanks filled.
fn brief(role: &str, source: &str, branch: &str, dir: &str, need: &str) -> String {
    format!(
        include_str!("../briefs/new.md"),
        role = role,
        source = source,
        branch = branch,
        dir = dir,
        need = need.trim(),
        max_name = library::MAX_NAME,
        max_description = library::MAX_ROLE_DESCRIPTION,
        max_core = library::MAX_CORE,
        max_skill_description = library::MAX_SKILL_DESCRIPTION,
        consumer_tables = library::CONSUMER_TABLES.join(", "),
    )
}
