# Glossary

**Role**: the definition of one kind of work for an agent: a directory `roles/<role>/` of a library, with its `role.toml`, core memory, prohibitions and skills. _Avoid_: bot, persona, agent (an agent is what uses a role)

**Requires**: the roles of the same library a role builds on, named in its `role.toml`'s `requires`; installing the role installs them, transitively, at the same rev, without the project's record naming them. _Avoid_: depends, extends, inherits

**Library**: a git repository of roles and tools, with a `library.toml` at its root. Public or private. _Avoid_: registry, store

**Source**: a library as one machine knows it: a local name, its URL, and its clone under `$XDG_DATA_HOME/rolesmith/sources/`. _Avoid_: remote

**Core memory**: a role's `core.md`: rules in force for every task of the role, installed as the role's core skill.

**Skill**: `skills/<skill>/SKILL.md` of a role, loaded by an agent when its description fits the task.

**Tool**: a program a role runs, described by `tools/<tool>/tool.toml`: how to install it (pinned), how to check it, and the credential it needs.

**Service**: a credential a role's work reads, named by its mytokens entry in `role.toml`'s `services`. rolesmith holds only the name.

**Target**: an agent a role is installed into (`claude-code`, `codex`, `agents`), and the skill directory that agent reads.

**Install**: the files rolesmith generates in a target's skill directory for a role, listed in a manifest and rewritten identically by `sync`. A **project install** is recorded in the project's `rolesmith.toml`; a **global install** in `global.toml`.

**Lesson**: what doing a role's work taught that the role should know next time, with its evidence: an open issue on the library's GitHub repository labelled `lesson` and `role:<role>`. _Avoid_: memory, note

**Distilling**: folding a role's open lessons into its core memory, skills or prohibitions, by an agent a person asked, landed as one pull request that closes them.

**Brief**: the Markdown rolesmith prints for the agent that is to draft a role (`new`) or distill one (`distill`): the task, the files, the limits, the last step.
