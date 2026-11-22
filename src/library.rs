//! A library working copy, read and validated (SPEC.md §2, §3, §7.2).
//!
//! [`load`] is how every command reads a library or one of its roles: it answers the parsed
//! [`Library`] when nothing is wrong, and otherwise every [`Problem`] it found, not only the first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use yaml_rust2::YamlLoader;

use crate::problem::{Problem, Rule};
use crate::secrets;

/// Top-level tables of `role.toml` that belong to another consumer: rolesmith keeps them untouched
/// and ignores them (§3.1). The one place they are listed.
pub const CONSUMER_TABLES: &[&str] = &["guild"];

/// At most this many characters in a role's `description` (§3.1).
pub const MAX_ROLE_DESCRIPTION: usize = 200;
/// At most this many characters in `core.md` (§3.2).
pub const MAX_CORE: usize = 8_000;
/// At most this many characters in a skill's `description` (§3.4).
pub const MAX_SKILL_DESCRIPTION: usize = 1_024;
/// A role or skill name is 2 to this many characters (§2).
pub const MAX_NAME: usize = 48;

/// A library that passed every check.
#[derive(Debug)]
pub struct Library {
    pub root: PathBuf,
    pub toml: LibraryToml,
    /// Every role, or only the one asked for.
    pub roles: Vec<Role>,
    /// Every tool, or only those the role asked for names.
    pub tools: Vec<Tool>,
}

/// `library.toml`.
#[derive(Debug, Deserialize)]
pub struct LibraryToml {
    pub name: String,
    pub description: Option<String>,
    /// The least rolesmith version the library needs.
    pub rolesmith: Option<String>,
}

/// `roles/<role>/`.
#[derive(Debug)]
pub struct Role {
    pub name: String,
    pub toml: RoleToml,
    /// `core.md`.
    pub core: String,
    /// `prohibitions.toml`; empty when the file is absent.
    pub prohibitions: Vec<Prohibition>,
    pub skills: Vec<Skill>,
}

/// `role.toml` (§3.1).
#[derive(Debug)]
pub struct RoleToml {
    pub description: String,
    pub programs: Vec<String>,
    pub services: Vec<String>,
    pub r#match: Option<Match>,
    /// The consumer tables present (one of [`CONSUMER_TABLES`] each), carried untouched.
    pub consumers: BTreeMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct RawRoleToml {
    description: String,
    #[serde(default)]
    programs: Vec<String>,
    #[serde(default)]
    services: Vec<String>,
    #[serde(default, rename = "match")]
    r#match: Option<Match>,
    #[serde(flatten)]
    rest: toml::Table,
}

/// `[match]` of `role.toml` (§5.5).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub contains: Vec<Contains>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contains {
    pub file: String,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProhibitionsToml {
    #[serde(default)]
    prohibition: Vec<Prohibition>,
}

/// One `[[prohibition]]` (§3.3).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prohibition {
    pub rule: String,
    pub why: String,
}

/// `skills/<skill>/` of a role (§3.4).
#[derive(Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// The skill's directory, whose files are copied with it.
    pub dir: PathBuf,
}

/// `tools/<tool>/`.
#[derive(Debug)]
pub struct Tool {
    pub name: String,
    pub toml: ToolToml,
}

/// `tool.toml` (§3.5).
#[derive(Debug, Deserialize)]
pub struct ToolToml {
    pub description: String,
    pub check: String,
    pub install: Install,
    pub credential: Option<Credential>,
}

/// `[install]`: the method and the pinned version. A method besides `brew` comes with an ADR.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub brew: Option<String>,
    pub version: Option<String>,
}

