//! `rolesmith distill` through the built binary, with a stand-in `gh` that
//! serves the role's open lessons.

mod common;

use common::{Machine, published, stderr};
use serde_json::Value;

const ROLE: &str = "app-store-releaser";

/// A machine with the source `felix` (its clone of a local library, its record naming GitHub) whose
/// role `app-store-releaser` has a core, a prohibition and a skill.
fn machine() -> Machine {
    let m = Machine::new();
    let (lib, _) = published(&m, "felix", &[ROLE]);
    m.json(&["source", "add", "felix", lib.url()]);
    std::fs::write(
        m.path("config/rolesmith/sources.toml"),
        "[source.felix]\nurl = \"https://github.com/acme/lore.git\"\n",
    )
    .unwrap();
    m
}

fn clone(m: &Machine) -> std::path::PathBuf {
    m.path("data/rolesmith/sources/felix")
}

const TWO_LESSONS: &str = r#"[
  {"number": 7, "title": "Bump the build number before uploading",
   "body": "The upload was refused: the build number was reused.\n\nEvidence: https://github.com/acme/app/pull/12\nHanded in by: agent\n",
   "url": "https://github.com/acme/lore/issues/7"},
  {"number": 9, "title": "Wait for processing before submitting",
   "body": "Submitting at once failed.\n\nEvidence: https://github.com/acme/app/pull/30\nHanded in by: felix\n",
   "url": "https://github.com/acme/lore/issues/9"}
]"#;

#[test]
fn distill_branches_and_prints_a_brief_with_both_lessons_and_the_role() {
    let m = machine();
    m.gh_answers("issue list", TWO_LESSONS);

    let doc = m.json(&["distill", ROLE]);
    assert_eq!(doc["branch"], "rolesmith/distill-app-store-releaser");
    assert_eq!(
        m.git(&clone(&m), &["branch", "--show-current"]),
        "rolesmith/distill-app-store-releaser"
    );
    assert_eq!(
        m.git(&clone(&m), &["rev-parse", "HEAD"]),
        m.git(&clone(&m), &["rev-parse", "main"])
    );

    let lessons = doc["lessons"].as_array().unwrap();
    assert_eq!(lessons.len(), 2);
    assert_eq!(lessons[0]["number"], 7);
    assert_eq!(
        lessons[0]["evidence"],
        "https://github.com/acme/app/pull/12"
    );
    assert_eq!(
        lessons[0]["body"],
        "The upload was refused: the build number was reused."
    );
    assert_eq!(
        lessons[1]["evidence"],
        "https://github.com/acme/app/pull/30"
    );

    let brief = doc["brief"].as_str().unwrap();
    for wanted in [
        // both lessons: number, title, body, evidence
        "#7 Bump the build number before uploading",
        "The upload was refused: the build number was reused.",
        "https://github.com/acme/app/pull/12",
        "#9 Wait for processing before submitting",
        "Submitting at once failed.",
        "https://github.com/acme/app/pull/30",
        // the role's current text
        "First rule.",
        "Never revoke a certificate (why: It breaks other machines)",
        "`submit-build`: Use when a build is ready for review.",
        // what to do
        "rolesmith lesson close felix#<n> --why",
        "rolesmith check app-store-releaser",
        "rolesmith propose app-store-releaser --closes <n>",
    ] {
        assert!(
            brief.contains(wanted),
            "the brief lacks {wanted:?}:\n{brief}"
        );
    }
    assert!(!brief.contains("Handed in by:"), "{brief}");

    // The human form is the brief.
    m.git(&clone(&m), &["checkout", "--quiet", "main"]);
    m.git(
        &clone(&m),
        &["branch", "-D", "rolesmith/distill-app-store-releaser"],
    );
    let o = m.rolesmith(&["distill", ROLE]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(String::from_utf8(o.stdout).unwrap(), brief);

    // gh was asked for this role's open lessons only.
    let (args, _) = m.gh_calls().remove(0);
    let labels: Vec<&str> = args
        .windows(2)
        .filter(|w| w[0] == "--label")
        .map(|w| w[1].as_str())
        .collect();
    assert_eq!(labels, ["lesson", "role:app-store-releaser"]);
    assert!(args.windows(2).any(|w| w == ["--state", "open"]));
    assert!(args.windows(2).any(|w| w == ["--repo", "acme/lore"]));
}

#[test]
fn with_no_open_lessons_distill_says_so_and_makes_no_branch() {
    let m = machine();
    m.gh_answers("issue list", "[]");

    let o = m.rolesmith(&["distill", ROLE]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("no open lessons"),
        "{o:?}"
    );
    let doc = m.json(&["distill", ROLE]);
    assert_eq!(doc["branch"], Value::Null);
    assert_eq!(doc["brief"], Value::Null);
    assert_eq!(doc["lessons"], serde_json::json!([]));
    assert_eq!(m.git(&clone(&m), &["branch", "--show-current"]), "main");
    assert_eq!(m.git(&clone(&m), &["branch", "--list", "rolesmith/*"]), "");
}

#[test]
fn distill_refuses_uncommitted_changes_and_a_branch_that_exists() {
    let m = machine();
    m.gh_answers("issue list", TWO_LESSONS);

    std::fs::write(clone(&m).join("stray.txt"), "x").unwrap();
    let o = m.rolesmith(&["distill", ROLE]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("uncommitted changes"), "{}", stderr(&o));
    assert_eq!(m.git(&clone(&m), &["branch", "--show-current"]), "main");
    std::fs::remove_file(clone(&m).join("stray.txt")).unwrap();

    m.json(&["distill", ROLE]);
    m.git(&clone(&m), &["checkout", "--quiet", "main"]);
    let o = m.rolesmith(&["distill", ROLE]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("rolesmith/distill-app-store-releaser"));
}
