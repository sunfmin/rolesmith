//! What this machine has of what a role needs: whether a tool's `check`
//! passes here, which optokens Secrets exist and which are Profiles, with their field names, all
//! read with `optokens list`; and, for `env` only, a Secret's value.

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::brew;
use crate::commands::Failure;
use crate::host::Host;
use crate::library::{Method, STABLE, ToolToml};

/// What running a tool's `check` here answered.
pub struct Check {
    pub passes: bool,
    /// Why it does not pass, or "passes".
    pub reason: String,
}

/// Run `tool`'s check through `sh -c` on `host` with no input. It passes when it exits 0 and, for
/// a script, its stdout contains the pinned version (rolesmith cannot see what a script installed);
/// for a brew tool following `stable`, when brew there has its formula's stable version installed.
/// A pinned brew tool's version is held by its formula, so its check need only pass. Its output is
/// not kept. A host that cannot be reached is a failure, never a check that fails.
pub fn check_tool(host: &Host, tool: &ToolToml) -> Result<Check, Failure> {
    let o = host.sh(&tool.check)?;
    let (passes, reason) = if o.status.success() {
        match tool.install.pinned() {
            Ok((Method::Script(_), v)) if !String::from_utf8_lossy(&o.stdout).contains(v) => {
                (false, format!("does not show the pinned version {v}"))
            }
            Ok((Method::Brew(formula), STABLE)) => brew::at_stable(host, formula)?,
            _ => (true, "passes".to_owned()),
        }
    } else {
        match o.status.code() {
            // sh's answer for a command it cannot find
            Some(127) => (false, "not installed: its command is not found".into()),
            Some(code) => (false, format!("fails: exits {code}")),
            None => (false, "fails: killed by a signal".to_owned()),
        }
    };
    Ok(Check { passes, reason })
}

/// The names of the optokens Secrets on this machine under the account `default` (the one
/// `optokens get <entry>` reads), from `optokens list`; never a value.
pub fn entries() -> Result<BTreeSet<String>, Failure> {
    Ok(listed()?.into_iter().map(|s| s.service).collect())
}

/// The Profiles among the optokens Secrets on this machine, each with its field names (the
/// variables `optokens env <entry>` exports), from `optokens list`; never a value.
pub fn profiles() -> Result<BTreeMap<String, Vec<String>>, Failure> {
    Ok(listed()?
        .into_iter()
        .filter(|s| s.kind == "profile")
        .map(|s| (s.service, s.fields.unwrap_or_default()))
        .collect())
}

/// What `optokens list` prints: one JSON object, `{"secrets": [...]}`; never a value.
#[derive(Deserialize)]
struct Listing {
    secrets: Vec<Secret>,
}

/// One Secret of `optokens list`.
#[derive(Deserialize)]
struct Secret {
    service: String,
    account: String,
    /// `static` or `profile`.
    kind: String,
    /// Its fields' labels; `None` for a single value.
    fields: Option<Vec<String>>,
}

/// The Secrets under the account `default`: a Secret under another account is not what
/// `optokens get <service>` reads.
fn listed() -> Result<Vec<Secret>, Failure> {
    let out = optokens(&["list"])?;
    let listing: Listing = serde_json::from_slice(&out)
        .map_err(|e| Failure::new(format!("`optokens list` printed no listing: {e}")))?;
    Ok(listing
        .secrets
        .into_iter()
        .filter(|s| s.account == "default")
        .collect())
}

/// What `optokens <args>` prints, or why it could not; its stdin is closed. A failure names each
/// fault and fix of the error object optokens prints (on stdout for `list`, on stderr for `get`),
/// or its stderr when it printed none.
fn optokens(args: &[&str]) -> Result<Vec<u8>, Failure> {
    let call = format!("optokens {}", args[..args.len().min(2)].join(" "));
    let out = Command::new("optokens")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Failure::new(format!("cannot run `{call}`: {e}")))?;
    if !out.status.success() {
        let why = faults(&out.stderr)
            .or_else(|| faults(&out.stdout))
            .unwrap_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_owned());
        return Err(Failure::new(format!("`{call}` failed: {why}")));
    }
    Ok(out.stdout)
}

