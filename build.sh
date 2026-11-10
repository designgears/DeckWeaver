#!/bin/bash
# Build DeckWeaver for StreamController (Python/PyO3) and OpenDeck (native binary)
#
# Usage: ./build.sh [clean|dev|release] [targets] [--install|-i]
#   clean|dev|release              - build profile (default: release)
#   --streamcontroller, --sc, -sc  - build StreamController Python extension only
#   --opendeck, --od, -od          - build OpenDeck plugin binary only
#   --all                          - build both targets (default)
#   --install, -i                  - install whichever targets were built

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: cargo not found (install via rustup)"
  exit 1
fi

# The version comes from the git tag (scripts/version.sh), not from a file in the repo. It is
# compiled in through DECKWEAVER_VERSION and stamped into the built copies of the manifests only;
# the tracked manifests keep a 0.0.0 placeholder.
VERSION=$(scripts/version.sh)
export DECKWEAVER_VERSION="$VERSION"
# stamp_version FILE KEY: set the first "KEY": "..." in a JSON manifest to $VERSION
stamp_version() { sed -i "0,/\"$2\": \"[^\"]*\"/s//\"$2\": \"$VERSION\"/" "$1"; }

PROFILE="release"
BUILD_SC=true
BUILD_OD=true
INSTALL=false

for arg in "$@"; do
  case "$arg" in
    clean|dev|release) PROFILE="$arg" ;;
    --streamcontroller|--sc|-sc) BUILD_SC=true; BUILD_OD=false ;;
    --opendeck|--od|-od) BUILD_SC=false; BUILD_OD=true ;;
    --all) BUILD_SC=true; BUILD_OD=true ;;
    --install|-i) INSTALL=true ;;
    *)
      echo "Error: Unknown option '$arg'"
      echo "Usage: $0 [clean|dev|release] [--streamcontroller|--sc|-sc] [--opendeck|--od|-od] [--all] [--install|-i]"
      exit 1
      ;;
  esac
done

# Install follows the build selection, so `-i` alone installs both and
# `--od -i` installs only OpenDeck. Derived after parsing so flag order
# doesn't matter.
INSTALL_SC=false
INSTALL_OD=false
if [ "$INSTALL" = true ]; then
  INSTALL_SC="$BUILD_SC"
  INSTALL_OD="$BUILD_OD"
fi

if [ "$PROFILE" = "clean" ]; then
  echo "Cleaning build artifacts..."
  cargo clean
  rm -rf .venv-3.*
  rm -f deckweaver/_core*.so
  rm -rf opendeck/com.designgears.deckweaver.sdPlugin/*-unknown-linux-gnu deckweaver.streamDeckPlugin
  echo "Clean complete!"
  exit 0
fi

if [ "$PROFILE" != "dev" ] && [ "$PROFILE" != "release" ]; then
  echo "Error: Invalid profile '$PROFILE'"
  exit 1
fi

CARGO_FLAGS=()
TARGET_SUBDIR="debug"
if [ "$PROFILE" = "release" ]; then
  CARGO_FLAGS+=(--release)
  TARGET_SUBDIR="release"
fi

HOST_TRIPLE="$(rustc -vV | awk '/^host: / {print $2}')"
OD_SOURCE="opendeck/com.designgears.deckweaver.sdPlugin"
# The OpenDeck bundle is assembled here (sources + binary + stamped manifest), never in the tracked
# plugin folder.
OD_PLUGIN_ROOT="target/opendeck/com.designgears.deckweaver.sdPlugin"
OD_BIN_DIR="$OD_PLUGIN_ROOT/$HOST_TRIPLE/bin"
echo "DeckWeaver $VERSION"

if [ "$BUILD_SC" = true ]; then
  echo "Building StreamController extension (PyO3)..."
  cargo build -p deckweaver-py "${CARGO_FLAGS[@]}"
  mkdir -p deckweaver
  cp "target/$TARGET_SUBDIR/libdeckweaver.so" deckweaver/_core.abi3.so
  echo "StreamController extension: deckweaver/_core.abi3.so"
fi

if [ "$BUILD_OD" = true ]; then
  echo "Building OpenDeck plugin binary for $HOST_TRIPLE..."
  cargo build -p deckweaver-opendeck "${CARGO_FLAGS[@]}"

  # Must follow the build: the generator reads the fontawesome-free-pack sources out of the
  # cargo registry, and cargo only extracts them once something actually needs the crate. The
  # manifest is a property-inspector asset fetched at runtime, never a build input, so
  # generating it here is fine. Ordered the other way it works on a machine that happens to
  # have the crate cached and fails on a clean CI runner.
  echo "Generating Font Awesome icon manifest..."
  python3 "$SCRIPT_DIR/scripts/generate-fa-icons-json.py"

  rm -rf "$OD_PLUGIN_ROOT"
  mkdir -p "$(dirname "$OD_PLUGIN_ROOT")"
  # leftovers from builds that put the binary in the tracked folder are not copied
  rsync -a --exclude='*-unknown-linux-gnu/' "$OD_SOURCE/" "$OD_PLUGIN_ROOT/"
  stamp_version "$OD_PLUGIN_ROOT/manifest.json" Version
  # The [[bin]] in crates/deckweaver-opendeck/Cargo.toml; OpenDeck runs it as "deckweaver".
  install -Dm755 "target/$TARGET_SUBDIR/deckweaver-opendeck" "$OD_BIN_DIR/deckweaver"
  for icon in plugin plugin@2x knob button slider; do
    cp -f store/Thumbnail.png "$OD_PLUGIN_ROOT/icons/$icon.png"
  done

  # Rebuilt from scratch: zip updates an existing archive in place, which would keep files
  # that have since been removed from the bundle.
  rm -f "$SCRIPT_DIR/deckweaver.streamDeckPlugin"
  (cd "$(dirname "$OD_PLUGIN_ROOT")" && zip -qr "$SCRIPT_DIR/deckweaver.streamDeckPlugin" com.designgears.deckweaver.sdPlugin)

  echo "OpenDeck plugin bundle: $OD_PLUGIN_ROOT"
  echo "OpenDeck binary: $OD_BIN_DIR/deckweaver"
fi

if [ "$INSTALL_SC" = true ]; then
  PLUGIN_DEST="${DECKWEAVER_PLUGIN_DEST:-$HOME/.var/app/com.core447.StreamController/data/plugins/com_designgears_DeckWeaver}"
  if [ -d "$(dirname "$PLUGIN_DEST")" ]; then
    echo "Installing StreamController plugin to $PLUGIN_DEST"
    rm -rf "$PLUGIN_DEST"
    mkdir -p "$PLUGIN_DEST"
    rsync -a --exclude-from="$SCRIPT_DIR/scripts/streamcontroller-exclude.txt" \
      "$SCRIPT_DIR/" "$PLUGIN_DEST/"
    stamp_version "$PLUGIN_DEST/manifest.json" version
  else
    echo "Note: StreamController plugins dir not found, skipping install"
  fi
fi

if [ "$INSTALL_OD" = true ]; then
  OD_DEST="${DECKWEAVER_OPENDECK_DEST:-$HOME/.config/opendeck/plugins/com.designgears.deckweaver.sdPlugin}"
  mkdir -p "$(dirname "$OD_DEST")"
  rm -rf "$OD_DEST"
  cp -a "$OD_PLUGIN_ROOT" "$OD_DEST"
  echo "Installed OpenDeck plugin to $OD_DEST"
fi

echo "Build complete."
