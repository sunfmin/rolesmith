What a library holds, file by file: what each key means and what an install makes of it. `rolesmith check` enforces the limits; its rules are listed in [how a run ends](help:errors).

## The layout

```text
<library>/
  library.toml                 # name = "felix", description, rolesmith = "<least version>"
  roles/<role>/
    role.toml
    core.md                    # always in force while the role is in use
    prohibitions.toml          # what the role must never do
    skills/<skill>/SKILL.md    # loaded when a task needs it
  tools/<tool>/tool.toml       # how a program a role runs is installed and checked
```

Nothing else belongs in a library. A lesson is not a file: one waiting to be distilled is an issue on the library's GitHub repository (`rolesmith lesson --help`), and a distilled one is the text it became in the role. A skill of another repository is not copied in: the role names it as an external skill (below). Nor does a library say where a host reads a service's values (`rolesmith env --help`).

## role.toml

```toml
description = "Ships iOS and macOS apps to the App Store"   # required, one line
programs = ["asc", "xcodegen"]          # tools of tools/ in the same library, by name
services = ["asc-api-key"]              # optokens Secrets the role's work reads
requires = ["coder"]                    # roles of the same library this one builds on

[[skill]]                               # optional, repeated: an external skill
name = "codebase-design"
url = "https://github.com/mattpocock/skills"
path = "skills/engineering/codebase-design"
rev = "<full sha>"

[match]                                 # optional: what makes a project one this role fits
files = ["*.xcodeproj", "App/project.yml"]    # globs, relative to the project root
contains = [{ file = "Package.swift", text = ".iOS(" }]

[guild]                                 # another consumer's own keys: kept, and ignored
engine = { preferred = "claude-code", fallback = ["codex"] }
```

- `requires` names the roles whose work this role's includes: an iOS app coder is also a coder. Installing the role installs each, transitively, from the same source at the same rev, each with its own core skill and skills. One required twice is installed once; one required at two revs is refused. The role's core skill opens by naming the core skills of every role it requires, so one pointer to it reaches them all. A project's record keeps only the roles a person named.
- A top-level key rolesmith does not know is refused, except a table of another consumer ({const:CONSUMER_TABLES}), which rolesmith carries untouched and `show` shows.
- `[match]` is what `rolesmith suggest` reads.

## core.md and prohibitions.toml

`core.md` is the role's standing memory: rules that hold for every task of the role. Anything only some tasks need belongs in a skill.

```toml
[[prohibition]]
rule = "Never revoke a signing certificate"
why = "Revoking it breaks every other machine that signs with it"

[prohibition.guild]                     # another consumer's own keys: kept, and ignored
id = "no-revoke"
```

A prohibition takes `rule`, `why`, and a sub-table of another consumer; any other key is refused. An install renders the prohibitions as a list under the core memory, `rule` and `why` only. No file means no prohibitions.

## Skills

`skills/<skill>/SKILL.md` has YAML front matter `name`, its directory's name, and `description`, saying when to use it. The files beside it are copied with it. An install writes it as the skill `rolesmith-<role>-<skill>`, its `name` set to that, with a comment on the first line of its front matter naming the source, role and rev it came from.

An external skill, a `[[skill]]` of `role.toml`, is a skill of another git repository: its `url`, the skill directory's `path` there, and `rev`, a full SHA. `name` is what the role calls it. `check` fetches it into the cache ([sources and revs](help:sources)) and refuses a rev that is not a full SHA or that the repository lacks, a path with no `SKILL.md`, front matter without `name` or `description` (its `name` need not be the role's), and a name the role already has. Once checked it installs like the role's own skills, from the cache at its rev.

## tool.toml

```toml
description = "App Store Connect CLI"
check = "asc --version"                       # exits 0 when the tool is usable
[install]
brew = "asc"                                  # a Homebrew formula, into the user's prefix
version = "7.0.0"                             # pinned
[credential]                                  # optional: what the tool needs to run
mytokens = "asc-api-key"                      # the optokens Secret
env = { ASC_KEY_ID = "Key ID", ASC_PRIVATE_KEY_B64 = "Key (base64 .p8)" }
```

`[install]` names exactly one method, and a `version`:

- `brew = "<formula>"`: the formula holds the pinned version, so the check need only pass. `version = "stable"` follows the formula's stable version instead of a pin.
- `script = "<shell>"`: run with `sh -c`, as the user, never with `sudo`. rolesmith cannot see what a script installed, so its check passes only when its stdout shows the `version`, which must be a pin, never `stable`. pixi, mise, `uv tool`, `npm` into `~/.local`, a checked binary and Apple's components are each written as one:

```toml
check = "xcrun metal --version"
[install]
script = "xcodebuild -downloadComponent MetalToolchain"
version = "17C48"
```

`rolesmith tool install --help` says how each is installed.

`[credential]`'s `mytokens` is the optokens Secret it is read from (the key's name is kept from when credentials were read from mytokens). Its `env` maps each environment variable to the field of that Secret it is read from; a variable mapped to `""` is read from the Secret's single value. `rolesmith env` exports them.
