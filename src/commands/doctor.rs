//! `rolesmith doctor <role> | --project <dir> [--host <host>]`: what a role, or every role a project
//! records, needs (SPEC.md §3.5, §7.1, §8). Each program's `check` passes, on this machine or on the
//! host over ssh; each service and each program's credential has its mytokens entry on this
//! machine, read by name with `mytokens list` only. Anything missing is a refusal, each with its
//! fix.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::host::{self, Host};
use crate::machine::{self, Missing};
use crate::needs::{self, Entry, Needs};
use crate::probe::{self, Kept, Outcome};
use crate::service_map::{self, Asks, Mapped, ServiceMap};
use crate::sources::{self, Found, Sources};

/// Check what a role needs on this machine: each program's `check` passes, and each service and credential has its mytokens entry.
///
/// Run it before working under a role. It runs each program's `check` with `sh -c` (a `script` tool's check must also show its pinned version on stdout) and reads only which entries exist (`mytokens list`), never a value. A service this host maps to a command in its `services.toml` (see `rolesmith env --help`) is not looked for in mytokens: its `check` runs here in place of the command, which doctor runs only with `--probe`, and a failing check is missing (`kind` `service`); one without a `check` is answered `unchecked`, which does not make doctor refuse. It refuses when anything is missing, listing each with the command that fixes it (`rolesmith tool install <source>/<role>`, `mytokens put <entry> --fields ...`); with `--json` as `error.missing`. Do every fix, then run it again. A program without its `tool.toml` is refused as `show` refuses the role.
///
/// ## What a source asks for
///
/// A source that waits on a person (Touch ID, a password, a prompt) stalls an unattended worker, where nobody sees the prompt. doctor names what each mapped service's command asks for (`asks_for`, with a `hint` when it is a person), from the program the command starts with, and `rolesmith env` says the same on stderr before it runs one that asks for a person:
///
/// | source | asks for |
/// | --- | --- |
/// | mytokens | nothing |
/// | `op` with `OP_SERVICE_ACCOUNT_TOKEN` | nothing |
/// | `op` without it | Touch ID or your password, on each read |
/// | `sudo` | a password |
/// | any other command | unknown: probe it |
///
/// `OP_SERVICE_ACCOUNT_TOKEN` counts when it is set in the environment rolesmith runs in, or assigned before `op` in the command (`OP_SERVICE_ACCOUNT_TOKEN="$(cat ~/t)" op inject ...`).
///
/// ## Probing
///
/// With `--probe` doctor reads each mapped service's command once, as `rolesmith env` would, but in a session of its own with no controlling terminal and stdin a pipe nothing writes to, for at most `--timeout` seconds (default 20). It never prints or keeps a value: what the command prints is read for its variable names and dropped. Each service's `probe.outcome` is `unattended` (it finished printing every variable a program's credential expects, in `seconds`), `needs a person` (it did not finish within the bound, and was killed with everything it started; or it failed asking for a terminal), `refused` (it exited non-zero, with its stderr, or printed a line that is not a variable) or `incomplete` (`missing` names the expected variables it did not print). A probe may itself raise a Touch ID or password prompt: the prompt appearing is the answer, that source needs a person; leave it unanswered and the bound ends it. With `--probe` any outcome but `unattended` is missing.
///
/// Each outcome is kept on this machine, with the command it probed and when (`probe.at`), and plain doctor, which runs no source, answers it: a service whose last probe of the same command said `needs a person` is missing, naming when, with the fix (a service account token, or a source that needs no person), so a check before an unattended run holds it instead of starting work that will stall. A service never probed, or probed with another command, is `unchecked`.
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
    /// Read each service this host maps once, without a person, and keep how it went.
    #[arg(long)]
    pub probe: bool,
    /// With --probe: how long a source may take before it is taken to wait on a person.
    #[arg(long, value_name = "SECONDS", default_value_t = 20, requires = "probe", value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout: u64,
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
                    "type": "object", "required": ["name", "command", "state", "asks_for", "probe"],
                    "properties": {
                        "name": { "type": "string" },
                        "command": { "type": "string", "description": "what `rolesmith env` runs for it; doctor runs it only with --probe" },
                        "state": { "enum": ["checked", "unchecked"],
                                   "description": "checked: its check passed; unchecked: it has no check, which does not make doctor refuse" },
                        "asks_for": { "enum": ["nothing", "a person", "unknown"],
                                      "description": "what its command asks for when read, by the table of known commands; unknown: probe it" },
                        "hint": { "type": "string", "description": "with asks_for `a person`: what it asks for and how to read without a person" },
                        "probe": { "type": "object", "required": ["outcome"],
                                   "description": "how reading it went: probed now with --probe, else the probe this host keeps for the same command",
                                   "properties": {
                            "outcome": { "enum": ["unattended", "needs a person", "refused", "incomplete", "unchecked"],
                                         "description": "unchecked: never probed, or probed with another command" },
                            "at": { "type": "string", "description": "when it was probed, UTC (RFC 3339)" },
                            "seconds": { "type": "number", "description": "unattended: how long the read took" },
                            "reason": { "type": "string", "description": "needs a person or refused: why, with its exit and stderr" },
                            "missing": { "type": "array", "items": { "type": "string" },
                                         "description": "incomplete: the expected variables it did not print" }
                        } },
                        "from": { "type": "array", "items": { "type": "string" },
                                  "description": "with --project: the roles that read it, as <source>/<role>" }
                    }
                } }
            }
        })
    }

    fn run(self) -> Result<Output, Failure> {
        let host = Host::from_arg(self.host);
        let probe = self.probe.then(|| Duration::from_secs(self.timeout));
        match self.role {
            Some(role) => one_role(&role, &host, probe),
            None => project(self.project, &host, probe),
        }
    }
}

