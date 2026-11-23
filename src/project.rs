//! Where roles are installed (SPEC.md §4, §5.1, §5.2): a project, with its record `rolesmith.toml`,
//! the files generated from it into each target's skill directory, their manifest
//! `.rolesmith/manifest.json`, and the `.gitignore` lines that ignore them; or the user (`--global`),
//! with its record `global.toml` and manifest beside it in rolesmith's data directory, and the files
//! in each target's user skill directory under `HOME`.
//!
//! [`sync`] is the one way generated files are written: it computes every file the record asks
//! for, refuses before writing anything if one would overwrite a file rolesmith did not generate,
//! deletes only files the old manifest lists, and writes only what differs, so a second run
//! changes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::commands::Failure;
use crate::library;
use crate::sources::{self, Found, Sources};

/// The project's record, at its root, committed.
pub const RECORD: &str = "rolesmith.toml";
/// The list of every file generated into the project.
pub const MANIFEST: &str = ".rolesmith/manifest.json";

/// The record of the user's global installs, in rolesmith's data directory.
pub const GLOBAL_RECORD: &str = "global.toml";
/// The manifest of the user's global installs, beside [`GLOBAL_RECORD`].
pub const GLOBAL_MANIFEST: &str = "global.manifest.json";

/// Each target (§5.2) and the directory its agent reads skills from, relative to the project's
/// root and, for a global install, to `HOME`; the one table of them.
/// - claude-code: project skills are `.claude/skills/<name>/SKILL.md`, the user's
///   `~/.claude/skills/<name>/SKILL.md` (https://code.claude.com/docs/en/skills, read 2026-11-22).
/// - codex: Codex scans `.agents/skills` from the working directory up to the repository root, and
///   the user's `$HOME/.agents/skills` (https://learn.chatgpt.com/docs/build-skills, where
///   https://developers.openai.com/codex/skills redirects; read 2026-11-22).
/// - agents: `.agents/skills`, and the user's `~/.agents/skills`: where `npx skills` installs and
///   what Orca's agents read (SPEC.md §5.2).
///
/// Two targets may share a directory (codex and agents do); a role installed into both writes its
/// files there once.
pub const TARGETS: &[(&str, &str)] = &[
    ("claude-code", ".claude/skills"),
    ("codex", ".agents/skills"),
    ("agents", ".agents/skills"),
];

/// Where an install lives: the directory its generated paths are relative to, its record and its
/// manifest.
#[derive(Debug, Clone)]
pub struct Place {
    /// The project's root, or `HOME` for the user's global installs.
    pub root: PathBuf,
    record: PathBuf,
    manifest: PathBuf,
    /// A global install writes nothing into a project: no `.gitignore` lines, no `projects.toml`.
    pub global: bool,
}

impl Place {
    /// The project at `dir`.
    pub fn project(dir: PathBuf) -> Place {
        Place {
            record: dir.join(RECORD),
            manifest: dir.join(MANIFEST),
            root: dir,
            global: false,
        }
    }

    /// The user's global installs.
    pub fn global() -> Result<Place, Failure> {
        let home = match std::env::var_os("HOME") {
            Some(home) if Path::new(&home).is_absolute() => PathBuf::from(home),
            _ => {
                return Err(Failure::new(
                    "HOME is not set; a --global install goes under it",
                ));
            }
        };
        let data = sources::data_dir()?;
        Ok(Place {
            root: home,
            record: data.join(GLOBAL_RECORD),
            manifest: data.join(GLOBAL_MANIFEST),
            global: true,
        })
    }

    /// The record's file name, as messages name it.
    pub fn record_name(&self) -> &'static str {
        if self.global { GLOBAL_RECORD } else { RECORD }
    }

    /// The project's root as output names it; `None` for the global installs.
    pub fn project_name(&self) -> Option<String> {
        (!self.global).then(|| self.root.display().to_string())
    }
}

const GITIGNORE: &str = ".gitignore";
const GITIGNORE_BEGIN: &str =
    "# rolesmith: generated from rolesmith.toml; `rolesmith sync` rewrites these lines";
