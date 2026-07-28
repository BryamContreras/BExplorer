#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
SOURCE_ICON="$ROOT_DIR/assets/icons/appicon.png"
MODE=${1:---write}
MANIFEST_REL="assets/linux/hicolor/SHA256SUMS"
MANIFEST="$ROOT_DIR/$MANIFEST_REL"

case "$MODE" in
  --write)
    ;;
  --check)
    (
      cd "$ROOT_DIR"
      sha256sum -c "$MANIFEST_REL"
    )
    printf 'Validated Linux hicolor icons against %s\n' "$SOURCE_ICON"
    exit 0
    ;;
  *)
    printf 'Usage: %s [--write|--check]\n' "$0" >&2
    exit 2
    ;;
esac

if command -v magick >/dev/null 2>&1; then
  IMAGE_TOOL=magick
elif command -v convert >/dev/null 2>&1; then
  IMAGE_TOOL=convert
else
  printf 'ImageMagick is required to synchronize Linux application icons\n' >&2
  exit 1
fi

for SIZE in 16 24 32 48 64 128 256 512; do
  OUTPUT="$ROOT_DIR/assets/linux/hicolor/${SIZE}x${SIZE}/apps/bexplorer.png"
  mkdir -p "$(dirname "$OUTPUT")"
  "$IMAGE_TOOL" "$SOURCE_ICON" \
    -filter Lanczos \
    -resize "${SIZE}x${SIZE}" \
    -strip \
    "$OUTPUT"
done

MANIFEST_TMP=$(mktemp)
trap 'rm -f "$MANIFEST_TMP"' EXIT HUP INT TERM
(
  cd "$ROOT_DIR"
  sha256sum \
    assets/icons/appicon.png \
    assets/linux/hicolor/16x16/apps/bexplorer.png \
    assets/linux/hicolor/24x24/apps/bexplorer.png \
    assets/linux/hicolor/32x32/apps/bexplorer.png \
    assets/linux/hicolor/48x48/apps/bexplorer.png \
    assets/linux/hicolor/64x64/apps/bexplorer.png \
    assets/linux/hicolor/128x128/apps/bexplorer.png \
    assets/linux/hicolor/256x256/apps/bexplorer.png \
    assets/linux/hicolor/512x512/apps/bexplorer.png
) > "$MANIFEST_TMP"
mv "$MANIFEST_TMP" "$MANIFEST"
trap - EXIT HUP INT TERM

printf 'Synchronized Linux hicolor icons from %s\n' "$SOURCE_ICON"
