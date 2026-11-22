//! A project that uses roles (SPEC.md §4, §5.1): its record `rolesmith.toml`, the files generated
//! from it into each target's skill directory, their manifest `.rolesmith/manifest.json`, and the
//! `.gitignore` lines that ignore them.
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

/// Each target (§5.2) and the project directory its agent reads skills from; the one table of them.
/// claude-code: project skills are `.claude/skills/<name>/SKILL.md`
/// (https://code.claude.com/docs/en/skills, checked 2026-11-22).
pub const TARGETS: &[(&str, &str)] = &[("claude-code", ".claude/skills")];

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
    /// The record of the project at `dir`; empty when it has none.
    pub fn load(dir: &Path) -> Result<Record, Failure> {
        let file = dir.join(RECORD);
        let text = match std::fs::read_to_string(&file) {
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

    /// Write the record to the project at `dir`, unless it already says exactly this.
    pub fn save(&self, dir: &Path) -> Result<(), Failure> {
        let text = toml::to_string(self).expect("the record serializes");
        write_if_changed(&dir.join(RECORD), text.as_bytes(), false).map(|_| ())
    }
}

/// What [`sync`] changed in the project, as paths relative to its root.
#[derive(Debug, Default)]
pub struct Synced {
    pub written: Vec<String>,
    pub removed: Vec<String>,
}

/// A generated file: its bytes, and whether it is executable.
type Generated = BTreeMap<String, (Vec<u8>, bool)>;

/// Make the files generated into the project at `dir` equal what its `record` says, adding a source
/// the machine lacks from the record's `url`.
pub fn sync(dir: &Path, record: &Record) -> Result<Synced, Failure> {
    let mut generated = Generated::new();
    for entry in &record.role {
        generate(entry, &mut generated)?;
    }
    let old: BTreeSet<String> = read_manifest(dir)?;

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

    let mut synced = Synced::default();
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
        remove_empty_parents(dir, &path);
    }
    for (rel, (bytes, executable)) in &generated {
        if write_if_changed(&dir.join(rel), bytes, *executable)? {
            synced.written.push(rel.clone());
        }
    }

    let listed: Vec<&String> = generated.keys().collect();
    if !listed.is_empty() || !old.is_empty() {
        let manifest = serde_json::to_string_pretty(&serde_json::json!({ "files": listed }))
            .expect("the manifest serializes")
            + "\n";
        if write_if_changed(&dir.join(MANIFEST), manifest.as_bytes(), false)? {
            synced.written.push(MANIFEST.to_owned());
        }
    }
    if update_gitignore(dir, &generated)? {
        synced.written.push(GITIGNORE.to_owned());
    }
    Ok(synced)
}

/// Every file `entry` installs, added to `into`; a path two roles would both write is refused.
fn generate(entry: &RoleEntry, into: &mut Generated) -> Result<(), Failure> {
    let found = locate(entry)?;
    let (lib, _tree) = sources::read_role(&found)?;
    let role = &lib.roles[0];
    let comment = format!(
        "# Generated by rolesmith from {} at {}; `rolesmith sync` rewrites it, do not edit.",
        found.qualified(),
        found.rev
    );

    let mut files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    let core_name = format!("rolesmith-{}", role.name);
    let mut core = format!(
        "---\n{comment}\nname: {core_name}\ndescription: {}\n---\n\n{}\n",
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
                        "{}: skill {}'s SKILL.md has no front matter",
                        found.qualified(),
                        skill.name
                    ))
                })?
            } else {
                bytes
            };
            files.push((format!("{name}/{rel}"), bytes, executable(&path)));
        }
    }

    for target in &entry.into {
        let base = target_dir(target)?;
        for (rel, bytes, exec) in &files {
            let path = format!("{base}/{rel}");
            if into.insert(path.clone(), (bytes.clone(), *exec)).is_some() {
                return Err(Failure::new(format!(
                    "two roles would both write {path}; remove one of them from {RECORD}"
                )));
            }
        }
    }
    Ok(())
}

/// The role of `entry` in its source at its rev, adding the source from `url` when the machine
/// has none of that name, and fetching it when it lacks the rev.
fn locate(entry: &RoleEntry) -> Result<Found, Failure> {
    let mut record = Sources::load()?;
    match record.source.get(&entry.source) {
        None => sources::add(&mut record, &entry.source, &entry.url)?,
        Some(known) if known.url != entry.url => {
            return Err(Failure::new(format!(
                "{RECORD} says source {} is {}, but this machine has it as {}",
                entry.source, entry.url, known.url
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

/// The files the project's manifest lists; none when it has no manifest.
fn read_manifest(dir: &Path) -> Result<BTreeSet<String>, Failure> {
    #[derive(Deserialize)]
    struct Manifest {
        files: BTreeSet<String>,
    }
    let file = dir.join(MANIFEST);
    match std::fs::read(&file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(Failure::new(format!("cannot read {}: {e}", file.display()))),
        Ok(bytes) => serde_json::from_slice::<Manifest>(&bytes)
            .map(|m| m.files)
            .map_err(|e| Failure::new(format!("{} does not parse: {e}", file.display()))),
    }
}

/// Remove the directories above a deleted file that it left empty, up to its target's skill
/// directory.
fn remove_empty_parents(dir: &Path, file: &Path) {
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
    write_if_changed(&path, new.as_bytes(), false)
}
