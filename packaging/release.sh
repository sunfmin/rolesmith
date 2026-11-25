#!/bin/sh
# One version of rolesmith (ADR 0002): this repo builds it; sunfmin/homebrew-tap's
# release workflow signs, notarizes and publishes it. factorium.toml's cut step
# runs `packaging/release.sh cut $FACTORIUM_VERSION` in a worktree at the base's
# tip, then factorium tags what it left at HEAD and `brew upgrade`s the formula.
#
#   release.sh build <vX.Y.Z> <outdir>
#                             build aarch64 and x86_64 with the tag's version
#                             stamped (build.rs, ROLESMITH_RELEASE; Cargo.toml
#                             is not edited), check `rolesmith --version` says
#                             it, `lipo -create` them: <outdir> holds the
#                             universal `rolesmith` and the formula
#                             `rolesmith.rb`, its url and sha256 left as @URL@
#                             and @SHA256@ for the tap to fill in. Signs
#                             nothing, needs no secret.
#   release.sh cut <vX.Y.Z>   build, push the tag to origin, run the tap's
#                             release.yml on it and wait for that run, bounded
#                             (RELEASE_WAIT_LIMIT seconds from the cut's start,
#                             default 2400; a failing gh is asked again until
#                             then, and a run unfinished at the end is
#                             cancelled before the tag is taken away);
#                             publishes nothing from this host
#   release.sh check-tag <vX.Y.Z>  exit 0 if cut would take the version, 2 if not
#   release.sh dry            what the gate runs: build at the development form
#                             X.Y.Z+<commits>.g<commit>[.dirty], check the
#                             formula, publish nothing
#
# The formula installs the prebuilt binary and compiles nothing, so it
# installs on a standard account with Homebrew in ~/homebrew.
# DIST names another directory than dist/.
set -eu
cd "$(dirname "$0")/.."

# A cut runs under factorium's daemon, whose PATH may lack rustup's
# ~/.cargo/bin. Put it last, so a cargo already on PATH still comes first.
PATH="$PATH:$HOME/.cargo/bin"
export PATH

REPO=https://github.com/sunfmin/rolesmith
TAP=sunfmin/homebrew-tap
TARGETS="aarch64-apple-darwin x86_64-apple-darwin"
DIST=${DIST:-dist}

usage() {
    echo "usage: release.sh build <vX.Y.Z> <outdir> | cut <vX.Y.Z> | dry | check-tag <vX.Y.Z>" >&2
    exit 2
}

# check_tag <tag>: a version tag vX.Y.Z, or usage.
check_tag() {
    case "$1" in
    v*[!0-9.]* | v*..* | v.* | v*.) usage ;;
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) usage ;;
    esac
    case "$1" in v*.*.*.*) usage ;; esac
}

# build <release or empty> <outdir>: the universal binary and the formula in
# <outdir>; answers (in $version) the version the binary says. An empty
# release builds the development form.
build() {
    # shellcheck disable=SC2086 # the targets are words
    rustup target add $TARGETS >/dev/null 2>&1 || true
    # build.rs takes a set ROLESMITH_RELEASE, even an empty one, as the version.
    if [ -n "$1" ]; then export ROLESMITH_RELEASE="$1"; else unset ROLESMITH_RELEASE; fi
    slices=
    for target in $TARGETS; do
        cargo build --release --locked --quiet -p rolesmith --target "$target"
        slices="$slices target/$target/release/rolesmith"
    done
    rm -rf "$2"
    mkdir -p "$2"
    # shellcheck disable=SC2086 # the slices are paths without spaces
    lipo -create -output "$2/rolesmith" $slices
    archs=$(lipo -archs "$2/rolesmith")
    [ "$archs" = "x86_64 arm64" ] || {
        echo "release: $2/rolesmith holds '$archs', not 'x86_64 arm64'" >&2
        exit 1
    }
    version=$("$2/rolesmith" --version | sed 's/^rolesmith //')
    [ -z "$1" ] || [ "$version" = "$1" ] || {
        echo "release: $2/rolesmith --version says 'rolesmith $version', not 'rolesmith $1'" >&2
        exit 1
    }
    cat >"$2/rolesmith.rb" <<FORMULA
class Rolesmith < Formula
  desc "Roles for any coding agent"
  homepage "$REPO"
  url "@URL@"
  sha256 "@SHA256@"
  version "$version"
  license "MIT"

  depends_on :macos

  def install
    bin.install "rolesmith"
  end

  test do
    assert_equal "rolesmith #{version}", shell_output("#{bin}/rolesmith --version").strip
  end
end
FORMULA
    ruby -c "$2/rolesmith.rb" >/dev/null
}

