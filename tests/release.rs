//! `packaging/release.sh` (ADR 0002), run on a copy of it in a temporary repository with `cargo`,
//! `rustup`, `lipo` and `gh` faked: nothing is built and nothing is published. The real build is
//! the gate's `release.sh dry`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/release.sh");

/// A copy of release.sh in `<dir>/repo/packaging`, the fakes in `<dir>/bin`.
struct Release {
    dir: PathBuf,
}

const FAKE_CARGO: &str = r#"#!/bin/sh
for a; do case "$prev" in --target) target=$a ;; esac; prev=$a; done
mkdir -p "target/$target/release"
printf '#!/bin/sh\necho "rolesmith %s"\n' "${ROLESMITH_RELEASE-0.1.1+3.gabc1234}" >"target/$target/release/rolesmith"
chmod +x "target/$target/release/rolesmith"
"#;

const FAKE_LIPO: &str = r#"#!/bin/sh
case "$1" in -archs) echo "x86_64 arm64" ;; -create) cp "$4" "$3" ;; esac
"#;

// `workflow run` makes run <n+1>, which `run view` reports as $RUN_ENDS.
const FAKE_GH: &str = r#"#!/bin/sh
s=$FAKE_GH
case "$1 $2" in
"workflow run") echo "$*" >>"$s.calls"; echo $(( $(cat "$s.n") + 1 )) >"$s.n" ;;
"run list") n=$(cat "$s.n"); [ "$n" = 0 ] || echo "$n" ;;
"run view") case "$*" in *--log-failed*) echo "the run's failed log" ;; *) echo "$RUN_ENDS" ;; esac ;;
*) exit 1 ;;
esac
"#;

impl Release {
    fn new(name: &str) -> Release {
        let dir =
            std::env::temp_dir().join(format!("rolesmith-release-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("repo/packaging")).unwrap();
        std::fs::copy(SCRIPT, dir.join("repo/packaging/release.sh")).unwrap();
        for (tool, text) in [
            ("cargo", FAKE_CARGO),
            ("lipo", FAKE_LIPO),
            ("gh", FAKE_GH),
            ("rustup", "#!/bin/sh\n"),
        ] {
            let path = dir.join("bin").join(tool);
            std::fs::write(&path, text).unwrap();
            Command::new("chmod").arg("+x").arg(&path).status().unwrap();
        }
        std::fs::write(dir.join("gh.n"), "0\n").unwrap();
        Release { dir }
    }

    fn repo(&self) -> PathBuf {
        self.dir.join("repo")
    }

    fn run(&self, args: &[&str], run_ends: &str) -> Output {
        let path = format!(
            "{}:{}",
            self.dir.join("bin").display(),
            std::env::var("PATH").unwrap()
        );
        Command::new("sh")
            .arg(self.repo().join("packaging/release.sh"))
            .args(args)
            .env("PATH", path)
            .env("FAKE_GH", self.dir.join("gh"))
            .env("RUN_ENDS", run_ends)
            .env("RELEASE_WAIT_LIMIT", "20")
            .env("RELEASE_POLL", "0")
            .env_remove("ROLESMITH_RELEASE")
            .env_remove("DIST")
            .output()
            .expect("run release.sh")
    }

    fn gh_calls(&self) -> String {
        std::fs::read_to_string(self.dir.join("gh.calls")).unwrap_or_default()
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", text(&out.stderr));
    text(&out.stdout).trim().to_string()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn check_tag_takes_only_a_version_tag() {
    let r = Release::new("check-tag");
    for good in ["v0.1.0", "v1.22.333"] {
        assert!(r.run(&["check-tag", good], "").status.success(), "{good}");
    }
    for bad in [
        "", "v1", "v1.2", "1.2.3", "v1.2.3.4", "v1..3", "v1.2.x", "v1.2.3;x",
    ] {
        assert_eq!(
            r.run(&["check-tag", bad], "").status.code(),
            Some(2),
            "{bad:?}"
        );
    }
}

#[test]
fn build_leaves_the_universal_binary_and_a_formula_for_the_tap_to_fill_in() {
    let r = Release::new("build");
    let out = r.run(&["build", "v1.2.3", "out"], "");
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(r.gh_calls().is_empty(), "build ran gh: {}", r.gh_calls());

    let said = Command::new(r.repo().join("out/rolesmith"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(text(&said.stdout).trim(), "rolesmith 1.2.3");
    let formula = std::fs::read_to_string(r.repo().join("out/rolesmith.rb")).unwrap();
    assert!(formula.contains("class Rolesmith < Formula"));
    assert!(formula.contains("version \"1.2.3\""));
    assert!(formula.contains("url \"@URL@\""));
    assert!(formula.contains("sha256 \"@SHA256@\""));
    assert!(formula.contains("depends_on :macos"));
    // Universal: no arch, and the prebuilt binary is installed, nothing compiled.
    assert!(!formula.contains("depends_on arch"), "{formula}");
    assert!(!formula.contains("depends_on \"rust\""));
    assert!(formula.contains(
        "assert_equal \"rolesmith #{version}\", shell_output(\"#{bin}/rolesmith --version\").strip"
    ));

    // dry is the same build at the development form.
    let out = r.run(&["dry"], "");
    assert!(out.status.success(), "{}", text(&out.stderr));
    let formula = std::fs::read_to_string(r.repo().join("dist/rolesmith.rb")).unwrap();
    assert!(
        formula.contains("version \"0.1.1+3.gabc1234\""),
        "{formula}"
    );
}

#[test]
fn cut_runs_the_taps_workflow_waits_for_it_and_takes_away_the_tag_it_pushed() {
    let r = Release::new("cut");
    let repo = r.repo();
    let origin = r.dir.join("origin.git");
    git(&r.dir, &["init", "-q", "--bare", origin.to_str().unwrap()]);
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "one"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    let tagged = || git(&repo, &["ls-remote", "origin", "refs/tags/v1.2.3"]);

    let out = r.run(&["cut", "v1.2.3"], "completed success");
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        r.gh_calls().trim(),
        "workflow run release.yml -R sunfmin/homebrew-tap -f repo=rolesmith -f tag=v1.2.3"
    );
    assert_eq!(tagged(), "", "the tag the cut pushed is still on origin");
    assert_eq!(
        git(&repo, &["tag"]),
        "",
        "factorium's own tag step would fail"
    );

    let out = r.run(&["cut", "v1.2.3"], "completed failure");
    assert!(!out.status.success(), "a run that fails passes the cut");
    assert!(text(&out.stderr).contains("the run's failed log"));
    assert_eq!(tagged(), "", "the tag of a failed cut is still on origin");

    // factorium's tag step, after the cut, tags HEAD and pushes it: a cut run again leaves it.
    git(&repo, &["tag", "-a", "v1.2.3", "-m", "v1.2.3"]);
    git(&repo, &["push", "-q", "origin", "refs/tags/v1.2.3"]);
    git(&repo, &["tag", "-d", "v1.2.3"]);
    let out = r.run(&["cut", "v1.2.3"], "completed success");
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_ne!(tagged(), "", "the cut took away a tag it did not push");

    git(&repo, &["commit", "-q", "--allow-empty", "-m", "two"]);
    let out = r.run(&["cut", "v1.2.3"], "completed success");
    assert!(
        !out.status.success(),
        "a tag on another commit passes the cut"
    );
    assert!(
        text(&out.stderr).contains("not HEAD"),
        "{}",
        text(&out.stderr)
    );
}
