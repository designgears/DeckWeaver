#!/usr/bin/env bash
# Build a release of DeckWeaver - the StreamController plugin and the OpenDeck plugin - from committed sources.
#
#   ./release.sh VERSION            e.g. ./release.sh 0.8.0
#
# CI runs this for every v* tag (.github/workflows/build.yml), so a release is made by pushing a tag; running it by
# hand gives the same files. VERSION is stamped into Cargo.toml and the plugin manifests for the build only: the
# stamped files are put back afterwards, so the tag is the one place a version lives. Everything goes to out/VERSION
# (gitignored):
#
#   deckweaver-streamcontroller-VERSION.zip        the plugin folder; unzip into StreamController's plugins dir
#   deckweaver-opendeck-VERSION.streamDeckPlugin   install from OpenDeck's plugin manager ("Install from file")
#   SHA256SUMS
#
# Needs: cargo, python3, zip, rsync, file, libpulse headers (libpulse-dev / libpulse).
# ALLOW_DIRTY=1 builds from a tree with uncommitted changes (for trying the script out; not for releases).
set -euo pipefail
version=${1:?usage: ./release.sh VERSION}
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
fail() { echo "release: $*" >&2; exit 1; }

[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || fail "'$version' is not a version (MAJOR.MINOR.PATCH[-PRE])"
rel=$here/out/$version
[ ! -e "$rel" ] || fail "$rel exists; a version is built once (remove it to rebuild)"
if [ "${ALLOW_DIRTY:-0}" != 1 ] && [ -n "$(git status --porcelain)" ]; then
  fail "uncommitted changes (git status); commit them or set ALLOW_DIRTY=1"
fi

# The files the version is stamped into. Put back on exit, whatever happens, so a local run leaves the tree as it was.
stamped=(Cargo.toml Cargo.lock pyproject.toml manifest.json opendeck/com.designgears.deckweaver.sdPlugin/manifest.json)
backup=$(mktemp -d)
for f in "${stamped[@]}"; do mkdir -p "$backup/$(dirname "$f")"; cp -p "$f" "$backup/$f"; done
restore() { for f in "${stamped[@]}"; do cp -p "$backup/$f" "$f"; done; rm -rf "$backup"; }
trap restore EXIT

echo "STAGE Stamping version $version"
# The first `version = ` in Cargo.toml is [workspace.package]'s; build.sh copies it into the manifests.
sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml

echo "STAGE Building"
rm -f deckweaver.streamDeckPlugin
./build.sh release

echo "STAGE Packaging"
mkdir -p "$rel"
stage=$(mktemp -d)
mkdir -p "$stage/com_designgears_DeckWeaver"
# The StreamController plugin is the repo minus the sources and the OpenDeck side, as build.sh --install lays it out.
rsync -a --exclude-from=scripts/streamcontroller-exclude.txt ./ "$stage/com_designgears_DeckWeaver/"
[ -f "$stage/com_designgears_DeckWeaver/deckweaver/_core.abi3.so" ] || fail "the StreamController extension was not built"
(cd "$stage" && zip -qr "$rel/deckweaver-streamcontroller-$version.zip" com_designgears_DeckWeaver)
rm -rf "$stage"
cp deckweaver.streamDeckPlugin "$rel/deckweaver-opendeck-$version.streamDeckPlugin"
(cd "$rel" && sha256sum -- *.zip *.streamDeckPlugin > SHA256SUMS)

echo "STAGE Done"
ls -l "$rel"
