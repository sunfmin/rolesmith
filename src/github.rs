//! A library's GitHub repository, as `gh` reaches it (SPEC.md §6, §10): which repository a source's
//! URL names, and running `gh` against it.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::commands::Failure;

/// A GitHub repository, `owner/name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo(pub String);

impl Repo {
    /// The repository a git URL names, if it is one on github.com: `https://github.com/o/r`,
    /// `git@github.com:o/r` or `ssh://git@github.com/o/r`, with or without `.git`.
    pub fn from_url(url: &str) -> Option<Repo> {
        let path = [
            "https://github.com/",
            "http://github.com/",
            "ssh://git@github.com/",
        ]
        .iter()
        .find_map(|prefix| url.strip_prefix(prefix))
        .or_else(|| url.strip_prefix("git@github.com:"))?;
        let path = path.trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, name) = path.split_once('/')?;
        let part = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        };
        (part(owner) && part(name)).then(|| Repo(format!("{owner}/{name}")))
    }

    /// The repository of the source `name` recorded at `url`, or a refusal saying why lessons need
    /// one.
    pub fn of_source(name: &str, url: &str) -> Result<Repo, Failure> {
        Repo::from_url(url).ok_or_else(|| {
            Failure::new(format!(
                "source {name} ({url}) is not a GitHub repository; lessons are issues on the \
                 library's GitHub repository, so its URL must be one on github.com"
            ))
        })
    }

    /// Run `gh <args> --repo <repo>`, with `input` on its stdin when given; its stdout, or a
    /// failure carrying its stderr.
    pub fn gh(&self, args: &[&str], input: Option<&str>) -> Result<String, Failure> {
        let shown = || format!("gh {} --repo {self}", args.join(" "));
        let ran = |e: std::io::Error| Failure::new(format!("cannot run {}: {e}", shown()));
        let mut child = Command::new("gh")
            .args(args)
            .args(["--repo", &self.0])
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ran)?;
        if let Some(input) = input {
            let mut stdin = child.stdin.take().expect("stdin is piped");
            stdin.write_all(input.as_bytes()).map_err(ran)?;
        }
        let out = child.wait_with_output().map_err(ran)?;
        if !out.status.success() {
            return Err(Failure::new(format!(
                "{} failed: {}",
                shown(),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Like [`Repo::gh`] without input, parsing stdout as JSON.
    pub fn gh_json(&self, args: &[&str]) -> Result<Value, Failure> {
        let out = self.gh(args, None)?;
        serde_json::from_str(&out).map_err(|e| {
            Failure::new(format!(
                "gh {} answered what is not JSON: {e}",
                args.join(" ")
            ))
        })
    }
}

impl std::fmt::Display for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_urls_name_their_repository() {
        for url in [
            "https://github.com/sunfminforge/lore",
            "https://github.com/sunfminforge/lore.git",
            "https://github.com/sunfminforge/lore/",
            "git@github.com:sunfminforge/lore.git",
            "ssh://git@github.com/sunfminforge/lore",
        ] {
            assert_eq!(
                Repo::from_url(url),
                Some(Repo("sunfminforge/lore".into())),
                "{url}"
            );
        }
    }

    #[test]
    fn other_urls_are_not_github() {
        for url in [
            "/tmp/lore.git",
            "https://gitlab.com/a/b",
            "https://github.com/only-owner",
            "https://github.com/a/b/c",
            "https://github.com.evil.example/a/b",
            "file:///tmp/a/b",
        ] {
            assert_eq!(Repo::from_url(url), None, "{url}");
        }
    }
}
