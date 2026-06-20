#!/usr/bin/env bash
# Bundle Tesseract dylibs and English tessdata into a built WaveTrace.app.
set -eu

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
MACOS_DIR="$APP/Contents/MacOS"
FRAMEWORKS="$APP/Contents/Frameworks"
RESOURCES="$APP/Contents/Resources"

BIN=""
for candidate in "$MACOS_DIR/WaveTrace" "$MACOS_DIR/wavetrace"; do
  if [[ -f "$candidate" ]]; then
    BIN="$candidate"
    break
  fi
done
if [[ -z "$BIN" ]]; then
  shopt -s nullglob
  for candidate in "$MACOS_DIR"/*; do
    [[ -f "$candidate" ]] || continue
    BIN="$candidate"
    break
  done
  shopt -u nullglob
fi
if [[ -z "$BIN" ]]; then
  echo "No executable found in $MACOS_DIR" >&2
  ls -la "$MACOS_DIR" >&2 || true
  exit 1
fi
chmod +x "$BIN" 2>/dev/null || true

mkdir -p "$FRAMEWORKS" "$RESOURCES/tessdata"

TESSDATA_URL="https://github.com/tesseract-ocr/tessdata_fast/raw/main/eng.traineddata"
if ! curl -fsSL -o "$RESOURCES/tessdata/eng.traineddata" "$TESSDATA_URL"; then
  echo "Failed to download eng.traineddata from $TESSDATA_URL" >&2
  exit 1
fi

copy_lib() {
  local lib="$1"
  [[ -f "$lib" ]] || return 0
  local base dest
  base="$(basename "$lib")"
  dest="$FRAMEWORKS/$base"
  cp -f "$lib" "$dest"
  install_name_tool -id "@executable_path/../Frameworks/$base" "$dest" 2>/dev/null || true
}

BREW_PREFIX="$(brew --prefix)"
TESS_PREFIX="$(brew --prefix tesseract 2>/dev/null || true)"
LEPT_PREFIX="$(brew --prefix leptonica 2>/dev/null || true)"

shopt -s nullglob
for lib in \
  "${TESS_PREFIX:+$TESS_PREFIX/lib/libtesseract"*.dylib} \
  "${LEPT_PREFIX:+$LEPT_PREFIX/lib/libleptonica"*.dylib} \
  "$BREW_PREFIX/lib/libtesseract"*.dylib \
  "$BREW_PREFIX/lib/libleptonica"*.dylib \
  "$BREW_PREFIX/opt/tesseract/lib/libtesseract"*.dylib \
  "$BREW_PREFIX/opt/leptonica/lib/libleptonica"*.dylib; do
  copy_lib "$lib"
done
shopt -u nullglob

otool_lines="$(otool -L "$BIN" 2>/dev/null | awk '/opt\/homebrew|usr\/local/ {print $1}' || true)"
while IFS= read -r bad; do
  [[ -n "$bad" ]] || continue
  base="$(basename "$bad")"
  if [[ -f "$FRAMEWORKS/$base" ]]; then
    install_name_tool -change "$bad" "@executable_path/../Frameworks/$base" "$BIN" 2>/dev/null || true
  fi
done <<< "$otool_lines"

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP" 2>/dev/null || true
fi

dylib_count=0
shopt -s nullglob
for _ in "$FRAMEWORKS"/libtesseract*.dylib "$FRAMEWORKS"/libleptonica*.dylib; do
  dylib_count=$((dylib_count + 1))
done
shopt -u nullglob

echo "Bundled Tesseract into $APP (binary: $(basename "$BIN"), dylibs: $dylib_count)"
