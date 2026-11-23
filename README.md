# rolesmith

Roles for any coding agent. A **role** does one kind of work (ship an app to the App Store, write a
Rust CLI, test an iOS UI) and carries what an agent needs for it: core memory, skills, the programs
it runs and the credentials it reads. Roles live in git **libraries** anyone can share; rolesmith
installs them into Claude Code, Codex or anything that reads skills, and keeps the loop that makes
them better: lessons recorded as they happen, distilled into the role when a person says so.

rolesmith calls no model. The agent you are working with does the writing; rolesmith hands it a
brief and checks the result.

```sh
rolesmith source add felix https://github.com/sunfmin/lore
rolesmith suggest                                  # roles that fit this project
rolesmith use app-store-releaser --into claude-code
eval "$(rolesmith env app-store-releaser)"         # its credentials, from mytokens or this host's own command
rolesmith lesson add app-store-releaser --evidence <url> < lesson.md
rolesmith distill app-store-releaser               # when you decide to fold lessons in
```

`use` writes a short managed section into the project's `CLAUDE.md` (or `AGENTS.md`); commit it
with `rolesmith.toml`. An agent on a fresh clone reads it and, without being reminded, installs or
upgrades rolesmith, runs `rolesmith sync` and reads its roles' core skills: a new machine needs no
setup of its own.

`rolesmith --help` is the manual: what each command is for, grouped by who runs it, and its topics.
Run `rolesmith <command> --help` before a command's first run; `rolesmith help composing` says how to
put several roles together for one agent, `rolesmith help all` prints everything. The help is written
with [agent-help](https://github.com/sunfmin/agent-cli/tree/main/crates/agent-help), as Markdown on
the code each fact lives in.

Install: `brew install sunfmin/tap/rolesmith`. The binary is universal (Apple silicon and Intel),
Developer ID signed and notarized ([ADR 0002](docs/adr/0002-releases-are-universal-signed-and-notarized-by-the-taps-workflow.md)).

Status: being built. The contract is [SPEC.md](SPEC.md); the words are in
[GLOSSARY.md](GLOSSARY.md); the decisions in [docs/adr/](docs/adr/).

License: MIT.
