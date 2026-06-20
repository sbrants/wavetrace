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
  if [[ -f "$candidate" && -x "$candidate" ]]; then
    BIN="$candidate"
    break
  fi
done
if [[ -z "$BIN" ]]; then
  for candidate in "$MACOS_DIR"/*; do
    [[ -f "$candidate" && -x "$candidate" ]] || continue
    BIN="$candidate"
    break
  done
fi
if [[ -z "$BIN" ]]; then
  echo "No executable found in $MACOS_DIR" >&2
  exit 1
fi

mkdir -p "$FRAMEWORKS" "$RESOURCES/tessdata"

BREW_PREFIX="$(brew --prefix)"
ENG_DATA=""
for candidate in \
  "$BREW_PREFIX/share/tessdata/eng.traineddata" \
  "$BREW_PREFIX/opt/tesseract/share/tessdata/eng.traineddata" \
  "/opt/homebrew/share/tessdata/eng.traineddata" \
  "/usr/local/share/tessdata/eng.traineddata"; do
  if [[ -f "$candidate" ]]; then
    ENG_DATA="$candidate"
    break
  fi
done

if [[ -z "$ENG_DATA ]]; then
  echo "eng.traineddata not found (brew install tesseract)" >&2
  exit 1
fi
cp "$ENG_DATA" "$RESOURCES/tessdata/"

if command -v dylibbundler >/dev/null 2>&1; then
  if ! dylibbundler -of -b -x -d "$FRAMEWORKS" -p @executable_path/../Frameworks "$BIN"; then
    echo "dylibbundler failed; copying Homebrew OCR libs manually" >&2
    for lib in "$BREW_PREFIX"/lib/libtesseract*.dylib "$BREW_PREFIX"/lib/libleptonica*.dylib; do
      [[ -f "$lib" ]] || continue
      cp "$lib" "$FRAMEWORKS/"
    done
    for lib in "$FRAMEWORKS"/*.dylib; do
      [[ -f "$lib" ]] || continue
      install_name_tool -id "@executable_path/../Frameworks/$(basename "$lib")" "$lib" || true
    done
    for dep in libtesseract leptonica; do
      while IFS= read -r bad; do
        base="$(basename "$bad")"
        if [[ -f "$FRAMEWORKS/$base" ]]; then
          install_name_tool -change "$bad" "@executable_path/../Frameworks/$base" "$BIN" || true
        fi
      done < <(otool -L "$BIN" | awk '/opt\/homebrew|usr\/local/ {print $1}' | grep -i "$dep" || true)
    done
  fi
else
  echo "dylibbundler not installed; copying Homebrew OCR libs" >&2
  for lib in "$BREW_PREFIX"/lib/libtesseract*.dylib "$BREW_PREFIX"/lib/libleptonica*.dylib; do
    [[ -f "$lib" ]] || continue
    cp "$lib" "$FRAMEWORKS/"
  done
fi

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP" || true
fi

echo "Bundled Tesseract into $APP (binary: $(basename "$BIN"))"
