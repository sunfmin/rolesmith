//! `doctor`, `tool show`, `tool install` and `env` through the built
//! binary, with a local bare repository as the library, stand-in programs on `PATH`, a `mytokens`
//! stand-in that answers `list` and fails the test if anything reads a value (`get`, `env`) unless
//! the test serves values (for `rolesmith env`), a `brew` stand-in that offers the formulae a test
//! gives it, and a `sudo` stand-in that fails the test if anything runs it.

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
        m.program(
            "sudo",
            &format!(
                "echo \"$*\" >> '{}'\nexit 1\n",
                m.path("sudo-ran").display()
            ),
        );
        m.program(
            "brew",
            &format!(
                "echo \"$*\" >> '{calls}'\nf='{formulae}'/\"$3\"\ncase \"$1\" in\n  info) [ -f \"$f\" ] || {{ echo \"No available formula $3\" >&2; exit 1; }}; echo \"{{\\\"formulae\\\":[{{\\\"versions\\\":{{\\\"stable\\\":\\\"$(cat \"$f\")\\\"}}}}]}}\" ;;\n  install) f='{formulae}'/\"$2\"; [ -f \"$f.provides\" ] || exit 1; p='{bin}'/\"$(cat \"$f.provides\")\"; printf '#!/bin/sh\\nexit 0\\n' > \"$p\"; chmod +x \"$p\" ;;\n  *) exit 1 ;;\nesac\n",
                calls = m.path("brew-calls").display(),
                formulae = m.path("formulae").display(),
                bin = m.path("bin").display(),
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
        let o = self.rolesmith_with(args, &[]);
        assert!(
            !self.path("sudo-ran").exists(),
            "{args:?} ran sudo: {}",
            std::fs::read_to_string(self.path("sudo-ran")).unwrap_or_default()
        );
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

    /// Have brew offer `formula` at `version`; installing it puts `provides` on `PATH`, exiting 0.
    fn formula(&self, formula: &str, version: &str, provides: &str) {
        let dir = self.path("formulae");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(formula), version).unwrap();
        std::fs::write(dir.join(format!("{formula}.provides")), provides).unwrap();
    }

    /// What was asked of brew, one call a line.
    fn brew_calls(&self) -> String {
        std::fs::read_to_string(self.path("brew-calls")).unwrap_or_default()
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

/// The output shape `command` publishes (the first of a `oneOf`).
fn shape(command: &str) -> Value {
    let (_, shape) = rolesmith::commands::output_shapes()
        .into_iter()
        .find(|(n, _)| *n == command)
        .unwrap();
    shape.get("oneOf").map_or(shape.clone(), |s| s[0].clone())
}

/// `doc` has every key `shape` requires, and so do the items of each array it describes.
fn conforms(doc: &Value, shape: &Value) {
    let required = shape["required"]
        .as_array()
        .expect("the shape requires keys");
    assert!(!required.is_empty());
    for key in required {
        let key = key.as_str().unwrap();
        assert!(doc.get(key).is_some(), "{key} missing from {doc}");
        let items = &shape["properties"][key]["items"];
        if items.get("required").is_some() {
            for item in doc[key].as_array().unwrap() {
                conforms(item, items);
            }
        }
    }
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
    brewed(check, "x", "1.0.0", credential)
}

fn brewed(check: &str, formula: &str, version: &str, credential: &str) -> String {
    format!(
        "description = \"A tool\"\ncheck = \"{check}\"\n[install]\nbrew = \"{formula}\"\nversion = \"{version}\"\n{credential}"
    )
}

const ASC_CREDENTIAL: &str = "[credential]\nmytokens = \"asc-api-key\"\nenv = { ASC_KEY_ID = \"Key ID\", ASC_ISSUER_ID = \"Issuer ID\" }\n";

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
    for item in missing.as_array().unwrap() {
        conforms(item, &rolesmith::machine::Missing::shape());
    }
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
    let o = m.rolesmith(&["tool", "show", "asc"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("no tool asc: no sources"),
        "{}",
        stderr(&o)
    );
    let asc = tool("asc --version", ASC_CREDENTIAL);
    let xcodegen = tool("xcodegen --version", "");
    m.source(&[
        ("tools/asc/tool.toml", &asc),
        ("tools/xcodegen/tool.toml", &xcodegen),
        ("tools/broken/tool.toml", "description = 1\n"),
        ("tools/bare/README.md", "No tool.toml yet.\n"),
    ]);

    let o = m.rolesmith(&["--json", "tool", "show", "asc"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    conforms(&doc, &shape("tool"));
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
    let o = m.rolesmith(&["tool", "show", "bare"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("has no tools/bare/tool.toml"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn tool_install_installs_each_failing_program_pinned_and_leaves_a_passing_one_alone() {
    let m = Machine::new(&[]);
    let role_toml = role(&["asc", "swiftlint", "xcodegen"], &[]);
    // asc passes already; swiftlint has a versioned formula; xcodegen's formula is at the pin.
    let asc = brewed("asc --version", "asc", "7.0.0", "");
    let swiftlint = brewed(
        "rolesmith-test-swiftlint --version",
        "swiftlint",
        "0.57.0",
        "",
    );
    let xcodegen = brewed("xcodegen --version", "xcodegen", "2.42.0", "");
    m.source(&[
        ("roles/releaser/role.toml", &role_toml),
        ("roles/releaser/core.md", "Core.\n"),
        ("tools/asc/tool.toml", &asc),
        ("tools/swiftlint/tool.toml", &swiftlint),
        ("tools/xcodegen/tool.toml", &xcodegen),
    ]);
    m.formula("swiftlint@0.57.0", "0.57.0", "rolesmith-test-swiftlint");
    m.formula("xcodegen", "2.42.0", "xcodegen");

    let o = m.rolesmith(&["--json", "tool", "install", "releaser"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    let install_shape = rolesmith::commands::output_shapes()
        .into_iter()
        .find(|(n, _)| *n == "tool")
        .unwrap()
        .1["oneOf"][1]
        .clone();
    conforms(&doc, &install_shape);
    assert_eq!(
        doc["tools"],
        serde_json::json!([
            { "name": "asc", "method": null, "formula": null },
            { "name": "swiftlint", "method": "brew", "formula": "swiftlint@0.57.0" },
            { "name": "xcodegen", "method": "brew", "formula": "xcodegen" },
        ])
    );
    assert_eq!(
        m.brew_calls(),
        "info --json=v2 swiftlint@0.57.0\ninstall swiftlint@0.57.0\n\
         info --json=v2 xcodegen@2.42.0\ninfo --json=v2 xcodegen\ninstall xcodegen\n",
        "asc's check passes, so brew is not asked about it"
    );

    // Now every check passes: nothing more is asked of brew.
    let o = m.rolesmith(&["tool", "install", "felix/releaser"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(
        human.contains("- swiftlint: its check passes, left alone"),
        "{human}"
    );
    assert_eq!(m.brew_calls().lines().count(), 5);
}

#[test]
fn tool_install_refuses_a_version_brew_does_not_offer_and_a_check_that_still_fails() {
    let m = Machine::new(&[]);
    let old = brewed("rolesmith-test-old --version", "old", "1.0.0", "");
    let liar = brewed("rolesmith-test-liar --version", "liar", "3.0.0", "");
    let unknown = brewed("rolesmith-test-unknown --version", "unknown", "1.0.0", "");
    let first = brewed("rolesmith-test-first --version", "first", "1.0.0", "");
    let role_toml = role(&["first", "old"], &[]);
    m.source(&[
        ("roles/rr/role.toml", &role_toml),
        ("roles/rr/core.md", "Core.\n"),
        ("tools/first/tool.toml", &first),
        ("tools/old/tool.toml", &old),
        ("tools/liar/tool.toml", &liar),
        ("tools/unknown/tool.toml", &unknown),
    ]);
    m.formula("first", "1.0.0", "rolesmith-test-first");
    m.formula("old", "2.0.0", "rolesmith-test-old");
    m.formula("liar", "3.0.0", "rolesmith-test-something-else");

    let o = m.rolesmith(&["--json", "tool", "install", "rr"]);
    assert_eq!(o.status.code(), Some(1));
    let message = json(&o)["error"]["message"].as_str().unwrap().to_owned();
    assert_eq!(
        message,
        "brew offers old 2.0.0, not the pinned 1.0.0, and no old@1.0.0; nothing installed; \
         installed before it: first"
    );
    assert!(
        !m.brew_calls().contains("install old"),
        "{}",
        m.brew_calls()
    );

    let o = m.rolesmith(&["tool", "install", "--tool", "liar"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains(
            "installed liar, but tool liar's check `rolesmith-test-liar --version` not installed"
        ),
        "{}",
        stderr(&o)
    );

    let o = m.rolesmith(&["tool", "install", "--tool", "felix/unknown"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("brew has no formula unknown: No available formula unknown"),
        "{}",
        stderr(&o)
    );

    let o = m.rolesmith(&["tool", "install"]);
    assert_eq!(o.status.code(), Some(2), "a role or --tool is required");
    let o = m.rolesmith(&["tool", "install", "rr", "--tool", "liar"]);
    assert_eq!(o.status.code(), Some(2), "not both");

    // sh answers 127 for a program it cannot find.
    m.program("brew", "exit 127\n");
    let o = m.rolesmith(&["tool", "install", "--tool", "felix/unknown"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("cannot run `brew info --json=v2 unknown@1.0.0`: brew is not found"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn tool_install_refuses_an_unpinned_tool_named_alone() {
    let m = Machine::new(&[]);
    m.source(&[
        (
            "tools/loose/tool.toml",
            "description = \"A\"\ncheck = \"false\"\n[install]\nbrew = \"loose\"\n",
        ),
        ("tools/empty/tool.toml", &brewed("false", "empty", "", "")),
    ]);
    let o = m.rolesmith(&["tool", "install", "--tool", "loose"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("tool loose cannot be installed: [install] is not pinned"),
        "{}",
        stderr(&o)
    );
    let o = m.rolesmith(&["tool", "install", "--tool", "empty"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("its `version` is empty"),
        "{}",
        stderr(&o)
    );
    assert_eq!(m.brew_calls(), "");
}

impl Machine {
    /// Let `mytokens get <entry> [--field <f>]` answer each of `values` (entry, field, value),
    /// newline-terminated as mytokens prints it; `list` still names the entries of `new`.
    fn serve_values(&self, values: &[(&str, &str, &str)]) {
        let mut cases = String::new();
        for (i, (entry, field, value)) in values.iter().enumerate() {
            let file = self.path(&format!("value-{i}"));
            std::fs::write(&file, format!("{value}\n")).unwrap();
            let args = if field.is_empty() {
                format!("get {entry}")
            } else {
                format!("get {entry} --field {field}")
            };
            cases.push_str(&format!("  '{args}') cat '{}' ;;\n", file.display()));
        }
        self.program(
            "mytokens",
            &format!(
                "echo \"$*\" >> '{calls}'\ncase \"$*\" in\n  list) cat '{listing}' ;;\n{cases}  *) echo \"no $*\" >&2; exit 1 ;;\nesac\n",
                calls = self.path("calls").display(),
                listing = self.path("listing").display(),
            ),
        );
    }

    /// Every file under `dirs` with its contents and modification time.
    fn snapshot(&self, dirs: &[&str]) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut files = Vec::new();
        let mut todo: Vec<PathBuf> = dirs.iter().map(|d| self.path(d)).collect();
        while let Some(dir) = todo.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let meta = std::fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    todo.push(path.clone());
                }
                let bytes = if meta.is_file() {
                    std::fs::read(&path).unwrap()
                } else {
                    Vec::new()
                };
                files.push((path, bytes, meta.modified().unwrap()));
            }
        }
        files.sort();
        files
    }
}

/// A value with every character sh treats specially inside and outside quotes.
const AWKWARD: &str = "it's \"quoted\"\nline two $HOME `id` \\ end";

fn env_library(m: &Machine, services: &[&str]) {
    let role_toml = role(&["asc", "gh"], services);
    let asc = tool("asc --version", ASC_CREDENTIAL);
    let gh = tool(
        "true",
        "[credential]\nmytokens = \"github\"\nenv = { GH_TOKEN = \"\" }\n",
    );
    m.source(&[
        ("roles/releaser/role.toml", &role_toml),
        ("roles/releaser/core.md", "Core.\n"),
        ("tools/asc/tool.toml", &asc),
        ("tools/gh/tool.toml", &gh),
    ]);
    m.serve_values(&[
        ("asc-api-key", "Key ID", AWKWARD),
        ("asc-api-key", "Issuer ID", "issuer"),
        ("github", "", "gh-value"),
    ]);
}

#[test]
fn env_prints_export_lines_that_sh_evals_back_to_each_value() {
    let m = Machine::new(&["asc-api-key", "github"]);
    env_library(&m, &["github"]);

    // It writes no file under home, the XDG dirs or the cwd, printing or running a command.
    let dirs = ["home", "config", "data", "cache", "state", "work"];
    std::fs::create_dir_all(m.path("work")).unwrap();
    let before = m.snapshot(&dirs);
    assert!(!before.is_empty(), "the source's clone is under data");
    for args in [&["env", "releaser"][..], &["env", "releaser", "--", "true"]] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
        cmd.args(args).current_dir(m.path("work"));
        m.env(&mut cmd);
        cmd.env("XDG_CACHE_HOME", m.path("cache"))
            .env("XDG_STATE_HOME", m.path("state"));
        let o = cmd.output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    }
    assert_eq!(m.snapshot(&dirs), before, "env wrote a file");

    let o = m.rolesmith(&["env", "releaser"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let lines = String::from_utf8(o.stdout).unwrap();
    assert!(
        lines.starts_with("export ASC_ISSUER_ID='issuer'\n"),
        "{lines}"
    );
    let file = m.path("exports");
    std::fs::write(&file, &lines).unwrap();
    for (name, want) in [
        ("ASC_KEY_ID", AWKWARD),
        ("ASC_ISSUER_ID", "issuer"),
        ("GH_TOKEN", "gh-value"),
    ] {
        let script = format!(
            "eval \"$(cat '{}')\"; printf %s \"${name}\"",
            file.display()
        );
        let got = Command::new("sh").arg("-c").arg(script).output().unwrap();
        assert_eq!(String::from_utf8(got.stdout).unwrap(), want, "{name}");
    }

    let o = m.rolesmith(&["--json", "env", "releaser"]);
    let doc = json(&o);
    conforms(&doc, &shape("env"));
    assert_eq!(doc["variables"]["ASC_KEY_ID"], AWKWARD);
    assert_eq!(doc["role"], "felix/releaser");
}

#[test]
fn env_runs_a_command_after_the_double_dash_with_the_variables_set() {
    let m = Machine::new(&["asc-api-key", "github"]);
    env_library(&m, &[]);

    let o = m.rolesmith(&["env", "releaser", "--", "env"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let child = String::from_utf8(o.stdout).unwrap();
    let has = |want: &str| child.lines().any(|l| l == want);
    assert!(has("GH_TOKEN=gh-value"), "{child}");
    assert!(has("ASC_ISSUER_ID=issuer"), "{child}");
    assert!(
        child.lines().any(|l| l.starts_with("HOME=")),
        "the child keeps the environment: {child}"
    );

    let o = m.rolesmith(&[
        "env",
        "releaser",
        "--",
        "sh",
        "-c",
        "printf %s \"$ASC_KEY_ID\"; exit 7",
    ]);
    assert_eq!(o.status.code(), Some(7), "the child's exit is rolesmith's");
    assert_eq!(String::from_utf8(o.stdout).unwrap(), AWKWARD);

    let o = m.rolesmith(&["env", "releaser", "--", "rolesmith-test-nothere"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("cannot run `rolesmith-test-nothere`"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn env_refuses_a_service_no_credential_maps_and_a_failing_read() {
    let m = Machine::new(&["asc-api-key", "github", "sentry"]);
    env_library(&m, &["github", "sentry"]);
    let o = m.rolesmith(&["--json", "env", "releaser"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(json(&o)["error"]["unmapped"], serde_json::json!(["sentry"]));
    assert!(
        stderr(&o).contains("no program's [credential] maps"),
        "{}",
        stderr(&o)
    );
    assert!(
        !m.mytokens_calls().contains("get"),
        "nothing is read before the refusal"
    );

    let m = Machine::new(&["asc-api-key", "github"]);
    env_library(&m, &[]);
    m.serve_values(&[]);
    let o = m.rolesmith(&["env", "releaser", "--", "true"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("`mytokens get asc-api-key` failed: no get"),
        "{}",
        stderr(&o)
    );
}

/// A tool installed by `script`, pinned at `version`; the script is a TOML literal string.
fn scripted(check: &str, script: &str, version: &str) -> String {
    format!(
        "description = \"A tool\"\ncheck = \"{check}\"\n[install]\nscript = '''{script}'''\nversion = \"{version}\"\n"
    )
}

impl Machine {
    /// A script line that puts `name` on `PATH`, printing `says`.
    fn provides(&self, name: &str, says: &str) -> String {
        let p = self.path("bin").join(name);
        format!(
            "printf '#!/bin/sh\\necho {says}\\n' > '{p}' && chmod +x '{p}'",
            p = p.display()
        )
    }

    /// A `pixi` stand-in that logs its arguments and puts `rolesmith-test-av` on `PATH`,
    /// printing version 7.1.1.
    fn pixi(&self) {
        self.program(
            "pixi",
            &format!(
                "echo \"$*\" >> '{}'\n{}\n",
                self.path("pixi-calls").display(),
                self.provides("rolesmith-test-av", "av 7.1.1")
            ),
        );
    }
}

#[test]
fn tool_install_runs_a_script_until_the_check_shows_the_pinned_version() {
    let m = Machine::new(&[]);
    m.pixi();
    // `here` is installed at the pin; `stale` is installed at another version; `rolesmith-test-av`
    // is not installed, and comes from conda-forge with pixi.
    m.program("rolesmith-test-here", "echo 'here 17C48'\n");
    m.program("rolesmith-test-stale", "echo 'stale 1.0.0'\n");
    let marker = m.path("here-script-ran");
    let here = scripted(
        "rolesmith-test-here --version",
        &format!("touch '{}'", marker.display()),
        "17C48",
    );
    let stale = scripted(
        "rolesmith-test-stale --version",
        &m.provides("rolesmith-test-stale", "stale 2.0.0"),
        "2.0.0",
    );
    let av = scripted(
        "rolesmith-test-av --version",
        "pixi global install rolesmith-test-av=7.1.1",
        "7.1.1",
    );
    let role_toml = role(&["here", "stale", "rolesmith-test-av"], &[]);
    m.source(&[
        ("roles/builder/role.toml", &role_toml),
        ("roles/builder/core.md", "Core.\n"),
        ("tools/here/tool.toml", &here),
        ("tools/stale/tool.toml", &stale),
        ("tools/rolesmith-test-av/tool.toml", &av),
    ]);

    // A check that passes without printing the pinned version is missing.
    let o = m.rolesmith(&["--json", "doctor", "builder"]);
    assert_eq!(o.status.code(), Some(1));
    let missing = json(&o)["error"]["missing"].clone();
    let names: Vec<&str> = missing
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["stale", "rolesmith-test-av"], "{missing}");
    assert_eq!(
        missing[0]["reason"],
        "check `rolesmith-test-stale --version` does not show the pinned version 2.0.0"
    );
    let o = m.rolesmith(&["--json", "tool", "show", "stale"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(json(&o)["passes"], false);

    let o = m.rolesmith(&["--json", "tool", "install", "builder"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        json(&o)["tools"],
        serde_json::json!([
            { "name": "here", "formula": null, "method": null },
            { "name": "rolesmith-test-av", "formula": null, "method": "script" },
            { "name": "stale", "formula": null, "method": "script" },
        ])
    );
    assert!(!marker.exists(), "an installed tool's script is not run");
    assert_eq!(
        std::fs::read_to_string(m.path("pixi-calls")).unwrap(),
        "global install rolesmith-test-av=7.1.1\n"
    );
    assert_eq!(m.brew_calls(), "", "a script tool never asks brew");

    let o = m.rolesmith(&["doctor", "builder"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let o = m.rolesmith(&["tool", "install", "builder"]);
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(
        human.contains("- stale: its check passes, left alone"),
        "{human}"
    );
}

#[test]
fn tool_install_refuses_a_script_that_fails_or_installs_another_version() {
    let m = Machine::new(&[]);
    let broken = scripted(
        "rolesmith-test-broken --version",
        "echo compiling >&2; echo 'error: linker failed' >&2; exit 4",
        "1.0.0",
    );
    let wrong = scripted(
        "rolesmith-test-wrong --version",
        &m.provides("rolesmith-test-wrong", "wrong 1.9.0"),
        "2.0.0",
    );
    m.source(&[
        ("tools/broken/tool.toml", &broken),
        ("tools/wrong/tool.toml", &wrong),
    ]);

    let o = m.rolesmith(&["--json", "tool", "install", "--tool", "broken"]);
    assert_eq!(o.status.code(), Some(1));
    let message = json(&o)["error"]["message"].as_str().unwrap().to_owned();
    assert_eq!(
        message,
        "ran tool broken's script, which exits 4, and its check `rolesmith-test-broken --version` \
         not installed: its command is not found; the script's stderr ends:\n\
         compiling\nerror: linker failed"
    );

    let o = m.rolesmith(&["--json", "tool", "install", "--tool", "wrong"]);
    assert_eq!(o.status.code(), Some(1));
    let message = json(&o)["error"]["message"].as_str().unwrap().to_owned();
    assert_eq!(
        message,
        "ran tool wrong's script, which exits 0, and its check `rolesmith-test-wrong --version` \
         does not show the pinned version 2.0.0; the script's stderr is empty"
    );
}

impl Machine {
    /// A stand-in `ssh` that logs each call to `ssh-calls`, takes only `-o BatchMode=yes --
    /// <host> <command>...`, and runs the command as a remote sshd would (its words joined, by
    /// `sh -c`) with `hosts/<host>/bin` first on `PATH` and none of this machine's `bin`. A host
    /// without its directory cannot be reached: ssh's exit 255.
    fn ssh(&self) {
        self.program(
            "ssh",
            &format!(
                "echo \"$*\" >> '{calls}'\n[ \"$1 $2 $3\" = '-o BatchMode=yes --' ] || {{ echo \"ssh stand-in: unexpected $*\" >&2; exit 2; }}\nh='{hosts}'/\"$4\"\n[ -d \"$h\" ] || {{ echo \"ssh: connect to host $4 port 22: Connection refused\" >&2; exit 255; }}\nshift 4\nPATH=\"$h/bin:/usr/bin:/bin\" exec /bin/sh -c \"$*\"\n",
                calls = self.path("ssh-calls").display(),
                hosts = self.path("hosts").display(),
            ),
        );
    }

    /// A program `name` on `host`'s `PATH`, not this machine's.
    fn remote_program(&self, host: &str, name: &str, body: &str) {
        let file = self.path(&format!("hosts/{host}/bin/{name}"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// `host`'s own brew: it offers `formula` at `version`, and installing it puts `provides` on
    /// `host`'s `PATH`. It logs its calls to `hosts/<host>/brew-calls`.
    fn remote_brew(&self, host: &str, formula: &str, version: &str, provides: &str) {
        let dir = self.path(&format!("hosts/{host}"));
        std::fs::create_dir_all(dir.join("formulae")).unwrap();
        std::fs::write(dir.join("formulae").join(formula), version).unwrap();
        self.remote_program(
            host,
            "brew",
            &format!(
                "echo \"$*\" >> '{d}/brew-calls'\ncase \"$1\" in\n  info) f='{d}/formulae'/\"$3\"; [ -f \"$f\" ] || {{ echo \"No available formula $3\" >&2; exit 1; }}; echo \"{{\\\"formulae\\\":[{{\\\"versions\\\":{{\\\"stable\\\":\\\"$(cat \"$f\")\\\"}}}}]}}\" ;;\n  install) p='{d}/bin/{provides}'; printf '#!/bin/sh\\nexit 0\\n' > \"$p\"; chmod +x \"$p\" ;;\n  *) exit 1 ;;\nesac\n",
                d = dir.display(),
            ),
        );
    }

    fn ssh_calls(&self) -> String {
        std::fs::read_to_string(self.path("ssh-calls")).unwrap_or_default()
    }
}

/// A role `builder` with `metal` (a script tool pinned at 17C48, with a credential) and `asc`
/// (brew), and the service `github`.
fn remote_library(m: &Machine) {
    let metal = format!(
        "{}{ASC_CREDENTIAL}",
        scripted(
            "rolesmith-test-metal --version",
            "rolesmith-test-download-metal",
            "17C48"
        )
    );
    let asc = brewed("asc --version", "asc", "7.0.0", "");
    let role_toml = role(&["metal", "asc"], &["github"]);
    m.source(&[
        ("roles/builder/role.toml", &role_toml),
        ("roles/builder/core.md", "Core.\n"),
        ("tools/metal/tool.toml", &metal),
        ("tools/asc/tool.toml", &asc),
    ]);
}

#[test]
fn doctor_with_host_runs_each_check_there_over_ssh_and_reads_entries_here() {
    let m = Machine::new(&["asc-api-key", "github"]);
    m.ssh();
    remote_library(&m);
    m.remote_program("laptop", "rolesmith-test-metal", "echo 'metal 17C48'\n");
    m.remote_program("laptop", "asc", "exit 0\n");

    let o = m.rolesmith(&["--json", "doctor", "builder", "--host", "laptop"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    conforms(&doc, &shape("doctor"));
    assert_eq!(doc["host"], "laptop");
    assert_eq!(doc["entries"], serde_json::json!(["github", "asc-api-key"]));
    assert_eq!(
        m.ssh_calls(),
        "-o BatchMode=yes -- laptop sh -c 'rolesmith-test-metal --version'\n\
         -o BatchMode=yes -- laptop sh -c 'asc --version'\n"
    );
    assert_eq!(m.mytokens_calls(), "list\n", "entries are read here");

    // Without --host the checks run here, where metal is not installed.
    let o = m.rolesmith(&["--json", "doctor", "builder"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(json(&o)["error"]["missing"][0]["name"], "metal");

    // Once every program checks here too, doctor passes here.
    m.program("rolesmith-test-metal", "echo 'metal 17C48'\n");
    let o = m.rolesmith(&["--json", "doctor", "builder"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    conforms(&doc, &shape("doctor"));
    assert_eq!(doc["role"], "builder");
    assert_eq!(doc["programs"][0]["name"], "metal");
    assert_eq!(doc["entries"], serde_json::json!(["github", "asc-api-key"]));
    assert_eq!(doc["host"], Value::Null, "the checks ran here");

    let o = m.rolesmith(&["doctor", "builder"]);
    assert_eq!(o.status.code(), Some(0));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(human.contains("nothing missing"), "{human}");
    assert!(human.contains("- entry asc-api-key: present"), "{human}");
    assert!(
        m.mytokens_calls().lines().all(|l| l == "list"),
        "only `mytokens list` is run: {}",
        m.mytokens_calls()
    );
}

#[test]
fn doctor_with_host_names_what_the_host_lacks_with_fixes_that_carry_the_host() {
    let m = Machine::new(&["github"]);
    m.ssh();
    remote_library(&m);
    // The laptop has metal at another version and no asc, though this machine has asc.
    m.remote_program("laptop", "rolesmith-test-metal", "echo 'metal 16A1'\n");

    let o = m.rolesmith(&["--json", "doctor", "builder", "--host", "laptop"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    let fix = "rolesmith tool install felix/builder --host laptop";
    assert_eq!(
        json(&o)["error"]["missing"],
        serde_json::json!([
            { "kind": "program", "name": "metal",
              "reason": "check `rolesmith-test-metal --version` on laptop does not show the pinned version 17C48",
              "fix": fix },
            { "kind": "program", "name": "asc",
              "reason": "check `asc --version` on laptop not installed: its command is not found",
              "fix": fix },
            { "kind": "entry", "name": "asc-api-key", "reason": "no mytokens entry",
              "fix": "mytokens put asc-api-key --fields \"Issuer ID\",\"Key ID\"" },
        ])
    );
    let err = stderr(&o);
    assert!(
        err.contains("lacks 3 thing(s), programs on laptop and entries on this machine:"),
        "{err}"
    );
}

#[test]
fn doctor_and_tool_install_with_an_unreachable_host_fail_naming_it() {
    let m = Machine::new(&["asc-api-key", "github"]);
    m.ssh();
    remote_library(&m);

    for args in [
        &["--json", "doctor", "builder", "--host", "gone"][..],
        &["--json", "tool", "install", "builder", "--host", "gone"][..],
    ] {
        let o = m.rolesmith(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", stderr(&o));
        let error = json(&o)["error"].clone();
        assert_eq!(error["kind"], "failed", "{error}");
        assert!(
            error.get("missing").is_none(),
            "not a list of missing things: {error}"
        );
        assert_eq!(
            error["message"],
            "cannot reach host gone: `ssh -o BatchMode=yes gone` exits 255: \
             ssh: connect to host gone port 22: Connection refused",
            "{args:?}"
        );
    }
    assert_eq!(
        m.ssh_calls().lines().count(),
        2,
        "the first failure stops each"
    );
}

#[test]
fn tool_install_with_host_installs_there_with_its_own_brew_and_scripts() {
    let m = Machine::new(&[]);
    m.ssh();
    remote_library(&m);
    m.formula("asc", "7.0.0", "asc");
    m.remote_brew("laptop", "asc", "7.0.0", "asc");
    m.remote_program(
        "laptop",
        "rolesmith-test-download-metal",
        &format!(
            "printf '#!/bin/sh\\necho metal 17C48\\n' > '{p}' && chmod +x '{p}'\n",
            p = m.path("hosts/laptop/bin/rolesmith-test-metal").display()
        ),
    );

    let o = m.rolesmith(&["--json", "tool", "install", "builder", "--host", "laptop"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    assert_eq!(doc["host"], "laptop");
    assert_eq!(
        doc["tools"],
        serde_json::json!([
            { "name": "asc", "method": "brew", "formula": "asc" },
            { "name": "metal", "method": "script", "formula": null },
        ])
    );
    assert_eq!(
        std::fs::read_to_string(m.path("hosts/laptop/brew-calls")).unwrap(),
        "info --json=v2 asc@7.0.0\ninfo --json=v2 asc\ninstall asc\n"
    );
    assert_eq!(m.brew_calls(), "", "this machine's brew is not asked");

    let o = m.rolesmith(&["doctor", "builder", "--host", "laptop"]);
    assert!(
        !stderr(&o).contains("\n  program "),
        "every program is there now: {}",
        stderr(&o)
    );
}

/// A library where `app` requires `base`, and `app` and `web` share the tool `asc`, recorded in a
/// project by `use app web`; `asc` logs each run of its check. Answers the project directory.
fn project_library(m: &Machine) -> PathBuf {
    let asc = brewed("asc --version", "asc", "7.0.0", ASC_CREDENTIAL);
    let swiftlint = brewed(
        "rolesmith-test-swiftlint --version",
        "swiftlint",
        "0.57.0",
        "",
    );
    let xcodegen = brewed("xcodegen --version", "xcodegen", "2.42.0", "");
    let app = format!(
        "{}requires = [\"base\"]\n",
        role(&["asc", "swiftlint"], &[])
    );
    let web = role(&["asc"], &[]);
    let base = role(&["xcodegen"], &["github"]);
    m.source(&[
        ("roles/app/role.toml", &app),
        ("roles/app/core.md", "Core.\n"),
        ("roles/web/role.toml", &web),
        ("roles/web/core.md", "Core.\n"),
        ("roles/base/role.toml", &base),
        ("roles/base/core.md", "Core.\n"),
        ("tools/asc/tool.toml", &asc),
        ("tools/swiftlint/tool.toml", &swiftlint),
        ("tools/xcodegen/tool.toml", &xcodegen),
    ]);
    m.program(
        "asc",
        &format!("echo ran >> '{}'\nexit 0\n", m.path("asc-ran").display()),
    );
    let dir = m.path("project");
    std::fs::create_dir_all(&dir).unwrap();
    let o = m.rolesmith(&[
        "use",
        "app",
        "web",
        "--into",
        "claude-code",
        "--project",
        dir.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    dir
}

/// How many times `asc`'s check ran.
fn asc_runs(m: &Machine) -> usize {
    std::fs::read_to_string(m.path("asc-ran"))
        .unwrap_or_default()
        .lines()
        .count()
}

#[test]
fn doctor_project_checks_every_recorded_and_required_role_each_tool_once_naming_its_roles() {
    let m = Machine::new(&["github"]);
    let dir = project_library(&m);
    let project = dir.to_str().unwrap();

    let o = m.rolesmith(&["--json", "doctor", "--project", project]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert_eq!(
        asc_runs(&m),
        1,
        "asc is shared by app and web: checked once"
    );
    let fix = format!("rolesmith tool install --project {project}");
    let missing = json(&o)["error"]["missing"].clone();
    for item in missing.as_array().unwrap() {
        conforms(item, &rolesmith::machine::Missing::shape());
    }
    assert_eq!(
        missing,
        serde_json::json!([
            { "kind": "program", "name": "swiftlint",
              "reason": "check `rolesmith-test-swiftlint --version` not installed: its command is not found",
              "fix": fix, "from": ["felix/app"] },
            { "kind": "program", "name": "xcodegen",
              "reason": "check `xcodegen --version` fails: exits 3",
              "fix": fix, "from": ["felix/base"] },
            { "kind": "entry", "name": "asc-api-key", "reason": "no mytokens entry",
              "fix": "mytokens put asc-api-key --fields \"Issuer ID\",\"Key ID\"",
              "from": ["felix/app", "felix/web"] },
        ])
    );
    assert_eq!(m.mytokens_calls(), "list\n");
    let err = stderr(&o);
    assert!(err.contains("lacks 3 thing(s), on this machine:"), "{err}");
    assert!(
        err.contains("program xcodegen: check `xcodegen --version` fails: exits 3 (from felix/base); fix: rolesmith tool install --project"),
        "{err}"
    );

    // With everything present it answers the shape `doctor <role>` does, with the roles it read.
    m.program("xcodegen", "exit 0\n");
    m.program("rolesmith-test-swiftlint", "exit 0\n");
    std::fs::write(
        m.path("listing"),
        "github/default\tstatic\tx\nasc-api-key/default\tstatic\tx\n",
    )
    .unwrap();
    let o = m.rolesmith(&["--json", "doctor", "--project", project]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    conforms(&doc, &shape("doctor"));
    assert!(doc.get("role").is_none(), "{doc}");
    assert_eq!(doc["project"], project);
    let roles: Vec<&str> = doc["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["app", "base", "web"]);
    assert_eq!(
        doc["programs"],
        serde_json::json!([
            { "name": "asc", "check": "asc --version", "from": ["felix/app", "felix/web"] },
            { "name": "swiftlint", "check": "rolesmith-test-swiftlint --version", "from": ["felix/app"] },
            { "name": "xcodegen", "check": "xcodegen --version", "from": ["felix/base"] },
        ])
    );
    assert_eq!(doc["entries"], serde_json::json!(["asc-api-key", "github"]));

    // A service this host maps names the roles that need it, as a program does.
    m.map_service("github", "echo GITHUB_TOKEN=x", None);
    let o = m.rolesmith(&["--json", "doctor", "--project", project]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let services = json(&o)["services"].clone();
    assert_eq!(services[0]["name"], "github", "{services}");
    assert_eq!(services[0]["from"], serde_json::json!(["felix/base"]));
}

#[test]
fn doctor_project_reads_each_role_at_its_recorded_rev_not_the_source_main() {
    let m = Machine::new(&["github", "asc-api-key"]);
    let dir = project_library(&m);
    let recorded = std::fs::read_to_string(dir.join("rolesmith.toml")).unwrap();

    // The library moves on: base's xcodegen check now passes, and this machine brings it in.
    let work = m.path("lore");
    std::fs::write(
        work.join("tools/xcodegen/tool.toml"),
        brewed("true", "xcodegen", "2.42.0", ""),
    )
    .unwrap();
    m.git(&work, &["commit", "--quiet", "-am", "second"]);
    m.git(
        &work,
        &[
            "push",
            "--quiet",
            m.path("lore.git").to_str().unwrap(),
            "main",
        ],
    );
    let o = m.rolesmith(&["source", "update"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let o = m.rolesmith(&["doctor", "felix/base"]);
    assert_eq!(o.status.code(), Some(0), "main has the passing check");

    m.program("rolesmith-test-swiftlint", "exit 0\n");
    let o = m.rolesmith(&["--json", "doctor", "--project", dir.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    let missing = json(&o)["error"]["missing"].clone();
    assert_eq!(missing.as_array().unwrap().len(), 1, "{missing}");
    assert_eq!(
        missing[0]["name"], "xcodegen",
        "base is read at app's recorded rev"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("rolesmith.toml")).unwrap(),
        recorded,
        "doctor writes nothing to the record"
    );
}

#[test]
fn tool_install_project_installs_each_tool_of_every_role_once() {
    let m = Machine::new(&[]);
    let dir = project_library(&m);
    let project = dir.to_str().unwrap();
    m.formula("swiftlint@0.57.0", "0.57.0", "rolesmith-test-swiftlint");
    m.formula("xcodegen", "2.42.0", "xcodegen");

    let o = m.rolesmith(&["--json", "tool", "install", "--project", project]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        asc_runs(&m),
        1,
        "asc's check passes, run once for app and web"
    );
    let doc = json(&o);
    let install_shape = rolesmith::commands::output_shapes()
        .into_iter()
        .find(|(n, _)| *n == "tool")
        .unwrap()
        .1["oneOf"][1]
        .clone();
    conforms(&doc, &install_shape);
    assert_eq!(doc["project"], project);
    assert_eq!(
        doc["tools"],
        serde_json::json!([
            { "name": "asc", "method": null, "formula": null, "from": ["felix/app", "felix/web"] },
            { "name": "swiftlint", "method": "brew", "formula": "swiftlint@0.57.0", "from": ["felix/app"] },
            { "name": "xcodegen", "method": "brew", "formula": "xcodegen", "from": ["felix/base"] },
        ])
    );
    let o = m.rolesmith(&["tool", "install", "--project", project]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(
        human.contains("- xcodegen: its check passes, left alone"),
        "{human}"
    );
}

#[test]
fn doctor_and_tool_install_refuse_a_project_without_rolesmith_toml() {
    let m = Machine::new(&[]);
    let dir = m.path("empty");
    std::fs::create_dir_all(&dir).unwrap();
    let project = dir.to_str().unwrap();
    for args in [
        &["--json", "doctor", "--project", project][..],
        &["--json", "tool", "install", "--project", project][..],
    ] {
        let o = m.rolesmith(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", stderr(&o));
        let message = json(&o)["error"]["message"].as_str().unwrap().to_owned();
        assert!(
            message.contains(&format!("{project} has no rolesmith.toml")),
            "{message}"
        );
    }
    let o = m.rolesmith(&["doctor", "app", "--project", project]);
    assert_eq!(o.status.code(), Some(2), "a role and --project together");
    let o = m.rolesmith(&["tool", "install", "--tool", "asc", "--project", project]);
    assert_eq!(o.status.code(), Some(2), "--tool and --project together");
}

impl Machine {
    /// Have `mytokens list` also name `entry` as a Profile whose fields are `fields`.
    fn profile(&self, entry: &str, fields: &[&str]) {
        let mut listing = std::fs::read_to_string(self.path("listing")).unwrap();
        listing.push_str(&format!(
            "{entry}/default\tprofile\tserver  — a Profile, fields: not these  fields: {}\n",
            fields.join(", ")
        ));
        std::fs::write(self.path("listing"), listing).unwrap();
    }
}

#[test]
fn env_exports_a_profile_service_by_its_own_field_names() {
    let m = Machine::new(&[]);
    m.profile("signing", &["A", "B"]);
    m.source(&[
        ("roles/signer/role.toml", &role(&[], &["signing"])),
        ("roles/signer/core.md", "Core.\n"),
    ]);
    m.serve_values(&[("signing", "A", AWKWARD), ("signing", "B", "bee")]);

    let o = m.rolesmith(&["env", "signer"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let lines = String::from_utf8(o.stdout).unwrap();
    assert!(lines.contains("export B='bee'\n"), "{lines}");
    assert!(lines.starts_with("export A='"), "{lines}");

    let o = m.rolesmith(&[
        "env",
        "signer",
        "--",
        "sh",
        "-c",
        "printf '%s|%s' \"$A\" \"$B\"",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        format!("{AWKWARD}|bee")
    );

    let o = m.rolesmith(&["--json", "env", "signer"]);
    let doc = json(&o);
    conforms(&doc, &shape("env"));
    assert_eq!(
        doc["variables"],
        serde_json::json!({ "A": AWKWARD, "B": "bee" })
    );
}

#[test]
fn env_exports_a_profile_beside_credentials_and_refuses_a_name_from_two_sources() {
    let m = Machine::new(&["asc-api-key", "github"]);
    m.profile("signing", &["APPLE_TEAM"]);
    env_library(&m, &["github", "signing"]);
    m.serve_values(&[
        ("asc-api-key", "Key ID", "key"),
        ("asc-api-key", "Issuer ID", "issuer"),
        ("github", "", "gh-value"),
        ("signing", "APPLE_TEAM", "team"),
    ]);
    let o = m.rolesmith(&["--json", "env", "releaser"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        json(&o)["variables"],
        serde_json::json!({ "APPLE_TEAM": "team", "ASC_ISSUER_ID": "issuer",
                            "ASC_KEY_ID": "key", "GH_TOKEN": "gh-value" })
    );

    let m = Machine::new(&["asc-api-key", "github"]);
    m.profile("signing", &["ASC_KEY_ID", "GH_TOKEN", "OTHER"]);
    env_library(&m, &["signing"]);
    let o = m.rolesmith(&["--json", "env", "releaser"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        json(&o)["error"]["conflicting"],
        serde_json::json!([{ "name": "ASC_KEY_ID", "sources": [
            { "entry": "asc-api-key", "field": "Key ID" },
            { "entry": "signing", "field": "ASC_KEY_ID" } ] },
          { "name": "GH_TOKEN", "sources": [
            { "entry": "github", "field": "" },
            { "entry": "signing", "field": "GH_TOKEN" } ] }])
    );
    let err = stderr(&o);
    assert!(
        err.contains("ASC_KEY_ID: asc-api-key field \"Key ID\", signing field \"ASC_KEY_ID\""),
        "{err}"
    );
    assert!(
        err.contains("GH_TOKEN: github, signing field \"GH_TOKEN\""),
        "{err}"
    );
    assert!(
        !m.mytokens_calls().contains("get"),
        "nothing is read before the refusal"
    );
}

impl Machine {
    /// Map `service`, in this host's services.toml, to `command`, with `check` beside it, in
    /// place of what it was mapped to.
    fn map_service(&self, service: &str, command: &str, check: Option<&str>) {
        let file = self.path("config/rolesmith/services.toml");
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let mut doc: toml::Table = text.parse().unwrap();
        let mut mapped = toml::Table::new();
        mapped.insert("command".into(), command.into());
        if let Some(check) = check {
            mapped.insert("check".into(), check.into());
        }
        doc.entry("service")
            .or_insert_with(|| toml::Table::new().into())
            .as_table_mut()
            .unwrap()
            .insert(service.into(), mapped.into());
        std::fs::write(file, toml::to_string(&doc).unwrap()).unwrap();
    }

    /// A stand-in `name` on `PATH` that notes each run and what it read on stdin in `<name>-ran`,
    /// then runs `body`.
    fn noting(&self, name: &str, body: &str) {
        let ran = self.path(&format!("{name}-ran"));
        self.program(
            name,
            &format!("{{ echo run; cat; }} >> '{}' 2>&1\n{body}", ran.display()),
        );
    }

    fn ran(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(&format!("{name}-ran"))).unwrap_or_default()
    }
}

const PRINTS_A_AND_B: &str = "echo A=1\necho \"export B='two'\"\n";

#[test]
fn env_reads_a_service_this_host_maps_from_its_command_alone() {
    let m = Machine::new(&[]);
    m.source(&[
        ("roles/signer/role.toml", &role(&[], &["signing"])),
        ("roles/signer/core.md", "Core.\n"),
    ]);
    m.noting("signing-env", PRINTS_A_AND_B);
    m.map_service("signing", "signing-env", None);

    let o = m.rolesmith(&["env", "signer"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        "export A='1'\nexport B='two'\n"
    );
    assert_eq!(
        m.ran("signing-env"),
        "run\n",
        "stdin closed: it read nothing"
    );

    let o = m.rolesmith(&[
        "env",
        "signer",
        "--",
        "sh",
        "-c",
        "printf '%s|%s' \"$A\" \"$B\"",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(String::from_utf8(o.stdout).unwrap(), "1|two");

    let o = m.rolesmith(&["--json", "env", "signer"]);
    let doc = json(&o);
    conforms(&doc, &shape("env"));
    assert_eq!(
        doc["variables"],
        serde_json::json!({ "A": "1", "B": "two" })
    );
    assert_eq!(m.mytokens_calls(), "", "mytokens is never asked");
}

#[test]
fn env_refuses_a_mapped_command_that_fails_and_exports_nothing() {
    let m = Machine::new(&[]);
    m.source(&[
        ("roles/signer/role.toml", &role(&[], &["signing"])),
        ("roles/signer/core.md", "Core.\n"),
    ]);
    m.map_service(
        "signing",
        "echo A=1; echo 'vault is locked' >&2; exit 3",
        None,
    );
    let o = m.rolesmith(&["--json", "env", "signer"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(json(&o).get("variables").is_none());
    let err = stderr(&o);
    assert!(err.contains("exits 3: vault is locked"), "{err}");
    assert!(err.contains("service signing"), "{err}");

    let o = m.rolesmith(&["env", "signer", "--", "sh", "-c", "touch child-ran"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    assert!(!m.path("child-ran").exists());

    m.program("signing-env", "echo A=1\necho not-a-line-secret\n");
    m.map_service("signing", "signing-env", None);
    let o = m.rolesmith(&["env", "signer"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    let err = stderr(&o);
    assert!(err.contains("line 2 of what its command"), "{err}");
    assert!(!err.contains("not-a-line-secret"), "never echoed: {err}");
}

#[test]
fn env_refuses_a_credential_variable_the_mapped_command_does_not_print() {
    let m = Machine::new(&[]);
    m.source(&[
        ("roles/releaser/role.toml", &role(&["asc"], &[])),
        ("roles/releaser/core.md", "Core.\n"),
        ("tools/asc/tool.toml", &tool("true", ASC_CREDENTIAL)),
    ]);
    m.map_service("asc-api-key", "echo ASC_KEY_ID=k; echo OTHER=o", None);
    let o = m.rolesmith(&["--json", "env", "releaser"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        json(&o)["error"]["absent"],
        serde_json::json!([{ "name": "ASC_ISSUER_ID", "service": "asc-api-key" }])
    );
    assert!(stderr(&o).contains("ASC_ISSUER_ID"), "{}", stderr(&o));

    m.map_service(
        "asc-api-key",
        "echo ASC_KEY_ID=k; echo ASC_ISSUER_ID=i",
        None,
    );
    let o = m.rolesmith(&["--json", "env", "releaser"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        json(&o)["variables"],
        serde_json::json!({ "ASC_ISSUER_ID": "i", "ASC_KEY_ID": "k" })
    );
    assert_eq!(
        m.mytokens_calls(),
        "",
        "a mapped credential is not read from mytokens"
    );
}

#[test]
fn doctor_runs_a_mapped_services_check_never_its_command_and_lists_one_without_a_check_unchecked() {
    let m = Machine::new(&["github"]);
    m.source(&[
        (
            "roles/signer/role.toml",
            &role(&[], &["signing", "notary", "github"]),
        ),
        ("roles/signer/core.md", "Core.\n"),
    ]);
    m.noting("signing-env", PRINTS_A_AND_B);
    m.noting("notary-env", PRINTS_A_AND_B);
    m.noting("signing-check", "exit 0\n");
    m.map_service("signing", "signing-env", Some("signing-check"));
    m.map_service("notary", "notary-env", None);

    let o = m.rolesmith(&["--json", "doctor", "signer"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    conforms(&doc, &shape("doctor"));
    assert_eq!(doc["entries"], serde_json::json!(["github"]));
    assert_eq!(
        doc["services"],
        serde_json::json!([
            { "name": "signing", "command": "signing-env", "state": "checked",
              "asks_for": "unknown", "probe": { "outcome": "unchecked" } },
            { "name": "notary", "command": "notary-env", "state": "unchecked",
              "asks_for": "unknown", "probe": { "outcome": "unchecked" } }
        ])
    );
    assert_eq!(m.ran("signing-check"), "run\n");
    assert_eq!(m.ran("signing-env"), "", "doctor never runs the command");
    assert_eq!(m.ran("notary-env"), "");
    let o = m.rolesmith(&["doctor", "signer"]);
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(out.contains("- service notary: unchecked"), "{out}");

    m.map_service("signing", "signing-env", Some("echo locked >&2; exit 4"));
    let o = m.rolesmith(&["--json", "doctor", "signer"]);
    assert_eq!(o.status.code(), Some(1));
    let missing = &json(&o)["error"]["missing"][0];
    let item = rolesmith::machine::Missing::shape();
    conforms(missing, &item);
    assert!(
        item["properties"]["kind"]["enum"]
            .as_array()
            .unwrap()
            .contains(&missing["kind"]),
        "{item}"
    );
    assert_eq!(missing["kind"], "service");
    assert_eq!(missing["name"], "signing");
    assert!(
        missing["reason"].as_str().unwrap().contains("exits 4"),
        "{missing}"
    );
}

#[test]
fn env_and_doctor_fail_on_a_services_toml_they_cannot_read_rather_than_use_mytokens() {
    let m = Machine::new(&["signing"]);
    m.source(&[
        ("roles/signer/role.toml", &role(&[], &["signing"])),
        ("roles/signer/core.md", "Core.\n"),
    ]);
    std::fs::create_dir_all(m.path("config/rolesmith/services.toml")).unwrap();
    for args in [&["env", "signer"][..], &["doctor", "signer"]] {
        let o = m.rolesmith(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}");
        assert!(stderr(&o).contains("cannot read"), "{}", stderr(&o));
    }
    std::fs::remove_dir(m.path("config/rolesmith/services.toml")).unwrap();
    std::fs::write(
        m.path("config/rolesmith/services.toml"),
        "[service.signing\n",
    )
    .unwrap();
    for args in [&["env", "signer"][..], &["doctor", "signer"]] {
        let o = m.rolesmith(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&o).contains("services.toml does not parse"),
            "{}",
            stderr(&o)
        );
    }
    assert!(
        !m.mytokens_calls().contains("get"),
        "nothing is read from mytokens"
    );
}

#[test]
fn doctor_fails_on_kept_probes_it_cannot_read_rather_than_take_none() {
    let m = Machine::new(&[]);
    signer(&m, &["signing"]);
    m.map_service("signing", "signing-env", None);
    std::fs::create_dir_all(m.path("data/rolesmith/probes.json")).unwrap();
    let o = m.rolesmith(&["doctor", "signer"]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(
        err.contains("cannot read") && err.contains("probes.json"),
        "{err}"
    );
    std::fs::remove_dir(m.path("data/rolesmith/probes.json")).unwrap();
    std::fs::write(m.path("data/rolesmith/probes.json"), "{ not json").unwrap();
    let o = m.rolesmith(&["doctor", "signer"]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(err.contains("probes.json does not parse"), "{err}");
}

impl Machine {
    /// Run `rolesmith <args>` with `vars` set (`Some`) or removed (`None`) on top of the machine's
    /// environment.
    fn rolesmith_with(&self, args: &[&str], vars: &[(&str, Option<&str>)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
        cmd.args(args);
        self.env(&mut cmd);
        for (name, value) in vars {
            match value {
                Some(v) => cmd.env(name, v),
                None => cmd.env_remove(name),
            };
        }
        cmd.output().expect("the binary runs")
    }

    /// A stand-in `name` on `PATH` that notes each run in `<name>-ran`, reading no stdin, then
    /// runs `body`.
    fn counting(&self, name: &str, body: &str) {
        let ran = self.path(&format!("{name}-ran"));
        self.program(name, &format!("echo run >> '{}'\n{body}", ran.display()));
    }

    /// Every file under the machine's directory whose text holds `needle`.
    fn files_holding(&self, needle: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut dirs = vec![self.dir.path().to_path_buf()];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let p = e.unwrap().path();
                if p.ends_with("bin") || p.ends_with("lore") || p.ends_with("lore.git") {
                    continue; // the stand-ins and the library: the test's own
                }
                if p.is_dir() {
                    dirs.push(p);
                } else if std::fs::read(&p)
                    .is_ok_and(|b| String::from_utf8_lossy(&b).contains(needle))
                {
                    found.push(p);
                }
            }
        }
        found
    }
}

const NO_TOKEN: &[(&str, Option<&str>)] = &[("OP_SERVICE_ACCOUNT_TOKEN", None)];
const OP_HINT: &str = "1Password asks for Touch ID or your password on each read; an unattended worker stalls on it. Use a service account token to read without a person";

fn signer(m: &Machine, services: &[&str]) {
    m.source(&[
        ("roles/signer/role.toml", &role(&[], services)),
        ("roles/signer/core.md", "Core.\n"),
    ]);
}

#[test]
fn doctor_and_env_name_what_a_mapped_command_asks_for_from_the_table() {
    let m = Machine::new(&[]);
    signer(&m, &["vault", "tokens", "root", "script"]);
    m.noting("op", "echo A=1\n");
    m.map_service("vault", "op inject -i ~/vault.env", None);
    m.map_service("tokens", "mytokens env tokens", None);
    m.map_service("root", "sudo cat /etc/secret.env", None);
    m.map_service("script", "~/bin/script-env", None);

    let o = m.rolesmith_with(&["--json", "doctor", "signer"], NO_TOKEN);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    conforms(&doc, &shape("doctor"));
    let asks: Vec<(Value, Value)> = doc["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["asks_for"].clone(), s["hint"].clone()))
        .collect();
    assert_eq!(
        asks,
        [
            (Value::from("a person"), Value::from(OP_HINT)),
            (Value::from("nothing"), Value::Null),
            (
                Value::from("a person"),
                Value::from("sudo asks for a password")
            ),
            (Value::from("unknown"), Value::Null),
        ]
    );
    let out = String::from_utf8(m.rolesmith_with(&["doctor", "signer"], NO_TOKEN).stdout).unwrap();
    assert!(out.contains(OP_HINT), "{out}");
    assert!(out.contains("sudo asks for a password"), "{out}");

    // With a service account token, in rolesmith's environment or in the command, op asks for nothing.
    let token = [("OP_SERVICE_ACCOUNT_TOKEN", Some("ops_x"))];
    let doc = json(&m.rolesmith_with(&["--json", "doctor", "signer"], &token));
    assert_eq!(doc["services"][0]["asks_for"], "nothing");
    assert!(doc["services"][0].get("hint").is_none(), "{doc}");
    let out = String::from_utf8(m.rolesmith_with(&["doctor", "signer"], &token).stdout).unwrap();
    assert!(!out.contains("Touch ID"), "{out}");
    m.map_service(
        "vault",
        "OP_SERVICE_ACCOUNT_TOKEN=\"$(cat ~/t)\" op inject -i ~/vault.env",
        None,
    );
    let doc = json(&m.rolesmith_with(&["--json", "doctor", "signer"], NO_TOKEN));
    assert_eq!(doc["services"][0]["asks_for"], "nothing");

    // env, which runs the command, says so on stderr before it does.
    let only_vault = Machine::new(&[]);
    signer(&only_vault, &["vault"]);
    only_vault.noting("op", "echo A=1\n");
    only_vault.map_service("vault", "op inject -i ~/vault.env", None);
    let o = only_vault.rolesmith_with(&["env", "signer"], NO_TOKEN);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        stderr(&o).contains(&format!("service vault: {OP_HINT}")),
        "{}",
        stderr(&o)
    );
    let o = only_vault.rolesmith_with(&["env", "signer"], &token);
    assert!(!stderr(&o).contains("Touch ID"), "{}", stderr(&o));
}

/// A value no answer and no file rolesmith writes may hold.
const SECRET: &str = "s3cret-value-never-shown";

#[test]
fn doctor_probe_reads_each_source_once_and_answers_how_it_went_without_a_value() {
    let m = Machine::new(&[]);
    m.source(&[
        (
            "roles/signer/role.toml",
            &role(
                &["asc"],
                &["fast", "slow", "reader", "failing", "asking", "garbled"],
            ),
        ),
        ("roles/signer/core.md", "Core.\n"),
        ("tools/asc/tool.toml", &tool("true", ASC_CREDENTIAL)),
    ]);
    m.counting("fast-env", &format!("echo A={SECRET}\n"));
    let pid = m.path("slow-child");
    m.program(
        "slow-env",
        &format!(
            "sleep 30 &\necho $! > '{}'\nsleep 30\necho A={SECRET}\n",
            pid.display()
        ),
    );
    m.program("reader-env", "read v\necho \"A=$v\"\n");
    m.program("failing-env", "echo 'vault is locked' >&2\nexit 1\n");
    m.map_service("fast", "fast-env", None);
    m.map_service("slow", "slow-env", None);
    m.map_service("reader", "reader-env", None);
    m.map_service("failing", "failing-env", None);
    m.program(
        "asking-env",
        "echo 'sudo: a terminal is required to read the password' >&2\nexit 1\n",
    );
    m.map_service("asking", "asking-env", None);
    m.program(
        "garbled-env",
        &format!("echo A={SECRET}\necho 'not a variable'\n"),
    );
    m.map_service("garbled", "garbled-env", None);
    m.program("asc-env", &format!("echo ASC_KEY_ID={SECRET}\n"));
    m.map_service("asc-api-key", "asc-env", None);

    let started = std::time::Instant::now();
    let o = m.rolesmith(&["--json", "doctor", "signer", "--probe", "--timeout", "1"]);
    assert!(
        started.elapsed().as_secs() < 10,
        "bounded: {:?}",
        started.elapsed()
    );
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    let out = String::from_utf8(o.stdout.clone()).unwrap();
    let err = stderr(&o);
    for text in [&out, &err] {
        assert!(!text.contains(SECRET), "{text}");
    }
    let missing = json(&o)["error"]["missing"].clone();
    let by_name = |name: &str| -> Value {
        missing
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == name)
            .cloned()
            .unwrap_or(Value::Null)
    };
    assert_eq!(by_name("fast"), Value::Null, "unattended is not missing");
    for name in ["slow", "reader"] {
        let reason = by_name(name)["reason"].as_str().unwrap().to_owned();
        assert!(reason.starts_with("needs a person"), "{name}: {reason}");
        assert!(reason.contains("within 1s"), "{name}: {reason}");
    }
    let reason = by_name("failing")["reason"].as_str().unwrap().to_owned();
    assert!(reason.starts_with("refused"), "{reason}");
    assert!(reason.contains("exits 1: vault is locked"), "{reason}");
    let reason = by_name("asking")["reason"].as_str().unwrap().to_owned();
    assert!(reason.starts_with("needs a person"), "{reason}");
    assert!(
        reason.contains("exits 1 asking for a terminal: sudo: a terminal is required"),
        "{reason}"
    );
    let reason = by_name("garbled")["reason"].as_str().unwrap().to_owned();
    assert!(reason.starts_with("refused"), "{reason}");
    assert!(
        reason.contains("line 2 of what it prints has no `=` after its name"),
        "{reason}"
    );
    let reason = by_name("asc-api-key")["reason"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(reason.starts_with("incomplete"), "{reason}");
    assert!(reason.contains("ASC_ISSUER_ID"), "{reason}");
    assert!(!reason.contains("ASC_KEY_ID"), "{reason}");

    // The slow source was killed with everything it started.
    let child = std::fs::read_to_string(&pid).unwrap();
    let alive = Command::new("kill")
        .args(["-0", child.trim()])
        .output()
        .unwrap();
    assert!(!alive.status.success(), "its child {child} still runs");
    assert_eq!(m.ran("fast-env"), "run\n", "read once");
    assert_eq!(m.files_holding(SECRET), Vec::<PathBuf>::new());

    // Each outcome is kept: plain doctor answers it without running a source.
    std::fs::remove_file(m.path("fast-env-ran")).unwrap();
    let o = m.rolesmith(&["--json", "doctor", "signer"]);
    assert_eq!(o.status.code(), Some(1), "needs a person refuses");
    assert_eq!(m.ran("fast-env"), "", "plain doctor runs no source");
    let missing = json(&o)["error"]["missing"].clone();
    let names: Vec<&str> = missing
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["slow", "reader", "asking"],
        "only needs a person refuses: {missing}"
    );
    let item = &missing[0];
    assert_eq!(item["kind"], "service");
    let reason = item["reason"].as_str().unwrap();
    assert!(reason.starts_with("needs a person (probed "), "{reason}");
    assert!(
        item["fix"]
            .as_str()
            .unwrap()
            .contains("service account token"),
        "{item}"
    );
}

#[test]
fn doctor_reports_a_kept_probe_until_the_command_changes() {
    let m = Machine::new(&[]);
    signer(&m, &["fast", "slow"]);
    m.counting("fast-env", "sleep 0.3\necho A=1\n");
    m.map_service("fast", "fast-env", None);
    m.map_service("slow", "sleep 5", None);

    let doc = json(&m.rolesmith(&["--json", "doctor", "signer"]));
    assert_eq!(
        doc["services"][0]["probe"],
        serde_json::json!({ "outcome": "unchecked" })
    );
    let out = String::from_utf8(m.rolesmith(&["doctor", "signer"]).stdout).unwrap();
    assert!(out.contains("rolesmith doctor --probe signer"), "{out}");

    let o = m.rolesmith(&["doctor", "signer", "--probe", "--timeout", "1"]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(err.contains("slow: needs a person"), "{err}");

    let o = m.rolesmith(&["--json", "doctor", "signer"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("slow: needs a person (probed "),
        "{}",
        stderr(&o)
    );

    // A changed command is another source: its old outcome no longer counts.
    m.map_service("slow", "echo A=2", None);
    let o = m.rolesmith(&["--json", "doctor", "signer"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc = json(&o);
    conforms(&doc, &shape("doctor"));
    assert_eq!(
        doc["services"][1]["probe"],
        serde_json::json!({ "outcome": "unchecked" })
    );
    let fast = &doc["services"][0]["probe"];
    assert_eq!(fast["outcome"], "unattended");
    let seconds = fast["seconds"].as_f64().unwrap();
    assert!((0.25..1.0).contains(&seconds), "{fast}");
    assert!(fast["at"].as_str().unwrap().ends_with('Z'), "{fast}");
    assert_eq!(m.ran("fast-env"), "run\n", "only the probe ran it");
}

#[test]
fn doctor_help_tables_what_each_source_asks_for() {
    let m = Machine::new(&[]);
    let help = String::from_utf8(m.rolesmith(&["doctor", "--help"]).stdout).unwrap();
    for row in [
        "| mytokens | nothing |",
        "| `op` with `OP_SERVICE_ACCOUNT_TOKEN` | nothing |",
        "| `op` without it | Touch ID or your password, on each read |",
        "| `sudo` | a password |",
        "| any other command | unknown: probe it |",
    ] {
        assert!(help.contains(row), "{row}\n{help}");
    }
    for (program, _, _) in rolesmith::service_map::KNOWN {
        let row = |p: &str| format!("| {p} |");
        assert!(
            help.contains(&row(program)) || help.contains(&format!("| `{program}`")),
            "the table rolesmith reads has {program}; the help's table does not:\n{help}"
        );
    }
    assert!(
        help.contains("the prompt appearing is the answer"),
        "{help}"
    );
}

/// A tool `flux` that follows its formula's stable version (`sunfmin/tap/flux`).
const FLUX: &str = "description = \"Flux\"\ncheck = \"rolesmith-test-flux --version\"\n[install]\nbrew = \"sunfmin/tap/flux\"\nversion = \"stable\"\n";

/// The fix doctor names for the role `fluxed`.
const FLUX_FIX: &str = "rolesmith tool install felix/fluxed";

impl Machine {
    /// A brew in `d` (this machine's root, or `hosts/<host>`) whose tap `sunfmin/tap` is a git
    /// clone of `d/tap.git`, offering `sunfmin/tap/flux` at the version its `flux.stable` holds.
    /// Installing or upgrading copies that version into `d/cellar` (what brew has installed) and
    /// puts `rolesmith-test-flux` on `d/bin`; while `d/locked` exists, the next one fails as brew
    /// does when another brew holds the formula's lock, and removes it; while `d/broken` exists, each
    /// fails as a broken install does; while `d/unknown` exists, brew knows no such formula. Calls
    /// go to `d/brew-calls`.
    fn stable_brew(&self, d: &Path, stable: &str) {
        let tap = d.join("Taps/sunfmin/homebrew-tap");
        let (bare, work) = (d.join("tap.git"), d.join("tap-work"));
        for dir in [&bare, &work] {
            std::fs::create_dir_all(dir).unwrap();
        }
        self.git(&bare, &["init", "--quiet", "--bare", "-b", "main"]);
        self.git(&work, &["init", "--quiet", "-b", "main"]);
        self.publish_stable(d, stable);
        self.git(
            d,
            &[
                "clone",
                "--quiet",
                bare.to_str().unwrap(),
                tap.to_str().unwrap(),
            ],
        );
        let body = r#"echo "$*" >> '@D/brew-calls'
T='@D/Taps/sunfmin/homebrew-tap'
case "$1" in
  --repo) [ "$2" = sunfmin/tap ] || exit 1; echo "$T" ;;
  info) [ "$3" = sunfmin/tap/flux ] && [ ! -f '@D/unknown' ] || { echo "No available formula $3" >&2; exit 1; }
    i=; [ -f '@D/cellar' ] && i="{\"version\":\"$(cat '@D/cellar')\"}"
    echo "{\"formulae\":[{\"versions\":{\"stable\":\"$(cat "$T/flux.stable")\"},\"installed\":[$i]}]}" ;;
  install|upgrade) [ "$2" = sunfmin/tap/flux ] || exit 1
    if [ -f '@D/broken' ]; then echo "Error: $2: no bottle" >&2; exit 1; fi
    if [ -f '@D/locked' ]; then rm '@D/locked'; echo "Error: A \`brew $1 $2\` process has already locked @D/flux.formula.lock." >&2; exit 1; fi
    cp "$T/flux.stable" '@D/cellar'; printf '#!/bin/sh\nexit 0\n' > '@D/bin/rolesmith-test-flux'; chmod +x '@D/bin/rolesmith-test-flux' ;;
  *) exit 1 ;;
esac
"#;
        let file = d.join("bin/brew");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let body = body.replace("@D", d.to_str().unwrap());
        std::fs::write(&file, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The tap of [`Machine::stable_brew`] in `d` publishes `stable`, as the tap's own repository.
    fn publish_stable(&self, d: &Path, stable: &str) {
        let work = d.join("tap-work");
        std::fs::write(work.join("flux.stable"), stable).unwrap();
        self.git(&work, &["add", "-A"]);
        self.git(&work, &["commit", "--quiet", "-m", stable]);
        let bare = d.join("tap.git");
        self.git(&work, &["push", "--quiet", bare.to_str().unwrap(), "main"]);
    }

    /// Have the tap in `d` last refreshed `hours` ago, as its stamp says.
    fn fetched_ago(&self, d: &Path, hours: u64) {
        let head = d.join("Taps/sunfmin/homebrew-tap/.git/rolesmith-refreshed");
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(hours * 3600);
        std::fs::File::options()
            .write(true)
            .open(&head)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    /// Install `version` as brew's installed flux in `d`, with its program on `d/bin`.
    fn flux_installed(&self, d: &Path, version: &str) {
        std::fs::write(d.join("cellar"), version).unwrap();
        let p = d.join("bin/rolesmith-test-flux");
        std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn flux_library(m: &Machine) {
    m.source(&[
        ("roles/fluxed/role.toml", &role(&["flux"], &[])),
        ("roles/fluxed/core.md", "Core.\n"),
        ("tools/flux/tool.toml", FLUX),
    ]);
}

#[test]
fn a_brew_tool_following_stable_is_missing_while_older_and_tool_install_upgrades_it() {
    let m = Machine::new(&[]);
    let d = m.path("");
    m.stable_brew(&d, "1.0");
    m.flux_installed(&d, "1.0");
    flux_library(&m);

    // At stable: nothing missing; the tap is refreshed (it was never fetched) and no upgrade runs.
    let o = m.rolesmith(&["--json", "doctor", "fluxed"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let o = m.rolesmith(&["--json", "tool", "install", "fluxed"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        json(&o)["tools"],
        serde_json::json!([{ "name": "flux", "method": null, "formula": null }])
    );
    assert!(!m.brew_calls().contains("upgrade"), "{}", m.brew_calls());

    // The tap publishes 2.0. Refreshed within the day, it is not fetched again: still at stable.
    m.publish_stable(&d, "2.0");
    m.fetched_ago(&d, 2);
    let o = m.rolesmith(&["doctor", "fluxed"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));

    // A day later doctor refreshes the tap, and the installed 1.0 is missing.
    m.fetched_ago(&d, 25);
    let o = m.rolesmith(&["--json", "doctor", "fluxed"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert_eq!(
        json(&o)["error"]["missing"],
        serde_json::json!([{ "kind": "program", "name": "flux",
            "reason": "check `rolesmith-test-flux --version` passes, but brew has sunfmin/tap/flux 1.0 installed, not its stable 2.0",
            "fix": FLUX_FIX }])
    );

    // tool install upgrades it, waiting out another brew that holds the formula's lock.
    std::fs::write(m.path("brew-calls"), "").unwrap();
    std::fs::write(d.join("locked"), "").unwrap();
    let o = m.rolesmith(&["--json", "tool", "install", "fluxed"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        json(&o)["tools"],
        serde_json::json!([{ "name": "flux", "method": "brew", "formula": "sunfmin/tap/flux" }])
    );
    assert_eq!(
        m.brew_calls(),
        "--repo sunfmin/tap\ninfo --json=v2 sunfmin/tap/flux\n\
         info --json=v2 sunfmin/tap/flux\nupgrade sunfmin/tap/flux\nupgrade sunfmin/tap/flux\n\
         --repo sunfmin/tap\ninfo --json=v2 sunfmin/tap/flux\n"
    );
    assert_eq!(std::fs::read_to_string(d.join("cellar")).unwrap(), "2.0");
    let o = m.rolesmith(&["doctor", "fluxed"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));

    // A tap that cannot be fetched is compared as it is, and doctor says so.
    std::fs::rename(d.join("tap.git"), d.join("tap-gone.git")).unwrap();
    m.fetched_ago(&d, 25);
    let o = m.rolesmith(&["tool", "show", "flux"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(
        human.contains("`rolesmith-test-flux --version` passes; its tap sunfmin/tap could not be refreshed, so its stable is the tap's as it was:"),
        "{human}"
    );
    let o = m.rolesmith(&["doctor", "fluxed"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let human = String::from_utf8(o.stdout).unwrap();
    assert!(human.contains("could not be refreshed"), "{human}");
}

#[test]
fn a_stable_brew_tool_is_installed_by_its_formula_alone_and_upgraded_on_another_host_there() {
    let m = Machine::new(&[]);
    let d = m.path("");
    m.stable_brew(&d, "2.0");
    flux_library(&m);
    // Its program is on PATH, but not from brew.
    m.flux_installed(&d, "");
    std::fs::remove_file(d.join("cellar")).unwrap();

    // The laptop has its own brew, with 1.0 installed: it is compared and upgraded there.
    m.ssh();
    let laptop = m.path("hosts/laptop");
    m.stable_brew(&laptop, "2.0");
    m.flux_installed(&laptop, "1.0");
    let o = m.rolesmith(&["--json", "doctor", "fluxed", "--host", "laptop"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert_eq!(
        json(&o)["error"]["missing"][0]["reason"],
        "check `rolesmith-test-flux --version` on laptop passes, but brew has sunfmin/tap/flux 1.0 installed, not its stable 2.0"
    );
    let o = m.rolesmith(&["--json", "tool", "install", "fluxed", "--host", "laptop"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        std::fs::read_to_string(laptop.join("brew-calls"))
            .unwrap()
            .contains("\nupgrade sunfmin/tap/flux\n")
    );
    assert_eq!(m.brew_calls(), "", "this machine's brew is not asked");
    let o = m.rolesmith(&["doctor", "fluxed", "--host", "laptop"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));

    // A formula brew does not know is missing, saying so.
    std::fs::write(d.join("unknown"), "").unwrap();
    let o = m.rolesmith(&["--json", "doctor", "fluxed"]);
    assert_eq!(
        json(&o)["error"]["missing"][0]["reason"],
        "check `rolesmith-test-flux --version` passes, but brew has no formula sunfmin/tap/flux: No available formula sunfmin/tap/flux"
    );
    std::fs::remove_file(d.join("unknown")).unwrap();

    let o = m.rolesmith(&["--json", "doctor", "fluxed"]);
    assert_eq!(
        json(&o)["error"]["missing"][0]["reason"],
        "check `rolesmith-test-flux --version` passes, but brew has no sunfmin/tap/flux installed, not its stable 2.0"
    );

    // A failing install is refused at once, not tried again as a held lock is.
    std::fs::write(d.join("broken"), "").unwrap();
    let o = m.rolesmith(&["--json", "tool", "install", "fluxed"]);
    assert_eq!(
        json(&o)["error"]["message"],
        "`brew install sunfmin/tap/flux` failed: Error: sunfmin/tap/flux: no bottle"
    );
    assert_eq!(m.brew_calls().matches("\ninstall ").count(), 1);
    std::fs::remove_file(d.join("broken")).unwrap();

    let o = m.rolesmith(&["--json", "tool", "install", "fluxed"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        m.brew_calls().contains("\ninstall sunfmin/tap/flux\n"),
        "{}",
        m.brew_calls()
    );
    assert!(!m.brew_calls().contains('@'), "{}", m.brew_calls());
    assert_eq!(std::fs::read_to_string(d.join("cellar")).unwrap(), "2.0");
}
