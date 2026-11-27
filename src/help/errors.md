How a run ends: its exit code, what it writes on stderr, and the one JSON object `--json` prints when it refuses.

## Exit codes

- {const:EXIT_OK}: it did what was asked. Its answer is on stdout: human text, or with `--json` one JSON object on one line, whose keys each command's `--help` lists.
- {const:EXIT_FAILED}: it refused, or something it needed failed (`git`, `gh`, `optokens`, `brew`, a tool's install script, a service's mapped command, a file). Nothing it refused was half done.
- {const:EXIT_USAGE}: the command line is not one rolesmith accepts: an unknown command, flag or topic, a missing argument, an unknown `--into` target.

## stderr

Every refusal is one message on stderr, `rolesmith: <message>`; a usage error is clap's message. A refusal that is a list (problems, missing things) puts each item on its own indented line after the message. Nothing else is written there.

## With `--json`

A run that exits {const:EXIT_FAILED} or {const:EXIT_USAGE} still prints one JSON object on stdout, besides the message on stderr:

```json
{"error": {"kind": "failed", "message": "..."}}
```

`kind` is one of:

{table:error-kind}

A refusal that is a list adds it under `error`, by the command that refuses so:

- `problems` (`check`, `propose`, and every command that reads a role at a commit, such as `show`): every problem found, not only the first. Each has:
  {const:PROBLEM}
- `missing` (`doctor`): every thing the role needs that this machine lacks. Each has:
  {const:MISSING}
- `unmapped` (`env`): the role's services that no program's `[credential]` maps to variables, that are not optokens Profiles, and that this host maps to no command, by name.
- `absent` (`env`): each variable a program's `[credential]` needs that the command this host maps its service to did not print, with `name` and `service`.
- `conflicting` (`env`): each variable read from more than one place, with `name` and `sources` (each `entry` and `field`; `field` `""` is the entry's single value).
- `projects` (`source remove`): the installs that still record the source, each with `project` (its root; `null` for the user's own installs) and `roles` (the roles it records of the source).

## The rules `check` enforces

A problem's `rule` is one of:

{const:RULES}

## Help

`--help`, `-h`, `help <topic>` and `--version` answer on stdout and exit {const:EXIT_OK}, as Markdown or text even with `--json`.
