# 0004. A role uses skills of other repositories, pinned by commit

Status: accepted, 2026-11-23 (Felix, in the localcontrol session; rolesmith#30)

## Context

A general coding role wants skills that live elsewhere and are kept there: `codebase-design`, `tdd`,
`diagnosing-bugs`, `domain-modeling` and `writing-for-agents` from mattpocock/skills;
`complexity-review` and `single-source-of-truth` from sunfmin/product-team. The contract then let nothing
but roles and tools into a library, so they could only be copied in, and a copy drifts from
upstream.

## Decision

A role references an external skill by URL, path and pinned commit, and does not copy it:

```toml
[[skill]]
name = "codebase-design"
url = "https://github.com/mattpocock/skills"
path = "skills/engineering/codebase-design"
rev = "<full sha>"
```

Invariant: **an external skill's text has one home, its repository; the library holds only the
pin.**

- rolesmith fetches the repository into its own cache, `$XDG_DATA_HOME/rolesmith/skills/`, and
  keeps the tree of each pinned rev there. A cached rev is read without the network, so installs
  are byte-identical across runs.
- It installs as `rolesmith-<role>-<name>`, like the role's own skills: the files beside `SKILL.md`
  copied, front matter `name` set, the generated comment naming the URL and rev.
- `rev` is a full SHA. Moving it is a change to the library, a pull request like any other edit of
  the role; `rolesmith update` does not chase the upstream repository.
- `check` refuses a short or unknown rev, a path with no `SKILL.md` or with bad front matter, and a
  name that clashes with another skill of the role. `show` and `prompt` list it with its URL and rev.
- A role required through `requires` (ADR 0003) brings its external skills with it.

Options were a pinned reference, or a copy in the library with its source and license noted. A
pinned reference was chosen because a copy is a second home of the same text and drifts; the pin
keeps every update explicit.

## Consequences

- Undo: copy the skills into the role and drop `[[skill]]`.
- `check`, and an install on a machine whose cache lacks the rev, need to reach the repository.
- rolesmith's secret check (ADR 0007) covers the library's text, not an external skill's; what
  the pin names is reviewed in its repository.

Issue: rolesmith#30.
