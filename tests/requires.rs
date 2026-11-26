//! A role builds on other roles of its library: `requires` in role.toml,
//! checked, installed with the role that requires it, and accounted for by `prompt`, `status`,
//! `update` and `remove`, through the built binary.

mod common;

use std::path::{Path, PathBuf};

use common::{Machine, stderr};
use serde_json::{Value, json};

/// Role `name` in `lib`'s working copy, requiring `requires`, with a core and one skill.
fn role(lib: &common::Library, name: &str, requires: &[&str]) {
    let list: Vec<String> = requires.iter().map(|r| format!("\"{r}\"")).collect();
    lib.write(
        &format!("roles/{name}/role.toml"),
        &format!(
            "description = \"The {name}\"\nrequires = [{}]\n",
            list.join(", ")
        ),
    );
    lib.write(
        &format!("roles/{name}/core.md"),
        &format!("# {name}\n\nRule of {name}.\n"),
    );
    lib.write(
        &format!("roles/{name}/skills/step/SKILL.md"),
        &format!("---\nname: step\ndescription: Use for a step of {name}.\n---\n\nSteps.\n"),
    );
}

/// A machine with source `felix`: a chain ios-app-coder -> coder -> base, and a diamond
/// top -> left, right -> shared; published at the returned rev. And an empty project.
fn setup() -> (Machine, common::Library, String, PathBuf) {
    let m = Machine::new();
    let lib = m.library("felix");
    lib.write("library.toml", "name = \"felix\"\n");
    role(&lib, "ios-app-coder", &["coder"]);
    role(&lib, "coder", &["base"]);
    role(&lib, "base", &[]);
    role(&lib, "top", &["left", "right"]);
    role(&lib, "left", &["shared"]);
    role(&lib, "right", &["shared"]);
    role(&lib, "shared", &[]);
    let rev = m.publish(&lib, "first");
    m.json(&["source", "add", "felix", lib.url()]);
    let project = m.path("project");
    std::fs::create_dir_all(&project).unwrap();
    (m, lib, rev, project)
}

/// Every file a role installs into claude-code.
fn files_of(role: &str) -> Vec<String> {
    vec![
        format!(".claude/skills/rolesmith-{role}-step/SKILL.md"),
        format!(".claude/skills/rolesmith-{role}/SKILL.md"),
    ]
}

