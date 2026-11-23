//! What this machine has of what a role needs (SPEC.md §3.5, §7.1): whether a tool's `check`
//! passes here, which mytokens entries exist and which are Profiles, with their field names, all
//! read with `mytokens list`; and, for `env` only, an entry's value.

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::brew;
use crate::commands::Failure;
use crate::host::Host;
use crate::library::{Method, ToolToml, Version};

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
            Ok((Method::Script(_), Version::Exact(v)))
                if !String::from_utf8_lossy(&o.stdout).contains(v) =>
            {
                (false, format!("does not show the pinned version {v}"))
            }
            Ok((Method::Brew(formula), Version::Stable)) => brew::at_stable(host, formula)?,
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

/// The names of the mytokens entries on this machine, from `mytokens list`; never a value.
pub fn entries() -> Result<BTreeSet<String>, Failure> {
    Ok(listed()?.into_iter().map(|l| l.entry).collect())
}

/// The Profiles among the mytokens entries on this machine, each with its field names (the
/// variables `mytokens env <entry>` exports), from `mytokens list`; never a value.
pub fn profiles() -> Result<BTreeMap<String, Vec<String>>, Failure> {
    Ok(listed()?
        .into_iter()
        .filter_map(|l| Some((l.entry, l.profile?)))
        .collect())
}

/// One line of `mytokens list`: `<entry>/<account>\t<kind>\t<level>  — <description>` and, when
/// it has fields, `  fields: <name>, <name>` last.
struct Listed {
    entry: String,
    /// A Profile's field names; `None` for an entry of another kind.
    profile: Option<Vec<String>>,
}

fn listed() -> Result<Vec<Listed>, Failure> {
    Ok(String::from_utf8_lossy(&mytokens(&["list"])?)
        .lines()
        .filter_map(parse_listed)
        .collect())
}

fn parse_listed(line: &str) -> Option<Listed> {
    let mut columns = line.split_whitespace();
    let first = columns.next()?;
    let entry = first.split_once('/').map_or(first, |(entry, _)| entry);
    let profile = (columns.next() == Some("profile")).then(|| {
        line.rsplit_once("  fields: ")
            .map(|(_, names)| names.split(", ").map(|n| n.trim().to_owned()).collect())
            .unwrap_or_default()
    });
    Some(Listed {
        entry: entry.to_owned(),
        profile,
    })
}

/// What `mytokens <args>` prints, or why it could not; its stdin is closed.
fn mytokens(args: &[&str]) -> Result<Vec<u8>, Failure> {
    let call = format!("mytokens {}", args[..args.len().min(2)].join(" "));
    let out = Command::new("mytokens")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Failure::new(format!("cannot run `{call}`: {e}")))?;
    if !out.status.success() {
        return Err(Failure::new(format!(
            "`{call}` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

/// The value of `entry`'s `field` (`""`: its single value), read from mytokens now and kept
/// nowhere. The newline mytokens ends its answer with is not part of the value.
pub fn value(entry: &str, field: &str) -> Result<String, Failure> {
    let mut args = vec!["get", entry];
    if !field.is_empty() {
        args.extend(["--field", field]);
    }
    let out = mytokens(&args)?;
    Ok(String::from_utf8_lossy(out.strip_suffix(b"\n").unwrap_or(&out)).into_owned())
}

/// The command that stores the entry `name` with `fields` (none: one bare value).
pub fn put_command(name: &str, fields: &[String]) -> String {
    if fields.is_empty() {
        format!("mytokens put {name}")
    } else {
        format!("mytokens put {name} --fields \"{}\"", fields.join("\",\""))
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
    fn put_quotes_each_field() {
        assert_eq!(put_command("e", &[]), "mytokens put e");
        assert_eq!(
            put_command("e", &["Key ID".into(), "Issuer ID".into()]),
            "mytokens put e --fields \"Key ID\",\"Issuer ID\""
        );
    }

    #[test]
    fn a_listed_line_gives_its_entry_kind_and_last_fields() {
        let l = parse_listed(
            "apple-dev-signing/default\tprofile\tserver  — see fields: X  fields: APPLE_TEAM, ASC_KEY_ID",
        )
        .unwrap();
        assert_eq!(l.entry, "apple-dev-signing");
        assert_eq!(l.profile.unwrap(), ["APPLE_TEAM", "ASC_KEY_ID"]);
        let l = parse_listed("asc/default\tstatic\tserver  — a key  fields: Key ID").unwrap();
        assert_eq!((l.entry.as_str(), l.profile), ("asc", None));
    }
}
