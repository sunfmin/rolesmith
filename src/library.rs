//! A library working copy, read and validated.
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
use crate::service_map;
use crate::upstream;

/// Tables that belong to another consumer: top-level in `role.toml` and sub-tables of a
/// `[[prohibition]]`. rolesmith keeps them untouched and ignores them. The one place they
/// are listed.
pub const CONSUMER_TABLES: &[&str] = &["guild"];

/// At most this many characters in a role's `description`.
pub const MAX_ROLE_DESCRIPTION: usize = 200;
/// At most this many characters in `core.md`.
pub const MAX_CORE: usize = 8_000;
/// At most this many characters in a skill's `description`.
pub const MAX_SKILL_DESCRIPTION: usize = 1_024;
/// A role or skill name is 2 to this many characters.
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

/// `role.toml`.
#[derive(Debug)]
pub struct RoleToml {
    pub description: String,
    pub programs: Vec<String>,
    pub services: Vec<String>,
    /// Roles of the same library this one builds on, installed with it.
    pub requires: Vec<String>,
    /// `[[skill]]`: skills of other repositories, pinned by commit.
    pub external: Vec<External>,
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
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    skill: Vec<External>,
    #[serde(default, rename = "match")]
    r#match: Option<Match>,
    #[serde(flatten)]
    rest: toml::Table,
}

/// One `[[skill]]` of `role.toml`: a skill of another repository, at a pinned commit.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct External {
    pub name: String,
    pub url: String,
    /// The skill's directory in that repository.
    pub path: String,
    /// The commit, a full SHA.
    pub rev: String,
}

/// `[match]` of `role.toml`.
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

/// One `[[prohibition]]`.
#[derive(Debug)]
pub struct Prohibition {
    pub rule: String,
    pub why: String,
    /// The consumer sub-tables present (one of [`CONSUMER_TABLES`] each), carried untouched.
    pub consumers: BTreeMap<String, toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProhibitionsToml {
    #[serde(default)]
    prohibition: Vec<RawProhibition>,
}

#[derive(Deserialize)]
struct RawProhibition {
    rule: String,
    why: String,
    #[serde(flatten)]
    rest: toml::Table,
}

/// `skills/<skill>/` of a role.
#[derive(Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// The skill's directory, whose files are copied with it: in the library, or for an external
    /// skill in rolesmith's cache.
    pub dir: PathBuf,
    /// Where an external skill comes from; `None` for a skill of the role's own.
    pub external: Option<External>,
}

/// `tools/<tool>/`.
#[derive(Debug)]
pub struct Tool {
    pub name: String,
    pub toml: ToolToml,
}

/// `tool.toml`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ToolToml {
    pub description: String,
    pub check: String,
    pub install: Install,
    pub credential: Option<Credential>,
}

/// `[install]`: the one method (`brew` or `script`, ADR 0005) and the pinned version.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub brew: Option<String>,
    pub script: Option<String>,
    pub version: Option<String>,
}

/// The `version` of a brew tool that follows its formula's stable version (ADR 0005, amended).
pub const STABLE: &str = "stable";