/// `[credential]`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub mytokens: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Read the library at `root`, or only its role `only` (with `library.toml` and the tools the
/// role names), and check it.
pub fn load(root: &Path, only: Option<&str>) -> Result<Library, Vec<Problem>> {
    let mut c = Checker {
        root,
        problems: Vec::new(),
        programs: BTreeSet::new(),
    };
    let toml = c.library_toml();
    let roles: Vec<Role> = c
        .role_names(only)
        .iter()
        .filter_map(|name| c.role(name))
        .collect();

    let named = std::mem::take(&mut c.programs);
    let mut tool_names = named.clone();
    if only.is_none() {
        tool_names.extend(subdirectories(&root.join("tools")));
    }
    let tools = tool_names
        .iter()
        .filter_map(|name| c.tool(name, named.contains(name)))
        .collect();

    let scope: Vec<PathBuf> = match only {
        None => vec![root.to_path_buf()],
        Some(role) => std::iter::once(root.join("library.toml"))
            .chain(std::iter::once(root.join("roles").join(role)))
            .chain(named.iter().map(|t| root.join("tools").join(t)))
            .collect(),
    };
    for path in scope {
        c.secrets(&path);
    }

    match (toml, c.problems.is_empty()) {
        (Some(toml), true) => Ok(Library {
            root: root.to_path_buf(),
            toml,
            roles,
            tools,
        }),
        _ => Err(c.problems),
    }
}

