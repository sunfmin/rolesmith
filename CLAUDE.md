# rolesmith

A Rust CLI that keeps roles for coding agents in git libraries, installs them into any agent, and
records lessons that people have distilled into them. `rolesmith help all` is the manual: what
each command does, refuses and answers, written on the code each fact lives in. Read it before
changing behaviour, and change the help in the same commit when behaviour changes. A decision, and
a promise no help states, goes in an ADR (`docs/adr/`); a promise a run could break is registered
in `truefit.toml` too.

## Working here

- `./gate.sh` must pass before anything lands: formatting, clippy with warnings as errors, the tests.
- Tests run the built binary against temporary git repositories, with stand-ins for `gh` and
  `optokens` on `PATH`; no test reaches the network or 1Password.
- rolesmith never calls a model (ADR 0001) and never writes a secret (ADR 0007). A change that
  would is out of scope; say so on the issue.
- GitHub Issues on sunfmin/rolesmith are not this repo's tracker. (A library's GitHub issues are
  where rolesmith files *lessons*, `rolesmith lesson --help`; that is the product, not this repo's
  work.)
