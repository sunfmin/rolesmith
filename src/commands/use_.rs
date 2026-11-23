//! `rolesmith use <role>... --into <target>... [--project <dir> | --global] [--rev <rev>]`: record
//! roles in a project's `rolesmith.toml` (or the user's `global.toml`) and install them (SPEC.md §4,
//! §5.1, §5.2, §8); `use --auto` installs what `suggest` lists for a project (§5.5).

use serde_json::Value;

use super::{Command, Failure, Output, sync};
use crate::project::{self, Record, RoleEntry};
use crate::sources::{self, Sources};
use crate::suggestions;

/// Install roles into a project's agents (or the user's own, with `--global`) and record them.
///
/// Run it to give a project's agents roles; to put several roles together for one agent, read [composing roles](help:composing) first. `--into` names each agent: {const:TARGETS}; under `--global` the same directories under your home.
///
/// It records each role in the project's `rolesmith.toml` (or `global.toml`) with its source, URL, the full SHA it installed and its targets, then writes, for each target, the role's core skill `rolesmith-<role>` and a skill `rolesmith-<role>-<skill>` per skill, and the same for every role it requires. Generated files are listed in `.rolesmith/manifest.json` and added to `.gitignore`; commit `rolesmith.toml`, never the generated files. Into the entry file of each target (`CLAUDE.md` for `claude-code`, `AGENTS.md` for `codex` and `agents`; the other one when only it exists and imports or links it) it writes a managed section between two fixed lines, the same in every project, that tells an agent on a fresh clone to install or upgrade rolesmith, run `rolesmith sync`, read each role's core skill and run `rolesmith doctor`; commit it. It never edits anything outside that section, nor the project's code or settings.
///
/// It is idempotent: a role already recorded keeps its rev unless `--rev` is given (`rolesmith update` moves it). `--auto` installs exactly what `rolesmith suggest` lists.
///
/// ## Refusals
///
/// - an unknown role, or a bare name more than one source has;
/// - a `--rev` that names no commit in the clone (the refusal names it and the source);
/// - a role name the record already has from another source;
/// - a file in an install directory that the manifest does not list (it is never overwritten);
/// - two roles requiring one role at two revs;
/// - `--auto` with nothing to install (and it writes nothing).
///
/// An unknown `--into` is a usage error naming the targets.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    /// Each role: <role> when one source has it, <source>/<role> otherwise.
    #[arg(required_unless_present = "auto", conflicts_with = "auto")]
    pub roles: Vec<String>,

    /// Install exactly the roles `suggest` lists for the project.
    #[arg(long, conflicts_with_all = ["rev", "global"])]
    pub auto: bool,

    /// Each agent to install into, repeatable: one of the targets this page names.
    #[arg(long, value_name = "TARGET", required = true, value_parser = known_target)]
    pub into: Vec<String>,

    #[command(flatten)]
    pub place: sync::Where,

    /// Install the roles at this commit of their source: any name git resolves in the clone (a
    /// SHA, a branch such as main, a tag, HEAD); the record keeps the full SHA it resolved to,
    /// never the name (default: a role already recorded keeps its rev; a new one takes its
    /// source's local main).
    #[arg(long, value_name = "REV")]
    pub rev: Option<String>,
}

/// A `--into` value: a known target, or a usage error naming the known ones.
fn known_target(target: &str) -> Result<String, String> {
    project::target_dir(target)
        .map(|_| target.to_owned())
        .map_err(|f| f.message)
}

impl Command for Args {
    const NAME: &'static str = "use";

    fn output_shape() -> Value {
        sync::shape()
    }

    fn run(self) -> Result<Output, Failure> {
        let place = self.place.place()?;
        let mut record = Record::load(&place)?;
        let known = Sources::load()?;
        let roles = if self.auto {
            let found = suggestions::suggest(&place.root)?;
            if found.is_empty() {
                return Err(Failure::new(format!(
                    "no role matches {} (`rolesmith suggest`); nothing installed",
                    place.root.display()
                )));
            }
            found.iter().map(|s| s.qualified()).collect()
        } else {
            self.roles
        };

        for address in &roles {
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
                        place.record_name(),
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
        sync::install(&place, &record)
    }
}
