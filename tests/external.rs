//! A role uses skills of another repository, pinned by commit: `[[skill]]` in role.toml (SPEC.md
//! §3.4), checked, installed from rolesmith's cache, and listed by `show` and `prompt`, through the
//! built binary. A local bare repository stands in for the remote one.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Machine, stderr};
use serde_json::{Value, json};

/// A `[[skill]]` table.
fn skill(name: &str, url: &str, path: &str, rev: &str) -> String {
    format!("\n[[skill]]\nname = \"{name}\"\nurl = \"{url}\"\npath = \"{path}\"\nrev = \"{rev}\"\n")
}

/// Role `name` in `lib`'s working copy, with `requires`, one skill of its own (`step`) and
/// `extra` appended to its role.toml.
fn role(lib: &common::Library, name: &str, requires: &str, extra: &str) {
    lib.write(
        &format!("roles/{name}/role.toml"),
        &format!("description = \"The {name}\"\nrequires = [{requires}]\n{extra}"),
    );
    lib.write(&format!("roles/{name}/core.md"), &format!("# {name}\n"));
    lib.write(
        &format!("roles/{name}/skills/step/SKILL.md"),
        "---\nname: step\ndescription: Use for a step.\n---\n\nSteps.\n",
    );
}

/// The upstream repository `skills`, published: a good skill with a script beside it, and one
/// whose SKILL.md has no front matter. Answers it and its rev.
fn upstream(m: &Machine) -> (common::Library, String) {
    let up = m.library("skills");
    up.write(
        "skills/engineering/tdd/SKILL.md",
        "---\nname: tdd\ndescription: Use when writing code test first.\n---\n\nRed, green.\n",
    );
    up.write("skills/engineering/tdd/run.sh", "#!/bin/sh\necho red\n");
    std::fs::set_permissions(
        up.work.join("skills/engineering/tdd/run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    up.write("skills/bare/SKILL.md", "no front matter\n");
    let rev = m.publish(&up, "skills");
    (up, rev)
}

fn check(m: &Machine, lib: &Path) -> Vec<Value> {
    let out = m.rolesmith(&["check", "--library", lib.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    doc["error"]["problems"].as_array().unwrap().clone()
}

#[test]
fn check_refuses_a_short_or_unknown_rev_a_path_without_a_skill_and_a_clash() {
    let m = Machine::new();
    let (up, rev) = upstream(&m);
    let url = up.url();
    let lib = m.library("felix");
    lib.write("library.toml", "name = \"felix\"\n");
    let unknown = "0".repeat(40);
    role(
        &lib,
        "coder",
        "",
        &[
            skill("short", url, "skills/engineering/tdd", &rev[..12]),
            skill("unknown", url, "skills/engineering/tdd", &unknown),
            skill("nothing", url, "skills/none", &rev),
            skill("bare", url, "skills/bare", &rev),
            skill("step", url, "skills/engineering/tdd", &rev),
            skill("outside", url, "../tdd", &rev),
        ]
        .concat(),
    );
    let at = format!("{url} at {rev}");
    let problem = |line: usize, rule: &str, reason: String| json!({ "file": "roles/coder/role.toml", "line": line, "rule": rule, "reason": reason });
    assert_eq!(
        check(&m, &lib.work),
        vec![
            problem(
                5,
                "external-skill",
                format!(
                    "skill `short`: rev `{}` is not a full commit SHA (40 lowercase hex digits)",
                    &rev[..12]
                )
            ),
            problem(
                11,
                "external-skill",
                format!("skill `unknown`: {url} has no commit {unknown}")
            ),
            problem(
                17,
                "external-skill",
                format!("skill `nothing`: {at} has no skills/none/SKILL.md")
            ),
            problem(
                23,
                "skill-front-matter",
                format!(
                    "skill `bare`: {at}: skills/bare/SKILL.md: does not begin with YAML front \
                     matter between `---` lines"
                )
            ),
            problem(
                29,
                "skill-name",
                "skill `step`: the role already has a skill of this name".to_owned()
            ),
            problem(
                35,
                "external-skill",
                "skill `outside`: path `../tdd` is not a relative path inside the repository"
                    .to_owned()
            ),
        ]
    );
}

/// A machine with source `felix`: `coder` takes `tdd` from the upstream repository, and
/// `ios-app-coder` requires `coder`; an empty project. Answers the machine, the upstream, its rev,
/// the library's rev and the project.
fn setup() -> (Machine, common::Library, String, String, PathBuf) {
    let m = Machine::new();
    let (up, up_rev) = upstream(&m);
    let lib = m.library("felix");
    lib.write("library.toml", "name = \"felix\"\n");
    let tdd = skill("tdd", up.url(), "skills/engineering/tdd", &up_rev);
    role(&lib, "coder", "", &tdd);
    role(&lib, "ios-app-coder", "\"coder\"", "");
    let rev = m.publish(&lib, "first");
    m.json(&["source", "add", "felix", lib.url()]);
    let project = m.path("project");
    std::fs::create_dir_all(&project).unwrap();
    (m, up, up_rev, rev, project)
}

/// Every file under `dir`, relative to it, with its bytes and whether it is executable.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>, bool)> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(at) = pending.pop() {
        for e in std::fs::read_dir(&at).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                pending.push(p);
            } else {
                let exec = std::fs::metadata(&p).unwrap().permissions().mode() & 0o111 != 0;
                let rel = p.strip_prefix(dir).unwrap().display().to_string();
                out.push((rel, std::fs::read(&p).unwrap(), exec));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_required_role_brings_its_external_skill_identically_without_the_network_once_cached() {
    let (m, up, up_rev, rev, project) = setup();
    let p = project.to_str().unwrap();
    m.json(&[
        "use",
        "ios-app-coder",
        "--into",
        "claude-code",
        "--project",
        p,
    ]);

    let dir = project.join(".claude/skills/rolesmith-coder-tdd");
    let installed = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
    assert_eq!(
        installed,
        format!(
            "---\n# Generated by rolesmith from felix/coder at {rev}, its skill \
             skills/engineering/tdd from {} at {up_rev}; `rolesmith sync` rewrites it, do not \
             edit.\nname: rolesmith-coder-tdd\ndescription: Use when writing code test first.\n\
             ---\n\nRed, green.\n",
            up.url()
        )
    );
    let first = snapshot(&project.join(".claude/skills"));
    assert!(
        first
            .iter()
            .any(|(rel, _, exec)| rel == "rolesmith-coder-tdd/run.sh" && *exec),
        "{:?}",
        first.iter().map(|f| &f.0).collect::<Vec<_>>()
    );

    // The upstream gone, the cache holds the rev: sync, update and a fresh install still write
    // the same bytes.
    std::fs::rename(&up.bare, m.path("gone.git")).unwrap();
    std::fs::remove_dir_all(project.join(".claude")).unwrap();
    m.json(&["sync", "--project", p]);
    assert_eq!(snapshot(&project.join(".claude/skills")), first);
    m.json(&["update", "--project", p]);
    assert_eq!(snapshot(&project.join(".claude/skills")), first);
    let again = m.json(&["sync", "--project", p]);
    assert_eq!(again["written"], json!([]));
}

#[test]
fn show_and_prompt_list_an_external_skill_with_its_url_and_rev() {
    let (m, up, up_rev, _rev, _project) = setup();
    let url = up.url();
    let shown = m.json(&["show", "coder"]);
    assert_eq!(
        shown["skills"],
        json!([
            { "name": "step", "description": "Use for a step." },
            { "name": "tdd", "description": "Use when writing code test first.",
              "url": url, "path": "skills/engineering/tdd", "rev": up_rev },
        ])
    );
    let human = String::from_utf8(m.rolesmith(&["show", "coder"]).stdout).unwrap();
    assert!(
        human.contains(&format!(
            "- tdd: Use when writing code test first. (from {url} at {up_rev}, skills/engineering/tdd)\n"
        )),
        "{human}"
    );
    let prompt = m.json(&["prompt", "ios-app-coder"]);
    let md = prompt["markdown"].as_str().unwrap();
    assert!(
        md.contains(&format!(
            "- tdd: Use when writing code test first. (`skills/engineering/tdd/SKILL.md` in {url} at {up_rev})\n"
        )),
        "{md}"
    );
}

#[test]
fn the_cache_keeps_a_repository_under_its_escaped_url_and_refuses_a_tree_it_cannot_write() {
    let (m, up, up_rev, _rev, _project) = setup();
    let key: String = up
        .url()
        .bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' => char::from(b).to_string(),
            _ => format!("_{b:02x}"),
        })
        .collect();
    let tree = m.path("data/rolesmith/skills").join(key).join(&up_rev);
    std::fs::create_dir_all(tree.parent().unwrap()).unwrap();
    std::fs::write(&tree, "in the way\n").unwrap();
    let out = m.rolesmith(&["show", "coder"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains(&format!("cannot write {}", tree.display())),
        "{}",
        stderr(&out)
    );

    std::fs::remove_file(&tree).unwrap();
    m.json(&["show", "coder"]);
    assert!(tree.join("skills/engineering/tdd/run.sh").is_file());
}