/// What one run of doctor checks and how it names it.
struct Run<'a> {
    host: &'a Host,
    /// The fix of a missing program.
    fix: String,
    /// Whether each item names the roles it comes from (`--project`).
    with_from: bool,
    /// What was checked, in a refusal's header.
    what: String,
    /// What was read, in the human answer.
    read: String,
    /// With `--probe`: the bound each mapped service is read under.
    probe: Option<Duration>,
    /// The command that probes what this run checks.
    again: String,
}

fn one_role(address: &str, host: &Host, probe: Option<Duration>) -> Result<Output, Failure> {
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
    let run = Run {
        host,
        fix,
        with_from: false,
        read: what.clone(),
        what,
        probe,
        again: format!("rolesmith doctor --probe {}", host::quote_word(address)),
    };
    diagnose(
        &needs,
        &run,
        json!({ "source": found.source, "role": found.name, "rev": found.rev }),
    )
}

fn project(dir: Option<PathBuf>, host: &Host, probe: Option<Duration>) -> Result<Output, Failure> {
    let (dir, read, needs) = needs::of_project(dir)?;
    let shown = dir.display().to_string();
    let fix = format!(
        "rolesmith tool install --project {}{}",
        host::quote_word(&shown),
        host.flag()
    );
    let what = format!("the roles of {shown}");
    let run = Run {
        host,
        fix,
        with_from: true,
        read: format!("{what} ({})", roles_at(&read)),
        what,
        probe,
        again: format!(
            "rolesmith doctor --probe --project {}",
            host::quote_word(&shown)
        ),
    };
    diagnose(
        &needs,
        &run,
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

/// Check `needs` on the run's host (entries on this machine); refuse with everything missing, each
/// with its fix, naming the roles it comes from when `with_from`. Otherwise answer `head` (the keys
/// that say what was read) with the host, the programs, the entries and the mapped services.
fn diagnose(needs: &Needs, run: &Run, head: Value) -> Result<Output, Failure> {
    let host = run.host;
    let from = |roles: &[String]| run.with_from.then(|| roles.to_vec());
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
                fix: run.fix.clone(),
                from: from(&p.from),
            });
        }
        let mut doc = json!({ "name": p.name, "check": p.toml.check });
        if run.with_from {
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
    let mapped: Vec<(&Entry, &Mapped)> = mapped
        .into_iter()
        .map(|e| (e, map.get(&e.name).expect("partitioned on it")))
        .collect();
    let services = services(&mapped, run, &mut missing, &mut human)?;

    if !missing.is_empty() {
        let place = match host.name() {
            None => "on this machine".to_owned(),
            Some(name) => format!("programs on {name} and entries on this machine"),
        };
        let header = format!("{} lacks {} thing(s), {place}:", run.what, missing.len());
        return Err(Failure::listing(
            header,
            "missing",
            &missing,
            Missing::to_json,
        ));
    }
    let mut human = format!("{}: nothing missing\n{human}", run.read);
    for e in &entries {
        human.push_str(&format!(
            "- entry {e}: present; mytokens asks for nothing\n"
        ));
    }
    let mut json = head;
    json["host"] = json!(host.name());
    json["programs"] = json!(programs);
    json["services"] = json!(services);
    json["entries"] = json!(entries);
    Ok(Output { json, human })
}

