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
- GitHub Issues on sunfmin/rolesmith are not this repo's tracker. (A library's GitHub issues are
  where rolesmith files *lessons*, SPEC.md §6; that is the product, not this repo's work.)
