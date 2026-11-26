# 0003. A role names the roles it builds on in its role.toml

Status: accepted, 2026-11-23 (Felix, in the localcontrol session; rolesmith#29)

## Context

A project could already use several roles (`rolesmith use A B`, one `[[role]]` each in
`rolesmith.toml`), but "an iOS app coder is also a coder" had no home: every project, and every
factorium role directory, had to list the general coding role (code design, TDD, diagnosing bugs…)
again beside the iOS one, and keep the two in step by hand.

## Decision

What a role builds on is declared in that role's `role.toml`: `requires = ["coder"]`, role names of
the same library. Invariant: **"X is also a Y" is written once, in X's role.toml;
records name only the roles a person chose.**

- Installing a role installs every role it requires, transitively, from the same source at the same
  rev, each with its own core skill and skills. A role required twice is installed once; one required
  at two revs is refused.
- `rolesmith.toml` and `global.toml` keep only the roles a person named. What they require is
  derived at each install and listed in the manifest, never written into the record.
- A role's core skill opens by naming the core skills of the roles it requires, so a factorium role
  directory's `CLAUDE.md` stays one line: "read `rolesmith-<role>`".
- `check` refuses an unknown required role and a cycle; `prompt`, `status`, `update` and `remove`
  account for required roles.

Options were `requires` in role.toml, or each project's (and role directory's) `rolesmith.toml`
listing every role itself, which already worked. `requires` was chosen because the fact is about the
role and belongs in one place, not repeated per project.

## Consequences

- Undo: drop `requires`; projects list the general role themselves with `rolesmith use coder <role>`.
- A role and the roles it requires move together: one rev pins them all. A project cannot hold a
  required role at another rev than the role requiring it.
- `tool install <role>` installs the programs of the roles it requires too, since they are read
  with it.

Issue: rolesmith#29.
