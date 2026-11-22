//! What the tests that run the built binary share: a machine of its own (HOME, XDG directories,
//! a stand-in `gh` on PATH) and local bare git repositories as libraries; nothing reaches the
//! network.

#![allow(dead_code)] // each test crate uses its own part of this

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// A machine: its own HOME and XDG directories, and a place for libraries.
/// A `gh` that records each call (`call.<n>`: one argument a line; `stdin.<n>`) in
/// `$GH_STANDIN` and prints the answer [`Machine::gh_answers`] left for it; a call with no answer
/// prints nothing.
const GH_STANDIN: &str = r#"#!/bin/sh
n=1
while [ -e "$GH_STANDIN/call.$n" ]; do n=$((n + 1)); done
printf '%s\n' "$@" > "$GH_STANDIN/call.$n"
if [ "$1 $2" = "issue create" ]; then cat > "$GH_STANDIN/stdin.$n"; fi
answer="$GH_STANDIN/$1-$2"
if [ -e "$answer" ]; then cat "$answer"; fi
"#;

pub struct Machine {
    pub dir: tempfile::TempDir,
}

impl Machine {
    pub fn new() -> Machine {
        let m = Machine {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(m.path("gh")).unwrap();
        let gh = m.path("bin/gh");
        std::fs::create_dir_all(gh.parent().unwrap()).unwrap();
        std::fs::write(&gh, GH_STANDIN).unwrap();
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        m
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// What the stand-in `gh` prints for `gh <subcommand> <action>` (e.g. `"issue list"`).
    pub fn gh_answers(&self, call: &str, stdout: &str) {
        std::fs::write(self.path("gh").join(call.replace(' ', "-")), stdout).unwrap();
    }

    /// Every call the stand-in `gh` took, in order: its arguments, and what it read on stdin.
    pub fn gh_calls(&self) -> Vec<(Vec<String>, String)> {
        let mut calls = Vec::new();
        for i in 1.. {
            let Ok(args) = std::fs::read_to_string(self.path(&format!("gh/call.{i}"))) else {
                break;
            };
            let stdin =
                std::fs::read_to_string(self.path(&format!("gh/stdin.{i}"))).unwrap_or_default();
            calls.push((args.lines().map(str::to_owned).collect(), stdin));
        }
        calls
    }

    /// Run `rolesmith <args>` with `input` on stdin.
    pub fn rolesmith_input(&self, args: &[&str], input: &str) -> Output {
        use std::io::Write;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        self.env(&mut cmd);
        let mut child = cmd.spawn().expect("the binary runs");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    pub fn env(&self, cmd: &mut Command) {
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        cmd.env("PATH", path)
            .env("GH_STANDIN", self.path("gh"))
            .env("HOME", self.path("home"))
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

    pub fn rolesmith(&self, args: &[&str]) -> Output {
        self.rolesmith_input(args, "")
    }

    /// Run `rolesmith --json <args>`, expect success, answer its document.
    pub fn json(&self, args: &[&str]) -> Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let o = self.rolesmith(&all);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", stderr(&o));
        serde_json::from_slice(&o.stdout).unwrap()
    }

    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        self.env(&mut cmd);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", stderr(&o));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    }

    /// A bare repository `<name>.git` and a working clone `<name>` that pushes to it.
    pub fn library(&self, name: &str) -> Library {
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
    pub fn publish(&self, lib: &Library, message: &str) -> String {
        self.git(&lib.work, &["add", "-A"]);
        self.git(&lib.work, &["commit", "--quiet", "-m", message]);
        self.git(&lib.work, &["push", "--quiet", "origin", "main"]);
        self.git(&lib.work, &["rev-parse", "HEAD"])
    }
}

pub struct Library {
    pub bare: PathBuf,
    pub work: PathBuf,
}

impl Library {
    pub fn url(&self) -> &str {
        self.bare.to_str().unwrap()
    }

    pub fn write(&self, rel: &str, text: &str) {
        let path = self.work.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    pub fn role(&self, role: &str, description: &str, core: &str) {
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

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A library `name` with `library.toml` and the given roles, published.
pub fn published(m: &Machine, name: &str, roles: &[&str]) -> (Library, String) {
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
