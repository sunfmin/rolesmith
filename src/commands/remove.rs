//! `rolesmith remove <role> [--project <dir>]`: drop a role from a project's `rolesmith.toml` and
//! delete its generated files, only those the manifest lists (SPEC.md §4, §8).

use std::path::PathBuf;

use serde_json::Value;

use super::{Command, Failure, Output, sync};
use crate::project::{self, Record};

/// Drop a role from rolesmith.toml and delete its generated files.
#[derive(clap::Args)]
pub struct Args {
    /// The role, by its name in rolesmith.toml.
    pub role: String,

    /// The project's root (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,
}

impl Command for Args {
    const NAME: &'static str = "remove";

    fn output_shape() -> Value {
        sync::shape()
    }

    fn run(self) -> Result<Output, Failure> {
        let dir = sync::project_dir(self.project)?;
        let mut record = Record::load(&dir)?;
        let i = record.position(&self.role)?;
        record.role.remove(i);
        let synced = project::install(&dir, &record)?;
        Ok(sync::output(&dir, &record, &synced))
    }
}
