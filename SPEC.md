# rolesmith

rolesmith is a command-line tool that keeps **roles** for coding agents in git, installs them into
any agent (Claude Code, Codex, an Orca bot, or anything that reads skills or a prompt), and keeps
the loop by which roles get better: **lessons** recorded as they happen, **distilled** into the role
when a person says so.

A role does one kind of work: ship an app to the App Store, write a Rust CLI, test an iOS UI. It
carries what an agent needs for that work: its core memory, its skills, the programs it runs and the
services it may reach.

This spec is the contract. The words are defined in [GLOSSARY.md](GLOSSARY.md); the decisions, with
why, are in [docs/adr/](docs/adr/).

## 1. What rolesmith is and is not

1. **It never calls a model.** Everything rolesmith does is deterministic: store, install, check,
   record, propose. Work that needs judgment (drafting a role, distilling lessons into it) is done by
   the agent that runs rolesmith; rolesmith hands that agent a **brief** and checks what it wrote.
   This is what lets any agent use it.
2. **People schedule the work.** No daemon, no timer, no queue. A role is drafted when someone runs
   `rolesmith new`; lessons are distilled when someone runs `rolesmith distill`. Every change to a
   library lands as a pull request a person merges.
3. **It never holds a secret.** A role names the credentials it needs by their **mytokens** entry;
   rolesmith reads a value only to hand it to a process (`rolesmith env`), and never writes one to
   disk, to a library, to a project or to its own output files.
4. **It does not run agents, place work on machines or count quotas.** That is Guild's (sunfmin/guild),
   which becomes one consumer of rolesmith (§9).

## 2. Libraries and sources

A **library** is a git repository of roles. It may be public or private. A machine knows libraries
as **sources**: a local name for a library's URL.

```
<library>/
  library.toml                 # name = "felix", description, rolesmith = "<min version>"
  roles/<role>/
    role.toml                  # §3
    core.md                    # the role's core memory: always in force while the role is in use
    prohibitions.toml          # what the role must never do (§3.3)
    skills/<skill>/SKILL.md    # loaded on demand (§3.4)
  tools/<tool>/tool.toml       # how a program a role runs is installed and checked (§3.5)
```

Nothing else belongs in a library. In particular, **lessons are not files** (§6): a lesson waiting
to be distilled is an issue on the library's GitHub repository, and a distilled lesson is the text it
became in `core.md` or a skill, its origin the distilling pull request. Nor is a skill that lives in
another repository copied in: a role names it as an **external skill** (§3.4), by URL, path and
commit, and rolesmith fetches it into its own cache, `$XDG_DATA_HOME/rolesmith/skills/`.

Sources live in `$XDG_CONFIG_HOME/rolesmith/sources.toml` (default `~/.config/rolesmith/`); each
source's clone lives in `$XDG_DATA_HOME/rolesmith/sources/<name>/` (default
`~/.local/share/rolesmith/`). A role is addressed as `<role>` when one source has it and as
`<source>/<role>` otherwise; an ambiguous bare name is refused and the refusal lists the choices.

```toml
# sources.toml
[source.felix]
url = "https://github.com/sunfmin/lore"
```

A source name follows the role name's rule. A source's clone is of its library's `main`. `list` and
`show` read roles from a commit of the clone (its local `main`, or `show --rev`), never from its
working tree, so they print what an install at that commit writes. A `--rev` (of `show`, `prompt`,
`use`) is any name git resolves to a commit in the clone: a SHA or a prefix of one, a branch such as
`main`, a tag, `HEAD`; a name the clone does not resolve is refused, naming it and the source. What
is answered and recorded (`rolesmith.toml`, `global.toml`, the manifest, generated headers) is always
the full SHA it resolved to, never the name. `show` refuses a role that
`check` would refuse, with the same problems. `source update` fetches and fast-forwards the local
`main` (on whichever branch the working copy is); a `main` that moved apart from the library's is
refused, never forced.

A role name is lowercase ASCII letters, digits and single hyphens, 2–48 characters, unique within its
library.

## 3. A role

### 3.1 role.toml

```toml
description = "Ships iOS and macOS apps to the App Store"   # required, one line, <= 200 chars
programs = ["asc", "xcodegen"]          # tools of tools/ in the same library, by name
services = ["asc-api-key"]              # mytokens entries the role's work reads (§7)
requires = ["coder"]                    # roles of the same library this one builds on (§5.1)

[[skill]]                               # optional, repeated: an external skill (§3.4)
name = "codebase-design"
url = "https://github.com/mattpocock/skills"
path = "skills/engineering/codebase-design"
rev = "<full sha>"

[match]                                 # optional: what makes a project one this role fits (§5.5)
files = ["*.xcodeproj", "App/project.yml"]    # globs, relative to the project root
contains = [{ file = "Package.swift", text = ".iOS(" }]

[guild]                                 # optional: Guild's own keys; rolesmith keeps and ignores them
engine = { preferred = "claude-code", fallback = ["codex"] }
```

