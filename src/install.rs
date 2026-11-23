//! Installing a tool by its pinned method into the user's own prefix (SPEC.md §3.5, ADR 0005):
//! `brew`, or a `script` run with `sh -c`. Everything runs as the user: never `sudo` (`rolesmith
//! check` refuses a script that names it), never a system directory. A tool whose check passes is
//! left alone; one that is installed must pass it after.

use std::process::{Command, Stdio};

use serde_json::Value;

use crate::commands::Failure;
use crate::library::{Method, ToolToml};
use crate::machine;

/// The one program this module runs.
const BREW: &str = "brew";

/// What installing one tool did.
pub struct Done {
    pub name: String,
    /// How it was installed, or `None` when its check already passed.
    pub installed: Option<Installed>,
}

/// What an install did, by its method.
pub enum Installed {
    /// The formula brew installed.
    Brew(String),
    /// The tool's script ran.
    Script,
}

/// How many lines of a failed script's stderr a refusal carries.
const STDERR_TAIL: usize = 10;

/// Run `brew <args>` with no input, keeping its output.
fn brew(args: &[&str]) -> Result<std::process::Output, Failure> {
    Command::new(BREW)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Failure::new(format!("cannot run `{BREW} {}`: {e}", args.join(" "))))
}

/// The stable version `brew info --json=v2` reports for its one formula.
fn offered_version(info: &[u8]) -> Option<String> {
    let doc: Value = serde_json::from_slice(info).ok()?;
    doc["formulae"][0]["versions"]["stable"]
        .as_str()
        .map(str::to_owned)
}

/// The formula that installs `formula` at `version`: `<formula>@<version>` where brew offers it,
/// else `<formula>` when the version brew offers is the pinned one. Anything else is refused
/// before anything is installed.
fn pinned_formula(formula: &str, version: &str) -> Result<String, Failure> {
    let versioned = format!("{formula}@{version}");
    if brew(&["info", "--json=v2", &versioned])?.status.success() {
        return Ok(versioned);
    }
    let info = brew(&["info", "--json=v2", formula])?;
    if !info.status.success() {
        return Err(Failure::new(format!(
            "brew has no formula {formula}: {}",
            String::from_utf8_lossy(&info.stderr).trim()
        )));
    }
    match offered_version(&info.stdout) {
        Some(offered) if offered == version => Ok(formula.to_owned()),
        offered => Err(Failure::new(format!(
            "brew offers {formula} {}, not the pinned {version}, and no {versioned}; nothing installed",
            offered.as_deref().unwrap_or("at no stable version")
        ))),
    }
}

/// Install the tool `name` unless its check passes, and answer what was done.
pub fn install(name: &str, tool: &ToolToml) -> Result<Done, Failure> {
    let done = |installed| Done {
        name: name.to_owned(),
        installed,
    };
    if machine::check_tool(tool).passes {
        return Ok(done(None));
    }
    let (method, version) = tool.install.pinned().map_err(|(_, reason)| {
        Failure::new(format!("tool {name} cannot be installed: {reason}"))
    })?;
    match method {
        Method::Brew(formula) => {
            let target = pinned_formula(formula, version)?;
            let out = brew(&["install", &target])?;
            if !out.status.success() {
                return Err(Failure::new(format!(
                    "`brew install {target}` failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
            let after = machine::check_tool(tool);
            if !after.passes {
                return Err(Failure::new(format!(
                    "installed {target}, but tool {name}'s check `{}` {}",
                    tool.check, after.reason
                )));
            }
            Ok(done(Some(Installed::Brew(target))))
        }
        Method::Script(script) => {
            let out = Command::new("sh")
                .arg("-c")
                .arg(script)
                .stdin(Stdio::null())
                .output()
                .map_err(|e| Failure::new(format!("cannot run tool {name}'s script: {e}")))?;
            let after = machine::check_tool(tool);
            if out.status.success() && after.passes {
                return Ok(done(Some(Installed::Script)));
            }
            let exits = match out.status.code() {
                Some(code) => format!("exits {code}"),
                None => "is killed by a signal".to_owned(),
            };
            Err(Failure::new(format!(
                "ran tool {name}'s script, which {exits}, and its check `{}` {}; {}",
                tool.check,
                after.reason,
                stderr_tail(&out.stderr)
            )))
        }
    }
}

/// The last lines of a script's stderr, as a refusal says them.
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.trim_end().lines().collect();
    if lines.is_empty() {
        return "the script's stderr is empty".to_owned();
    }
    let tail = &lines[lines.len().saturating_sub(STDERR_TAIL)..];
    format!("the script's stderr ends:\n{}", tail.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offered_version_is_the_formulas_stable_one() {
        let info = br#"{"formulae":[{"name":"asc","versions":{"stable":"7.0.0","head":null}}],"casks":[]}"#;
        assert_eq!(offered_version(info).as_deref(), Some("7.0.0"));
        assert_eq!(offered_version(br#"{"formulae":[]}"#), None);
        assert_eq!(offered_version(b"not json"), None);
    }
}
