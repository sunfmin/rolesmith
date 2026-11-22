//! The libraries a machine knows, as sources (SPEC.md §2): their record in
//! `$XDG_CONFIG_HOME/rolesmith/sources.toml`, their clones under
//! `$XDG_DATA_HOME/rolesmith/sources/<name>/`, and the roles they hold at a commit.
//!
//! Roles are read from a commit of the clone (its local `main` unless a rev is asked for), never
//! from its working tree, so what `list` and `show` print is what an install at that rev writes.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::commands::Failure;
use crate::library::{self, Library};

/// The branch of a library that is its current state (§5.4).
pub const MAIN: &str = "main";

/// `sources.toml`: each source by its name.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sources {
    #[serde(default)]
    pub source: BTreeMap<String, Entry>,
}

/// One source's record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub url: String,
}

/// `$XDG_CONFIG_HOME/rolesmith/sources.toml` (default `~/.config/rolesmith/`).
pub fn sources_file() -> Result<PathBuf, Failure> {
    Ok(xdg("XDG_CONFIG_HOME", ".config")?
        .join("rolesmith")
        .join("sources.toml"))
}

/// `$XDG_DATA_HOME/rolesmith/sources/` (default `~/.local/share/rolesmith/`).
pub fn clones_dir() -> Result<PathBuf, Failure> {
    Ok(xdg("XDG_DATA_HOME", ".local/share")?
        .join("rolesmith")
        .join("sources"))
}

/// The clone of the source `name`.
pub fn clone_dir(name: &str) -> Result<PathBuf, Failure> {
    Ok(clones_dir()?.join(name))
}

fn xdg(var: &str, default_under_home: &str) -> Result<PathBuf, Failure> {
    match std::env::var_os(var) {
        Some(dir) if Path::new(&dir).is_absolute() => Ok(PathBuf::from(dir)),
        _ => match std::env::var_os("HOME") {
            Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(default_under_home)),
            _ => Err(Failure::new(format!("neither {var} nor HOME is set"))),
        },
    }
}

impl Sources {
    /// The record; empty when there is no file yet.
    pub fn load() -> Result<Sources, Failure> {
        let file = sources_file()?;
        match std::fs::read_to_string(&file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Sources::default()),
            Err(e) => Err(Failure::new(format!("cannot read {}: {e}", file.display()))),
            Ok(text) => toml::from_str(&text).map_err(|e| {
                Failure::new(format!(
                    "{} does not parse: {}",
                    file.display(),
                    e.message().trim()
                ))
            }),
        }
    }

    /// Write the record, replacing the file whole so a reader never sees half of it.
    pub fn save(&self) -> Result<(), Failure> {
        let file = sources_file()?;
        let fail =
            |e: std::io::Error| Failure::new(format!("cannot write {}: {e}", file.display()));
        std::fs::create_dir_all(file.parent().expect("the file has a directory")).map_err(fail)?;
        let text = toml::to_string(self).expect("the record serializes");
        let partial = file.with_extension("toml.partial");
        std::fs::write(&partial, text).map_err(fail)?;
        std::fs::rename(&partial, &file).map_err(fail)
    }

    /// The source `name`, or a refusal naming the known ones.
    pub fn get(&self, name: &str) -> Result<&Entry, Failure> {
        self.source.get(name).ok_or_else(|| {
            Failure::new(format!(
                "no source named {name} (known: {})",
                self.names_or_none()
            ))
        })
    }

    fn names_or_none(&self) -> String {
        if self.source.is_empty() {
            "none; add one with `rolesmith source add <name> <git-url>`".to_owned()
        } else {
            self.source.keys().cloned().collect::<Vec<_>>().join(", ")
        }
    }
}

