//! The repositories external skills come from, kept in rolesmith's own cache:
//! `$XDG_DATA_HOME/rolesmith/skills/<repository>/`, with a bare repository `repo.git` and, per
//! rev a role pins, the tree of that commit `<rev>/`. A rev's tree is written once and never
//! changes, so a cache that holds it answers without git or the network.

use std::path::PathBuf;

use crate::commands::Failure;
use crate::sources::{self, git};

/// Whether `rev` is a full commit SHA, as a pin must be: 40 lowercase hex digits.
pub fn is_full_sha(rev: &str) -> bool {
    rev.len() == 40 && rev.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The directory of `url`'s repository in the cache: its URL with every byte other than ASCII
/// letters, digits, `.` and `-` written as `_<hex>`, so two URLs never share one.
fn repo_dir(url: &str) -> Result<PathBuf, Failure> {
    let mut name = String::new();
    for b in url.bytes() {
        if b.is_ascii_alphanumeric() || b == b'.' || b == b'-' {
            name.push(char::from(b));
        } else {
            name.push_str(&format!("_{b:02x}"));
        }
    }
    Ok(sources::data_dir()?.join("skills").join(name))
}

/// The tree of the commit `rev` of the repository at `url`, from the cache; the repository is
/// fetched only when the cache lacks that tree. A rev the repository does not have is refused.
pub fn tree(url: &str, rev: &str) -> Result<PathBuf, Failure> {
    let dir = repo_dir(url)?;
    let tree = dir.join(rev);
    if tree.is_dir() {
        return Ok(tree);
    }
    let repo = dir.join("repo.git");
    std::fs::create_dir_all(&repo)
        .map_err(|e| Failure::new(format!("cannot create {}: {e}", repo.display())))?;
    git(Some(&repo), &["init", "--quiet", "--bare"])?;
    // Its branches and tags first, then `rev` itself, which a server may give though no branch
    // reaches it.
    let fetch = |refspecs: &[&str]| {
        git(
            Some(&repo),
            &[&["fetch", "--quiet", "--", url], refspecs].concat(),
        )
    };
    if sources::commit(&repo, rev).is_none() {
        fetch(&["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"])?;
    }
    if sources::commit(&repo, rev).is_none() {
        let _ = fetch(&[rev]);
    }
    if sources::commit(&repo, rev).is_none() {
        return Err(Failure::new(format!("{url} has no commit {rev}")));
    }
    // Written beside its place and moved there whole, so a tree in the cache is never partial.
    let staging = tempfile::Builder::new()
        .prefix(&format!(".{rev}."))
        .tempdir_in(&dir)
        .map_err(|e| Failure::new(format!("cannot stage in {}: {e}", dir.display())))?;
    sources::extract(&repo, rev, staging.path())?;
    if std::fs::rename(staging.path(), &tree).is_err() && !tree.is_dir() {
        return Err(Failure::new(format!("cannot write {}", tree.display())));
    }
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_is_a_full_lowercase_sha() {
        assert!(is_full_sha(&"0a".repeat(20)));
        assert!(!is_full_sha(&"0a".repeat(19)));
        assert!(!is_full_sha(&"0A".repeat(20)));
        assert!(!is_full_sha(&"0g".repeat(20)));
    }
}
