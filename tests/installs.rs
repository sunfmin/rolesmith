//! `status`, `update`, `remove` (SPEC.md §5.4, §8) and `source remove` refusing a source a project
//! records, through the built binary.

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

const CORE: &str = ".claude/skills/rolesmith-app-store-releaser/SKILL.md";

/// A machine with source `felix` (role app-store-releaser, published at the returned rev) and a
/// project holding its own files.
fn setup() -> (Machine, common::Library, String, std::path::PathBuf) {
    let m = Machine::new();
    let (lib, rev) = published(&m, "felix", &["app-store-releaser"]);
    m.json(&["source", "add", "felix", lib.url()]);
    let project = m.path("project");
    std::fs::create_dir_all(project.join(".claude/skills/my-own")).unwrap();
    std::fs::write(project.join(".claude/skills/my-own/SKILL.md"), "mine\n").unwrap();
    std::fs::write(project.join(".gitignore"), "/target\n").unwrap();
    std::fs::write(project.join("CLAUDE.md"), "# Mine\n").unwrap();
    (m, lib, rev, project)
}

fn use_role(m: &Machine, project: &Path, extra: &[&str]) -> serde_json::Value {
    let mut args = vec![
        "use",
        "app-store-releaser",
        "--into",
        "claude-code",
        "--project",
        project.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    m.json(&args)
}

#[test]
fn status_shows_behind_ahead_and_a_hand_edited_file_as_drift() {
    let (m, lib, first, project) = setup();
    let p = project.to_str().unwrap();
    use_role(&m, &project, &[]);

    let out = m.json(&["status", "--project", p]);
    assert_eq!(
        out["roles"],
        json!([{ "name": "app-store-releaser", "source": "felix", "rev": first, "main": first,
                 "behind": 0, "ahead": 0, "requires": [], "drift": [] }])
    );
    let human = String::from_utf8(m.rolesmith(&["status", "--project", p]).stdout).unwrap();
    assert_eq!(
        human,
        format!("felix/app-store-releaser at {first}; at main; files match\n")
    );

    lib.write(
        "roles/app-store-releaser/core.md",
        "# Core\n\nSecond rule.\n",
    );
    let second = m.publish(&lib, "second");
    m.json(&["source", "update"]);
    std::fs::write(project.join(CORE), "edited by hand\n").unwrap();
    let out = m.json(&["status", "--project", p]);
    let role = &out["roles"][0];
    assert_eq!(
        (&role["rev"], &role["main"], &role["behind"], &role["ahead"]),
        (&json!(first), &json!(second), &json!(1), &json!(0))
    );
    assert_eq!(role["drift"], json!([CORE]));
    let human = String::from_utf8(m.rolesmith(&["status", "--project", p]).stdout).unwrap();
    assert_eq!(
        human,
        format!(
            "felix/app-store-releaser at {first}; main {second}: 1 behind, 0 ahead; \
             drift (run `rolesmith sync`): {CORE}\n"
        )
    );

    // Pinned to a commit on a branch off main: ahead of it.
    m.git(&lib.work, &["checkout", "--quiet", "-b", "side"]);
    lib.write("roles/app-store-releaser/core.md", "# Core\n\nSide rule.\n");
    m.git(&lib.work, &["commit", "--quiet", "-am", "side"]);
    m.git(&lib.work, &["push", "--quiet", "origin", "side"]);
    let side = m.git(&lib.work, &["rev-parse", "HEAD"]);
    m.git(
        &m.path("data/rolesmith/sources/felix"),
        &["fetch", "--quiet", "origin", "side"],
    );
    use_role(&m, &project, &["--rev", &side]);
    let role = &m.json(&["status", "--project", p])["roles"][0];
    assert_eq!(
        (&role["behind"], &role["ahead"], &role["drift"]),
        (&json!(0), &json!(1), &json!([]))
    );
}

#[test]
fn use_rev_main_records_the_sha_main_points_to() {
    let (m, lib, first, project) = setup();
    let p = project.to_str().unwrap();
    use_role(&m, &project, &[]);
    lib.write(
        "roles/app-store-releaser/core.md",
        "# Core\n\nSecond rule.\n",
    );
    let second = m.publish(&lib, "second");
    m.json(&["source", "update"]);
    assert_eq!(
        m.json(&["status", "--project", p])["roles"][0]["rev"],
        first.as_str()
    );

    use_role(&m, &project, &["--rev", "main"]);
    let record = std::fs::read_to_string(project.join("rolesmith.toml")).unwrap();
    assert!(record.contains(&format!("rev = \"{second}\"")), "{record}");
    assert!(!record.contains("\"main\""), "{record}");
    let role = &m.json(&["status", "--project", p])["roles"][0];
    assert_eq!(
        (&role["rev"], &role["main"], &role["behind"], &role["ahead"]),
        (&json!(second), &json!(second), &json!(0), &json!(0))
    );
}

#[test]
fn update_moves_the_rev_and_the_files() {
    let (m, lib, first, project) = setup();
    let p = project.to_str().unwrap();
    use_role(&m, &project, &[]);
    lib.write(
        "roles/app-store-releaser/core.md",
        "# Core\n\nSecond rule.\n",
    );
    let second = m.publish(&lib, "second");

    // Before `source update`, the clone's main is still the first rev: nothing moves.
    let out = m.json(&["update", "--project", p]);
    assert_eq!(out["written"], json!([]));
    m.json(&["source", "update"]);

    let out = m.json(&["update", "app-store-releaser", "--project", p]);
    assert_eq!(out["roles"][0]["rev"], second.as_str());
    let skill = ".claude/skills/rolesmith-app-store-releaser-submit-build/SKILL.md";
    assert_eq!(out["written"], json!([skill, CORE]), "both carry the rev");
    let record = std::fs::read_to_string(project.join("rolesmith.toml")).unwrap();
    assert!(record.contains(&format!("rev = \"{second}\"")), "{record}");
    assert!(!record.contains(&first));
    let core = std::fs::read_to_string(project.join(CORE)).unwrap();
    assert!(
        core.contains("Second rule.") && core.contains(&second),
        "{core}"
    );

    let o = m.rolesmith(&["update", "rust-cli", "--project", p]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("records no role rust-cli (it records: app-store-releaser)"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn remove_leaves_the_project_as_it_was_before_use() {
    let (m, _lib, _, project) = setup();
    let p = project.to_str().unwrap();
    // No .gitignore of its own: the one rolesmith makes goes too.
    std::fs::remove_file(project.join(".gitignore")).unwrap();
    let before = snapshot(&project);
    use_role(&m, &project, &[]);
    assert_ne!(snapshot(&project), before);

    let o = m.rolesmith(&["remove", "app-store-releaser", "--project", p]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(
        // CLAUDE.md is written without rolesmith's section, as it was before `use`.
        human.starts_with("no roles in rolesmith.toml\nwrote CLAUDE.md\nremoved ")
            && !human.contains("up to date"),
        "{human}"
    );
    assert_eq!(snapshot(&project), before);
    assert!(!project.join(".rolesmith").exists());
    let skills: Vec<_> = std::fs::read_dir(project.join(".claude/skills"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        skills,
        ["my-own"],
        "the generated skill directories are gone"
    );

    let o = m.rolesmith(&["remove", "app-store-releaser", "--project", p]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("(it records: none)"), "{}", stderr(&o));
}

#[test]
fn source_remove_is_refused_while_a_project_records_it() {
    let (m, _lib, _, project) = setup();
    let p = project.to_str().unwrap();
    use_role(&m, &project, &[]);
    let canonical = project.canonicalize().unwrap();

    let o = m.rolesmith(&["--json", "source", "remove", "felix"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains(&format!(
            "source felix is recorded by 1 record(s); `rolesmith remove` its roles there first:\n  {}: app-store-releaser",
            canonical.display()
        )),
        "{}",
        stderr(&o)
    );
    let doc: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        doc["error"]["projects"],
        json!([{ "project": canonical.display().to_string(), "roles": ["app-store-releaser"] }])
    );
    assert!(m.path("data/rolesmith/sources/felix").is_dir());

    m.json(&["remove", "app-store-releaser", "--project", p]);
    assert_eq!(m.json(&["source", "remove", "felix"])["removed"], "felix");
}

#[test]
fn a_file_rolesmith_cannot_delete_or_read_fails_the_command() {
    use std::os::unix::fs::PermissionsExt;
    let (m, _lib, _, project) = setup();
    let p = project.to_str().unwrap();
    use_role(&m, &project, &[]);

    // The record cannot be deleted from a project directory that is not writable.
    let mode = |m| std::fs::set_permissions(&project, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    let o = m.rolesmith(&["remove", "app-store-releaser", "--project", p]);
    mode(0o755);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("cannot delete"), "{}", stderr(&o));

    // A projects record that cannot be read does not let a source go.
    let projects = m.path("data/rolesmith/projects.toml");
    std::fs::remove_file(&projects).unwrap();
    std::fs::create_dir(&projects).unwrap();
    let o = m.rolesmith(&["source", "remove", "felix"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("cannot read"), "{}", stderr(&o));
    assert!(m.path("data/rolesmith/sources/felix").is_dir());
}

#[test]
fn a_prohibitions_consumer_sub_table_changes_nothing_use_renders_and_show_shows_it() {
    let (m, lib, first, project) = setup();
    let p = project.to_str().unwrap();
    use_role(&m, &project, &[]);
    let before = std::fs::read_to_string(project.join(CORE)).unwrap();

    lib.write(
        "roles/app-store-releaser/prohibitions.toml",
        "[[prohibition]]\nrule = \"Never revoke a certificate\"\nwhy = \"It breaks other machines\"\n\
         [prohibition.guild]\nid = \"no-revoke\"\nguards = \"signing\"\n",
    );
    let second = m.publish(&lib, "guild keys");
    m.json(&["source", "update"]);
    m.json(&["update", "app-store-releaser", "--project", p]);
    let after = std::fs::read_to_string(project.join(CORE)).unwrap();
    assert_eq!(after.replace(&second, &first), before, "only the rev moved");
    assert!(!after.contains("no-revoke"));

    let shown = m.json(&["show", "app-store-releaser"]);
    assert_eq!(
        shown["prohibitions"][0],
        json!({ "rule": "Never revoke a certificate", "why": "It breaks other machines",
                "guild": { "id": "no-revoke", "guards": "signing" } })
    );
}
