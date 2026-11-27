# 0008. Credentials are read from optokens

Status: accepted, 2026-11-27 (tech-lead, for Felix, when mytokens was removed from mac-studio). Amends 0006 and 0007 where they
name mytokens.

## Context

rolesmith read every service no host maps (ADR 0006) from mytokens, the macOS keychain CLI. mytokens is being removed from
the factorium host mac-studio; its credentials are read there through `optokens` instead, a 1Password
vault reached through a service account and answered from a sealed local Copy that optokens keeps
(5 min Life; `--fresh` syncs first). Without mytokens, `rolesmith doctor` failed on every role with a
service, and factorium held their issues.

## Decision

The program rolesmith runs for a service no host maps is **optokens**, and nothing else about how a
credential is read changes: a mapped service still runs its command, and doctor still reads presence
with one listing and never a value.

- `doctor` runs `optokens list` once (no `--fresh`: the Copy's Life is fresh enough for a check the
  daemon repeats) and reads its JSON. A service is present when a Secret of that `service` is listed
  under the account `default`, the one `optokens get <service>` reads; a Secret under another account
  only is missing. A Profile is a Secret of kind `profile`, and its `fields` are its variables.
- `env` reads each value with `optokens get <entry> [--field <f>]` and keeps it byte for byte:
  optokens prints no newline of its own, so a value's own trailing newline is kept.
- A failing optokens (exit 2) is reported by the faults and fixes of its error object.
- The fix for a missing Secret is `optokens put <name>`, or `optokens put <name> --fields "<A>,<B>"`.

The data format stays: a missing credential is still `kind` `entry`, and the tool.toml key
`[credential] mytokens = "<entry>"` keeps its name, now naming the optokens Secret. factorium reads
`kind`, libraries already hold the key, and one swap of the program should not also be a format
change.

ADR 0007's invariant holds: rolesmith keeps no value. optokens' Copy is sealed and belongs to
optokens, not to rolesmith.

Measured on mac-studio, optokens 0.1.9: `/usr/bin/time -p optokens list --fresh` took 4.80 s (a Sync);
`/usr/bin/time -p optokens list`, within the Copy's Life, 0.00 s (2026-11-27).

## Considered

- A) Map each service to `optokens env <service>` in every host's services.toml. Rejected: each
  host repeats it, and doctor would run a `check` per service instead of one listing.
- B) Swap the program, and keep the data format. Chosen.
- C) Swap the program and rename the key `mytokens` (with or without a serde alias). Left to its own
  issue, if wanted.

## Consequences

- A host without optokens fails `doctor` and `env` for every unmapped service with "cannot run
  `optokens list`", as it did with mytokens before; it maps the service in services.toml instead.
- A Secret listed under an account other than `default` no longer counts, where mytokens' listing
  counted any account.
- Undo: revert the change; rolesmith reads mytokens again.
