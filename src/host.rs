//! Where a role's programs are checked and installed (SPEC.md §3.5): this machine, or another one
//! over ssh. This is the one place a shell line is run for a tool, and the only place that builds
//! the `ssh` command line; the other host needs nothing of rolesmith, only `sh`.

use std::process::{Command, Output, Stdio};

use crate::commands::Failure;

/// ssh's exit code when it could not reach the host, set apart from the command's own.
const SSH_UNREACHABLE: i32 = 255;

/// The host a command checks and installs on.
pub enum Host {
    /// This machine.
    Here,
    /// Another machine, by the name `ssh` takes for it.
    Ssh(String),
}

impl Host {
    /// `--host <name>` given, or this machine.
    pub fn from_arg(name: Option<String>) -> Host {
        name.map_or(Host::Here, Host::Ssh)
    }

    /// The host's name as `--json` answers it: `None` for this machine.
    pub fn name(&self) -> Option<&str> {
        match self {
            Host::Here => None,
            Host::Ssh(name) => Some(name),
        }
    }

    /// What a command line that acts on this host adds: ` --host <name>`, or nothing here.
    pub fn flag(&self) -> String {
        self.name().map_or(String::new(), |name| {
            format!(" --host {}", quote_word(name))
        })
    }

    /// Run `script` with `sh -c` on this host, stdin closed, and answer what it did. On another
    /// host it runs as `ssh -o BatchMode=yes -- <host> sh -c '<script>'`, so ssh never asks for a
    /// password; ssh failing to reach the host is a failure naming it, never the script's answer.
    pub fn sh(&self, script: &str) -> Result<Output, Failure> {
        let mut cmd = match self {
            Host::Here => {
                let mut cmd = Command::new("sh");
                cmd.arg("-c").arg(script);
                cmd
            }
            Host::Ssh(host) => {
                let mut cmd = Command::new("ssh");
                // ssh joins the words after the host and hands them to the remote shell.
                cmd.args(["-o", "BatchMode=yes", "--", host, "sh", "-c"])
                    .arg(quote(script));
                cmd
            }
        };
        let out = cmd.stdin(Stdio::null()).output().map_err(|e| match self {
            Host::Here => Failure::new(format!("cannot run `sh -c {script}`: {e}")),
            Host::Ssh(host) => Failure::new(format!("cannot run `ssh` to reach {host}: {e}")),
        })?;
        if let Host::Ssh(host) = self
            && out.status.code() == Some(SSH_UNREACHABLE)
        {
            return Err(Failure::new(format!(
                "cannot reach host {host}: `ssh -o BatchMode=yes {host}` exits {SSH_UNREACHABLE}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(out)
    }

    /// How a message names this host after what was run on it: ` on <host>`, or nothing here.
    pub fn on(&self) -> String {
        self.name()
            .map_or(String::new(), |name| format!(" on {name}"))
    }
}

/// `value` in single quotes for sh, each `'` written as `'\''`.
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `word` as sh reads it back: as it is when it holds nothing sh would split or expand, quoted
/// otherwise.
fn quote_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.@:/".contains(c));
    if plain { word.to_owned() } else { quote(word) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_quoted_only_when_sh_would_change_it() {
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote_word("felix@laptop.local"), "felix@laptop.local");
        assert_eq!(quote_word("a b"), "'a b'");
        assert_eq!(Host::Here.flag(), "");
        assert_eq!(Host::Ssh("lap top".into()).flag(), " --host 'lap top'");
    }
}
