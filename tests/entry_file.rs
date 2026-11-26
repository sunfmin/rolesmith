//! The managed section of a project's entry files through the built binary: written
//! by `use` and `sync` into the entry file of each target, nothing outside it touched, written
//! again identically, and taken out by `remove`.

mod common;

use std::path::{Path, PathBuf};

use common::{Machine, published, stderr};
use rolesmith::entry_file::{BEGIN, END, section};

/// A machine with source `felix` (role app-store-releaser) and an empty project directory.
fn setup() -> (Machine, PathBuf) {
    let m = Machine::new();
    let (lib, _) = published(&m, "felix", &["app-store-releaser"]);
    m.json(&["source", "add", "felix", lib.url()]);
    let project = m.path("project");
    std::fs::create_dir_all(&project).unwrap();
    (m, project)
}

fn use_into(m: &Machine, project: &Path, into: &[&str]) -> serde_json::Value {
    let mut args = vec!["use", "app-store-releaser"];
    for t in into {
        args.extend(["--into", t]);
    }
    args.extend(["--project", project.to_str().unwrap()]);
    m.json(&args)
}

fn sync(m: &Machine, project: &Path) -> serde_json::Value {
    m.json(&["sync", "--project", project.to_str().unwrap()])
}

fn remove(m: &Machine, project: &Path) -> serde_json::Value {
    m.json(&[
        "remove",
        "app-store-releaser",
        "--project",
        project.to_str().unwrap(),
    ])
}

fn read(project: &Path, rel: &str) -> String {
    std::fs::read_to_string(project.join(rel)).unwrap()
}

#[test]
fn a_missing_claude_md_is_created_with_the_section_alone_and_removed_with_the_last_role() {
    let (m, project) = setup();
    let out = use_into(&m, &project, &["claude-code"]);
    assert!(
        out["written"]
            .as_array()
            .unwrap()
            .contains(&"CLAUDE.md".into()),
        "{out}"
    );
    assert_eq!(read(&project, "CLAUDE.md"), section());
    assert!(!project.join("AGENTS.md").exists());
    assert!(
        !read(&project, ".gitignore").contains("CLAUDE.md"),
        "the section is committed"
    );

    let bytes = std::fs::read(project.join("CLAUDE.md")).unwrap();
    assert_eq!(sync(&m, &project)["written"], serde_json::json!([]));
    assert_eq!(std::fs::read(project.join("CLAUDE.md")).unwrap(), bytes);

    let out = remove(&m, &project);
    assert!(
        out["removed"]
            .as_array()
            .unwrap()
            .contains(&"CLAUDE.md".into()),
        "{out}"
    );
    assert!(!project.join("CLAUDE.md").exists());
}

#[test]
fn text_outside_the_section_is_kept_and_a_hand_edit_inside_it_is_written_back() {
    let (m, project) = setup();
    std::fs::write(project.join("CLAUDE.md"), "# Mine\n\nKeep it small.\n").unwrap();
    use_into(&m, &project, &["claude-code"]);
    assert_eq!(
        read(&project, "CLAUDE.md"),
        format!("# Mine\n\nKeep it small.\n\n{}", section())
    );

    // Edits on both sides of the section, and one inside it.
    let edited = format!(
        "# Mine, edited\n\nKeep it small.\n\n{}More of mine.\n",
        section().replace("Before you start any task", "Whenever")
    );
    std::fs::write(project.join("CLAUDE.md"), &edited).unwrap();
    let out = sync(&m, &project);
    assert_eq!(out["written"], serde_json::json!(["CLAUDE.md"]));
    assert_eq!(
        read(&project, "CLAUDE.md"),
        format!(
            "# Mine, edited\n\nKeep it small.\n\n{}More of mine.\n",
            section()
        )
    );

    remove(&m, &project);
    assert_eq!(
        read(&project, "CLAUDE.md"),
        "# Mine, edited\n\nKeep it small.\n\nMore of mine.\n"
    );
}