# end: when the cut ends, however it ends, a run it dispatched and has not
# seen finish ($unfinished) is cancelled, then a tag it pushed ($pushed) is
# taken away, in that order: a run left going without its tag fails.
pushed=
unfinished=
end() {
    [ -z "$unfinished" ] || gh run cancel "$unfinished" -R "$TAP" >&2 || true
    [ -z "$pushed" ] || git push --quiet origin ":refs/tags/$pushed"
}
trap end EXIT

# push_tag <tag>: origin has <tag> on HEAD. factorium's own tag step tags
# HEAD after the cut and fails on a tag origin already holds, so a tag this
# cut pushed is taken away again when the cut ends (end).
push_tag() {
    head=$(git rev-parse HEAD)
    # An annotated tag's peeled line (^{}) comes after it: the last line is the commit.
    there=$(git ls-remote origin "refs/tags/$1" "refs/tags/$1^{}" | tail -n1 | cut -f1)
    if [ -z "$there" ]; then
        git push --quiet origin "$head:refs/tags/$1"
        pushed=$1
    elif [ "$there" != "$head" ]; then
        echo "release: origin's $1 is on $there, not HEAD ($head)" >&2
        exit 1
    fi
}

# latest_run <tag>: the id of the newest run of the tap's release.yml on
# rolesmith <tag>, or nothing.
latest_run() {
    gh run list -R "$TAP" --workflow release.yml --event workflow_dispatch --limit 50 \
        --json databaseId,displayTitle \
        --jq "[.[] | select(.displayTitle == \"rolesmith $1\") | .databaseId] | max // empty"
}

case "${1:-}" in
check-tag)
    [ $# -eq 2 ] || usage
    check_tag "$2"
    exit 0
    ;;
build)
    [ $# -eq 3 ] || usage
    check_tag "$2"
    build "${2#v}" "$3"
    echo "release: built rolesmith $version, universal, in $3"
    exit 0
    ;;
dry)
    [ $# -eq 1 ] || usage
    build "" "$DIST"
    case "$version" in
    *[!0-9a-z.+]* | *+*+*) dev=no ;;
    [0-9]*.[0-9]*.[0-9]*+[0-9]*.g[0-9a-f]*) dev=yes ;;
    *) dev=no ;;
    esac
    [ "$dev" = yes ] || {
        echo "release: $DIST/rolesmith --version says '$version', not X.Y.Z+<commits>.g<commit>[.dirty]" >&2
        exit 1
    }
    echo "release: dry ok (rolesmith $version, universal, $DIST)"
    exit 0
    ;;
cut) [ $# -eq 2 ] || usage ;;
*) usage ;;
esac
check_tag "$2"
tag=$2
started=$(date +%s)
limit=${RELEASE_WAIT_LIMIT:-2400}

# Built here first: a tag goes out only on a commit that builds at its version.
build "${tag#v}" "$DIST"
before=$(latest_run "$tag")
push_tag "$tag"
gh workflow run release.yml -R "$TAP" -f repo=rolesmith -f "tag=$tag"

# In the waits, a gh that fails (GitHub's API answers HTTP 500 now and then)
# is asked again until the limit, like a run not there or not done yet.
run=
while :; do
    run=$(latest_run "$tag") || run=
    [ -n "$run" ] && [ "$run" != "$before" ] && break
    [ $(($(date +%s) - started)) -lt "$limit" ] || {
        echo "release: no release.yml run of rolesmith $tag in $TAP after ${limit}s" >&2
        exit 1
    }
    sleep "${RELEASE_POLL:-5}"
done
unfinished=$run
echo "release: waiting for https://github.com/$TAP/actions/runs/$run"
while :; do
    state=$(gh run view "$run" -R "$TAP" --json status,conclusion --jq '.status + " " + .conclusion') ||
        state="unknown (gh run view failed)"
    case "$state" in
    "completed success") break ;;
    completed*)
        unfinished=
        echo "release: run $run of rolesmith $tag ended '$state'" >&2
        gh run view "$run" -R "$TAP" --log-failed >&2 || true
        exit 1
        ;;
    esac
    [ $(($(date +%s) - started)) -lt "$limit" ] || {
        echo "release: run $run of rolesmith $tag still '$state' after ${limit}s, cancelled: gh run view $run -R $TAP" >&2
        exit 1
    }
    sleep "${RELEASE_POLL:-15}"
done
unfinished=
echo "release: rolesmith $tag is published ($(($(date +%s) - started))s)"
