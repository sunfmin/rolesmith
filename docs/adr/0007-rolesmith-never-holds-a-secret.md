# 0007. rolesmith never holds a secret

Status: accepted, 2026-11-22 (Felix, with rolesmith's first contract); recorded here on 2026-11-26, when
that contract's promises moved to their homes.

## Context

A role's work reads credentials: an App Store Connect key, a GitHub token. Libraries are shared, often
public, and a project's files are committed. Anything rolesmith wrote a value into would be copied to
every machine and every reader of that library or project.

## Decision

Invariant: **rolesmith never holds a secret.** A role names the credentials it needs by their
**mytokens** entry; rolesmith reads a value only to hand it to a process (`rolesmith env`), and never
writes one to disk, to a library, to a project or to its own output files.

- A role and its tools name entries (`services`, `[credential]`), never values. Where a host reads a
  service is the host's own setting (ADR 0006), never the library's.
- `rolesmith check` and `rolesmith lesson add` refuse text that looks like a secret, by one table of
  patterns in the code (`src/secrets.rs`), naming the file, line and pattern, never echoing the match.
- `rolesmith doctor` reads only whether an entry exists (`mytokens list`), never its value.

Options were a store of rolesmith's own, or names only with the values left in mytokens (or a host's
command). Names only was chosen: a value has one home, the keychain it is kept in, and rolesmith
cannot leak what it never keeps.

## Consequences

- Whoever runs a role's work brings their own credentials: sharing a library shares no secret.
- Undo: none short of a store of rolesmith's own; every command that reads a credential would change.
