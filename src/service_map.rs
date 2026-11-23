//! Where this host reads a service's credentials (SPEC.md §7.1, ADR 0006): its own
//! `$XDG_CONFIG_HOME/rolesmith/services.toml` maps a service to a command whose stdout is the
//! service's variables, with an optional `check`; a service it does not map is read from mytokens.
//! The setting is the host's, never a library's.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::commands::Failure;
use crate::sources;

/// The setting's file name, beside `sources.toml`; a library never holds a file of this name.
pub const FILE: &str = "services.toml";

/// The top-level table of the setting; a library never holds a table of this name.
pub const TABLE: &str = "service";

/// The host's `services.toml`: each mapped service by its name.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceMap {
    #[serde(default)]
    service: BTreeMap<String, Mapped>,
}

/// One mapped service: the command that prints its variables, and what tells it is present.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mapped {
    /// Run with `sh -c`, stdin closed; its stdout is `NAME=value` / `export NAME='value'` lines.
    pub command: String,
    /// Run by `doctor` in place of the command, which it never runs: exit 0 means present.
    pub check: Option<String>,
}

/// `$XDG_CONFIG_HOME/rolesmith/services.toml` (default `~/.config/rolesmith/`).
pub fn file() -> Result<PathBuf, Failure> {
    Ok(sources::sources_file()?.with_file_name(FILE))
}

impl ServiceMap {
    /// The host's setting; empty (every service from mytokens) when there is no file.
    pub fn load() -> Result<ServiceMap, Failure> {
        let file = file()?;
        match std::fs::read_to_string(&file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ServiceMap::default()),
            Err(e) => Err(Failure::new(format!("cannot read {}: {e}", file.display()))),
            Ok(text) => toml::from_str(&text).map_err(|e| {
                Failure::new(format!(
                    "{} does not parse: {}",
                    file.display(),
                    e.message().trim()
                ))
            }),
        }
    }

    /// How this host reads `service`, or `None` for mytokens.
    pub fn get(&self, service: &str) -> Option<&Mapped> {
        self.service.get(service)
    }
}

