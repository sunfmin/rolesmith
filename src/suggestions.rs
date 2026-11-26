//! Which roles fit a project by what it is: each role's `[match]` on its source's
//! local `main`, met by the project's paths and files. `suggest` prints [`suggest`]'s answer and
//! `use --auto` installs it, so the two cannot disagree.

use std::path::Path;

use serde::Deserialize;

use crate::commands::Failure;
use crate::library::Match;
use crate::sources::{self, Kind, Sources};

/// Directories never walked, besides what `.gitignore` ignores.
const SKIPPED: &[&str] = &[".git", "target", "node_modules"];

/// A role whose `[match]` the project meets, and the first rule that met it.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub source: String,
    pub role: String,
    /// The rule as role.toml writes it: `files = "<glob>"` or `contains = { file, text }`.
    pub rule: String,
    /// The project path (relative to its root, with `/`) that met it.
    pub path: String,
}

impl Suggestion {
    pub fn qualified(&self) -> String {
        format!("{}/{}", self.source, self.role)
    }
}

/// Every role of every source whose `[match]` the project at `dir` meets, by source then role.
/// A role whose role.toml does not parse matches nothing (`check` says why).
pub fn suggest(dir: &Path) -> Result<Vec<Suggestion>, Failure> {
    let paths = project_paths(dir)?;
    let known = Sources::load()?;
    let mut found = Vec::new();
    for source in known.source.keys() {
        let clone = sources::clone_dir(source)?;
        let Some(sha) = sources::main_commit(&clone) else {
            continue;
        };
        for role in sources::roles_at(&clone, &sha) {
            let Some(rules) = rules_of(&clone, &sha, &role) else {
                continue;
            };
            let met = first_met(&rules, dir, &paths)
                .map_err(|e| Failure::new(format!("{source}/{role}'s role.toml: {e}")))?;
            if let Some((rule, path)) = met {
                found.push(Suggestion {
                    source: source.clone(),
                    role,
                    rule,
                    path,
                });
            }
        }
    }
    Ok(found)
}

/// `[match]` of the role's role.toml at `sha`, if it has one that parses.
fn rules_of(clone: &Path, sha: &str, role: &str) -> Option<Match> {
    #[derive(Deserialize)]
    struct OnlyMatch {
        #[serde(rename = "match")]
        rules: Option<Match>,
    }
    let text = sources::toml_at(clone, sha, Kind::Role, role)?;
    toml::from_str::<OnlyMatch>(&text).ok()?.rules
}

/// The first rule of `rules` the project meets, with the path that met it. A glob is matched
/// against each path relative to the root; `*` does not cross `/`, `**` does.
fn first_met(
    rules: &Match,
    dir: &Path,
    paths: &[String],
) -> Result<Option<(String, String)>, String> {
    for glob in &rules.files {
        let matcher = globset::GlobBuilder::new(glob)
            .literal_separator(true)
            .build()
            .map_err(|e| format!("files glob {glob:?} does not parse: {e}"))?
            .compile_matcher();
        if let Some(path) = paths.iter().find(|p| matcher.is_match(p.as_str())) {
            return Ok(Some((format!("files = {glob:?}"), path.clone())));
        }
    }
    for c in &rules.contains {
        let holds = std::fs::read(dir.join(&c.file))
            .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(&c.text));
        if holds {
            let rule = format!("contains = {{ file = {:?}, text = {:?} }}", c.file, c.text);
            return Ok(Some((rule, c.file.clone())));
        }
    }
    Ok(None)
}

/// Every file and directory under `dir`, relative to it with `/`, sorted; skipping [`SKIPPED`]
/// and what `.gitignore` ignores, whether or not the project is a git repository.
fn project_paths(dir: &Path) -> Result<Vec<String>, Failure> {
    let walk = ignore::WalkBuilder::new(dir)
        .standard_filters(false)
        .git_ignore(true)
        .require_git(false)
        .filter_entry(|e| !SKIPPED.iter().any(|s| e.file_name() == *s))
        .build();
    let mut paths = Vec::new();
    for entry in walk {
        let entry = entry.map_err(|e| Failure::new(format!("cannot read the project: {e}")))?;
        let Ok(rel) = entry.path().strip_prefix(dir) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        paths.push(parts.join("/"));
    }
    paths.sort();
    Ok(paths)
}
