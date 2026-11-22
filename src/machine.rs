//! What this machine has of what a role needs (SPEC.md §3.5, §7.1): whether a tool's `check`
//! passes here, and which mytokens entries exist. An entry is known only by its name, read with
//! `mytokens list`; its value is never read.

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::commands::Failure;

/// What running a tool's `check` here answered.
pub struct Check {
    pub passes: bool,
    /// Why it does not pass, or "passes".
    pub reason: String,
}

/// Run `command` through `sh -c` with no input; it passes when it exits 0. Its output is not kept.
pub fn check(command: &str) -> Check {
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let (passes, reason) = match status {
        Ok(s) if s.success() => (true, "passes".to_owned()),
        // sh's answer for a command it cannot find
        Ok(s) if s.code() == Some(127) => (false, "not installed: its command is not found".into()),
        Ok(s) => match s.code() {
            Some(code) => (false, format!("fails: exits {code}")),
            None => (false, "fails: killed by a signal".to_owned()),
        },
        Err(e) => (false, format!("cannot be run: {e}")),
    };
    Check { passes, reason }
}

/// The names of the mytokens entries on this machine, from `mytokens list` (each line begins
/// `<entry>/<account>`); never a value.
pub fn entries() -> Result<BTreeSet<String>, Failure> {
    let out = Command::new("mytokens")
        .arg("list")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Failure::new(format!("cannot run `mytokens list`: {e}")))?;
    if !out.status.success() {
        return Err(Failure::new(format!(
            "`mytokens list` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(|first| first.split_once('/').map_or(first, |(entry, _)| entry))
        .map(str::to_owned)
        .collect())
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
    /// `program` or `entry`.
    pub kind: &'static str,
    pub name: String,
    pub reason: String,
    pub fix: String,
}

impl Missing {
    pub fn to_json(&self) -> Value {
        json!({ "kind": self.kind, "name": self.name, "reason": self.reason, "fix": self.fix })
    }

    /// The JSON Schema of [`Missing::to_json`].
    pub fn shape() -> Value {
        json!({
            "type": "object",
            "required": ["kind", "name", "reason", "fix"],
            "properties": {
                "kind": { "enum": ["program", "entry"] },
                "name": { "type": "string" },
                "reason": { "type": "string" },
                "fix": { "type": "string", "description": "the command that fixes it" }
            }
        })
    }
}

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}: {}; fix: {}",
            self.kind, self.name, self.reason, self.fix
        )
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
}
