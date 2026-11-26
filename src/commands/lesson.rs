//! `rolesmith lesson add|list|close`: lessons as issues on a library's GitHub repository, labelled
//! `lesson` and `role:<role>`, opened and closed through `gh`.
///
/// Each subcommand's `rolesmith lesson <subcommand> --help` says when to run it and what it answers.
use std::io::Read;

use serde_json::{Value, json};

use super::{Command, Failure, Output};
use crate::github::Repo;
use crate::secrets;
use crate::sources::{self, Sources};

/// The label every lesson carries.
const LESSON: &str = "lesson";
/// The colour the labels rolesmith creates are given.
const LABEL_COLOR: &str = "5319e7";

/// Hand a role a lesson, list the open ones, close one that will not be absorbed.
///
/// A lesson is what doing a role's work taught that the role should know next time: what happened, the rule that would have prevented it, and the evidence. It is an issue on the library's GitHub repository, labelled `lesson` and `role:<role>`, opened and closed through `gh`; `rolesmith distill` folds open lessons into the role. A source whose URL is not on GitHub is refused.
#[derive(clap::Args)]
#[command(verbatim_doc_comment)]
pub struct Args {
    #[command(subcommand)]
    pub action: Action,
}

#[derive(clap::Subcommand)]
pub enum Action {
    /// Open a lesson on the role's library; its text comes on stdin, the first line its title.
    ///
    /// The rest of the text is the issue's body, which ends with `Evidence: <url>` and `Handed in by: <source>`. The labels are created on the repository when it lacks them. When the library is public it reminds you to keep project details (customer names, private URLs) general. A lesson that must stay private goes to a private library, kept as a second source.
    ///
    /// ## Refusals
    ///
    /// - empty text, or text that looks like a secret (nothing is sent);
    /// - an `--evidence` that is not an `http://` or `https://` link.
    #[command(verbatim_doc_comment)]
    Add {
        /// The role: <role> when one source has it, <source>/<role> otherwise.
        role: String,
        /// The link to what shows it happened.
        #[arg(long, value_name = "URL")]
        evidence: String,
        /// Who hands the lesson in.
        #[arg(long, value_enum, default_value_t = By::Agent)]
        source: By,
    },
    /// List open lessons: number, role, title and age, of one role or of every source.
    ///
    /// Without a role, a source whose URL is not on GitHub is skipped and listed as skipped, not refused. A lesson is numbered `<n>` when the machine has one source, `<source>#<n>` otherwise, as `lesson close` takes it.
    #[command(verbatim_doc_comment)]
    List {
        /// Only this role's lessons.
        role: Option<String>,
    },
    /// Close a lesson that will not be absorbed, with the reason as its comment.
    ///
    /// Run it for a lesson judged wrong; one a change absorbs is closed by that change's pull request (`rolesmith propose --closes <n>`). It refuses an empty `--why` or one that looks like a secret, a bare `<n>` when the machine has more than one source, an issue without the `lesson` label, and one already closed.
    #[command(verbatim_doc_comment)]
    Close {
        /// The lesson: <n> when there is one source, <source>#<n> otherwise.
        lesson: String,
        /// Why it will not be absorbed.
        #[arg(long)]
        why: String,
    },
}

/// Who handed a lesson in.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum By {
    Felix,
    Agent,
}

impl By {
    fn as_str(self) -> &'static str {
        match self {
            By::Felix => "felix",
            By::Agent => "agent",
        }
    }
}

impl Command for Args {
    const NAME: &'static str = "lesson";

    fn output_shape() -> Value {
        let string = json!({ "type": "string" });
        let number = json!({ "type": "integer", "minimum": 1 });
        let array = |items: Value| json!({ "type": "array", "items": items });
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "description": "by subcommand: add prints the lesson opened; list the open lessons and the sources skipped; close the lesson closed",
            "oneOf": [
                titled("add", object(json!({
                    "opened": object(json!({ "source": string, "repo": string, "role": string,
                        "number": number, "url": string, "title": string,
                        "by": { "enum": ["felix", "agent"] } })),
                    "reminder": { "type": ["string", "null"],
                        "description": "set when the library is public: keep project details general" }
                }))),
                titled("list", object(json!({
                    "lessons": array(object(json!({ "source": string, "repo": string, "number": number,
                        "role": { "type": ["string", "null"], "description": "from its role:<role> label" },
                        "title": string, "created_at": string,
                        "age": { "type": "string", "description": "e.g. 3d, 5h, 12m" },
                        "url": string }))),
                    "skipped": array(object(json!({ "source": string, "reason": string })))
                }))),
                titled("close", object(json!({
                    "closed": object(json!({ "source": string, "repo": string, "number": number, "url": string }))
                })))
            ]
        })
    }

    fn run(self) -> Result<Output, Failure> {
        match self.action {
            Action::Add {
                role,
                evidence,
                source,
            } => {
                let mut text = String::new();
                std::io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|e| Failure::new(format!("cannot read the lesson from stdin: {e}")))?;
                add(&role, &evidence, source, &text)
            }
            Action::List { role } => list(role.as_deref()),
            Action::Close { lesson, why } => close(&lesson, &why),
        }
    }
}

