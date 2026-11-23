//! `rolesmith doctor <role> [--host <host>]`: what the role needs (SPEC.md §3.5, §7.1, §8). Each
//! program's `check` passes, on this machine or on the host over ssh; each service and each
//! program's credential has its mytokens entry on this machine, read by name with `mytokens list`
//! only. Anything missing is a refusal, each with its fix.

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::host::Host;
use crate::machine::{self, Missing};
use crate::sources::{self, Sources};

/// Check what a role needs on this machine: each program's `check` passes, and each service and credential has its mytokens entry.
///
/// Run it before working under a role. It runs each program's `check` with `sh -c` (a `script` tool's check must also show its pinned version on stdout) and reads only which entries exist (`mytokens list`), never a value. It refuses when anything is missing, listing each with the command that fixes it (`rolesmith tool install <source>/<role>`, `mytokens put <entry> --fields ...`); with `--json` as `error.missing`. Do every fix, then run it again. A program without its `tool.toml` is refused as `show` refuses the role.
///
/// ## Another host
///
/// With `--host <host>` each `check` runs on that machine instead, as `ssh -o BatchMode=yes -- <host> sh -c '<check>'`, for work that runs there (a build on the machine a device is plugged into). The host needs `sh` and ssh access by key, not rolesmith; the check sees the `PATH` that a command run over ssh gets there, so a program or brew reached only by an interactive shell's profile is missing. A program's fix then carries `--host <host>`. Entries are still read on this machine: a credential never leaves it, and whoever runs work on the host brings the values (`rolesmith env`). A host ssh cannot reach fails, naming the host, rather than listing everything as missing.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    /// The role: <role> when one source has it, <source>/<role> otherwise.
    pub role: String,
    /// Run the checks on this host over ssh instead of on this machine.
    #[arg(long)]
    pub host: Option<String>,
}

impl Command for Args {
    const NAME: &'static str = "doctor";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "nothing is missing; a role that lacks anything here exits 1 and lists it under error.missing",
            "type": "object",
            "required": ["source", "role", "rev", "host", "programs", "entries"],
            "properties": {
                "source": { "type": "string" },
                "role": { "type": "string" },
                "rev": { "type": "string", "description": "the full SHA the role was read at" },
                "host": { "type": ["string", "null"], "description": "the host the checks ran on; null for this machine" },
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
        let host = Host::from_arg(self.host);
        let fix_programs = format!(
            "rolesmith tool install {}{}",
            found.qualified(),
            host.flag()
        );

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
            let check = machine::check_tool(&host, &tool.toml)?;
            if !check.passes {
                missing.push(Missing {
                    kind: "program",
                    name: name.clone(),
                    reason: format!("check `{}`{} {}", tool.toml.check, host.on(), check.reason),
                    fix: fix_programs.clone(),
                });
            }
            programs.push(json!({ "name": name, "check": tool.toml.check }));
            human.push_str(&format!(
                "- program {name}: `{}` passes{}\n",
                tool.toml.check,
                host.on()
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
            let place = match host.name() {
                None => "on this machine".to_owned(),
                Some(name) => format!("programs on {name} and entries on this machine"),
            };
            let header = format!(
                "{} at {} lacks {} thing(s), {place}:",
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
                "host": host.name(),
                "programs": programs,
                "entries": entries,
            }),
            human,
        })
    }
}