#[test]
fn a_file_that_already_has_the_section_is_left_as_it_is() {
    let (m, project) = setup();
    let had = format!("# Mine\n\n{}\nAfter.\n", section());
    std::fs::write(project.join("CLAUDE.md"), &had).unwrap();
    let out = use_into(&m, &project, &["claude-code"]);
    assert!(
        !out["written"]
            .as_array()
            .unwrap()
            .contains(&"CLAUDE.md".into()),
        "{out}"
    );
    assert_eq!(read(&project, "CLAUDE.md"), had);
}

#[test]
fn two_targets_of_one_entry_file_write_one_section_and_each_file_gets_its_own() {
    let (m, project) = setup();
    use_into(&m, &project, &["codex", "agents"]);
    assert_eq!(read(&project, "AGENTS.md"), section());
    assert!(!project.join("CLAUDE.md").exists());

    let (m, project) = setup();
    use_into(&m, &project, &["claude-code", "codex", "agents"]);
    assert_eq!(read(&project, "AGENTS.md"), section());
    assert_eq!(read(&project, "CLAUDE.md"), section());
    remove(&m, &project);
    assert!(!project.join("AGENTS.md").exists());
    assert!(!project.join("CLAUDE.md").exists());
}

#[test]
fn an_entry_file_that_imports_or_links_the_other_gets_the_section_and_no_second_file_is_made() {
    for (have, mine, into) in [
        ("CLAUDE.md", "@AGENTS.md\n", "codex"),
        ("AGENTS.md", "Read [the rules](CLAUDE.md).\n", "claude-code"),
    ] {
        let (m, project) = setup();
        std::fs::write(project.join(have), mine).unwrap();
        use_into(&m, &project, &[into]);
        assert_eq!(read(&project, have), format!("{mine}\n{}", section()));
        let other = if have == "CLAUDE.md" {
            "AGENTS.md"
        } else {
            "CLAUDE.md"
        };
        assert!(!project.join(other).exists(), "{other} was made");
        remove(&m, &project);
        assert_eq!(read(&project, have), mine);
    }
}

#[test]
fn a_symlinked_entry_file_gets_one_section() {
    let (m, project) = setup();
    std::fs::write(project.join("AGENTS.md"), "# Mine\n").unwrap();
    std::os::unix::fs::symlink("AGENTS.md", project.join("CLAUDE.md")).unwrap();
    use_into(&m, &project, &["claude-code", "codex"]);
    assert_eq!(
        read(&project, "AGENTS.md"),
        format!("# Mine\n\n{}", section())
    );
    assert!(
        std::fs::symlink_metadata(project.join("CLAUDE.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn a_section_without_its_last_line_is_refused() {
    let (m, project) = setup();
    let broken = format!("# Mine\n{BEGIN}\nhalf\n");
    std::fs::write(project.join("CLAUDE.md"), &broken).unwrap();
    let o = m.rolesmith(&[
        "use",
        "app-store-releaser",
        "--into",
        "claude-code",
        "--project",
        project.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains(END), "{}", stderr(&o));
    assert_eq!(read(&project, "CLAUDE.md"), broken);
}

#[test]
fn an_entry_file_or_gitignore_that_cannot_be_read_as_text_is_refused_and_left_alone() {
    for rel in ["CLAUDE.md", ".gitignore"] {
        let (m, project) = setup();
        let bytes = b"# Mine \xff\n".to_vec();
        std::fs::write(project.join(rel), &bytes).unwrap();
        let o = m.rolesmith(&[
            "use",
            "app-store-releaser",
            "--into",
            "claude-code",
            "--project",
            project.to_str().unwrap(),
        ]);
        assert_eq!(o.status.code(), Some(1), "{rel}");
        assert!(stderr(&o).contains("cannot read"), "{}", stderr(&o));
        assert_eq!(std::fs::read(project.join(rel)).unwrap(), bytes, "{rel}");
    }
}
