//! A project's entry files: the `CLAUDE.md` or `AGENTS.md` an agent reads first,
//! committed with the project. rolesmith writes one **managed section** into the entry file of
//! each target the record installs into, between fixed lines, and nothing outside them. The
//! section is the same in every project: it names no role, so `rolesmith.toml` stays the one home
//! of which roles a project uses, and tells an agent on a fresh clone to install rolesmith, sync
//! and read the roles' core skills.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::commands::Failure;
use crate::project::{Synced, TARGETS};

/// The line that opens the managed section.
pub const BEGIN: &str = "<!-- rolesmith: begin. `rolesmith sync` writes this section from rolesmith.toml; edit outside it. -->";
/// The line that closes it.
pub const END: &str = "<!-- rolesmith: end -->";

/// The managed section, its two lines included, ending in a newline.
pub fn section() -> String {
    let mut dirs: Vec<(&str, Vec<&str>)> = Vec::new();
    for t in TARGETS {
        match dirs.iter_mut().find(|(dir, _)| *dir == t.skills) {
            Some((_, names)) => names.push(t.name),
            None => dirs.push((t.skills, vec![t.name])),
        }
    }
    let dirs: Vec<String> = dirs
        .iter()
        .map(|(dir, names)| format!("`{dir}/` for {}", names.join(" and ")))
        .collect();
    format!(
        "{BEGIN}
## Roles (rolesmith)

This project's roles for coding agents are recorded in `rolesmith.toml`: which roles, from which library, at which revision. Before you start any task here, do these steps yourself, without asking anyone:

1. If `command -v rolesmith` finds nothing, install it: `brew install sunfmin/tap/rolesmith`. Otherwise upgrade it: `brew upgrade sunfmin/tap/rolesmith` (it does nothing when rolesmith is the latest; brew's auto-update interval decides how often it reads the tap). If `brew` is not on `PATH`, find it the way this machine installs software.
2. If the skills `rolesmith-*` are missing from your skill directory ({dirs}), or `rolesmith status` reports drift, run `rolesmith sync`.
3. Read the core skill of each role, `rolesmith-<role>/SKILL.md` in that directory, before anything else; every one is in force for all your work here.
4. Run `rolesmith doctor <role>` for each role `rolesmith.toml` records. For a missing program, run the fix it gives (`rolesmith tool install <source>/<role>`), then `rolesmith doctor <role>` again; say in your reply what would not install, and go on with the work that does not need it. For a missing credential (an optokens Secret), tell a person the fix doctor gives; never make one up.

Never run `rolesmith update` during a task here: a role's revision moves only in a commit of its own, never inside another change.
{END}
",
        dirs = dirs.join(", ")
    )
}

/// Give the entry file of each of `targets` the managed section, and take it out of every other
/// entry file of the project at `dir`; with no targets, out of all of them. An entry file that
/// held only the section is deleted. A target's entry file that does not exist is written into
/// the other entry file when that one exists and imports or links it, else created. What changed
/// is added to `synced`.
pub fn update(dir: &Path, targets: &BTreeSet<&str>, synced: &mut Synced) -> Result<(), Failure> {
    let names: BTreeSet<&str> = TARGETS.iter().map(|t| t.entry).collect();
    let wanted: BTreeSet<PathBuf> = TARGETS
        .iter()
        .filter(|t| targets.contains(t.name))
        .map(|t| destination(dir, t.entry, &names))
        .collect();
    let mut seen = BTreeSet::new();
    for name in names {
        let key = identity(dir, name);
        if !seen.insert(key.clone()) {
            continue;
        }
        let path = dir.join(name);
        let text = match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Failure::new(format!("cannot read {}: {e}", path.display()))),
            Ok(text) => text,
        };
        let new = with_section(&text, wanted.contains(&key)).ok_or_else(|| {
            Failure::new(format!(
                "{name} has rolesmith's line `{BEGIN}` without its end line `{END}` after it; \
                 delete the section's lines by hand, then run `rolesmith sync` again"
            ))
        })?;
        if new == text {
            continue;
        }
        if new.is_empty() {
            // The file held only the section, so rolesmith made it.
            std::fs::remove_file(&path)
                .map_err(|e| Failure::new(format!("cannot delete {}: {e}", path.display())))?;
            synced.removed.push(name.to_owned());
        } else {
            std::fs::write(&path, new)
                .map_err(|e| Failure::new(format!("cannot write {}: {e}", path.display())))?;
            synced.written.push(name.to_owned());
        }
    }
    Ok(())
}

