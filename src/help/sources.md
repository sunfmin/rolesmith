Where roles come from: a library is a git repository of roles, a source is this machine's name for one, and a rev is the commit a role is read and installed at.

## Libraries and sources

A library holds `library.toml`, `roles/<role>/` (`role.toml`, `core.md`, `prohibitions.toml`, `skills/<skill>/SKILL.md`) and `tools/<tool>/tool.toml`. `rolesmith source add <name> <git-url>` clones one and records it under a name; the clone is of the library's `main`. For example, `rolesmith source add felix https://github.com/sunfmin/lore` adds Felix's library. Sources are recorded in `$XDG_CONFIG_HOME/rolesmith/sources.toml` (default `~/.config/rolesmith/`), clones kept in `$XDG_DATA_HOME/rolesmith/sources/<name>/` (default `~/.local/share/rolesmith/`).

## Addressing a role

A role (or a tool) is `<role>` when one source has it and `<source>/<role>` otherwise. A bare name two sources have is refused, and the refusal lists the choices. A name, of a role, a skill or a source, is lowercase letters, digits and single hyphens, 2 to 48 characters.

## Revs

- `list`, `show`, `prompt` and `suggest` read roles from a commit of the clone, its local `main` (or `--rev <rev>`), never from its working tree: they show what an install at that commit writes.
- A `--rev` (of `show`, `prompt`, `use`) is any name git resolves to a commit in the clone: a SHA or a prefix of one, a branch such as `main`, a tag, `HEAD`. Everything answered or recorded keeps the full SHA it resolved to, never the name. A name the clone does not resolve is refused, naming it and the source.
- `use` records each role with the full SHA it installed (`rev` in `rolesmith.toml`, or `global.toml` for `--global`); `sync` installs exactly that rev again. A role already recorded keeps its rev unless `use --rev` is given.
- Nothing fetches on its own. `source update` fetches and fast-forwards each clone's local `main`, never forced; `update` then moves the recorded revs to it. `status` says how far each rev is behind and ahead of `main`.
- A record names each source's `url`, so `sync`, `status` and `update` on a fresh machine add a source the machine lacks.

## External skills

A role may use a skill of another git repository, named in its `role.toml` by URL, path and a full commit SHA. rolesmith fetches that repository into its own cache, `$XDG_DATA_HOME/rolesmith/skills/`, and reads the rev's tree from there without the network once it holds it. `update` does not move an external skill's rev: changing it is an edit of the role.
