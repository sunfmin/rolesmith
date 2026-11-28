# 0005. A tool installs by brew or by a pinned script

Status: accepted, 2026-11-23 (control session, for Felix). Amended the same day: a `brew` tool may
follow its formula's stable version.

## Context

rolesmith knew one install method, `brew`, and promised an ADR for each other one. A standard
(non-admin) Mac account installs by several: its own `~/homebrew`, pixi, mise, `uv tool`, `npm` into
`~/.local`, a checked binary, Apple's components through `xcodebuild`. One ADR and one block of code
each would grow without end, and each would only run a line of shell.

## Decision

`[install]` names one of `brew = "<formula>"` and `script = "<shell>"` (run with `sh -c`, as the
user, with no input), and a `version`.

Invariant: **a tool counts as installed only when its check shows the version it pins.** rolesmith
cannot see what a script installed, so a script's check passes only when its stdout contains
`version`. A brew tool's version is held by its formula, so its check need only pass.

`rolesmith check` refuses a `script` without `version`, `brew` and `script` together, and a script
that names `sudo` (rule `tool-sudo`). A script writes only under the user's home, and is reviewed
and landed like any other edit of a role.

A brew tool may write `version = "stable"`, for a toolchain that publishes daily: it counts as
installed only when brew has the formula's stable version installed, and `tool install` upgrades
one that is older.

Before comparing, the formula's own tap is refreshed with `git pull --ff-only`, at most once a day,
never with `brew update`: a host that sets `HOMEBREW_NO_AUTO_UPDATE` would otherwise never see a
newer stable.

`stable` is for `brew` only. A `script` must still pin; `rolesmith check` refuses `script` with
`stable` (`tool-unpinned`).

## Considered

- One ADR and one built-in method each. Rejected for one general script (Felix: any script, brew or
  mise or another tool or what Apple's system needs), guarded by the pin and the check.
- For a daily toolchain, a fixed pin bumped by a library PR at each release. Rejected: a commit a day
  and a cross-repo write credential, for a version that already has one home, the tap's formula.

## Consequences

- A new install method needs no ADR and no code: it is a script in a tool.toml, written as the
  examples in `src/help/library.md` are.
- Undo: remove `script` from `[install]` and `stable` from what a version parses as; revert this ADR.
