//! Sources, `list` and `show` (SPEC.md §2, §8) through the built binary, with local bare git
//! repositories as libraries; nothing reaches the network.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// A machine: its own HOME and XDG directories, and a place for libraries.
struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    fn new() -> Machine {
        Machine {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn env(&self, cmd: &mut Command) {
        cmd.env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .env_remove("RUST_BACKTRACE");
    }

    fn rolesmith(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
        cmd.args(args);
        self.env(&mut cmd);
        cmd.output().expect("the binary runs")
    }

    /// Run `rolesmith --json <args>`, expect success, answer its document.
    fn json(&self, args: &[&str]) -> Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let o = self.rolesmith(&all);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", stderr(&o));
        serde_json::from_slice(&o.stdout).unwrap()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        self.env(&mut cmd);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", stderr(&o));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    }

    /// A bare repository `<name>.git` and a working clone `<name>` that pushes to it.
    fn library(&self, name: &str) -> Library {
        let bare = self.path(&format!("{name}.git"));
        let work = self.path(name);
        std::fs::create_dir_all(&bare).unwrap();
        self.git(&bare, &["init", "--quiet", "--bare", "-b", "main"]);
        self.git(
            self.dir.path(),
            &["clone", "--quiet", bare.to_str().unwrap(), name],
        );
        self.git(&work, &["checkout", "--quiet", "-b", "main"]);
        Library { bare, work }
    }

    /// Commit everything in `lib`'s working clone and push it; answer the commit.
    fn publish(&self, lib: &Library, message: &str) -> String {
        self.git(&lib.work, &["add", "-A"]);
        self.git(&lib.work, &["commit", "--quiet", "-m", message]);
        self.git(&lib.work, &["push", "--quiet", "origin", "main"]);
        self.git(&lib.work, &["rev-parse", "HEAD"])
    }
}

struct Library {
    bare: PathBuf,
    work: PathBuf,
}

impl Library {
    fn url(&self) -> &str {
        self.bare.to_str().unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.work.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn role(&self, role: &str, description: &str, core: &str) {
        self.write(
            &format!("roles/{role}/role.toml"),
            &format!(
                "description = \"{description}\"\nprograms = [\"asc\"]\nservices = [\"asc-api-key\"]\n"
            ),
        );
        self.write(&format!("roles/{role}/core.md"), core);
        self.write(
            &format!("roles/{role}/prohibitions.toml"),
            "[[prohibition]]\nrule = \"Never revoke a certificate\"\nwhy = \"It breaks other machines\"\n",
        );
        self.write(
            &format!("roles/{role}/skills/submit-build/SKILL.md"),
            "---\nname: submit-build\ndescription: Use when a build is ready for review.\n---\n\nSteps.\n",
        );
        self.write(
            "tools/asc/tool.toml",
            "description = \"App Store Connect CLI\"\ncheck = \"asc --version\"\n[install]\nbrew = \"asc\"\nversion = \"7.0.0\"\n",
        );
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A library `name` with `library.toml` and the given roles, published.
fn published(m: &Machine, name: &str, roles: &[&str]) -> (Library, String) {
    let lib = m.library(name);
    lib.write("library.toml", &format!("name = \"{name}\"\n"));
    for role in roles {
        lib.role(
            role,
            &format!("The {role} of {name}"),
            "# Core\n\nFirst rule.\n",
        );
    }
    let sha = m.publish(&lib, "first");
    (lib, sha)
}

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
    let (before_need, after_need) = template.split_once("{{need}}").unwrap();
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
}

#[test]
fn new_refuses_a_dirty_working_copy_an_existing_role_and_a_taken_branch() {
    let m = Machine::new();
    let (lib, _) = published(&m, "felix", &["rust-cli"]);
    m.json(&["source", "add", "felix", lib.url()]);
    let clone = m.path("data/rolesmith/sources/felix");
    let refused = |args: &[&str], why: &str| {
        let o = m.rolesmith(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", stderr(&o));
        assert!(stderr(&o).contains(why), "{args:?}: {}", stderr(&o));
    };

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
        "already has a branch rolesmith/new-web",
    );

    let (other, _) = published(&m, "other", &[]);
    m.json(&["source", "add", "other", other.url()]);
    refused(&["new", "api", "--need", "x"], "more than one source");
}
