#!/usr/bin/env bash
# Build a release of DeckWeaver - the StreamController plugin and the OpenDeck plugin - from committed sources.
#
#   ./release.sh VERSION            e.g. ./release.sh 0.8.0
#
# CI runs this for every v* tag (.github/workflows/build.yml), so a release is made by pushing a tag; running it by
# hand gives the same files. VERSION is compiled in and stamped into the built manifests (build.sh, through
# DECKWEAVER_VERSION); no file in the repo is changed, so the tag is the one place a version lives. Everything goes
# to out/VERSION (gitignored):
#
#   deckweaver-streamcontroller-VERSION.zip        the plugin folder; unzip into StreamController's plugins dir
#   deckweaver-opendeck-VERSION.streamDeckPlugin   install from OpenDeck's plugin manager ("Install from file")
#   SHA256SUMS
#
# Needs: cargo, python3, zip, rsync, libpulse headers (libpulse-dev / libpulse).
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

echo "STAGE Building $version"
DECKWEAVER_VERSION=$version ./build.sh release

echo "STAGE Packaging"
mkdir -p "$rel"
stage=$(mktemp -d)
mkdir -p "$stage/com_designgears_DeckWeaver"
# The StreamController plugin is the repo minus the sources and the OpenDeck side, as build.sh --install lays it out.
rsync -a --exclude-from=scripts/streamcontroller-exclude.txt ./ "$stage/com_designgears_DeckWeaver/"
sed -i "0,/\"version\": \"[^\"]*\"/s//\"version\": \"$version\"/" "$stage/com_designgears_DeckWeaver/manifest.json"
[ -f "$stage/com_designgears_DeckWeaver/deckweaver/_core.abi3.so" ] || fail "the StreamController extension was not built"
(cd "$stage" && zip -qr "$rel/deckweaver-streamcontroller-$version.zip" com_designgears_DeckWeaver)
rm -rf "$stage"
cp deckweaver.streamDeckPlugin "$rel/deckweaver-opendeck-$version.streamDeckPlugin"
(cd "$rel" && sha256sum -- *.zip *.streamDeckPlugin > SHA256SUMS)

echo "STAGE Done"
ls -l "$rel"
