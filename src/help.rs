//! The manual: every command's help, written as Markdown on the type that is its one home, the
//! topics that belong to no command, and the codes and numbers they place by name. `rolesmith
//! --help` answers its index, `rolesmith <command> --help` a command, `rolesmith help <topic>` a
//! topic, `rolesmith help all` all of it.

use agent_help::{Help, Topic};
use clap::CommandFactory;
use serde_json::Value;

use crate::cli::{Cli, EXIT_FAILED, EXIT_OK, EXIT_USAGE, ErrorKind};
use crate::commands::output_shapes;

#[doc = include_str!("help/composing.md")]
#[derive(Topic)]
#[topic(name = "composing")]
pub struct Composing;

#[doc = include_str!("help/errors.md")]
#[derive(Topic)]
#[topic(name = "errors")]
pub struct Errors;

#[doc = include_str!("help/sources.md")]
#[derive(Topic)]
#[topic(name = "sources")]
pub struct Sources;

/// The manual, assembled from the parser's command tree, each command's `--json` answer, the
/// topics, the tables and the constants.
pub fn manual() -> Help {
    Help::new(tree())
        .topic::<Composing>()
        .topic::<Errors>()
        .topic::<Sources>()
        .table::<ErrorKind>("error-kind")
        .constant("EXIT_OK", EXIT_OK)
        .constant("EXIT_FAILED", EXIT_FAILED)
        .constant("EXIT_USAGE", EXIT_USAGE)
        .constant("RULES", crate::problem::Rule::help().trim_end())
        .constant(
            "PROBLEM",
            indented(&keys(&crate::problem::Problem::shape())),
        )
        .constant(
            "MISSING",
            indented(&keys(&crate::machine::Missing::shape())),
        )
        .constant("TARGETS", targets())
}

/// The parser's command tree, each registered command's text followed by what its `--json` prints
/// on success, rendered from its output shape: a key is written once, in that shape.
fn tree() -> clap::Command {
    let mut root = Cli::command();
    for (name, shape) in output_shapes() {
        root = root.mut_subcommand(name, |command| {
            let alternatives = shape["oneOf"].as_array().cloned().unwrap_or_default();
            let mut command = with_answer(command, &shape);
            for alternative in alternatives {
                if let Some(title) = alternative["title"].as_str() {
                    command = command.mut_subcommand(title, |sub| with_answer(sub, &alternative));
                }
            }
            command
        });
    }
    root
}

/// `command` with a section saying what `--json` prints on success, from `shape`. A command
/// without text of its own is left without, so `agent_help::check` refuses it.
fn with_answer(command: clap::Command, shape: &Value) -> clap::Command {
    let Some(text) = command
        .get_long_about()
        .or(command.get_about())
        .map(|t| t.to_string())
    else {
        return command;
    };
    let mut answer = String::from("## With `--json`\n\n");
    if let Some(description) = shape["description"].as_str() {
        answer.push_str(&sentence(description));
        answer.push_str("\n\n");
    }
    if shape["oneOf"].is_array() {
        answer.push_str("Each subcommand's `--help` lists the keys it prints.\n");
    } else {
        answer.push_str("It prints one JSON object on one line:\n\n");
        answer.push_str(&keys(shape));
        let defs = shape["$defs"].as_object().into_iter().flatten();
        for (name, def) in defs {
            answer.push_str(&format!("\nEach `{name}` object has:\n\n{}", keys(def)));
        }
    }
    let text = format!("{}\n\n{answer}", text.trim_end());
    command.long_about(text)
}

/// The keys a JSON Schema object declares, as a Markdown list, nested objects indented under
/// their key. Ends with a newline; empty for a schema that declares none.
pub fn keys(schema: &Value) -> String {
    let mut list = String::new();
    push_keys(&mut list, schema, "");
    list
}

