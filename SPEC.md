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
became in `core.md` or a skill, its origin the distilling pull request.

Sources live in `$XDG_CONFIG_HOME/rolesmith/sources.toml` (default `~/.config/rolesmith/`); each
source's clone lives in `$XDG_DATA_HOME/rolesmith/sources/<name>/` (default
`~/.local/share/rolesmith/`). A role is addressed as `<role>` when one source has it and as
`<source>/<role>` otherwise; an ambiguous bare name is refused and the refusal lists the choices.

```toml
# sources.toml
[source.felix]
url = "https://github.com/sunfminforge/lore"
```

A source name follows the role name's rule. A source's clone is of its library's `main`. `list` and
`show` read roles from a commit of the clone (its local `main`, or `show --rev`), never from its
working tree, so they print what an install at that commit writes; `show` refuses a role that
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

[match]                                 # optional: what makes a project one this role fits (§5.5)
files = ["*.xcodeproj", "App/project.yml"]    # globs, relative to the project root
contains = [{ file = "Package.swift", text = ".iOS(" }]

[guild]                                 # optional: Guild's own keys; rolesmith keeps and ignores them
engine = { preferred = "claude-code", fallback = ["codex"] }
```

Unknown top-level keys are refused by `rolesmith check`, except tables named by another consumer
(`[guild]`), which rolesmith carries untouched. The list of such consumer tables is in rolesmith's
code, one place.

### 3.2 core.md

Markdown, at most 8,000 characters. It is the role's standing memory: rules that hold for every task
of the role. Anything that applies only to some tasks belongs in a skill.

### 3.3 prohibitions.toml

```toml
[[prohibition]]
rule = "Never revoke a signing certificate"
why = "Revoking it breaks every other machine that signs with it"
```

Installed with the core (§5), rendered as a list under the core memory. An absent file means no prohibitions.

### 3.4 Skills

`skills/<skill>/SKILL.md` with YAML front matter `name` (equal to the directory name) and
`description` (required, at most 1,024 characters, saying when to use it). Other files beside
`SKILL.md` (scripts, references) are copied with it. A skill name follows the role name's rule.

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

`rolesmith tool install` leaves a tool whose `check` passes alone. Otherwise it installs
`<formula>@<version>` where brew offers that formula, else `<formula>` when `brew info` reports the
pinned version as the formula's stable one, and refuses before installing anything when it reports
another. After installing, the tool's `check` must pass, or the install is refused. It runs only
`brew`, as the user, into brew's own prefix: never `sudo`, never a system directory. An install
method other than `brew` (`cargo`, `uv tool`, `npm -g`, a release tarball) is added by an ADR when a
tool needs it. `env` maps each environment variable to the field of the mytokens entry
it is filled from; a variable mapped to `""` is filled from the entry's single value.

## 4. Projects

A project that uses roles says so in **`rolesmith.toml`** at its root, committed:

```toml
[[role]]
name = "app-store-releaser"
source = "felix"
url = "https://github.com/sunfminforge/lore"     # so a fresh clone can add the source
rev = "3f2c9e1d..."                              # the library commit, always a full SHA
into = ["claude-code"]                           # the targets it is installed into (§5.2)
```

This file is the one home of which roles a project uses, at which revision, into which agents.
Everything rolesmith writes into a project besides it is **generated** from it: listed in
`.rolesmith/manifest.json`, ignored by git (rolesmith adds the lines to `.gitignore`), rewritten
identically by `rolesmith sync`. rolesmith deletes only files its manifest lists, and refuses to
overwrite a file its manifest does not list.

rolesmith never edits a project's `CLAUDE.md`, `AGENTS.md`, code, build files or settings.

## 5. Using a role

### 5.1 What an install writes

For each target, a role becomes skills in the directory that agent reads:

- one skill per skill of the role, named `rolesmith-<role>-<skill>`, its files copied as they are
  (its `SKILL.md`'s front matter `name` set to that name);
- one **core skill**, `rolesmith-<role>`, whose `SKILL.md` is `core.md` followed by the
  prohibitions, with the description "Always in force in this project: <role description>".

Every generated `SKILL.md` carries a comment on the first line of its front matter naming the source,
role and rev it came from and saying it is generated. (On the file's first line it would hide the
front matter: agents read front matter only from a first line `---`.)

### 5.2 Targets

| `--into` | project install | `--global` install |
| --- | --- | --- |
| `claude-code` | `.claude/skills/` | `~/.claude/skills/` |
| `codex` | the project skill directory Codex reads | Codex's user skill directory |
| `agents` | `.agents/skills/` | `~/.agents/skills/` (what `npx skills` and Orca's agents read) |

The directory each target reads is checked against that agent's documentation when the target is
built and recorded in one table in the code. An unknown target is refused.

A `--global` install writes nothing into any project; it is recorded in
`$XDG_DATA_HOME/rolesmith/global.toml` with the same fields as `rolesmith.toml`, and its manifest
beside it (`global.manifest.json`, its paths relative to `HOME`). It adds no `.gitignore` lines and
no entry in `projects.toml`. `use`, `sync`, `status`, `update` and `remove` take `--global` in place
of `--project`, and their `--json` document's `project` is then `null`; `use --auto` matches a
project, so it does not take `--global`.

### 5.3 Prompt

`rolesmith prompt <role> [--rev <sha>]` prints the role as one Markdown document for an agent that
takes instructions but reads no skills, such as a bot's standing instructions: a first-line comment
naming the source, role and rev it came from, then `core.md`, then the prohibitions, then each skill's
name, description and the path of its `SKILL.md` in the source at that rev. It writes nothing. With
`--json` it prints `{ "role": "<source>/<role>", "rev", "markdown" }`.

### 5.4 Updates

`rolesmith update [<role>]` moves a project's role (default: every role) to its source's local
`main`, rewrites `rolesmith.toml`'s `rev`, and syncs; it does not fetch, so `rolesmith source update`
comes first. `rolesmith status` says, per role, the pinned rev, the source's local `main`, how many
commits the rev is behind and ahead of it, and the generated files that differ from what the record
generates or are missing (drift, which `sync` repairs). Like `sync`, it adds a source the machine
lacks from the record's `url`.

`rolesmith remove <role>` drops the role from `rolesmith.toml` and syncs, so only files its manifest
lists are deleted. A record left with no roles is deleted, and with it the manifest and the
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
program's `[credential]` names has no mapping to variables: `env` refuses the role, listing each such
service (`error.unmapped`), and reads nothing. `rolesmith env <role> -- <command> [args]` runs the
command in rolesmith's place with them added to its environment instead of printing. Neither writes
anything to disk. `rolesmith doctor` reads only whether each entry exists (`mytokens list`), never its value.

### 7.2 Keeping secrets out

`rolesmith check` and `rolesmith lesson add` refuse text matching any of the patterns in one table in
the code (private key blocks, `ghp_`/`github_pat_`, `sk-`, AWS access keys, JWTs, long base64 or hex
runs after `key`/`token`/`secret`/`password`), naming the file, line and pattern, never echoing the
match.

## 8. Commands

Every command is non-interactive, takes `--json` (one JSON document on stdout; human text
otherwise), writes diagnostics to stderr, and exits 0 on success, 1 when it refused or something
failed, 2 on a usage error. `rolesmith agent-context` prints every command, flag and JSON shape as
one JSON document, generated from the same definitions the parser uses.

- `--help` and `--version` print on stdout and exit 0.
- With `--json`, a run that exits 1 or 2 still prints one JSON document on stdout,
  `{"error": {"kind": "failed" | "usage", "message": "..."}}`, besides the diagnostic on stderr. A refusal that is a list of problems (`check`) adds `"problems": [{"file", "line"?, "rule", "reason"}]` to `error`. A refusal that is what this machine lacks (`doctor`) adds `"missing": [{"kind": "program" | "entry", "name", "reason", "fix"}]`, `fix` being the command that fixes it.
- `agent-context` lists the global flags, then each command with its `about`, flags, positionals,
  subcommands and `output`: the JSON Schema of what its `--json` prints on success. Its human form
  is the same document, pretty-printed.
- Each command is one module of `src/commands/`, registered on one line of its `registry!`; the
  parser, the dispatch and `agent-context` are generated from that list.

| Command | What it does |
| --- | --- |
| `source add <name> <git-url>` | clone the library, record it; refuse a name in use or a repo without `library.toml` |
| `source list` | each source: name, URL, local `main` commit, roles count |
| `source update [<name>]` | fetch and fast-forward each source's clone; never force |
| `source remove <name>` | forget it and delete its clone; refuse while a project or global install records it, listing them |
| `list [--source <name>]` | roles: address, description |
| `show <role> [--rev <sha>]` | role.toml, core, prohibitions, skills with descriptions, programs, services |
| `check [<role>] [--library <dir>]` | validate a library working copy (default: the current directory) or one role (§2, §3, §7.2); prints every problem, not the first, each with its file, line where there is one, and rule; exits 1 if there is any, and with `--json` lists them as `error.problems` |
| `use <role>... --into <target>... [--project <dir> \| --global] [--rev <sha>]` | install (§5); record in `rolesmith.toml` or `global.toml`; idempotent: a role already recorded keeps its rev unless `--rev` is given (`update` moves it) |
| `use --auto --into <target>... [--project <dir>]` | install what `suggest` lists |
| `sync [--project <dir> \| --global]` | make the generated files equal what the record says; add missing sources from `url` |
| `status [--project <dir> \| --global]` | §5.4 |
| `update [<role>] [--project <dir> \| --global]` | §5.4 |
| `remove <role> [--project <dir> \| --global]` | drop it from the record, delete its generated files |
| `prompt <role> [--rev <sha>]` | §5.3 |
| `suggest [--dir <dir>]` | §5.5 |
| `doctor <role>` | for each program: tool.toml present (a role without one is refused as `show` refuses it), `check` passes (run by `sh -c`, exit 0); for each service and credential: mytokens entry present, read by `mytokens list` alone. Exit 1 if anything is missing, each with the command that fixes it (`rolesmith tool install <source>/<role>`, `mytokens put <entry> [--fields ...]`) |
| `tool show <tool>` | the tool.toml on its source's `main`, and whether its check passes here; a tool is addressed as a role is (§2) |
| `tool install <role> \| --tool <tool>` | install each program whose check fails by its pinned method into the user's own prefix (§3.5), in order; the first refusal stops it and names what was installed before it |
| `env <role> [-- <command>...]` | §7.1 |
| `new <role> --need "<text>" [--source <name>]` | scaffold `roles/<role>/` (`role.toml` with the need as its draft description, empty `core.md` and `prohibitions.toml`, no skills) in the source's working copy on a new branch `rolesmith/new-<role>` from `main`, and print the drafting brief; `--source` may be left out when there is one source; refuse a working copy with uncommitted changes, a role `main` has, and a branch that exists |
| `propose [<role>] [--source <name>] [--closes <n>...]` | run `check` (of the role, or the whole working copy) and refuse with its problems if it fails; commit the working copy's changes on its current `rolesmith/*` branch (refuse on `main` or any other branch, and when the branch does not differ from `main`); push that branch to the source's `origin`, never `main`, never forced; open a pull request onto `main` with `gh pr create`, its body listing what changed per role (and outside any role) and a `Closes #<n>` line per `--closes`; print its URL. `--source` may be left out when there is one source |
| `lesson add <role> --evidence <url> [--source felix\|agent]` | §6; the lesson text on stdin |
| `lesson list [<role>]` | open lessons: number, role, title, age |
| `lesson close [<source>#]<n> --why "<text>"` | close a lesson that will not be absorbed |
| `distill <role>` | ask `gh` for the role's open lessons (`lesson` and `role:<role>`); with none, say so and change nothing (`branch` and `brief` null); otherwise refuse a working copy with uncommitted changes, put it on a new branch `rolesmith/distill-<role>` from `main` (refuse one that exists), and print the distilling brief: the role's current `core.md`, prohibitions and skills, each open lesson (number, title, body, evidence), where each belongs (the narrowest place; core only for rules of every task), `lesson close` for one judged wrong, and the last step `check`, then `propose --closes <n>...` |
| `agent-context` | §8 |

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
