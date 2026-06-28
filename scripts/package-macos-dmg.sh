#!/usr/bin/env bash
# Create a DMG from a bundled WaveTrace.app (after bundle-macos-deps.sh).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

TARGET="${1:?Usage: package-macos-dmg.sh <rust-target> <arch-label>}"
ARCH_LABEL="${2:?Usage: package-macos-dmg.sh <rust-target> <arch-label>}"

VERSION="$(node -p "require('./package.json').version")"
APP_DIR="src-tauri/target/${TARGET}/release/bundle/macos"
APP="$APP_DIR/WaveTrace.app"

if [[ ! -d "$APP" ]]; then
  echo "Missing app bundle: $APP" >&2
  exit 1
fi

DMG_NAME="WaveTrace_${VERSION}_macos_${ARCH_LABEL}.dmg"
DMG_PATH="$APP_DIR/$DMG_NAME"

# Stage the app next to an /Applications symlink so the mounted DMG shows the
# familiar "drag WaveTrace into Applications" layout instead of just the bare
# app. ditto preserves the code signature and extended attributes.
STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT
ditto "$APP" "$STAGING/WaveTrace.app"
ln -s /Applications "$STAGING/Applications"

rm -f "$DMG_PATH"
hdiutil create -volname "WaveTrace" -srcfolder "$STAGING" -ov -format UDZO "$DMG_PATH"
echo "Created $DMG_PATH"
