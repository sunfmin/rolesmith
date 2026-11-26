//! `rolesmith remove <role> [--project <dir> | --global]`: drop a role from the record
//! (`rolesmith.toml` or `global.toml`) and delete its generated files, only those the manifest
//! lists.

use serde_json::Value;

use super::{Command, Failure, Output, sync};
use crate::project::{self, Record};

/// Drop a role from the record and delete its generated files.
///
/// Only files the manifest lists are deleted, and a role another remaining role requires keeps its files. A record left with no roles is deleted, with its manifest, the `.gitignore` lines rolesmith added and the managed section of each entry file (the file itself when the section was all it held): the project is as it was before `use`. It refuses a role the record does not name and one installed only because others require it, naming those.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
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
        let i = project::named_position(&place, &record, &self.role)?;
        record.role.remove(i);
        sync::install(&place, &record)
    }
}
