//! Sources, `list` and `show` (SPEC.md §2, §8) through the built binary, with local bare git
//! repositories as libraries; nothing reaches the network.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{Machine, published, stderr};

#[test]
fn add_list_update_and_remove_a_source() {
    let m = Machine::new();
    let (lib, first) = published(&m, "felix", &["app-store-releaser", "rust-cli"]);

    let added = m.json(&["source", "add", "felix", lib.url()]);
    assert_eq!(added["added"]["main"], first.as_str());
    assert!(m.path("config/rolesmith/sources.toml").is_file());
    assert!(
        m.path("data/rolesmith/sources/felix/library.toml")
            .is_file()
    );

    let listed = m.json(&["source", "list"]);
    let sources = listed["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["name"], "felix");
    assert_eq!(sources[0]["url"], lib.url());
    assert_eq!(sources[0]["main"], first.as_str());
    assert_eq!(sources[0]["roles"], 2);

    let human = m.rolesmith(&["source", "list"]);
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("felix") && text.contains(&first) && text.contains("2 role(s)"));

    // A new commit upstream: update moves the clone's main to it, and list sees the new role.
    lib.role("ios-ui-tester", "Tests iOS UIs", "# Core\n");
    let second = m.publish(&lib, "a third role");
    let updated = m.json(&["source", "update"]);
    assert_eq!(updated["updated"][0]["from"], first.as_str());
    assert_eq!(updated["updated"][0]["to"], second.as_str());
    let listed = m.json(&["source", "list"]);
    assert_eq!(listed["sources"][0]["main"], second.as_str());
    assert_eq!(listed["sources"][0]["roles"], 3);

    lib.write("roles/rust-cli/core.md", "# Core\n\nAnother rule.\n");
    m.publish(&lib, "another rule");
    let moved = String::from_utf8(m.rolesmith(&["source", "update"]).stdout).unwrap();
    assert!(
        moved.contains(" -> ") && !moved.contains("up to date"),
        "{moved}"
    );
    let still = String::from_utf8(m.rolesmith(&["source", "update"]).stdout).unwrap();
    assert!(
        still.contains("up to date") && !still.contains(" -> "),
        "{still}"
    );

    // Up to date: nothing moves.
    let again = m.json(&["source", "update", "felix"]);
    assert_eq!(again["updated"][0]["from"], again["updated"][0]["to"]);

    let removed = m.json(&["source", "remove", "felix"]);
    assert_eq!(removed["removed"], "felix");
    assert!(!m.path("data/rolesmith/sources/felix").exists());
    assert_eq!(
        m.json(&["source", "list"])["sources"],
        serde_json::json!([])
    );
}

