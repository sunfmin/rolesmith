# 0005. A project's entry file carries rolesmith's managed section

Status: accepted, 2026-11-23 (the control session, for Felix; rolesmith#37)

## Context

A project's generated skills are ignored by git (SPEC §4), so a fresh clone holds only
`rolesmith.toml`. An agent started there does not know to run `rolesmith sync`, or that rolesmith
exists, and its roles are not in force until a person reminds it. SPEC §4 forbade rolesmith to edit
`CLAUDE.md` or `AGENTS.md`, the files an agent reads first.

## Decision

`use` and `sync` write one managed section into the entry file of each target the record installs
into: `CLAUDE.md` for `claude-code`, `AGENTS.md` for `codex` and `agents`. The section lies between
fixed first and last lines; rolesmith writes nothing outside them. It is committed with the project.

Invariant: **which roles a project uses, at which rev, is written once, in `rolesmith.toml`; the
section names no role and is the same in every project.** So `update` never changes it, and it can
never disagree with the record.

- The section tells the agent, before any task: install rolesmith if `command -v rolesmith` finds
  nothing, else `brew upgrade` it; `rolesmith sync` when the skills are missing or `status` reports
  drift; read each role's core skill; `doctor`, and `tool install` for a missing program, a missing
  credential reported to a person. It forbids `rolesmith update` during a task: a role's rev moves
  only in a commit of its own, never inside another change (SPEC §5.4). The section runs at the
  start of every session, so an agent allowed to `update` there would move the rev inside an
  unrelated task's commit, and change its own instructions in the middle of that task. Only the
  rolesmith program is upgraded.
- An entry file that does not exist is created, unless the other one exists and imports or links it
  (or is a symlink to it): then the section goes there, so a project keeps one file.
- `remove` of the last role takes the section out, and deletes an entry file that held only it.

Options were a user-level `SessionStart` hook that syncs and tells the agent (`rolesmith setup
claude-code`, once per machine and per agent), committing the generated skills, or the managed
section. The section was chosen because it arrives with the clone, so a new machine needs no setup
and every agent that reads an entry file gets it; committing generated skills would give them a
second home beside the record.

## Consequences

- Undo: revert this ADR's change, so `sync` no longer writes the section, and delete it from each
  project's entry file.
- An agent upgrades rolesmith at the start of each session; brew's auto-update interval bounds how
  often that reads the network.
- The section's text is rolesmith's; changing it changes every project's entry file at its next
  `sync`, as a diff the project commits.
- rolesmith#55 rewrote the reason the section gives for forbidding `update`, to the commit-of-its-own
  invariant (a library's cut may make that commit): every project's section changes at its next
  `sync`, as a diff the project commits.

Issue: rolesmith#37.