const GITIGNORE_END: &str = "# rolesmith: end";

/// `rolesmith.toml`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    #[serde(default)]
    pub role: Vec<RoleEntry>,
}

/// One `[[role]]` of `rolesmith.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleEntry {
    pub name: String,
    pub source: String,
    /// The source's URL, so a fresh clone can add the source.
    pub url: String,
    /// The library commit, a full SHA.
    pub rev: String,
    pub into: Vec<String>,
}

/// The skill directory `target` installs into, or a refusal naming the known targets.
pub fn target_dir(target: &str) -> Result<&'static str, Failure> {
    TARGETS
        .iter()
        .find(|(name, _)| *name == target)
        .map(|(_, dir)| *dir)
        .ok_or_else(|| {
            let known: Vec<&str> = TARGETS.iter().map(|(name, _)| *name).collect();
            Failure::new(format!(
                "unknown target {target} (known: {})",
                known.join(", ")
            ))
        })
}

impl Record {
    /// The record at `place`; empty when it has none.
    pub fn load(place: &Place) -> Result<Record, Failure> {
        let file = &place.record;
        let text = match std::fs::read_to_string(file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Record::default()),
            Err(e) => return Err(Failure::new(format!("cannot read {}: {e}", file.display()))),
            Ok(text) => text,
        };
        let record: Record = toml::from_str(&text).map_err(|e| {
            Failure::new(format!(
                "{} does not parse: {}",
                file.display(),
                e.message().trim()
            ))
        })?;
        for entry in &record.role {
            if entry.rev.len() != 40 || !entry.rev.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Failure::new(format!(
                    "{}: role {}'s rev {} is not a full commit SHA",
                    file.display(),
                    entry.name,
                    entry.rev
                )));
            }
            for target in &entry.into {
                target_dir(target)?;
            }
        }
        Ok(record)
    }

    /// Write the record at `place`, unless it already says exactly this; a record of no roles is
    /// no file.
    pub fn save(&self, place: &Place) -> Result<(), Failure> {
        if self.role.is_empty() {
            return remove_if_present(&place.record);
        }
        let text = toml::to_string(self).expect("the record serializes");
        write_if_changed(&place.record, text.as_bytes(), false).map(|_| ())
    }

    /// The index of the role `name` in the record at `place`, or a refusal naming the roles it has.
    pub fn position(&self, place: &Place, name: &str) -> Result<usize, Failure> {
        self.role
            .iter()
            .position(|r| r.name == name)
            .ok_or_else(|| {
                let names: Vec<&str> = self.role.iter().map(|r| r.name.as_str()).collect();
                Failure::new(format!(
                    "{} records no role {name} (it records: {})",
                    place.record_name(),
                    if names.is_empty() {
                        "none".to_owned()
                    } else {
                        names.join(", ")
                    }
                ))
            })
    }
}

/// Save `record` at `place` and sync it; when the sync fails, the record file is put back as it
/// was.
pub fn install(place: &Place, record: &Record) -> Result<Synced, Failure> {
    let file = &place.record;
    let before = std::fs::read(file).ok();
    record.save(place)?;
    sync(place, record).inspect_err(|_| {
        let _ = match &before {
            Some(bytes) => std::fs::write(file, bytes),
            None => std::fs::remove_file(file),
        };
    })
}

/// What one recorded role installs that is not as generated.
pub struct Drift {
    /// The generated files of the role and the roles it requires that differ from what it
    /// generates (a hand edit) or are missing, relative to the place's root.
    pub files: Vec<String>,
    /// The roles it requires, transitively.
    pub requires: Vec<String>,
}

/// The drift of `entry` at `place`.
pub fn drift(place: &Place, entry: &RoleEntry) -> Result<Drift, Failure> {
    let dir = &place.root;
    let mut generated = Generated::new();
    let requires = generate(place, entry, &mut generated)?;
    let files = generated
        .into_iter()
        .filter(|(rel, (bytes, exec))| {
            let path = dir.join(rel);
            !(std::fs::read(&path).is_ok_and(|have| have == *bytes) && executable(&path) == *exec)
        })
        .map(|(rel, _)| rel)
        .collect();
    Ok(Drift { files, requires })
}

