//! Sources, `list` and `show` (SPEC.md §2, §8) through the built binary, with local bare git
//! repositories as libraries; nothing reaches the network.

mod common;

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