/// Each mapped service with its check run here (never its command), what the table says its
/// command asks for, and its probe: run now with `--probe` (and kept), else the one kept for the
/// same command. A fresh outcome other than `unattended`, or a kept `needs a person`, is missing.
fn services(
    mapped: &[(&Entry, &Mapped)],
    run: &Run,
    missing: &mut Vec<Missing>,
    human: &mut String,
) -> Result<Vec<Value>, Failure> {
    let file = service_map::file()?;
    let fresh: Vec<(String, Kept)> = match run.probe {
        None => Vec::new(),
        Some(bound) => {
            let probed = mapped
                .iter()
                .map(|(e, m)| {
                    let outcome = probe::probe(&m.command, &e.variables, bound);
                    let at = probe::now();
                    let kept = Kept {
                        command: m.command.clone(),
                        at,
                        outcome,
                    };
                    (e.name.clone(), kept)
                })
                .collect::<Vec<_>>();
            probe::keep(probed.clone())?;
            probed
        }
    };
    let kept = match run.probe {
        Some(_) => fresh.into_iter().collect(),
        None => probe::kept()?,
    };
    let mut services = Vec::new();
    for (e, mapped) in mapped {
        let from = run.with_from.then(|| e.from.clone());
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
                            file.display()
                        ),
                        from: from.clone(),
                    });
                }
                human.push_str(&format!(
                    "- service {}: `{check}` passes; read by `{}`\n",
                    e.name, mapped.command
                ));
                "checked"
            }
        };
        let asks = mapped.asks_for();
        let mut doc = json!({
            "name": e.name, "command": mapped.command, "state": state, "asks_for": asks.name(),
        });
        match asks {
            Asks::Person(hint) => {
                doc["hint"] = json!(hint);
                human.push_str(&format!("  asks for a person: {hint}\n"));
            }
            Asks::Nothing => human.push_str("  asks for nothing\n"),
            Asks::Unknown => {
                human.push_str("  asks for what rolesmith cannot tell from the command\n")
            }
        }
        match kept.get(&e.name).filter(|k| k.command == mapped.command) {
            None => {
                doc["probe"] = json!({ "outcome": "unchecked" });
                human.push_str(&format!(
                    "  probe: unchecked; `{}` reads it once without a person\n",
                    run.again
                ));
            }
            Some(k) => {
                let mut kept_doc = serde_json::to_value(k).expect("serializes");
                kept_doc
                    .as_object_mut()
                    .expect("an object")
                    .remove("command");
                doc["probe"] = kept_doc;
                let (when, refuses) = match run.probe {
                    Some(_) => (
                        String::new(),
                        !matches!(k.outcome, Outcome::Unattended { .. }),
                    ),
                    None => (
                        format!(" (probed {})", k.at),
                        matches!(k.outcome, Outcome::NeedsAPerson { .. }),
                    ),
                };
                let said = format!("{}{when}: {}", k.outcome.name(), k.outcome.detail());
                human.push_str(&format!("  probe: {said}\n"));
                if refuses {
                    missing.push(Missing {
                        kind: "service",
                        name: e.name.clone(),
                        reason: said,
                        fix: probe_fix(&k.outcome, &e.name, mapped, &file, &run.again),
                        from: from.clone(),
                    });
                }
            }
        }
        if let Some(from) = from {
            doc["from"] = json!(from);
        }
        services.push(doc);
    }
    Ok(services)
}

/// What fixes a probe's outcome of `service`, read by `mapped` as `file` maps it.
fn probe_fix(
    outcome: &Outcome,
    service: &str,
    mapped: &Mapped,
    file: &std::path::Path,
    again: &str,
) -> String {
    let change = format!("change service.{service} in {}", file.display());
    let what = match outcome {
        Outcome::NeedsAPerson { .. } => format!(
            "read it without a person: for 1Password, a service account token (OP_SERVICE_ACCOUNT_TOKEN) in the command's environment; else {change} to a source that needs no person"
        ),
        Outcome::Refused { .. } => format!(
            "make `{}` exit 0 without a person, or {change}",
            mapped.command
        ),
        Outcome::Incomplete { missing } => format!(
            "print {} from `{}`, or {change}",
            missing.join(", "),
            mapped.command
        ),
        Outcome::Unattended { .. } => unreachable!("unattended is never missing"),
    };
    format!("{what}; then `{again}`")
}
