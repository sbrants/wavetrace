#!/usr/bin/env bash
set -euo pipefail

if ! command -v pacman >/dev/null 2>&1; then
  echo "This script must be run on Arch Linux." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

echo "Installing Arch build dependencies (may prompt for sudo)..."
sudo pacman -Syu --needed --noconfirm \
  base-devel git nodejs npm rust patchelf \
  gtk3 webkit2gtk-4.1 librsvg libappindicator-gtk3 \
  tesseract tesseract-data-eng openssl pkgconf \
  pipewire libpipewire

npm ci
npm run tauri build

echo
echo "Built:"
echo "  $repo_root/src-tauri/target/release/wavetrace"
echo "  $repo_root/src-tauri/target/release/bundle/appimage/"
echo "  $repo_root/src-tauri/target/release/bundle/deb/"