/// The file the section for the entry file `name` goes into: `name` itself, unless it does not
/// exist and another entry file that does imports or links it.
fn destination(dir: &Path, name: &str, names: &BTreeSet<&str>) -> PathBuf {
    if !dir.join(name).exists() {
        for other in names.iter().filter(|o| **o != name) {
            let refers = std::fs::read_to_string(dir.join(other)).is_ok_and(|text| {
                [
                    format!("@{name}"),
                    format!("@./{name}"),
                    format!("]({name})"),
                    format!("](./{name})"),
                ]
                .iter()
                .any(|r| text.contains(r.as_str()))
            });
            if refers {
                return identity(dir, other);
            }
        }
    }
    identity(dir, name)
}

/// What the entry file `name` is on disk: its resolved path when it exists (so a symlink and the
/// file it links are one), else where it would be.
fn identity(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    path.canonicalize().unwrap_or_else(|_| {
        dir.canonicalize()
            .unwrap_or_else(|_| dir.to_path_buf())
            .join(name)
    })
}

/// `text` with the managed section in place (`wanted`) or taken out; the rest exactly as it was,
/// except the blank line put before a section appended to a file that had text. `None` when
/// the section's first line has no end line after it.
fn with_section(text: &str, wanted: bool) -> Option<String> {
    let (before, after) = match text.split_once(BEGIN) {
        None => (text, ""),
        Some((before, rest)) => {
            let (_, after) = rest.split_once(END)?;
            (before, after.strip_prefix('\n').unwrap_or(after))
        }
    };
    if !wanted {
        let before = if after.is_empty() && before.ends_with("\n\n") {
            &before[..before.len() - 1]
        } else {
            before
        };
        return Some(if before.trim().is_empty() && after.trim().is_empty() {
            String::new()
        } else {
            format!("{before}{after}")
        });
    }
    let lead = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Some(format!("{before}{lead}{}{after}", section()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_is_appended_after_a_blank_line_and_taken_out_with_it() {
        for mine in ["# Mine\n", "# Mine", "# Mine\n\n"] {
            let with = with_section(mine, true).unwrap();
            assert!(with.starts_with(mine), "{with}");
            assert!(with.contains(&format!("\n\n{BEGIN}\n")), "{with}");
            assert_eq!(with_section(&with, true).unwrap(), with);
            // The blank line before the section goes with it: a file that ended in one ends in
            // a single newline after.
            assert_eq!(with_section(&with, false).unwrap(), "# Mine\n", "{mine:?}");
        }
    }

    #[test]
    fn a_section_alone_is_a_file_of_nothing_and_text_after_it_is_kept() {
        let alone = with_section("", true).unwrap();
        assert_eq!(alone, section());
        assert_eq!(with_section(&alone, false).unwrap(), "");
        let edited = format!("# Mine\n\n{BEGIN}\nhand edit\n{END}\nAfter.\n");
        assert_eq!(
            with_section(&edited, true).unwrap(),
            format!("# Mine\n\n{}After.\n", section())
        );
        assert_eq!(with_section(&edited, false).unwrap(), "# Mine\n\nAfter.\n");
    }

    #[test]
    fn a_first_line_without_an_end_is_refused() {
        assert!(with_section(&format!("{BEGIN}\nno end\n"), true).is_none());
    }

    #[test]
    fn the_section_names_every_skill_directory_and_no_role() {
        let s = section();
        for t in TARGETS {
            assert!(s.contains(&format!("`{}/`", t.skills)), "{s}");
        }
        // What it tells the agent to do.
        for step in [
            "If `command -v rolesmith` finds nothing, install it: `brew install sunfmin/tap/rolesmith`",
            "Otherwise upgrade it: `brew upgrade sunfmin/tap/rolesmith`",
            "or `rolesmith status` reports drift, run `rolesmith sync`",
            "Read the core skill of each role",
            "Run `rolesmith doctor <role>`",
            "`rolesmith tool install <source>/<role>`",
            "For a missing credential (an optokens Secret), tell a person",
            "Never run `rolesmith update` during a task here: a role's revision moves only in a commit of its own, never inside another change.",
        ] {
            assert!(s.contains(step), "{step}: {s}");
        }
    }
}
