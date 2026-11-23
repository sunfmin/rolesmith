//! `rolesmith use <role>... --into <target>... [--project <dir> | --global] [--rev <sha>]`: record
//! roles in a project's `rolesmith.toml` (or the user's `global.toml`) and install them (SPEC.md §4,
//! §5.1, §5.2, §8); `use --auto` installs what `suggest` lists for a project (§5.5).

use serde_json::Value;

use super::{Command, Failure, Output, sync};
use crate::project::{self, Record, RoleEntry};
use crate::sources::{self, Sources};
use crate::suggestions;

/// How to put several roles together for one agent: `use --help` and `agent-context` print it, from
/// this one string (in GLOSSARY.md's words).
pub const COMPOSING: &str = "\
Composing roles for an agent:

1. Requires or use. A role that every agent of another role needs (\"an iOS app coder is also a
   coder\") is named in that role's `requires` (ADR 0003), never listed again per project. A role
   some projects want and others do not (for example running tests on a real phone) is not
   required by anything; it is named in `rolesmith use` only for the projects that want it.

2. One project per combination. An agent started outside any repository (a harness given a
   directory of roles, such as factorium's `--role <dir>`) gets a directory that holds only
   `rolesmith.toml` and the installs: `rolesmith use A B --into <target> --project <dir>`. Each
   combination of roles is its own directory.

3. Name it by the kind of work, not by the project that first uses it, so another project doing
   the same work reuses it (e.g. `ios-ml-app`, not `chessmirror-roles`).

4. Its entry file (`CLAUDE.md` / `AGENTS.md` beside the installs) lists the roles the directory
   installs and tells the agent to read each one's core skill `rolesmith-<role>`. Required roles
   are not listed: the core skill names them (ADR 0003).

5. Changing a combination makes a new directory rather than editing one agents are running from.
   The old one is removed (`rolesmith remove`, then the directory) only after every agent started
   from it has finished.

6. New versions: `rolesmith source update <source> && rolesmith update --project <dir>`; agents
   started after that read the new version.
";

/// Install roles into a project's agents (or the user's, with --global) and record them; idempotent.
#[derive(clap::Args)]
#[command(after_long_help = COMPOSING)]
pub struct Args {
    /// Each role: <role> when one source has it, <source>/<role> otherwise.
    #[arg(required_unless_present = "auto", conflicts_with = "auto")]
    pub roles: Vec<String>,

    /// Install exactly the roles `suggest` lists for the project.
    #[arg(long, conflicts_with_all = ["rev", "global"])]
    pub auto: bool,

    /// Each agent to install into (claude-code, codex, agents).
    #[arg(long, value_name = "TARGET", required = true, value_parser = known_target)]
    pub into: Vec<String>,

    #[command(flatten)]
    pub place: sync::Where,

    /// Install the roles at this commit of their source (default: a role already recorded keeps
    /// its rev; a new one takes its source's local main).
    #[arg(long, value_name = "SHA")]
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
