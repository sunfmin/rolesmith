//! A problem a check found: the file, the line where there is one, the rule it breaks and why.

use serde_json::{Value, json};

use crate::secrets;

/// The rules `rolesmith check` enforces. Each variant's doc comment is its line in help; its name,
/// in kebab-case, is what a problem reports it under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, agent_help::Help)]
pub enum Rule {
    /// `library.toml` is present, parses and has a `name`.
    LibraryToml,
    /// A role that was asked for exists.
    RoleMissing,
    /// A role name is lowercase letters, digits and single hyphens, 2–48 characters.
    RoleName,
    /// `role.toml` is present and parses with a `description`.
    RoleToml,
    /// The description is one line of at most 200 characters.
    RoleDescription,
    /// No top-level key outside the known ones and the consumer tables.
    RoleUnknownKey,
    /// Every role a role requires is a role of the same library, and requires form no cycle.
    RoleRequires,
    /// `core.md` is present and at most 8,000 characters.
    Core,
    /// `prohibitions.toml` parses.
    Prohibitions,
    /// A skill's name follows the role name's rule.
    SkillName,
    /// `SKILL.md` is present with front matter `name` equal to its directory.
    SkillFrontMatter,
    /// The skill's `description` is present and at most 1,024 characters.
    SkillDescription,
    /// An external skill's rev is a full SHA its repository has, and its path holds a `SKILL.md`.
    ExternalSkill,
    /// Every program a role names has `tools/<tool>/tool.toml`.
    ToolMissing,
    /// `tool.toml` parses.
    ToolToml,
    /// A tool's `[install]` names one method, `brew` or `script`, and a pinned `version`.
    ToolUnpinned,
    /// A tool's install `script` never runs `sudo`: rolesmith installs as the user.
    ToolSudo,
    /// Nothing matches a secret pattern.
    Secret,
}

impl Rule {
    /// The rule's name, as a problem reports it: its variant in kebab-case.
    pub fn name(self) -> String {
        agent_help::Table::entry(&self).name.replace('_', "-")
    }

    /// Every rule, one line each, as help lists them.
    pub fn help() -> String {
        <Rule as agent_help::Table>::ENTRIES
            .iter()
            .map(|e| format!("- `{}`: {}\n", e.name.replace('_', "-"), e.markdown))
            .collect()
    }
}

/// One problem. Its text never carries what a secret pattern matches: a file name or reason that
/// would is withheld when the problem is made, so no output can echo a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The file, relative to the library root, `/`-separated.
    pub file: String,
    pub line: Option<usize>,
    pub rule: Rule,
    pub reason: String,
}

const WITHHELD: &str = "(withheld: it matches a secret pattern)";

impl Problem {
    pub fn new(
        file: impl Into<String>,
        line: Option<usize>,
        rule: Rule,
        reason: impl Into<String>,
    ) -> Self {
        let withhold = |s: String| {
            if secrets::matches(&s).is_empty() {
                s
            } else {
                WITHHELD.to_owned()
            }
        };
        Problem {
            file: withhold(file.into()),
            line,
            rule,
            reason: withhold(reason.into()),
        }
    }

    pub fn to_json(&self) -> Value {
        let mut v = json!({ "file": self.file, "rule": self.rule.name(), "reason": self.reason });
        if let Some(line) = self.line {
            v["line"] = json!(line);
        }
        v
    }

    /// The JSON Schema of [`Problem::to_json`].
    pub fn shape() -> Value {
        json!({
            "type": "object",
            "required": ["file", "rule", "reason"],
            "properties": {
                "file": { "type": "string", "description": "relative to the library root" },
                "line": { "type": "integer", "minimum": 1, "description": "where there is one" },
                "rule": { "type": "string" },
                "reason": { "type": "string" }
            }
        })
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(
                f,
                "{}:{line}: {} [{}]",
                self.file,
                self.reason,
                self.rule.name()
            ),
            None => write!(f, "{}: {} [{}]", self.file, self.reason, self.rule.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reason_carrying_a_secret_is_withheld() {
        let key = format!("ghp_{}", "z".repeat(36));
        let p = Problem::new(
            "roles/r/role.toml",
            None,
            Rule::RoleUnknownKey,
            format!("unknown key {key}"),
        );
        assert!(!p.reason.contains(&key));
        assert!(!p.to_string().contains(&key));
    }
}
