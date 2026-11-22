//! The targets (SPEC.md §5.2): `codex` and `agents` install the same files as `claude-code`, under
//! their own skill directory, in a project and for the user; an unknown target is a usage error.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{Machine, published, stderr};
use serde_json::json;

/// Every file under `dir`, by its path relative to `dir`, with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(at) = pending.pop() {
        for entry in std::fs::read_dir(&at).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let rel = path.strip_prefix(dir).unwrap().to_str().unwrap().to_owned();
                files.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

/// Each target this file covers and its skill directory.
const OTHERS: &[(&str, &str)] = &[("codex", ".agents/skills"), ("agents", ".agents/skills")];

/// A machine with source `felix` (role app-store-releaser, which has a skill), published.
fn machine() -> Machine {
    let m = Machine::new();
    let (lib, _) = published(&m, "felix", &["app-store-releaser"]);
    m.json(&["source", "add", "felix", lib.url()]);
    m
}

/// Under `root`, what `target` installed in `dir` is what claude-code installed in
/// `.claude/skills`, file for file.
fn same_as_claude_code(root: &Path, dir: &str) {
    let claude = snapshot(&root.join(".claude/skills"));
    assert!(
        claude.contains_key("rolesmith-app-store-releaser/SKILL.md"),
        "{claude:?}"
    );
    assert!(claude.contains_key("rolesmith-app-store-releaser-submit-build/SKILL.md"));
    assert_eq!(snapshot(&root.join(dir)), claude);
}

#[test]
fn each_target_installs_claude_codes_files_under_its_directory_in_a_project() {
    let m = machine();
    for (target, dir) in OTHERS {
        let project = m.path(&format!("project-{target}"));
        std::fs::create_dir_all(&project).unwrap();
        let p = project.to_str().unwrap();
        let out = m.json(&[
            "use",
            "app-store-releaser",
            "--into",
            "claude-code",
            "--into",
            target,
            "--project",
            p,
        ]);
        assert_eq!(out["roles"][0]["into"], json!(["claude-code", target]));
        same_as_claude_code(&project, dir);
        let ignore = std::fs::read_to_string(project.join(".gitignore")).unwrap();
        assert!(
            ignore.contains(&format!("\n/{dir}/rolesmith-app-store-releaser/\n")),
            "{ignore}"
        );

        // A fresh clone (the record alone) is brought to the same files.
        let clone = m.path(&format!("clone-{target}"));
        std::fs::create_dir_all(&clone).unwrap();
        std::fs::copy(project.join("rolesmith.toml"), clone.join("rolesmith.toml")).unwrap();
        m.json(&["sync", "--project", clone.to_str().unwrap()]);
        assert_eq!(snapshot(&clone.join(dir)), snapshot(&project.join(dir)));

        // Removing the role removes its files from both directories.
        m.json(&["remove", "app-store-releaser", "--project", p]);
        assert!(
            !project
                .join(dir)
                .join("rolesmith-app-store-releaser")
                .exists()
        );
        assert!(
            !project
                .join(".claude/skills/rolesmith-app-store-releaser")
                .exists()
        );
    }
}

#[test]
fn each_target_installs_claude_codes_files_under_its_directory_for_the_user() {
    for (target, dir) in OTHERS {
        let m = machine();
        let out = m.json(&[
            "use",
            "app-store-releaser",
            "--into",
            "claude-code",
            "--into",
            target,
            "--global",
        ]);
        assert_eq!(out["project"], json!(null));
        same_as_claude_code(&m.path("home"), dir);
    }
}

#[test]
fn codex_and_agents_share_a_directory_and_a_role_in_both_writes_it_once() {
    let m = machine();
    let project = m.path("project");
    std::fs::create_dir_all(&project).unwrap();
    let p = project.to_str().unwrap();
    let args = [
        "use",
        "app-store-releaser",
        "--into",
        "codex",
        "--into",
        "agents",
        "--project",
        p,
    ];
    m.json(&args);
    let files = snapshot(&project.join(".agents/skills"));
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "rolesmith-app-store-releaser-submit-build/SKILL.md",
            "rolesmith-app-store-releaser/SKILL.md"
        ]
    );
    let again = m.rolesmith(&args);
    assert_eq!(again.status.code(), Some(0), "{}", stderr(&again));
}

#[test]
fn an_unknown_target_is_a_usage_error_naming_the_known_ones() {
    let m = machine();
    let project = m.path("project");
    std::fs::create_dir_all(&project).unwrap();
    let o = m.rolesmith(&[
        "use",
        "app-store-releaser",
        "--into",
        "cursor",
        "--project",
        project.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("unknown target cursor (known: claude-code, codex, agents)"),
        "{}",
        stderr(&o)
    );
    assert!(!project.join("rolesmith.toml").exists());
}
