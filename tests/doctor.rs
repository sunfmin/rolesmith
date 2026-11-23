//! `doctor`, `tool show`, `tool install` and `env` (SPEC.md §3.5, §7.1, §8) through the built
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
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rolesmith"));
        cmd.args(args);
        self.env(&mut cmd);
        let o = cmd.output().expect("the binary runs");
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
    conforms(&doc, &shape("doctor"));
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
fn env_writes_no_file_under_home_the_xdg_dirs_or_the_cwd() {
    let m = Machine::new(&["asc-api-key", "github"]);
    env_library(&m, &[]);
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
    assert_eq!(m.snapshot(&dirs), before);
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
            { "name": "signing", "command": "signing-env", "state": "checked" },
            { "name": "notary", "command": "notary-env", "state": "unchecked" }
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
}
