#!/usr/bin/env sh
set -eu

usage() {
  printf 'Usage: %s path-to-package.deb\n' "$0"
}

case "$#" in
  1) ;;
  *) usage >&2; exit 2 ;;
esac

DEB=$1
if [ ! -f "$DEB" ]; then
  printf 'Package not found: %s\n' "$DEB" >&2
  exit 1
fi
if ! command -v dpkg-deb >/dev/null 2>&1; then
  printf 'dpkg-deb is required to validate %s\n' "$DEB" >&2
  exit 1
fi

PACKAGE=$(dpkg-deb -f "$DEB" Package)
VERSION=$(dpkg-deb -f "$DEB" Version)
ARCH=$(dpkg-deb -f "$DEB" Architecture)
DEPENDS=$(dpkg-deb -f "$DEB" Depends)
MAINTAINER=$(dpkg-deb -f "$DEB" Maintainer)

[ "$PACKAGE" = bexplorer ] || {
  printf 'Unexpected package name: %s\n' "$PACKAGE" >&2
  exit 1
}
if [ -n "${EXPECTED_VERSION:-}" ] && [ "$VERSION" != "$EXPECTED_VERSION" ]; then
  printf 'Unexpected package version: %s (expected %s)\n' \
    "$VERSION" "$EXPECTED_VERSION" >&2
  exit 1
fi
if [ -n "${EXPECTED_ARCH:-}" ] && [ "$ARCH" != "$EXPECTED_ARCH" ]; then
  printf 'Unexpected package architecture: %s (expected %s)\n' \
    "$ARCH" "$EXPECTED_ARCH" >&2
  exit 1
fi
[ "$MAINTAINER" = "BExplorer Project <noreply@github.com>" ] || {
  printf 'Unexpected package maintainer: %s\n' "$MAINTAINER" >&2
  exit 1
}

field_contains_package() {
  printf '%s\n' "$1" \
    | tr ',|' '\n\n' \
    | sed 's/([^)]*)//g; s/^[[:space:]]*//; s/[[:space:]]*$//' \
    | grep -Fqx "$2"
}

# These commands enhance individual integrations, but BExplorer can start and
# retain its native clipboard/file-management path without them. Keeping them
# out of Depends prevents a disabled Ubuntu component from blocking the whole
# package installation.
for OPTIONAL_PACKAGE in \
  wl-clipboard xclip xsel libfile-mimeinfo-perl \
  kde-cli-tools kio-extras kio-fuse
do
  if field_contains_package "$DEPENDS" "$OPTIONAL_PACKAGE"; then
    printf 'Optional helper must not be in Depends: %s\n' \
      "$OPTIONAL_PACKAGE" >&2
    exit 1
  fi
done

TMPDIR_ROOT=$(mktemp -d)
trap 'rm -rf "$TMPDIR_ROOT"' EXIT HUP INT TERM
dpkg-deb -x "$DEB" "$TMPDIR_ROOT/root"
if [ ! -x "$TMPDIR_ROOT/root/usr/bin/bexplorer" ]; then
  printf 'Package does not contain executable /usr/bin/bexplorer\n' >&2
  exit 1
fi

printf 'Validated %s %s (%s)\n' "$PACKAGE" "$VERSION" "$ARCH"
