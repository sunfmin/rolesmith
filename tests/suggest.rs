//! `suggest` and `use --auto` (SPEC.md §5.5) through the built binary: roles found for a project
//! by what the project is.

mod common;

use std::path::Path;

use common::{Machine, stderr};

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A source `felix` with an iOS role matched by `*.xcodeproj` or `**/*.xcworkspace`, a Swift-package role matched by
/// what Package.swift contains, and a role with no `[match]`.
fn machine() -> Machine {
    let m = Machine::new();
    let lib = m.library("felix");
    lib.write("library.toml", "name = \"felix\"\n");
    for role in ["ios-dev", "swift-pkg", "plain"] {
        lib.role(role, &format!("The {role}"), "# Core\n\nFirst rule.\n");
    }
    let toml = |role: &str, rules: &str| {
        let path = lib.work.join(format!("roles/{role}/role.toml"));
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str(rules);
        std::fs::write(path, text).unwrap();
    };
    toml(
        "ios-dev",
        "\n[match]\nfiles = [\"*.xcodeproj\", \"**/*.xcworkspace\"]\n",
    );
    toml(
        "swift-pkg",
        "\n[match]\ncontains = [{ file = \"Package.swift\", text = \".iOS(\" }]\n",
    );
    m.publish(&lib, "roles");
    m.json(&["source", "add", "felix", lib.url()]);
    m
}

fn suggest(m: &Machine, dir: &Path) -> serde_json::Value {
    m.json(&["suggest", "--dir", dir.to_str().unwrap()])
}

#[test]
fn an_ios_project_matches_by_its_xcodeproj_and_a_rust_one_does_not() {
    let m = machine();
    let ios = m.path("ios");
    write(&ios, "App.xcodeproj/project.pbxproj", "// pbx\n");
    write(&ios, "App/main.swift", "print(1)\n");
    let rust = m.path("rust");
    write(&rust, "Cargo.toml", "[package]\nname = \"x\"\n");
    write(&rust, "src/main.rs", "fn main() {}\n");
    // Only at the root: `*` does not cross `/`; target/ is never walked.
    write(&rust, "vendor/Old.xcodeproj/project.pbxproj", "// pbx\n");
    std::fs::create_dir_all(rust.join("target/T.xcodeproj")).unwrap();

    let out = suggest(&m, &ios);
    assert_eq!(
        out["roles"],
        serde_json::json!([{
            "address": "felix/ios-dev", "source": "felix", "role": "ios-dev",
            "rule": "files = \"*.xcodeproj\"", "path": "App.xcodeproj",
        }])
    );
    let o = m.rolesmith(&["suggest", "--dir", ios.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        "felix/ios-dev\tfiles = \"*.xcodeproj\" (App.xcodeproj)\n"
    );

    assert_eq!(suggest(&m, &rust)["roles"], serde_json::json!([]));
    let o = m.rolesmith(&["suggest", "--dir", rust.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        "no role matches this project\n"
    );
}

#[test]
fn gitignored_and_skipped_directories_are_not_walked() {
    let m = machine();
    let p = m.path("p");
    write(&p, ".gitignore", "/build/\n");
    // `**` crosses `/`, but not into what is ignored or skipped.
    std::fs::create_dir_all(p.join("build/A.xcworkspace")).unwrap();
    std::fs::create_dir_all(p.join("node_modules/B.xcworkspace")).unwrap();
    std::fs::create_dir_all(p.join(".git/C.xcworkspace")).unwrap();
    assert_eq!(suggest(&m, &p)["roles"], serde_json::json!([]));
    std::fs::create_dir_all(p.join("ios/D.xcworkspace")).unwrap();
    let out = suggest(&m, &p);
    assert_eq!(out["roles"][0]["rule"], "files = \"**/*.xcworkspace\"");
    assert_eq!(out["roles"][0]["path"], "ios/D.xcworkspace");
}

#[test]
fn use_auto_installs_exactly_what_suggest_printed() {
    let m = machine();
    let p = m.path("app");
    write(&p, "App.xcodeproj/project.pbxproj", "// pbx\n");
    write(&p, "Package.swift", "platforms: [.iOS(.v17)]\n");

    let suggested = suggest(&m, &p);
    let addresses: Vec<String> = suggested["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["address"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(addresses, ["felix/ios-dev", "felix/swift-pkg"]);
    assert_eq!(
        suggested["roles"][1]["rule"],
        "contains = { file = \"Package.swift\", text = \".iOS(\" }"
    );

    let out = m.json(&[
        "use",
        "--auto",
        "--into",
        "claude-code",
        "--project",
        p.to_str().unwrap(),
    ]);
    let installed: Vec<String> = out["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            format!(
                "{}/{}",
                r["source"].as_str().unwrap(),
                r["name"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(installed, addresses);
    assert!(
        p.join(".claude/skills/rolesmith-ios-dev/SKILL.md")
            .is_file()
    );
    assert!(!p.join(".claude/skills/rolesmith-plain").exists());
}

#[test]
fn use_auto_with_no_match_installs_nothing_and_says_so() {
    let m = machine();
    let p = m.path("empty");
    std::fs::create_dir_all(&p).unwrap();
    let o = m.rolesmith(&[
        "use",
        "--auto",
        "--into",
        "claude-code",
        "--project",
        p.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("no role matches"), "{}", stderr(&o));
    assert!(!p.join("rolesmith.toml").exists());

    let o = m.rolesmith(&["use", "ios-dev", "--auto", "--into", "claude-code"]);
    assert_eq!(o.status.code(), Some(2));
}
