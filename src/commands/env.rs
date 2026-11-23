//! `rolesmith env <role> [-- <command>...]`: the role's credentials, read from mytokens at that
//! moment, printed as `export` lines or handed to a command; written nowhere (SPEC.md §7.1).

use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::machine;
use crate::sources::{self, Sources};

/// Print a role's credentials as `export NAME='value'` lines, or run a command with them set.
///
/// Run `eval "$(rolesmith env <role>)"`, or `rolesmith env <role> -- <command> [args]` to run the command in rolesmith's place with the variables added to its environment. Every variable of the role's programs' `[credential]` is read from mytokens at that moment; nothing is written to disk. It refuses a role with a service no program's `[credential]` maps to variables, listing each (with `--json`, `error.unmapped`), and reads nothing then.
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
    const GROUP: &'static str = super::groups::ROLE;

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
        let unmapped: Vec<&String> = role
            .toml
            .services
            .iter()
            .filter(|s| !credentials.iter().any(|c| &c.mytokens == *s))
            .collect();
        if !unmapped.is_empty() {
            return Err(Failure::listing(
                format!(
                    "{}: no program's [credential] maps these services to variables; add one to a tool.toml (SPEC.md §3.5):",
                    found.qualified()
                ),
                "unmapped",
                &unmapped,
                |s| json!(s),
            ));
        }
        let mut variables = BTreeMap::new();
        for cred in credentials {
            for (name, field) in &cred.env {
                variables.insert(name.clone(), machine::value(&cred.mytokens, field)?);
            }
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
            .map(|(name, value)| format!("export {name}={}\n", shell_quote(value)))
            .collect();
        Ok(Output {
            json: json!({ "role": found.qualified(), "rev": found.rev, "variables": variables }),
            human,
        })
    }
}

/// `value` in single quotes for sh, each `'` written as `'\''`.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}
