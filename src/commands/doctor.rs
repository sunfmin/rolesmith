//! `rolesmith doctor <role> | --project <dir> [--host <host>]`: what a role, or every role a project
//! records, needs (SPEC.md §3.5, §7.1, §8). Each program's `check` passes, on this machine or on the
//! host over ssh; each service and each program's credential has its mytokens entry on this
//! machine, read by name with `mytokens list` only. Anything missing is a refusal, each with its
//! fix.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::host::{self, Host};
use crate::machine::{self, Missing};
use crate::needs::{self, Needs};
use crate::service_map::{self, ServiceMap};
use crate::sources::{self, Found, Sources};

/// Check what a role needs on this machine: each program's `check` passes, and each service and credential has its mytokens entry.
///
/// Run it before working under a role. It runs each program's `check` with `sh -c` (a `script` tool's check must also show its pinned version on stdout) and reads only which entries exist (`mytokens list`), never a value. A service this host maps to a command in its `services.toml` (see `rolesmith env --help`) is not looked for in mytokens: its `check` runs here in place of the command, which doctor never runs, and a failing check is missing (`kind` `service`); one without a `check` is answered `unchecked`, which does not make doctor refuse. It refuses when anything is missing, listing each with the command that fixes it (`rolesmith tool install <source>/<role>`, `mytokens put <entry> --fields ...`); with `--json` as `error.missing`. Do every fix, then run it again. A program without its `tool.toml` is refused as `show` refuses the role.
///
/// ## A project's roles
///
/// With `--project <dir>` in place of a role it checks every role the project's `rolesmith.toml` records, each at its recorded rev (never the source's `main`), and every role each requires, at that same rev: one command for all a project's agents need. A tool two roles name is checked once, and an entry two roles read is looked for once. Its answer is the shape a role's is, with `project` and the `roles` it read in place of `source`, `role` and `rev`; each program, and each item of `error.missing`, adds `from`, the roles that need it. A program's fix is then `rolesmith tool install --project <dir>`. A directory without `rolesmith.toml` is refused. A source the record names that this machine lacks is added from its `url`, as `rolesmith sync` adds it.
///
/// ## Another host
///
/// With `--host <host>` each `check` runs on that machine instead, as `ssh -o BatchMode=yes -- <host> sh -c '<check>'`, for work that runs there (a build on the machine a device is plugged into). The host needs `sh` and ssh access by key, not rolesmith; the check sees the `PATH` that a command run over ssh gets there, so a program or brew reached only by an interactive shell's profile is missing. A program's fix then carries `--host <host>`. Entries are still read on this machine: a credential never leaves it, and whoever runs work on the host brings the values (`rolesmith env`). A host ssh cannot reach fails, naming the host, rather than listing everything as missing.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    /// The role: <role> when one source has it, <source>/<role> otherwise.
    #[arg(required_unless_present = "project", conflicts_with = "project")]
    pub role: Option<String>,
    /// Every role this project's rolesmith.toml records, at its rev, instead of one role.
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,
    /// Run the checks on this host over ssh instead of on this machine.
    #[arg(long)]
    pub host: Option<String>,
}

impl Command for Args {
    const NAME: &'static str = "doctor";

    fn output_shape() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "nothing is missing; roles that lack anything here exit 1 and list it under error.missing",
            "type": "object",
            "required": ["host", "programs", "entries", "services"],
            "properties": {
                "source": { "type": "string", "description": "with a role" },
                "role": { "type": "string", "description": "with a role" },
                "rev": { "type": "string", "description": "with a role: the full SHA it was read at" },
                "project": { "type": "string", "description": "with --project: the project's root" },
                "roles": { "type": "array", "description": "with --project: each role read, recorded or required, at its rev",
                           "items": {
                    "type": "object", "required": ["source", "role", "rev"],
                    "properties": { "source": { "type": "string" }, "role": { "type": "string" },
                                    "rev": { "type": "string" } }
                } },
                "host": { "type": ["string", "null"], "description": "the host the checks ran on; null for this machine" },
                "programs": { "type": "array", "items": {
                    "type": "object", "required": ["name", "check"],
                    "properties": {
                        "name": { "type": "string" },
                        "check": { "type": "string" },
                        "from": { "type": "array", "items": { "type": "string" },
                                  "description": "with --project: the roles that name it, as <source>/<role>" }
                    }
                } },
                "entries": { "type": "array", "items": { "type": "string" },
                             "description": "the mytokens entries the roles read, each present" },
                "services": { "type": "array", "description": "each service this host maps to a command (services.toml), read from it in place of mytokens",
                              "items": {
                    "type": "object", "required": ["name", "command", "state"],
                    "properties": {
                        "name": { "type": "string" },
                        "command": { "type": "string", "description": "what `rolesmith env` runs for it; doctor never runs it" },
                        "state": { "enum": ["checked", "unchecked"],
                                   "description": "checked: its check passed; unchecked: it has no check, which does not make doctor refuse" },
                        "from": { "type": "array", "items": { "type": "string" },
                                  "description": "with --project: the roles that read it, as <source>/<role>" }
                    }
                } }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let host = Host::from_arg(self.host);
        match self.role {
            Some(role) => one_role(&role, &host),
            None => project(self.project, &host),
        }
    }
}