/// A role installed only because roles of the record require it (§3.1): from the source and at
/// the rev of the roles that require it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Required {
    pub name: String,
    pub source: String,
    pub rev: String,
    /// The recorded roles that require it, directly or through others.
    pub by: Vec<String>,
}

/// Every file the record at `place` generates, and the roles it installs that it does not name.
fn plan(place: &Place, record: &Record) -> Result<(Generated, Vec<Required>), Failure> {
    let mut generated = Generated::new();
    let mut required: Vec<Required> = Vec::new();
    for entry in &record.role {
        for name in generate(place, entry, &mut generated)? {
            if record.role.iter().any(|r| r.name == name) {
                continue;
            }
            match required.iter_mut().find(|r| r.name == name) {
                Some(r) => r.by.push(entry.name.clone()),
                None => required.push(Required {
                    name,
                    source: entry.source.clone(),
                    rev: entry.rev.clone(),
                    by: vec![entry.name.clone()],
                }),
            }
        }
    }
    Ok((generated, required))
}

/// The index of the role `name` in `record`, or a refusal naming the roles it has and, when `name`
/// is installed only because others require it, those roles.
pub fn named_position(place: &Place, record: &Record, name: &str) -> Result<usize, Failure> {
    record.position(place, name).map_err(|refusal| {
        let by = plan(place, record)
            .ok()
            .and_then(|(_, required)| required.into_iter().find(|r| r.name == name).map(|r| r.by));
        match by {
            Some(by) => Failure::new(format!(
                "{} does not name {name}; it is installed because {} requires it",
                place.record_name(),
                by.join(", ")
            )),
            None => refusal,
        }
    })
}

/// `$XDG_DATA_HOME/rolesmith/projects.toml`: each project directory rolesmith has installed into,
/// so `source remove` can see which record a source.
pub fn projects_file() -> Result<PathBuf, Failure> {
    Ok(sources::data_dir()?.join("projects.toml"))
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Projects {
    #[serde(default)]
    projects: BTreeSet<PathBuf>,
}

/// The project directories rolesmith has installed into.
pub fn known_projects() -> Result<BTreeSet<PathBuf>, Failure> {
    let file = projects_file()?;
    match std::fs::read_to_string(&file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(Failure::new(format!("cannot read {}: {e}", file.display()))),
        Ok(text) => toml::from_str::<Projects>(&text)
            .map(|p| p.projects)
            .map_err(|e| {
                Failure::new(format!(
                    "{} does not parse: {}",
                    file.display(),
                    e.message().trim()
                ))
            }),
    }
}

/// Add the project at `dir` to the projects rolesmith has installed into.
fn remember(dir: &Path) -> Result<(), Failure> {
    let dir = dir
        .canonicalize()
        .map_err(|e| Failure::new(format!("cannot resolve {}: {e}", dir.display())))?;
    let mut projects = known_projects()?;
    if projects.insert(dir) {
        let text = toml::to_string(&Projects { projects }).expect("the projects serialize");
        write_if_changed(&projects_file()?, text.as_bytes(), false)?;
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), Failure> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Failure::new(format!(
            "cannot delete {}: {e}",
            path.display()
        ))),
        _ => Ok(()),
    }
}

/// What [`sync`] changed, as paths relative to the place's root (the manifest of a global install
/// by its file name).
#[derive(Debug, Default)]
pub struct Synced {
    pub written: Vec<String>,
    pub removed: Vec<String>,
    /// The roles installed only because recorded roles require them.
    pub required: Vec<Required>,
}

/// A generated file: its bytes, and whether it is executable.
type Generated = BTreeMap<String, (Vec<u8>, bool)>;

