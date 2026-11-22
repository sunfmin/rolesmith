//! `rolesmith remove <role> [--project <dir> | --global]`: drop a role from the record
//! (`rolesmith.toml` or `global.toml`) and delete its generated files, only those the manifest
//! lists (SPEC.md §4, §8).

use serde_json::Value;

use super::{Command, Failure, Output, sync};
use crate::project::Record;

/// Drop a role from rolesmith.toml and delete its generated files.
#[derive(clap::Args)]
pub struct Args {
    /// The role, by its name in rolesmith.toml.
    pub role: String,

    #[command(flatten)]
    pub place: sync::Where,
}

impl Command for Args {
    const NAME: &'static str = "remove";

    fn output_shape() -> Value {
        sync::shape()
    }

    fn run(self) -> Result<Output, Failure> {
        let place = self.place.place()?;
        let mut record = Record::load(&place)?;
        let i = record.position(&place, &self.role)?;
        record.role.remove(i);
        sync::install(&place, &record)
    }
}
