//! `rolesmith list [--source <name>]`: the roles of every source (or one), by the address that
//! reaches them, read from each source's local `main`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::sources::{self, Kind, Sources};

/// List the roles of every source (or of one): the address that reaches each, and its description.
///
/// Run it to find a role to `show` or `use`. It reads each source's local `main`, never its working tree, and fetches nothing (`rolesmith source update` does). An address is `<role>` when only one source has the role, `<source>/<role>` otherwise. It refuses an unknown `--source`.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    /// Only the roles of this source.
    #[arg(long, value_name = "NAME")]
    pub source: Option<String>,
}

impl Command for Args {
    const NAME: &'static str = "list";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["roles"],
            "properties": {
                "roles": { "type": "array", "items": {
                    "type": "object",
                    "required": ["address", "source", "role", "description"],
                    "properties": {
                        "address": { "type": "string", "description": "<role> when one source has it, <source>/<role> otherwise" },
                        "source": { "type": "string" },
                        "role": { "type": "string" },
                        "description": { "type": ["string", "null"], "description": "null when role.toml has none that parses" }
                    }
                } }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let record = Sources::load()?;
        if let Some(only) = &self.source {
            record.get(only)?;
        }

        // Every source's roles, so an address is bare only when no other source has the name.
        let mut by_source = Vec::new();
        let mut holders: BTreeMap<String, usize> = BTreeMap::new();
        for name in record.source.keys() {
            let clone = sources::clone_dir(name)?;
            let roles: Vec<(String, Option<String>)> = match sources::main_commit(&clone) {
                None => Vec::new(),
                Some(sha) => sources::roles_at(&clone, &sha)
                    .into_iter()
                    .map(|role| {
                        let description = description(&clone, &sha, &role);
                        (role, description)
                    })
                    .collect(),
            };
            for (role, _) in &roles {
                *holders.entry(role.clone()).or_default() += 1;
            }
            by_source.push((name.clone(), roles));
        }

        let mut listed = Vec::new();
        let mut human = String::new();
        for (source, roles) in by_source {
            if self.source.as_ref().is_some_and(|only| *only != source) {
                continue;
            }
            for (role, description) in roles {
                let address = if holders[&role] == 1 {
                    role.clone()
                } else {
                    format!("{source}/{role}")
                };
                human.push_str(&format!(
                    "{address}\t{}\n",
                    description.as_deref().unwrap_or("(no description)")
                ));
                listed.push(json!({
                    "address": address, "source": source, "role": role, "description": description,
                }));
            }
        }
        if listed.is_empty() {
            human.push_str("no roles\n");
        }
        Ok(Output {
            json: json!({ "roles": listed }),
            human,
        })
    }
}

/// `role.toml`'s `description` at `sha`, if it parses.
fn description(clone: &std::path::Path, sha: &str, role: &str) -> Option<String> {
    let text = sources::toml_at(clone, sha, Kind::Role, role)?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    table.get("description")?.as_str().map(str::to_owned)
}
