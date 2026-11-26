//! `rolesmith propose` through the built binary, with a local bare repository
//! as the library's remote and a `gh` stand-in on `PATH` that records its arguments and prints a
//! pull request URL; nothing reaches the network.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const URL: &str = "https://github.com/example/lore/pull/12";

struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    /// A machine with a source `felix` whose library (a bare repository) has the role `rust-cli`.
    fn new() -> Machine {
        let m = Machine {
            dir: tempfile::tempdir().unwrap(),
        };
        let gh = m.path("bin/gh");
        std::fs::create_dir_all(gh.parent().unwrap()).unwrap();
        std::fs::write(
            &gh,
            format!(
                "#!/bin/sh\nprintf '%s\\0' \"$@\" > '{calls}'\npwd > '{cwd}'\necho 'Creating pull request'\necho '{URL}'\n",
                calls = m.path("gh-calls").display(),
                cwd = m.path("gh-cwd").display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();

        let bare = m.path("felix.git");
        std::fs::create_dir_all(&bare).unwrap();
        m.git(&bare, &["init", "--quiet", "--bare", "-b", "main"]);
        let work = m.path("work");
        std::fs::create_dir_all(&work).unwrap();
        m.git(&work, &["init", "--quiet", "-b", "main"]);
        m.write(&work, "library.toml", "name = \"felix\"\n");
        m.write(
            &work,
            "roles/rust-cli/role.toml",
            "description = \"Writes Rust CLIs\"\n",
        );
        m.write(&work, "roles/rust-cli/core.md", "# Core\n\nFirst rule.\n");
        m.git(&work, &["add", "-A"]);
        m.git(&work, &["commit", "--quiet", "-m", "first"]);
        m.git(&work, &["push", "--quiet", bare.to_str().unwrap(), "main"]);
        let o = m.rolesmith(&["source", "add", "felix", bare.to_str().unwrap()]);
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
        m
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn clone(&self) -> PathBuf {
        self.path("data/rolesmith/sources/felix")
    }

    fn bare(&self) -> PathBuf {
        self.path("felix.git")
    }

    fn env(&self, cmd: &mut Command) {
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap()
        );
        cmd.env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("PATH", path)
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

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        self.env(&mut cmd);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", stderr(&o));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    }

    fn write(&self, root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The arguments `gh` was last called with, if it was.
    fn gh_args(&self) -> Option<Vec<String>> {
        let raw = std::fs::read_to_string(self.path("gh-calls")).ok()?;
        Some(
            raw.split_terminator('\0')
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    }

    /// The bare remote's branches with their commits.
    fn remote_branches(&self) -> String {
        self.git(
            &self.bare(),
            &["for-each-ref", "--format=%(refname) %(objectname)"],
        )
    }

    fn refused(&self, args: &[&str], why: &str) {
        let before = self.remote_branches();
        let o = self.rolesmith(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", stderr(&o));
        assert!(stderr(&o).contains(why), "{args:?}: {}", stderr(&o));
        assert_eq!(self.remote_branches(), before, "{args:?} pushed");
        assert_eq!(self.gh_args(), None, "{args:?} ran gh");
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The value after `flag` in `args`.
fn flag<'a>(args: &'a [String], flag: &str) -> &'a str {
    let at = args.iter().position(|a| a == flag).expect(flag);
    &args[at + 1]
}

#[test]
fn propose_commits_pushes_the_branch_and_opens_the_pull_request() {
    let m = Machine::new();
    let clone = m.clone();
    let main_before = m.git(&m.bare(), &["rev-parse", "refs/heads/main"]);
    let o = m.rolesmith(&["new", "web-tester", "--need", "Tests web UIs"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    m.write(
        &clone,
        "roles/web-tester/core.md",
        "# Core\n\nUse a real browser.\n",
    );
    m.write(&clone, "roles/rust-cli/core.md", "# Core\n\nSecond rule.\n");

    let o = m.rolesmith(&["propose", "--closes", "3", "--closes", "7"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(String::from_utf8(o.stdout).unwrap(), format!("{URL}\n"));

    // The working copy's changes are one commit on its branch, and that branch is what was pushed.
    let branch = "rolesmith/new-web-tester";
    assert_eq!(m.git(&clone, &["status", "--porcelain"]), "");
    assert_eq!(m.git(&clone, &["symbolic-ref", "--short", "HEAD"]), branch);
    assert_eq!(
        m.git(&clone, &["rev-parse", "HEAD^"]),
        main_before,
        "one commit on main"
    );
    let head = m.git(&clone, &["rev-parse", "HEAD"]);
    assert_eq!(
        m.git(&m.bare(), &["rev-parse", &format!("refs/heads/{branch}")]),
        head
    );
    assert_eq!(
        m.git(&m.bare(), &["rev-parse", "refs/heads/main"]),
        main_before,
        "main was pushed to"
    );

    let args = m.gh_args().expect("gh ran");
    assert_eq!(&args[..2], ["pr", "create"]);
    assert_eq!(flag(&args, "--base"), "main");
    assert_eq!(flag(&args, "--head"), branch);
    assert_eq!(
        flag(&args, "--title"),
        "Change the roles rust-cli, web-tester"
    );
    assert_eq!(
        flag(&args, "--body"),
        "Proposed by `rolesmith propose`.\n\
         \n## rust-cli\n\n- `M roles/rust-cli/core.md`\n\
         \n## web-tester\n\n- `A roles/web-tester/core.md`\n- `A roles/web-tester/prohibitions.toml`\n- `A roles/web-tester/role.toml`\n\
         \nCloses #3\nCloses #7\n"
    );
    let cwd = std::fs::read_to_string(m.path("gh-cwd")).unwrap();
    assert_eq!(
        Path::new(cwd.trim()).canonicalize().unwrap(),
        clone.canonicalize().unwrap(),
        "gh runs in the source's working copy, so it finds its repository"
    );

    // Again with --json, after one more change outside any role: committed on top, pushed, and
    // the document is what agent-context says `propose` prints.
    m.write(&clone, "tools/asc/tool.toml", "description = \"asc\"\ncheck = \"asc --version\"\n[install]\nbrew = \"asc\"\nversion = \"7.0.0\"\n");
    let o = m.rolesmith(&["--json", "propose", "web-tester", "--source", "felix"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    let head = m.git(&clone, &["rev-parse", "HEAD"]);
    assert_eq!(
        doc,
        serde_json::json!({
            "source": "felix", "branch": branch, "commit": head,
            "roles": ["rust-cli", "web-tester"], "closes": [], "url": URL,
        })
    );
    assert_eq!(
        m.git(&m.bare(), &["rev-parse", &format!("refs/heads/{branch}")]),
        head
    );
    let body = flag(&m.gh_args().unwrap(), "--body").to_owned();
    assert!(
        body.ends_with("\n## Outside any role\n\n- `A tools/asc/tool.toml`\n"),
        "{body}"
    );
    let shapes = rolesmith::commands::output_shapes();
    let (_, shape) = shapes.iter().find(|(n, _)| *n == "propose").unwrap();
    let required = shape["required"].as_array().unwrap();
    assert_eq!(required.len(), doc.as_object().unwrap().len());
    for key in required {
        assert!(doc.get(key.as_str().unwrap()).is_some(), "{key}");
    }
}

#[test]
fn propose_refuses_main_another_branch_a_failing_check_and_nothing_to_propose() {
    let m = Machine::new();
    let clone = m.clone();

    m.write(&clone, "roles/rust-cli/core.md", "# Core\n\nOn main.\n");
    m.refused(&["propose"], "is on main, not a rolesmith/* branch");
    m.git(&clone, &["checkout", "--quiet", "-b", "feature"]);
    m.refused(&["propose"], "is on feature, not a rolesmith/* branch");
    m.git(&clone, &["checkout", "--quiet", "-f", "main"]);

    m.git(
        &clone,
        &["checkout", "--quiet", "-b", "rolesmith/distill-rust-cli"],
    );
    m.refused(&["propose"], "does not differ from main");

    // A failing check: nothing committed, nothing pushed, and the problems printed.
    m.write(
        &clone,
        "roles/rust-cli/role.toml",
        "description = \"\"\nflavour = 1\n",
    );
    let head = m.git(&clone, &["rev-parse", "HEAD"]);
    m.refused(
        &["propose", "rust-cli"],
        "`rolesmith check` finds 2 problem(s)",
    );
    let o = m.rolesmith(&["--json", "propose"]);
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    let problems = doc["error"]["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 2, "{doc}");
    assert!(
        problems
            .iter()
            .all(|p| p["file"] == "roles/rust-cli/role.toml"),
        "{doc}"
    );
    assert_eq!(m.git(&clone, &["rev-parse", "HEAD"]), head, "it committed");
    assert_ne!(m.git(&clone, &["status", "--porcelain"]), "");

    m.refused(&["propose", "--source", "nope"], "no source named nope");
}

#[test]
fn a_failing_gh_is_reported_after_the_push() {
    let m = Machine::new();
    std::fs::write(
        m.path("bin/gh"),
        "#!/bin/sh\necho 'a pull request already exists' >&2\nexit 1\n",
    )
    .unwrap();
    let clone = m.clone();
    m.git(
        &clone,
        &["checkout", "--quiet", "-b", "rolesmith/distill-rust-cli"],
    );
    m.write(
        &clone,
        "roles/rust-cli/core.md",
        "# Core\n\nAnother rule.\n",
    );
    let o = m.rolesmith(&["propose"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("is pushed, but `gh pr create` failed: a pull request already exists"),
        "{}",
        stderr(&o)
    );
    assert_eq!(
        m.git(
            &m.bare(),
            &["rev-parse", "refs/heads/rolesmith/distill-rust-cli"]
        ),
        m.git(&clone, &["rev-parse", "HEAD"])
    );
}