fn push_keys(list: &mut String, schema: &Value, indent: &str) {
    let required: Vec<&str> = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let properties = schema["properties"].as_object().into_iter().flatten();
    for (key, property) in properties {
        let mut line = format!("{indent}- `{key}`: {}", kind(property));
        if !required.contains(&key.as_str()) {
            line.push_str(", only where it applies");
        }
        if let Some(description) = property["description"].as_str() {
            line.push_str(": ");
            line.push_str(description);
        }
        list.push_str(&line);
        list.push('\n');
        let inner = format!("{indent}  ");
        push_keys(list, property, &inner);
        push_keys(list, &property["items"], &inner);
        if let Some(extra) = property["additionalProperties"]
            .as_object()
            .and_then(|o| o.get("description"))
            .and_then(Value::as_str)
        {
            list.push_str(&format!("{inner}- any other key: {extra}\n"));
        }
        let items_extra = property["items"]["additionalProperties"]["description"].as_str();
        if let Some(extra) = items_extra {
            list.push_str(&format!("{inner}- any other key: {extra}\n"));
        }
    }
}

/// What a property is: its type (a list of what, nullable), its fixed value or its choices.
fn kind(property: &Value) -> String {
    if let Some(value) = property.get("const") {
        return format!("always {value}");
    }
    if let Some(choices) = property["enum"].as_array() {
        let choices: Vec<String> = choices
            .iter()
            .map(|c| format!("`{}`", unquoted(c)))
            .collect();
        return format!("one of {}", choices.join(", "));
    }
    if let Some(reference) = property["$ref"].as_str() {
        return format!(
            "a `{}` object",
            reference.rsplit('/').next().unwrap_or(reference)
        );
    }
    let types: Vec<&str> = match &property["type"] {
        Value::String(t) => vec![t.as_str()],
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let mut words: Vec<String> = types
        .iter()
        .filter(|t| **t != "null")
        .map(|t| match *t {
            "array" => format!("a list of {}", plural(&property["items"])),
            "object" => "an object".to_owned(),
            "integer" => "an integer".to_owned(),
            "boolean" => "a boolean".to_owned(),
            other => format!("a {other}"),
        })
        .collect();
    if types.contains(&"null") {
        words.push("`null`".to_owned());
    }
    if words.is_empty() {
        return "any value".to_owned();
    }
    words.join(" or ")
}

/// What a list holds, for "a list of ...".
fn plural(items: &Value) -> String {
    if let Some(reference) = items["$ref"].as_str() {
        return format!(
            "`{}` objects",
            reference.rsplit('/').next().unwrap_or(reference)
        );
    }
    match items["type"].as_str() {
        Some("object") => "objects".to_owned(),
        Some("integer") => "integers".to_owned(),
        Some(other) => format!("{other}s"),
        None => "values".to_owned(),
    }
}

fn unquoted(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// `text` with its first letter capitalised and a closing period.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let first = chars
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    let rest = chars.as_str().trim_end_matches('.');
    format!("{first}{rest}.")
}

/// A list nested one level, to follow its placeholder's own indent under an item of another list.
fn indented(list: &str) -> String {
    list.trim_end().replace('\n', "\n  ")
}

/// The targets `--into` takes, each with the directory it installs into.
fn targets() -> String {
    crate::project::TARGETS
        .iter()
        .map(|(name, dir)| format!("`{name}` (`{dir}/`)"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manual_is_whole() {
        agent_help::check(&manual()).unwrap();
    }

    #[test]
    fn keys_lists_nested_and_optional_keys() {
        let shape = serde_json::json!({
            "type": "object",
            "required": ["roles"],
            "properties": {
                "roles": { "type": "array", "items": {
                    "type": "object", "required": ["name"],
                    "properties": { "name": { "type": "string" }, "rev": { "type": ["string", "null"], "description": "full SHA" } }
                } },
                "kind": { "enum": ["a", "b"] }
            }
        });
        assert_eq!(
            keys(&shape),
            "- `kind`: one of `a`, `b`, only where it applies\n\
             - `roles`: a list of objects\n  \
             - `name`: a string\n  \
             - `rev`: a string or `null`, only where it applies: full SHA\n"
        );
    }
}
