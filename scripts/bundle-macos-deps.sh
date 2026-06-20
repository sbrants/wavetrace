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
MACOS_DIR="$APP/Contents/MacOS"
FRAMEWORKS="$APP/Contents/Frameworks"
RESOURCES="$APP/Contents/Resources"

BIN=""
for candidate in "$MACOS_DIR/WaveTrace" "$MACOS_DIR/wavetrace"; do
  if [[ -f "$candidate" ]]; then
    BIN="$candidate"
    chmod +x "$BIN" 2>/dev/null || true
    break
  fi
done
if [[ -z "$BIN" ]]; then
  shopt -s nullglob
  for candidate in "$MACOS_DIR"/*; do
    if [[ -f "$candidate" ]]; then
      BIN="$candidate"
      chmod +x "$BIN" 2>/dev/null || true
      break
    fi
  done
  shopt -u nullglob
fi
if [[ -z "$BIN" ]]; then
  echo "No executable found in $MACOS_DIR" >&2
  ls -la "$MACOS_DIR" >&2 || true
  exit 1
fi

mkdir -p "$FRAMEWORKS" "$RESOURCES/tessdata"

BREW_PREFIX="$(brew --prefix)"
TESS_PREFIX="$(brew --prefix tesseract 2>/dev/null || true)"
LEPT_PREFIX="$(brew --prefix leptonica 2>/dev/null || true)"

ENG_DATA=""
for candidate in \
  "${TESS_PREFIX:+$TESS_PREFIX/share/tessdata/eng.traineddata}" \
  "$BREW_PREFIX/share/tessdata/eng.traineddata" \
  "/opt/homebrew/share/tessdata/eng.traineddata" \
  "/opt/homebrew/opt/tesseract/share/tessdata/eng.traineddata" \
  "/usr/local/share/tessdata/eng.traineddata" \
  "/usr/local/opt/tesseract/share/tessdata/eng.traineddata"; do
  [[ -n "$candidate" && -f "$candidate" ]] || continue
  ENG_DATA="$candidate"
  break
done
if [[ -z "$ENG_DATA" ]]; then
  ENG_DATA="$(find "$BREW_PREFIX" -path '*/tessdata/eng.traineddata' 2>/dev/null | head -1 || true)"
fi
if [[ -z "$ENG_DATA" ]]; then
  echo "eng.traineddata not found (brew install tesseract)" >&2
  exit 1
fi
cp "$ENG_DATA" "$RESOURCES/tessdata/"

copy_lib() {
  local lib="$1"
  [[ -f "$lib" ]] || return 0
  local base dest
  base="$(basename "$lib")"
  dest="$FRAMEWORKS/$base"
  cp -f "$lib" "$dest"
  install_name_tool -id "@executable_path/../Frameworks/$base" "$dest" 2>/dev/null || true
}

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

set +e
linked="$(otool -L "$BIN" 2>/dev/null | awk '/tesseract|leptonica/ {print $1}' | grep '^/' || true)"
set -e
for lib in $linked; do
  copy_lib "$lib"
done

dylib_count=0
shopt -s nullglob
for lib in "$FRAMEWORKS"/libtesseract*.dylib "$FRAMEWORKS"/libleptonica*.dylib; do
  dylib_count=$((dylib_count + 1))
done
shopt -u nullglob

needs_dylibs=false
if otool -L "$BIN" 2>/dev/null | grep -Eqi 'tesseract|leptonica'; then
  needs_dylibs=true
fi

if [[ "$dylib_count" -eq 0 && "$needs_dylibs" == true ]]; then
  while IFS= read -r lib; do
    copy_lib "$lib"
  done < <(find "$BREW_PREFIX/opt/tesseract" "$BREW_PREFIX/opt/leptonica" "$BREW_PREFIX/Cellar/tesseract" "$BREW_PREFIX/Cellar/leptonica" \
    \( -name 'libtesseract*.dylib' -o -name 'libleptonica*.dylib' \) 2>/dev/null | sort -u | head -8)
  dylib_count=0
  shopt -s nullglob
  for lib in "$FRAMEWORKS"/libtesseract*.dylib "$FRAMEWORKS"/libleptonica*.dylib; do
    dylib_count=$((dylib_count + 1))
  done
  shopt -u nullglob
fi

if [[ "$needs_dylibs" == true && "$dylib_count" -eq 0 ]]; then
  echo "Binary links Tesseract/Leptonica but no dylibs were bundled" >&2
  otool -L "$BIN" 2>/dev/null | head -20 >&2 || true
  exit 1
fi

set +e
otool_lines="$(otool -L "$BIN" 2>/dev/null | awk '/opt\/homebrew|usr\/local/ {print $1}')"
set -e
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

echo "Bundled Tesseract into $APP (binary: $(basename "$BIN"), dylibs: $dylib_count, tessdata: $ENG_DATA)"