impl Mapped {
    /// The variables `service`'s command prints now, kept nowhere. A command that cannot be run,
    /// exits non-zero (refused with its stderr) or prints a line that is not a variable is a
    /// failure, and none of its variables is answered.
    pub fn variables(&self, service: &str) -> Result<BTreeMap<String, String>, Failure> {
        let out = Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| Failure::new(format!("service {service}: cannot run `sh -c`: {e}")))?;
        if !out.status.success() {
            let code = out
                .status
                .code()
                .map_or("is killed by a signal".to_owned(), |c| format!("exits {c}"));
            return Err(Failure::new(format!(
                "service {service}: its command `{}` {code}: {}",
                self.command,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        parse(&String::from_utf8_lossy(&out.stdout)).map_err(|(line, why)| {
            Failure::new(format!(
                "service {service}: line {line} of what its command `{}` prints {why}; it prints `NAME=value` or `export NAME='value'` lines",
                self.command
            ))
        })
    }
}

/// The variables of `text`: `NAME=value` statements, each optionally after `export`, one a line,
/// with blank lines and `#` comments between them. A value is read as sh reads a word: `'…'`
/// literally (spanning lines, as `mytokens env` quotes), `"…"` with `\` escaping `$`, `` ` ``,
/// `"`, `\` and a newline, and `\` outside quotes escaping the next character. A later statement
/// of a name wins, as in sh. A statement that is not of this form is the line it starts on with
/// what is wrong, never its text (it may hold a secret).
pub fn parse(text: &str) -> Result<BTreeMap<String, String>, (usize, &'static str)> {
    let mut vars = BTreeMap::new();
    let mut chars = text.chars().peekable();
    let mut line = 1;
    loop {
        // Between statements: blanks, newlines and comments.
        while let Some(&c) = chars.peek() {
            match c {
                '\n' => line += 1,
                ' ' | '\t' | '\r' | ';' => {}
                '#' => {
                    while chars.peek().is_some_and(|&c| c != '\n') {
                        chars.next();
                    }
                    continue;
                }
                _ => break,
            }
            chars.next();
        }
        if chars.peek().is_none() {
            return Ok(vars);
        }
        let at = line;
        let mut word = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            word.push(c);
            chars.next();
        }
        if word == "export" {
            while chars.peek().is_some_and(|&c| c == ' ' || c == '\t') {
                chars.next();
            }
            word.clear();
            while let Some(&c) = chars.peek() {
                if c == '=' || c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
        }
        if !is_name(&word) {
            return Err((at, "does not start with a variable name"));
        }
        if chars.next() != Some('=') {
            return Err((at, "has no `=` after its name"));
        }
        let value = value(&mut chars, &mut line).map_err(|why| (at, why))?;
        // The statement ends at the end of its line, a `;` or a comment.
        while chars
            .peek()
            .is_some_and(|&c| c == ' ' || c == '\t' || c == '\r')
        {
            chars.next();
        }
        if !matches!(chars.peek(), None | Some('\n' | ';' | '#')) {
            return Err((at, "holds more than one word after `=`"));
        }
        vars.insert(word, value);
    }
}

/// One sh word, up to unquoted whitespace, `;` or the end.
fn value(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line: &mut usize,
) -> Result<String, &'static str> {
    let mut value = String::new();
    let take = |c: char, value: &mut String, line: &mut usize| {
        if c == '\n' {
            *line += 1;
        }
        value.push(c);
    };
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || c == ';' {
            break;
        }
        chars.next();
        match c {
            '\'' => loop {
                match chars.next() {
                    None => return Err("opens a `'` it never closes"),
                    Some('\'') => break,
                    Some(c) => take(c, &mut value, line),
                }
            },
            '"' => loop {
                match chars.next() {
                    None => return Err("opens a `\"` it never closes"),
                    Some('"') => break,
                    Some('\\') => match chars.next() {
                        None => return Err("opens a `\"` it never closes"),
                        // An escaped newline joins the lines, as sh reads it.
                        Some('\n') => *line += 1,
                        Some(e @ ('$' | '`' | '"' | '\\')) => value.push(e),
                        Some(e) => {
                            value.push('\\');
                            take(e, &mut value, line);
                        }
                    },
                    Some(c) => take(c, &mut value, line),
                }
            },
            '\\' => match chars.next() {
                None => return Err("ends in a `\\`"),
                Some('\n') => *line += 1,
                Some(e) => value.push(e),
            },
            c => value.push(c),
        }
    }
    Ok(value)
}

/// A name sh takes for a variable: a letter or `_`, then letters, digits and `_`.
fn is_name(word: &str) -> bool {
    let mut cs = word.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn plain_and_exported_lines_with_comments_and_blanks() {
        let text = "# from a script\nA=1\n\n  export B='two'\nC=\"th ree\" # three\nexport D=\n";
        assert_eq!(
            parse(text).unwrap(),
            vars(&[("A", "1"), ("B", "two"), ("C", "th ree"), ("D", "")])
        );
    }

    #[test]
    fn a_value_reads_back_as_sh_quoted_it() {
        let awkward = "it's \"quoted\"\nline two $HOME `id` \\ end";
        let line = format!("export X={}\nY=after\n", crate::host::quote(awkward));
        assert_eq!(
            parse(&line).unwrap(),
            vars(&[("X", awkward), ("Y", "after")])
        );
        assert_eq!(
            parse(r#"Z="a\"b\$c\\d\n" W=x\ y"#).err(),
            Some((1, "holds more than one word after `=`"))
        );
        assert_eq!(
            parse("Z=\"a\\\"b\\$c\\\\d\\n\"\nW=x\\ y").unwrap(),
            vars(&[("Z", "a\"b$c\\d\\n"), ("W", "x y")])
        );
    }

    #[test]
    fn a_later_name_wins() {
        assert_eq!(parse("A=1\nA=2\n").unwrap(), vars(&[("A", "2")]));
    }

    #[test]
    fn what_is_not_a_variable_is_named_by_its_line_alone() {
        assert_eq!(
            parse("A=1\necho secret-value\n").err(),
            Some((2, "has no `=` after its name"))
        );
        assert_eq!(
            parse("A=1\n\nB='open\nC=2\n").err(),
            Some((3, "opens a `'` it never closes"))
        );
        assert_eq!(
            parse("1A=x").err(),
            Some((1, "does not start with a variable name"))
        );
        assert_eq!(
            parse("export\n").err(),
            Some((1, "does not start with a variable name"))
        );
        assert_eq!(
            parse("-x=1\n").err(),
            Some((1, "does not start with a variable name"))
        );
    }
}
