//! Installing a tool by its pinned method into the user's own prefix (SPEC.md §3.5). `brew` is the
//! one method. The only program run here is `brew`, as the user: never `sudo`, never a system
//! directory. A tool whose `check` passes is left alone; one that is installed must pass it after.

use std::process::{Command, Stdio};

use serde_json::Value;

use crate::commands::Failure;
use crate::library::ToolToml;
use crate::machine;

/// The one program this module runs.
const BREW: &str = "brew";

/// What installing one tool did.
pub struct Done {
    pub name: String,
    /// The formula installed, or `None` when the tool's check already passed.
    pub formula: Option<String>,
}

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
    if machine::check(&tool.check).passes {
        return Ok(Done {
            name: name.to_owned(),
            formula: None,
        });
    }
    let (Some(formula), Some(version)) = (&tool.install.brew, &tool.install.version) else {
        return Err(Failure::new(format!(
            "tool {name} is not pinned: [install] needs `brew` and `version`"
        )));
    };
    if version.is_empty() {
        return Err(Failure::new(format!(
            "tool {name} is not pinned: its `version` is empty"
        )));
    }
    let target = pinned_formula(formula, version)?;
    let out = brew(&["install", &target])?;
    if !out.status.success() {
        return Err(Failure::new(format!(
            "`brew install {target}` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let after = machine::check(&tool.check);
    if !after.passes {
        return Err(Failure::new(format!(
            "installed {target}, but tool {name}'s check `{}` {}",
            tool.check, after.reason
        )));
    }
    Ok(Done {
        name: name.to_owned(),
        formula: Some(target),
    })
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
