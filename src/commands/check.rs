//! `rolesmith check [<role>] [--library <dir>]`: validate a library working copy, or one of its
//! roles, against SPEC.md §2, §3 and §7.2, and report every problem, not only the first.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::library;

/// Validate a library working copy or one role; print every problem.
#[derive(clap::Args)]
pub struct Args {
    /// Check only this role (with library.toml and the tools it names).
    pub role: Option<String>,

    /// The library working copy to check.
    #[arg(long, value_name = "DIR", default_value = ".")]
    pub library: PathBuf,
}

impl Command for Args {
    const NAME: &'static str = "check";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "the library passed; a library with problems exits 1 and lists them under error.problems",
            "type": "object",
            "required": ["library", "roles", "tools"],
            "properties": {
                "library": { "type": "string", "description": "library.toml's name" },
                "roles": { "type": "array", "items": { "type": "string" } },
                "tools": { "type": "array", "items": { "type": "string" } }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        match library::load(&self.library, self.role.as_deref()) {
            Ok(lib) => {
                let roles: Vec<&str> = lib.roles.iter().map(|r| r.name.as_str()).collect();
                let tools: Vec<&str> = lib.tools.iter().map(|t| t.name.as_str()).collect();
                let human = format!(
                    "{}: {} {}, {} {}, no problems\n",
                    lib.toml.name,
                    roles.len(),
                    plural(roles.len(), "role"),
                    tools.len(),
                    plural(tools.len(), "tool"),
                );
                Ok(Output {
                    json: json!({ "library": lib.toml.name, "roles": roles, "tools": tools }),
                    human,
                })
            }
            Err(problems) => {
                let mut message = format!(
                    "{} {} in {}:",
                    problems.len(),
                    plural(problems.len(), "problem"),
                    self.library.display()
                );
                for p in &problems {
                    message.push_str(&format!("\n  {p}"));
                }
                Err(Failure {
                    message,
                    problems,
                    missing: Vec::new(),
                })
            }
        }
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}