fn manifest(project: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(project.join(".rolesmith/manifest.json")).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    let mut files: Vec<String> = doc["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap().to_owned())
        .collect();
    files.sort();
    files
}

fn expected(roles: &[&str]) -> Vec<String> {
    let mut files: Vec<String> = roles.iter().flat_map(|r| files_of(r)).collect();
    files.sort();
    files
}

fn recorded(project: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(project.join("rolesmith.toml")).unwrap();
    let doc: toml::Table = toml::from_str(&text).unwrap();
    doc["role"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_owned())
        .collect()
}

fn core(project: &Path, role: &str) -> String {
    std::fs::read_to_string(project.join(format!(".claude/skills/rolesmith-{role}/SKILL.md")))
        .unwrap()
}

fn use_roles(m: &Machine, project: &Path, roles: &[&str]) -> Value {
    let mut args = vec!["use"];
    args.extend_from_slice(roles);
    args.extend_from_slice(&[
        "--into",
        "claude-code",
        "--project",
        project.to_str().unwrap(),
    ]);
    m.json(&args)
}

#[test]
fn check_refuses_an_unknown_required_role_and_a_cycle_naming_them() {
    let m = Machine::new();
    let lib = m.library("felix");
    lib.write("library.toml", "name = \"felix\"\n");
    role(&lib, "lonely", &["nowhere"]);
    role(&lib, "alpha", &["beta"]);
    role(&lib, "beta", &["gamma"]);
    role(&lib, "gamma", &["alpha"]);
    role(&lib, "selfish", &["selfish"]);
    role(&lib, "fine", &["lonely"]);

    let out = m.rolesmith(&["check", "--library", lib.work.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut problems: Vec<Value> = doc["error"]["problems"].as_array().unwrap().clone();
    problems.sort_by_key(|p| p["file"].as_str().unwrap().to_owned());
    assert_eq!(
        problems,
        vec![
            json!({ "file": "roles/alpha/role.toml", "line": 2, "rule": "role-requires",
                    "reason": "requires form a cycle: alpha -> beta -> gamma -> alpha" }),
            json!({ "file": "roles/lonely/role.toml", "line": 2, "rule": "role-requires",
                    "reason": "requires `nowhere`, which is not a role of this library" }),
            json!({ "file": "roles/selfish/role.toml", "line": 2, "rule": "role-requires",
                    "reason": "requires form a cycle: selfish -> selfish" }),
        ]
    );

    // Checking one role checks the roles it requires.
    let out = m.rolesmith(&["check", "fine", "--library", lib.work.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("roles/lonely/role.toml:2: requires `nowhere`, which is not a role of this library [role-requires]"),
        "{}",
        stderr(&out)
    );
    let out = m.rolesmith(&["check", "beta", "--library", lib.work.to_str().unwrap()]);
    assert!(
        stderr(&out).contains("requires form a cycle: alpha -> beta -> gamma -> alpha"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn use_installs_a_chain_of_required_roles_and_records_only_the_named_one() {
    let (m, _lib, rev, project) = setup();
    let out = use_roles(&m, &project, &["ios-app-coder"]);

    assert_eq!(recorded(&project), vec!["ios-app-coder"]);
    assert_eq!(
        manifest(&project),
        expected(&["ios-app-coder", "coder", "base"])
    );
    assert_eq!(
        out["required"],
        json!([
            { "name": "coder", "source": "felix", "rev": rev, "by": ["ios-app-coder"] },
            { "name": "base", "source": "felix", "rev": rev, "by": ["ios-app-coder"] },
        ])
    );

    let ios = core(&project, "ios-app-coder");
    assert!(
        ios.contains(
            "---\n\nThis role builds on others; their core skills are in force too, read them: \
             `rolesmith-coder`, `rolesmith-base`.\n\n# ios-app-coder\n"
        ),
        "{ios}"
    );
    let coder = core(&project, "coder");
    assert!(coder.contains(&format!(
        "# Generated by rolesmith from felix/coder at {rev};"
    )));
    assert!(
        coder.contains("read them: `rolesmith-base`.\n\n# coder\n"),
        "{coder}"
    );
    assert!(!core(&project, "base").contains("builds on"));

    // A second run writes nothing: the generated files are identical on sync.
    let p = project.to_str().unwrap();
    let again = m.json(&["sync", "--project", p]);
    assert_eq!(again["written"], json!([]));
    assert_eq!(again["removed"], json!([]));
    let human = String::from_utf8(m.rolesmith(&["sync", "--project", p]).stdout).unwrap();
    assert_eq!(
        human,
        format!(
            "felix/ios-app-coder at {rev} into claude-code\n\
             felix/coder at {rev}, required by ios-app-coder\n\
             felix/base at {rev}, required by ios-app-coder\nup to date\n"
        )
    );
}

#[test]
fn a_role_required_twice_is_installed_once() {
    let (m, _lib, rev, project) = setup();
    // A diamond: top requires left and right, which both require shared.
    let out = use_roles(&m, &project, &["top"]);
    assert_eq!(recorded(&project), vec!["top"]);
    assert_eq!(
        manifest(&project),
        expected(&["top", "left", "right", "shared"])
    );
    assert_eq!(
        out["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["left", "right", "shared"]
    );
    assert!(
        core(&project, "top").contains("`rolesmith-left`, `rolesmith-right`, `rolesmith-shared`.")
    );

    // Two named roles requiring the same role, one of them also named.
    let out = use_roles(&m, &project, &["left", "right"]);
    assert_eq!(recorded(&project), vec!["top", "left", "right"]);
    assert_eq!(
        out["required"],
        json!([{ "name": "shared", "source": "felix", "rev": rev, "by": ["top", "left", "right"] }])
    );
    assert_eq!(
        manifest(&project),
        expected(&["top", "left", "right", "shared"])
    );
    assert_eq!(out["written"], json!([]));
}

#[test]
fn prompt_includes_the_required_roles() {
    let (m, _lib, rev, _project) = setup();
    let out = m.json(&["prompt", "coder"]);
    assert_eq!(
        out["markdown"].as_str().unwrap(),
        format!(
            "<!-- Generated by rolesmith from felix/coder at {rev}; do not edit. -->\n\n\
             This role builds on base: their core, prohibitions and skills follow its own, and are \
             in force too.\n\n\
             # coder\n\nRule of coder.\n\n## Prohibitions\n\n\n## Skills\n\n\
             When a task matches a skill's description, read its full text: the path is in the \
             source felix at {rev}.\n\n\
             - step: Use for a step of coder. (`roles/coder/skills/step/SKILL.md`)\n\
             \n# Required role felix/base: The base\n\n\
             # base\n\nRule of base.\n\n## Prohibitions\n\n\n## Skills\n\n\
             - step: Use for a step of base. (`roles/base/skills/step/SKILL.md`)\n"
        )
    );
}

#[test]
fn status_and_update_carry_the_required_roles() {
    let (m, lib, first, project) = setup();
    let p = project.to_str().unwrap();
    use_roles(&m, &project, &["ios-app-coder"]);

    let role = &m.json(&["status", "--project", p])["roles"][0];
    assert_eq!(role["requires"], json!(["coder", "base"]));
    assert_eq!(role["drift"], json!([]));
    let human = String::from_utf8(m.rolesmith(&["status", "--project", p]).stdout).unwrap();
    assert_eq!(
        human,
        format!("felix/ios-app-coder at {first}; requires coder, base; at main; files match\n")
    );

    // A hand edit of a required role's file is drift of the role that requires it.
    let edited = project.join(".claude/skills/rolesmith-base/SKILL.md");
    std::fs::write(&edited, "edited\n").unwrap();
    let role = &m.json(&["status", "--project", p])["roles"][0];
    assert_eq!(
        role["drift"],
        json!([".claude/skills/rolesmith-base/SKILL.md"])
    );

    // update moves the required roles with the role, at its new rev.
    lib.write("roles/base/core.md", "# base\n\nSecond rule of base.\n");
    let second = m.publish(&lib, "second");
    m.json(&["source", "update"]);
    let out = m.json(&["update", "--project", p]);
    assert_eq!(out["roles"][0]["rev"], second.as_str());
    assert_eq!(out["required"][1]["rev"], second.as_str());
    let base = core(&project, "base");
    assert!(base.contains("Second rule of base."), "{base}");
    assert!(base.contains(&format!("felix/base at {second};")));
    assert_eq!(recorded(&project), vec!["ios-app-coder"]);

    // A role installed only because another requires it is not the record's to update.
    let out = m.rolesmith(&["update", "coder", "--project", p]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains(
            "rolesmith.toml does not name coder; it is installed because ios-app-coder requires it"
        ),
        "{}",
        stderr(&out)
    );
}

#[test]
fn remove_keeps_a_role_another_remaining_role_requires() {
    let (m, _lib, _rev, project) = setup();
    let p = project.to_str().unwrap();
    use_roles(&m, &project, &["ios-app-coder", "coder"]);
    assert_eq!(recorded(&project), vec!["ios-app-coder", "coder"]);

    // coder is still required by ios-app-coder: its files stay.
    let out = m.json(&["remove", "coder", "--project", p]);
    assert_eq!(out["removed"], json!([]));
    assert_eq!(recorded(&project), vec!["ios-app-coder"]);
    assert_eq!(
        manifest(&project),
        expected(&["ios-app-coder", "coder", "base"])
    );

    // A role only required is refused, naming the role that requires it.
    let out = m.rolesmith(&["remove", "base", "--project", p]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains(
            "rolesmith.toml does not name base; it is installed because ios-app-coder requires it"
        ),
        "{}",
        stderr(&out)
    );

    // With the role that required them gone, they go too.
    let out = m.json(&["remove", "ios-app-coder", "--project", p]);
    let mut removed: Vec<String> = out["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap().to_owned())
        .filter(|r| r.starts_with(".claude/"))
        .collect();
    removed.sort();
    assert_eq!(removed, expected(&["ios-app-coder", "coder", "base"]));
    assert!(!project.join("rolesmith.toml").exists());
    assert!(!project.join(".rolesmith").exists());
}

#[test]
fn a_role_required_at_two_revs_is_refused() {
    let (m, lib, _first, project) = setup();
    let p = project.to_str().unwrap();
    use_roles(&m, &project, &["coder"]);
    lib.write("roles/base/core.md", "# base\n\nSecond rule of base.\n");
    let second = m.publish(&lib, "second");
    m.json(&["source", "update"]);

    // base would be both coder's (at the first rev) and ios-app-coder's (at the second).
    let out = m.rolesmith(&[
        "use",
        "ios-app-coder",
        "--into",
        "claude-code",
        "--project",
        p,
        "--rev",
        &second,
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("a role required at two revs"),
        "{}",
        stderr(&out)
    );
    assert_eq!(recorded(&project), vec!["coder"]);
}