/// Whether `name` is a valid role or skill name (§2): lowercase ASCII letters, digits and single
/// hyphens, neither first nor last, 2 to [`MAX_NAME`] characters.
pub fn valid_name(name: &str) -> bool {
    (2..=MAX_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

struct Checker<'a> {
    root: &'a Path,
    problems: Vec<Problem>,
    /// Every program a checked role names.
    programs: BTreeSet<String>,
}

impl Checker<'_> {
    fn problem(
        &mut self,
        file: impl Into<String>,
        line: Option<usize>,
        rule: Rule,
        reason: impl Into<String>,
    ) {
        self.problems.push(Problem::new(file, line, rule, reason));
    }

    /// The text of the file `rel`; a problem under `rule` if it is missing or not UTF-8.
    fn read(&mut self, rel: &str, rule: Rule) -> Option<String> {
        match std::fs::read(self.root.join(rel)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.problem(rel, None, rule, "missing");
                None
            }
            Err(e) => {
                self.problem(rel, None, rule, format!("cannot be read: {e}"));
                None
            }
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Some(text),
                Err(_) => {
                    self.problem(rel, None, rule, "is not UTF-8 text");
                    None
                }
            },
        }
    }

    /// `text` of the file `rel` as TOML; a problem under `rule`, with its line, if it does not parse.
    fn parse<T: DeserializeOwned>(&mut self, rel: &str, text: &str, rule: Rule) -> Option<T> {
        match toml::from_str(text) {
            Ok(v) => Some(v),
            Err(e) => {
                let line = e.span().map(|s| line_of(text, s.start));
                let reason = e.message().trim().replace('\n', "; ");
                self.problem(rel, line, rule, format!("does not parse: {reason}"));
                None
            }
        }
    }

    fn library_toml(&mut self) -> Option<LibraryToml> {
        let text = self.read("library.toml", Rule::LibraryToml)?;
        self.parse("library.toml", &text, Rule::LibraryToml)
    }

    fn role_names(&mut self, only: Option<&str>) -> Vec<String> {
        match only {
            None => subdirectories(&self.root.join("roles")),
            Some(role) if self.root.join("roles").join(role).is_dir() => vec![role.to_owned()],
            Some(role) => {
                self.problem(
                    format!("roles/{role}"),
                    None,
                    Rule::RoleMissing,
                    "no such role in this library",
                );
                Vec::new()
            }
        }
    }

    fn role(&mut self, name: &str) -> Option<Role> {
        let dir = format!("roles/{name}");
        if !valid_name(name) {
            self.problem(&dir, None, Rule::RoleName, name_reason("role"));
        }
        let toml = self.role_toml(&format!("{dir}/role.toml"));

        let core_file = format!("{dir}/core.md");
        let core = self.read(&core_file, Rule::Core);
        if let Some(core) = &core {
            let n = core.chars().count();
            if n > MAX_CORE {
                self.problem(
                    &core_file,
                    None,
                    Rule::Core,
                    format!("{n} characters, more than {MAX_CORE}"),
                );
            }
        }

        let prohibitions_file = format!("{dir}/prohibitions.toml");
        let prohibitions = if self.root.join(&prohibitions_file).exists() {
            let text = self.read(&prohibitions_file, Rule::Prohibitions);
            text.and_then(|t| {
                self.parse::<ProhibitionsToml>(&prohibitions_file, &t, Rule::Prohibitions)
            })
            .map(|p| p.prohibition)
        } else {
            Some(Vec::new())
        };

        let skills: Vec<Option<Skill>> = subdirectories(&self.root.join(&dir).join("skills"))
            .iter()
            .map(|skill| self.skill(&format!("{dir}/skills/{skill}"), skill))
            .collect();

        Some(Role {
            name: name.to_owned(),
            toml: toml?,
            core: core?,
            prohibitions: prohibitions?,
            skills: skills.into_iter().collect::<Option<_>>()?,
        })
    }

    fn role_toml(&mut self, file: &str) -> Option<RoleToml> {
        let text = self.read(file, Rule::RoleToml)?;
        let raw: RawRoleToml = self.parse(file, &text, Rule::RoleToml)?;
        for program in &raw.programs {
            self.programs.insert(program.clone());
            if !self
                .root
                .join("tools")
                .join(program)
                .join("tool.toml")
                .is_file()
            {
                self.problem(
                    file,
                    None,
                    Rule::ToolMissing,
                    format!("program {program} has no tools/{program}/tool.toml"),
                );
            }
        }

        let n = raw.description.chars().count();
        if raw.description.trim().is_empty() {
            self.problem(file, None, Rule::RoleDescription, "description is empty");
        } else if raw.description.contains('\n') {
            self.problem(
                file,
                None,
                Rule::RoleDescription,
                "description is more than one line",
            );
        }
        if n > MAX_ROLE_DESCRIPTION {
            self.problem(
                file,
                None,
                Rule::RoleDescription,
                format!("description is {n} characters, more than {MAX_ROLE_DESCRIPTION}"),
            );
        }

        let mut consumers = BTreeMap::new();
        for (key, value) in raw.rest {
            if CONSUMER_TABLES.contains(&key.as_str()) {
                consumers.insert(key, value);
            } else {
                let line = text
                    .lines()
                    .position(|l| {
                        let l = l.trim_start();
                        l.starts_with(&format!("[{key}]"))
                            || l.starts_with(&format!("[{key}."))
                            || l.split(['=', '.']).next().map(str::trim) == Some(key.as_str())
                    })
                    .map(|i| i + 1);
                self.problem(
                    file,
                    line,
                    Rule::RoleUnknownKey,
                    format!(
                        "unknown top-level key `{key}` (consumer tables: {})",
                        CONSUMER_TABLES.join(", ")
                    ),
                );
            }
        }
        Some(RoleToml {
            description: raw.description,
            programs: raw.programs,
            services: raw.services,
            r#match: raw.r#match,
            consumers,
        })
    }

    fn skill(&mut self, dir: &str, name: &str) -> Option<Skill> {
        if !valid_name(name) {
            self.problem(dir, None, Rule::SkillName, name_reason("skill"));
        }
        let file = format!("{dir}/SKILL.md");
        let text = self.read(&file, Rule::SkillFrontMatter)?;
        let Some((yaml, first_line)) = front_matter(&text) else {
            self.problem(
                &file,
                Some(1),
                Rule::SkillFrontMatter,
                "does not begin with YAML front matter between `---` lines",
            );
            return None;
        };
        let doc = match YamlLoader::load_from_str(yaml) {
            Ok(docs) => docs.into_iter().next(),
            Err(e) => {
                self.problem(
                    &file,
                    Some(first_line + e.marker().line().saturating_sub(1)),
                    Rule::SkillFrontMatter,
                    format!("front matter does not parse: {}", e.info()),
                );
                return None;
            }
        };
        let field = |key: &str| {
            doc.as_ref()
                .and_then(|d| d[key].as_str())
                .map(str::to_owned)
        };

        let front_name = field("name");
        match &front_name {
            None => self.problem(
                &file,
                None,
                Rule::SkillFrontMatter,
                "front matter has no `name`",
            ),
            Some(n) if n != name => self.problem(
                &file,
                None,
                Rule::SkillFrontMatter,
                format!("front matter name `{n}` is not its directory `{name}`"),
            ),
            Some(_) => {}
        }
        let description = field("description").filter(|d| !d.trim().is_empty());
        match &description {
            None => self.problem(
                &file,
                None,
                Rule::SkillDescription,
                "front matter has no `description`",
            ),
            Some(d) => {
                let n = d.chars().count();
                if n > MAX_SKILL_DESCRIPTION {
                    self.problem(
                        &file,
                        None,
                        Rule::SkillDescription,
                        format!("description is {n} characters, more than {MAX_SKILL_DESCRIPTION}"),
                    );
                }
            }
        }
        front_name?;
        Some(Skill {
            name: name.to_owned(),
            description: description?,
            dir: self.root.join(dir),
        })
    }

    /// The tool `name`. A tool a role names and that has no `tool.toml` is reported as missing
    /// by the role; any other tool directory without one is reported here.
    fn tool(&mut self, name: &str, named: bool) -> Option<Tool> {
        let file = format!("tools/{name}/tool.toml");
        if named && !self.root.join(&file).is_file() {
            return None;
        }
        let text = self.read(&file, Rule::ToolToml)?;
        let toml: ToolToml = self.parse(&file, &text, Rule::ToolToml)?;
        match (&toml.install.brew, &toml.install.version) {
            (None, _) => self.problem(
                &file,
                None,
                Rule::ToolUnpinned,
                "[install] names no install method (brew)",
            ),
            (Some(_), None) => self.problem(
                &file,
                None,
                Rule::ToolUnpinned,
                "[install] is not pinned: it has no `version`",
            ),
            (Some(_), Some(v)) if v.trim().is_empty() => self.problem(
                &file,
                None,
                Rule::ToolUnpinned,
                "[install] is not pinned: its `version` is empty",
            ),
            _ => {}
        }
        Some(Tool {
            name: name.to_owned(),
            toml,
        })
    }

    /// Every line of every file under `path` (or `path` itself) that matches a secret pattern.
    fn secrets(&mut self, path: &Path) {
        for file in files_under(path) {
            let Ok(bytes) = std::fs::read(&file) else {
                continue;
            };
            let rel = relative(self.root, &file);
            for (line, pattern) in secrets::scan(&String::from_utf8_lossy(&bytes)) {
                self.problem(
                    &rel,
                    Some(line),
                    Rule::Secret,
                    format!("matches the secret pattern: {pattern}"),
                );
            }
        }
    }
}

