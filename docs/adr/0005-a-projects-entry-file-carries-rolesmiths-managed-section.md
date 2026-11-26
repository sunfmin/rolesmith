# 0005. A project's entry file carries rolesmith's managed section

Status: accepted, 2026-11-23 (the control session, for Felix)

## Context

A project's generated skills are ignored by git, so a fresh clone holds only
`rolesmith.toml`. An agent started there does not know to run `rolesmith sync`, or that rolesmith
exists, and its roles are not in force until a person reminds it. The contract then forbade rolesmith to edit
`CLAUDE.md` or `AGENTS.md`, the files an agent reads first.

## Decision

`use` and `sync` write one managed section, between fixed first and last lines, into the entry file
of each target the record installs into; what it says and where it goes, `rolesmith use --help` says.

Invariant: **which roles a project uses, at which rev, is written once, in `rolesmith.toml`; the
section names no role and is the same in every project.** So `update` never changes it, and it can
never disagree with the record.

The section forbids `rolesmith update` during a task because it runs at the start of every session:
an agent allowed to `update` there would move the rev inside an unrelated task's commit, and change
its own instructions in the middle of that task. Only the rolesmith program is upgraded.

Options were a user-level `SessionStart` hook that syncs and tells the agent (`rolesmith setup
claude-code`, once per machine and per agent), committing the generated skills, or the managed
section. The section was chosen because it arrives with the clone, so a new machine needs no setup
and every agent that reads an entry file gets it; committing generated skills would give them a
second home beside the record.

## Consequences

- An agent upgrades rolesmith at the start of each session; brew's auto-update interval bounds how
  often that reads the network.
- The section's text is rolesmith's; changing it changes every project's entry file at its next
  `sync`, as a diff the project commits.
