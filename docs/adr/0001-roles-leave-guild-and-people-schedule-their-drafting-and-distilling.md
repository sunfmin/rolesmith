# 0001. Roles leave Guild, and people schedule their drafting and distilling

Status: accepted, 2026-11-22 (Felix, in the control session)

## Context

Guild (sunfmin/guild) keeps roles in lore (sunfminforge/lore): a directory per role with its core
memory and skills. Its trainer crew drafts roles and distills lessons, as instances Guild's scheduler
starts. A role is therefore usable only as a Guild instance, on a Mac with guildd, a Forgejo Hub and
the tailnet: not in a Claude Code session, not in Orca on a phone, not by someone who runs no Hub.

The question came from an App Store release (sunfmin/chessmirror#20): that know-how belongs to no one
repo and should reach every similar project, on any agent. Three ways were laid out: keep Guild
whole and add a way to install a role elsewhere; split roles out of Guild; or leave roles Guild-only.

Felix, 2026-11-22: 「我想做二。提炼经验和创建角色，由人来调度。这样的话，一个命令行。工具就可以被任何一个agent使用。」

## Decision

- Roles, their libraries and the loop that improves them are rolesmith's, a project of their own.
  Guild keeps machines, admission, quotas and its scheduler, and reads roles through rolesmith.
- Drafting a role and distilling lessons are started by a person, never by a timer or a scheduler.
  The agent the person is working with does the writing; rolesmith hands it a brief and checks the
  result. rolesmith calls no model, so any agent can drive it.
- Every change to a library is a pull request a person merges. There is no automatic approver.
- A lesson waiting to be distilled is an issue on the library's repository; a distilled one is the
  text it became, its origin the pull request. No lesson files are kept.
- The CLI is named `rolesmith`; `lore` alone is too short to be safe from collisions (checked
  2026-11-22: `rolesmith` is free on npm and crates.io, one GitHub repository of that name).

## Consequences

- Guild's trainer crew and its scheduled distilling retire once Guild reads roles through rolesmith.
  An instance's start then runs `rolesmith use <role> --into <engine> --project <instance dir> --rev
  <sha>`, and Guild's own role keys move under `[guild]` of role.toml: Guild's changes, made in Guild.
- A role is shared by sharing its library; whoever installs it brings their own credentials.
