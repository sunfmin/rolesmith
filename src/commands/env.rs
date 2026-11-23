//! `rolesmith env <role> [-- <command>...]`: the role's credentials, read from mytokens at that
//! moment (by its programs' `[credential]`, or a Profile service's own field names), printed as
//! `export` lines or handed to a command; written nowhere (SPEC.md §7.1).

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::machine;
use crate::sources::{self, Sources};

/// Print a role's credentials as `export NAME='value'` lines, or run a command with them set.
///
/// Run `eval "$(rolesmith env <role>)"`, or `rolesmith env <role> -- <command> [args]` to run the command in rolesmith's place with the variables added to its environment. Every variable of the role's programs' `[credential]` is read from mytokens at that moment; nothing is written to disk. A service no `[credential]` maps that `mytokens list` lists as a Profile is exported by its own field names: each field `NAME` becomes `NAME`, read with `mytokens get <service> --field NAME`, as `mytokens env <service>` would export it. It refuses a role with a service that is neither, listing each (with `--json`, `error.unmapped`), and a variable read from two different places, naming both (`error.conflicting`); it reads nothing then.
///
/// ## Pitfall
///
/// The values are secrets: never print them into a log, a lesson or a file.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    /// The role: <role> when one source has it, <source>/<role> otherwise.
    pub role: String,
    /// Run this command with the variables set instead of printing them.
    #[arg(last = true)]
    pub command: Vec<String>,
}

impl Command for Args {
    const NAME: &'static str = "env";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "without `--`; with it the command is run in rolesmith's place",
            "type": "object",
            "required": ["role", "rev", "variables"],
            "properties": {
                "role": { "type": "string", "description": "<source>/<role>" },
                "rev": { "type": "string", "description": "the full SHA the role was read at" },
                "variables": { "type": "object", "additionalProperties": { "type": "string" } }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let record = Sources::load()?;
        let found = sources::resolve(&record, &self.role, None)?;
        let (lib, _tree) = sources::read_role(&found)?;
        let role = &lib.roles[0];
        let credentials: Vec<_> = role
            .toml
            .programs
            .iter()
            .filter_map(|p| lib.tools.iter().find(|t| &t.name == p))
            .filter_map(|t| t.toml.credential.as_ref())
            .collect();
        let mapped = |s: &String| credentials.iter().any(|c| &c.mytokens == s);
        let unmapped: Vec<&String> = role.toml.services.iter().filter(|s| !mapped(s)).collect();
        let profiles = if unmapped.is_empty() {
            BTreeMap::new()
        } else {
            machine::profiles()?
        };
        let unmapped: Vec<&String> = unmapped
            .into_iter()
            .filter(|s| !profiles.contains_key(*s))
            .collect();
        if !unmapped.is_empty() {
            return Err(Failure::listing(
                format!(
                    "{}: no program's [credential] maps these services to variables, and none is a mytokens Profile; add a [credential] to a tool.toml (SPEC.md §3.5), or make the entry a Profile:",
                    found.qualified()
                ),
                "unmapped",
                &unmapped,
                |s| json!(s),
            ));
        }
        // Each variable with every (entry, field) it is read from: a program's [credential], or
        // a Profile service's own field of that name.
        let mut sources: BTreeMap<&str, BTreeSet<(&str, &str)>> = BTreeMap::new();
        for cred in &credentials {
            for (name, field) in &cred.env {
                sources
                    .entry(name)
                    .or_default()
                    .insert((&cred.mytokens, field));
            }
        }
        for service in role.toml.services.iter().filter(|s| !mapped(s)) {
            for field in &profiles[service] {
                sources.entry(field).or_default().insert((service, field));
            }
        }
        let conflicting: Vec<Conflict> = sources
            .iter()
            .filter(|(_, from)| from.len() > 1)
            .map(|(name, from)| Conflict { name, from })
            .collect();
        if !conflicting.is_empty() {
            return Err(Failure::listing(
                format!(
                    "{}: these variables are read from more than one place; keep one (a program's [credential], or the Profile's field):",
                    found.qualified()
                ),
                "conflicting",
                &conflicting,
                Conflict::to_json,
            ));
        }
        let mut variables = BTreeMap::new();
        for (name, from) in &sources {
            let (entry, field) = from.first().expect("every variable has a source");
            variables.insert(name.to_string(), machine::value(entry, field)?);
        }
        if let Some((program, args)) = self.command.split_first() {
            let e = std::process::Command::new(program)
                .args(args)
                .envs(&variables)
                .exec();
            return Err(Failure::new(format!("cannot run `{program}`: {e}")));
        }
        let human = variables
            .iter()
            .map(|(name, value)| format!("export {name}={}\n", crate::host::quote(value)))
            .collect();
        Ok(Output {
            json: json!({ "role": found.qualified(), "rev": found.rev, "variables": variables }),
            human,
        })
    }
}

/// A variable read from more than one (entry, field).
struct Conflict<'a> {
    name: &'a str,
    from: &'a BTreeSet<(&'a str, &'a str)>,
}

impl Conflict<'_> {
    fn to_json(&self) -> Value {
        let sources: Vec<Value> = self
            .from
            .iter()
            .map(|(entry, field)| json!({ "entry": entry, "field": field }))
            .collect();
        json!({ "name": self.name, "sources": sources })
    }
}

impl std::fmt::Display for Conflict<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let from: Vec<String> = self
            .from
            .iter()
            .map(|(entry, field)| match *field {
                "" => entry.to_string(),
                _ => format!("{entry} field \"{field}\""),
            })
            .collect();
        write!(f, "{}: {}", self.name, from.join(", "))
    }
}