/// `<fault>; fix: <fix>` for each fault of optokens' error object
/// `{"error": [{"fault", "fix", "kind"}]}`, or `None` when `printed` is not one.
fn faults(printed: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Errors {
        error: Vec<Fault>,
    }
    #[derive(Deserialize)]
    struct Fault {
        fault: String,
        fix: String,
    }
    let errors: Errors = serde_json::from_slice(printed).ok()?;
    (!errors.error.is_empty()).then(|| {
        (errors.error.iter())
            .map(|f| format!("{}; fix: {}", f.fault, f.fix))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

/// The value of `entry`'s `field` (`""`: its single value), read from optokens now and kept
/// nowhere. optokens prints it as it is, with no newline of its own, so every byte is the value's.
pub fn value(entry: &str, field: &str) -> Result<String, Failure> {
    let mut args = vec!["get", entry];
    if !field.is_empty() {
        args.extend(["--field", field]);
    }
    Ok(String::from_utf8_lossy(&optokens(&args)?).into_owned())
}

/// The command that stores the Secret `name` with `fields` (none: one bare value), the labels
/// comma-joined in one argument, as `optokens put` takes them.
pub fn put_command(name: &str, fields: &[String]) -> String {
    if fields.is_empty() {
        format!("optokens put {name}")
    } else {
        format!("optokens put {name} --fields \"{}\"", fields.join(","))
    }
}

/// One thing a role needs that this machine lacks, with the command that fixes it.
pub struct Missing {
    /// `program`, `entry` or `service` (a service this host maps whose check fails).
    pub kind: &'static str,
    pub name: String,
    pub reason: String,
    pub fix: String,
    /// The roles that need it, as `<source>/<role>`, when more than one role was checked
    /// (`doctor --project`); `None` for one role.
    pub from: Option<Vec<String>>,
}

impl Missing {
    pub fn to_json(&self) -> Value {
        let mut doc =
            json!({ "kind": self.kind, "name": self.name, "reason": self.reason, "fix": self.fix });
        if let Some(from) = &self.from {
            doc["from"] = json!(from);
        }
        doc
    }

    /// The JSON Schema of [`Missing::to_json`].
    pub fn shape() -> Value {
        json!({
            "type": "object",
            "required": ["kind", "name", "reason", "fix"],
            "properties": {
                "kind": { "enum": ["program", "entry", "service"] },
                "name": { "type": "string" },
                "reason": { "type": "string" },
                "fix": { "type": "string", "description": "the command that fixes it" },
                "from": { "type": "array", "items": { "type": "string" },
                          "description": "with --project: the roles that need it, as <source>/<role>" }
            }
        })
    }
}

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: {}", self.kind, self.name, self.reason)?;
        if let Some(from) = &self.from {
            write!(f, " (from {})", from.join(", "))?;
        }
        write!(f, "; fix: {}", self.fix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_joins_the_fields_in_one_argument() {
        assert_eq!(put_command("e", &[]), "optokens put e");
        assert_eq!(
            put_command("e", &["Key ID".into(), "Issuer ID".into()]),
            "optokens put e --fields \"Key ID,Issuer ID\""
        );
    }

    #[test]
    fn an_error_object_gives_each_fault_and_fix() {
        assert_eq!(
            faults(br#"{"error":[{"fault":"F","fix":"X","kind":"k"},{"fault":"G","fix":"Y"}]}"#)
                .as_deref(),
            Some("F; fix: X; G; fix: Y")
        );
        assert_eq!(faults(b"locked"), None);
        assert_eq!(faults(br#"{"error":[]}"#), None);
    }
}
