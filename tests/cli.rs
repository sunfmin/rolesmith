//! The frame of SPEC.md §8, through the built binary.

use std::process::{Command, Output};

use serde_json::Value;

fn rolesmith(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rolesmith"))
        .args(args)
        .env_remove("RUST_BACKTRACE")
        .output()
        .expect("the binary runs")
}

fn stdout(o: &Output) -> &str {
    std::str::from_utf8(&o.stdout).unwrap()
}

fn one_json_document(o: &Output) -> Value {
    serde_json::from_str(stdout(o))
        .unwrap_or_else(|e| panic!("stdout is not one JSON document ({e}): {}", stdout(o)))
}

fn commands_by_name(doc: &Value) -> Vec<(String, &Value)> {
    doc["commands"]
        .as_array()
        .expect("commands is an array")
        .iter()
        .map(|c| (c["name"].as_str().unwrap().to_owned(), c))
        .collect()
}

#[test]
fn agent_context_lists_every_registered_command_with_its_flags_and_output_shape() {
    let o = rolesmith(&["agent-context", "--json"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stderr.is_empty(), "nothing on stderr on success");
    let doc = one_json_document(&o);
    let listed = commands_by_name(&doc);

    let registered = rolesmith::commands::output_shapes();
    assert!(!registered.is_empty());
    for (name, shape) in &registered {
        let (_, entry) = listed
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("registered command {name} is missing from agent-context"));
        assert_eq!(&entry["output"], shape, "{name}'s output shape");
        assert!(entry["flags"].is_array(), "{name} lists its flags");
    }
    // and nothing listed that the parser does not know
    let parser = rolesmith::cli::command();
    let parsed: Vec<&str> = parser
        .get_subcommands()
        .map(|c| c.get_name())
        .filter(|n| *n != "help")
        .collect();
    let mut names: Vec<&str> = listed.iter().map(|(n, _)| n.as_str()).collect();
    names.sort();
    let mut expected = parsed.clone();
    expected.sort();
    assert_eq!(names, expected);
    assert_eq!(
        registered.len(),
        parsed.len(),
        "every parsed command is registered"
    );
}

#[test]
fn agent_context_lists_the_json_flag_on_each_command() {
    let doc = one_json_document(&rolesmith(&["agent-context", "--json"]));
    for (name, entry) in commands_by_name(&doc) {
        let flags = entry["flags"].as_array().unwrap();
        assert!(
            flags
                .iter()
                .any(|f| f["long"] == "json" && f["global"] == true),
            "{name} takes --json"
        );
    }
    assert_eq!(doc["conventions"]["exit_codes"]["2"], "usage error");
}

#[test]
fn agent_context_without_json_still_prints_the_json_document() {
    let o = rolesmith(&["agent-context"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stderr.is_empty());
    let human = one_json_document(&o);
    let json = one_json_document(&rolesmith(&["agent-context", "--json"]));
    assert_eq!(human, json);
    assert!(
        stdout(&o).lines().count() > 1,
        "the human form is pretty-printed"
    );
}

#[test]
fn json_output_is_exactly_one_line_document() {
    let o = rolesmith(&["--json", "agent-context"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o).lines().count(), 1);
    one_json_document(&o);
}

#[test]
fn version_exits_0_with_the_package_version_on_stdout() {
    let o = rolesmith(&["--version"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        format!("rolesmith {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(o.stderr.is_empty());
}

#[test]
fn an_unknown_flag_is_a_usage_error_exit_2_on_stderr() {
    let o = rolesmith(&["agent-context", "--no-such-flag"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty(), "without --json nothing on stdout");
    assert!(String::from_utf8_lossy(&o.stderr).contains("--no-such-flag"));

    let o = rolesmith(&["--no-such-flag"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn a_usage_error_with_json_prints_one_error_document() {
    let o = rolesmith(&["agent-context", "--no-such-flag", "--json"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(!o.stderr.is_empty());
    let doc = one_json_document(&o);
    assert_eq!(doc["error"]["kind"], "usage");
}

#[test]
fn an_unknown_command_or_none_is_a_usage_error() {
    assert_eq!(rolesmith(&["no-such-command"]).status.code(), Some(2));
    let o = rolesmith(&[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
}

#[test]
fn help_exits_0_on_stdout() {
    let o = rolesmith(&["--help"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).contains("agent-context"));
}