`requires` names, by their names, the roles of the same library whose work this role's includes:
an iOS app coder is also a coder. Installing a role installs every role it requires, transitively
(§5.1). `rolesmith check` refuses a name that is not a role of the library and a cycle (a role that
requires itself, directly or through others), naming it; checking one role checks the roles it
requires. What a role builds on is a fact about the role, so it lives here and nowhere else (ADR
0003).

Unknown top-level keys are refused by `rolesmith check`, except tables named by another consumer
(`[guild]`), which rolesmith carries untouched. The list of such consumer tables is in rolesmith's
code, one place; the same list names a prohibition's consumer sub-tables (§3.3).

### 3.2 core.md

Markdown, at most 8,000 characters. It is the role's standing memory: rules that hold for every task
of the role. Anything that applies only to some tasks belongs in a skill.

### 3.3 prohibitions.toml

```toml
[[prohibition]]
rule = "Never revoke a signing certificate"
why = "Revoking it breaks every other machine that signs with it"

[prohibition.guild]                     # optional: Guild's own keys; rolesmith keeps and ignores them
id = "no-revoke"
guards = "signing"
```

Installed with the core (§5), rendered as a list under the core memory: `rule` and `why` only. An
absent file means no prohibitions. A prohibition takes `rule`, `why`, and sub-tables named by a
consumer from the same list as §3.1 (`[prohibition.guild]`), which rolesmith carries untouched:
`rolesmith check` keeps them and `show` shows them. Any other key or sub-table is refused.

### 3.4 Skills

`skills/<skill>/SKILL.md` with YAML front matter `name` (equal to the directory name) and
`description` (required, at most 1,024 characters, saying when to use it). Other files beside
`SKILL.md` (scripts, references) are copied with it. A skill name follows the role name's rule.

