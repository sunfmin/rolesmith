# 0006. A host maps a service to a command that prints its variables

Status: accepted, 2026-11-23 (control session, for Felix; rolesmith#41). Amended by 0008: the default
source is optokens, where this ADR says mytokens.

## Context

`rolesmith env` read every credential from mytokens. A library is shared, but where its credentials
live is not: one machine keeps them in mytokens, another in 1Password, another behind a script of
its own. Felix: "我打算这个东西可以支持 OP 或者 my tokens。或者任意的 shell 脚本".

## Decision

Each host may map a service to **a command whose stdout is the service's variables**, as
`NAME=value` / `export NAME='value'` lines, in its own `$XDG_CONFIG_HOME/rolesmith/services.toml`:

```toml
[service.asc-api-key]
command = "op inject -i ~/.config/rolesmith/asc-api-key.env"
check = "op whoami"
```

A service the host does not map is read from mytokens, as before (amended by 0008: optokens). `env` runs the command
(`sh -c`, stdin closed) when it reads, and exports what it prints; a `[credential]` of the service
names what it must print. `doctor` never runs the command: it runs the optional `check`, and lists a
service without one as `unchecked`, which does not make it refuse.

Invariant: **the choice of where a credential comes from belongs to the host, never to a library.**
`rolesmith check` refuses the setting in a library (`host-only`): a `services.toml` at its root, or
a `[service]` table in `library.toml`, `role.toml` or `tool.toml`.

## Considered

- A) Built-in providers (`mytokens`, `op`, `command`), each with its own keys. Rejected: `mytokens
  env` and `op inject` already print the same lines, so one rule covers all three with no
  per-vendor code to keep. (A service-account-only 1Password mapping was proposed and withdrawn the
  same day, for this more general one.)
- B) One per-host command per service, mytokens the default (amended by 0008: optokens). Chosen.

## Consequences

- rolesmith still writes no secret anywhere; the values pass through its memory to `export` lines or
  the child's environment, as mytokens values do.
- A mapped command's output is read before variables are checked against each other, so a conflict
  with a mapped service is found after it ran.
- Undo: drop the setting; every service is read from mytokens again (amended by 0008: optokens).
