# 0005. A tool installs by brew or by a pinned script

Status: accepted, 2026-11-23 (control session, for Felix; rolesmith#34)
Amended: 2026-11-23 (control session, for Felix; rolesmith#43): a `brew` tool may follow its
formula's stable version (see Amendment).

## Context

rolesmith knew one install method, `brew`, and said each other method (`cargo`, `uv tool`,
`npm -g`, a release tarball) would come with its own ADR. A standard (non-admin) Mac account
installs by several: its own Homebrew in `~/homebrew`, pixi for what brew would build from source,
mise for runtimes, `uv tool`, `npm` into `~/.local`, a checked binary in `~/.local/bin`, and
Apple's system components through `xcodebuild`. One ADR and one block of code per method would grow
without end, and each would only run a program the user could have named in a line of shell.

## Decision

`[install]` names exactly one of two methods, and a pinned `version`:

- `brew = "<formula>"`: as before, `<formula>@<version>` or the formula whose stable version is the
  pin, refused before anything is installed otherwise.
- `script = "<shell>"`: rolesmith runs it with `sh -c`, as the user, with no input.

Invariant: **a tool counts as installed only when its check shows the version it pins.** For a
script, rolesmith cannot see what the script installed, so the check's stdout must contain
`version`; a check that exits 0 without it is missing (doctor reports it, `tool install` runs the
script). A brew tool's version is held by its formula, so its check need only pass.

What `script` keeps:

- `tool install` leaves a tool whose check passes alone; otherwise it runs the script, then the
  check, and refuses unless both succeed, naming the script's exit code and the end of its stderr.
- `rolesmith check` refuses a `script` without `version`, `brew` and `script` together, and a
  script that names `sudo` (rule `tool-sudo`). rolesmith never runs as root; a script writes only
  under the user's home, as brew does into `~/homebrew`.
- The script is part of the library, reviewed and landed like any other edit of a role.

`script` is enough because every method is a command the user runs, and the pinned version in the
check's output is what says the right thing was installed, whatever ran. Options were one ADR and
one built-in method each, or one general script; the script was chosen (Felix: any script, brew or
mise or another tool or what Apple's system needs), guarded by the pin and the check.

## Examples

Each runs as the user, without `sudo`, writing under the home directory only.

Homebrew, in the account's own `~/homebrew`:

```toml
description = "App Store Connect CLI"
check = "asc --version"
[install]
brew = "asc"
version = "7.0.0"
```

pixi, from conda-forge, for what brew could only build from source:

```toml
description = "PostgreSQL client and server"
check = "postgres --version"
[install]
script = "pixi global install postgresql=17.2"
version = "17.2"
```

mise, for a runtime:

```toml
description = "Node.js"
check = "node --version"
[install]
script = "mise use -g node@22.11.0"
version = "22.11.0"
```

uv, for a command-line tool written in Python:

```toml
description = "Ruff, the Python linter"
check = "ruff --version"
[install]
script = "uv tool install ruff==0.8.1"
version = "0.8.1"
```

npm, into `~/.local`:

```toml
description = "Prettier"
check = "prettier --version"
[install]
script = "npm i -g --prefix ~/.local prettier@3.4.2"
version = "3.4.2"
```

A single binary, its sha checked before it goes into `~/.local/bin`:

```toml
description = "jq"
check = "jq --version"
[install]
script = '''
set -e
f=$(mktemp)
curl -fsSL -o "$f" https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-macos-arm64
echo "<sha256>  $f" | shasum -a 256 -c -
mkdir -p ~/.local/bin && install -m 755 "$f" ~/.local/bin/jq
'''
version = "1.7.1"
```

An Apple system component:

```toml
description = "Metal Toolchain (Xcode component; MLX compiles shaders with it)"
check = "xcrun metal --version"
[install]
script = "xcodebuild -downloadComponent MetalToolchain"
version = "17C48"
```

A tool built from source (`cargo install --git <repo> --rev <sha> <crate>`, pinned to the crate's
version) is written the same way.

## Amendment: a brew tool may follow its formula's stable version (rolesmith#43)

A toolchain that publishes daily (sunfmin/flux-toolchain) cannot be kept current by a pin: the pin
would be copied from the tap into the library every day, by a commit a day and a cross-repo write
credential, for the same result. So a `brew` tool may write `version = "stable"`:

- It counts as installed only when brew has the formula's stable version installed (`brew info
  --json=v2 <formula>`: `versions.stable` among `installed[].version`, or a rebuild `<stable>_<n>`).
  One installed but older is missing, for `doctor` (naming both versions) and for `tool install`,
  which runs `brew upgrade <formula>` (or `brew install <formula>` when none is installed), then
  the check.
- Before comparing, the formula's own tap is refreshed with `git pull --ff-only`, at most once a day
  (a stamp in the tap's `.git`), never with `brew update`: a host that sets
  `HOMEBREW_NO_AUTO_UPDATE` would otherwise never see a newer stable. A failed pull compares the tap
  as it is and says so.
- `stable` is for `brew` only. A `script` must still pin, since rolesmith cannot see what a script
  installed; `rolesmith check` refuses `script` with `stable` (`tool-unpinned`).

The invariant becomes: a tool counts as installed only when its check shows the version it pins,
or, for a brew tool following `stable`, when brew has its formula's stable version installed.

Options were A) a fixed pin, bumped by a lore PR that each flux-toolchain release opens and merges,
and B) `version = "stable"` for brew tools; B was chosen, because the version already has one home,
the tap's formula. Undo: revert rolesmith#43; `stable` no longer parses as anything but a version
named `stable`, and a library's tool goes back to a fixed pin.

## Consequences

- A new install method needs no ADR and no code: it is a script in a tool.toml.
- A check must print the version for a script tool; a check that prints nothing (`test -x …`)
  cannot pin one and is the wrong check.
- Undo: remove `script` from `[install]` and revert this ADR.
