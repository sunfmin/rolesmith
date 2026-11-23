//! `rolesmith env <role> [-- <command>...]`: the role's credentials, read at that moment from
//! mytokens (by its programs' `[credential]`, or a Profile service's own field names) or, for a
//! service this host maps in its `services.toml`, from the command it maps it to; printed as
//! `export` lines or handed to a command; written nowhere (SPEC.md §7.1, ADR 0006).

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::library::Credential;
use crate::machine;
use crate::service_map::{self, ServiceMap};
use crate::sources::{self, Sources};

/// Print a role's credentials as `export NAME='value'` lines, or run a command with them set.
///
/// Run `eval "$(rolesmith env <role>)"`, or `rolesmith env <role> -- <command> [args]` to run the command in rolesmith's place with the variables added to its environment. Each variable is read at that moment; nothing is written to disk. Where a service's values come from is this host's choice, never the library's: a service this host maps (below) is read from its command; any other from mytokens. Every variable of the role's programs' `[credential]` of a service read from mytokens is read with `mytokens get <entry> --field <field>`. A service no `[credential]` maps that `mytokens list` lists as a Profile is exported by its own field names: each field `NAME` becomes `NAME`, read with `mytokens get <service> --field NAME`, as `mytokens env <service>` would export it. It refuses a role with a service that is none of these, listing each (with `--json`, `error.unmapped`), and a variable read from two different places, naming both (`error.conflicting`); it reads nothing from mytokens then.
///
/// ## A service this host reads elsewhere
///
/// `services.toml`, beside the sources' record in rolesmith's config directory (`$XDG_CONFIG_HOME/rolesmith/`, default `~/.config/rolesmith/`), maps a service to a `command` whose stdout is the service's variables, `NAME=value` or `export NAME='value'` lines (as `mytokens env` and `op inject` print them), with an optional `check` that `rolesmith doctor` runs in its place. The command runs with `sh -c`, stdin closed, and every variable it prints is exported; a program's `[credential]` for the service names the variables it must print. 1Password, over a template of `export ASC_KEY_ID='{{ op://Private/asc-api-key/key-id }}'` lines kept at `~/.config/rolesmith/asc-api-key.env`:
///
/// ```toml
/// [service.asc-api-key]
/// command = "op inject -i ~/.config/rolesmith/asc-api-key.env"
/// check = "op whoami"
/// ```
///
/// A script of the host's own, printing `SENTRY_TOKEN=...` lines:
///
/// ```toml
/// [service.sentry]
/// command = "~/bin/sentry-env"
/// check = "test -x ~/bin/sentry-env"
/// ```
///
/// A command that exits non-zero is refused with its stderr, and a line that is not a variable is refused by its number; nothing is exported then. A `[credential]` variable the command does not print is refused, each with its service (`error.absent`). The command runs before the variables are checked against each other, so a conflict with a mapped service is found after it ran.
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
        let map = ServiceMap::load()?;
        let credentials: Vec<&Credential> = role
            .toml
            .programs
            .iter()
            .filter_map(|p| lib.tools.iter().find(|t| &t.name == p))
            .filter_map(|t| t.toml.credential.as_ref())
            .collect();
        // Each variable with every (entry, field) it is read from: a program's [credential], a
        // Profile service's own field of that name, or a mapped service's printed name.
        let mut sources: BTreeMap<&str, BTreeSet<(&str, &str)>> = BTreeMap::new();
        for cred in credentials
            .iter()
            .filter(|c| map.get(&c.mytokens).is_none())
        {
            for (name, field) in &cred.env {
                let from = (cred.mytokens.as_str(), field.as_str());
                sources.entry(name).or_default().insert(from);
            }
        }
        let mut unmapped: Vec<&String> = (role.toml.services.iter())
            .filter(|s| map.get(s).is_none())
            .filter(|s| !credentials.iter().any(|c| &c.mytokens == *s))
            .collect();
        let profiles = match unmapped.is_empty() {
            true => BTreeMap::new(),
            false => machine::profiles()?,
        };
        unmapped.retain(|service| {
            let Some(fields) = profiles.get(*service) else {
                return true;
            };
            for field in fields {
                sources.entry(field).or_default().insert((service, field));
            }
            false
        });
        if !unmapped.is_empty() {
            return Err(Failure::listing(
                format!(
                    "{}: no program's [credential] maps these services to variables, none is a mytokens Profile, and this host maps none to a command; add a [credential] to a tool.toml (SPEC.md §3.5), make the entry a Profile, or map it in {}:",
                    found.qualified(),
                    service_map::file()?.display()
                ),
                "unmapped",
                &unmapped,
                |s| json!(s),
            ));
        }

        // Each service this host maps, run once, in the order the role names it.
        let mut given: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
        let named = (role.toml.services.iter()).chain(credentials.iter().map(|c| &c.mytokens));
        for service in named {
            if let Some(mapped) = map.get(service)
                && !given.contains_key(service.as_str())
            {
                given.insert(service, mapped.variables(service)?);
            }
        }
        let absent: Vec<Value> = credentials
            .iter()
            .filter_map(|c| Some((c, given.get(c.mytokens.as_str())?)))
            .flat_map(|(c, vars)| {
                (c.env.keys())
                    .filter(|name| !vars.contains_key(*name))
                    .map(|name| json!({ "name": name, "service": c.mytokens }))
            })
            .collect();
        if !absent.is_empty() {
            return Err(Failure::listing(
                format!(
                    "{}: a program's [credential] needs these variables, and the command this host maps their service to does not print them; print them from the command in {}:",
                    found.qualified(),
                    service_map::file()?.display()
                ),
                "absent",
                &absent,
                |v| v.clone(),
            ));
        }
        for (service, vars) in &given {
            for name in vars.keys() {
                sources.entry(name).or_default().insert((service, name));
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
                    "{}: these variables are read from more than one place; keep one (a program's [credential], a Profile's field, or a mapped command's line):",
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
            let value = match given.get(entry) {
                Some(vars) => vars[*field].clone(),
                None => machine::value(entry, field)?,
            };
            variables.insert(name.to_string(), value);
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
