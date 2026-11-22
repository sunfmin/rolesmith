//! What the integration tests share: a machine of their own, and libraries as local bare git
//! repositories; nothing reaches the network.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// A machine: its own HOME and XDG directories, and a place for libraries.
pub struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    pub fn new() -> Machine {
        Machine {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    pub fn env(&self, cmd: &mut Command) {
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

    pub fn rolesmith(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
        cmd.args(args);
        self.env(&mut cmd);
        cmd.output().expect("the binary runs")
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
