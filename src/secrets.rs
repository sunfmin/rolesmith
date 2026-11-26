//! The one table of patterns that look like a secret. `rolesmith check` and
//! `rolesmith lesson add` refuse text matching any of them. A match is named by its pattern,
//! never echoed.

use std::sync::LazyLock;

use regex::Regex;

/// Each pattern's name (what a refusal says) and its regular expression.
const PATTERNS: &[(&str, &str)] = &[
    (
        "private key block",
        r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY( BLOCK)?-----",
    ),
    (
        "GitHub token (ghp_ and kin)",
        r"\bgh[pousr]_[A-Za-z0-9]{36,}",
    ),
    (
        "GitHub token (github_pat_)",
        r"\bgithub_pat_[A-Za-z0-9_]{22,}",
    ),
    ("API key (sk-)", r"\bsk-[A-Za-z0-9_-]{20,}"),
    ("AWS access key", r"\b(AKIA|ASIA)[0-9A-Z]{16}\b"),
    (
        "JWT",
        r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
    ),
    (
        "long base64 or hex value after key/token/secret/password",
        r#"(?i)(key|token|secret|password)[A-Za-z0-9_-]*["']?\s*[:=]\s*["']?[A-Za-z0-9+/]{32,}"#,
    ),
];

static COMPILED: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .map(|(name, re)| (*name, Regex::new(re).expect("a secret pattern compiles")))
        .collect()
});

/// The names of the patterns `text` matches, in table order.
pub fn matches(text: &str) -> Vec<&'static str> {
    COMPILED
        .iter()
        .filter(|(_, re)| re.is_match(text))
        .map(|(name, _)| *name)
        .collect()
}

/// Each line (1-based) of `text` that matches a pattern, with the pattern's name.
pub fn scan(text: &str) -> Vec<(usize, &'static str)> {
    text.lines()
        .enumerate()
        .flat_map(|(i, line)| matches(line).into_iter().map(move |name| (i + 1, name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_secret_is_caught() {
        let samples = [
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            &format!("token ghp_{}", "a".repeat(36)),
            &format!("github_pat_{}", "A1".repeat(20)),
            &format!("OPENAI=sk-{}", "x".repeat(40)),
            "AKIAIOSFODNN7EXAMPLE",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N",
            &format!("api_key = \"{}\"", "0123456789abcdef".repeat(3)),
            &format!("password: {}", "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo="),
        ];
        for s in samples {
            assert!(!matches(s).is_empty(), "not caught: {s}");
        }
    }

    #[test]
    fn ordinary_role_text_is_not_a_secret() {
        let samples = [
            "mytokens = \"asc-api-key\"",
            "ASC_PRIVATE_KEY_B64 = \"Key (base64 .p8)\"",
            "services = [\"asc-api-key\"]",
            "Never paste a token into a commit message; read it with mytokens.",
            "task-runner-for-the-very-long-hyphenated-thing-here",
            "-----BEGIN PUBLIC KEY-----",
        ];
        for s in samples {
            assert!(matches(s).is_empty(), "false alarm: {s}");
        }
    }

    #[test]
    fn scan_names_the_line() {
        let text = format!("one\ntwo ghp_{}\nthree\n", "b".repeat(36));
        assert_eq!(scan(&text), vec![(2, "GitHub token (ghp_ and kin)")]);
    }
}
