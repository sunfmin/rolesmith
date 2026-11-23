//! `rolesmith doctor <role>`: what the role needs on this machine (SPEC.md §3.5, §7.1, §8). Each
//! program's `check` passes; each service and each program's credential has its mytokens entry,
//! read by name with `mytokens list` only. Anything missing is a refusal, each with its fix.

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::machine::{self, Missing};
use crate::sources::{self, Sources};

/// Check what a role needs on this machine: each program's `check` passes, and each service and credential has its mytokens entry.
///
/// Run it before working under a role. It runs each program's `check` with `sh -c` and reads only which entries exist (`mytokens list`), never a value. It refuses when anything is missing, listing each with the command that fixes it (`rolesmith tool install <source>/<role>`, `mytokens put <entry> --fields ...`); with `--json` as `error.missing`. Do every fix, then run it again. A program without its `tool.toml` is refused as `show` refuses the role.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    /// The role: <role> when one source has it, <source>/<role> otherwise.
    pub role: String,
}

impl Command for Args {
    const NAME: &'static str = "doctor";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "nothing is missing; a role that lacks anything here exits 1 and lists it under error.missing",
            "type": "object",
            "required": ["source", "role", "rev", "programs", "entries"],
            "properties": {
                "source": { "type": "string" },
                "role": { "type": "string" },
                "rev": { "type": "string", "description": "the full SHA the role was read at" },
                "programs": { "type": "array", "items": {
                    "type": "object", "required": ["name", "check"],
                    "properties": { "name": { "type": "string" }, "check": { "type": "string" } }
                } },
                "entries": { "type": "array", "items": { "type": "string" },
                             "description": "the mytokens entries the role reads, each present" }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let record = Sources::load()?;
        let found = sources::resolve(&record, &self.role, None)?;
        let (lib, _tree) = sources::read_role(&found)?;
        let role = &lib.roles[0];
        let fix_programs = format!("rolesmith tool install {}", found.qualified());

        let mut missing = Vec::new();
        let mut programs = Vec::new();
        let mut human = String::new();
        // Each entry the role reads, with the fields its credentials fill variables from.
        let mut wanted: Vec<(String, Vec<String>)> = role
            .toml
            .services
            .iter()
            .map(|s| (s.clone(), Vec::new()))
            .collect();
        for name in &role.toml.programs {
            let tool = lib
                .tools
                .iter()
                .find(|t| &t.name == name)
                .expect("a checked role has a tool.toml for each program");
            let check = machine::check(&tool.toml.check);
            if !check.passes {
                missing.push(Missing {
                    kind: "program",
                    name: name.clone(),
                    reason: format!("check `{}` {}", tool.toml.check, check.reason),
                    fix: fix_programs.clone(),
                });
            }
            programs.push(json!({ "name": name, "check": tool.toml.check }));
            human.push_str(&format!(
                "- program {name}: `{}` passes
",
                tool.toml.check
            ));
            if let Some(cred) = &tool.toml.credential {
                if !wanted.iter().any(|(e, _)| e == &cred.mytokens) {
                    wanted.push((cred.mytokens.clone(), Vec::new()));
                }
                let (_, fields) = wanted
                    .iter_mut()
                    .find(|(e, _)| e == &cred.mytokens)
                    .expect("pushed above");
                for field in cred.env.values().filter(|f| !f.is_empty()) {
                    if !fields.contains(field) {
                        fields.push(field.clone());
                    }
                }
            }
        }

        if !wanted.is_empty() {
            let present = machine::entries()?;
            for (entry, fields) in &wanted {
                if !present.contains(entry) {
                    missing.push(Missing {
                        kind: "entry",
                        name: entry.clone(),
                        reason: "no mytokens entry".to_owned(),
                        fix: machine::put_command(entry, fields),
                    });
                }
            }
        }

        if !missing.is_empty() {
            let header = format!(
                "{} at {} lacks {} thing(s) on this machine:",
                found.qualified(),
                found.rev,
                missing.len()
            );
            return Err(Failure::listing(
                header,
                "missing",
                &missing,
                Missing::to_json,
            ));
        }

        let entries: Vec<&String> = wanted.iter().map(|(e, _)| e).collect();
        let mut human = format!(
            "{} at {}: nothing missing
{human}",
            found.qualified(),
            found.rev
        );
        for e in &entries {
            human.push_str(&format!("- entry {e}: present\n"));
        }
        Ok(Output {
            json: json!({
                "source": found.source,
                "role": found.name,
                "rev": found.rev,
                "programs": programs,
                "entries": entries,
            }),
            human,
        })
    }
}
