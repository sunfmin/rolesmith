# rolesmith

A Rust CLI that keeps roles for coding agents in git libraries, installs them into any agent, and
records lessons that people have distilled into them. SPEC.md is the contract; read it before
changing behaviour, and change it in the same commit when behaviour changes.

## Working here

- `./gate.sh` must pass before anything lands: formatting, clippy with warnings as errors, the tests.
- Tests run the built binary against temporary git repositories, with stand-ins for `gh` and
  `mytokens` on `PATH`; no test reaches the network or a real keychain.
- rolesmith never calls a model and never writes a secret (SPEC.md §1). A change that would is out of
  scope; say so on the issue.

## Agent skills

### Issue tracker

Issues are in factorium's local tracker (`factorium issue … rolesmith`). See `docs/agents/issue-tracker.md`.

### Domain docs

single-context: `GLOSSARY.md` and `docs/adr/` at the root. See `docs/agents/domain.md`.
