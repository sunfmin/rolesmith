//! The Homebrew a `brew` tool is installed by, on the host asked for (SPEC.md §3.5, ADR 0005):
//! what it offers and has installed of a formula, refreshing a formula's own tap, and installing or
//! upgrading a formula while another brew may hold its lock. Every command runs on the host, so
//! its prefix, taps and formulae are the host's own brew's.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::commands::Failure;
use crate::host::{self, Host};

/// The one program this module runs.
const BREW: &str = "brew";

/// How long a tap's last fetch keeps it fresh: within it, a tap is not fetched again.
const TAP_FRESH_MINUTES: u32 = 24 * 60;

/// The file in a tap's `.git` whose time is when rolesmith last refreshed it.
const TAP_STAMP: &str = "rolesmith-refreshed";

/// What brew says when another brew process holds a formula's lock (Homebrew's `OperationInProgressError`).
const LOCKED: &str = "has already locked";

/// How long an install waits for another brew to let go of the formula, and how often it tries.
const LOCK_WAIT: Duration = Duration::from_secs(30 * 60);
const LOCK_POLL: Duration = Duration::from_secs(1);

/// Run `brew <args>` on `host` with no input, keeping its output; a host without brew is refused.
pub fn run(host: &Host, args: &[&str]) -> Result<std::process::Output, Failure> {
    let line: Vec<String> = std::iter::once(BREW)
        .chain(args.iter().copied())
        .map(host::quote)
        .collect();
    let out = host.sh(&line.join(" "))?;
    if out.status.code() == Some(127) {
        return Err(Failure::new(format!(
            "cannot run `{BREW} {}`{}: {BREW} is not found",
            args.join(" "),
            host.on()
        )));
    }
    Ok(out)
}

/// What `brew info --json=v2` says of one formula.
pub struct Info {
    /// Its stable version, if it has one.
    pub stable: Option<String>,
    /// The versions brew has installed of it.
    pub installed: Vec<String>,
}

impl Info {
    /// Whether brew has the formula's stable version installed (`1.0` or a rebuild, `1.0_1`).
    fn at_stable(&self) -> bool {
        self.stable.as_deref().is_some_and(|s| {
            self.installed
                .iter()
                .any(|v| v == s || v.strip_prefix(s).is_some_and(|r| r.starts_with('_')))
        })
    }
}