/// How a tool is installed, as its `[install]` pins it.
pub enum Method<'a> {
    /// A Homebrew formula.
    Brew(&'a str),
    /// A shell script rolesmith runs with `sh -c`, as the user.
    Script(&'a str),
}

impl Install {
    /// The method and the pinned version, or the rule it breaks and why: exactly one method, a
    /// version that is not blank (`stable` only for brew), and a script that never runs `sudo`.
    pub fn pinned(&self) -> Result<(Method<'_>, &str), (Rule, &'static str)> {
        let unpinned = |reason| Err((Rule::ToolUnpinned, reason));
        let method = match (&self.brew, &self.script) {
            (Some(formula), None) => Method::Brew(formula),
            (None, Some(script)) => Method::Script(script),
            (None, None) => return unpinned("[install] names no install method (brew or script)"),
            (Some(_), Some(_)) => {
                return unpinned(
                    "[install] names two install methods, brew and script; it takes one",
                );
            }
        };
        let version = match self.version.as_deref() {
            None => return unpinned("[install] is not pinned: it has no `version`"),
            Some(v) if v.trim().is_empty() => {
                return unpinned("[install] is not pinned: its `version` is empty");
            }
            Some(v) => v,
        };
        if let (Method::Script(_), STABLE) = (&method, version) {
            return unpinned(
                "[install] script follows `stable`; a script pins its version, since rolesmith cannot see what it installed",
            );
        }
        if let Method::Script(script) = method
            && runs_sudo(script)
        {
            return Err((
                Rule::ToolSudo,
                "[install] script runs `sudo`; rolesmith installs as the user, never as root",
            ));
        }
        Ok((method, version))
    }
}

/// Whether `script` names `sudo` as a word (`sudo`, `/usr/bin/sudo`), not inside one (`pseudo`).
fn runs_sudo(script: &str) -> bool {
    script
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'))
        .any(|word| word == "sudo")
}

/// `[credential]`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    /// The optokens Secret it is read from (the key's name is kept from when credentials were read
    /// from mytokens).
    pub mytokens: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// The library working copy that holds `start`: the nearest of `start` and its parents with a
/// `library.toml`, the way git finds its repository. Outside any library, `start` itself, so that
/// checking it refuses as a directory with no `library.toml`. Either is absolute where `start`
/// exists, so a run answers the same from anywhere inside the library.
pub fn find_root(start: &Path) -> PathBuf {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    start
        .ancestors()
        .find(|dir| dir.join("library.toml").is_file())
        .unwrap_or(&start)
        .to_path_buf()
}

/// Read the library at `root`, or only its role `only` (with `library.toml` and the tools the
/// role names), and check it.
pub fn load(root: &Path, only: Option<&str>) -> Result<Library, Vec<Problem>> {
    let mut c = Checker {
        root,
        problems: Vec::new(),
        programs: BTreeSet::new(),
        requires: BTreeMap::new(),
    };
    let toml = c.library_toml();
    // One role asked for brings every role it requires, transitively, in the order first reached.
    let mut names = c.role_names(only);
    let mut roles = Vec::new();
    let mut next = 0;
    while let Some(name) = names.get(next).cloned() {
        next += 1;
        roles.extend(c.role(&name));
        if only.is_some() {
            let required = c.requires.get(&name).map(|r| r.names.clone());
            for req in required.unwrap_or_default() {
                if !names.contains(&req) && is_role(root, &req) {
                    names.push(req);
                }
            }
        }
    }
    c.check_requires();

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
        Some(_) => std::iter::once(root.join("library.toml"))
            .chain(names.iter().map(|r| root.join("roles").join(r)))
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

impl Library {
    /// The roles `name` requires, transitively, each once, in the order first reached; not `name`
    /// itself. Every one is among [`Library::roles`] when the library was loaded for `name`.
    pub fn requires(&self, name: &str) -> Vec<&Role> {
        let mut found: Vec<&Role> = Vec::new();
        let mut pending = vec![name];
        let mut next = 0;
        while let Some(at) = pending.get(next).copied() {
            next += 1;
            let role = self.roles.iter().find(|r| r.name == at);
            for req in role.into_iter().flat_map(|r| &r.toml.requires) {
                if req != name && !pending.contains(&req.as_str()) {
                    pending.push(req);
                    found.extend(self.roles.iter().find(|r| &r.name == req));
                }
            }
        }
        found
    }
}

/// Whether `name` is a valid role or skill name: lowercase ASCII letters, digits and single
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

/// Whether `name` is a role of the library at `root`.
fn is_role(root: &Path, name: &str) -> bool {
    valid_name(name) && root.join("roles").join(name).is_dir()
}

struct Checker<'a> {
    root: &'a Path,
    problems: Vec<Problem>,
    /// Every program a checked role names.
    programs: BTreeSet<String>,
    /// What each checked role's `role.toml` requires.
    requires: BTreeMap<String, Requires>,
}

/// A role's `requires`, and the line of `role.toml` it is on.
struct Requires {
    names: Vec<String>,
    line: Option<usize>,
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
        if self.root.join(service_map::FILE).exists() {
            self.problem(
                service_map::FILE,
                None,
                Rule::HostOnly,
                "maps services to where a host reads them; that is each host's own setting, kept beside its sources.toml, never in a library",
            );
        }
        self.host_only("library.toml", &text);
        self.parse("library.toml", &text, Rule::LibraryToml)
    }

    /// A problem when `text` of the file `rel` has the host's own `[service]` table.
    fn host_only(&mut self, rel: &str, text: &str) {
        let Ok(table) = text.parse::<toml::Table>() else {
            return;
        };
        if table.contains_key(service_map::TABLE) {
            self.problem(
                rel,
                key_line(text, service_map::TABLE),
                Rule::HostOnly,
                format!(
                    "`[{}]` maps a service to the command a host reads it with; that is each host's own setting (its {}), never a library's",
                    service_map::TABLE,
                    service_map::FILE
                ),
            );
        }
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
        let toml = self.role_toml(name, &format!("{dir}/role.toml"));

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
            self.prohibitions(&prohibitions_file)
        } else {
            Some(Vec::new())
        };

        let own = subdirectories(&self.root.join(&dir).join("skills"));
        let mut skills: Vec<Option<Skill>> = own
            .iter()
            .map(|skill| self.skill(&format!("{dir}/skills/{skill}"), skill))
            .collect();
        if let Some((toml, text)) = &toml {
            let file = format!("{dir}/role.toml");
            let mut seen: BTreeSet<String> = own.into_iter().collect();
            for (i, ext) in toml.external.iter().enumerate() {
                let line = nth_table_line(text, "skill", i, "name");
                let clash = !seen.insert(ext.name.clone());
                skills.push(self.external(&file, line, ext, clash));
            }
        }
        let toml = toml.map(|(toml, _)| toml);

        Some(Role {
            name: name.to_owned(),
            toml: toml?,
            core: core?,
            prohibitions: prohibitions?,
            skills: skills.into_iter().collect::<Option<_>>()?,
        })
    }

    fn prohibitions(&mut self, file: &str) -> Option<Vec<Prohibition>> {
        let text = self.read(file, Rule::Prohibitions)?;
        let raw: RawProhibitionsToml = self.parse(file, &text, Rule::Prohibitions)?;
        let mut ok = true;
        let mut prohibitions = Vec::new();
        for (i, p) in raw.prohibition.into_iter().enumerate() {
            let mut consumers = BTreeMap::new();
            for (key, value) in p.rest {
                if CONSUMER_TABLES.contains(&key.as_str()) && value.is_table() {
                    consumers.insert(key, value);
                    continue;
                }
                ok = false;
                let line = nth_table_line(&text, "prohibition", i, &key);
                self.problem(
                    file,
                    line,
                    Rule::Prohibitions,
                    format!(
                        "prohibition {}: unknown key `{key}` (consumer sub-tables: {})",
                        i + 1,
                        CONSUMER_TABLES.join(", ")
                    ),
                );
            }
            prohibitions.push(Prohibition {
                rule: p.rule,
                why: p.why,
                consumers,
            });
        }
        ok.then_some(prohibitions)
    }

    /// No role requires itself, directly or through others.
    fn check_requires(&mut self) {
        let mut done = BTreeSet::new();
        let starts: Vec<String> = self.requires.keys().cloned().collect();
        for start in starts {
            self.visit(&start, &mut Vec::new(), &mut done);
        }
    }

    /// Walk what `role` requires depth first, reporting a cycle back onto `path`.
    fn visit(&mut self, role: &str, path: &mut Vec<String>, done: &mut BTreeSet<String>) {
        if done.contains(role) {
            return;
        }
        if let Some(i) = path.iter().position(|r| r == role) {
            let mut cycle = path[i..].to_vec();
            cycle.push(role.to_owned());
            let line = self.requires.get(role).and_then(|r| r.line);
            self.problem(
                format!("roles/{role}/role.toml"),
                line,
                Rule::RoleRequires,
                format!("requires form a cycle: {}", cycle.join(" -> ")),
            );
            return;
        }
        let Some(next) = self.requires.get(role).map(|r| r.names.clone()) else {
            done.insert(role.to_owned());
            return;
        };
        path.push(role.to_owned());
        for req in next {
            self.visit(&req, path, done);
        }
        path.pop();
        done.insert(role.to_owned());
    }

    /// The external skill `ext` of the role whose `role.toml` is `file` (its `[[skill]]` at
    /// `line`), fetched into the cache when it lacks the rev; `clash` when the role already has a
    /// skill of that name.
    fn external(
        &mut self,
        file: &str,
        line: Option<usize>,
        ext: &External,
        clash: bool,
    ) -> Option<Skill> {
        let before = self.problems.len();
        let name = &ext.name;
        let refuse = |c: &mut Self, rule: Rule, reason: String| {
            c.problem(file, line, rule, format!("skill `{name}`: {reason}"));
        };
        if !valid_name(name) {
            refuse(self, Rule::SkillName, name_reason("skill"));
        }
        if clash {
            let reason = "the role already has a skill of this name";
            refuse(self, Rule::SkillName, reason.to_owned());
        }
        if !upstream::is_full_sha(&ext.rev) {
            let reason = format!(
                "rev `{}` is not a full commit SHA (40 lowercase hex digits)",
                ext.rev
            );
            refuse(self, Rule::ExternalSkill, reason);
        }
        let inside = !ext.path.is_empty()
            && Path::new(&ext.path)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)));
        if !inside {
            let reason = format!(
                "path `{}` is not a relative path inside the repository",
                ext.path
            );
            refuse(self, Rule::ExternalSkill, reason);
        }
        if self.problems.len() > before {
            return None;
        }
        let at = format!("{} at {}", ext.url, ext.rev);
        let skill_md = format!("{}/SKILL.md", ext.path);
        let dir = match upstream::tree(&ext.url, &ext.rev) {
            Ok(tree) => tree.join(&ext.path),
            Err(e) => {
                refuse(self, Rule::ExternalSkill, e.message);
                return None;
            }
        };
        let Ok(text) = std::fs::read_to_string(dir.join("SKILL.md")) else {
            refuse(self, Rule::ExternalSkill, format!("{at} has no {skill_md}"));
            return None;
        };
        let description = check_skill_md(&text, None, &mut |_, rule, reason| {
            refuse(self, rule, format!("{at}: {skill_md}: {reason}"))
        });
        Some(Skill {
            name: name.clone(),
            description: description?,
            dir,
            external: Some(ext.clone()),
        })
    }

    fn role_toml(&mut self, name: &str, file: &str) -> Option<(RoleToml, String)> {
        let text = self.read(file, Rule::RoleToml)?;
        self.host_only(file, &text);
        let raw: RawRoleToml = self.parse(file, &text, Rule::RoleToml)?;
        let root = self.root;
        let line = key_line(&text, "requires");
        for req in raw.requires.iter().filter(|r| !is_role(root, r)) {
            self.problem(
                file,
                line,
                Rule::RoleRequires,
                format!("requires `{req}`, which is not a role of this library"),
            );
        }
        self.requires.insert(
            name.to_owned(),
            Requires {
                names: raw.requires.clone(),
                line,
            },
        );
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
            } else if key != service_map::TABLE {
                let line = key_line(&text, &key);
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
        let toml = RoleToml {
            description: raw.description,
            programs: raw.programs,
            services: raw.services,
            requires: raw.requires,
            external: raw.skill,
            r#match: raw.r#match,
            consumers,
        };
        Some((toml, text))
    }

    fn skill(&mut self, dir: &str, name: &str) -> Option<Skill> {
        if !valid_name(name) {
            self.problem(dir, None, Rule::SkillName, name_reason("skill"));
        }
        let file = format!("{dir}/SKILL.md");
        let text = self.read(&file, Rule::SkillFrontMatter)?;
        let description = check_skill_md(&text, Some(name), &mut |line, rule, reason| {
            self.problem(&file, line, rule, reason)
        });
        Some(Skill {
            name: name.to_owned(),
            description: description?,
            dir: self.root.join(dir),
            external: None,
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
        self.host_only(&file, &text);
        let toml: ToolToml = self.parse(&file, &text, Rule::ToolToml)?;
        if let Err((rule, reason)) = toml.install.pinned() {
            self.problem(&file, None, rule, reason);
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

/// The description of a `SKILL.md` of `text`; each thing wrong with its front matter goes
/// to `report`, with its line where there is one. `dir` is the
/// directory name its `name` must equal, if any.
fn check_skill_md(
    text: &str,
    dir: Option<&str>,
    report: &mut dyn FnMut(Option<usize>, Rule, String),
) -> Option<String> {
    let Some((yaml, first_line)) = front_matter(text) else {
        let reason = "does not begin with YAML front matter between `---` lines";
        report(Some(1), Rule::SkillFrontMatter, reason.to_owned());
        return None;
    };
    let doc = match YamlLoader::load_from_str(yaml) {
        Ok(docs) => docs.into_iter().next(),
        Err(e) => {
            let line = first_line + e.marker().line().saturating_sub(1);
            let reason = format!("front matter does not parse: {}", e.info());
            report(Some(line), Rule::SkillFrontMatter, reason);
            return None;
        }
    };
    let field = |key: &str| {
        doc.as_ref()
            .and_then(|d| d[key].as_str())
            .map(str::to_owned)
    };
    match (field("name"), dir) {
        (None, _) => report(
            None,
            Rule::SkillFrontMatter,
            "front matter has no `name`".to_owned(),
        ),
        (Some(n), Some(dir)) if n != dir => report(
            None,
            Rule::SkillFrontMatter,
            format!("front matter name `{n}` is not its directory `{dir}`"),
        ),
        _ => {}
    }
    let description = field("description").filter(|d| !d.trim().is_empty());
    match &description {
        None => report(
            None,
            Rule::SkillDescription,
            "front matter has no `description`".to_owned(),
        ),
        Some(d) => {
            let n = d.chars().count();
            if n > MAX_SKILL_DESCRIPTION {
                report(
                    None,
                    Rule::SkillDescription,
                    format!("description is {n} characters, more than {MAX_SKILL_DESCRIPTION}"),
                );
            }
        }
    }
    description
}

fn name_reason(what: &str) -> String {
    format!(
        "not a valid {what} name: lowercase letters, digits and single hyphens, 2-{MAX_NAME} characters"
    )
}

/// The YAML between a first line `---` and the next `---`, and the file line it starts on.
fn front_matter(text: &str) -> Option<(&str, usize)> {
    split_front_matter(text).map(|(yaml, _)| (yaml, 2))
}

/// A file that begins with YAML front matter, split into that YAML (without its `---` lines) and
/// the text after its closing `---` line.
pub fn split_front_matter(text: &str) -> Option<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// The 1-based line where the top-level `key` of a TOML `text` is set, if it can be found.
fn key_line(text: &str, key: &str) -> Option<usize> {
    text.lines()
        .position(|l| {
            let l = l.trim_start();
            l.starts_with(&format!("[{key}]"))
                || l.starts_with(&format!("[{key}."))
                || l.split(['=', '.']).next().map(str::trim) == Some(key)
        })
        .map(|i| i + 1)
}

/// The line of `key` inside the `n`th (from 0) `[[<table>]]` of `text`, if it can be found.
fn nth_table_line(text: &str, table: &str, n: usize, key: &str) -> Option<usize> {
    let mut seen = None;
    for (i, l) in text.lines().enumerate() {
        let l = l.trim_start();
        if l.starts_with(&format!("[[{table}]]")) {
            seen = Some(seen.map_or(0, |s| s + 1));
        } else if seen == Some(n)
            && (l.starts_with(&format!("[{table}.{key}]"))
                || l.starts_with(&format!("[{table}.{key}."))
                || l.split(['=', '.']).next().map(str::trim) == Some(key))
        {
            return Some(i + 1);
        }
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
    fn a_role_description_must_be_one_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let role = root.join("roles/ab");
        std::fs::create_dir_all(&role).unwrap();
        std::fs::write(root.join("library.toml"), "name = \"felix\"\n").unwrap();
        std::fs::write(role.join("core.md"), "# Core\n").unwrap();
        let with = |description: &str| {
            std::fs::write(
                role.join("role.toml"),
                format!("description = {description}\n"),
            )
            .unwrap();
            load(root, None).err().unwrap_or_default()
        };
        assert!(with("\"one line\"").is_empty());
        let found = with("\"\"\"one\nline\"\"\"");
        assert!(
            found.iter().any(|p| p.file == "roles/ab/role.toml"
                && p.rule == Rule::RoleDescription
                && p.reason == "description is more than one line"),
            "{found:?}"
        );
    }

    #[test]
    fn line_of_counts_from_one() {
        assert_eq!(line_of("a\nb\nc", 0), 1);
        assert_eq!(line_of("a\nb\nc", 2), 2);
        assert_eq!(line_of("a\nb\nc", 4), 3);
    }
}
