//! What roles need of a machine (SPEC.md §3.5, §7.1): the tool of each program they name and the
//! mytokens entry of each service and credential they read, each once, with the roles that need it.
//! `doctor` checks it and `tool install` installs it, for one role or for every role a project
//! records.

use std::path::{Path, PathBuf};

use crate::commands::Failure;
use crate::library::{Library, Role, ToolToml};
use crate::project::{self, Place, RECORD};
use crate::sources::{self, Found};

/// What a set of roles needs, in the order first reached.
#[derive(Default)]
pub struct Needs {
    pub programs: Vec<Program>,
    pub entries: Vec<Entry>,
}

/// A program's tool. Two roles name the same one when it is the same tool of the same source with
/// the same tool.toml; a tool pinned differently at another rev is another one.
pub struct Program {
    pub name: String,
    pub source: String,
    pub toml: ToolToml,
    /// The roles that name it, as `<source>/<role>`.
    pub from: Vec<String>,
}

/// A mytokens entry, with the fields its credentials fill variables from.
pub struct Entry {
    pub name: String,
    pub fields: Vec<String>,
    /// The roles that read it, as `<source>/<role>`.
    pub from: Vec<String>,
}

impl Needs {
    /// Add what `role` of `lib`, read from `source`, needs: its services, then each program's tool
    /// and that tool's credential.
    pub fn add(&mut self, source: &str, lib: &Library, role: &Role) {
        let from = format!("{source}/{}", role.name);
        for service in &role.toml.services {
            self.entry(service, &[], &from);
        }
        for name in &role.toml.programs {
            let tool = lib
                .tools
                .iter()
                .find(|t| &t.name == name)
                .expect("a checked role has a tool.toml for each program");
            match self
                .programs
                .iter_mut()
                .find(|p| p.source == source && &p.name == name && p.toml == tool.toml)
            {
                Some(p) => push_once(&mut p.from, &from),
                None => self.programs.push(Program {
                    name: name.clone(),
                    source: source.to_owned(),
                    toml: tool.toml.clone(),
                    from: vec![from.clone()],
                }),
            }
            if let Some(cred) = &tool.toml.credential {
                let fields: Vec<String> = cred
                    .env
                    .values()
                    .filter(|f| !f.is_empty())
                    .cloned()
                    .collect();
                self.entry(&cred.mytokens, &fields, &from);
            }
        }
    }

    fn entry(&mut self, name: &str, fields: &[String], from: &str) {
        let at = match self.entries.iter().position(|e| e.name == name) {
            Some(at) => at,
            None => {
                self.entries.push(Entry {
                    name: name.to_owned(),
                    fields: Vec::new(),
                    from: Vec::new(),
                });
                self.entries.len() - 1
            }
        };
        let entry = &mut self.entries[at];
        push_once(&mut entry.from, from);
        for field in fields {
            push_once(&mut entry.fields, field);
        }
    }
}

fn push_once(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|x| x == item) {
        list.push(item.to_owned());
    }
}

/// What the project at `dir` needs: every role its `rolesmith.toml` records, at its recorded rev,
/// and every role each requires, at that same rev. Answers the project's root and each role read,
/// each once, in the order first reached. A directory without `rolesmith.toml` is refused.
pub fn of_project(dir: Option<PathBuf>) -> Result<(PathBuf, Vec<Found>, Needs), Failure> {
    let dir = crate::commands::sync::project_dir(dir)?;
    if !Path::new(&dir).join(RECORD).is_file() {
        return Err(Failure::new(format!(
            "{} has no {RECORD}: it records no roles; `rolesmith use <role> --into <target> --project {}` records one",
            dir.display(),
            dir.display()
        )));
    }
    let place = Place::project(dir.clone());
    let record = project::Record::load(&place)?;
    let mut read: Vec<Found> = Vec::new();
    let mut needs = Needs::default();
    for entry in &record.role {
        let found = project::locate(&place, entry)?;
        let (lib, _tree) = sources::read_role(&found)?;
        // Loaded for one role, the library holds it first and then every role it requires.
        for role in &lib.roles {
            if read
                .iter()
                .any(|f| f.source == found.source && f.name == role.name && f.rev == found.rev)
            {
                continue;
            }
            needs.add(&found.source, &lib, role);
            read.push(Found {
                source: found.source.clone(),
                name: role.name.clone(),
                rev: found.rev.clone(),
            });
        }
    }
    Ok((dir, read, needs))
}
