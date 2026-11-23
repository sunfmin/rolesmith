//! `rolesmith agent-context`: every command, flag and JSON output shape as one JSON document,
//! generated from the parser's own definition, the registry and the manual, so it cannot drift
//! from them.

use serde_json::{Map, Value, json};

use super::{Command, Failure, Output, output_shapes};
use crate::cli::ErrorKind;

/// Print every command, flag and JSON output shape as one JSON document.
///
/// Run it when a program, not a reader, needs rolesmith's commands: the document is generated from the parser's own definitions, so it is never out of date. Each command's `long_help` is the page its `--help` prints, and its `output` the JSON Schema of what its `--json` prints on success. It reads and writes nothing, and never refuses. Without `--json` it prints the same document, pretty-printed.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {}

impl Command for Args {
    const NAME: &'static str = "agent-context";
    const GROUP: &'static str = super::groups::LEARNING;

    fn output_shape() -> Value {
        let arg = json!({
            "type": "object",
            "required": ["id", "long", "short", "help", "required", "takes_value",
                         "value_names", "possible_values", "default_values", "global"],
            "properties": {
                "id": { "type": "string" },
                "long": { "type": ["string", "null"] },
                "short": { "type": ["string", "null"] },
                "help": { "type": ["string", "null"] },
                "required": { "type": "boolean" },
                "takes_value": { "type": "boolean" },
                "value_names": { "type": "array", "items": { "type": "string" } },
                "possible_values": { "type": "array", "items": { "type": "string" } },
                "default_values": { "type": "array", "items": { "type": "string" } },
                "global": { "type": "boolean" }
            }
        });
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["name", "version", "conventions", "flags", "commands"],
            "properties": {
                "name": { "const": "rolesmith" },
                "version": { "type": "string" },
                "conventions": {
                    "type": "object",
                    "required": ["json_flag", "diagnostics", "exit_codes", "error_output"],
                    "properties": {
                        "json_flag": { "type": "string" },
                        "diagnostics": { "type": "string" },
                        "exit_codes": { "type": "object", "additionalProperties": { "type": "string" } },
                        "error_output": { "type": "object" }
                    }
                },
                "flags": { "type": "array", "items": { "$ref": "#/$defs/arg" } },
                "commands": { "type": "array", "items": { "$ref": "#/$defs/command" } }
            },
            "$defs": {
                "arg": arg,
                "command": {
                    "type": "object",
                    "required": ["name", "about", "long_help", "flags", "positionals", "subcommands"],
                    "properties": {
                        "name": { "type": "string" },
                        "about": { "type": ["string", "null"] },
                        "long_help": {
                            "description": "what the command's --help prints: its page of the manual",
                            "type": ["string", "null"]
                        },
                        "flags": { "type": "array", "items": { "$ref": "#/$defs/arg" } },
                        "positionals": { "type": "array", "items": { "$ref": "#/$defs/arg" } },
                        "subcommands": { "type": "array", "items": { "$ref": "#/$defs/command" } },
                        "output": {
                            "description": "JSON Schema of what --json prints on success; on every registered top-level command",
                            "type": "object"
                        }
                    }
                }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let doc = document();
        let human = format!("{doc:#}\n");
        Ok(Output { json: doc, human })
    }
}

/// The document: the parser's definition walked, each registered command given its output shape.
pub fn document() -> Value {
    let mut root = crate::cli::command();
    root.build();
    let shapes: Map<String, Value> = output_shapes()
        .into_iter()
        .map(|(name, shape)| (name.to_owned(), shape))
        .collect();
    let manual = crate::help::manual();
    let commands: Vec<Value> = root
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .map(|c| {
            let mut entry = describe(&manual, &[], c);
            if let Some(shape) = shapes.get(c.get_name()) {
                entry["output"] = shape.clone();
            }
            entry
        })
        .collect();
    json!({
        "name": root.get_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "conventions": {
            "json_flag": "--json: print one JSON document on stdout instead of human text",
            "diagnostics": "stderr",
            "exit_codes": {
                "0": "success",
                "1": "refused or failed",
                "2": "usage error"
            },
            "error_output": {
                "description": "with --json, a run that exits 1 or 2 prints this on stdout",
                "type": "object",
                "properties": {
                    "error": {
                        "type": "object",
                        "properties": {
                            "kind": { "enum": <ErrorKind as agent_help::Table>::ENTRIES.iter().map(|e| e.name).collect::<Vec<_>>() },
                            "message": { "type": "string" },
                            "problems": {
                                "description": "present when the refusal is a list of problems (check)",
                                "type": "array",
                                "items": crate::problem::Problem::shape()
                            },
                            "missing": {
                                "description": "present when the refusal is what this machine lacks (doctor)",
                                "type": "array",
                                "items": crate::machine::Missing::shape()
                            }
                        }
                    }
                }
            }
        },
        "flags": root.get_arguments().filter(|a| !a.is_positional()).map(arg).collect::<Vec<_>>(),
        "commands": commands,
    })
}

/// One command at `parent`'s path: its parser definition, and its page of the manual as `long_help`.
fn describe(manual: &agent_help::Help, parent: &[&str], c: &clap::Command) -> Value {
    let path: Vec<&str> = parent.iter().copied().chain([c.get_name()]).collect();
    json!({
        "name": c.get_name(),
        "about": c.get_about().map(|s| s.to_string()),
        "long_help": manual.command_help(&path).expect("checked by the_manual_is_whole"),
        "flags": c.get_arguments().filter(|a| !a.is_positional()).map(arg).collect::<Vec<_>>(),
        "positionals": c.get_arguments().filter(|a| a.is_positional()).map(arg).collect::<Vec<_>>(),
        "subcommands": c
            .get_subcommands()
            .filter(|s| s.get_name() != "help")
            .map(|s| describe(manual, &path, s))
            .collect::<Vec<_>>(),
    })
}

fn arg(a: &clap::Arg) -> Value {
    // clap gives a switch a value name and a default of "false"; a switch has neither.
    let takes_value = a.get_action().takes_values();
    let strings = |v: &[&std::ffi::OsStr]| -> Vec<String> {
        v.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    };
    json!({
        "id": a.get_id().as_str(),
        "long": a.get_long(),
        "short": a.get_short().map(String::from),
        "help": a.get_help().map(|s| s.to_string()),
        "required": a.is_required_set(),
        "takes_value": takes_value,
        "value_names": if !takes_value { Vec::new() } else { a.get_value_names().unwrap_or_default().iter().map(|s| s.as_str()).collect::<Vec<_>>() },
        "possible_values": a.get_possible_values().iter().map(|p| p.get_name().to_owned()).collect::<Vec<_>>(),
        "default_values": if !takes_value { Vec::new() } else { strings(&a.get_default_values().iter().map(|s| s.as_os_str()).collect::<Vec<_>>()) },
        "global": a.is_global_set(),
    })
}
