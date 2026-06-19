#!/usr/bin/env bash
# Create a signed macOS updater bundle (.app.tar.gz + .sig) after bundle-macos-deps.sh.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

TARGET="${1:?Usage: package-macos-updater.sh <rust-target> <arch-label>}"
ARCH_LABEL="${2:?Usage: package-macos-updater.sh <rust-target> <arch-label>}"

VERSION="$(node -p "require('./package.json').version")"
APP_DIR="src-tauri/target/${TARGET}/release/bundle/macos"
APP="$APP_DIR/WaveTrace.app"
TAR_NAME="WaveTrace_${VERSION}_macos_${ARCH_LABEL}.app.tar.gz"
TAR_PATH="$APP_DIR/$TAR_NAME"

if [[ ! -d "$APP" ]]; then
  echo "Missing app bundle: $APP" >&2
  exit 1
fi

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo "TAURI_SIGNING_PRIVATE_KEY is required" >&2
  exit 1
fi

rm -f "$TAR_PATH" "${TAR_PATH}.sig"
tar -czf "$TAR_PATH" -C "$APP_DIR" WaveTrace.app

npx tauri signer sign "$TAR_PATH"
echo "Created $TAR_PATH and ${TAR_PATH}.sig"