/// Run git with `args` (in `dir` when given); its stdout, trimmed, or a failure carrying its stderr.
pub fn git<S: AsRef<OsStr>>(dir: Option<&Path>, args: &[S]) -> Result<String, Failure> {
    let mut cmd = Command::new("git");
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args).stdin(Stdio::null());
    let shown = || {
        args.iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let out = cmd
        .output()
        .map_err(|e| Failure::new(format!("cannot run git: {e}")))?;
    if !out.status.success() {
        return Err(Failure::new(format!(
            "git {} failed: {}",
            shown(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The full SHA of the clone's local `main`, if it has one.
pub fn main_commit(clone: &Path) -> Option<String> {
    commit(clone, MAIN)
}

/// The full SHA of the commit `rev` names in `clone`, if it names one.
fn commit(clone: &Path, rev: &str) -> Option<String> {
    git(
        Some(clone),
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .ok()
}

/// The names of the roles of `clone` at the commit `sha`, sorted.
pub fn roles_at(clone: &Path, sha: &str) -> Vec<String> {
    let Ok(listing) = git(
        Some(clone),
        &["ls-tree", "-d", "--name-only", &format!("{sha}:roles")],
    ) else {
        return Vec::new();
    };
    let mut names: Vec<String> = listing
        .lines()
        .filter(|n| !n.starts_with('.'))
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// The text of `path` at the commit `sha` of `clone`, if it is a file there.
pub fn file_at(clone: &Path, sha: &str, path: &str) -> Option<String> {
    git(Some(clone), &["show", &format!("{sha}:{path}")]).ok()
}

/// A role found in a source at a commit.
#[derive(Debug, Clone)]
pub struct Found {
    pub source: String,
    pub role: String,
    /// The full SHA it was read at.
    pub rev: String,
}

impl Found {
    pub fn qualified(&self) -> String {
        format!("{}/{}", self.source, self.role)
    }
}

/// What `rev` a lookup reads: the local `main` of each source, or a commit asked for.
fn wanted_rev(rev: Option<&str>) -> Result<&str, Failure> {
    match rev {
        None => Ok(MAIN),
        Some(r) if (4..=64).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(r),
        Some(r) => Err(Failure::new(format!(
            "--rev {r} is not a commit SHA (4 to 64 hex digits)"
        ))),
    }
}

/// Resolve a role address (§2): `<source>/<role>`, or `<role>` when exactly one source has it at
/// `rev` (default: each source's local `main`). An ambiguous bare name is refused, listing the
/// addresses that would work.
pub fn resolve(sources: &Sources, address: &str, rev: Option<&str>) -> Result<Found, Failure> {
    let wanted = wanted_rev(rev)?;
    let at = |source: &str, role: &str| -> Result<Option<Found>, Failure> {
        let clone = clone_dir(source)?;
        let Some(sha) = commit(&clone, wanted) else {
            return Ok(None);
        };
        let has = library::valid_name(role)
            && git(
                Some(&clone),
                &["cat-file", "-t", &format!("{sha}:roles/{role}")],
            )
            .is_ok_and(|t| t == "tree");
        Ok(has.then(|| Found {
            source: source.to_owned(),
            role: role.to_owned(),
            rev: sha,
        }))
    };
    let at_what = match rev {
        None => format!("on {MAIN}"),
        Some(r) => format!("at {r}"),
    };

    if let Some((source, role)) = address.split_once('/') {
        sources.get(source)?;
        return at(source, role)?
            .ok_or_else(|| Failure::new(format!("source {source} has no role {role} {at_what}")));
    }

    let mut found = Vec::new();
    for source in sources.source.keys() {
        if let Some(f) = at(source, address)? {
            found.push(f);
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 if sources.source.is_empty() => Err(Failure::new(format!(
            "no role {address}: no sources ({})",
            sources.names_or_none()
        ))),
        0 => Err(Failure::new(format!(
            "no source has a role {address} {at_what}"
        ))),
        _ => Err(Failure::new(format!(
            "role {address} is in more than one source; name one of: {}",
            found
                .iter()
                .map(Found::qualified)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The library of `found`'s source as it was at `found.rev`, read and checked with only that role.
/// The tree is extracted into a temporary directory that lives as long as the answer.
pub fn read_role(found: &Found) -> Result<(Library, tempfile::TempDir), Failure> {
    let clone = clone_dir(&found.source)?;
    let tree = tempfile::tempdir()
        .map_err(|e| Failure::new(format!("cannot make a temporary directory: {e}")))?;
    let archive = tree.path().join(".tree.tar");
    git(
        Some(&clone),
        &[
            OsStr::new("archive"),
            OsStr::new("--format=tar"),
            OsStr::new("-o"),
            archive.as_os_str(),
            OsStr::new(&found.rev),
        ],
    )?;
    let out = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(tree.path())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Failure::new(format!("cannot run tar: {e}")))?;
    if !out.status.success() {
        return Err(Failure::new(format!(
            "cannot extract {} at {}: {}",
            found.source,
            found.rev,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let _ = std::fs::remove_file(&archive);
    match library::load(tree.path(), Some(&found.role)) {
        Ok(lib) => Ok((lib, tree)),
        Err(problems) => {
            let mut message = format!(
                "{} at {} has {} problem(s); `rolesmith check` in the library says the same:",
                found.qualified(),
                found.rev,
                problems.len()
            );
            for p in &problems {
                message.push_str(&format!("\n  {p}"));
            }
            Err(Failure { message, problems })
        }
    }
}