fn name_reason(what: &str) -> String {
    format!(
        "not a valid {what} name: lowercase letters, digits and single hyphens, 2-{MAX_NAME} characters"
    )
}

/// The YAML between a first line `---` and the next `---`, and the file line it starts on.
fn front_matter(text: &str) -> Option<(&str, usize)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..offset], 2));
        }
        offset += line.len();
    }
    None
}

/// The 1-based line of byte `offset` in `text`.
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// The names of the visible subdirectories of `dir`, sorted; none if it does not exist.
fn subdirectories(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// Every file under `path` (or `path` itself if it is a file), sorted, leaving out `.git`.
fn files_under(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n != ".git"))
        .collect();
    paths.sort();
    paths.iter().flat_map(|p| files_under(p)).collect()
}

/// `path` relative to `root`, `/`-separated.
fn relative(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_names_follow_the_rule() {
        for ok in ["ab", "app-store-releaser", "rust-cli-2", &"a".repeat(48)] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in [
            "a",
            "-ab",
            "ab-",
            "a--b",
            "App",
            "a_b",
            "a b",
            &"a".repeat(49),
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn front_matter_is_found_between_dashes() {
        assert_eq!(
            front_matter("---\nname: x\n---\nbody"),
            Some(("name: x\n", 2))
        );
        assert_eq!(front_matter("no front matter"), None);
        assert_eq!(front_matter("---\nname: x\n"), None);
    }

    #[test]
    fn line_of_counts_from_one() {
        assert_eq!(line_of("a\nb\nc", 0), 1);
        assert_eq!(line_of("a\nb\nc", 2), 2);
        assert_eq!(line_of("a\nb\nc", 4), 3);
    }
}
