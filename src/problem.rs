//! A problem a check found: the file, the line where there is one, the rule it breaks and why.

use serde_json::{Value, json};

use crate::secrets;

/// The rules `rolesmith check` enforces, by the name `--json` reports them under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// `library.toml` is present, parses and has a `name` (§2).
    LibraryToml,
    /// A role that was asked for exists.
    RoleMissing,
    /// A role name is lowercase letters, digits and single hyphens, 2–48 characters (§2).
    RoleName,
    /// `role.toml` is present and parses with a `description` (§3.1).
    RoleToml,
    /// The description is one line of at most 200 characters (§3.1).
    RoleDescription,
    /// No top-level key outside the known ones and the consumer tables (§3.1).
    RoleUnknownKey,
    /// Every role a role requires is a role of the same library, and requires form no cycle (§3.1).
    RoleRequires,
    /// `core.md` is present and at most 8,000 characters (§3.2).
    Core,
    /// `prohibitions.toml` parses (§3.3).
    Prohibitions,
    /// A skill's name follows the role name's rule (§3.4).
    SkillName,
    /// `SKILL.md` is present with front matter `name` equal to its directory (§3.4).
    SkillFrontMatter,
    /// The skill's `description` is present and at most 1,024 characters (§3.4).
    SkillDescription,
    /// An external skill's rev is a full SHA its repository has, and its path holds a `SKILL.md`
    /// (§3.4).
    ExternalSkill,
    /// Every program a role names has `tools/<tool>/tool.toml` (§3.5).
    ToolMissing,
    /// `tool.toml` parses (§3.5).
    ToolToml,
    /// A tool's `[install]` names a known method and a pinned version (§3.5).
    ToolUnpinned,
    /// Nothing matches a secret pattern (§7.2).
    Secret,
}

impl Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::LibraryToml => "library-toml",
            Rule::RoleMissing => "role-missing",
            Rule::RoleName => "role-name",
            Rule::RoleToml => "role-toml",
            Rule::RoleDescription => "role-description",
            Rule::RoleUnknownKey => "role-unknown-key",
            Rule::RoleRequires => "role-requires",
            Rule::Core => "core",
            Rule::Prohibitions => "prohibitions",
            Rule::SkillName => "skill-name",
            Rule::SkillFrontMatter => "skill-front-matter",
            Rule::SkillDescription => "skill-description",
            Rule::ExternalSkill => "external-skill",
            Rule::ToolMissing => "tool-missing",
            Rule::ToolToml => "tool-toml",
            Rule::ToolUnpinned => "tool-unpinned",
            Rule::Secret => "secret",
        }
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
        let mut v = json!({ "file": self.file, "rule": self.rule.as_str(), "reason": self.reason });
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
                self.rule.as_str()
            ),
            None => write!(f, "{}: {} [{}]", self.file, self.reason, self.rule.as_str()),
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
