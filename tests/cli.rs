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
fn version_exits_0_with_the_stamped_version_on_stdout() {
    let o = rolesmith(&["--version"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        format!("rolesmith {}\n", env!("ROLESMITH_VERSION"))
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

#[test]
fn help_prints_the_index_each_command_under_its_group_and_each_topic() {
    let o = rolesmith(&["--help"]);
    assert_eq!(o.status.code(), Some(0));
    let index = stdout(&o);
    assert!(index.starts_with("# `rolesmith`\n"), "{index}");
    let commands = index.find("## Commands").expect("a commands section");
    let topics = index.find("## Topics").expect("a topics section");
    assert!(
        index[commands..topics].contains("### Run "),
        "commands are grouped: {index}"
    );
    let doc = one_json_document(&rolesmith(&["agent-context", "--json"]));
    for (name, entry) in commands_by_name(&doc) {
        let line = format!("- `rolesmith {name}`: {}", entry["about"].as_str().unwrap());
        assert!(
            index[commands..topics].contains(&line),
            "{line:?} in {index}"
        );
    }
    for topic in ["composing", "errors", "sources"] {
        assert!(
            index[topics..].contains(&format!("- `rolesmith help {topic}`: ")),
            "{topic} in {index}"
        );
    }
    assert!(index.contains("- `rolesmith help all`: "));
    assert_eq!(
        stdout(&rolesmith(&["-h"])),
        index,
        "-h answers what --help does"
    );
    assert_eq!(
        stdout(&rolesmith(&["help"])),
        index,
        "help alone answers the index"
    );
}

#[test]
fn help_all_is_the_whole_manual_and_each_topic_answers() {
    let o = rolesmith(&["help", "all"]);
    assert_eq!(o.status.code(), Some(0));
    let manual = stdout(&o);
    let doc = one_json_document(&rolesmith(&["agent-context", "--json"]));
    for (name, _) in commands_by_name(&doc) {
        assert!(manual.contains(&format!("`rolesmith {name}`")), "{name}");
    }
    for topic in ["composing", "errors", "sources"] {
        let o = rolesmith(&["help", topic]);
        assert_eq!(o.status.code(), Some(0), "help {topic}");
        let text = stdout(&o);
        assert!(
            text.starts_with(&format!("# `rolesmith help {topic}`\n")),
            "{text}"
        );
        let opening = text.lines().nth(2).expect("a topic opens with a sentence");
        assert!(
            manual.contains(opening),
            "the manual holds the {topic} topic"
        );
    }
}

#[test]
fn an_unknown_topic_is_a_usage_error_listing_the_topics() {
    let o = rolesmith(&["help", "no-such-topic"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let err = std::str::from_utf8(&o.stderr).unwrap();
    for topic in ["composing", "errors", "sources"] {
        assert!(err.contains(&format!("`{topic}`")), "{err}");
    }
    let o = rolesmith(&["--json", "help", "no-such-topic"]);
    assert_eq!(one_json_document(&o)["error"]["kind"], "usage");
}

#[test]
fn use_help_says_how_to_compose_roles() {
    let o = rolesmith(&["use", "--help"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).contains("(help:composing)"), "{}", stdout(&o));
    let composing = stdout(&rolesmith(&["help", "composing"])).to_owned();
    for point in [
        "Requires or use",
        "One project per combination",
        "Name it by the kind of work",
        "Its entry file",
        "Changing a combination",
        "New versions",
    ] {
        assert!(composing.contains(point), "the topic has {point:?}");
    }
    assert_eq!(stdout(&rolesmith(&["use", "-h"])), stdout(&o));
}

#[test]
fn each_command_help_says_every_key_its_json_answers() {
    let doc = one_json_document(&rolesmith(&["agent-context", "--json"]));
    for (name, entry) in commands_by_name(&doc) {
        // A command with subcommands gives each one's keys on that subcommand's page.
        let mut help = stdout(&rolesmith(&[name.as_str(), "--help"])).to_owned();
        for sub in entry["subcommands"].as_array().unwrap() {
            let sub = sub["name"].as_str().unwrap();
            help.push_str(stdout(&rolesmith(&[name.as_str(), sub, "--help"])));
        }
        let mut keys = Vec::new();
        collect_keys(&entry["output"], &mut keys);
        for key in keys {
            assert!(
                help.contains(&format!("`{key}`")),
                "{name} --help names `{key}`"
            );
        }
    }
}

/// Every property name a JSON Schema declares, at any depth.
fn collect_keys(schema: &Value, keys: &mut Vec<String>) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(props)) = map.get("properties") {
                keys.extend(props.keys().cloned());
            }
            map.values().for_each(|v| collect_keys(v, keys));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_keys(v, keys)),
        _ => {}
    }
}

#[test]
fn agent_context_texts_are_the_manuals() {
    let doc = one_json_document(&rolesmith(&["agent-context", "--json"]));
    fn walk(entry: &Value, path: &mut Vec<String>) {
        path.push(entry["name"].as_str().unwrap().to_owned());
        let args: Vec<&str> = path.iter().map(String::as_str).chain(["--help"]).collect();
        let help = stdout(&rolesmith(&args)).to_owned();
        assert_eq!(entry["long_help"].as_str(), Some(help.as_str()), "{path:?}");
        let about = entry["about"].as_str().expect("every command has an about");
        assert!(
            help.contains(about),
            "{path:?}'s about is its text's first line"
        );
        for sub in entry["subcommands"].as_array().unwrap() {
            walk(sub, path);
        }
        path.pop();
    }
    for (_, entry) in commands_by_name(&doc) {
        walk(entry, &mut Vec::new());
    }
}

#[test]
fn no_help_names_what_its_reader_cannot_open() {
    let manual = stdout(&rolesmith(&["help", "all"])).to_owned();
    for word in ["ADR", "SPEC", "docs/", "{const:", "§"] {
        assert!(!manual.contains(word), "the manual holds {word:?}");
    }
}

#[test]
fn use_help_names_every_target_with_its_directory() {
    let help = stdout(&rolesmith(&["use", "--help"])).to_owned();
    for (target, dir) in rolesmith::project::TARGETS {
        assert!(
            help.contains(&format!("`{target}` (`{dir}/`)")),
            "{target} in {help}"
        );
    }
}

#[test]
fn the_errors_topic_lists_checks_rules_and_a_problems_keys() {
    let errors = stdout(&rolesmith(&["help", "errors"])).to_owned();
    for rule in ["library-toml", "role-requires", "external-skill", "secret"] {
        assert!(
            errors.contains(&format!("- `{rule}`: ")),
            "{rule} in {errors}"
        );
    }
    for key in [
        "  - `file`: a string: relative to the library root",
        "  - `line`: an integer, only where it applies: where there is one",
        "  - `kind`: one of `program`, `entry`",
    ] {
        assert!(errors.contains(key), "{key:?} in {errors}");
    }
}
