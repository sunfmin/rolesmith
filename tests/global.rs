//! `--global` installs (SPEC.md §5.2): a role for the user, in `HOME`'s skill directories, recorded
//! in `$XDG_DATA_HOME/rolesmith/global.toml`, and nothing written into the project the command runs
//! in. `HOME` and the XDG directories are temporary ones (tests/common).

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{Machine, published, stderr};
use serde_json::{Value, json};

/// Every file under `dir`, by its path relative to `dir`, with its bytes; none when it is missing.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(at) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in entries.flatten() {
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
const SKILL: &str = ".claude/skills/rolesmith-app-store-releaser-submit-build/SKILL.md";

/// Installs the role for the user.
const USE: &[&str] = &[
    "use",
    "app-store-releaser",
    "--into",
    "claude-code",
    "--global",
];

/// A machine with source `felix` (role app-store-releaser, published at the returned rev), a skill
/// of the user's own in `~/.claude/skills/`, and a project directory to run commands in.
fn setup() -> (Machine, common::Library, String, std::path::PathBuf) {
    let m = Machine::new();
    let (lib, rev) = published(&m, "felix", &["app-store-releaser"]);
    m.json(&["source", "add", "felix", lib.url()]);
    let home = m.path("home");
    std::fs::create_dir_all(home.join(".claude/skills/my-own")).unwrap();
    std::fs::write(home.join(".claude/skills/my-own/SKILL.md"), "mine\n").unwrap();
    let project = m.path("project");
    std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
    std::fs::write(project.join(".gitignore"), "/target\n").unwrap();
    std::fs::write(project.join("CLAUDE.md"), "# Mine\n").unwrap();
    (m, lib, rev, project)
}

