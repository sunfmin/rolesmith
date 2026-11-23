//! `doctor --probe` (SPEC.md §7.3): a mapped service's command read once, the way `env` reads it
//! but with nothing that could answer a prompt, under a bound; and the outcome each probe keeps on
//! this host, in `$XDG_DATA_HOME/rolesmith/probes.json`, by service, with the command it probed and
//! when. A value it reads is parsed for its names and dropped: it is never answered or kept.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::commands::Failure;
use crate::service_map;
use crate::sources;

/// How reading a source went.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome")]
pub enum Outcome {
    /// It finished, printing every expected variable, in `seconds`.
    #[serde(rename = "unattended")]
    Unattended { seconds: f64 },
    /// It did not finish within the bound, or failed asking for a terminal.
    #[serde(rename = "needs a person")]
    NeedsAPerson { reason: String },
    /// It exited non-zero (with its stderr) or printed a line that is not a variable.
    #[serde(rename = "refused")]
    Refused { reason: String },
    /// It finished without these expected variables.
    #[serde(rename = "incomplete")]
    Incomplete { missing: Vec<String> },
}

impl Outcome {
    /// Its name: the `outcome` it is written with.
    pub fn name(&self) -> String {
        let doc = serde_json::to_value(self).expect("serializes");
        doc["outcome"].as_str().expect("tagged").to_owned()
    }

    /// What it found, after its name.
    pub fn detail(&self) -> String {
        match self {
            Outcome::Unattended { seconds } => format!("read in {seconds}s"),
            Outcome::NeedsAPerson { reason } | Outcome::Refused { reason } => reason.clone(),
            Outcome::Incomplete { missing } => format!("does not print {}", missing.join(", ")),
        }
    }
}

/// One probe: when, and how it went; `doctor` answers it as `probe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probed {
    /// When, in UTC (RFC 3339).
    pub at: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// One probe as this host keeps it, with the command it probed: it counts only while the service
/// maps to that command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kept {
    pub command: String,
    #[serde(flatten)]
    pub probed: Probed,
}

/// What a source says on stderr when it fails for want of a terminal to ask on.
const ASKING: &[&str] = &[
    "a terminal is required",
    "no tty present",
    "not a tty",
    "/dev/tty",
    "Device not configured",
];

/// The most of a source's stderr an outcome keeps.
const STDERR_KEPT: usize = 400;

/// Read `command` once with `sh -c`, in a session of its own (no controlling terminal) with stdin a
/// pipe nothing writes to, for at most `bound`; kill it and everything it started when the bound
/// passes, and whatever it left running when it ends. `expected` are the variables it must print.
pub fn probe(command: &str, expected: &[String], bound: Duration) -> Outcome {
    let started = Instant::now();
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid is async-signal-safe and touches nothing of this process's memory. Its one
    // error is EPERM for a group leader, which a child just forked never is.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Outcome::Refused {
                reason: format!("cannot run `sh -c`: {e}"),
            };
        }
    };
    // Held open and never written: a source that reads stdin waits, as on a person.
    let _stdin = child.stdin.take();
    let (tx, rx) = mpsc::channel();
    for (is_out, mut pipe) in [
        (
            true,
            Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
        ),
        (false, Box::new(child.stderr.take().unwrap())),
    ] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            let _ = tx.send((is_out, bytes));
        });
    }
    drop(tx);
    let group = child.id() as libc::pid_t;
    // SAFETY: kill takes plain integers; the group is the session the child leads.
    let kill_group = || unsafe { libc::kill(-group, libc::SIGKILL) };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < bound => std::thread::sleep(Duration::from_millis(10)),
            _ => break None,
        }
    };
    let seconds = (started.elapsed().as_secs_f64() * 100.0).round() / 100.0;
    kill_group();
    let Some(status) = status else {
        let _ = child.wait();
        return Outcome::NeedsAPerson {
            reason: format!(
                "did not finish within {}s; killed with everything it started",
                bound.as_secs()
            ),
        };
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let left = bound.saturating_sub(started.elapsed()) + Duration::from_secs(1);
    while let Ok((is_out, bytes)) = rx.recv_timeout(left) {
        *(if is_out { &mut stdout } else { &mut stderr }) = bytes;
    }
    let said = String::from_utf8_lossy(&stderr).trim().to_owned();
    let said: String = said.chars().take(STDERR_KEPT).collect();
    if !status.success() {
        let exit = status
            .code()
            .map_or("is killed by a signal".to_owned(), |c| format!("exits {c}"));
        return match ASKING.iter().any(|a| said.contains(a)) {
            true => Outcome::NeedsAPerson {
                reason: format!("{exit} asking for a terminal: {said}"),
            },
            false => Outcome::Refused {
                reason: format!("{exit}: {said}"),
            },
        };
    }
    match service_map::parse(&String::from_utf8_lossy(&stdout)) {
        Err((line, why)) => Outcome::Refused {
            reason: format!("line {line} of what it prints {why}"),
        },
        Ok(vars) => {
            let missing: Vec<String> = (expected.iter())
                .filter(|v| !vars.contains_key(*v))
                .cloned()
                .collect();
            match missing.is_empty() {
                true => Outcome::Unattended { seconds },
                false => Outcome::Incomplete { missing },
            }
        }
    }
}

/// `$XDG_DATA_HOME/rolesmith/probes.json`.
pub fn file() -> Result<PathBuf, Failure> {
    Ok(sources::data_dir()?.join("probes.json"))
}

/// Every outcome this host keeps, by service; none when there is no file.
pub fn kept() -> Result<BTreeMap<String, Kept>, Failure> {
    let file = file()?;
    match std::fs::read_to_string(&file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(Failure::new(format!("cannot read {}: {e}", file.display()))),
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| Failure::new(format!("{} does not parse: {e}", file.display()))),
    }
}

/// Keep `outcomes` (service, what it now keeps), each in place of what that service kept, under a
/// lock so probes run at once lose none; the file is replaced whole.
pub fn keep(outcomes: &BTreeMap<String, Kept>) -> Result<(), Failure> {
    let file = file()?;
    let fail = |e: std::io::Error| Failure::new(format!("cannot write {}: {e}", file.display()));
    std::fs::create_dir_all(file.parent().expect("the file has a directory")).map_err(fail)?;
    let lock = std::fs::File::create(file.with_extension("lock")).map_err(fail)?;
    lock.lock().map_err(fail)?;
    let mut all = kept()?;
    all.extend(outcomes.clone());
    let partial = file.with_extension("json.partial");
    std::fs::write(
        &partial,
        serde_json::to_string_pretty(&all).expect("serializes"),
    )
    .map_err(fail)?;
    std::fs::rename(&partial, &file).map_err(fail)
}

/// Now, in UTC, as RFC 3339 to the second.
pub fn now() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(secs)
}

/// `secs` after the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ` (the civil calendar, days from 1970).
fn rfc3339(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's days-to-civil, shifted to an era starting on March 1st.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_is_written_in_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        // Worked by `date -u -r <secs> +%FT%TZ`: a leap day and the end of a year.
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_798_761_599), "2026-12-31T23:59:59Z");
    }
}
