#!/usr/bin/env bash
# The version a build carries. DECKWEAVER_VERSION wins when set (release.sh, CI); otherwise it comes from the nearest
# v* tag, so a local build says where it was built from:
#
#   on tag v0.8.1                    0.8.1
#   8 commits after it               0.8.1-8-ge50c4f1
#   with uncommitted changes         0.8.1-8-ge50c4f1-dirty
#   no tag (or no git)               0.0.0-dev
set -euo pipefail
if [ -n "${DECKWEAVER_VERSION:-}" ]; then echo "$DECKWEAVER_VERSION"; exit 0; fi
cd "$(dirname "$0")/.."
d=$(git describe --tags --match 'v[0-9]*' --dirty 2>/dev/null) || { echo 0.0.0-dev; exit 0; }
echo "${d#v}"
