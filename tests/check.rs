//! `rolesmith check` (SPEC.md §2, §3, §7.2) on temporary libraries, through the built binary.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn check(library: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rolesmith"))
        .arg("check")
        .arg("--library")
        .arg(library)
        .args(extra)
        .output()
        .expect("the binary runs")
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A library that passes: one role with a skill, prohibitions, a consumer table and a program
/// whose tool is pinned.
fn sound() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "library.toml",
        "name = \"felix\"\ndescription = \"Felix's roles\"\n",
    );
    write(
        root,
        "roles/app-store-releaser/role.toml",
        r#"description = "Ships iOS and macOS apps to the App Store"
programs = ["asc"]
services = ["asc-api-key"]

[match]
files = ["*.xcodeproj"]
contains = [{ file = "Package.swift", text = ".iOS(" }]

[guild]
engine = { preferred = "claude-code", fallback = ["codex"] }
"#,
    );
    write(
        root,
        "roles/app-store-releaser/core.md",
        "# Core\n\nRead the key with mytokens; never paste a token.\n",
    );
    write(
        root,
        "roles/app-store-releaser/prohibitions.toml",
        "[[prohibition]]\nrule = \"Never revoke a signing certificate\"\nwhy = \"It breaks other machines\"\n",
    );
    write(
        root,
        "roles/app-store-releaser/skills/submit-build/SKILL.md",
        "---\nname: submit-build\ndescription: Use when a build is ready for review.\n---\n\nSteps.\n",
    );
    write(
        root,
        "tools/asc/tool.toml",
        r#"description = "App Store Connect CLI"
check = "asc --version"
[install]
brew = "asc"
version = "7.0.0"
[credential]
mytokens = "asc-api-key"
env = { ASC_KEY_ID = "Key ID", ASC_PRIVATE_KEY_B64 = "Key (base64 .p8)" }
"#,
    );
    dir
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The problems of a `--json` run that refused, as (file, line, rule).
fn problems(root: &Path) -> Vec<(String, Option<u64>, String)> {
    let o = check(root, &["--json"]);
    assert_eq!(o.status.code(), Some(1), "refused: {}", stderr(&o));
    let doc: Value = serde_json::from_str(&stdout(&o)).expect("one JSON document");
    assert_eq!(doc["error"]["kind"], "failed");
    doc["error"]["problems"]
        .as_array()
        .expect("error.problems is a list")
        .iter()
        .map(|p| {
            assert!(p["reason"].as_str().is_some_and(|r| !r.is_empty()));
            (
                p["file"].as_str().unwrap().to_owned(),
                p["line"].as_u64(),
                p["rule"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn has(problems: &[(String, Option<u64>, String)], file: &str, rule: &str) -> bool {
    problems.iter().any(|(f, _, r)| f == file && r == rule)
}

#[test]
fn a_sound_library_passes() {
    let lib = sound();
    let o = check(lib.path(), &[]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(o.stderr.is_empty());
    assert!(stdout(&o).contains("no problems"));

    let o = check(lib.path(), &["--json"]);
    assert_eq!(o.status.code(), Some(0));
    let doc: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(doc["library"], "felix");
    assert_eq!(doc["roles"], serde_json::json!(["app-store-releaser"]));
    assert_eq!(doc["tools"], serde_json::json!(["asc"]));
}

#[test]
fn one_role_can_be_checked() {
    let lib = sound();
    write(lib.path(), "roles/broken/role.toml", "nonsense");
    let o = check(lib.path(), &["app-store-releaser"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));

    assert!(has(
        &problems(lib.path()),
        "roles/broken/role.toml",
        "role-toml"
    ));

    let o = check(lib.path(), &["no-such-role", "--json"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("role-missing"));
}

/// Break one rule in a sound library; check names the file and the rule.
fn broken(rel: &str, text: &str, file: &str, rule: &str) {
    let lib = sound();
    write(lib.path(), rel, text);
    let found = problems(lib.path());
    assert!(
        has(&found, file, rule),
        "{rule} in {file} not reported; got {found:?}"
    );
}

#[test]
fn library_toml_needs_a_name() {
    broken(
        "library.toml",
        "description = \"x\"\n",
        "library.toml",
        "library-toml",
    );
    let lib = sound();
    std::fs::remove_file(lib.path().join("library.toml")).unwrap();
    assert!(has(&problems(lib.path()), "library.toml", "library-toml"));
}

#[test]
fn a_role_name_follows_the_rule() {
    broken(
        "roles/Bad_Name/role.toml",
        "description = \"x\"\n",
        "roles/Bad_Name",
        "role-name",
    );
}

#[test]
fn role_toml_needs_a_short_one_line_description() {
    broken(
        "roles/app-store-releaser/role.toml",
        "programs = []\n",
        "roles/app-store-releaser/role.toml",
        "role-toml",
    );
    broken(
        "roles/app-store-releaser/role.toml",
        &format!("description = \"{}\"\n", "d".repeat(201)),
        "roles/app-store-releaser/role.toml",
        "role-description",
    );
}

#[test]
fn an_unknown_top_level_key_is_refused_but_a_consumer_table_is_kept() {
    let lib = sound();
    write(
        lib.path(),
        "roles/app-store-releaser/role.toml",
        "description = \"x\"\nprograms = [\"asc\"]\nengine = \"codex\"\n[guild]\nx = 1\n[other]\ny = 2\n",
    );
    let found = problems(lib.path());
    let unknown: Vec<_> = found
        .iter()
        .filter(|(_, _, r)| r == "role-unknown-key")
        .collect();
    assert_eq!(
        unknown.len(),
        2,
        "engine and [other], not [guild]: {found:?}"
    );
    assert!(unknown.iter().any(|(_, line, _)| *line == Some(3)));
    assert!(unknown.iter().any(|(_, line, _)| *line == Some(6)));
}

#[test]
fn core_md_is_at_most_8000_characters() {
    broken(
        "roles/app-store-releaser/core.md",
        &"é".repeat(8_001),
        "roles/app-store-releaser/core.md",
        "core",
    );
    // characters, not bytes
    let lib = sound();
    write(
        lib.path(),
        "roles/app-store-releaser/core.md",
        &"é".repeat(8_000),
    );
    assert_eq!(check(lib.path(), &[]).status.code(), Some(0));
}

#[test]
fn prohibitions_toml_must_parse() {
    let lib = sound();
    write(
        lib.path(),
        "roles/app-store-releaser/prohibitions.toml",
        "[[prohibition]]\nrule = \"x\"\nwhy = \n",
    );
    let found = problems(lib.path());
    assert!(found.contains(&(
        "roles/app-store-releaser/prohibitions.toml".into(),
        Some(3),
        "prohibitions".into()
    )));
}

#[test]
fn a_prohibition_keeps_a_consumer_sub_table_and_refuses_any_other_key() {
    let lib = sound();
    let file = "roles/app-store-releaser/prohibitions.toml";
    write(
        lib.path(),
        file,
        "[[prohibition]]\nrule = \"Never spend money\"\nwhy = \"Felix approves\"\n\
         [prohibition.guild]\nid = \"money\"\nguards = \"money\"\non = \"PreToolUse\"\nmatch = \"pay\"\n\
         [[prohibition]]\nrule = \"x\"\nwhy = \"y\"\n[prohibition.guild]\nid = \"x\"\n",
    );
    let o = check(lib.path(), &[]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));

    write(
        lib.path(),
        file,
        "[[prohibition]]\nrule = \"x\"\nwhy = \"y\"\n[prohibition.guild]\nid = \"x\"\n\
         [[prohibition]]\nrule = \"x\"\nwhy = \"y\"\nseverity = \"high\"\n[prohibition.other]\nz = 1\n",
    );
    let found = problems(lib.path());
    let mut lines: Vec<_> = found
        .iter()
        .filter(|(f, _, r)| f == file && r == "prohibitions")
        .map(|(_, line, _)| *line)
        .collect();
    lines.sort();
    assert_eq!(
        lines,
        [Some(9), Some(10)],
        "severity and [other], not [guild]: {found:?}"
    );

    // a consumer's key is a sub-table, not a value
    write(
        lib.path(),
        file,
        "[[prohibition]]\nrule = \"x\"\nwhy = \"y\"\nguild = 1\n",
    );
    assert!(has(&problems(lib.path()), file, "prohibitions"));
}

#[test]
fn a_skill_names_its_directory_and_says_when_to_use_it() {
    let skill = "roles/app-store-releaser/skills/submit-build/SKILL.md";
    broken(
        skill,
        "---\nname: other\ndescription: d\n---\n",
        skill,
        "skill-front-matter",
    );
    broken(skill, "no front matter\n", skill, "skill-front-matter");
    broken(
        skill,
        "---\nname: submit-build\n---\n",
        skill,
        "skill-description",
    );
    broken(
        skill,
        &format!(
            "---\nname: submit-build\ndescription: {}\n---\n",
            "w".repeat(1_025)
        ),
        skill,
        "skill-description",
    );
    broken(
        "roles/app-store-releaser/skills/Bad/SKILL.md",
        "---\nname: Bad\ndescription: d\n---\n",
        "roles/app-store-releaser/skills/Bad",
        "skill-name",
    );
}

#[test]
fn every_program_has_a_pinned_tool() {
    let lib = sound();
    std::fs::remove_dir_all(lib.path().join("tools/asc")).unwrap();
    assert!(has(
        &problems(lib.path()),
        "roles/app-store-releaser/role.toml",
        "tool-missing"
    ));

    broken(
        "tools/asc/tool.toml",
        "description = \"d\"\ncheck = \"asc --version\"\n[install]\nbrew = \"asc\"\n",
        "tools/asc/tool.toml",
        "tool-unpinned",
    );
    broken(
        "tools/asc/tool.toml",
        "description = \"d\"\ncheck = \"asc\"\n[install]\ncargo = \"asc\"\nversion = \"1\"\n",
        "tools/asc/tool.toml",
        "tool-toml",
    );
}

#[test]
fn several_broken_rules_are_all_reported() {
    let lib = sound();
    let root = lib.path();
    write(root, "roles/app-store-releaser/core.md", &"x".repeat(8_001));
    write(
        root,
        "roles/app-store-releaser/skills/submit-build/SKILL.md",
        "---\nname: wrong\ndescription: d\n---\n",
    );
    write(
        root,
        "tools/asc/tool.toml",
        "description = \"d\"\ncheck = \"c\"\n[install]\nbrew = \"asc\"\n",
    );
    write(root, "roles/x/role.toml", "description = \"x\"\n");

    let found = problems(root);
    for (file, rule) in [
        ("roles/app-store-releaser/core.md", "core"),
        (
            "roles/app-store-releaser/skills/submit-build/SKILL.md",
            "skill-front-matter",
        ),
        ("tools/asc/tool.toml", "tool-unpinned"),
        ("roles/x", "role-name"),
        ("roles/x/core.md", "core"),
    ] {
        assert!(has(&found, file, rule), "{rule} in {file}: {found:?}");
    }

    let o = check(root, &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty(), "human problems go to stderr");
    let err = stderr(&o);
    assert!(err.contains("roles/app-store-releaser/core.md"));
    assert!(err.contains("tools/asc/tool.toml"));
}

#[test]
fn a_planted_key_is_refused_and_never_echoed() {
    let token = format!("ghp_{}", "Zq9".repeat(12));
    let pem_body = "MIIEowIBAAKCAQEA7bq5tW1Xh0Lq2yqQk8mN3pLs9vF4hJdQ";
    let lib = sound();
    let root = lib.path();
    write(
        root,
        "roles/app-store-releaser/skills/submit-build/notes.md",
        &format!("Use this:\n{token}\n"),
    );
    write(
        root,
        "roles/app-store-releaser/core.md",
        &format!(
            "# Core\n-----BEGIN RSA PRIVATE KEY-----\n{pem_body}\n-----END RSA PRIVATE KEY-----\n"
        ),
    );
    // even a key name carrying the token is not echoed
    write(
        root,
        "roles/app-store-releaser/role.toml",
        &format!("description = \"x\"\n{token} = 1\n"),
    );

    for extra in [&[][..], &["--json"][..]] {
        let o = check(root, extra);
        assert_eq!(o.status.code(), Some(1));
        let all = format!("{}{}", stdout(&o), stderr(&o));
        assert!(!all.contains(&token), "token echoed: {all}");
        assert!(!all.contains("Zq9Zq9"), "part of the token echoed");
        assert!(!all.contains(pem_body), "key echoed");
    }

    let found = problems(root);
    assert!(found.contains(&(
        "roles/app-store-releaser/skills/submit-build/notes.md".into(),
        Some(2),
        "secret".into()
    )));
    assert!(found.contains(&(
        "roles/app-store-releaser/core.md".into(),
        Some(2),
        "secret".into()
    )));
    assert!(has(&found, "roles/app-store-releaser/role.toml", "secret"));
}

#[test]
fn the_git_directory_is_not_scanned() {
    let lib = sound();
    write(
        lib.path(),
        ".git/config",
        &format!("token ghp_{}\n", "a".repeat(36)),
    );
    assert_eq!(check(lib.path(), &[]).status.code(), Some(0));
}

#[test]
fn a_missing_tool_is_reported_even_when_its_role_is_broken_otherwise() {
    let lib = sound();
    std::fs::remove_file(lib.path().join("roles/app-store-releaser/core.md")).unwrap();
    std::fs::remove_dir_all(lib.path().join("tools/asc")).unwrap();
    let found = problems(lib.path());
    assert!(has(&found, "roles/app-store-releaser/core.md", "core"));
    assert!(has(
        &found,
        "roles/app-store-releaser/role.toml",
        "tool-missing"
    ));
}
