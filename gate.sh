#!/bin/sh
# The gate: what must pass before anything lands (factorium.toml's [gate]).
set -eu
cd "$(dirname "$0")"
cargo fmt --all --check
cargo clippy --all-targets --quiet -- -D warnings
# The tests, measured against .slimfit/baseline.json (in place of `cargo test`).
slimfit check
# Each document in its one home, within its budget, measured against .docfit/baseline.json.
docfit check
# The release build, universal, at the development form (ADR 0002): what a cut builds.
DIST=target/dist packaging/release.sh dry
