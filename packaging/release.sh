#!/bin/sh
# One version of rolesmith, cut on this Mac: factorium.toml's cut step runs
# `packaging/release.sh cut $FACTORIUM_VERSION` in a worktree at the base's
# tip, then factorium tags what it left at HEAD and `brew upgrade`s the
# formula.
#
#   release.sh cut <vX.Y.Z>   build for Apple Silicon with the tag's version
#                             stamped (build.rs, ROLESMITH_RELEASE; Cargo.toml
#                             is not edited and nothing is committed), check
#                             `rolesmith --version` equals the tag, package,
#                             and publish: the archive to the tap's release
#                             rolesmith-v<X.Y.Z>, the formula to the tap's
#                             Formula/rolesmith.rb. Run again after a cut that
#                             failed half way, it publishes over what is there.
#   release.sh check-tag <vX.Y.Z>  exit 0 if cut would take the version, 2 if not
#   release.sh dry            what the tests run: check the binary this machine
#                             built ($BUILT, target/debug/rolesmith by default)
#                             says the development form
#                             X.Y.Z+<commits>.g<commit>[.dirty], package it at
#                             that version, check the formula, publish nothing
#
# The formula installs the prebuilt binary and compiles nothing, so it
# installs on a standard account with Homebrew in ~/homebrew. The archive is
# downloaded from the tap (sunfmin/homebrew-tap), as every formula there is.
# DIST names another directory than dist/.
set -eu
cd "$(dirname "$0")/.."

# A cut runs under factorium's daemon, whose PATH may lack rustup's
# ~/.cargo/bin. Put it last, so a cargo already on PATH still comes first.
PATH="$PATH:$HOME/.cargo/bin"
export PATH

REPO=https://github.com/sunfmin/rolesmith
TAP=sunfmin/homebrew-tap
TARGET=aarch64-apple-darwin
DIST=${DIST:-dist}

usage() {
    echo "usage: release.sh cut <vX.Y.Z> | dry | check-tag <vX.Y.Z>" >&2
    exit 2
}

# check_binary <path> <version>: an arm64 binary that says the version.
check_binary() {
    lipo -archs "$1" | grep -qx arm64 || {
        echo "release: $1 is not an arm64 binary" >&2
        exit 1
    }
    said=$("$1" --version)
    [ "$said" = "rolesmith $2" ] || {
        echo "release: $1 --version says '$said', not 'rolesmith $2'" >&2
        exit 1
    }
}

# package <binary> <version>: the archive and the formula, checked, in $DIST.
package() {
    mkdir -p "$DIST"
    archive=rolesmith-$2-macos-arm64.tar.gz
    staged=$(mktemp -d)
    cp "$1" "$staged/rolesmith"
    tar -czf "$DIST/$archive" -C "$staged" rolesmith
    rm -rf "$staged"
    sha=$(shasum -a 256 "$DIST/$archive" | cut -d' ' -f1)
    release=rolesmith-v$2
    formula=$DIST/rolesmith.rb
    cat > "$formula" <<FORMULA
class Rolesmith < Formula
  desc "Roles for any coding agent"
  homepage "$REPO"
  url "https://github.com/$TAP/releases/download/$release/$archive"
  sha256 "$sha"
  version "$2"
  license "MIT"

  depends_on :macos
  depends_on arch: :arm64

  def install
    bin.install "rolesmith"
  end

  test do
    assert_equal "rolesmith #{version}", shell_output("#{bin}/rolesmith --version").strip
  end
end
FORMULA
    ruby -c "$formula" >/dev/null
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

case "${1:-}" in
check-tag)
    [ $# -eq 2 ] || usage
    check_tag "$2"
    exit 0
    ;;
dry)
    [ $# -eq 1 ] || usage
    built=${BUILT:-target/debug/rolesmith}
    [ -x "$built" ] || {
        echo "release: $built is missing: build it first" >&2
        exit 1
    }
    version=$("$built" --version | sed 's/^rolesmith //')
    case "$version" in
    *[!0-9a-z.+]* | *+*+*) dev=no ;;
    [0-9]*.[0-9]*.[0-9]*+[0-9]*.g[0-9a-f]*) dev=yes ;;
    *) dev=no ;;
    esac
    [ "$dev" = yes ] || {
        echo "release: $built --version says '$version', not X.Y.Z+<commits>.g<commit>[.dirty]" >&2
        exit 1
    }
    check_binary "$built" "$version"
    package "$built" "$version"
    echo "release: dry ok (rolesmith $version, $DIST/$archive, $formula)"
    exit 0
    ;;
cut) [ $# -eq 2 ] || usage ;;
*) usage ;;
esac
check_tag "$2"
version=${2#v}

# The tag's version, stamped by build.rs: the tag goes on HEAD as it is.
ROLESMITH_RELEASE=$version cargo build --release --locked -p rolesmith --target "$TARGET"
built=target/$TARGET/release/rolesmith
check_binary "$built" "$version"
package "$built" "$version"

# A release that is there already takes the archive again: a cut that
# failed half way is run a second time.
if gh release view "$release" -R "$TAP" >/dev/null 2>&1; then
    gh release upload "$release" -R "$TAP" --clobber "$DIST/$archive"
else
    gh release create "$release" -R "$TAP" --title "rolesmith $version" \
        --notes "The binary the formula \`rolesmith\` installs." "$DIST/$archive"
fi
tap=$(mktemp -d)
gh repo clone "$TAP" "$tap" -- --quiet --depth 1
cp "$formula" "$tap/Formula/rolesmith.rb"
git -C "$tap" add Formula/rolesmith.rb
if git -C "$tap" diff --cached --quiet; then
    echo "release: the tap's formula is rolesmith $version already"
else
    git -C "$tap" commit --quiet -m "rolesmith $version"
    git -C "$tap" push --quiet origin HEAD
fi
rm -rf "$tap"