/// Make the files generated at `place` equal what its `record` says, adding a source the machine
/// lacks from the record's `url`.
pub fn sync(place: &Place, record: &Record) -> Result<Synced, Failure> {
    let dir = &place.root;
    let (generated, required) = plan(place, record)?;
    let old: BTreeSet<String> = read_manifest(&place.manifest)?;

    let foreign: Vec<&str> = generated
        .iter()
        .filter(|(rel, (bytes, _))| {
            !old.contains(*rel) && std::fs::read(dir.join(rel)).is_ok_and(|have| have != *bytes)
        })
        .map(|(rel, _)| rel.as_str())
        .collect();
    if !foreign.is_empty() {
        return Err(Failure::new(format!(
            "rolesmith did not generate these files and will not overwrite them; move them away first: {}",
            foreign.join(", ")
        )));
    }

    let mut synced = Synced {
        required,
        ..Synced::default()
    };
    for rel in old.iter().filter(|rel| !generated.contains_key(*rel)) {
        let path = dir.join(rel);
        match std::fs::remove_file(&path) {
            Ok(()) => synced.removed.push(rel.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(Failure::new(format!(
                    "cannot delete {}: {e}",
                    path.display()
                )));
            }
        }
        remove_empty_parents(place, &path);
    }
    for (rel, (bytes, executable)) in &generated {
        if write_if_changed(&dir.join(rel), bytes, *executable)? {
            synced.written.push(rel.clone());
        }
    }

    let listed: Vec<&String> = generated.keys().collect();
    let manifest_path = &place.manifest;
    let manifest_name = if place.global {
        GLOBAL_MANIFEST
    } else {
        MANIFEST
    };
    if !listed.is_empty() {
        let manifest = serde_json::to_string_pretty(&serde_json::json!({ "files": listed }))
            .expect("the manifest serializes")
            + "\n";
        if write_if_changed(manifest_path, manifest.as_bytes(), false)? {
            synced.written.push(manifest_name.to_owned());
        }
    } else if manifest_path.exists() {
        // Nothing generated: the place is left as it was before rolesmith wrote into it.
        remove_if_present(manifest_path)?;
        if !place.global {
            let _ = std::fs::remove_dir(manifest_path.parent().expect("a file has a directory"));
        }
        synced.removed.push(manifest_name.to_owned());
    }
    if place.global {
        return Ok(synced);
    }
    if update_gitignore(dir, &generated)? {
        let changed = if dir.join(GITIGNORE).exists() {
            &mut synced.written
        } else {
            &mut synced.removed
        };
        changed.push(GITIGNORE.to_owned());
    }
    if !record.role.is_empty() {
        remember(dir)?;
    }
    Ok(synced)
}

/// Every file `entry` installs at `place`, for its role and each role it requires, added to
/// `into`; answers the roles it requires, transitively. A path already in `into` with other
/// content is refused: two roles of one name, or one role required at two revs.
fn generate(
    place: &Place,
    entry: &RoleEntry,
    into: &mut Generated,
) -> Result<Vec<String>, Failure> {
    let found = locate(place, entry)?;
    let (lib, _tree) = sources::read_role(&found)?;
    // Loaded for one role, the library holds it first and then every role it requires.
    let required = &lib.roles[1..];

    let mut files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    for role in &lib.roles {
        role_files(&lib, role, &found, &mut files)?;
    }

    let mut bases = BTreeSet::new();
    for target in &entry.into {
        bases.insert(target_dir(target)?);
    }
    for base in bases {
        for (rel, bytes, exec) in &files {
            let path = format!("{base}/{rel}");
            let file = (bytes.clone(), *exec);
            if into
                .insert(path.clone(), file.clone())
                .is_some_and(|had| had != file)
            {
                return Err(Failure::new(format!(
                    "two roles would both write {path} differently (one role name from two \
                         sources, or a role required at two revs); bring them to one rev or remove \
                         one of them from {}",
                    place.record_name()
                )));
            }
        }
    }
    Ok(required.iter().map(|r| r.name.clone()).collect())
}

