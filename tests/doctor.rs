//! `doctor` and `tool show` (SPEC.md §3.5, §7.1, §8) through the built binary, with a local bare
//! repository as the library, stand-in programs on `PATH`, and a `mytokens` stand-in that answers
//! `list` and fails the test if anything reads a value (`get`, `env`).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    /// A machine whose `mytokens list` names `entries`, and whose `PATH` has `asc` (exits 0) and
    /// `xcodegen` (exits 3).
    fn new(entries: &[&str]) -> Machine {
        let m = Machine {
            dir: tempfile::tempdir().unwrap(),
        };
        let listing: String = entries
            .iter()
            .map(|e| format!("{e}/default\tstatic\tserver  — a description  fields: A, B\n"))
            .collect();
        std::fs::write(m.path("listing"), listing).unwrap();
        m.program(
            "mytokens",
            &format!(
                "echo \"$*\" >> '{calls}'\ncase \"$1\" in\n  list) cat '{listing}' ;;\n  *) echo read >> '{read}'; exit 1 ;;\nesac\n",
                calls = m.path("calls").display(),
                listing = m.path("listing").display(),
                read = m.path("value-read").display(),
            ),
        );
        m.program("asc", "exit 0\n");
        m.program("xcodegen", "exit 3\n");
        m
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn program(&self, name: &str, body: &str) {
        let file = self.path("bin").join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
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
        let o = cmd.output().expect("the binary runs");
        assert!(
            !self.path("value-read").exists(),
            "{args:?} asked mytokens for a value: {}",
            std::fs::read_to_string(self.path("calls")).unwrap_or_default()
        );
        o
    }

    fn git(&self, dir: &Path, args: &[&str]) {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        self.env(&mut cmd);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", stderr(&o));
    }

    /// Publish a library `lore` of `files` to a bare repository and add it as the source `felix`.
    fn source(&self, files: &[(&str, &str)]) {
        let work = self.path("lore");
        let bare = self.path("lore.git");
        std::fs::create_dir_all(&bare).unwrap();
        self.git(&bare, &["init", "--quiet", "--bare", "-b", "main"]);
        std::fs::create_dir_all(&work).unwrap();
        self.git(&work, &["init", "--quiet", "-b", "main"]);
        for (rel, text) in std::iter::once(&("library.toml", "name = \"lore\"\n")).chain(files) {
            let file = work.join(rel);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        self.git(&work, &["add", "-A"]);
        self.git(&work, &["commit", "--quiet", "-m", "first"]);
        self.git(&work, &["push", "--quiet", bare.to_str().unwrap(), "main"]);
        let o = self.rolesmith(&["source", "add", "felix", bare.to_str().unwrap()]);
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    }

    /// What was asked of mytokens, one call a line.
    fn mytokens_calls(&self) -> String {
        std::fs::read_to_string(self.path("calls")).unwrap_or_default()
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap()
}

fn role(programs: &[&str], services: &[&str]) -> String {
    let list = |xs: &[&str]| {
        xs.iter()
            .map(|x| format!("\"{x}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "description = \"Ships apps\"\nprograms = [{}]\nservices = [{}]\n",
        list(programs),
        list(services)
    )
}

fn tool(check: &str, credential: &str) -> String {
    format!(
        "description = \"A tool\"\ncheck = \"{check}\"\n[install]\nbrew = \"x\"\nversion = \"1.0.0\"\n{credential}"
    )
}

const ASC_CREDENTIAL: &str = "[credential]\nmytokens = \"asc-api-key\"\nenv = { ASC_KEY_ID = \"Key ID\", ASC_ISSUER_ID = \"Issuer ID\" }\n";

#[test]
fn doctor_passes_when_every_program_checks_and_every_entry_exists() {
    let m = Machine::new(&["asc-api-key", "github"]);
    let role_toml = role(&["asc"], &["github"]);
    let asc = tool("asc --version", ASC_CREDENTIAL);
    m.source(&[
        ("roles/releaser/role.toml", &role_toml),
        ("roles/releaser/core.md", "Core.\n"),
        ("tools/asc/tool.toml", &asc),
    ]);

    let o = m.rolesmith(&["--json", "doctor", "releaser"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    assert_eq!(doc["role"], "releaser");
    assert_eq!(doc["programs"][0]["name"], "asc");
    assert_eq!(doc["entries"], serde_json::json!(["github", "asc-api-key"]));
    assert_eq!(m.mytokens_calls(), "list\n", "only `mytokens list` is run");

    let o = m.rolesmith(&["doctor", "releaser"]);
    assert_eq!(o.status.code(), Some(0));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(human.contains("nothing missing"), "{human}");
    assert!(human.contains("- entry asc-api-key: present"), "{human}");
}

#[test]
fn doctor_reports_a_missing_program_a_failing_check_and_a_missing_entry_each_with_its_fix() {
    let m = Machine::new(&["github"]);
    let role_toml = role(&["asc", "xcodegen", "nothere"], &["github", "sentry"]);
    let asc = tool("asc --version", ASC_CREDENTIAL);
    let xcodegen = tool("xcodegen --version", "");
    let nothere = tool("rolesmith-test-nothere --version", "");
    m.source(&[
        ("roles/releaser/role.toml", &role_toml),
        ("roles/releaser/core.md", "Core.\n"),
        ("tools/asc/tool.toml", &asc),
        ("tools/xcodegen/tool.toml", &xcodegen),
        ("tools/nothere/tool.toml", &nothere),
    ]);

    let o = m.rolesmith(&["--json", "doctor", "releaser"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    let missing = json(&o)["error"]["missing"].clone();
    let fix_program = "rolesmith tool install felix/releaser";
    let expected = serde_json::json!([
        { "kind": "program", "name": "xcodegen",
          "reason": "check `xcodegen --version` fails: exits 3", "fix": fix_program },
        { "kind": "program", "name": "nothere",
          "reason": "check `rolesmith-test-nothere --version` not installed: its command is not found",
          "fix": fix_program },
        { "kind": "entry", "name": "sentry", "reason": "no mytokens entry",
          "fix": "mytokens put sentry" },
        { "kind": "entry", "name": "asc-api-key", "reason": "no mytokens entry",
          "fix": "mytokens put asc-api-key --fields \"Issuer ID\",\"Key ID\"" },
    ]);
    assert_eq!(missing, expected);
    assert_eq!(m.mytokens_calls(), "list\n");

    let err = stderr(&o);
    assert!(err.contains("lacks 4 thing(s)"), "{err}");
    assert!(
        err.contains("entry sentry: no mytokens entry; fix: mytokens put sentry"),
        "{err}"
    );
    assert!(
        err.contains("program xcodegen: check `xcodegen --version` fails: exits 3; fix: rolesmith tool install felix/releaser"),
        "{err}"
    );
}

#[test]
fn doctor_runs_no_mytokens_when_the_role_reads_no_entry_and_fails_when_it_cannot_list() {
    let m = Machine::new(&[]);
    let plain = role(&["xcodegen"], &[]);
    let reads = role(&[], &["github"]);
    let xcodegen = tool("true", "");
    m.source(&[
        ("roles/plain/role.toml", &plain),
        ("roles/plain/core.md", "Core.\n"),
        ("roles/reads/role.toml", &reads),
        ("roles/reads/core.md", "Core.\n"),
        ("tools/xcodegen/tool.toml", &xcodegen),
    ]);
    let o = m.rolesmith(&["doctor", "plain"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(m.mytokens_calls(), "");

    m.program("mytokens", "echo locked >&2; exit 1\n");
    let o = m.rolesmith(&["doctor", "reads"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("`mytokens list` failed: locked"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn doctor_refuses_a_role_whose_program_has_no_tool_toml() {
    let m = Machine::new(&[]);
    let role_toml = role(&["asc"], &[]);
    m.source(&[
        ("roles/releaser/role.toml", &role_toml),
        ("roles/releaser/core.md", "Core.\n"),
    ]);
    let o = m.rolesmith(&["--json", "doctor", "releaser"]);
    assert_eq!(o.status.code(), Some(1));
    let problems = json(&o)["error"]["problems"].clone();
    assert_eq!(problems[0]["rule"], "tool-missing", "{problems}");
}

#[test]
fn tool_show_prints_the_tool_toml_and_whether_its_check_passes_here() {
    let m = Machine::new(&[]);
    let asc = tool("asc --version", ASC_CREDENTIAL);
    let xcodegen = tool("xcodegen --version", "");
    m.source(&[
        ("tools/asc/tool.toml", &asc),
        ("tools/xcodegen/tool.toml", &xcodegen),
        ("tools/broken/tool.toml", "description = 1\n"),
    ]);

    let o = m.rolesmith(&["--json", "tool", "show", "asc"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    assert_eq!(doc["tool_toml"], asc.trim_end());
    assert_eq!(doc["check"], "asc --version");
    assert_eq!(doc["passes"], true);

    let o = m.rolesmith(&["tool", "show", "felix/xcodegen"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(human.contains("check = \"xcodegen --version\""), "{human}");
    assert!(
        human.contains("`xcodegen --version` fails: exits 3"),
        "{human}"
    );

    let o = m.rolesmith(&["tool", "show", "nothere"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("no source has a tool nothere on main"),
        "{}",
        stderr(&o)
    );
    let o = m.rolesmith(&["tool", "show", "broken"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("does not parse"), "{}", stderr(&o));
}