#[test]
fn add_refuses_a_name_in_use_and_a_repo_without_library_toml() {
    let m = Machine::new();
    let (lib, _) = published(&m, "felix", &["rust-cli"]);
    m.json(&["source", "add", "felix", lib.url()]);

    let o = m.rolesmith(&["source", "add", "felix", lib.url()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("already recorded"), "{}", stderr(&o));

    let not_a_library = m.library("plain");
    not_a_library.write("README.md", "not a library\n");
    m.publish(&not_a_library, "readme");
    let o = m.rolesmith(&["source", "add", "plain", not_a_library.url()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("no library.toml"), "{}", stderr(&o));
    assert!(!m.path("data/rolesmith/sources/plain").exists());
    assert_eq!(
        m.json(&["source", "list"])["sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let o = m.rolesmith(&["source", "add", "Bad_Name", lib.url()]);
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn update_never_forces_a_main_that_moved_apart() {
    let m = Machine::new();
    let (lib, first) = published(&m, "felix", &["rust-cli"]);
    m.json(&["source", "add", "felix", lib.url()]);

    // Upstream rewrites main.
    lib.write(
        "library.toml",
        "name = \"felix\"\ndescription = \"rewritten\"\n",
    );
    m.git(&lib.work, &["add", "-A"]);
    m.git(
        &lib.work,
        &["commit", "--quiet", "--amend", "-m", "rewritten"],
    );
    m.git(&lib.work, &["push", "--quiet", "--force", "origin", "main"]);

    let o = m.rolesmith(&["source", "update"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    let clone = m.path("data/rolesmith/sources/felix");
    assert_eq!(m.git(&clone, &["rev-parse", "main"]), first);
}

#[test]
fn a_bare_name_two_sources_have_is_refused_listing_both_addresses() {
    let m = Machine::new();
    let (felix, _) = published(&m, "felix", &["rust-cli", "app-store-releaser"]);
    let (team, _) = published(&m, "team", &["rust-cli"]);
    m.json(&["source", "add", "felix", felix.url()]);
    m.json(&["source", "add", "team", team.url()]);

    let o = m.rolesmith(&["show", "rust-cli"]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(
        err.contains("felix/rust-cli") && err.contains("team/rust-cli"),
        "{err}"
    );

    let shown = m.json(&["show", "team/rust-cli"]);
    assert_eq!(shown["source"], "team");
    assert_eq!(shown["description"], "The rust-cli of team");

    // The name only one source has is addressed bare.
    assert_eq!(m.json(&["show", "app-store-releaser"])["source"], "felix");

    let listed = m.json(&["list"]);
    let addresses: Vec<&str> = listed["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["address"].as_str().unwrap())
        .collect();
    assert_eq!(
        addresses,
        ["app-store-releaser", "felix/rust-cli", "team/rust-cli"]
    );
    let only_team = m.json(&["list", "--source", "team"]);
    assert_eq!(only_team["roles"].as_array().unwrap().len(), 1);
    assert_eq!(only_team["roles"][0]["address"], "team/rust-cli");
    assert_eq!(only_team["roles"][0]["description"], "The rust-cli of team");

    let o = m.rolesmith(&["show", "nobody"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("no source has a role nobody"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn show_prints_the_role_and_reads_it_at_a_rev() {
    let m = Machine::new();
    let (lib, first) = published(&m, "felix", &["app-store-releaser"]);
    m.json(&["source", "add", "felix", lib.url()]);

    lib.write(
        "roles/app-store-releaser/core.md",
        "# Core\n\nSecond rule.\n",
    );
    let second = m.publish(&lib, "a new rule");
    m.json(&["source", "update"]);

    let now = m.json(&["show", "app-store-releaser"]);
    assert_eq!(now["rev"], second.as_str());
    assert!(now["core"].as_str().unwrap().contains("Second rule"));
    assert!(
        now["role_toml"]
            .as_str()
            .unwrap()
            .contains("programs = [\"asc\"]")
    );
    assert_eq!(now["prohibitions"][0]["rule"], "Never revoke a certificate");
    assert_eq!(now["skills"][0]["name"], "submit-build");
    assert_eq!(
        now["skills"][0]["description"],
        "Use when a build is ready for review."
    );
    assert_eq!(now["programs"][0]["name"], "asc");
    assert_eq!(now["programs"][0]["description"], "App Store Connect CLI");
    assert_eq!(now["services"], serde_json::json!(["asc-api-key"]));

    let then = m.json(&["show", "felix/app-store-releaser", "--rev", &first]);
    assert_eq!(then["rev"], first.as_str());
    assert!(then["core"].as_str().unwrap().contains("First rule"));
    let short = m.json(&["show", "app-store-releaser", "--rev", &first[..12]]);
    assert_eq!(short["rev"], first.as_str());

    let human = m.rolesmith(&["show", "app-store-releaser"]);
    let text = String::from_utf8(human.stdout).unwrap();
    for part in [
        "## role.toml",
        "Second rule",
        "Never revoke a certificate",
        "submit-build: Use when",
        "asc: App Store Connect CLI",
        "asc-api-key",
    ] {
        assert!(text.contains(part), "{part} in:\n{text}");
    }

    // A branch name is not a SHA, though git would resolve it.
    let o = m.rolesmith(&["show", "app-store-releaser", "--rev", "main"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("not a commit SHA"), "{}", stderr(&o));

    for bad in ["--not-a-sha", "0000000000000000000000000000000000000000"] {
        let o = m.rolesmith(&["show", "app-store-releaser", "--rev", bad]);
        assert_ne!(o.status.code(), Some(0), "{bad}");
    }
}

#[test]
fn a_relative_xdg_directory_falls_back_to_home_and_an_unreadable_record_fails() {
    let m = Machine::new();
    let (lib, _) = published(&m, "felix", &["rust-cli"]);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
    m.env(&mut cmd);
    let o = cmd
        .env("XDG_CONFIG_HOME", "relative/config")
        .args(["source", "add", "felix", lib.url()])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(m.path("home/.config/rolesmith/sources.toml").is_file());

    // A record that cannot be read is a failure, not an empty list.
    std::fs::create_dir_all(m.path("config/rolesmith/sources.toml")).unwrap();
    let o = m.rolesmith(&["source", "list"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("cannot read"), "{}", stderr(&o));
}

#[test]
fn new_branches_from_main_scaffolds_the_role_and_prints_the_brief() {
    let m = Machine::new();
    let (lib, main) = published(&m, "felix", &["rust-cli"]);
    m.json(&["source", "add", "felix", lib.url()]);
    let clone = m.path("data/rolesmith/sources/felix");
    let need = "Ship iOS apps to the App Store, with \"quotes\" and {{role}}";

    let o = m.rolesmith(&["new", "ios-releaser", "--need", need]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let brief = String::from_utf8(o.stdout).unwrap();
    let template = include_str!("../src/briefs/new.md");
    let (before_need, after_need) = template.split_once("{need}").unwrap();
    assert!(
        brief.contains(&format!("## The need\n\n{need}\n")),
        "{brief}"
    );
    assert!(brief.starts_with("# Draft the role `ios-releaser`\n"));
    assert!(brief.contains("rolesmith check ios-releaser\n    rolesmith propose ios-releaser"));
    assert!(brief.contains(clone.to_str().unwrap()));
    assert!(brief.contains("at most 8000 characters"), "{brief}");
    assert_eq!(
        brief.lines().count(),
        before_need.lines().count() + after_need.lines().count(),
        "the brief is the template, filled in"
    );
    assert!(
        !brief.replace(need, "").contains('{'),
        "a blank left: {brief}"
    );

    assert_eq!(
        m.git(&clone, &["symbolic-ref", "--short", "HEAD"]),
        "rolesmith/new-ios-releaser"
    );
    assert_eq!(m.git(&clone, &["rev-parse", "HEAD"]), main);
    let role = clone.join("roles/ios-releaser");
    let role_toml = std::fs::read_to_string(role.join("role.toml")).unwrap();
    let parsed: toml::Table = toml::from_str(&role_toml).unwrap();
    assert_eq!(parsed["description"].as_str(), Some(need));
    assert_eq!(parsed.len(), 1, "{role_toml}");
    assert_eq!(std::fs::read_to_string(role.join("core.md")).unwrap(), "");
    assert_eq!(
        std::fs::read_to_string(role.join("prohibitions.toml")).unwrap(),
        ""
    );
    assert!(!role.join("skills").exists());

    // The scaffold is what `check` accepts as a role, the need standing in as its description.
    let o = m.rolesmith(&[
        "check",
        "ios-releaser",
        "--library",
        clone.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));

    std::fs::remove_dir_all(&role).unwrap();
    let doc = m.json(&[
        "new",
        "web-tester",
        "--need",
        "Test web UIs",
        "--source",
        "felix",
    ]);
    assert_eq!(doc["source"], "felix");
    assert_eq!(doc["branch"], "rolesmith/new-web-tester");
    assert!(doc["brief"].as_str().unwrap().contains("Test web UIs"));

    // What --json printed is what agent-context says `new` prints.
    let context = m.json(&["agent-context"]);
    let new = context["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "new")
        .unwrap();
    let required = new["output"]["required"].as_array().unwrap();
    for key in required {
        assert!(doc.get(key.as_str().unwrap()).is_some(), "{key}");
    }
    assert_eq!(required.len(), doc.as_object().unwrap().len());
}

#[test]
fn new_refuses_a_dirty_working_copy_an_existing_role_and_a_taken_branch() {
    let m = Machine::new();
    let refused = |args: &[&str], why: &str| {
        let o = m.rolesmith(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", stderr(&o));
        assert!(stderr(&o).contains(why), "{args:?}: {}", stderr(&o));
    };
    refused(&["new", "web", "--need", "x"], "no sources; add one");
    let (lib, _) = published(&m, "felix", &["rust-cli"]);
    m.json(&["source", "add", "felix", lib.url()]);
    let clone = m.path("data/rolesmith/sources/felix");

    refused(
        &["new", "rust-cli", "--need", "x"],
        "already has a role rust-cli",
    );
    refused(&["new", "Bad_Name", "--need", "x"], "not a valid role name");
    refused(
        &["new", "web", "--need", "x", "--source", "nope"],
        "no source named nope",
    );

    std::fs::write(clone.join("stray.txt"), "half done").unwrap();
    refused(&["new", "web", "--need", "x"], "uncommitted changes");
    assert_eq!(m.git(&clone, &["symbolic-ref", "--short", "HEAD"]), "main");
    assert!(!clone.join("roles/web").exists());
    std::fs::remove_file(clone.join("stray.txt")).unwrap();

    m.json(&["new", "web", "--need", "x"]);
    m.git(&clone, &["checkout", "--quiet", "-f", "main"]);
    std::fs::remove_dir_all(clone.join("roles/web")).unwrap();
    refused(
        &["new", "web", "--need", "x"],
        "rolesmith/new-web' already exists",
    );

    let (other, _) = published(&m, "other", &[]);
    m.json(&["source", "add", "other", other.url()]);
    refused(&["new", "api", "--need", "x"], "more than one source");
}

/// Every file under `dir` with its contents, to see that a command wrote nothing.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                todo.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                files.push((path, bytes));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn prompt_prints_the_role_as_one_document_and_writes_nothing() {
    let m = Machine::new();
    let (lib, _) = published(&m, "felix", &["app-store-releaser"]);
    lib.write(
        "roles/app-store-releaser/skills/notarize/SKILL.md",
        "---\nname: notarize\ndescription: Use when a macOS build needs notarizing.\n---\n\nSteps.\n",
    );
    let rev = m.publish(&lib, "a second skill");
    m.json(&["source", "add", "felix", lib.url()]);
    let project = m.path("project");
    std::fs::create_dir_all(&project).unwrap();

    let before = snapshot(m.dir.path());
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
    cmd.args(["prompt", "app-store-releaser"])
        .current_dir(&project);
    m.env(&mut cmd);
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(snapshot(m.dir.path()), before, "prompt wrote a file");

    let golden = include_str!("golden/prompt.md").replace("<rev>", &rev);
    assert_eq!(String::from_utf8(o.stdout).unwrap(), golden);

    let doc = m.json(&["prompt", "felix/app-store-releaser", "--rev", &rev[..12]]);
    assert_eq!(
        doc,
        serde_json::json!({ "role": "felix/app-store-releaser", "rev": rev, "markdown": golden })
    );
    let shapes = rolesmith::commands::output_shapes();
    let (_, shape) = shapes.iter().find(|(n, _)| *n == "prompt").unwrap();
    assert_eq!(
        shape["required"],
        serde_json::json!(["role", "rev", "markdown"])
    );
}

#[test]
fn list_and_show_never_read_the_working_tree_and_show_refuses_what_check_refuses() {
    let m = Machine::new();
    let (lib, first) = published(&m, "felix", &["rust-cli"]);
    m.json(&["source", "add", "felix", lib.url()]);

    // The clone's working tree changes, uncommitted: a new core and a new role.
    let clone = m.path("data/rolesmith/sources/felix");
    std::fs::write(clone.join("roles/rust-cli/core.md"), "# Core\n\nEdited.\n").unwrap();
    std::fs::create_dir_all(clone.join("roles/web")).unwrap();
    std::fs::write(clone.join("roles/web/role.toml"), "description = \"Web\"\n").unwrap();
    std::fs::write(clone.join("roles/web/core.md"), "# Core\n").unwrap();

    let shown = m.json(&["show", "rust-cli"]);
    assert_eq!(shown["rev"], first.as_str());
    assert!(shown["core"].as_str().unwrap().contains("First rule"));
    let listed = m.json(&["list"]);
    let roles = listed["roles"].as_array().unwrap();
    assert_eq!(roles.len(), 1, "{listed}");
    assert_eq!(roles[0]["address"], "rust-cli");
    assert_ne!(m.rolesmith(&["show", "web"]).status.code(), Some(0));
    m.git(&clone, &["checkout", "--quiet", "--", "."]);
    m.git(&clone, &["clean", "--quiet", "-fd"]);

    // A commit whose role `check` refuses: `show` refuses it, with the same problems.
    lib.write("roles/rust-cli/core.md", &"x".repeat(8001));
    let broken = m.publish(&lib, "too long a core");
    m.json(&["source", "update"]);
    let o = m.rolesmith(&["--json", "show", "rust-cli"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    let shown: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let mut check = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
    m.env(&mut check);
    let o = check
        .args(["--json", "check", "rust-cli"])
        .current_dir(&lib.work)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    let checked: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(
        !checked["error"]["problems"].as_array().unwrap().is_empty(),
        "{checked}"
    );
    assert_eq!(
        shown["error"]["problems"], checked["error"]["problems"],
        "show at {broken}"
    );
}

#[test]
fn update_fast_forwards_main_on_whichever_branch_the_clone_is() {
    let m = Machine::new();
    let (lib, first) = published(&m, "felix", &["rust-cli"]);
    m.json(&["source", "add", "felix", lib.url()]);
    let clone = m.path("data/rolesmith/sources/felix");
    m.git(
        &clone,
        &["checkout", "--quiet", "-b", "rolesmith/distill-rust-cli"],
    );

    lib.write("roles/rust-cli/core.md", "# Core\n\nSecond rule.\n");
    let second = m.publish(&lib, "a second rule");
    let updated = m.json(&["source", "update"]);
    assert_eq!(updated["updated"][0]["from"], first.as_str());
    assert_eq!(updated["updated"][0]["to"], second.as_str());
    assert_eq!(m.git(&clone, &["rev-parse", "main"]), second);
    assert_eq!(
        m.git(&clone, &["symbolic-ref", "--short", "HEAD"]),
        "rolesmith/distill-rust-cli"
    );
    assert_eq!(
        m.git(&clone, &["rev-parse", "HEAD"]),
        first,
        "the branch is left alone"
    );

    // Upstream rewrites main: refused off main too, and the local main stays.
    lib.write(
        "library.toml",
        "name = \"felix\"\ndescription = \"rewritten\"\n",
    );
    m.git(&lib.work, &["add", "-A"]);
    m.git(
        &lib.work,
        &["commit", "--quiet", "--amend", "-m", "rewritten"],
    );
    m.git(&lib.work, &["push", "--quiet", "--force", "origin", "main"]);
    let o = m.rolesmith(&["source", "update"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert_eq!(m.git(&clone, &["rev-parse", "main"]), second);
}