/// A JSON Schema object whose every property is required.
fn object(properties: Value) -> Value {
    let required: Vec<&String> = properties
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k)
        .collect();
    json!({ "type": "object", "required": required, "properties": properties })
}

fn titled(title: &str, mut schema: Value) -> Value {
    schema["title"] = json!(title);
    schema
}

/// Refuse `text` (called `what`) if any line matches a secret pattern, naming the line and the
/// pattern, never the match.
fn refuse_secrets(what: &str, text: &str) -> Result<(), Failure> {
    let found = secrets::scan(text);
    if found.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = found
        .iter()
        .map(|(line, pattern)| format!("line {line}: {pattern}"))
        .collect();
    Err(Failure::new(format!(
        "{what} looks like it holds a secret ({}); nothing was sent. Take it out and try again",
        lines.join("; ")
    )))
}

fn add(address: &str, evidence: &str, by: By, text: &str) -> Result<Output, Failure> {
    let (title, rest) = text.trim_start().split_once('\n').unwrap_or((text, ""));
    let (title, rest) = (title.trim(), rest.trim());
    if title.is_empty() {
        return Err(Failure::new(
            "the lesson text on stdin is empty; its first line is the title",
        ));
    }
    refuse_secrets("the lesson text", text)?;
    refuse_secrets("--evidence", evidence)?;
    if !(evidence.starts_with("https://") || evidence.starts_with("http://")) {
        return Err(Failure::new(format!(
            "--evidence {evidence} is not a link (http:// or https://)"
        )));
    }

    let record = Sources::load()?;
    let found = sources::resolve(&record, address, None)?;
    let repo = Repo::of_source(&found.source, &record.get(&found.source)?.url)?;

    let visibility = repo.gh_json(&["repo", "view", "--json", "visibility"])?;
    let reminder = (visibility["visibility"] != "PRIVATE"
        && visibility["visibility"] != "INTERNAL")
        .then(|| {
            format!(
                "{repo} is public: write project details (customer names, private URLs other than \
                 the evidence link) in general terms"
            )
        });

    let role_label = format!("role:{}", found.name);
    ensure_labels(&repo, &[LESSON, &role_label])?;

    let mut body = String::new();
    if !rest.is_empty() {
        body.push_str(rest);
        body.push_str("\n\n");
    }
    body.push_str(&format!(
        "Evidence: {evidence}\nHanded in by: {}\n",
        by.as_str()
    ));
    let url = repo.gh(
        &[
            "issue",
            "create",
            "--title",
            title,
            "--body-file",
            "-",
            "--label",
            LESSON,
            "--label",
            &role_label,
        ],
        Some(&body),
    )?;
    let url = url.lines().last().unwrap_or_default().trim().to_owned();
    let number = url
        .rsplit('/')
        .next()
        .and_then(|n| n.parse::<u64>().ok())
        .ok_or_else(|| {
            Failure::new(format!(
                "gh issue create answered {url}, not an issue's URL"
            ))
        })?;

    let mut human = String::new();
    if let Some(r) = &reminder {
        human.push_str(&format!("note: {r}\n"));
    }
    human.push_str(&format!("opened lesson #{number} on {repo}: {url}\n"));
    Ok(Output {
        json: json!({
            "opened": {
                "source": found.source, "repo": repo.0, "role": found.name, "number": number,
                "url": url, "title": title, "by": by.as_str(),
            },
            "reminder": reminder,
        }),
        human,
    })
}

/// Create each label `repo` does not have yet.
fn ensure_labels(repo: &Repo, labels: &[&str]) -> Result<(), Failure> {
    let have = repo.gh_json(&["label", "list", "--json", "name", "--limit", "1000"])?;
    let have: Vec<&str> = have
        .as_array()
        .map(|a| a.iter().filter_map(|l| l["name"].as_str()).collect())
        .unwrap_or_default();
    for label in labels.iter().filter(|l| !have.contains(l)) {
        let description = if *label == LESSON {
            "A lesson for a role, waiting to be distilled (rolesmith)".to_owned()
        } else {
            format!("A lesson for the role {}", &label["role:".len()..])
        };
        repo.gh(
            &[
                "label",
                "create",
                label,
                "--color",
                LABEL_COLOR,
                "--description",
                &description,
            ],
            None,
        )?;
    }
    Ok(())
}

/// The open lessons on `repo` (only those labelled `role_label` when it is given), each with the
/// `fields` `gh issue list --json` names.
pub fn open(repo: &Repo, role_label: Option<&str>, fields: &str) -> Result<Vec<Value>, Failure> {
    let mut args = vec![
        "issue", "list", "--state", "open", "--label", LESSON, "--json", fields, "--limit", "1000",
    ];
    if let Some(label) = role_label {
        args.extend(["--label", label]);
    }
    match repo.gh_json(&args)? {
        Value::Array(issues) => Ok(issues),
        other => Err(Failure::new(format!(
            "gh issue list answered {other}, not a list of issues"
        ))),
    }
}

