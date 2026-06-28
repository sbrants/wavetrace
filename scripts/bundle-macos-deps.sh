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

BREW_PREFIX="$(brew --prefix)"

is_system_lib() {
  case "$1" in
    /usr/lib/* | /System/*)
      return 0
      ;;
  esac
  return 1
}

needs_bundling() {
  case "$1" in
    *homebrew* | *usr/local* | /opt/homebrew/* | /usr/local/* | @rpath/* | @loader_path/*)
      return 0
      ;;
  esac
  return 1
}

resolve_dylib() {
  local ref="$1"
  local base candidate

  if [[ -f "$ref" ]]; then
    echo "$ref"
    return 0
  fi

  base="$(basename "$ref")"
  if [[ -f "$FRAMEWORKS/$base" ]]; then
    echo "$FRAMEWORKS/$base"
    return 0
  fi

  for candidate in \
    "$BREW_PREFIX/lib/$base" \
    "$BREW_PREFIX/opt/libarchive/lib/$base" \
    "$BREW_PREFIX/opt/tesseract/lib/$base" \
    "$BREW_PREFIX/opt/leptonica/lib/$base" \
    "$BREW_PREFIX/opt/libpng/lib/$base" \
    "$BREW_PREFIX/opt/jpeg-turbo/lib/$base" \
    "$BREW_PREFIX/opt/webp/lib/$base" \
    "$BREW_PREFIX/opt/libtiff/lib/$base" \
    "$BREW_PREFIX/opt/zstd/lib/$base" \
    "$BREW_PREFIX/opt/xz/lib/$base" \
    "$BREW_PREFIX/opt/lz4/lib/$base" \
    "$BREW_PREFIX/opt/little-cms2/lib/$base" \
    "$BREW_PREFIX/opt/openjpeg/lib/$base" \
    "$BREW_PREFIX/opt/giflib/lib/$base" \
    "/opt/homebrew/lib/$base" \
    "/usr/local/lib/$base"; do
    if [[ -f "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  shopt -s nullglob
  for candidate in "$BREW_PREFIX"/opt/*/lib/"$base"; do
    if [[ -f "$candidate" ]]; then
      shopt -u nullglob
      echo "$candidate"
      return 0
    fi
  done
  shopt -u nullglob

  return 1
}

copy_lib() {
  local lib="$1"
  local base dest
  [[ -f "$lib" ]] || return 0
  base="$(basename "$lib")"
  dest="$FRAMEWORKS/$base"
  cp -f "$lib" "$dest"
  install_name_tool -id "@loader_path/$base" "$dest" 2>/dev/null || true
}

seen_file() {
  local needle="$1"
  local item
  for item in "${SEEN[@]:-}"; do
    [[ "$item" == "$needle" ]] && return 0
  done
  return 1
}

mark_seen() {
  SEEN+=("$1")
}

bundle_dylibs_recursive() {
  local -a queue=("$BIN")
  local target dep resolved base dest

  SEEN=()
  while [[ ${#queue[@]} -gt 0 ]]; do
    target="${queue[0]}"
    queue=("${queue[@]:1}")
    seen_file "$target" && continue
    mark_seen "$target"

    while IFS= read -r dep; do
      [[ -n "$dep" ]] || continue
      is_system_lib "$dep" && continue
      needs_bundling "$dep" || continue
      resolved="$(resolve_dylib "$dep" || true)"
      [[ -n "$resolved" && -f "$resolved" ]] || continue
      base="$(basename "$resolved")"
      dest="$FRAMEWORKS/$base"
      if [[ ! -f "$dest" ]]; then
        copy_lib "$resolved"
        queue+=("$dest")
      fi
    done < <(otool -L "$target" 2>/dev/null | tail -n +2 | awk '{print $1}')
  done
}

framework_ref_for() {
  local file="$1"
  local base="$2"
  if [[ "$file" == "$FRAMEWORKS"/* ]]; then
    echo "@loader_path/$base"
  else
    echo "@executable_path/../Frameworks/$base"
  fi
}

fix_dylib_paths() {
  local file="$1"
  local dep base new_path
  while IFS= read -r dep; do
    [[ -n "$dep" ]] || continue
    is_system_lib "$dep" && continue
    base="$(basename "$dep")"
    [[ -f "$FRAMEWORKS/$base" ]] || continue
    new_path="$(framework_ref_for "$file" "$base")"
    [[ "$dep" == "$new_path" ]] && continue
    install_name_tool -change "$dep" "$new_path" "$file" 2>/dev/null || true
  done < <(otool -L "$file" 2>/dev/null | tail -n +2 | awk '{print $1}')
}

bundle_dylibs_recursive

fix_dylib_paths "$BIN"
shopt -s nullglob
for lib in "$FRAMEWORKS"/*.dylib; do
  fix_dylib_paths "$lib"
done
shopt -u nullglob

verify_bundle() {
  local file dep bad=0
  shopt -s nullglob
  for file in "$BIN" "$FRAMEWORKS"/*.dylib; do
    while IFS= read -r dep; do
      [[ -n "$dep" ]] || continue
      case "$dep" in
        *homebrew* | *usr/local* | @rpath/*)
          echo "Unresolved dependency in $file: $dep" >&2
          bad=1
          ;;
      esac
    done < <(otool -L "$file" 2>/dev/null | tail -n +2 | awk '{print $1}')
  done
  shopt -u nullglob
  return "$bad"
}

if ! verify_bundle; then
  echo "Dylib bundling verification failed" >&2
  exit 1
fi

# Re-sign inside-out. The `install_name_tool` rewrites above invalidate the
# ad-hoc signature the linker applied to the binary and every bundled dylib. On
# Apple Silicon the kernel SIGKILLs a Mach-O with a stale/invalid signature at
# launch, so signing is mandatory and must fail loudly: a silently broken sign
# here is what shipped non-launching DMGs (issue #4). Sign each dylib first,
# then the bundle (which seals the main executable), then verify the whole app.
if ! command -v codesign >/dev/null 2>&1; then
  echo "codesign not found; cannot produce a launchable macOS bundle" >&2
  exit 1
fi

shopt -s nullglob
for lib in "$FRAMEWORKS"/*.dylib; do
  codesign --force --sign - --timestamp=none "$lib"
done
shopt -u nullglob
codesign --force --sign - --timestamp=none "$BIN"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

dylib_count=0
shopt -s nullglob
for _ in "$FRAMEWORKS"/*.dylib; do
  dylib_count=$((dylib_count + 1))
done
shopt -u nullglob

echo "Bundled Tesseract into $APP (binary: $(basename "$BIN"), dylibs: $dylib_count)"
