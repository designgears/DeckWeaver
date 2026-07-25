#!/bin/bash
# Build DeckWeaver for StreamController (Python/PyO3) and OpenDeck (native binary)
#
# Usage: ./build.sh [clean|dev|release] [targets] [install flags]
#   clean|dev|release - build profile (default: release)
#   --streamcontroller, --sc  - build StreamController Python extension only
#   --opendeck, --od          - build OpenDeck plugin binary only
#   --all                     - build both targets (default)
#   --install, -i             - install StreamController plugin after build
#   --install-opendeck        - symlink/copy OpenDeck plugin into OpenDeck plugins dir

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: cargo not found (install via rustup)"
  exit 1
fi

sync_version() {
  VERSION=$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)
  if [ -n "$VERSION" ]; then
    sed -i "s/^version = \".*\"/version = \"$VERSION\"/" pyproject.toml
    sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" manifest.json
    sed -i "s/\"Version\": \".*\"/\"Version\": \"$VERSION\"/" opendeck/com.designgears.deckweaver.sdPlugin/manifest.json
  fi
}
sync_version

PROFILE="release"
BUILD_SC=true
BUILD_OD=true
INSTALL_SC=false
INSTALL_OD=false

for arg in "$@"; do
  case "$arg" in
    clean|dev|release) PROFILE="$arg" ;;
    --streamcontroller|--sc) BUILD_SC=true; BUILD_OD=false ;;
    --opendeck|--od) BUILD_SC=false; BUILD_OD=true ;;
    --all) BUILD_SC=true; BUILD_OD=true ;;
    --install|-i) INSTALL_SC=true ;;
    --install-opendeck) INSTALL_OD=true ;;
    *)
      echo "Error: Unknown option '$arg'"
      echo "Usage: $0 [clean|dev|release] [--streamcontroller|--opendeck|--all] [--install|-i] [--install-opendeck]"
      exit 1
      ;;
  esac
done

if [ "$PROFILE" = "clean" ]; then
  echo "Cleaning build artifacts..."
  cargo clean
  rm -rf .venv-3.*
  rm -f deckweaver/_core*.so
  rm -rf opendeck/com.designgears.deckweaver.sdPlugin/*/bin
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
OD_PLUGIN_ROOT="opendeck/com.designgears.deckweaver.sdPlugin"
OD_BIN_DIR="$OD_PLUGIN_ROOT/$HOST_TRIPLE/bin"

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

  mkdir -p "$OD_BIN_DIR"
  OD_BIN=""
  for candidate in "target/$TARGET_SUBDIR/deckweaver-opendeck" target/$TARGET_SUBDIR/deps/deckweaver-*; do
    if [ -f "$candidate" ] && [ -x "$candidate" ] && file "$candidate" | grep -q 'ELF.*executable'; then
      OD_BIN="$candidate"
      break
    fi
  done
  if [ -z "$OD_BIN" ]; then
    echo "Error: OpenDeck binary not found after build"
    exit 1
  fi
  cp "$OD_BIN" "$OD_BIN_DIR/deckweaver"
  chmod +x "$OD_BIN_DIR/deckweaver"

  mkdir -p "$OD_PLUGIN_ROOT/icons"
  cp -f store/Thumbnail.png "$OD_PLUGIN_ROOT/icons/plugin.png" 2>/dev/null || true
  cp -f store/Thumbnail.png "$OD_PLUGIN_ROOT/icons/plugin@2x.png" 2>/dev/null || true
  cp -f store/Thumbnail.png "$OD_PLUGIN_ROOT/icons/knob.png" 2>/dev/null || true
  cp -f store/Thumbnail.png "$OD_PLUGIN_ROOT/icons/button.png" 2>/dev/null || true
  cp -f store/Thumbnail.png "$OD_PLUGIN_ROOT/icons/slider.png" 2>/dev/null || true

  echo "OpenDeck plugin bundle: $OD_PLUGIN_ROOT"
  echo "OpenDeck binary: $OD_BIN_DIR/deckweaver"
fi

if [ "$INSTALL_SC" = true ]; then
  PLUGIN_DEST="${DECKWEAVER_PLUGIN_DEST:-$HOME/.var/app/com.core447.StreamController/data/plugins/com_designgears_DeckWeaver}"
  if [ -d "$(dirname "$PLUGIN_DEST")" ]; then
    echo "Installing StreamController plugin to $PLUGIN_DEST"
    rm -rf "$PLUGIN_DEST"
    mkdir -p "$PLUGIN_DEST"
    rsync -a \
      --exclude='.git' \
      --exclude='target' \
      --exclude='crates' \
      --exclude='opendeck' \
      --exclude='streamcontroller' \
      --exclude='.venv*' \
      --exclude='build.sh' \
      --exclude='Cargo.toml' \
      --exclude='Cargo.lock' \
      --exclude='*.md' \
      --exclude='.gitignore' \
      "$SCRIPT_DIR/" "$PLUGIN_DEST/"
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
