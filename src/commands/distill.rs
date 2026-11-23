//! `rolesmith distill <role>`: put the role's source on a new branch and print the distilling brief
//! with the role's current text and its open lessons (SPEC.md §6, §8.1).

use serde_json::{Value, json};

use super::{Command, Failure, Output, lesson};
use crate::github::Repo;
use crate::library::{self, Role};
use crate::sources::{self, MAIN, Sources};

/// Fold a role's open lessons into it: put its source's working copy on a new branch `rolesmith/distill-<role>` and print the distilling brief.
///
/// Run it when someone asks an agent to distill a role. It asks `gh` for the role's open lessons; with none, it says so and changes nothing (`branch` and `brief` are `null`). The brief holds the role's current core, prohibitions and skills, each open lesson, where each belongs (the narrowest place; the core only for rules of every task), `rolesmith lesson close` for one judged wrong, and the last step: `rolesmith check`, then `rolesmith propose --closes <n>...`.
///
/// ## Refusals
///
/// - a working copy with uncommitted changes;
/// - a branch `rolesmith/distill-<role>` that exists.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    /// The role: <role> when one source has it, <source>/<role> otherwise.
    pub role: String,
}

/// An open lesson as the brief shows it.
struct Lesson {
    number: u64,
    title: String,
    /// What it says, without the `Evidence:` and `Handed in by:` lines `lesson add` ends it with.
    body: String,
    evidence: Option<String>,
    url: String,
}

impl Lesson {
    fn from_issue(issue: &Value) -> Lesson {
        let text = issue["body"].as_str().unwrap_or_default();
        let mut evidence = None;
        let mut body = Vec::new();
        for line in text.lines() {
            if let Some(link) = line.strip_prefix("Evidence:") {
                evidence = Some(link.trim().to_owned());
            } else if !line.starts_with("Handed in by:") {
                body.push(line);
            }
        }
        Lesson {
            number: issue["number"].as_u64().unwrap_or_default(),
            title: issue["title"].as_str().unwrap_or_default().to_owned(),
            body: body.join("\n").trim().to_owned(),
            evidence,
            url: issue["url"].as_str().unwrap_or_default().to_owned(),
        }
    }

    fn to_json(&self) -> Value {
        json!({ "number": self.number, "title": self.title, "body": self.body,
                "evidence": self.evidence, "url": self.url })
    }

    fn to_markdown(&self) -> String {
        let body = if self.body.is_empty() {
            "(no text beyond the title)"
        } else {
            &self.body
        };
        format!(
            "### #{} {}\n\n{body}\n\nEvidence: {}\n\nIssue: {}\n",
            self.number,
            self.title,
            self.evidence.as_deref().unwrap_or("(none given)"),
            self.url
        )
    }
}

impl Command for Args {
    const NAME: &'static str = "distill";
    const GROUP: &'static str = super::groups::LIBRARY;

    fn output_shape() -> Value {
        let string = json!({ "type": "string" });
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["source", "role", "branch", "dir", "lessons", "brief"],
            "properties": {
                "source": string,
                "role": string,
                "branch": { "type": ["string", "null"],
                    "description": "the branch made from main; null when there were no open lessons" },
                "dir": { "type": "string", "description": "the source's working copy" },
                "lessons": { "type": "array", "items": {
                    "type": "object", "required": ["number", "title", "body", "evidence", "url"],
                    "properties": {
                        "number": { "type": "integer", "minimum": 1 },
                        "title": string,
                        "body": string,
                        "evidence": { "type": ["string", "null"] },
                        "url": string
                    }
                } },
                "brief": { "type": ["string", "null"],
                    "description": "the distilling brief, Markdown; null when there were no open lessons" }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let record = Sources::load()?;
        let found = sources::resolve(&record, &self.role, None)?;
        let (source, role) = (found.source.clone(), found.name.clone());
        let repo = Repo::of_source(&source, &record.get(&source)?.url)?;
        let clone = sources::clone_dir(&source)?;
        let shown_dir = clone.display().to_string();

        let lessons: Vec<Lesson> = lesson::open(
            &repo,
            Some(&format!("role:{role}")),
            "number,title,body,url",
        )?
        .iter()
        .map(Lesson::from_issue)
        .collect();
        if lessons.is_empty() {
            return Ok(Output {
                json: json!({ "source": source, "role": role, "branch": null, "dir": shown_dir,
                              "lessons": [], "brief": null }),
                human: format!(
                    "{} has no open lessons on {repo}; nothing to distill, no branch made\n",
                    found.qualified()
                ),
            });
        }

        let dir = Some(clone.as_path());
        let dirty = sources::git(dir, &["status", "--porcelain"])?;
        if !dirty.is_empty() {
            return Err(Failure::new(format!(
                "the working copy of {source} ({shown_dir}) has uncommitted changes; commit, propose or discard them first:\n{dirty}"
            )));
        }
        let (lib, _tree) = sources::read_role(&found)?;
        let branch = format!("rolesmith/distill-{role}");
        sources::git(dir, &["checkout", "--quiet", "-b", &branch, MAIN])?;

        let brief = brief(&lib.roles[0], &source, &repo, &branch, &shown_dir, &lessons);
        Ok(Output {
            json: json!({
                "source": source,
                "role": role,
                "branch": branch,
                "dir": shown_dir,
                "lessons": lessons.iter().map(Lesson::to_json).collect::<Vec<_>>(),
                "brief": brief,
            }),
            human: brief,
        })
    }
}

/// A Markdown list of `items`, or `(none)`.
fn bullets(items: Vec<String>) -> String {
    if items.is_empty() {
        return "(none)".to_owned();
    }
    items
        .iter()
        .map(|i| format!("- {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The distilling brief, kept as a file so it is reviewed like code (§8.1), its blanks filled.
fn brief(
    role: &Role,
    source: &str,
    repo: &Repo,
    branch: &str,
    dir: &str,
    lessons: &[Lesson],
) -> String {
    let core = role.core.trim();
    format!(
        include_str!("../briefs/distill.md"),
        role = role.name,
        source = source,
        repo = repo,
        branch = branch,
        dir = dir,
        count = lessons.len(),
        core = if core.is_empty() { "(empty)" } else { core },
        prohibitions = bullets(
            role.prohibitions
                .iter()
                .map(|p| format!("{} (why: {})", p.rule, p.why))
                .collect()
        ),
        skills = bullets(
            role.skills
                .iter()
                .map(|s| format!("`{}`: {}", s.name, s.description))
                .collect()
        ),
        lessons = lessons
            .iter()
            .map(Lesson::to_markdown)
            .collect::<Vec<_>>()
            .join("\n"),
        max_name = library::MAX_NAME,
        max_description = library::MAX_ROLE_DESCRIPTION,
        max_core = library::MAX_CORE,
        max_skill_description = library::MAX_SKILL_DESCRIPTION,
    )
}
