# 0002. Releases are universal, signed and notarized by the tap's workflow

Status: accepted, 2026-11-22 (Felix; rolesmith#19, after sunfmin/slimfit#100)

## Context

Until rolesmith#19 a cut (`packaging/release.sh cut`) built on the cutting host for aarch64 only and
published from there: the archive to the tap's release, the formula (`depends_on arch: :arm64`) to
the tap. The binary was only ad hoc signed: Intel Macs could not run it, and Gatekeeper rejected a
copy downloaded in a browser.

The owner, 2026-11-22: 「把所有的工具都变成 universal。」「把公开的工具全部在 GitHub CI 上签。」
「这个密钥的环境变量可以统一配置到账号上面吗？」 A personal account has no account-wide secrets, so
slimfit's [ADR 0013](https://github.com/sunfmin/slimfit/blob/main/docs/adr/0013-releases-are-signed-and-notarized-by-the-taps-workflow.md)
put them in one repo, sunfmin/homebrew-tap, whose `release` workflow signs every tool
(contract: the tap's `.github/workflows/README.md`).

## Decision

rolesmith follows slimfit's ADR 0013. Invariant: **the Developer ID identity and the notary key live
only in sunfmin/homebrew-tap, and only its `release` workflow signs.** sunfmin/rolesmith has no
signing secret and no release workflow.

- **rolesmith builds.** `packaging/release.sh build <vX.Y.Z> <outdir>` builds `aarch64-apple-darwin`
  and `x86_64-apple-darwin` with `ROLESMITH_RELEASE` set, checks the binary says the version,
  `lipo -create`s them, and leaves the universal `rolesmith` and `rolesmith.rb` (no
  `depends_on arch`, `url "@URL@"`, `sha256 "@SHA256@"`) in `<outdir>`. The gate's `dry` and the
  cut run the same `build`.
- **The tap signs and publishes**: it checks out rolesmith at the tag, runs `build`, signs with
  hardened runtime, notarizes, verifies, and publishes `rolesmith-v<X.Y.Z>` and
  `Formula/rolesmith.rb`.
- **The cut waits.** `cut` builds here, pushes the tag, runs
  `gh workflow run release.yml -R sunfmin/homebrew-tap -f repo=rolesmith -f tag=<tag>`, waits for
  that run bounded by `RELEASE_WAIT_LIMIT` (2400 s), and fails with its `--log-failed` when it
  fails. A failing `gh run list` or `gh run view` (GitHub answers HTTP 500 now and then) is asked
  again until that limit (rolesmith#57). It takes away a tag it pushed when it ends, since factorium
  tags HEAD itself afterwards, and first cancels a run it dispatched and has not seen finish, which
  would fail without its tag.

## Consequences

- The tap's workflow needed nothing new for rolesmith.
- The gate builds two release targets; `[cut] limit` is 2700 s, the build plus the wait.
- A cut needs only `gh` logged in as a user who may push rolesmith's tags and dispatch the tap's
  workflow; no tap token or deploy key on the host.
