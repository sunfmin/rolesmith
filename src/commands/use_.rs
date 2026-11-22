//! `rolesmith use <role>... --into <target>... [--project <dir>] [--rev <sha>]`: record roles in
//! a project's `rolesmith.toml` and install them (SPEC.md §4, §5.1, §8).

use std::path::PathBuf;

use serde_json::Value;

use super::{Command, Failure, Output, sync};
use crate::project::{self, Record, RoleEntry};
use crate::sources::{self, Sources};

/// Install roles into a project's agents and record them in its rolesmith.toml; idempotent.
#[derive(clap::Args)]
pub struct Args {
    /// Each role: <role> when one source has it, <source>/<role> otherwise.
    #[arg(required = true)]
    pub roles: Vec<String>,

    /// Each agent to install into (claude-code).
    #[arg(long, value_name = "TARGET", required = true)]
    pub into: Vec<String>,

    /// The project's root (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,

    /// Install the roles at this commit of their source (default: a role already recorded keeps
    /// its rev; a new one takes its source's local main).
    #[arg(long, value_name = "SHA")]
    pub rev: Option<String>,
}

impl Command for Args {
    const NAME: &'static str = "use";

    fn output_shape() -> Value {
        sync::shape()
    }

    fn run(self) -> Result<Output, Failure> {
        for target in &self.into {
            project::target_dir(target)?;
        }
        let dir = sync::project_dir(self.project)?;
        let mut record = Record::load(&dir)?;
        let known = Sources::load()?;

        for address in &self.roles {
            let name = address.rsplit('/').next().unwrap_or(address);
            let recorded = record.role.iter().position(|r| r.name == name);
            // A recorded role keeps its rev; a bare name reaches it in its recorded source.
            let address = match (recorded, address.contains('/')) {
                (Some(i), false) => format!("{}/{name}", record.role[i].source),
                _ => address.clone(),
            };
            let rev = self.rev.as_deref().or_else(|| {
                recorded
                    .filter(|&i| address == format!("{}/{name}", record.role[i].source))
                    .map(|i| record.role[i].rev.as_str())
            });
            let found = sources::resolve(&known, &address, rev)?;
            let url = known.get(&found.source)?.url.clone();
            let entry = match recorded {
                Some(i) if record.role[i].source != found.source => {
                    return Err(Failure::new(format!(
                        "{} already records {name} from source {}; a role name is installed once",
                        project::RECORD,
                        record.role[i].source
                    )));
                }
                Some(i) => {
                    let entry = &mut record.role[i];
                    entry.rev = found.rev;
                    entry.url = url;
                    entry
                }
                None => {
                    record.role.push(RoleEntry {
                        name: found.name,
                        source: found.source,
                        url,
                        rev: found.rev,
                        into: Vec::new(),
                    });
                    record.role.last_mut().expect("just pushed")
                }
            };
            for target in &self.into {
                if !entry.into.contains(target) {
                    entry.into.push(target.clone());
                }
            }
        }
        // A record whose install fails is put back as it was.
        let file = dir.join(project::RECORD);
        let before = std::fs::read(&file).ok();
        record.save(&dir)?;
        let synced = project::sync(&dir, &record).inspect_err(|_| {
            let _ = match &before {
                Some(bytes) => std::fs::write(&file, bytes),
                None => std::fs::remove_file(&file),
            };
        })?;
        Ok(sync::output(&dir, &record, &synced))
    }
}