/// `brew info --json=v2 <formula>` on `host`; a formula brew does not know is refused.
pub fn info(host: &Host, formula: &str) -> Result<Info, Failure> {
    let out = run(host, &["info", "--json=v2", formula])?;
    if !out.status.success() {
        return Err(Failure::new(format!(
            "brew{} has no formula {formula}: {}",
            host.on(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(parse_info(&out.stdout))
}

fn parse_info(json: &[u8]) -> Info {
    let doc: Value = serde_json::from_slice(json).unwrap_or_default();
    let formula = &doc["formulae"][0];
    Info {
        stable: formula["versions"]["stable"].as_str().map(str::to_owned),
        installed: formula["installed"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["version"].as_str().map(str::to_owned))
            .collect(),
    }
}

/// Whether brew on `host` has `formula` at its stable version, and why not, after refreshing the
/// formula's tap; a tap that cannot be refreshed is compared as it is, and the reason says so.
pub fn at_stable(host: &Host, formula: &str) -> Result<(bool, String), Failure> {
    let note = refresh_tap(host, formula)?;
    let (passes, mut reason) = match info(host, formula) {
        Err(f) => (false, format!("passes, but {}", f.message)),
        Ok(info) if info.at_stable() => (true, "passes".to_owned()),
        Ok(Info { stable, installed }) => {
            let has = match installed.as_slice() {
                [] => format!("no {formula}"),
                _ => format!("{formula} {}", installed.join(", ")),
            };
            let stable = stable.map_or("no stable version".to_owned(), |s| {
                format!("its stable {s}")
            });
            (
                false,
                format!("passes, but brew has {has} installed, not {stable}"),
            )
        }
    };
    if let Some(note) = note {
        reason.push_str("; ");
        reason.push_str(&note);
    }
    Ok((passes, reason))
}

/// Bring the tap `formula` names (`<user>/<repo>/<name>`) up to its remote, unless it was fetched
/// within [`TAP_FRESH_MINUTES`] (the time of [`TAP_STAMP`] in its `.git`, touched only when a pull
/// succeeds: a failed fetch rewrites `FETCH_HEAD` too): a `git pull --ff-only` of that tap
/// alone, never `brew update`. A formula without a tap, or a tap brew has not tapped, is left as it
/// is. Answers why the tap could not be refreshed, if it could not; that is not a failure.
fn refresh_tap(host: &Host, formula: &str) -> Result<Option<String>, Failure> {
    let Some(tap) = tap_of(formula) else {
        return Ok(None);
    };
    let script = format!(
        "r=$({BREW} --repo {tap}) && [ -d \"$r/.git\" ] || exit 0\n\
         s=\"$r/.git/{TAP_STAMP}\"\n\
         [ -n \"$(find \"$s\" -mmin -{TAP_FRESH_MINUTES} 2>/dev/null)\" ] && exit 0\n\
         git -C \"$r\" pull --ff-only --quiet && touch \"$s\"",
        tap = host::quote(tap)
    );
    let out = host.sh(&script)?;
    if out.status.success() {
        return Ok(None);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Ok(Some(format!(
        "its tap {tap} could not be refreshed, so its stable is the tap's as it was: {}",
        stderr.trim().lines().last().unwrap_or("git exits non-zero")
    )))
}

/// The tap of a formula named with one, `<user>/<repo>` of `<user>/<repo>/<name>`.
fn tap_of(formula: &str) -> Option<&str> {
    let (tap, _name) = formula.rsplit_once('/')?;
    tap.contains('/').then_some(tap)
}

/// `brew <verb> <formula>` on `host` (`install` or `upgrade`). While another brew holds the
/// formula's lock it waits and tries again, so that two at once do one install between them;
/// brew answers 0 to the second once the first has installed it. Any other failure is refused.
pub fn change(host: &Host, verb: &str, formula: &str) -> Result<(), Failure> {
    let start = Instant::now();
    loop {
        let out = run(host, &[verb, formula])?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains(LOCKED) && start.elapsed() < LOCK_WAIT {
            std::thread::sleep(LOCK_POLL);
            continue;
        }
        return Err(Failure::new(format!(
            "`brew {verb} {formula}` failed{}: {}",
            host.on(),
            stderr.trim()
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_gives_the_stable_and_the_installed_versions() {
        let info = parse_info(
            br#"{"formulae":[{"versions":{"stable":"7.0.0"},"installed":[{"version":"6.0"},{"version":"7.0.0_1"}]}]}"#,
        );
        assert_eq!(info.stable.as_deref(), Some("7.0.0"));
        assert!(info.at_stable(), "a rebuild of the stable version is it");
        let info = parse_info(
            br#"{"formulae":[{"versions":{"stable":"7.0.10"},"installed":[{"version":"7.0.1"}]}]}"#,
        );
        assert!(!info.at_stable());
        let none = parse_info(b"not json");
        assert_eq!((none.stable, none.installed.len()), (None, 0));
    }

    #[test]
    fn a_formula_without_a_tap_has_no_tap_to_refresh() {
        assert!(matches!(refresh_tap(&Host::Here, "jq"), Ok(None)));
    }

    #[test]
    fn an_install_waits_for_a_held_lock_as_long_as_spec_says() {
        // SPEC.md §3.5: tried again each second for up to 30 minutes.
        assert_eq!((LOCK_WAIT.as_secs(), LOCK_POLL.as_secs()), (30 * 60, 1));
    }

    #[test]
    fn only_a_formula_named_with_its_tap_has_one() {
        assert_eq!(tap_of("sunfmin/tap/flux"), Some("sunfmin/tap"));
        assert_eq!(tap_of("asc"), None);
        assert_eq!(tap_of("odd/asc"), None);
    }
}
