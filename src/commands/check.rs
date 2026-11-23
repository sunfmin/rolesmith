//! `rolesmith check [<role>] [--library <dir>]`: validate a library working copy, or one of its
//! roles, against SPEC.md §2, §3 and §7.2, and report every problem, not only the first.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::library;
use crate::problem::Problem;

/// Check a library working copy, or one role of it, and print every problem, not only the first.
///
/// Run it in a library after changing a role, before `rolesmith propose` (which runs it too). It checks the library that holds `--library` (default: the current directory), found the way git finds its repository: the nearest of that directory and its parents with a `library.toml`; run anywhere inside a library, in a role or one of its skills, it checks the whole library as from its root. Outside any library it refuses, naming the directory it started from. It reads the working tree as it is, and fetches what the external-skill cache lacks. Checking one role checks `library.toml`, the roles it requires and the tools it names. Each problem has its file, its line where there is one, and its rule (see [how a run ends](help:errors)). It refuses when there is any problem, with `--json` as `error.problems`.
///
/// ## Pitfall
///
/// A problem that matches a secret pattern never echoes the match: its file or reason is withheld.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    /// Check only this role (with library.toml and the tools it names).
    pub role: Option<String>,

    /// A directory in the library working copy to check; its nearest parent with a library.toml is the library.
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
        let root = library::find_root(&self.library);
        match library::load(&root, self.role.as_deref()) {
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
                let header = format!(
                    "{} {} in {}:",
                    problems.len(),
                    plural(problems.len(), "problem"),
                    root.display()
                );
                Err(Failure::listing(
                    header,
                    "problems",
                    &problems,
                    Problem::to_json,
                ))
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