#[test]
fn a_global_install_goes_to_home_and_is_recorded_in_global_toml_writing_nothing_into_the_project() {
    let (m, lib, first, project) = setup();
    let home = m.path("home");
    let before = snapshot(&project);

    let out = m.json_in(Some(&project), USE);
    assert_eq!(out["project"], Value::Null);
    assert_eq!(
        out["roles"],
        json!([{ "name": "app-store-releaser", "source": "felix", "url": lib.url(),
                 "rev": first, "into": ["claude-code"] }])
    );
    assert_eq!(out["written"], json!([SKILL, CORE, "global.manifest.json"]));
    let core = std::fs::read_to_string(home.join(CORE)).unwrap();
    assert!(
        core.contains(&format!("from felix/app-store-releaser at {first}")),
        "{core}"
    );
    assert!(home.join(SKILL).is_file());
    assert_eq!(
        std::fs::read_to_string(home.join(".claude/skills/my-own/SKILL.md")).unwrap(),
        "mine\n"
    );
    assert!(
        !home.join(".gitignore").exists(),
        "no .gitignore lines for HOME"
    );

    // The record has rolesmith.toml's fields; the manifest is beside it.
    let data = m.path("data/rolesmith");
    let record = std::fs::read_to_string(data.join("global.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&record).unwrap();
    assert_eq!(parsed["role"][0]["rev"].as_str(), Some(first.as_str()));
    assert_eq!(parsed["role"][0]["url"].as_str(), Some(lib.url()));
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(data.join("global.manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest, json!({ "files": [SKILL, CORE] }));

    // Nothing under the current directory, and no project remembered.
    assert_eq!(snapshot(&project), before);
    assert!(!data.join("projects.toml").exists());

    // Idempotent; sync and status see the same record from anywhere.
    let again = m.json_in(Some(&project), USE);
    assert_eq!(
        (&again["written"], &again["removed"]),
        (&json!([]), &json!([]))
    );
    let o = m.rolesmith(&["sync", "--global"]);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        format!("felix/app-store-releaser at {first} into claude-code\nup to date\n")
    );
    assert_eq!(snapshot(&project), before);
}

#[test]
fn global_status_and_update_follow_the_source_and_leave_the_project_alone() {
    let (m, lib, first, project) = setup();
    let home = m.path("home");
    m.json_in(Some(&project), USE);
    let before = snapshot(&project);

    let out = m.json_in(Some(&project), &["status", "--global"]);
    assert_eq!(
        out,
        json!({ "project": null, "roles": [{ "name": "app-store-releaser", "source": "felix",
                 "rev": first, "main": first, "behind": 0, "ahead": 0, "requires": [], "drift": [] }] })
    );

    lib.write(
        "roles/app-store-releaser/core.md",
        "# Core\n\nSecond rule.\n",
    );
    let second = m.publish(&lib, "second");
    m.json(&["source", "update"]);
    std::fs::write(home.join(CORE), "edited by hand\n").unwrap();
    let role = &m.json_in(Some(&project), &["status", "--global"])["roles"][0];
    assert_eq!(
        (&role["main"], &role["behind"], &role["drift"]),
        (&json!(second), &json!(1), &json!([CORE]))
    );

    let out = m.json_in(
        Some(&project),
        &["update", "app-store-releaser", "--global"],
    );
    assert_eq!(out["roles"][0]["rev"], second.as_str());
    assert_eq!(out["written"], json!([SKILL, CORE]));
    let core = std::fs::read_to_string(home.join(CORE)).unwrap();
    assert!(
        core.contains("Second rule.") && core.contains(&second),
        "{core}"
    );
    let record = std::fs::read_to_string(m.path("data/rolesmith/global.toml")).unwrap();
    assert!(record.contains(&format!("rev = \"{second}\"")), "{record}");
    let role = &m.json_in(Some(&project), &["status", "--global"])["roles"][0];
    assert_eq!((&role["behind"], &role["drift"]), (&json!(0), &json!([])));

    let o = m.rolesmith_in(Some(&project), &["update", "rust-cli", "--global"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o)
            .contains("global.toml records no role rust-cli (it records: app-store-releaser)"),
        "{}",
        stderr(&o)
    );
    assert_eq!(snapshot(&project), before);
    assert!(!project.join("rolesmith.toml").exists());
}

#[test]
fn global_remove_leaves_home_as_it_was_and_source_remove_waits_for_it() {
    let (m, _lib, _, project) = setup();
    let home = m.path("home");
    let data = m.path("data/rolesmith");
    let (home_before, project_before) = (snapshot(&home), snapshot(&project));
    m.json_in(Some(&project), USE);

    let o = m.rolesmith(&["--json", "source", "remove", "felix"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains(
            "source felix is recorded by 1 record(s); `rolesmith remove` its roles there first:\n  --global: app-store-releaser"
        ),
        "{}",
        stderr(&o)
    );
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        doc["error"]["projects"],
        json!([{ "project": null, "roles": ["app-store-releaser"] }])
    );
    assert!(data.join("sources/felix").is_dir());

    let o = m.rolesmith_in(
        Some(&project),
        &["remove", "app-store-releaser", "--global"],
        "",
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(
        human.starts_with("no roles in global.toml\nremoved "),
        "{human}"
    );
    assert_eq!(snapshot(&home), home_before);
    assert!(!data.join("global.toml").exists());
    assert!(!data.join("global.manifest.json").exists());
    assert_eq!(snapshot(&project), project_before);

    assert_eq!(m.json(&["source", "remove", "felix"])["removed"], "felix");
}

#[test]
fn global_takes_the_place_of_project_and_of_auto() {
    let (m, _lib, _, project) = setup();
    let p = project.to_str().unwrap();
    for args in [
        &["status", "--global", "--project", p][..],
        &[
            "use",
            "app-store-releaser",
            "--into",
            "claude-code",
            "--global",
            "--project",
            p,
        ],
        &["use", "--auto", "--into", "claude-code", "--global"],
    ] {
        let o = m.rolesmith(args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", stderr(&o));
    }
    assert!(!m.path("data/rolesmith/global.toml").exists());
}

#[test]
fn a_global_install_does_not_overwrite_a_skill_rolesmith_did_not_generate() {
    let (m, _lib, _, project) = setup();
    let home = m.path("home");
    std::fs::create_dir_all(home.join(CORE).parent().unwrap()).unwrap();
    std::fs::write(home.join(CORE), "the user's own\n").unwrap();
    let o = m.rolesmith_in(Some(&project), USE, "");
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("will not overwrite"), "{}", stderr(&o));
    assert_eq!(
        std::fs::read_to_string(home.join(CORE)).unwrap(),
        "the user's own\n"
    );
    assert!(
        !m.path("data/rolesmith/global.toml").exists(),
        "the record is put back"
    );
}