fn list(address: Option<&str>) -> Result<Output, Failure> {
    let record = Sources::load()?;
    // Each source to ask, with the role label to filter by.
    let asked: Vec<(String, Option<String>)> = match address {
        Some(address) => {
            let found = sources::resolve(&record, address, None)?;
            vec![(found.source, Some(format!("role:{}", found.name)))]
        }
        None => record.source.keys().map(|s| (s.clone(), None)).collect(),
    };

    let now = now_seconds();
    let (mut lessons, mut skipped) = (Vec::new(), Vec::new());
    let (mut rows, mut skips) = (String::new(), String::new());
    for (source, role_label) in asked {
        let repo = match Repo::of_source(&source, &record.get(&source)?.url) {
            Ok(repo) => repo,
            // A role asked for by name is refused; a sweep of every source skips what it cannot ask.
            Err(e) if role_label.is_some() => return Err(e),
            Err(e) => {
                skips.push_str(&format!("skipped {source}: {}\n", e.message));
                skipped.push(json!({ "source": source, "reason": e.message }));
                continue;
            }
        };
        let issues = open(
            &repo,
            role_label.as_deref(),
            "number,title,labels,createdAt,url",
        )?;
        for issue in &issues {
            let role = issue["labels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l["name"].as_str()?.strip_prefix("role:"))
                .next();
            let created = issue["createdAt"].as_str().unwrap_or_default();
            let age = parse_time(created).map_or_else(|| "?".to_owned(), |t| age(now - t));
            let (number, title) = (
                &issue["number"],
                issue["title"].as_str().unwrap_or_default(),
            );
            rows.push_str(&format!(
                "{source}#{number}\t{}\t{title}\t{age}\n",
                role.unwrap_or("(no role)")
            ));
            lessons.push(json!({
                "source": source, "repo": repo.0, "number": number, "role": role,
                "title": title, "created_at": created, "age": age, "url": issue["url"],
            }));
        }
    }

    if lessons.is_empty() {
        rows.push_str("no open lessons\n");
    }
    Ok(Output {
        json: json!({ "lessons": lessons, "skipped": skipped }),
        human: rows + &skips,
    })
}

fn close(lesson: &str, why: &str) -> Result<Output, Failure> {
    let why = why.trim();
    if why.is_empty() {
        return Err(Failure::new(
            "--why is empty; say why the lesson will not be absorbed",
        ));
    }
    refuse_secrets("--why", why)?;
    let record = Sources::load()?;
    let (source, number) = match lesson.split_once('#') {
        Some((source, n)) => (source.to_owned(), n),
        None => match record.source.keys().collect::<Vec<_>>()[..] {
            [only] => (only.clone(), lesson),
            _ => {
                return Err(Failure::new(format!(
                    "lesson {lesson}: name its source as <source>#{lesson} (sources: {})",
                    record.source.keys().cloned().collect::<Vec<_>>().join(", ")
                )));
            }
        },
    };
    let number: u64 = number
        .parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| Failure::new(format!("{lesson} is not a lesson number")))?;
    let repo = Repo::of_source(&source, &record.get(&source)?.url)?;
    let n = number.to_string();

    let issue = repo.gh_json(&["issue", "view", &n, "--json", "labels,state,url"])?;
    let is_lesson = issue["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|l| l["name"] == LESSON);
    if !is_lesson {
        return Err(Failure::new(format!(
            "{repo}#{n} is not a lesson (it has no {LESSON} label); nothing was closed"
        )));
    }
    if issue["state"] != "OPEN" {
        return Err(Failure::new(format!("lesson {repo}#{n} is already closed")));
    }
    repo.gh(&["issue", "close", &n, "--comment", why], None)?;
    let url = issue["url"].as_str().unwrap_or_default();
    Ok(Output {
        json: json!({ "closed": { "source": source, "repo": repo.0, "number": number, "url": url } }),
        human: format!("closed lesson #{n} on {repo}: {why}\n"),
    })
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Seconds since the epoch of a UTC timestamp as GitHub writes it, `YYYY-MM-DDTHH:MM:SSZ`.
fn parse_time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3_600 + mm * 60 + ss)
}

/// An age in the largest whole unit: days, hours or minutes.
fn age(seconds: i64) -> String {
    let s = seconds.max(0);
    match s {
        s if s >= 86_400 => format!("{}d", s / 86_400),
        s if s >= 3_600 => format!("{}h", s / 3_600),
        s => format!("{}m", s / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_timestamps_are_read_as_utc_seconds() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(parse_time("2026-11-22T10:30:15Z"), Some(1_795_343_415));
        assert_eq!(parse_time("2026-11-22"), None);
        assert_eq!(parse_time("2026-13-22T10:30:15Z"), None);
        assert_eq!(parse_time("2026-11-22T10:30:15X"), None);
    }

    #[test]
    fn an_age_is_in_its_largest_unit() {
        assert_eq!(age(59), "0m");
        assert_eq!(age(3_599), "59m");
        assert_eq!(age(3_600), "1h");
        assert_eq!(age(86_399), "23h");
        assert_eq!(age(3 * 86_400 + 5), "3d");
        assert_eq!(age(-5), "0m");
    }
}
