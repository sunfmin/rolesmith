//! `rolesmith show <role> [--rev <sha>]`: a role as it is on its source's local `main`, or at a
//! commit (SPEC.md §2, §8).

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::sources::{self, Sources};

/// A role: role.toml, core, prohibitions, skills with descriptions, programs, services.
#[derive(clap::Args)]
pub struct Args {
    /// The role: <role> when one source has it, <source>/<role> otherwise.
    pub role: String,

    /// Read the role at this commit of its source instead of its local main.
    #[arg(long, value_name = "SHA")]
    pub rev: Option<String>,
}

impl Command for Args {
    const NAME: &'static str = "show";

    fn output_shape() -> Value {
        let strings = json!({ "type": "array", "items": { "type": "string" } });
        let named = json!({ "type": "array", "items": {
            "type": "object", "required": ["name", "description"],
            "properties": { "name": { "type": "string" }, "description": { "type": "string" } }
        } });
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["source", "role", "rev", "description", "role_toml", "core",
                         "prohibitions", "skills", "programs", "services"],
            "properties": {
                "source": { "type": "string" },
                "role": { "type": "string" },
                "rev": { "type": "string", "description": "the full SHA the role was read at" },
                "description": { "type": "string" },
                "role_toml": { "type": "string", "description": "the text of role.toml" },
                "core": { "type": "string", "description": "the text of core.md" },
                "prohibitions": { "type": "array", "items": {
                    "type": "object", "required": ["rule", "why"],
                    "properties": { "rule": { "type": "string" }, "why": { "type": "string" } }
                } },
                "skills": named.clone(),
                "programs": named,
                "services": strings
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let record = Sources::load()?;
        let found = sources::resolve(&record, &self.role, self.rev.as_deref())?;
        let (lib, _tree) = sources::read_role(&found)?;
        let role = &lib.roles[0];
        let role_toml =
            std::fs::read_to_string(lib.root.join("roles").join(&role.name).join("role.toml"))
                .map_err(|e| Failure::new(format!("cannot read role.toml: {e}")))?;
        let program_description = |name: &str| {
            lib.tools
                .iter()
                .find(|t| t.name == name)
                .map(|t| t.toml.description.clone())
                .unwrap_or_default()
        };

        let mut h = format!(
            "# {} ({} at {})\n\n{}\n\n## role.toml\n\n{}\n",
            found.role,
            found.source,
            found.rev,
            role.toml.description,
            role_toml.trim_end()
        );
        h.push_str(&format!("\n## Core\n\n{}\n", role.core.trim_end()));
        let sections: [(&str, Vec<String>); 4] = [
            (
                "Prohibitions",
                role.prohibitions
                    .iter()
                    .map(|p| format!("{} ({})", p.rule, p.why))
                    .collect(),
            ),
            (
                "Skills",
                role.skills
                    .iter()
                    .map(|s| format!("{}: {}", s.name, s.description))
                    .collect(),
            ),
            (
                "Programs",
                role.toml
                    .programs
                    .iter()
                    .map(|p| format!("{p}: {}", program_description(p)))
                    .collect(),
            ),
            ("Services", role.toml.services.clone()),
        ];
        for (title, items) in sections {
            h.push_str(&format!("\n## {title}\n\n"));
            if items.is_empty() {
                h.push_str("(none)\n");
            }
            for item in items {
                h.push_str(&format!("- {item}\n"));
            }
        }

        let json = json!({
            "source": found.source,
            "role": found.role,
            "rev": found.rev,
            "description": role.toml.description,
            "role_toml": role_toml,
            "core": role.core,
            "prohibitions": role.prohibitions.iter()
                .map(|p| json!({ "rule": p.rule, "why": p.why })).collect::<Vec<_>>(),
            "skills": role.skills.iter()
                .map(|s| json!({ "name": s.name, "description": s.description })).collect::<Vec<_>>(),
            "programs": role.toml.programs.iter()
                .map(|p| json!({ "name": p, "description": program_description(p) })).collect::<Vec<_>>(),
            "services": role.toml.services,
        });
        Ok(Output { json, human: h })
    }
}
