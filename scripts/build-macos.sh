#!/usr/bin/env bash
# Build WaveTrace for macOS (native arch). Requires: Xcode CLT, Node, Rust, Homebrew tesseract + dylibbundler.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

ARCH="$(uname -m)"
case "$ARCH" in
  arm64) TARGET="aarch64-apple-darwin"; LABEL="aarch64" ;;
  x86_64) TARGET="x86_64-apple-darwin"; LABEL="x86_64" ;;
  *)
    echo "Unsupported macOS arch: $ARCH" >&2
    exit 1
    ;;
esac

if ! command -v brew >/dev/null 2>&1; then
  echo "Homebrew is required: https://brew.sh" >&2
  exit 1
fi

brew list tesseract >/dev/null 2>&1 || brew install tesseract
brew list dylibbundler >/dev/null 2>&1 || brew install dylibbundler

rustup target add "$TARGET" >/dev/null 2>&1 || true

npm ci
npm run tauri build -- --target "$TARGET" --bundles app --config src-tauri/tauri.macos.ci.conf.json
bash scripts/bundle-macos-deps.sh "$TARGET"
if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  bash scripts/package-macos-updater.sh "$TARGET" "$LABEL"
else
  echo "Skipping updater bundle (set TAURI_SIGNING_PRIVATE_KEY to create one)"
fi
bash scripts/package-macos-dmg.sh "$TARGET" "$LABEL"

echo "Done. DMG under src-tauri/target/$TARGET/release/bundle/macos/"