fn one_role(address: &str, host: &Host) -> Result<Output, Failure> {
    let record = Sources::load()?;
    let found = sources::resolve(&record, address, None)?;
    let (lib, _tree) = sources::read_role(&found)?;
    let mut needs = Needs::default();
    needs.add(&found.source, &lib, &lib.roles[0]);
    let fix = format!(
        "rolesmith tool install {}{}",
        found.qualified(),
        host.flag()
    );
    let what = format!("{} at {}", found.qualified(), found.rev);
    diagnose(
        &needs,
        host,
        &fix,
        false,
        &what,
        &what,
        json!({ "source": found.source, "role": found.name, "rev": found.rev }),
    )
}

fn project(dir: Option<PathBuf>, host: &Host) -> Result<Output, Failure> {
    let (dir, read, needs) = needs::of_project(dir)?;
    let shown = dir.display().to_string();
    let fix = format!(
        "rolesmith tool install --project {}{}",
        host::quote_word(&shown),
        host.flag()
    );
    let what = format!("the roles of {shown}");
    diagnose(
        &needs,
        host,
        &fix,
        true,
        &what,
        &format!("{what} ({})", roles_at(&read)),
        json!({
            "project": shown,
            "roles": read.iter().map(|f| json!({ "source": f.source, "role": f.name, "rev": f.rev })).collect::<Vec<_>>(),
        }),
    )
}

/// Each role read, as `<source>/<role>@<short rev>`.
fn roles_at(read: &[Found]) -> String {
    read.iter()
        .map(|f| format!("{}@{}", f.qualified(), &f.rev[..f.rev.len().min(12)]))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Check `needs` on `host` (entries on this machine); refuse with everything missing, each with
/// its fix (`fix` for a program), naming the roles it comes from when `with_from` and what was
/// checked as `what`. Otherwise answer `head` (the keys that say what was read, `read` in the human
/// answer) with the host, the programs, the entries and the mapped services.
fn diagnose(
    needs: &Needs,
    host: &Host,
    fix: &str,
    with_from: bool,
    what: &str,
    read: &str,
    head: Value,
) -> Result<Output, Failure> {
    let from = |roles: &[String]| with_from.then(|| roles.to_vec());
    let mut missing = Vec::new();
    let mut programs = Vec::new();
    let mut human = String::new();
    for p in &needs.programs {
        let check = machine::check_tool(host, &p.toml)?;
        if !check.passes {
            missing.push(Missing {
                kind: "program",
                name: p.name.clone(),
                reason: format!("check `{}`{} {}", p.toml.check, host.on(), check.reason),
                fix: fix.to_owned(),
                from: from(&p.from),
            });
        }
        let mut doc = json!({ "name": p.name, "check": p.toml.check });
        if with_from {
            doc["from"] = json!(p.from);
        }
        programs.push(doc);
        human.push_str(&format!(
            "- program {}: `{}` passes{}\n",
            p.name,
            p.toml.check,
            host.on()
        ));
    }

    let map = ServiceMap::load()?;
    let (mapped, in_mytokens): (Vec<_>, Vec<_>) =
        (needs.entries.iter()).partition(|e| map.get(&e.name).is_some());
    let entries: Vec<String> = in_mytokens.iter().map(|e| e.name.clone()).collect();
    if !in_mytokens.is_empty() {
        let present = machine::entries()?;
        for e in in_mytokens {
            if !present.contains(&e.name) {
                missing.push(Missing {
                    kind: "entry",
                    name: e.name.clone(),
                    reason: "no mytokens entry".to_owned(),
                    fix: machine::put_command(&e.name, &e.fields),
                    from: from(&e.from),
                });
            }
        }
    }
    let mut services = Vec::new();
    for e in mapped {
        let mapped = map.get(&e.name).expect("partitioned on it");
        // Only the check runs, here, where the command would: never the command.
        let state = match &mapped.check {
            None => {
                human.push_str(&format!(
                    "- service {}: unchecked: read by `{}`, with no check\n",
                    e.name, mapped.command
                ));
                "unchecked"
            }
            Some(check) => {
                let o = Host::Here.sh(check)?;
                if !o.status.success() {
                    missing.push(Missing {
                        kind: "service",
                        name: e.name.clone(),
                        reason: format!(
                            "check `{check}` exits {}",
                            o.status
                                .code()
                                .map_or("by a signal".to_owned(), |c| c.to_string())
                        ),
                        fix: format!(
                            "make `{check}` exit 0, or change service.{} in {}",
                            e.name,
                            service_map::file()?.display()
                        ),
                        from: from(&e.from),
                    });
                }
                human.push_str(&format!("- service {}: `{check}` passes\n", e.name));
                "checked"
            }
        };
        let mut doc = json!({ "name": e.name, "command": mapped.command, "state": state });
        if with_from {
            doc["from"] = json!(e.from);
        }
        services.push(doc);
    }

    if !missing.is_empty() {
        let place = match host.name() {
            None => "on this machine".to_owned(),
            Some(name) => format!("programs on {name} and entries on this machine"),
        };
        let header = format!("{what} lacks {} thing(s), {place}:", missing.len());
        return Err(Failure::listing(
            header,
            "missing",
            &missing,
            Missing::to_json,
        ));
    }
    let mut human = format!("{read}: nothing missing\n{human}");
    for e in &entries {
        human.push_str(&format!("- entry {e}: present\n"));
    }
    let mut json = head;
    json["host"] = json!(host.name());
    json["programs"] = json!(programs);
    json["services"] = json!(services);
    json["entries"] = json!(entries);
    Ok(Output { json, human })
}