/// The files of `role`, read from `lib` (the source and rev of `found`), by their path in a
/// target's skill directory: its core skill, which opens by naming the core skills of the roles
/// it requires, and each of its skills.
fn role_files(
    lib: &library::Library,
    role: &library::Role,
    found: &Found,
    files: &mut Vec<(String, Vec<u8>, bool)>,
) -> Result<(), Failure> {
    let qualified = format!("{}/{}", found.source, role.name);
    let comment = format!(
        "# Generated by rolesmith from {qualified} at {}; `rolesmith sync` rewrites it, do not edit.",
        found.rev
    );
    let core_name = format!("rolesmith-{}", role.name);
    let requires = lib.requires(&role.name);
    let builds_on = if requires.is_empty() {
        String::new()
    } else {
        let cores: Vec<String> = requires
            .iter()
            .map(|r| format!("`rolesmith-{}`", r.name))
            .collect();
        format!(
            "This role builds on others; their core skills are in force too, read them: {}.\n\n",
            cores.join(", ")
        )
    };
    let mut core = format!(
        "---\n{comment}\nname: {core_name}\ndescription: {}\n---\n\n{builds_on}{}\n",
        serde_json::to_string(&format!(
            "Always in force in this project: {}",
            role.toml.description
        ))
        .expect("a string serializes"),
        role.core.trim_end()
    );
    if !role.prohibitions.is_empty() {
        core.push_str("\n## Prohibitions\n\n");
        for p in &role.prohibitions {
            core.push_str(&format!("- {} ({})\n", p.rule, p.why));
        }
    }
    files.push((format!("{core_name}/SKILL.md"), core.into_bytes(), false));

    for skill in &role.skills {
        let name = format!("rolesmith-{}-{}", role.name, skill.name);
        for (rel, path) in files_of(&skill.dir)? {
            let bytes = std::fs::read(&path)
                .map_err(|e| Failure::new(format!("cannot read {}: {e}", path.display())))?;
            let bytes = if rel == "SKILL.md" {
                renamed_skill(&bytes, &comment, &name).ok_or_else(|| {
                    Failure::new(format!(
                        "{qualified}: skill {}'s SKILL.md has no front matter",
                        skill.name
                    ))
                })?
            } else {
                bytes
            };
            files.push((format!("{name}/{rel}"), bytes, executable(&path)));
        }
    }
    Ok(())
}

/// The role of `entry` in its source at its rev, adding the source from `url` when the machine
/// has none of that name, and fetching it when it lacks the rev.
fn locate(place: &Place, entry: &RoleEntry) -> Result<Found, Failure> {
    let mut record = Sources::load()?;
    match record.source.get(&entry.source) {
        None => sources::add(&mut record, &entry.source, &entry.url)?,
        Some(known) if known.url != entry.url => {
            return Err(Failure::new(format!(
                "{} says source {} is {}, but this machine has it as {}",
                place.record_name(),
                entry.source,
                entry.url,
                known.url
            )));
        }
        Some(_) => {}
    }
    let clone = sources::clone_dir(&entry.source)?;
    if sources::commit(&clone, &entry.rev).is_none() {
        sources::git(Some(&clone), &["fetch", "--quiet", "origin"])?;
    }
    sources::resolve(
        &record,
        &format!("{}/{}", entry.source, entry.name),
        Some(&entry.rev),
    )
}

