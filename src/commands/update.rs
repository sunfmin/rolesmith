//! `rolesmith update [<role>] [--project <dir>]`: move a project's roles to their source's `main`,
//! rewrite their `rev` in `rolesmith.toml`, and sync (SPEC.md §5.4).

use std::path::PathBuf;

use serde_json::Value;

use super::{Command, Failure, Output, sync};
use crate::project::Record;
use crate::sources::{self, MAIN};

/// Move roles (default: every role) to their source's local main and sync.
#[derive(clap::Args)]
pub struct Args {
    /// Only this role, by its name in rolesmith.toml.
    pub role: Option<String>,

    /// The project's root (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,
}

impl Command for Args {
    const NAME: &'static str = "update";

    fn output_shape() -> Value {
        sync::shape()
    }

    fn run(self) -> Result<Output, Failure> {
        let dir = sync::project_dir(self.project)?;
        let mut record = Record::load(&dir)?;
        let only = self
            .role
            .as_deref()
            .map(|r| record.position(r))
            .transpose()?;
        for (i, entry) in record.role.iter_mut().enumerate() {
            if only.is_some_and(|o| o != i) {
                continue;
            }
            let clone = sources::clone_dir(&entry.source)?;
            entry.rev = sources::main_commit(&clone).ok_or_else(|| {
                Failure::new(format!(
                    "source {} has no {MAIN} on this machine; run `rolesmith sync` or `rolesmith source add`",
                    entry.source
                ))
            })?;
        }
        sync::install(&dir, &record)
    }
}
