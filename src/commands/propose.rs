//! `rolesmith propose [<role>] [--source <name>] [--closes <n>...]`: what an agent changed in a
//! source's working copy becomes a pull request a person merges (SPEC.md §1.2, §8). It checks the
//! library, commits on the working copy's `rolesmith/*` branch, pushes that branch (never `main`,
//! never forced) and opens the pull request through `gh`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command as Process, Stdio};

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::library;
use crate::problem::Problem;
use crate::sources::{self, MAIN, Sources};

/// The branches `propose` pushes: those `new` and `distill` put the working copy on.
const BRANCH_PREFIX: &str = "rolesmith/";

/// Check, commit and push a source's working copy branch, and open its pull request.
#[derive(clap::Args)]
pub struct Args {
    /// Check only this role before proposing (default: the whole library).
    pub role: Option<String>,

    /// The source whose working copy to propose; may be left out when there is only one.
    #[arg(long, value_name = "NAME")]
    pub source: Option<String>,

    /// A lesson issue the change absorbs; the pull request closes it. Repeatable.
    #[arg(long, value_name = "N")]
    pub closes: Vec<u64>,
}

impl Command for Args {
    const NAME: &'static str = "propose";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["source", "branch", "commit", "roles", "closes", "url"],
            "properties": {
                "source": { "type": "string" },
                "branch": { "type": "string", "description": "the branch pushed" },
                "commit": { "type": "string", "description": "the full SHA pushed" },
                "roles": { "type": "array", "items": { "type": "string" },
                           "description": "the roles the branch changes from main" },
                "closes": { "type": "array", "items": { "type": "integer" } },
                "url": { "type": "string", "description": "the pull request" }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let record = Sources::load()?;
        let source = record.pick(self.source)?;
        let clone = sources::clone_dir(&source)?;
        let dir = Some(clone.as_path());

        let branch = sources::git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .map_err(|_| Failure::new(format!("the working copy of {source} is on no branch")))?;
        if !branch.starts_with(BRANCH_PREFIX) {
            return Err(Failure::new(format!(
                "the working copy of {source} ({}) is on {branch}, not a {BRANCH_PREFIX}* branch; \
                 `rolesmith new` or `rolesmith distill` puts it on one",
                clone.display()
            )));
        }

        if let Err(problems) = library::load(&clone, self.role.as_deref()) {
            let header = format!(
                "nothing proposed: `rolesmith check` finds {} problem(s) in {}:",
                problems.len(),
                clone.display()
            );
            return Err(Failure::listing(
                header,
                "problems",
                &problems,
                Problem::to_json,
            ));
        }

        sources::git(dir, &["add", "--all"])?;
        let changes = changes(&clone)?;
        if changes.is_empty() {
            return Err(Failure::new(format!(
                "nothing to propose: {branch} of {source} does not differ from {MAIN}"
            )));
        }
        let roles: Vec<&str> = changes.keys().filter_map(|r| r.as_deref()).collect();
        let title = title(&roles);
        let staged = sources::git(dir, &["diff", "--cached", "--name-only"])?;
        if !staged.is_empty() {
            sources::git(dir, &["commit", "--quiet", "-m", &title])?;
        }
        let commit = sources::git(dir, &["rev-parse", "HEAD"])?;

        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        sources::git(dir, &["push", "--quiet", "origin", &refspec])?;

        let body = body(&changes, &self.closes);
        let url = gh_pr_create(&clone, &branch, &title, &body)?;
        Ok(Output {
            json: json!({
                "source": source,
                "branch": branch,
                "commit": commit,
                "roles": roles,
                "closes": self.closes,
                "url": url,
            }),
            human: format!("{url}\n"),
        })
    }
}

/// What the working copy (staged) changes from `main`'s merge base, by role (`None`: outside
/// any role), each a `git diff --name-status` line.
fn changes(clone: &Path) -> Result<BTreeMap<Option<String>, Vec<String>>, Failure> {
    let base = sources::git(Some(clone), &["merge-base", MAIN, "HEAD"])?;
    let listing = sources::git(
        Some(clone),
        &["diff", "--cached", "--no-renames", "--name-status", &base],
    )?;
    let mut by_role: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
    for line in listing.lines() {
        let Some((status, path)) = line.split_once('\t') else {
            continue;
        };
        let role = path
            .strip_prefix("roles/")
            .and_then(|rest| rest.split_once('/'))
            .map(|(role, _)| role.to_owned());
        by_role
            .entry(role)
            .or_default()
            .push(format!("{status} {path}"));
    }
    Ok(by_role)
}

fn title(roles: &[&str]) -> String {
    match roles {
        [] => "Change the library".to_owned(),
        [one] => format!("Change the role {one}"),
        many => format!("Change the roles {}", many.join(", ")),
    }
}

/// The pull request's body: what changed per role, then a `Closes #<n>` line per lesson.
fn body(changes: &BTreeMap<Option<String>, Vec<String>>, closes: &[u64]) -> String {
    let mut text = String::from("Proposed by `rolesmith propose`.\n");
    // Roles first (`Some` sorts after `None`), then what lies outside them.
    let ordered = changes
        .iter()
        .filter(|(r, _)| r.is_some())
        .chain(changes.iter().filter(|(r, _)| r.is_none()));
    for (role, files) in ordered {
        match role {
            Some(role) => text.push_str(&format!("\n## {role}\n\n")),
            None => text.push_str("\n## Outside any role\n\n"),
        }
        for file in files {
            text.push_str(&format!("- `{file}`\n"));
        }
    }
    if !closes.is_empty() {
        text.push('\n');
        for n in closes {
            text.push_str(&format!("Closes #{n}\n"));
        }
    }
    text
}

/// Open the pull request of `branch` onto `main` in the clone's repository; answer its URL, the
/// last line `gh pr create` prints.
fn gh_pr_create(clone: &Path, branch: &str, title: &str, body: &str) -> Result<String, Failure> {
    let out = Process::new("gh")
        .args(["pr", "create", "--base", MAIN, "--head", branch])
        .args(["--title", title, "--body", body])
        .current_dir(clone)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Failure::new(format!("cannot run gh: {e}")))?;
    if !out.status.success() {
        return Err(Failure::new(format!(
            "{branch} is pushed, but `gh pr create` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Failure::new("`gh pr create` printed no pull request URL"))
}