/// A role skill's SKILL.md as installed: the generated comment first in its front matter, and its
/// `name` the installed one.
fn renamed_skill(bytes: &[u8], comment: &str, name: &str) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let (yaml, body) = library::split_front_matter(text)?;
    let mut out = format!("---\n{comment}\n");
    for line in yaml.lines() {
        if line.starts_with("name:") {
            out.push_str(&format!("name: {name}\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str("---\n");
    out.push_str(body);
    Some(out.into_bytes())
}

/// Every file under `dir`, by its path relative to `dir` (with `/`), sorted; a symbolic link is
/// refused, since it could point out of the library.
fn files_of(dir: &Path) -> Result<Vec<(String, PathBuf)>, Failure> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(at) = pending.pop() {
        let entries = std::fs::read_dir(&at)
            .map_err(|e| Failure::new(format!("cannot read {}: {e}", at.display())))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|e| Failure::new(format!("cannot read {}: {e}", path.display())))?;
            if kind.is_symlink() {
                return Err(Failure::new(format!(
                    "{} is a symbolic link; a skill's files are copied as files",
                    path.display()
                )));
            } else if kind.is_dir() {
                pending.push(path);
            } else {
                let rel = path.strip_prefix(dir).expect("under dir");
                let rel: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push((rel.join("/"), path));
            }
        }
    }
    found.sort();
    Ok(found)
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

/// Write `bytes` to `path` (making its directory), unless it already holds exactly them with the
/// same executable bit; answers whether it wrote.
fn write_if_changed(path: &Path, bytes: &[u8], exec: bool) -> Result<bool, Failure> {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::read(path).is_ok_and(|have| have == bytes) && executable(path) == exec {
        return Ok(false);
    }
    let fail = |e: std::io::Error| Failure::new(format!("cannot write {}: {e}", path.display()));
    std::fs::create_dir_all(path.parent().expect("a file has a directory")).map_err(fail)?;
    std::fs::write(path, bytes).map_err(fail)?;
    let mode = if exec { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(fail)?;
    Ok(true)
}

/// The files the manifest at `file` lists; none when there is no manifest.
fn read_manifest(file: &Path) -> Result<BTreeSet<String>, Failure> {
    #[derive(Deserialize)]
    struct Manifest {
        files: BTreeSet<String>,
    }
    match std::fs::read(file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(Failure::new(format!("cannot read {}: {e}", file.display()))),
        Ok(bytes) => serde_json::from_slice::<Manifest>(&bytes)
            .map(|m| m.files)
            .map_err(|e| Failure::new(format!("{} does not parse: {e}", file.display()))),
    }
}

/// Remove the directories above a deleted file that it left empty, up to its target's skill
/// directory.
fn remove_empty_parents(place: &Place, file: &Path) {
    let dir = &place.root;
    let bases: Vec<PathBuf> = TARGETS.iter().map(|(_, base)| dir.join(base)).collect();
    let mut at = file.parent();
    while let Some(parent) = at {
        if parent == dir || bases.iter().any(|b| b == parent) {
            break;
        }
        if std::fs::remove_dir(parent).is_err() {
            break;
        }
        at = parent.parent();
    }
}

/// Make `.gitignore`'s rolesmith block ignore the manifest's directory and each generated skill;
/// the rest of the file is left as it is. Answers whether it wrote.
fn update_gitignore(dir: &Path, generated: &Generated) -> Result<bool, Failure> {
    let path = dir.join(GITIGNORE);
    let text = match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Failure::new(format!("cannot read {}: {e}", path.display()))),
        Ok(text) => text,
    };
    let (before, after) = match (text.find(GITIGNORE_BEGIN), text.find(GITIGNORE_END)) {
        (Some(b), Some(e)) if b < e => {
            let end = e + GITIGNORE_END.len();
            let end = end + usize::from(text[end..].starts_with('\n'));
            (&text[..b], &text[end..])
        }
        _ => (text.as_str(), ""),
    };
    let mut block = String::new();
    if !generated.is_empty() {
        let skills: BTreeSet<String> = generated
            .keys()
            .filter_map(|rel| {
                let base = TARGETS.iter().find(|(_, b)| rel.starts_with(b))?.1;
                let skill = rel[base.len() + 1..].split('/').next()?;
                Some(format!("/{base}/{skill}/\n"))
            })
            .collect();
        let manifest_dir = MANIFEST.split('/').next().expect("a path");
        let lead = if before.is_empty() || before.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        block = format!(
            "{lead}{GITIGNORE_BEGIN}\n/{manifest_dir}/\n{}{GITIGNORE_END}\n",
            skills.into_iter().collect::<String>()
        );
    }
    let new = format!("{before}{block}{after}");
    if new == text {
        return Ok(false);
    }
    if new.is_empty() {
        // The file held only rolesmith's block, so rolesmith made it.
        remove_if_present(&path)?;
        return Ok(true);
    }
    write_if_changed(&path, new.as_bytes(), false)
}
