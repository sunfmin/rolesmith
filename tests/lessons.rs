//! `rolesmith lesson add|list|close` through the built binary, with a stand-in
//! `gh` that records what it was asked and answers what each test leaves it.

mod common;

use common::{Machine, published, stderr};
use serde_json::Value;

/// A machine with the source `felix`, whose role `app-store-releaser` exists, recorded at the
/// GitHub URL `url` (its clone is of a local library: only the record names GitHub).
fn machine_with_source_at(url: &str) -> Machine {
    let m = Machine::new();
    let (lib, _) = published(&m, "felix", &["app-store-releaser"]);
    m.json(&["source", "add", "felix", lib.url()]);
    std::fs::write(
        m.path("config/rolesmith/sources.toml"),
        format!("[source.felix]\nurl = \"{url}\"\n"),
    )
    .unwrap();
    m
}

fn on_github() -> Machine {
    machine_with_source_at("https://github.com/acme/lore.git")
}

/// The value after `flag` in a call's arguments, each time it is given.
fn values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].as_str())
        .collect()
}

fn verb(args: &[String]) -> String {
    format!("{} {}", args[0], args[1])
}

/// Assert `doc` has the shape agent-context advertises for `lesson <action>`: every required key
/// present, all the way down.
fn assert_shaped(m: &Machine, action: &str, doc: &Value) {
    let context = m.json(&["agent-context"]);
    let lesson = context["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "lesson")
        .unwrap();
    let variant = lesson["output"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["title"] == action)
        .unwrap_or_else(|| panic!("no shape for lesson {action}"));
    fn conforms(schema: &Value, doc: &Value, at: &str) {
        for key in schema["required"].as_array().into_iter().flatten() {
            let key = key.as_str().unwrap();
            assert!(doc.get(key).is_some(), "{at} lacks {key}: {doc}");
            conforms(
                &schema["properties"][key],
                &doc[key],
                &format!("{at}.{key}"),
            );
        }
        for item in doc.as_array().into_iter().flatten() {
            conforms(&schema["items"], item, &format!("{at}[]"));
        }
    }
    assert!(
        variant["required"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
    );
    conforms(variant, doc, action);
}

const LESSON: &str = "Bump the build number before uploading\n\nThe upload was refused because the \
                      build number was reused.\nRule: bump it first.\n";
const EVIDENCE: &str = "https://github.com/acme/app/pull/12";

#[test]
fn add_opens_an_issue_with_title_body_and_labels() {
    let m = on_github();
    m.gh_answers("repo view", r#"{"visibility":"PUBLIC"}"#);
    m.gh_answers("label list", r#"[{"name":"lesson"},{"name":"bug"}]"#);
    m.gh_answers("issue create", "https://github.com/acme/lore/issues/7\n");

    let o = m.rolesmith_input(
        &[
            "--json",
            "lesson",
            "add",
            "app-store-releaser",
            "--evidence",
            EVIDENCE,
            "--source",
            "felix",
        ],
        LESSON,
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let doc: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_shaped(&m, "add", &doc);
    assert_eq!(doc["opened"]["number"], 7);
    assert_eq!(doc["opened"]["repo"], "acme/lore");
    assert_eq!(doc["opened"]["role"], "app-store-releaser");
    assert_eq!(doc["opened"]["by"], "felix");
    assert_eq!(
        doc["opened"]["title"],
        "Bump the build number before uploading"
    );
    assert!(
        doc["reminder"]
            .as_str()
            .unwrap()
            .contains("in general terms")
    );

    let calls = m.gh_calls();
    let verbs: Vec<String> = calls.iter().map(|(a, _)| verb(a)).collect();
    assert_eq!(
        verbs,
        ["repo view", "label list", "label create", "issue create"]
    );
    for (args, _) in &calls {
        assert_eq!(values(args, "--repo"), ["acme/lore"], "{args:?}");
    }
    // Only the missing label is created.
    assert_eq!(calls[2].0[2], "role:app-store-releaser");

    let (create, body) = &calls[3];
    assert_eq!(
        values(create, "--title"),
        ["Bump the build number before uploading"]
    );
    assert_eq!(
        values(create, "--label"),
        ["lesson", "role:app-store-releaser"]
    );
    assert_eq!(values(create, "--body-file"), ["-"]);
    assert_eq!(
        body,
        &format!(
            "The upload was refused because the build number was reused.\nRule: bump it first.\n\n\
             Evidence: {EVIDENCE}\nHanded in by: felix\n"
        )
    );
}

#[test]
fn a_private_library_gets_no_reminder_and_both_labels_are_created_when_missing() {
    let m = on_github();
    m.gh_answers("repo view", r#"{"visibility":"PRIVATE"}"#);
    m.gh_answers("label list", "[]");
    m.gh_answers("issue create", "https://github.com/acme/lore/issues/3\n");

    let o = m.rolesmith_input(
        &[
            "lesson",
            "add",
            "felix/app-store-releaser",
            "--evidence",
            EVIDENCE,
        ],
        "Only a title\n",
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = String::from_utf8(o.stdout).unwrap();
    assert_eq!(
        out,
        "opened lesson #3 on acme/lore: https://github.com/acme/lore/issues/3\n"
    );

    let calls = m.gh_calls();
    let created: Vec<&str> = calls
        .iter()
        .filter(|(a, _)| verb(a) == "label create")
        .map(|(a, _)| a[2].as_str())
        .collect();
    assert_eq!(created, ["lesson", "role:app-store-releaser"]);
    let descriptions: Vec<&str> = calls
        .iter()
        .filter(|(a, _)| verb(a) == "label create")
        .flat_map(|(a, _)| values(a, "--description"))
        .collect();
    assert_eq!(
        descriptions,
        [
            "A lesson for a role, waiting to be distilled (rolesmith)",
            "A lesson for the role app-store-releaser"
        ]
    );
    let body = &calls.last().unwrap().1;
    assert_eq!(
        body,
        &format!("Evidence: {EVIDENCE}\nHanded in by: agent\n")
    );
}

#[test]
fn a_planted_key_is_refused_without_echo_and_nothing_reaches_gh() {
    let m = on_github();
    let key = format!("ghp_{}", "Q".repeat(36));
    let text = format!("Use the token\n\nIt was {key} all along.\n");
    let o = m.rolesmith_input(
        &[
            "--json",
            "lesson",
            "add",
            "app-store-releaser",
            "--evidence",
            EVIDENCE,
        ],
        &text,
    );
    assert_eq!(o.status.code(), Some(1));
    let (err, out) = (stderr(&o), String::from_utf8(o.stdout).unwrap());
    assert!(
        err.contains("line 3") && err.contains("GitHub token"),
        "{err}"
    );
    assert!(!out.contains(&key) && !err.contains(&key), "{out}{err}");

    let o = m.rolesmith_input(
        &[
            "lesson",
            "add",
            "app-store-releaser",
            "--evidence",
            &format!("https://example.com/?token={key}"),
        ],
        "A title\n",
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(!stderr(&o).contains(&key), "{}", stderr(&o));

    let o = m.rolesmith(&["lesson", "close", "4", "--why", &key]);
    assert_eq!(o.status.code(), Some(1));
    assert!(!stderr(&o).contains(&key), "{}", stderr(&o));

    assert!(m.gh_calls().is_empty(), "{:?}", m.gh_calls());
}

#[test]
fn a_source_whose_remote_is_not_github_is_refused_with_the_reason() {
    let m = machine_with_source_at("https://gitlab.com/acme/lore.git");
    let o = m.rolesmith_input(
        &[
            "lesson",
            "add",
            "app-store-releaser",
            "--evidence",
            EVIDENCE,
        ],
        LESSON,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("is not a GitHub repository"),
        "{}",
        stderr(&o)
    );
    for args in [
        &["lesson", "list", "app-store-releaser"][..],
        &["lesson", "close", "4", "--why", "wrong"][..],
    ] {
        let o = m.rolesmith(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&o).contains("is not a GitHub repository"),
            "{args:?}"
        );
    }

    // A sweep of every source skips it and says so.
    let listed = m.json(&["lesson", "list"]);
    assert_eq!(listed["lessons"], serde_json::json!([]));
    assert_eq!(listed["skipped"][0]["source"], "felix");
    assert!(m.gh_calls().is_empty());

    let o = m.rolesmith_input(
        &[
            "lesson",
            "add",
            "app-store-releaser",
            "--evidence",
            "not a link",
        ],
        LESSON,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("is not a link"), "{}", stderr(&o));
    let o = m.rolesmith_input(
        &[
            "lesson",
            "add",
            "app-store-releaser",
            "--evidence",
            EVIDENCE,
        ],
        "  \n",
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("empty"), "{}", stderr(&o));
}

#[test]
fn list_shows_open_lessons_with_number_role_title_and_age() {
    let m = on_github();
    m.gh_answers(
        "issue list",
        r#"[{"number":7,"title":"Bump the build number","createdAt":"2020-01-01T00:00:00Z",
             "url":"https://github.com/acme/lore/issues/7",
             "labels":[{"name":"lesson"},{"name":"role:app-store-releaser"}]}]"#,
    );

    let listed = m.json(&["lesson", "list"]);
    assert_shaped(&m, "list", &listed);
    let lesson = &listed["lessons"][0];
    assert_eq!(lesson["number"], 7);
    assert_eq!(lesson["role"], "app-store-releaser");
    assert_eq!(lesson["title"], "Bump the build number");
    assert_eq!(lesson["created_at"], "2020-01-01T00:00:00Z");
    let age = lesson["age"].as_str().unwrap();
    assert!(
        age.ends_with('d')
            && (2000..10_000).contains(&age.trim_end_matches('d').parse::<u32>().unwrap()),
        "{age}"
    );
    assert_eq!(listed["skipped"], serde_json::json!([]));

    let human = String::from_utf8(m.rolesmith(&["lesson", "list"]).stdout).unwrap();
    assert_eq!(
        human,
        format!("felix#7\tapp-store-releaser\tBump the build number\t{age}\n")
    );

    m.json(&["lesson", "list", "app-store-releaser"]);
    let calls = m.gh_calls();
    let (all, one) = (&calls[0].0, &calls[2].0);
    assert_eq!(verb(all), "issue list");
    assert_eq!(values(all, "--state"), ["open"]);
    assert_eq!(values(all, "--label"), ["lesson"]);
    assert_eq!(values(all, "--repo"), ["acme/lore"]);
    assert_eq!(
        values(one, "--label"),
        ["lesson", "role:app-store-releaser"]
    );

    m.gh_answers("issue list", "[]");
    let none = String::from_utf8(m.rolesmith(&["lesson", "list"]).stdout).unwrap();
    assert_eq!(none, "no open lessons\n");
}

#[test]
fn close_comments_the_reason_and_closes_only_an_open_lesson() {
    let m = on_github();
    m.gh_answers(
        "issue view",
        r#"{"state":"OPEN","url":"https://github.com/acme/lore/issues/4","labels":[{"name":"lesson"}]}"#,
    );
    let closed = m.json(&[
        "lesson",
        "close",
        "4",
        "--why",
        "It contradicts the core rule",
    ]);
    assert_shaped(&m, "close", &closed);
    assert_eq!(closed["closed"]["number"], 4);
    assert_eq!(
        closed["closed"]["url"],
        "https://github.com/acme/lore/issues/4"
    );
    let calls = m.gh_calls();
    let close = &calls[1].0;
    assert_eq!(&close[..3], ["issue", "close", "4"]);
    assert_eq!(values(close, "--comment"), ["It contradicts the core rule"]);
    assert_eq!(values(close, "--repo"), ["acme/lore"]);

    // The same, addressed with its source.
    m.json(&["lesson", "close", "felix#4", "--why", "x"]);

    // An issue that is not a lesson, or one already closed, is not closed.
    m.gh_answers(
        "issue view",
        r#"{"state":"OPEN","url":"u","labels":[{"name":"bug"}]}"#,
    );
    let o = m.rolesmith(&["lesson", "close", "5", "--why", "x"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("is not a lesson"), "{}", stderr(&o));
    m.gh_answers(
        "issue view",
        r#"{"state":"CLOSED","url":"u","labels":[{"name":"lesson"}]}"#,
    );
    let o = m.rolesmith(&["lesson", "close", "5", "--why", "x"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("already closed"), "{}", stderr(&o));
    let closes = m
        .gh_calls()
        .iter()
        .filter(|(a, _)| verb(a) == "issue close")
        .count();
    assert_eq!(closes, 2);

    for (bad, says) in [
        ("x", "not a lesson number"),
        ("0", "not a lesson number"),
        ("nobody#4", "no source named"),
    ] {
        let o = m.rolesmith(&["lesson", "close", bad, "--why", "x"]);
        assert_eq!(o.status.code(), Some(1));
        assert!(stderr(&o).contains(says), "{bad}: {}", stderr(&o));
    }
}

#[test]
fn a_bare_lesson_number_needs_its_source_when_there_are_two() {
    let m = on_github();
    let (team, _) = published(&m, "team", &["rust-cli"]);
    m.json(&["source", "add", "team", team.url()]);
    let o = m.rolesmith(&["lesson", "close", "4", "--why", "x"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("<source>#4"), "{}", stderr(&o));
    assert!(m.gh_calls().is_empty());
}

#[test]
fn what_gh_fails_or_answers_amiss_is_a_failure_that_says_so() {
    let m = on_github();
    m.gh_answers("issue list", "not json");
    let o = m.rolesmith(&["lesson", "list"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("gh issue list") && stderr(&o).contains("answered what is not JSON"),
        "{}",
        stderr(&o)
    );

    m.gh_fails("issue list", "HTTP 401: Bad credentials");
    let o = m.rolesmith(&["lesson", "list"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("--repo acme/lore failed: HTTP 401: Bad credentials"),
        "{}",
        stderr(&o)
    );

    m.gh_answers("repo view", r#"{"visibility":"PRIVATE"}"#);
    m.gh_answers(
        "label list",
        r#"[{"name":"lesson"},{"name":"role:app-store-releaser"}]"#,
    );
    m.gh_answers("issue create", "created\n");
    let o = m.rolesmith_input(
        &[
            "lesson",
            "add",
            "app-store-releaser",
            "--evidence",
            EVIDENCE,
        ],
        LESSON,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("gh issue create answered created, not an issue's URL"),
        "{}",
        stderr(&o)
    );
}
