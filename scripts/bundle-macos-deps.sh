#!/usr/bin/env bash
# Bundle Tesseract dylibs and English tessdata into a built WaveTrace.app.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

TARGET="${1:-}"
if [[ -n "$TARGET" ]]; then
  APP_GLOB="src-tauri/target/${TARGET}/release/bundle/macos/*.app"
else
  APP_GLOB="src-tauri/target/*/release/bundle/macos/*.app"
fi

shopt -s nullglob
apps=($APP_GLOB)
shopt -u nullglob

if [[ ${#apps[@]} -eq 0 ]]; then
  echo "No .app bundle found (glob: $APP_GLOB)" >&2
  exit 1
fi

APP="${apps[0]}"
BIN="$APP/Contents/MacOS/wavetrace"
FRAMEWORKS="$APP/Contents/Frameworks"
RESOURCES="$APP/Contents/Resources"

if [[ ! -x "$BIN" ]]; then
  echo "Missing executable: $BIN" >&2
  exit 1
fi

mkdir -p "$FRAMEWORKS" "$RESOURCES/tessdata"

TESS_PREFIX="$(brew --prefix tesseract 2>/dev/null || brew --prefix)"
ENG_DATA="$TESS_PREFIX/share/tessdata/eng.traineddata"
if [[ ! -f "$ENG_DATA" ]]; then
  echo "eng.traineddata not found at $ENG_DATA (brew install tesseract)" >&2
  exit 1
fi
cp "$ENG_DATA" "$RESOURCES/tessdata/"

if command -v dylibbundler >/dev/null 2>&1; then
  dylibbundler -of -b -x -d "$FRAMEWORKS" -p @executable_path/../Frameworks "$BIN"
else
  echo "dylibbundler not installed; OCR may fail on machines without Homebrew Tesseract" >&2
fi

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP" || true
fi

echo "Bundled Tesseract into $APP"