An **external skill** is a skill of another git repository that a role uses without a copy in the
library: a `[[skill]]` of `role.toml` names it (`name`, as the role calls it), the repository's
`url`, the skill directory's `path` in it, and the commit `rev`, a full SHA (40 lowercase hex
digits). rolesmith fetches the repository into `$XDG_DATA_HOME/rolesmith/skills/<repository>/`
and keeps the tree of each rev it was asked for there; a rev whose tree the cache holds is read
without git or the network. Moving `rev` is a change to the library, landed like any other edit of
the role; `update` does not chase the repository. `rolesmith check` fetches what the cache lacks
and refuses a rev that is not a full SHA or that the repository does not have, a path outside the
repository or with no `SKILL.md`, front matter without `name` or `description` (its `name` need
not be the role's), and a name the role already has for a skill of its own or an earlier
`[[skill]]`. Once checked, it is a skill of the role like the others (§5).

### 3.5 tools/<tool>/tool.toml

```toml
description = "App Store Connect CLI"
check = "asc --version"                       # exits 0 when the tool is usable
[install]
brew = "asc"                                  # a Homebrew formula, installed into the user's prefix
version = "7.0.0"                             # pinned; `rolesmith tool install` refuses unpinned
[credential]                                  # optional: what the tool needs to run
mytokens = "asc-api-key"                      # the entry
env = { ASC_KEY_ID = "Key ID", ASC_ISSUER_ID = "Issuer ID", ASC_PRIVATE_KEY_B64 = "Key (base64 .p8)" }
```

`[install]` names exactly one method, `brew` or `script`, and a pinned `version` (ADR 0005); any
other key does not parse. A `script` is a shell script rolesmith runs with `sh -c`, as the user;
pixi, mise, `uv tool`, `npm` into `~/.local`, a checked binary and Apple's components are each
written as one:

```toml
check = "xcrun metal --version"
[install]
script = "xcodebuild -downloadComponent MetalToolchain"
version = "17C48"                             # a script's check must show it on stdout
```

A `brew` tool may follow its formula's stable version instead of a pin, `version = "stable"`
(ADR 0005, amended): its version then has one home, the formula in its tap.

```toml
check = "flux --version"
[install]
brew = "sunfmin/tap/flux-toolchain"
version = "stable"                            # brew only: whatever the formula's stable is now
```

A tool's check passes when it exits 0 and, for a `script`, its stdout contains `version`: a check
that exits 0 without showing the pinned version is missing, for `doctor` and for `tool install`. For
a `brew` tool following `stable`, the check passes only when the versions brew has installed (`brew
info --json=v2 <formula>`'s `installed`) include the formula's stable one (or a rebuild of it,
`<stable>_<n>`); one installed but older is missing, its reason naming the installed and the stable
versions. Before comparing, the formula's own tap (`<user>/<repo>` of `<user>/<repo>/<name>`) is
brought up to its remote with `git pull --ff-only` in `brew --repo <tap>`, never `brew update`, and
at most once a day: a tap whose `.git/rolesmith-refreshed` was touched within 24 hours is not
fetched, and the file is touched after each pull that succeeds. A pull that fails is not an error:
the tap is compared as it is and the check's reason says so. A formula without a tap in its name,
or a tap brew has not tapped, is compared as brew has it. A pinned `brew` tool's version is held by
its formula, so its check need only pass.
`rolesmith check` refuses an `[install]` with no method, both methods, no or an empty `version`, or
a `script` with `version = "stable"` (`tool-unpinned`: rolesmith cannot see what a script
installed), and a script that names `sudo` (`tool-sudo`).

`rolesmith tool install` leaves a tool whose `check` passes alone. Otherwise, for `brew`, it
installs `<formula>@<version>` where brew offers that formula, else `<formula>` when `brew info`
reports the pinned version as the formula's stable one, and refuses before installing anything when
it reports another. A `brew` tool following `stable` is installed with `brew install <formula>` when
brew has none of it installed, and upgraded with `brew upgrade <formula>` when it has an older one.
While another brew holds the formula's lock (two `tool install`s at once), brew's install or upgrade
is tried again each second for up to 30 minutes, so the second does nothing once the first has
installed it; any other failure of brew is refused. After installing, the tool's `check` must pass,
or the install is refused. For a
`script`, it runs the script, then the check, and refuses unless the script exits 0 and the check
passes, naming the script's exit code and the last lines of its stderr. Everything runs as the
user, into the user's own prefix: never `sudo`, never a system directory.

`rolesmith doctor <role> --host <host>` and `rolesmith tool install … --host <host>` do the same on
another machine: each `check`, each `brew` command and each script runs there as
`ssh -o BatchMode=yes -- <host> sh -c '<command>'` (a password prompt is never waited on), so a
`brew` tool goes into that host's own brew and prefix. The host needs `sh` and ssh access, not
rolesmith, and its commands see the `PATH` a command run over ssh gets there. A program's fix that
`doctor` names then carries `--host <host>`. Services and credentials are still read only on this
machine, from its mytokens or the commands it maps them to (§7.3): a credential never leaves it, and whoever runs work on the host brings the values
with it. When ssh cannot reach the host (it exits 255) the command fails (`error.kind` `failed`),
naming the host, rather than listing every program as missing.

`rolesmith doctor --project <dir>` and `rolesmith tool install --project <dir>` (either with
`--host`) do the same for every role the project's `rolesmith.toml` records, each at its recorded
rev and never the source's `main`, and for every role each requires, at that same rev (§5.1): one
command for what all a project's agents need. A tool is checked and installed once however many
roles name it (the same tool of the same source with the same `tool.toml`; one pinned differently at
another rev is another tool), and an entry is looked for once. The answer is the shape a role's is,
with `project` and the `roles` read in place of `source`, `role` and `rev`, and each program, tool
and item of `error.missing` adds `from`, the roles (`<source>/<role>`) that need it; a program's fix
is then `rolesmith tool install --project <dir>`. A directory without `rolesmith.toml` is refused. A
source the record names that this machine lacks is added from its `url`, as `sync` adds it.

`env` maps each environment variable to the field of the mytokens entry it is filled from; a variable mapped to `""` is filled from the entry's single value.

## 4. Projects

A project that uses roles says so in **`rolesmith.toml`** at its root, committed:

```toml
[[role]]
name = "app-store-releaser"
source = "felix"
url = "https://github.com/sunfmin/lore"     # so a fresh clone can add the source
rev = "3f2c9e1d..."                              # the library commit, always a full SHA
into = ["claude-code"]                           # the targets it is installed into (§5.2)
```

This file is the one home of which roles a project uses, at which revision, into which agents.
Everything rolesmith writes into a project besides it is **generated** from it: listed in
`.rolesmith/manifest.json`, ignored by git (rolesmith adds the lines to `.gitignore`), rewritten
identically by `rolesmith sync`. rolesmith deletes only files its manifest lists, and refuses to
overwrite a file its manifest does not list.

An agent on a fresh clone has only `rolesmith.toml`, so the project's **entry file**, which it reads
first and which is committed, tells it what to do: `use` and `sync` write one **managed section**
into the entry file of each target the record installs into (`claude-code`: `CLAUDE.md`; `codex`
and `agents`: `AGENTS.md`, at the project's root; ADR 0005). The section lies between a fixed first
line and a fixed last line; rolesmith writes only between them and nothing outside. It is the same
in every project and names no role: it tells the agent to install rolesmith when `command -v
rolesmith` finds nothing (`brew install sunfmin/tap/rolesmith`) and otherwise upgrade it (`brew
upgrade sunfmin/tap/rolesmith`), to run `rolesmith sync` when the skills are missing or `status`
reports drift, to read each role's core skill, and to run `doctor` and `tool install` for what is
missing, reporting a missing credential to a person; and not to run `update`, since a role's rev
moves only as §5.4 says. The section is committed, not ignored by git, and is not in the manifest.

- A target's entry file that does not exist is created, unless the other entry file exists and
  imports (`@AGENTS.md`) or links (`](AGENTS.md)`) it, or is a symlink to it: then the section goes
  into the one that exists. Two targets whose entry files are one file write one section.
- A section appended to a file with text is preceded by a blank line. A section whose first line
  has no last line after it is refused, writing nothing into that file.
- An entry file no target of the record names loses the section; a record with no roles removes it
  from every entry file, and an entry file that held only the section is deleted, as `.gitignore`'s
  lines are.

Apart from that section and the `.gitignore` lines, rolesmith never edits a project's
`CLAUDE.md`, `AGENTS.md`, code, build files or settings.

## 5. Using a role

### 5.1 What an install writes

For each target, a role becomes skills in the directory that agent reads:

- one skill per skill of the role, its external skills (§3.4) among them, named
  `rolesmith-<role>-<skill>`, its files copied as they are (its `SKILL.md`'s front matter `name` set
  to that name); an external skill's files come from the cache, at its rev;
- one **core skill**, `rolesmith-<role>`, whose `SKILL.md` is `core.md` followed by the
  prohibitions, with the description "Always in force in this project: <role description>".

A role that requires others (§3.1) installs each of them too, transitively, from the same source at
the same rev, each with its own core skill and skills as above; a role required twice (a diamond, or
two installed roles requiring the same one) is installed once, and one required at two revs (or a
role name two sources would both write) is refused. The core skill of a role that requires others
opens, before `core.md`, by naming the core skills of every role it requires, transitively, so one
pointer to a role's core skill reaches them all. The record keeps only the roles a person named;
what they require is derived at each install and listed in the manifest with the rest, never written
into the record. `use` and `sync` list the roles installed only because others require them as
`required` (`name`, `source`, `rev`, `by`: the recorded roles requiring them).

Every generated `SKILL.md` carries a comment on the first line of its front matter naming the source,
role and rev it came from (for an external skill, also its path, URL and rev) and saying it is
generated. (On the file's first line it would hide the
front matter: agents read front matter only from a first line `---`.)

### 5.2 Targets

| `--into` | project install | `--global` install |
| --- | --- | --- |
| `claude-code` | `.claude/skills/` | `~/.claude/skills/` |
| `codex` | `.agents/skills/` | `~/.agents/skills/` (what Codex reads) |
| `agents` | `.agents/skills/` | `~/.agents/skills/` (what `npx skills` and Orca's agents read) |

The directory each target reads is checked against that agent's documentation when the target is
built and recorded, with the page and the date it was read, in one table in the code. Targets may
share a directory (`codex` and `agents` do); a role installed into both writes its files there once.
An unknown `--into` is a usage error (exit 2) naming the known targets; an unknown target in a
record is refused.

A `--global` install writes nothing into any project; it is recorded in
`$XDG_DATA_HOME/rolesmith/global.toml` with the same fields as `rolesmith.toml`, and its manifest
beside it (`global.manifest.json`, its paths relative to `HOME`). It adds no `.gitignore` lines and
no entry in `projects.toml`, and no managed section. `use`, `sync`, `status`, `update` and `remove` take `--global` in place
of `--project`, and their `--json` document's `project` is then `null`; `use --auto` matches a
project, so it does not take `--global`.

### 5.3 Prompt

`rolesmith prompt <role> [--rev <rev>]` prints the role as one Markdown document for an agent that
takes instructions but reads no skills, such as a bot's standing instructions: a first-line comment
naming the source, role and rev it came from, then `core.md`, then the prohibitions, then each skill's
name, description and the path of its `SKILL.md` in the source at that rev (for an external skill,
its path in its repository, the URL and the rev); for a role that requires
others, a line naming them after the comment, and after its own skills each required role in turn
(transitively, each once) under a heading `# Required role <source>/<role>: <description>`: its core,
prohibitions and skills. It writes nothing. With
`--json` it prints `{ "role": "<source>/<role>", "rev", "markdown" }`.

### 5.4 Updates

`rolesmith update [<role>]` moves a project's role (default: every role) to its source's local
`main`, rewrites `rolesmith.toml`'s `rev`, and syncs; it does not fetch, so `rolesmith source update`
comes first. `rolesmith status` says, per role, the pinned rev, the source's local `main`, how many
commits the rev is behind and ahead of it, the roles it requires (`requires`), and the generated
files of it and the roles it requires that differ from what the record generates or are missing
(drift, which `sync` repairs). The roles a role requires move with it: they are at its rev. Like `sync`, it adds a source the machine
lacks from the record's `url`.

`rolesmith remove <role>` drops the role from `rolesmith.toml` and syncs, so only files its manifest
lists are deleted, and a role another remaining role requires keeps its files. `remove` and
`update` of a role the record does not name but installs because others require it are refused,
naming those roles. A record left with no roles is deleted, and with it the manifest and the
`.gitignore` lines rolesmith added (the file itself when they were all it held): the project is as it
was before `use`.

Each project directory rolesmith syncs a role into is kept in `$XDG_DATA_HOME/rolesmith/projects.toml`
(`projects = [<absolute path>...]`), since rolesmith cannot otherwise know them; `source remove` reads
the record of each that still exists and refuses while one names the source, listing the projects
and their roles (with `--json`, `error.projects: [{"project", "roles"}]`); it refuses likewise while
`global.toml` names the source, listed as `--global` (`"project": null`).

### 5.5 Suggestions

`rolesmith suggest [--dir <dir>]` lists roles of every source whose `[match]` the project meets,
with which rule matched. `rolesmith use --auto` installs exactly what `suggest` lists, into the
targets given. Nothing is installed by matching alone.

- Roles are read from each source's local `main`; a role whose `role.toml` does not parse matches
  nothing.
- A role matches when one of its `files` globs matches a file or directory path relative to the
  project root (`*` does not cross `/`, `**` does), or one of its `contains` files (a path relative
  to the root) contains its text. The walk skips `.git`, `target`, `node_modules` and what the
  project's `.gitignore` files ignore, whether or not the project is a git repository.
- Each role is listed once, as `<source>/<role>`, with the first rule that matched (`files` before
  `contains`, in role.toml's order) and the path that met it.
- `use --auto` with nothing to install is refused, and writes nothing.

## 6. Lessons and distilling

A **lesson** is what an agent or a person learned doing a role's work that the role should know next
time: what happened, the rule that would have prevented it, and the evidence (a link).

- **A lesson is an issue** on the library's GitHub repository, labelled `lesson` and `role:<role>`.
  `rolesmith lesson add` opens it through `gh`. Anyone who can open an issue on the library can
  hand it a lesson, without push rights.
- Before opening it, rolesmith refuses text that looks like a secret (§7.2) and prints a reminder
  to write project details (customer names, private URLs other than the evidence link) in general
  terms when the library is public.
- **Distilling** is a person asking an agent to fold a role's open lessons into the role.
  `rolesmith distill <role>` prints the brief and the open lessons. The agent edits `core.md`,
  skills or prohibitions in the source's working copy, then `rolesmith check` and
  `rolesmith propose`, whose pull request says `Closes #<n>` for each lesson it absorbed. A lesson
  the agent judges wrong is closed with a comment saying why, by `rolesmith lesson close`.
- The library's repository is the one its source's URL names on github.com
  (`https://github.com/<owner>/<repo>`, `git@github.com:…`, `ssh://git@github.com/…`); a source
  whose URL is not on GitHub is refused, and `lesson list` without a role skips it, saying so.
- `lesson add` reads the lesson on stdin: its first line is the issue's title, the rest its body,
  which ends with `Evidence: <url>` and `Handed in by: felix|agent` (`--source`, default `agent`).
  The labels are created on the repository when it lacks them.
- `lesson close` takes `<n>` when the machine has one source and `<source>#<n>` otherwise (as
  `lesson list` prints them); it closes only an open issue labelled `lesson`, with the reason as
  its comment. The reason is refused like the lesson text when it looks like a secret.
- There is no other record of absorbed lessons: the closed issue and the merged pull request are it.

A private lesson goes to a private library: a person who wants one keeps a private library as a
second source.

## 7. Credentials

### 7.1 Reading

`rolesmith env <role>` prints `export NAME='value'` lines (values single-quoted for sh) for every
variable of its programs' `[credential]` (§3.5), each read from mytokens at that moment
(`mytokens get <entry> --field <field>`, or `mytokens get <entry>` for `""`), for
`eval "$(rolesmith env <role>)"`; `--json` gives them as `variables`. A service of the role that no
program's `[credential]` names is read as a mytokens **Profile** when `mytokens list` lists it with
kind `profile`: a Profile's fields are already variable names (what `mytokens env <entry>` exports),
so each field is exported under its own name, read with `mytokens get <entry> --field <NAME>`. A
service that is neither, and that this host does not map (§7.3), has no mapping to variables: `env`
refuses the role, listing each such service (`error.unmapped`), and reads nothing. A variable read
from two different (entry, field) pairs — two `[credential]`s, a `[credential]` and a Profile's
field, or either and a mapped service's printed name — is refused, each with both sources
(`error.conflicting`), and nothing is read from mytokens. `rolesmith env <role> -- <command> [args]`
runs the command in rolesmith's place with them added to its environment instead of printing.
Neither writes anything to disk. `rolesmith doctor` reads only whether each entry exists
(`mytokens list`), never its value.

### 7.2 Keeping secrets out

`rolesmith check` and `rolesmith lesson add` refuse text matching any of the patterns in one table in
the code (private key blocks, `ghp_`/`github_pat_`, `sk-`, AWS access keys, JWTs, long base64 or hex
runs after `key`/`token`/`secret`/`password`), naming the file, line and pattern, never echoing the
match.

### 7.3 Where a host reads a service

Where a service's values come from is the host's choice, never a library's (ADR 0006). A host keeps
it in **`$XDG_CONFIG_HOME/rolesmith/services.toml`** (default `~/.config/rolesmith/`), beside
`sources.toml`; a service it does not name is read from mytokens as in §7.1.

```toml
# services.toml
[service.asc-api-key]                       # the service (or [credential] entry) by its name
command = "op inject -i ~/.config/rolesmith/asc-api-key.env"
check = "op whoami"                         # optional; doctor runs it, never the command
```

`[service.<name>]` takes `command` (required) and `check` (optional); any other key does not parse.
`command` runs with `sh -c`, stdin closed, at the moment `env` reads; its stdout is the service's
variables: `NAME=value` statements, each optionally after `export`, one a line, values read as sh
reads a word (`'…'` literally and across lines, `"…"` with `\` escapes), blank lines and `#`
comments between them. `mytokens env <entry>` and `op inject` over a template of
`export NAME='{{ op://<vault>/<item>/<field> }}'` lines both print this; so may any script.

- `env` exports every variable the command prints, under its own name. A program's `[credential]`
  whose entry is a mapped service is not read from mytokens: its `env` names the variables the
  command must print, and one it does not print is refused, each with its `name` and `service`
  (`error.absent`).
- A command that cannot run or exits non-zero is refused with its exit and its stderr; a statement
  that is not a variable is refused by its line number, never its text. Nothing is exported, and
  after `--` the command is not run.
- `doctor` runs `command` only with `--probe` (§7.4). A mapped service is not looked for in
  mytokens: its `check` runs on this machine (with `--host` too, as values are read only here);
  exit 0 is present, anything else is missing (`kind` `service`). One without a `check` is answered
  `unchecked` and does not make doctor refuse. `doctor`'s answer lists the mapped services as
  `services` (`name`, `command`, `state` `checked` or `unchecked`, `asks_for`, `hint`, `probe`, and
  `from` with `--project`).
- A library never holds this setting: `rolesmith check` refuses a `services.toml` at a library's root
  and a `[service]` table in `library.toml`, `role.toml` or `tool.toml` (`host-only`).

### 7.4 What a source asks for

A source that waits on a person (Touch ID, a password, a prompt) stalls an unattended worker. A
person learns which of theirs do before any unattended run.

- **From the mapping.** One table in the code names what a command asks for by the program its
  first word names (after `env` and `NAME=value` assignments). `doctor` answers it per mapped
  service as `asks_for` (`nothing`, `a person` with a `hint`, or `unknown`), and `env` writes the
  hint on stderr before it runs a command that asks for a person.

  | source | asks for |
  | --- | --- |
  | mytokens | nothing |
  | `op` with `OP_SERVICE_ACCOUNT_TOKEN` (in rolesmith's environment or assigned in the command) | nothing |
  | `op` without it | Touch ID or your password, on each read |
  | `sudo` | a password |
  | any other command | unknown: probe it |

- **Measured.** `rolesmith doctor --probe <role> | --project <dir> [--timeout <seconds>]` reads
  each mapped service's command once as `env` does, but in a session of its own (no controlling
  terminal) with stdin a pipe nothing writes to, for at most the bound (`--timeout`, default 20 s), and never
  prints or keeps a value. `probe.outcome` is `unattended` (finished, every variable the role's
  `[credential]`s expect printed; `seconds`), `needs a person` (not finished within the bound, then
  killed with its whole session; or failed saying it needs a terminal), `refused` (exited non-zero,
  with its stderr; or printed a line that is not a variable) or `incomplete` (`missing` names the
  expected variables it did not print). A probe may itself raise a Touch ID prompt; the prompt
  appearing is the answer. With `--probe` any outcome but `unattended` is missing.
- **Kept.** Each outcome is kept in `$XDG_DATA_HOME/rolesmith/probes.json` by service, with the
  command probed and when (`at`, UTC), written whole under a lock. Plain `doctor` runs no source and
  answers the kept outcome while the service maps to the same command; a kept `needs a person` is
  missing (`kind` `service`), naming when, with the fix (a service account token, or a source that
  needs no person). A service never probed, or probed with another command, is `unchecked`.

## 8. Commands

Every command is non-interactive, takes `--json` (one JSON document on stdout; human text
otherwise), writes diagnostics to stderr, and exits 0 on success, 1 when it refused or something
failed, 2 on a usage error. `rolesmith agent-context` prints every command, flag and JSON shape as
one JSON document, generated from the same definitions the parser uses.

- Help is written with agent-help (`sunfmin/agent-cli`, `crates/agent-help`, pinned by commit): Markdown doc
  comments on the type that is each fact's one home (a command's `Args` struct or subcommand
  variant, a refusal kind's enum variant, a topic's struct over a file in `src/help/`), assembled
  into one manual. `--help` and `-h` print its index (the tool's text, each command's line under
  its group, each topic's line); `<command> --help` that command's page; `help <topic>` a topic;
  `help all` the whole manual. All print on stdout and exit 0, as `--version` does; an unknown
  topic is a usage error naming the topics. A command's page says when to run it, what it reads
  and writes, what it refuses, and every key its `--json` prints, rendered from its output shape.
  Codes and numbers (exit codes, `error.kind`, `check`'s rules, targets) are written once in code
  and placed by placeholder. Help names no ADR, SPEC section or repository path. A test runs
  `agent_help::check`, so a command without text fails it.
- The topics: `composing` (how to put roles together for one agent: requires or use, one project
  per combination, naming it, its entry file, changing it, new versions; `use`'s page points to
  it), `errors` (exit codes, stderr, the `--json` error object and its lists, `check`'s rules) and
  `sources` (libraries, sources, addressing, revs, external skills).
- With `--json`, a run that exits 1 or 2 still prints one JSON document on stdout,
  `{"error": {"kind": "failed" | "usage", "message": "..."}}`, besides the diagnostic on stderr. A refusal that is a list of problems (`check`) adds `"problems": [{"file", "line"?, "rule", "reason"}]` to `error`. A refusal that is what this machine lacks (`doctor`) adds `"missing": [{"kind": "program" | "entry" | "service", "name", "reason", "fix"}]`, `fix` being the command that fixes it.
- `agent-context` lists the global flags, then each command with its `about`, `long_help`, flags,
  positionals, subcommands and `output`: the JSON Schema of what its `--json` prints on success. Its
  `about` is the first line of the command's doc comment and its `long_help` the page its `--help`
  prints, so the manual is their one home. Its human form is the same document, pretty-printed.
- Each command is one module of `src/commands/`, registered on one line of its `registry!`, with
  its group (who runs it) beside its name; the parser, the dispatch, the manual and
  `agent-context` are generated from that list.

| Command | What it does |
| --- | --- |
| `source add <name> <git-url>` | clone the library, record it; refuse a name in use or a repo without `library.toml` |
| `source list` | each source: name, URL, local `main` commit, roles count |
| `source update [<name>]` | fetch and fast-forward each source's clone; never force |
| `source remove <name>` | forget it and delete its clone; refuse while a project or global install records it, listing them |
| `list [--source <name>]` | roles: address, description |
| `show <role> [--rev <rev>]` | role.toml, core, prohibitions, skills with descriptions (an external skill with its `url`, `path` and `rev`), programs, services |
| `check [<role>] [--library <dir>]` | validate a library working copy or one role; the library is the nearest of `--library` (default: the current directory) and its parents holding `library.toml`, as git finds its repository, so run in a role or a skill it checks the whole library as from the root, and outside any library it refuses as a directory with no `library.toml`, naming the directory it started from (§2, §3, §7.2); prints every problem, not the first, each with its file, line where there is one, and rule; exits 1 if there is any, and with `--json` lists them as `error.problems` |
| `use <role>... --into <target>... [--project <dir> \| --global] [--rev <rev>]` | install (§5); record in `rolesmith.toml` or `global.toml`, always with the full SHA `--rev` resolved to; idempotent: a role already recorded keeps its rev unless `--rev` is given (`update` moves it) |
| `use --auto --into <target>... [--project <dir>]` | install what `suggest` lists |
| `sync [--project <dir> \| --global]` | make the generated files equal what the record says; add missing sources from `url` |
| `status [--project <dir> \| --global]` | §5.4 |
| `update [<role>] [--project <dir> \| --global]` | §5.4 |
| `remove <role> [--project <dir> \| --global]` | drop it from the record, delete its generated files |
| `prompt <role> [--rev <rev>]` | §5.3 |
| `suggest [--dir <dir>]` | §5.5 |
| `doctor <role> \| --project <dir> [--host <host>]` | for each program: tool.toml present (a role without one is refused as `show` refuses it), `check` passes (run by `sh -c`, exit 0); for each service and credential: mytokens entry present, read by `mytokens list` alone, or, for a service the host maps (§7.3), its `check` passes (without one it is `unchecked`, not missing). Exit 1 if anything is missing, each with the command that fixes it (`rolesmith tool install <source>/<role> [--host <host>]`, `mytokens put <entry> [--fields ...]`); with `--host` the checks run on that host over ssh, the entries are still read here; with `--project` every role the record names and each role it requires, at its recorded rev, each tool once (§3.5) |
| `tool show <tool>` | the tool.toml on its source's `main`, and whether its check passes here; a tool is addressed as a role is (§2) |
| `tool install <role> \| --tool <tool> \| --project <dir> [--host <host>]` | install each program whose check fails by its pinned method into the user's own prefix (§3.5), on that host over ssh with `--host`, of every role a project records and the roles they require with `--project`, each tool once, in order; the first refusal stops it and names what was installed before it |
| `env <role> [-- <command>...]` | §7.1 |
| `new <role> --need "<text>" [--source <name>]` | scaffold `roles/<role>/` (`role.toml` with the need as its draft description, empty `core.md` and `prohibitions.toml`, no skills) in the source's working copy on a new branch `rolesmith/new-<role>` from `main`, and print the drafting brief; `--source` may be left out when there is one source; refuse a working copy with uncommitted changes, a role `main` has, and a branch that exists |
| `propose [<role>] [--source <name>] [--closes <n>...]` | run `check` (of the role, or the whole working copy) and refuse with its problems if it fails; commit the working copy's changes on its current `rolesmith/*` branch (refuse on `main` or any other branch, and when the branch does not differ from `main`); push that branch to the source's `origin`, never `main`, never forced; open a pull request onto `main` with `gh pr create`, its body listing what changed per role (and outside any role) and a `Closes #<n>` line per `--closes`; print its URL. `--source` may be left out when there is one source |
| `lesson add <role> --evidence <url> [--source felix\|agent]` | §6; the lesson text on stdin |
| `lesson list [<role>]` | open lessons: number, role, title, age |
| `lesson close [<source>#]<n> --why "<text>"` | close a lesson that will not be absorbed |
| `distill <role>` | ask `gh` for the role's open lessons (`lesson` and `role:<role>`); with none, say so and change nothing (`branch` and `brief` null); otherwise refuse a working copy with uncommitted changes, put it on a new branch `rolesmith/distill-<role>` from `main` (refuse one that exists), and print the distilling brief: the role's current `core.md`, prohibitions and skills, each open lesson (number, title, body, evidence), where each belongs (the narrowest place; core only for rules of every task), `lesson close` for one judged wrong, and the last step `check`, then `propose --closes <n>...` |
| `agent-context` | §8 |
| `help [<topic>]` | §8: the manual's index, a topic, or `all` |

### 8.1 Briefs

`new` and `distill` print a **brief**: what the agent is to do, the files it may change, the limits
`check` enforces, the role's current text where it exists, and the last step (`rolesmith check`, then
`rolesmith propose`). Briefs are Markdown kept as files in rolesmith's source and compiled in, one
per kind, so they are reviewed like code.

## 9. Guild

Guild keeps what is Guild's: machines, admission, quotas, the scheduler and its instances. It reads
roles through rolesmith: an instance's start runs `rolesmith use <role> --into <engine> --project
<instance directory> --rev <sha>`. Guild's role keys move under
`[guild]`. Guild's trainer crew and its scheduled distilling retire: drafting and distilling are
started by people (ADR 0001). These are Guild's changes, made in Guild after rolesmith is released.

## 10. Build and release

Rust, one binary, no runtime dependency besides `git`, `gh` (for `propose` and lessons) and
`mytokens` (for `env` and `doctor`). Tests run the real binary against temporary git repositories
and stand-ins for `gh` and `mytokens` on `PATH`. Released by factorium's cut to the Homebrew tap
`sunfmin/tap/rolesmith` (ADR 0002): `packaging/release.sh build <vX.Y.Z> <outdir>` builds a universal
binary (aarch64 + x86_64) and the formula, which installs the prebuilt binary and compiles nothing;
`packaging/release.sh cut <vX.Y.Z>` pushes the tag and runs the tap's release workflow, which signs
it with Developer ID, notarizes it and publishes it to the tap's release `rolesmith-v<X.Y.Z>` with the
formula. rolesmith holds no signing secret. `--version` says the release's version (build.rs,
`ROLESMITH_RELEASE`); any other build says the development form `X.Y.Z+<commits>.g<commit>[.dirty]`.
